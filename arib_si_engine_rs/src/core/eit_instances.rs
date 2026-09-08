use crate::eit::{parse_eit_section_facts, EitEvent};
use crate::sections::{parse_section_header, section_crc_valid_with_header, SectionTracker};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct TableKey(u8, u16, u16, u16, bool);

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Instance {
    tracker: SectionTracker,
    sections: BTreeMap<u8, Vec<EitEvent>>,
    safe: BTreeSet<u8>,
    segment_ends: BTreeMap<u8, u8>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EitInstanceState {
    pub table_id: u8,
    pub original_network_id: u16,
    pub transport_stream_id: u16,
    pub service_id: u16,
    pub version: u8,
    pub current_next_indicator: bool,
    pub last_section_number: u8,
    pub received_sections: Vec<u8>,
    pub missing_sections: Vec<u8>,
    pub safe_sections: Vec<u8>,
    pub complete: bool,
    pub inconsistent: bool,
}

/// 放送表の現版と次版を分けて保持する。公開可否や削除区間は所有しない。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EitInstances {
    instances: BTreeMap<TableKey, Instance>,
}

impl EitInstances {
    pub fn ingest(&mut self, section: &[u8]) {
        let Some(header) = parse_section_header(section) else {
            return;
        };
        if !(0x4e..=0x6f).contains(&header.table_id)
            || section.len() < 18
            || header.total_length != section.len()
            || !section_crc_valid_with_header(section, &header)
        {
            return;
        }
        let (Some(version), Some(number), Some(last), Some(current)) = (
            header.version,
            header.section_number,
            header.last_section_number,
            header.current_next_indicator,
        ) else {
            return;
        };
        let key = TableKey(
            header.table_id,
            u16::from_be_bytes([section[10], section[11]]),
            u16::from_be_bytes([section[8], section[9]]),
            u16::from_be_bytes([section[3], section[4]]),
            current,
        );
        let instance = self.instances.entry(key).or_default();
        if !instance.tracker.accepts_version(version) {
            return;
        }
        if instance.tracker.version != Some(version) {
            instance.sections.clear();
            instance.safe.clear();
            instance.segment_ends.clear();
        }
        if !instance.tracker.observe(version, number, last, section) {
            return;
        }
        if header.table_id >= 0x50 {
            let start = number & 0xf8;
            let end = section[12];
            if end < number
                || end > last
                || end & 0xf8 != start
                || instance
                    .segment_ends
                    .get(&start)
                    .is_some_and(|old| *old != end)
            {
                instance.tracker.inconsistent = true;
            }
            instance.segment_ends.insert(start, end);
        }
        let facts = parse_eit_section_facts(section);
        let events = facts.events;
        instance.safe.remove(&number);
        if facts.event_loop_complete
            && events.iter().all(|event| {
                event
                    .diagnostics
                    .iter()
                    .all(|diagnostic| !diagnostic.parse_status.is_structural_error())
            })
        {
            instance.safe.insert(number);
        }
        instance.sections.insert(number, events);
    }

    pub fn states(&self) -> Vec<EitInstanceState> {
        self.instances
            .iter()
            .map(|(key, value)| {
                let required: BTreeSet<u8> = if key.0 < 0x50 {
                    (0..=value.tracker.last_section_number.unwrap_or(0)).collect()
                } else {
                    (0..=value.tracker.last_section_number.unwrap_or(0))
                        .step_by(8)
                        .flat_map(|start| {
                            start..=value.segment_ends.get(&start).copied().unwrap_or(start)
                        })
                        .collect()
                };
                let received: BTreeSet<u8> = value.sections.keys().copied().collect();
                let missing: Vec<u8> = required.difference(&received).copied().collect();
                EitInstanceState {
                    table_id: key.0,
                    original_network_id: key.1,
                    transport_stream_id: key.2,
                    service_id: key.3,
                    version: value.tracker.version.unwrap_or(0),
                    current_next_indicator: key.4,
                    last_section_number: value.tracker.last_section_number.unwrap_or(0),
                    complete: !value.tracker.inconsistent && missing.is_empty(),
                    inconsistent: value.tracker.inconsistent,
                    received_sections: received.into_iter().collect(),
                    missing_sections: missing,
                    safe_sections: value.safe.iter().copied().collect(),
                }
            })
            .collect()
    }

    pub fn events(&self) -> Vec<EitEvent> {
        self.instances
            .iter()
            .filter(|(key, _)| key.4)
            .flat_map(|(_, value)| value.sections.values().flatten().cloned())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sections::crc32_mpeg;
    fn section(version: u8, current: bool, number: u8, last: u8) -> Vec<u8> {
        let mut bytes = vec![
            0x4e,
            0xf0,
            15,
            0,
            1,
            0xc0 | (version << 1) | u8::from(current),
            number,
            last,
            0,
            17,
            0,
            34,
            last,
            0x4e,
        ];
        bytes.extend_from_slice(&crc32_mpeg(&bytes).to_be_bytes());
        bytes
    }
    #[test]
    fn repeated_conflicting_payload_and_version_rollover_are_not_mixed() {
        let mut store = EitInstances::default();
        store.ingest(&section(31, true, 0, 0));
        store.ingest(&section(0, true, 0, 1));
        store.ingest(&section(31, true, 0, 0));
        store.ingest(&section(16, true, 0, 0));
        assert_eq!(store.states()[0].version, 0);
        assert_eq!(store.states()[0].missing_sections, vec![1]);
        let mut conflict = section(0, true, 0, 1);
        conflict.truncate(conflict.len() - 4);
        conflict[12] = 0;
        conflict.extend_from_slice(&crc32_mpeg(&conflict).to_be_bytes());
        store.ingest(&conflict);
        assert!(store.states()[0].inconsistent);
    }

    #[test]
    fn unknown_descriptor_is_safe_but_truncated_loop_is_not() {
        let mut bytes = section(1, true, 0, 0);
        bytes.truncate(14);
        bytes.extend_from_slice(&[0, 1, 0xee, 0, 0x12, 0, 0, 0x01, 0, 0, 0xf0, 3, 0x90, 1, 0]);
        bytes[2] = (bytes.len() + 1) as u8;
        bytes.extend_from_slice(&crc32_mpeg(&bytes).to_be_bytes());
        let mut store = EitInstances::default();
        store.ingest(&bytes);
        assert_eq!(store.states()[0].safe_sections, vec![0]);
        assert_eq!(store.events()[0].diagnostics[0].parse_status, crate::descriptors::DescriptorParseStatus::UnsupportedValue);
        bytes.truncate(bytes.len() - 4);
        bytes[5] = 0xc5;
        bytes[25] = 4;
        bytes.extend_from_slice(&crc32_mpeg(&bytes).to_be_bytes());
        store.ingest(&bytes);
        assert!(store.states()[0].safe_sections.is_empty());
        assert!(store.states()[0].complete);
    }

    #[test]
    fn schedule_segment_gap_and_shrinking_version_follow_wire_facts() {
        let mut store = EitInstances::default();
        for number in [0, 1, 8, 9] {
            let mut bytes = section(1, true, number, 9);
            bytes.truncate(14);
            bytes[0] = 0x50;
            bytes[12] = if number < 8 { 1 } else { 9 };
            bytes.extend_from_slice(&crc32_mpeg(&bytes).to_be_bytes());
            store.ingest(&bytes);
        }
        assert!(store.states()[0].complete);
        assert!(store.states()[0].missing_sections.is_empty());
        let mut bytes = section(2, true, 0, 0);
        bytes.truncate(14);
        bytes[0] = 0x50;
        bytes.extend_from_slice(&crc32_mpeg(&bytes).to_be_bytes());
        store.ingest(&bytes);
        assert_eq!(store.states()[0].received_sections, vec![0]);
    }

    #[test]
    fn versions_next_table_and_inconsistency_remain_distinct() {
        let mut store = EitInstances::default();
        store.ingest(&section(1, true, 0, 1));
        assert_eq!(store.states()[0].missing_sections, vec![1]);
        store.ingest(&section(1, true, 1, 1));
        assert!(store.states()[0].complete);
        store.ingest(&section(2, false, 0, 0));
        assert_eq!(store.states().len(), 2);
        store.ingest(&section(2, true, 0, 1));
        let current = store
            .states()
            .into_iter()
            .find(|state| state.current_next_indicator)
            .unwrap();
        assert_eq!(current.version, 2);
        assert!(!current.complete);
        store.ingest(&section(2, true, 1, 2));
        let current = store
            .states()
            .into_iter()
            .find(|state| state.current_next_indicator)
            .unwrap();
        assert!(current.inconsistent);
        assert!(!current.complete);
    }
}

//! TIS専用のEPG保存policy。放送由来factのpure parserとは別の責務を持つ。
use crate::discovery_requirements::DiscoveryProfile;
use crate::eit::{
    parse_eit_section_facts, u16_at, EitEvent, EitStableEventIdentity, EitTimingState,
};
use crate::sections::{parse_section_header, section_crc_valid_with_header, SectionTracker};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// Program publishへ渡してよいEIT sectionかを媒体profile込みで判定する。
/// BS/110CSのEIT[p/f] actual (table_id 0x4e) はsection 0/1だけが現在/次番組の
/// publish事実であり、section 2以降をProgram行へ投影しない。
pub fn is_program_publish_eit_section(profile: DiscoveryProfile, pid: u16, section: &[u8]) -> bool {
    if pid != 0x0012 {
        return true;
    }
    let Some(header) = parse_section_header(section) else {
        return true;
    };
    if header.table_id != 0x4e {
        return true;
    }
    match profile {
        DiscoveryProfile::Bs | DiscoveryProfile::Cs110 => {
            matches!(header.section_number, Some(0 | 1))
        }
        DiscoveryProfile::IsdbT => true,
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EitUpdateWindow {
    pub original_network_id: u16,
    pub transport_stream_id: u16,
    pub service_id: u16,
    pub window_start_millis: i64,
    pub window_end_millis: i64,
    pub valid_event_identities: Vec<EitStableEventIdentity>,
    pub deletion_authoritative: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct EitEventKey {
    table_id: u8,
    original_network_id: u16,
    transport_stream_id: u16,
    service_id: u16,
    event_id: u16,
}

pub fn program_identity(event: &EitEvent) -> Option<EitStableEventIdentity> {
    has_stable_identity(event.timing_state).then_some(EitStableEventIdentity {
        original_network_id: event.original_network_id,
        transport_stream_id: event.transport_stream_id,
        service_id: event.service_id,
        event_id: event.event_id,
    })
}

fn has_stable_identity(state: EitTimingState) -> bool {
    matches!(
        state,
        EitTimingState::Defined | EitTimingState::UndefinedTime
    )
}

impl From<&EitEvent> for EitEventKey {
    fn from(event: &EitEvent) -> Self {
        Self {
            table_id: event.table_id,
            original_network_id: event.original_network_id,
            transport_stream_id: event.transport_stream_id,
            service_id: event.service_id,
            event_id: event.event_id,
        }
    }
}

fn stable_event_key(event: &EitEvent) -> Option<EitEventKey> {
    has_stable_identity(event.timing_state).then(|| event.into())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct EitTableKey {
    table_id: u8,
    original_network_id: u16,
    transport_stream_id: u16,
    service_id: u16,
}

impl EitTableKey {
    fn owns(&self, event: &EitEvent) -> bool {
        self.table_id == event.table_id
            && self.original_network_id == event.original_network_id
            && self.transport_stream_id == event.transport_stream_id
            && self.service_id == event.service_id
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct EitInstance {
    tracker: SectionTracker,
    sections: BTreeMap<u8, Vec<EitEvent>>,
    safe_sections: BTreeSet<u8>,
    segment_ends: BTreeMap<u8, u8>,
    required_last_section_number: Option<u8>,
}

impl EitInstance {
    fn required_sections(&self, table_id: u8) -> BTreeSet<u8> {
        let Some(last) = self.required_last_section_number else {
            return BTreeSet::new();
        };
        if table_id < 0x50 {
            return (0..=last).collect();
        }
        (0..=last)
            .step_by(8)
            .flat_map(|start| {
                let end = self.segment_ends.get(&start).copied().unwrap_or(start);
                start..=end
            })
            .collect()
    }

    fn is_complete(&self, table_id: u8) -> bool {
        self.tracker.last_section_number.is_some()
            && !self.tracker.inconsistent
            && self
                .required_sections(table_id)
                .iter()
                .all(|number| self.tracker.seen_sections.contains(number))
    }
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
    pub last_section_number: Option<u8>,
    pub required_last_section_number: Option<u8>,
    pub received_sections: Vec<u8>,
    pub missing_sections: Vec<u8>,
    pub complete: bool,
    pub inconsistent: bool,
    pub deletion_authoritative: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EitStore {
    discovery_profile: DiscoveryProfile,
    // 前回完成した公開用事実。新版の未完成sectionと混ぜず、削除・移動区間の旧側だけに使う。
    events: BTreeMap<EitEventKey, EitEvent>,
    instances: BTreeMap<EitTableKey, EitInstance>,
    last_update_windows: Vec<EitUpdateWindow>,
}

impl EitStore {
    pub fn reset_collection(&mut self) {
        *self = Self {
            discovery_profile: self.discovery_profile,
            ..Self::default()
        };
    }

    pub fn set_discovery_profile(&mut self, profile: DiscoveryProfile) {
        if self.discovery_profile != profile {
            *self = Self {
                discovery_profile: profile,
                ..Self::default()
            };
        }
    }

    pub fn upsert_section(&mut self, section: &[u8]) {
        let Some(header) = parse_section_header(section) else {
            return;
        };
        if !(0x4e..=0x6f).contains(&header.table_id)
            || header.total_length != section.len()
            || section.len() < 18
            || header.current_next_indicator != Some(true)
            || !section_crc_valid_with_header(section, &header)
        {
            return;
        }
        let (Some(version), Some(number), Some(last)) = (
            header.version,
            header.section_number,
            header.last_section_number,
        ) else {
            return;
        };
        let key = EitTableKey {
            table_id: header.table_id,
            service_id: u16_at(section, 3),
            transport_stream_id: u16_at(section, 8),
            original_network_id: u16_at(section, 10),
        };
        if !is_program_publish_eit_section(self.discovery_profile, 0x0012, section) {
            return;
        }
        let required_last = if key.table_id == 0x4e
            && matches!(
                self.discovery_profile,
                DiscoveryProfile::Bs | DiscoveryProfile::Cs110
            ) {
            last.min(1)
        } else {
            last
        };
        let instance = self.instances.entry(key).or_default();
        if !instance.tracker.accepts_version(version) {
            return;
        }
        if instance.tracker.version != Some(version) {
            instance.sections.clear();
            instance.safe_sections.clear();
            instance.segment_ends.clear();
        }
        if !instance.tracker.observe(version, number, last, section) {
            if instance.tracker.inconsistent && key.table_id == 0x4e {
                for window in self
                    .last_update_windows
                    .iter_mut()
                    .filter(|window| same_service(window, key))
                {
                    window.deletion_authoritative = false;
                }
            }
            return;
        }
        if key.table_id >= 0x50 {
            let segment_start = number & 0xf8;
            let segment_end = section[12];
            if segment_end < number
                || segment_end > last
                || (segment_end & 0xf8) != segment_start
                || instance
                    .segment_ends
                    .get(&segment_start)
                    .is_some_and(|previous| *previous != segment_end)
            {
                instance.tracker.inconsistent = true;
                return;
            }
            instance.segment_ends.insert(segment_start, segment_end);
        }
        instance.required_last_section_number = Some(required_last);
        let facts = parse_eit_section_facts(section);
        if facts.event_loop_complete
            && facts.events.iter().all(|event| {
                has_stable_identity(event.timing_state) && event.diagnostics.is_empty()
            })
        {
            instance.safe_sections.insert(number);
        }
        instance.sections.insert(number, facts.events);
        if !instance.is_complete(key.table_id) {
            // 未完成新版が来た時点で、未排出の旧版の削除権限も無効にする。
            if key.table_id == 0x4e {
                for window in self
                    .last_update_windows
                    .iter_mut()
                    .filter(|window| same_service(window, key))
                {
                    window.deletion_authoritative = false;
                }
            }
            return;
        }
        let parsed: Vec<_> = instance.sections.values().flatten().cloned().collect();
        let new_keys: BTreeSet<_> = parsed.iter().filter_map(stable_event_key).collect();
        let deletion_authoritative =
            key.table_id == 0x4e && instance.safe_sections.len() == instance.sections.len();
        if key.table_id != 0x4e {
            return;
        }
        let old_events: Vec<_> = self
            .events
            .values()
            .filter(|event| key.owns(event))
            .cloned()
            .collect();
        let window_events: Vec<_> = old_events
            .iter()
            .chain(&parsed)
            .filter(|event| event.timing_state == EitTimingState::Defined)
            .cloned()
            .collect();
        if let Some(mut window) = build_update_window(
            key.original_network_id,
            key.transport_stream_id,
            key.service_id,
            &window_events,
            &parsed,
            deletion_authoritative,
        ) {
            // 排出間に複数の完成版を受けてもServiceごとに一区間だけを保持する。
            for previous in self
                .last_update_windows
                .iter()
                .filter(|old| same_service(old, key))
            {
                window.window_start_millis =
                    window.window_start_millis.min(previous.window_start_millis);
                window.window_end_millis = window.window_end_millis.max(previous.window_end_millis);
            }
            self.last_update_windows
                .retain(|old| !same_service(old, key));
            self.last_update_windows.push(window);
        }
        if deletion_authoritative {
            self.events
                .retain(|event_key, event| !key.owns(event) || new_keys.contains(event_key));
        }
        for event in parsed {
            if let Some(event_key) = stable_event_key(&event) {
                // 時刻未定で以前の有効区間を失わず、現在の事実はsectionsから公開する。
                if event.timing_state == EitTimingState::Defined
                    || !self.events.contains_key(&event_key)
                {
                    self.events.insert(event_key, event);
                }
            }
        }
    }

    pub fn instance_states(&self) -> Vec<EitInstanceState> {
        self.instances
            .iter()
            .map(|(key, instance)| {
                let tracker = &instance.tracker;
                EitInstanceState {
                    table_id: key.table_id,
                    original_network_id: key.original_network_id,
                    transport_stream_id: key.transport_stream_id,
                    service_id: key.service_id,
                    version: tracker.version.unwrap_or(0),
                    current_next_indicator: true,
                    last_section_number: tracker.last_section_number,
                    required_last_section_number: instance.required_last_section_number,
                    received_sections: tracker.seen_sections.iter().copied().collect(),
                    missing_sections: instance
                        .required_sections(key.table_id)
                        .difference(&tracker.seen_sections)
                        .copied()
                        .collect(),
                    complete: instance.is_complete(key.table_id),
                    inconsistent: tracker.inconsistent,
                    deletion_authoritative: key.table_id == 0x4e
                        && instance.is_complete(key.table_id)
                        && instance.safe_sections.len() == instance.sections.len(),
                }
            })
            .collect()
    }

    pub fn take_present_following_actual_update_windows(&mut self) -> Vec<EitUpdateWindow> {
        let (mut out, pending): (Vec<_>, Vec<_>) = std::mem::take(&mut self.last_update_windows)
            .into_iter()
            .partition(|window| {
                self.instances.iter().any(|(key, instance)| {
                    key.table_id == 0x4e
                        && same_service(window, *key)
                        && instance.is_complete(key.table_id)
                })
            });
        self.last_update_windows = pending;
        out.sort_by_key(|w| (w.original_network_id, w.transport_stream_id, w.service_id));
        out
    }

    pub fn snapshot_present_following_actual(&self) -> Vec<EitEvent> {
        let mut out: Vec<_> = self
            .events
            .values()
            .filter(|event| event.table_id == 0x4e && event.timing_state == EitTimingState::Defined)
            .cloned()
            .collect();
        out.sort_by_key(|e| {
            (
                e.original_network_id,
                e.transport_stream_id,
                e.service_id,
                e.start_time_millis,
                e.event_id,
            )
        });
        out
    }

    pub fn snapshot_all_for_diagnostic(&self) -> Vec<EitEvent> {
        self.instances
            .values()
            .filter(|instance| !instance.tracker.inconsistent)
            .flat_map(|instance| instance.sections.values().flatten().cloned())
            .collect()
    }

    #[cfg(test)]
    pub fn section_count_for_diagnostic(&self) -> usize {
        self.instances
            .values()
            .map(|instance| instance.sections.len())
            .sum()
    }
}

fn same_service(window: &EitUpdateWindow, key: EitTableKey) -> bool {
    window.original_network_id == key.original_network_id
        && window.transport_stream_id == key.transport_stream_id
        && window.service_id == key.service_id
}

fn build_update_window(
    onid: u16,
    tsid: u16,
    sid: u16,
    window_events: &[EitEvent],
    current_events: &[EitEvent],
    deletion_authoritative: bool,
) -> Option<EitUpdateWindow> {
    if window_events.is_empty() {
        return None;
    }
    let start = window_events
        .iter()
        .map(|event| event.start_time_millis)
        .min()?;
    let end = window_events
        .iter()
        .filter_map(|event| event.start_time_millis.checked_add(event.duration_millis))
        .max()?;
    if end <= start {
        return None;
    }
    let mut valid_event_identities: Vec<_> =
        current_events.iter().filter_map(program_identity).collect();
    valid_event_identities.sort_by_key(|identity| {
        (
            identity.original_network_id,
            identity.transport_stream_id,
            identity.service_id,
            identity.event_id,
        )
    });
    valid_event_identities.dedup();
    Some(EitUpdateWindow {
        original_network_id: onid,
        transport_stream_id: tsid,
        service_id: sid,
        window_start_millis: start,
        window_end_millis: end,
        valid_event_identities,
        deletion_authoritative,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::descriptors::{DescriptorParseStatus, EventDescriptors};
    use crate::eit::{parse_eit_section, EitScope};
    use crate::sections::crc32_mpeg;

    fn section_with_crc(mut body: Vec<u8>) -> Vec<u8> {
        let crc = crc32_mpeg(&body);
        body.extend_from_slice(&crc.to_be_bytes());
        body
    }

    fn eit_body(version: u8, events: &[(u16, [u8; 5])]) -> Vec<u8> {
        eit_body_with_table_id(0x4e, version, events)
    }

    fn eit_body_with_table_id(table_id: u8, version: u8, events: &[(u16, [u8; 5])]) -> Vec<u8> {
        let mut body = vec![
            table_id,
            0xf0,
            0x00,
            0x00,
            0x01,
            0xc1 | ((version & 0x1f) << 1),
            0x00,
            0x00,
            0x00,
            0x11,
            0x00,
            0x22,
            0x00,
            0x00,
        ];
        for (event_id, start) in events {
            body.extend_from_slice(&event_id.to_be_bytes());
            body.extend_from_slice(start);
            body.extend_from_slice(&[0x00, 0x30, 0x00, 0xf0, 0x00]);
        }
        let section_length = body.len() - 3 + 4;
        body[1] = 0xf0 | (((section_length >> 8) & 0x0f) as u8);
        body[2] = (section_length & 0xff) as u8;
        body
    }

    #[test]
    fn conflicting_same_version_keeps_the_previous_events_and_revokes_deletion() {
        let mut store = EitStore::default();
        let start1 = [0xee, 0x00, 0x12, 0x00, 0x00];
        let start2 = [0xee, 0x01, 0x13, 0x00, 0x00];
        store.upsert_section(&section_with_crc(eit_body(1, &[(1, start1), (2, start2)])));
        assert_eq!(store.snapshot_present_following_actual().len(), 2);
        store.upsert_section(&section_with_crc(eit_body(1, &[(1, start1)])));
        let events = store.snapshot_present_following_actual();
        assert_eq!(events.len(), 2);
        assert!(store.instance_states()[0].inconsistent);
        assert!(!store.instance_states()[0].complete);
        assert!(store
            .take_present_following_actual_update_windows()
            .is_empty());
    }

    #[test]
    fn version_update_removes_events_absent_from_new_section() {
        let mut store = EitStore::default();
        let start1 = [0xee, 0x00, 0x12, 0x00, 0x00];
        let start2 = [0xee, 0x01, 0x13, 0x00, 0x00];
        store.upsert_section(&section_with_crc(eit_body(1, &[(1, start1), (2, start2)])));
        assert_eq!(store.snapshot_present_following_actual().len(), 2);
        store.upsert_section(&section_with_crc(eit_body(2, &[(2, start2)])));
        let events = store.snapshot_present_following_actual();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_id, 2);
    }

    #[test]
    fn incomplete_new_version_keeps_old_baseline_without_mixing_publication_facts() {
        let mut store = EitStore::default();
        let start1 = [0xee, 0x00, 0x12, 0x00, 0x00];
        let start2 = [0xee, 0x01, 0x13, 0x00, 0x00];
        let mut section0 = eit_body(1, &[(1, start1)]);
        section0[6] = 0;
        section0[7] = 1;
        let mut section1 = eit_body(1, &[(2, start2)]);
        section1[6] = 1;
        section1[7] = 1;
        store.upsert_section(&section_with_crc(section0));
        store.upsert_section(&section_with_crc(section1));
        assert_eq!(store.snapshot_present_following_actual().len(), 2);

        let mut new_section0 = eit_body(2, &[(1, start1)]);
        new_section0[6] = 0;
        new_section0[7] = 1;
        store.upsert_section(&section_with_crc(new_section0));

        assert!(store
            .snapshot_present_following_actual()
            .iter()
            .all(|event| event.version == 1));
        assert!(store
            .snapshot_all_for_diagnostic()
            .iter()
            .all(|event| event.version == 2));
        let state = &store.instance_states()[0];
        assert_eq!(state.received_sections, vec![0]);
        assert_eq!(state.missing_sections, vec![1]);
        assert!(!state.complete);
        assert!(store
            .take_present_following_actual_update_windows()
            .is_empty());
        let mut new_section1 = eit_body(2, &[(2, start2)]);
        new_section1[6] = 1;
        new_section1[7] = 1;
        store.upsert_section(&section_with_crc(new_section1));
        assert!(store
            .snapshot_present_following_actual()
            .iter()
            .all(|event| event.version == 2));
        assert!(store.instance_states()[0].complete);
        let windows = store.take_present_following_actual_update_windows();
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].valid_event_identities.len(), 2);
    }

    #[test]
    fn version_update_shrinking_last_section_number_reclaims_obsolete_sections() {
        let mut store = EitStore::default();
        let start0 = [0xee, 0x00, 0x12, 0x00, 0x00];
        let start1 = [0xee, 0x01, 0x13, 0x00, 0x00];
        let start2 = [0xee, 0x02, 0x14, 0x00, 0x00];
        for (section_number, event_id, start) in [
            (0_u8, 1_u16, start0),
            (1_u8, 2_u16, start1),
            (2_u8, 3_u16, start2),
        ] {
            let mut section = eit_body(1, &[(event_id, start)]);
            section[6] = section_number;
            section[7] = 2;
            store.upsert_section(&section_with_crc(section));
        }
        assert_eq!(store.section_count_for_diagnostic(), 3);
        let mut new_section0 = eit_body(2, &[(1, start0)]);
        new_section0[6] = 0;
        new_section0[7] = 1;
        store.upsert_section(&section_with_crc(new_section0));
        assert_eq!(store.section_count_for_diagnostic(), 1);
        assert_eq!(store.snapshot_present_following_actual().len(), 3);
        assert!(store
            .take_present_following_actual_update_windows()
            .is_empty());
        let mut new_section1 = eit_body(2, &[(2, start1)]);
        new_section1[6] = 1;
        new_section1[7] = 1;
        store.upsert_section(&section_with_crc(new_section1));
        let events = store.snapshot_present_following_actual();
        assert_eq!(events.len(), 2);
        assert!(events.iter().all(|event| event.version == 2));
        assert!(!events.iter().any(|event| event.event_id == 3));
    }

    #[test]
    fn satellite_completion_does_not_wait_for_ignored_pf_sections() {
        for profile in [DiscoveryProfile::Bs, DiscoveryProfile::Cs110] {
            let mut store = EitStore::default();
            store.set_discovery_profile(profile);
            for number in [0, 1, 2] {
                let mut body = eit_body(1, &[]);
                body[6] = number;
                body[7] = 7;
                store.upsert_section(&section_with_crc(body));
            }
            let states = store.instance_states();
            assert_eq!(states[0].last_section_number, Some(7));
            assert_eq!(states[0].required_last_section_number, Some(1));
            assert_eq!(states[0].received_sections, vec![0, 1]);
            assert!(states[0].complete);
        }
    }

    #[test]
    fn schedule_segment_gaps_are_not_missing_sections() {
        let mut store = EitStore::default();
        for (number, end) in [(0, 1), (1, 1), (8, 8)] {
            let mut body = eit_body_with_table_id(0x50, 1, &[]);
            body[6] = number;
            body[7] = 8;
            body[12] = end;
            store.upsert_section(&section_with_crc(body));
        }
        let states = store.instance_states();
        assert_eq!(states[0].received_sections, vec![0, 1, 8]);
        assert!(states[0].missing_sections.is_empty());
        assert!(states[0].complete);
        assert!(!states[0].deletion_authoritative);
    }

    #[test]
    fn version_rollover_rejects_older_and_ambiguous_versions_until_reset() {
        let mut store = EitStore::default();
        let start = [0xee, 0x00, 0x12, 0x00, 0x00];
        for version in [31, 0, 31, 16] {
            store.upsert_section(&section_with_crc(eit_body(version, &[(1, start)])));
        }
        assert_eq!(store.instance_states()[0].version, 0);
        assert_eq!(store.snapshot_all_for_diagnostic()[0].version, 0);
        assert_eq!(
            store.take_present_following_actual_update_windows().len(),
            1
        );
    }

    #[test]
    fn next_table_and_truncated_event_tail_never_authorize_deletion() {
        let mut store = EitStore::default();
        let mut next = eit_body(1, &[]);
        next[5] &= !1;
        store.upsert_section(&section_with_crc(next));
        assert!(store.instance_states().is_empty());
        let mut truncated = eit_body(1, &[]);
        truncated.push(0x12);
        truncated[2] += 1;
        store.upsert_section(&section_with_crc(truncated));
        let states = store.instance_states();
        assert!(states[0].complete);
        assert!(!states[0].deletion_authoritative);
        assert!(store
            .take_present_following_actual_update_windows()
            .is_empty());
    }

    #[test]
    fn authoritative_valid_update_window_marks_obsolete_delete_allowed() {
        let mut store = EitStore::default();
        let start1 = [0xee, 0x00, 0x12, 0x00, 0x00];
        let start2 = [0xee, 0x01, 0x13, 0x00, 0x00];
        store.upsert_section(&section_with_crc(eit_body(1, &[(1, start1), (2, start2)])));
        let _ = store.take_present_following_actual_update_windows();

        store.upsert_section(&section_with_crc(eit_body(2, &[(2, start2)])));
        let windows = store.take_present_following_actual_update_windows();
        assert!(
            windows.iter().any(|w| w.deletion_authoritative),
            "{:?}",
            windows
        );
        assert!(
            windows
                .iter()
                .any(|w| w.valid_event_identities.iter().any(|id| id.event_id == 2)),
            "{:?}",
            windows
        );
    }

    #[test]
    fn undefined_time_identity_protects_existing_program_in_authoritative_window() {
        let defined_start = [0xee, 0x00, 0x12, 0x00, 0x00];
        let undefined_duration_start = [0xee, 0x01, 0x13, 0x00, 0x00];
        let mut body = eit_body(1, &[(1, defined_start), (2, undefined_duration_start)]);
        // EIT header(14) + 先頭event(12) + event_id(2) + start_time(5)。
        body[33..36].copy_from_slice(&[0xff, 0xff, 0xff]);

        let mut store = EitStore::default();
        store.upsert_section(&section_with_crc(body));

        assert_eq!(store.snapshot_present_following_actual().len(), 1);
        let windows = store.take_present_following_actual_update_windows();
        assert_eq!(windows.len(), 1);
        assert!(windows[0].deletion_authoritative);
        assert_eq!(
            windows[0]
                .valid_event_identities
                .iter()
                .map(|identity| identity.event_id)
                .collect::<Vec<_>>(),
            vec![1, 2],
        );
    }

    #[test]
    fn schedule_other_is_not_r51_snapshot_or_update_window() {
        let mut store = EitStore::default();
        let start = [0xee, 0x00, 0x12, 0x00, 0x00];
        store.upsert_section(&section_with_crc(eit_body_with_table_id(
            0x60,
            1,
            &[(1, start)],
        )));
        assert!(store.snapshot_present_following_actual().is_empty());
        assert!(store
            .take_present_following_actual_update_windows()
            .is_empty());
        assert_eq!(
            store.snapshot_all_for_diagnostic().len(),
            1,
            "診断用snapshotには保持してよい"
        );
    }

    #[test]
    fn moved_event_window_includes_the_previous_and_new_intervals() {
        let mut store = EitStore::default();
        let earlier = [0xee, 0x00, 0x12, 0x00, 0x00];
        let later = [0xee, 0x01, 0x13, 0x00, 0x00];
        store.upsert_section(&section_with_crc(eit_body(1, &[(1, earlier)])));
        let old = store.snapshot_present_following_actual().remove(0);
        store.take_present_following_actual_update_windows();
        store.upsert_section(&section_with_crc(eit_body(2, &[(1, later)])));
        let new = store.snapshot_present_following_actual().remove(0);
        let windows = store.take_present_following_actual_update_windows();
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].window_start_millis, old.start_time_millis);
        assert_eq!(
            windows[0].window_end_millis,
            new.start_time_millis + new.duration_millis
        );
        assert_eq!(windows[0].valid_event_identities.len(), 1);
    }

    #[test]
    fn start_time_change_updates_existing_stable_event_identity() {
        let mut store = EitStore::default();
        let start1 = [0xee, 0x00, 0x12, 0x00, 0x00];
        let start2 = [0xee, 0x02, 0x14, 0x00, 0x00];
        store.upsert_section(&section_with_crc(eit_body(1, &[(3, start1)])));
        store.upsert_section(&section_with_crc(eit_body(2, &[(3, start2)])));
        let events = store.snapshot_present_following_actual();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_id, 3);
    }

    #[test]
    fn stable_identity_is_independent_from_start_time_for_tvprovider_keying() {
        let event = EitEvent {
            diagnostics: Vec::new(),
            table_id: 0x4e,
            version: 0,
            section_number: 0,
            last_section_number: 0,
            scope: EitScope::PresentFollowingActual,
            service_id: 1,
            transport_stream_id: 0x11,
            original_network_id: 0x22,
            event_id: 3,
            timing_state: EitTimingState::Defined,
            raw_start_time: [0; 5],
            raw_duration: [0; 3],
            start_time_millis: 12345,
            duration_millis: 60000,
            free_ca_mode: false,
            descriptors: EventDescriptors::default(),
        };
        assert_eq!(
            program_identity(&event),
            Some(EitStableEventIdentity {
                original_network_id: 0x22,
                transport_stream_id: 0x11,
                service_id: 1,
                event_id: 3,
            })
        );
    }

    #[test]
    fn diagnostic_section_count_tracks_distinct_sections() {
        let mut store = EitStore::default();
        let start1 = [0xee, 0x00, 0x12, 0x00, 0x00];
        store.upsert_section(&section_with_crc(eit_body(1, &[(1, start1)])));
        assert_eq!(store.section_count_for_diagnostic(), 1);
    }

    #[test]
    fn invalid_bcd_start_time_is_rejected() {
        let mut store = EitStore::default();
        let invalid = [0xee, 0x00, 0x7a, 0x00, 0x00];
        store.upsert_section(&section_with_crc(eit_body(1, &[(1, invalid)])));
        assert!(store.snapshot_present_following_actual().is_empty());
    }

    #[test]
    fn invalid_duration_bcd_is_rejected() {
        let mut body = eit_body(1, &[(1, [0xee, 0x00, 0x12, 0x00, 0x00])]);
        // duration は 14 バイトの EIT header、event_id 2 バイト、start_time 5 バイトの後に始まる。
        body[21] = 0x00;
        body[22] = 0x7a;
        body[23] = 0x00;
        let mut store = EitStore::default();
        store.upsert_section(&section_with_crc(body));
        assert!(store.snapshot_present_following_actual().is_empty());
    }

    #[test]
    fn invalid_hour_minute_second_ranges_are_rejected() {
        let mut store = EitStore::default();
        store.upsert_section(&section_with_crc(eit_body(
            1,
            &[(1, [0xee, 0x00, 0x24, 0x00, 0x00])],
        )));
        store.upsert_section(&section_with_crc(eit_body(
            1,
            &[(2, [0xee, 0x00, 0x12, 0x60, 0x00])],
        )));
        store.upsert_section(&section_with_crc(eit_body(
            1,
            &[(3, [0xee, 0x00, 0x12, 0x00, 0x60])],
        )));
        assert!(store.snapshot_present_following_actual().is_empty());
    }

    #[test]
    fn undefined_mjd_is_rejected() {
        let mut store = EitStore::default();
        store.upsert_section(&section_with_crc(eit_body(
            1,
            &[(1, [0xff, 0xff, 0x12, 0x00, 0x00])],
        )));
        assert!(store.snapshot_present_following_actual().is_empty());
    }

    #[test]
    fn descriptor_loop_overflow_is_kept_as_event_diagnostic() {
        let mut body = eit_body(1, &[(1, [0xee, 0x00, 0x12, 0x00, 0x00])]);
        body[24] = 0xf0;
        body[25] = 0x05;
        let mut store = EitStore::default();
        store.upsert_section(&section_with_crc(body));
        let events = store.snapshot_present_following_actual();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].diagnostics.len(), 1);
        assert_eq!(
            events[0].diagnostics[0].parse_status,
            DescriptorParseStatus::TruncatedDescriptor
        );
        assert_eq!(events[0].diagnostics[0].malformed_descriptor_count, 1);
    }

    #[test]
    fn malformed_only_section_does_not_delete_previous_valid_event() {
        let mut store = EitStore::default();
        let valid = [0xee, 0x00, 0x12, 0x00, 0x00];
        store.upsert_section(&section_with_crc(eit_body(1, &[(1, valid)])));
        assert_eq!(store.snapshot_present_following_actual().len(), 1);
        let invalid = [0xee, 0x00, 0x7a, 0x00, 0x00];
        store.upsert_section(&section_with_crc(eit_body(2, &[(1, invalid)])));
        let events = store.snapshot_present_following_actual();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_id, 1);
        assert_eq!(
            events[0].start_time_millis,
            parse_eit_section(&section_with_crc(eit_body(1, &[(1, valid)])))[0].start_time_millis
        );
    }

    #[test]
    fn mixed_valid_and_malformed_section_is_not_deletion_authoritative() {
        let mut store = EitStore::default();
        let start1 = [0xee, 0x00, 0x12, 0x00, 0x00];
        let start2 = [0xee, 0x01, 0x13, 0x00, 0x00];
        store.upsert_section(&section_with_crc(eit_body(1, &[(1, start1), (2, start2)])));
        assert_eq!(store.snapshot_present_following_actual().len(), 2);

        let invalid = [0xee, 0x01, 0x7a, 0x00, 0x00];
        store.upsert_section(&section_with_crc(eit_body(2, &[(1, start1), (2, invalid)])));

        let events = store.snapshot_present_following_actual();
        assert_eq!(
            events.len(),
            2,
            "不正要素を含む混在sectionは前回の正常eventを削除してはなりません"
        );
        let windows = store.take_present_following_actual_update_windows();
        assert!(windows.iter().any(|w| !w.deletion_authoritative));
    }
}

#[cfg(test)]
mod publication_scope_tests {
    use super::is_program_publish_eit_section;
    use crate::discovery_requirements::DiscoveryProfile;

    fn syntax_section(table_id: u8, section_number: u8) -> Vec<u8> {
        // policy判定に必要なsyntax headerだけを持つ最小section。CRC妥当性は上位のingestで検証する。
        vec![
            table_id,
            0xb0,
            0x05,
            0x00,
            0x01,
            0xc1,
            section_number,
            section_number,
        ]
    }

    #[test]
    fn satellite_pf_actual_allows_only_sections_zero_and_one() {
        for profile in [DiscoveryProfile::Bs, DiscoveryProfile::Cs110] {
            assert!(is_program_publish_eit_section(
                profile,
                0x0012,
                &syntax_section(0x4e, 0),
            ));
            assert!(is_program_publish_eit_section(
                profile,
                0x0012,
                &syntax_section(0x4e, 1),
            ));
            assert!(!is_program_publish_eit_section(
                profile,
                0x0012,
                &syntax_section(0x4e, 2),
            ));
            assert!(!is_program_publish_eit_section(
                profile,
                0x0012,
                &syntax_section(0x4e, 7),
            ));
        }
    }

    #[test]
    fn terrestrial_and_non_pf_actual_sections_are_unchanged() {
        assert!(is_program_publish_eit_section(
            DiscoveryProfile::IsdbT,
            0x0012,
            &syntax_section(0x4e, 2),
        ));
        assert!(is_program_publish_eit_section(
            DiscoveryProfile::Bs,
            0x0012,
            &syntax_section(0x50, 2),
        ));
        assert!(is_program_publish_eit_section(
            DiscoveryProfile::Bs,
            0x0011,
            &syntax_section(0x4e, 2),
        ));
    }
}

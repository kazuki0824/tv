//! TIS専用のEPG保存policy。放送由来factのpure parserとは別の責務を持つ。
use crate::discovery_requirements::DiscoveryProfile;
use crate::eit::{
    classify_timing, parse_eit_section, u16_at, EitEvent, EitStableEventIdentity, EitTimingState,
};
use crate::sections::parse_section_header;
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
struct EitSectionKey {
    table_id: u8,
    service_id: u16,
    transport_stream_id: u16,
    original_network_id: u16,
    section_number: u8,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct VersionedEventSet {
    version: u8,
    event_keys: BTreeSet<EitEventKey>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EitStore {
    events: BTreeMap<EitEventKey, EitEvent>,
    section_events: BTreeMap<EitSectionKey, VersionedEventSet>,
    last_update_windows: Vec<EitUpdateWindow>,
    diagnostic_section_events: BTreeMap<EitSectionKey, Vec<EitEvent>>,
}

impl EitStore {
    pub fn upsert_section(&mut self, section: &[u8]) {
        let Some(header) = parse_section_header(section) else {
            return;
        };
        let (Some(version), Some(section_number)) = (header.version, header.section_number) else {
            return;
        };
        if section.len() < 14 {
            return;
        }
        let service_id = u16_at(section, 3);
        let transport_stream_id = u16_at(section, 8);
        let original_network_id = u16_at(section, 10);
        let malformed_event_keys = malformed_eit_event_keys(section);
        let parsed = parse_eit_section(section);
        let deletion_authoritative = header.table_id == 0x4e
            && malformed_event_keys.is_empty()
            && parsed.iter().all(|event| {
                has_stable_identity(event.timing_state) && event.diagnostics.is_empty()
            });
        if parsed.is_empty() && !malformed_event_keys.is_empty() {
            // 不正 event だけの EIT section は、同じ section 内の既存有効 event を
            // すべて削除する根拠として扱わない。既存の VersionedEventSet と保存済み event を維持する。
            return;
        }
        let section_key = EitSectionKey {
            table_id: header.table_id,
            service_id,
            transport_stream_id,
            original_network_id,
            section_number,
        };
        let previous_keys: BTreeSet<EitEventKey> = self
            .section_events
            .get(&section_key)
            .map(|old| old.event_keys.clone())
            .unwrap_or_default();
        let obsolete_section_keys: BTreeSet<EitSectionKey> = if deletion_authoritative {
            self.section_events
                .iter()
                .filter(|(key, old)| {
                    key.table_id == header.table_id
                        && key.service_id == service_id
                        && key.transport_stream_id == transport_stream_id
                        && key.original_network_id == original_network_id
                        && old.version != version
                        && key.section_number > header.last_section_number.unwrap_or(section_number)
                })
                .map(|(key, _)| *key)
                .collect()
        } else {
            BTreeSet::new()
        };
        let obsolete_event_keys: BTreeSet<EitEventKey> = obsolete_section_keys
            .iter()
            .filter_map(|key| self.section_events.get(key))
            .flat_map(|old| old.event_keys.iter().copied())
            .collect();
        let surviving_section_references: BTreeSet<EitEventKey> = self
            .section_events
            .iter()
            .filter(|(key, _)| **key != section_key && !obsolete_section_keys.contains(*key))
            .flat_map(|(_, old)| old.event_keys.iter().copied())
            .collect();
        let new_keys: BTreeSet<_> = parsed.iter().filter_map(stable_event_key).collect();
        let removal_candidates: BTreeSet<EitEventKey> = previous_keys
            .difference(&new_keys)
            .copied()
            .chain(obsolete_event_keys.iter().copied())
            .collect();
        let removable_previous_keys: BTreeSet<_> = if deletion_authoritative {
            removal_candidates
                .into_iter()
                .filter(|old_key| !malformed_event_keys.contains(old_key))
                .filter(|old_key| !new_keys.contains(old_key))
                .filter(|old_key| !surviving_section_references.contains(old_key))
                .collect()
        } else {
            BTreeSet::new()
        };
        let mut window_events: Vec<EitEvent> = parsed.clone();
        for old_key in previous_keys.union(&removable_previous_keys) {
            if let Some(old_event) = self.events.get(old_key) {
                window_events.push(old_event.clone());
            }
        }
        if header.table_id == 0x4e && (!previous_keys.is_empty() || !new_keys.is_empty()) {
            let pf_actual_window_events: Vec<_> = window_events
                .iter()
                .filter(|event| {
                    event.table_id == 0x4e && event.timing_state == EitTimingState::Defined
                })
                .cloned()
                .collect();
            let pf_actual_current_events: Vec<_> = parsed
                .iter()
                .filter(|event| event.table_id == 0x4e && program_identity(event).is_some())
                .cloned()
                .collect();
            if let Some(window) = build_update_window(
                original_network_id,
                transport_stream_id,
                service_id,
                &pf_actual_window_events,
                &pf_actual_current_events,
                deletion_authoritative,
            ) {
                self.last_update_windows.retain(|existing| {
                    !(existing.original_network_id == window.original_network_id
                        && existing.transport_stream_id == window.transport_stream_id
                        && existing.service_id == window.service_id
                        && existing.window_start_millis == window.window_start_millis
                        && existing.window_end_millis == window.window_end_millis)
                });
                self.last_update_windows.push(window);
            }
        }
        for obsolete_section_key in &obsolete_section_keys {
            self.section_events.remove(obsolete_section_key);
            self.diagnostic_section_events.remove(obsolete_section_key);
        }
        for old_key in &removable_previous_keys {
            self.events.remove(old_key);
        }
        self.diagnostic_section_events
            .insert(section_key, parsed.clone());
        for event in parsed {
            if let Some(key) = stable_event_key(&event) {
                self.events.insert(key, event);
            }
        }
        self.section_events.insert(
            section_key,
            VersionedEventSet {
                version,
                event_keys: new_keys,
            },
        );
    }

    pub fn take_present_following_actual_update_windows(&mut self) -> Vec<EitUpdateWindow> {
        let mut out: Vec<_> = self
            .last_update_windows
            .drain(..)
            .filter(|window| window.window_end_millis > window.window_start_millis)
            .collect();
        out.sort_by_key(|w| {
            (
                w.original_network_id,
                w.transport_stream_id,
                w.service_id,
                w.window_start_millis,
                w.window_end_millis,
            )
        });
        out
    }

    #[cfg(test)]
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
        self.diagnostic_section_events
            .values()
            .flatten()
            .cloned()
            .collect()
    }

    #[cfg(test)]
    pub fn section_count_for_diagnostic(&self) -> usize {
        self.section_events.len()
    }
}

fn malformed_eit_event_keys(section: &[u8]) -> BTreeSet<EitEventKey> {
    let mut malformed = BTreeSet::new();
    let Some(header) = parse_section_header(section) else {
        return malformed;
    };
    if !(0x4e..=0x6f).contains(&header.table_id)
        || header.total_length > section.len()
        || header.section_length < 4
    {
        return malformed;
    }
    let body_end = 3 + header.section_length - 4;
    if section.len() < 14 || body_end <= 14 {
        return malformed;
    }
    let service_id = u16_at(section, 3);
    let tsid = u16_at(section, 8);
    let onid = u16_at(section, 10);
    let mut cursor = 14usize;
    while cursor + 12 <= body_end {
        let event_id = u16_at(section, cursor);
        let (timing_state, _, _) = classify_timing(section, cursor + 2, cursor + 7);
        let desc_len =
            (((section[cursor + 10] & 0x0f) as usize) << 8) | section[cursor + 11] as usize;
        let desc_start = cursor + 12;
        let Some(desc_end) = desc_start.checked_add(desc_len) else {
            break;
        };
        if timing_state == EitTimingState::MalformedTiming {
            malformed.insert(EitEventKey {
                table_id: header.table_id,
                original_network_id: onid,
                transport_stream_id: tsid,
                service_id,
                event_id,
            });
        }
        if desc_end > body_end {
            break;
        }
        cursor = desc_end;
    }
    malformed
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
    use crate::eit::EitScope;
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
    fn same_version_update_removes_events_absent_from_new_section() {
        let mut store = EitStore::default();
        let start1 = [0xee, 0x00, 0x12, 0x00, 0x00];
        let start2 = [0xee, 0x01, 0x13, 0x00, 0x00];
        store.upsert_section(&section_with_crc(eit_body(1, &[(1, start1), (2, start2)])));
        assert_eq!(store.snapshot_present_following_actual().len(), 2);
        store.upsert_section(&section_with_crc(eit_body(1, &[(1, start1)])));
        let events = store.snapshot_present_following_actual();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_id, 1);
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
    fn version_update_replaces_only_matching_section_number() {
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

        let events = store.snapshot_present_following_actual();
        assert_eq!(events.len(), 2);
        assert!(events
            .iter()
            .any(|event| event.event_id == 1 && event.version == 2));
        assert!(events
            .iter()
            .any(|event| event.event_id == 2 && event.version == 1));
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
        let events = store.snapshot_present_following_actual();
        assert_eq!(store.section_count_for_diagnostic(), 2);
        assert!(events
            .iter()
            .any(|event| event.event_id == 1 && event.version == 2));
        assert!(events
            .iter()
            .any(|event| event.event_id == 2 && event.version == 1));
        assert!(!events.iter().any(|event| event.event_id == 3));
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

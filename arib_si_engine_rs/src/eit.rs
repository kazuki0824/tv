use crate::descriptors::{event_descriptor_loop_truncated_diagnostic, parse_event_descriptors, DescriptorParseStatus, EventDescriptors, DescriptorDiagnostic};
use crate::sections::parse_section_header;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EitScope { PresentFollowing, R51MinimumSchedule, R53LongSchedule }

impl EitScope {
    pub fn as_str(self) -> &'static str {
        match self {
            EitScope::PresentFollowing => "present_following",
            EitScope::R51MinimumSchedule => "r51_minimum_schedule",
            EitScope::R53LongSchedule => "r53_long_schedule",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EitEvent {
    pub diagnostics: Vec<EitEventDiagnostic>,
    pub table_id: u8,
    pub scope: EitScope,
    pub service_id: u16,
    pub transport_stream_id: u16,
    pub original_network_id: u16,
    pub event_id: u16,
    pub start_time_millis: i64,
    pub duration_millis: i64,
    pub free_ca_mode: bool,
    pub descriptors: EventDescriptors,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct EitStableEventIdentity {
    pub original_network_id: u16,
    pub transport_stream_id: u16,
    pub service_id: u16,
    pub event_id: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EitEventDiagnostic {
    pub event_identity: EitStableEventIdentity,
    pub parse_status: DescriptorParseStatus,
    pub reason: String,
    pub descriptor_diagnostics: Vec<DescriptorDiagnostic>,
    pub malformed_descriptor_count: usize,
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
    original_network_id: u16,
    transport_stream_id: u16,
    service_id: u16,
    event_id: u16,
}

impl EitEvent {
    pub fn stable_identity(&self) -> EitStableEventIdentity {
        EitStableEventIdentity {
            original_network_id: self.original_network_id,
            transport_stream_id: self.transport_stream_id,
            service_id: self.service_id,
            event_id: self.event_id,
        }
    }
}

impl From<&EitEvent> for EitEventKey {
    fn from(event: &EitEvent) -> Self {
        Self {
            original_network_id: event.original_network_id,
            transport_stream_id: event.transport_stream_id,
            service_id: event.service_id,
            event_id: event.event_id,
        }
    }
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
}

impl EitStore {
    pub fn upsert_section(&mut self, section: &[u8]) {
        let Some(header) = parse_section_header(section, 12) else { return; };
        let (Some(version), Some(section_number)) = (header.version, header.section_number) else { return; };
        if section.len() < 14 { return; }
        let service_id = u16_at(section, 3);
        let transport_stream_id = u16_at(section, 8);
        let original_network_id = u16_at(section, 10);
        let malformed_event_keys = malformed_eit_event_keys(section);
        let parsed = parse_eit_section(section);
        let deletion_authoritative = malformed_event_keys.is_empty() && parsed.iter().all(|event| event.diagnostics.is_empty());
        if parsed.is_empty() && !malformed_event_keys.is_empty() {
            // Phase 6/N-23: a malformed-only EIT section must not be interpreted as
            // a deletion of all previously valid events in the same section. Keep
            // the previous VersionedEventSet and stored events intact.
            return;
        }
        let section_key = EitSectionKey {
            table_id: header.table_id,
            service_id,
            transport_stream_id,
            original_network_id,
            section_number,
        };
        let scope_section_keys: Vec<EitSectionKey> = self.section_events.keys()
            .filter(|key| key.table_id == header.table_id
                && key.service_id == service_id
                && key.transport_stream_id == transport_stream_id
                && key.original_network_id == original_network_id)
            .cloned()
            .collect();
        let scope_version_changed = scope_section_keys.iter().any(|key| {
            self.section_events.get(key).map(|old| old.version != version).unwrap_or(false)
        });
        let mut previous_keys: BTreeSet<EitEventKey> = BTreeSet::new();
        if scope_version_changed {
            for key in scope_section_keys {
                if let Some(old) = self.section_events.remove(&key) {
                    previous_keys.extend(old.event_keys);
                }
            }
        } else {
            previous_keys = self.section_events.get(&section_key).map(|old| old.event_keys.clone()).unwrap_or_default();
        }
        let new_keys: BTreeSet<_> = parsed.iter().map(EitEventKey::from).collect();
        let removable_previous_keys: BTreeSet<_> = if deletion_authoritative {
            previous_keys.difference(&new_keys)
                .filter(|old_key| !malformed_event_keys.contains(old_key))
                .copied()
                .collect()
        } else {
            BTreeSet::new()
        };
        let mut window_events: Vec<EitEvent> = parsed.clone();
        for old_key in &removable_previous_keys {
            if let Some(old_event) = self.events.get(old_key) {
                window_events.push(old_event.clone());
            }
        }
        if !previous_keys.is_empty() || !new_keys.is_empty() {
            let r51_window_events: Vec<_> = window_events.iter().filter(|event| event.scope != EitScope::R53LongSchedule).cloned().collect();
            let r51_current_events: Vec<_> = parsed.iter().filter(|event| event.scope != EitScope::R53LongSchedule).cloned().collect();
            if let Some(window) = build_update_window(original_network_id, transport_stream_id, service_id, &r51_window_events, &r51_current_events, deletion_authoritative) {
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
        for old_key in &removable_previous_keys {
            self.events.remove(old_key);
        }
        for event in parsed {
            self.events.insert(EitEventKey::from(&event), event);
        }
        self.section_events.insert(section_key, VersionedEventSet { version, event_keys: new_keys });
    }

    pub fn take_update_windows_r51(&mut self) -> Vec<EitUpdateWindow> {
        let mut out: Vec<_> = self.last_update_windows.drain(..)
            .filter(|window| window.window_end_millis > window.window_start_millis)
            .collect();
        out.sort_by_key(|w| (w.original_network_id, w.transport_stream_id, w.service_id, w.window_start_millis, w.window_end_millis));
        out
    }


    pub fn snapshot_r51(&self) -> Vec<EitEvent> {
        let mut out: Vec<_> = self.events.values().filter(|e| e.scope != EitScope::R53LongSchedule).cloned().collect();
        out.sort_by_key(|e| (e.original_network_id, e.transport_stream_id, e.service_id, e.start_time_millis, e.event_id));
        out
    }

    pub fn clear_update_windows(&mut self) { self.last_update_windows.clear(); }

    pub fn snapshot_all_for_diagnostic(&self) -> Vec<EitEvent> { self.events.values().cloned().collect() }

    pub fn section_count_for_diagnostic(&self) -> usize { self.section_events.len() }
}


fn malformed_eit_event_keys(section: &[u8]) -> BTreeSet<EitEventKey> {
    let mut malformed = BTreeSet::new();
    let Some(header) = parse_section_header(section, 12) else { return malformed; };
    if !(0x4e..=0x6f).contains(&header.table_id) || header.total_length > section.len() || header.section_length < 4 { return malformed; }
    let body_end = 3 + header.section_length - 4;
    if section.len() < 14 || body_end <= 14 { return malformed; }
    let service_id = u16_at(section, 3);
    let tsid = u16_at(section, 8);
    let onid = u16_at(section, 10);
    let mut cursor = 14usize;
    while cursor + 12 <= body_end {
        let event_id = u16_at(section, cursor);
        let start = decode_mjd_bcd_millis(section, cursor + 2);
        let duration = decode_duration_millis(section, cursor + 7);
        let desc_len = (((section[cursor + 10] & 0x0f) as usize) << 8) | section[cursor + 11] as usize;
        let desc_start = cursor + 12;
        let Some(desc_end) = desc_start.checked_add(desc_len) else { break; };
        if start.is_none() || duration.is_none() || start.unwrap_or(0) <= 0 || duration.unwrap_or(0) <= 0 {
            malformed.insert(EitEventKey { original_network_id: onid, transport_stream_id: tsid, service_id, event_id });
        }
        if desc_end > body_end { break; }
        cursor = desc_end;
    }
    malformed
}

fn build_update_window(onid: u16, tsid: u16, sid: u16, window_events: &[EitEvent], current_events: &[EitEvent], deletion_authoritative: bool) -> Option<EitUpdateWindow> {
    if window_events.is_empty() {
        return None;
    }
    let start = window_events.iter().map(|event| event.start_time_millis).min()?;
    let end = window_events.iter().map(|event| event.start_time_millis + event.duration_millis).max()?;
    if end <= start {
        return None;
    }
    let mut valid_event_identities: Vec<_> = current_events.iter().map(|event| event.stable_identity()).collect();
    valid_event_identities.sort_by_key(|identity| (identity.original_network_id, identity.transport_stream_id, identity.service_id, identity.event_id));
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

pub fn classify_table_id(table_id: u8) -> EitScope {
    match table_id { 0x4e | 0x4f => EitScope::PresentFollowing, 0x50..=0x5f => EitScope::R51MinimumSchedule, _ => EitScope::R53LongSchedule }
}

pub fn parse_eit_section(section: &[u8]) -> Vec<EitEvent> {
    let Some(header) = parse_section_header(section, 12) else { return Vec::new(); };
    if !(0x4e..=0x6f).contains(&header.table_id) || header.total_length > section.len() || header.section_length < 4 { return Vec::new(); }
    let body_end = 3 + header.section_length - 4;
    if section.len() < 14 || body_end <= 14 { return Vec::new(); }
    let service_id = u16_at(section, 3);
    let tsid = u16_at(section, 8);
    let onid = u16_at(section, 10);
    let scope = classify_table_id(header.table_id);
    let mut out = Vec::new();
    let mut cursor = 14usize;
    while cursor + 12 <= body_end {
        let event_id = u16_at(section, cursor);
        let start = decode_mjd_bcd_millis(section, cursor + 2);
        let duration = decode_duration_millis(section, cursor + 7);
        let free_ca_mode = (section[cursor + 10] & 0x10) != 0;
        let desc_len = (((section[cursor + 10] & 0x0f) as usize) << 8) | section[cursor + 11] as usize;
        let desc_start = cursor + 12;
        let Some(desc_end) = desc_start.checked_add(desc_len) else { break; };
        if desc_end > body_end {
            if let (Some(start), Some(duration)) = (start, duration) {
                if start > 0 && duration > 0 {
                    let identity = EitStableEventIdentity { original_network_id: onid, transport_stream_id: tsid, service_id, event_id };
                    let mut descriptors = EventDescriptors::default();
                    descriptors.diagnostics.push(event_descriptor_loop_truncated_diagnostic(
                        desc_start,
                        desc_len,
                        body_end.saturating_sub(desc_start),
                        &section[desc_start..body_end],
                    ));
                    let diagnostics = vec![EitEventDiagnostic {
                        event_identity: identity,
                        parse_status: DescriptorParseStatus::TruncatedDescriptor,
                        reason: "event descriptors_loop_length exceeds EIT section body".to_string(),
                        malformed_descriptor_count: descriptors.diagnostics.len(),
                        descriptor_diagnostics: descriptors.diagnostics.clone(),
                    }];
                    out.push(EitEvent { diagnostics, table_id: header.table_id, scope, service_id, transport_stream_id: tsid, original_network_id: onid, event_id, start_time_millis: start, duration_millis: duration, free_ca_mode, descriptors });
                }
            }
            break;
        }
        if let (Some(start), Some(duration)) = (start, duration) {
            if start > 0 && duration > 0 {
                let descriptors = parse_event_descriptors(&section[desc_start..desc_end]);
                let identity = EitStableEventIdentity { original_network_id: onid, transport_stream_id: tsid, service_id, event_id };
                let diagnostics = if descriptors.diagnostics.is_empty() {
                    Vec::new()
                } else {
                    vec![EitEventDiagnostic {
                        event_identity: identity,
                        parse_status: DescriptorParseStatus::TruncatedDescriptor,
                        reason: "event descriptor loop contains malformed descriptor".to_string(),
                        malformed_descriptor_count: descriptors.diagnostics.len(),
                        descriptor_diagnostics: descriptors.diagnostics.clone(),
                    }]
                };
                out.push(EitEvent { diagnostics, table_id: header.table_id, scope, service_id, transport_stream_id: tsid, original_network_id: onid, event_id, start_time_millis: start, duration_millis: duration, free_ca_mode, descriptors });
            }
        }
        cursor = desc_end;
    }
    out
}

fn u16_at(bytes: &[u8], offset: usize) -> u16 { u16::from_be_bytes([bytes[offset], bytes[offset + 1]]) }
fn decode_bcd2(v: u8) -> Option<i32> {
    let hi = (v >> 4) & 0x0f;
    let lo = v & 0x0f;
    (hi <= 9 && lo <= 9).then_some((hi as i32) * 10 + lo as i32)
}
fn decode_duration_millis(bytes: &[u8], offset: usize) -> Option<i64> {
    let h = decode_bcd2(bytes[offset])?;
    let m = decode_bcd2(bytes[offset + 1])?;
    let s = decode_bcd2(bytes[offset + 2])?;
    if m > 59 || s > 59 { return None; }
    Some(((h * 3600 + m * 60 + s) as i64) * 1000)
}
fn decode_mjd_bcd_millis(bytes: &[u8], offset: usize) -> Option<i64> {
    let mjd = u16_at(bytes, offset) as i32;
    if mjd == 0xffff { return None; }
    let (year, month, day) = mjd_to_ymd(mjd);
    let h = decode_bcd2(bytes[offset+2])?;
    let m = decode_bcd2(bytes[offset+3])?;
    let s = decode_bcd2(bytes[offset+4])?;
    if h > 23 || m > 59 || s > 59 { return None; }
    Some(civil_to_unix_millis(year, month, day, h, m, s) - 9 * 60 * 60 * 1000)
}
fn mjd_to_ymd(mjd: i32) -> (i32, i32, i32) {
    let jd = mjd + 2400001;
    let mut l = jd + 68569;
    let n = 4 * l / 146097;
    l -= (146097 * n + 3) / 4;
    let i = 4000 * (l + 1) / 1461001;
    l = l - 1461 * i / 4 + 31;
    let j = 80 * l / 2447;
    let day = l - 2447 * j / 80;
    l = j / 11;
    let month = j + 2 - 12 * l;
    let year = 100 * (n - 49) + i + l;
    (year, month, day)
}
fn civil_to_unix_millis(year: i32, month: i32, day: i32, hour: i32, minute: i32, second: i32) -> i64 {
    let y = year - (month <= 2) as i32;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    ((days as i64) * 86400 + (hour as i64) * 3600 + (minute as i64) * 60 + second as i64) * 1000
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sections::crc32_mpeg;

    fn section_with_crc(mut body: Vec<u8>) -> Vec<u8> {
        let crc = crc32_mpeg(&body);
        body.extend_from_slice(&crc.to_be_bytes());
        body
    }

    fn eit_body(version: u8, events: &[(u16, [u8; 5])]) -> Vec<u8> {
        let mut body = vec![0x50, 0xf0, 0x00, 0x00, 0x01, 0xc1 | ((version & 0x1f) << 1), 0x00, 0x00, 0x00, 0x11, 0x00, 0x22, 0x00, 0x00];
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
        assert_eq!(store.snapshot_r51().len(), 2);
        store.upsert_section(&section_with_crc(eit_body(1, &[(1, start1)])));
        let events = store.snapshot_r51();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_id, 1);
    }

    #[test]
    fn version_update_removes_events_absent_from_new_section() {
        let mut store = EitStore::default();
        let start1 = [0xee, 0x00, 0x12, 0x00, 0x00];
        let start2 = [0xee, 0x01, 0x13, 0x00, 0x00];
        store.upsert_section(&section_with_crc(eit_body(1, &[(1, start1), (2, start2)])));
        assert_eq!(store.snapshot_r51().len(), 2);
        store.upsert_section(&section_with_crc(eit_body(2, &[(2, start2)])));
        let events = store.snapshot_r51();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_id, 2);
    }

    #[test]
    fn start_time_change_updates_existing_stable_event_identity() {
        let mut store = EitStore::default();
        let start1 = [0xee, 0x00, 0x12, 0x00, 0x00];
        let start2 = [0xee, 0x02, 0x14, 0x00, 0x00];
        store.upsert_section(&section_with_crc(eit_body(1, &[(3, start1)])));
        store.upsert_section(&section_with_crc(eit_body(2, &[(3, start2)])));
        let events = store.snapshot_r51();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_id, 3);
    }

    #[test]
    fn stable_identity_is_independent_from_start_time_for_tvprovider_keying() {
        let event = EitEvent {
            diagnostics: Vec::new(),
            table_id: 0x50,
            scope: EitScope::R51MinimumSchedule,
            service_id: 1,
            transport_stream_id: 0x11,
            original_network_id: 0x22,
            event_id: 3,
            start_time_millis: 12345,
            duration_millis: 60000,
            free_ca_mode: false,
            descriptors: EventDescriptors::default(),
        };
        assert_eq!(event.stable_identity(), EitStableEventIdentity {
            original_network_id: 0x22,
            transport_stream_id: 0x11,
            service_id: 1,
            event_id: 3,
        });
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
        assert!(store.snapshot_r51().is_empty());
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
        assert!(store.snapshot_r51().is_empty());
    }


    #[test]
    fn invalid_hour_minute_second_ranges_are_rejected() {
        let mut store = EitStore::default();
        store.upsert_section(&section_with_crc(eit_body(1, &[(1, [0xee, 0x00, 0x24, 0x00, 0x00])])));
        store.upsert_section(&section_with_crc(eit_body(1, &[(2, [0xee, 0x00, 0x12, 0x60, 0x00])])));
        store.upsert_section(&section_with_crc(eit_body(1, &[(3, [0xee, 0x00, 0x12, 0x00, 0x60])])));
        assert!(store.snapshot_r51().is_empty());
    }

    #[test]
    fn undefined_mjd_is_rejected() {
        let mut store = EitStore::default();
        store.upsert_section(&section_with_crc(eit_body(1, &[(1, [0xff, 0xff, 0x12, 0x00, 0x00])])));
        assert!(store.snapshot_r51().is_empty());
    }

    #[test]
    fn descriptor_loop_overflow_is_kept_as_event_diagnostic() {
        let mut body = eit_body(1, &[(1, [0xee, 0x00, 0x12, 0x00, 0x00])]);
        body[24] = 0xf0;
        body[25] = 0x05;
        let mut store = EitStore::default();
        store.upsert_section(&section_with_crc(body));
        let events = store.snapshot_r51();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].diagnostics.len(), 1);
        assert_eq!(events[0].diagnostics[0].parse_status, DescriptorParseStatus::TruncatedDescriptor);
        assert_eq!(events[0].diagnostics[0].malformed_descriptor_count, 1);
    }


    #[test]
    fn malformed_only_section_does_not_delete_previous_valid_event() {
        let mut store = EitStore::default();
        let valid = [0xee, 0x00, 0x12, 0x00, 0x00];
        store.upsert_section(&section_with_crc(eit_body(1, &[(1, valid)])));
        assert_eq!(store.snapshot_r51().len(), 1);
        let invalid = [0xee, 0x00, 0x7a, 0x00, 0x00];
        store.upsert_section(&section_with_crc(eit_body(2, &[(1, invalid)])));
        let events = store.snapshot_r51();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_id, 1);
        assert_eq!(events[0].start_time_millis, parse_eit_section(&section_with_crc(eit_body(1, &[(1, valid)])))[0].start_time_millis);
    }

    #[test]
    fn mixed_valid_and_malformed_section_is_not_deletion_authoritative() {
        let mut store = EitStore::default();
        let start1 = [0xee, 0x00, 0x12, 0x00, 0x00];
        let start2 = [0xee, 0x01, 0x13, 0x00, 0x00];
        store.upsert_section(&section_with_crc(eit_body(1, &[(1, start1), (2, start2)])));
        assert_eq!(store.snapshot_r51().len(), 2);

        let invalid = [0xee, 0x01, 0x7a, 0x00, 0x00];
        store.upsert_section(&section_with_crc(eit_body(2, &[(1, start1), (2, invalid)])));

        let events = store.snapshot_r51();
        assert_eq!(events.len(), 2, "malformed mixed section must not remove previous normal event");
        let windows = store.take_update_windows_r51();
        assert!(windows.iter().any(|w| !w.deletion_authoritative));
    }

}

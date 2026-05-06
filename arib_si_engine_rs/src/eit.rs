use crate::descriptors::{parse_event_descriptors, EventDescriptors};
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct EitEventKey {
    original_network_id: u16,
    transport_stream_id: u16,
    service_id: u16,
    event_id: u16,
    start_time_millis: i64,
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
            start_time_millis: event.start_time_millis,
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
}

impl EitStore {
    pub fn upsert_section(&mut self, section: &[u8]) {
        let Some(header) = parse_section_header(section, 12) else { return; };
        let (Some(version), Some(section_number)) = (header.version, header.section_number) else { return; };
        if section.len() < 14 { return; }
        let service_id = u16_at(section, 3);
        let transport_stream_id = u16_at(section, 8);
        let original_network_id = u16_at(section, 10);
        let parsed = parse_eit_section(section);
        let section_key = EitSectionKey {
            table_id: header.table_id,
            service_id,
            transport_stream_id,
            original_network_id,
            section_number,
        };
        let new_keys: BTreeSet<_> = parsed.iter().map(EitEventKey::from).collect();
        if let Some(old) = self.section_events.get(&section_key) {
            if old.version != version {
                for old_key in old.event_keys.difference(&new_keys) {
                    self.events.remove(old_key);
                }
            }
        }
        for event in parsed {
            self.events.insert(EitEventKey::from(&event), event);
        }
        self.section_events.insert(section_key, VersionedEventSet { version, event_keys: new_keys });
    }

    pub fn snapshot_r51(&self) -> Vec<EitEvent> {
        let mut out: Vec<_> = self.events.values().filter(|e| e.scope != EitScope::R53LongSchedule).cloned().collect();
        out.sort_by_key(|e| (e.original_network_id, e.transport_stream_id, e.service_id, e.start_time_millis, e.event_id));
        out
    }

    pub fn snapshot_all_for_diagnostic(&self) -> Vec<EitEvent> { self.events.values().cloned().collect() }

    pub fn section_count_for_diagnostic(&self) -> usize { self.section_events.len() }
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
        if desc_end > body_end { break; }
        if start > 0 && duration > 0 {
            out.push(EitEvent { table_id: header.table_id, scope, service_id, transport_stream_id: tsid, original_network_id: onid, event_id, start_time_millis: start, duration_millis: duration, free_ca_mode, descriptors: parse_event_descriptors(&section[desc_start..desc_end]) });
        }
        cursor = desc_end;
    }
    out
}

fn u16_at(bytes: &[u8], offset: usize) -> u16 { u16::from_be_bytes([bytes[offset], bytes[offset + 1]]) }
fn bcd(v: u8) -> i32 { (((v >> 4) & 0x0f) as i32) * 10 + ((v & 0x0f) as i32) }
fn decode_duration_millis(bytes: &[u8], offset: usize) -> i64 { ((bcd(bytes[offset]) * 3600 + bcd(bytes[offset+1]) * 60 + bcd(bytes[offset+2])) as i64) * 1000 }
fn decode_mjd_bcd_millis(bytes: &[u8], offset: usize) -> i64 {
    let mjd = u16_at(bytes, offset) as i32;
    if mjd == 0xffff { return 0; }
    let (year, month, day) = mjd_to_ymd(mjd);
    let h = bcd(bytes[offset+2]); let m = bcd(bytes[offset+3]); let s = bcd(bytes[offset+4]);
    civil_to_unix_millis(year, month, day, h, m, s) - 9 * 60 * 60 * 1000
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
    fn start_time_change_is_old_key_delete_plus_new_key_upsert() {
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
}

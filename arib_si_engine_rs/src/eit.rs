use crate::descriptors::{
    event_descriptor_loop_truncated_diagnostic, parse_event_descriptors, DescriptorDiagnostic,
    DescriptorParseStatus, EventDescriptors,
};
use crate::sections::parse_section_header;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EitScope {
    PresentFollowingActual,
    PresentFollowingOther,
    ScheduleActual,
    ScheduleOther,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EitTimingState {
    Defined,
    UndefinedTime,
    BothTimingUndefined,
    MalformedTiming,
}

impl EitTimingState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Defined => "DEFINED",
            Self::UndefinedTime => "UNDEFINED_TIME",
            Self::BothTimingUndefined => "BOTH_TIMING_UNDEFINED",
            Self::MalformedTiming => "MALFORMED_TIMING",
        }
    }

    fn has_stable_identity(self) -> bool {
        matches!(self, Self::Defined | Self::UndefinedTime)
    }
}

impl EitScope {
    pub fn as_str(self) -> &'static str {
        match self {
            EitScope::PresentFollowingActual => "present_following_actual",
            EitScope::PresentFollowingOther => "present_following_other",
            EitScope::ScheduleActual => "schedule_actual",
            EitScope::ScheduleOther => "schedule_other",
            EitScope::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EitEvent {
    pub diagnostics: Vec<EitEventDiagnostic>,
    pub table_id: u8,
    pub version: u8,
    pub section_number: u8,
    pub last_section_number: u8,
    pub scope: EitScope,
    pub service_id: u16,
    pub transport_stream_id: u16,
    pub original_network_id: u16,
    pub event_id: u16,
    pub timing_state: EitTimingState,
    pub raw_start_time: [u8; 5],
    pub raw_duration: [u8; 3],
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
    pub event_identity: Option<EitStableEventIdentity>,
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
    table_id: u8,
    original_network_id: u16,
    transport_stream_id: u16,
    service_id: u16,
    event_id: u16,
}

impl EitEvent {
    pub fn stable_identity(&self) -> Option<EitStableEventIdentity> {
        self.timing_state
            .has_stable_identity()
            .then_some(EitStableEventIdentity {
                original_network_id: self.original_network_id,
                transport_stream_id: self.transport_stream_id,
                service_id: self.service_id,
                event_id: self.event_id,
            })
    }
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
    event
        .timing_state
        .has_stable_identity()
        .then(|| event.into())
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
        let Some(header) = parse_section_header(section, 12) else {
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
                event.timing_state.has_stable_identity() && event.diagnostics.is_empty()
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
        let scope_section_keys: Vec<EitSectionKey> = self
            .section_events
            .keys()
            .filter(|key| {
                key.table_id == header.table_id
                    && key.service_id == service_id
                    && key.transport_stream_id == transport_stream_id
                    && key.original_network_id == original_network_id
            })
            .cloned()
            .collect();
        let scope_version_changed = scope_section_keys.iter().any(|key| {
            self.section_events
                .get(key)
                .map(|old| old.version != version)
                .unwrap_or(false)
        });
        let mut previous_keys: BTreeSet<EitEventKey> = BTreeSet::new();
        if scope_version_changed {
            for key in scope_section_keys {
                if let Some(old) = self.section_events.remove(&key) {
                    previous_keys.extend(old.event_keys);
                }
                self.diagnostic_section_events.remove(&key);
            }
        } else {
            previous_keys = self
                .section_events
                .get(&section_key)
                .map(|old| old.event_keys.clone())
                .unwrap_or_default();
        }
        let new_keys: BTreeSet<_> = parsed.iter().filter_map(stable_event_key).collect();
        let removable_previous_keys: BTreeSet<_> = if deletion_authoritative {
            previous_keys
                .difference(&new_keys)
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
                .filter(|event| event.table_id == 0x4e && event.stable_identity().is_some())
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

    pub fn clear_update_windows(&mut self) {
        self.last_update_windows.clear();
    }

    pub fn snapshot_all_for_diagnostic(&self) -> Vec<EitEvent> {
        self.diagnostic_section_events
            .values()
            .flatten()
            .cloned()
            .collect()
    }

    pub fn section_count_for_diagnostic(&self) -> usize {
        self.section_events.len()
    }
}

fn malformed_eit_event_keys(section: &[u8]) -> BTreeSet<EitEventKey> {
    let mut malformed = BTreeSet::new();
    let Some(header) = parse_section_header(section, 12) else {
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
    let mut valid_event_identities: Vec<_> = current_events
        .iter()
        .filter_map(|event| event.stable_identity())
        .collect();
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

pub fn classify_table_id(table_id: u8) -> EitScope {
    match table_id {
        0x4e => EitScope::PresentFollowingActual,
        0x4f => EitScope::PresentFollowingOther,
        0x50..=0x5f => EitScope::ScheduleActual,
        0x60..=0x6f => EitScope::ScheduleOther,
        _ => EitScope::Unknown,
    }
}

pub fn parse_eit_section(section: &[u8]) -> Vec<EitEvent> {
    let Some(header) = parse_section_header(section, 12) else {
        return Vec::new();
    };
    if !(0x4e..=0x6f).contains(&header.table_id)
        || header.total_length > section.len()
        || header.section_length < 4
    {
        return Vec::new();
    }
    let body_end = 3 + header.section_length - 4;
    if section.len() < 14 || body_end <= 14 {
        return Vec::new();
    }
    let service_id = u16_at(section, 3);
    let tsid = u16_at(section, 8);
    let onid = u16_at(section, 10);
    let scope = classify_table_id(header.table_id);
    let mut out = Vec::new();
    let mut cursor = 14usize;
    while cursor + 12 <= body_end {
        let event_id = u16_at(section, cursor);
        let mut raw_start_time = [0u8; 5];
        raw_start_time.copy_from_slice(&section[cursor + 2..cursor + 7]);
        let mut raw_duration = [0u8; 3];
        raw_duration.copy_from_slice(&section[cursor + 7..cursor + 10]);
        let (timing_state, start, duration) = classify_timing(section, cursor + 2, cursor + 7);
        let free_ca_mode = (section[cursor + 10] & 0x10) != 0;
        let desc_len =
            (((section[cursor + 10] & 0x0f) as usize) << 8) | section[cursor + 11] as usize;
        let desc_start = cursor + 12;
        let Some(desc_end) = desc_start.checked_add(desc_len) else {
            break;
        };
        let descriptor_truncated = desc_end > body_end;
        let mut descriptors = if descriptor_truncated {
            EventDescriptors::default()
        } else {
            parse_event_descriptors(&section[desc_start..desc_end])
        };
        if descriptor_truncated {
            descriptors
                .diagnostics
                .push(event_descriptor_loop_truncated_diagnostic(
                    desc_start,
                    desc_len,
                    body_end.saturating_sub(desc_start),
                    &section[desc_start..body_end],
                ));
        }
        let identity = timing_state
            .has_stable_identity()
            .then_some(EitStableEventIdentity {
                original_network_id: onid,
                transport_stream_id: tsid,
                service_id,
                event_id,
            });
        let mut diagnostics = Vec::new();
        if timing_state == EitTimingState::MalformedTiming {
            diagnostics.push(EitEventDiagnostic {
                event_identity: None,
                parse_status: DescriptorParseStatus::InvalidSequence,
                reason: "EIT start_time or duration contains malformed BCD/time fields".to_string(),
                malformed_descriptor_count: 0,
                descriptor_diagnostics: Vec::new(),
            });
        }
        if !descriptors.diagnostics.is_empty() {
            diagnostics.push(EitEventDiagnostic {
                event_identity: identity,
                parse_status: if descriptor_truncated {
                    DescriptorParseStatus::TruncatedDescriptor
                } else {
                    DescriptorParseStatus::MalformedLength
                },
                reason: if descriptor_truncated {
                    "event descriptors_loop_length exceeds EIT section body".to_string()
                } else {
                    "event descriptor loop contains malformed descriptor".to_string()
                },
                malformed_descriptor_count: descriptors.diagnostics.len(),
                descriptor_diagnostics: descriptors.diagnostics.clone(),
            });
        }
        out.push(EitEvent {
            diagnostics,
            table_id: header.table_id,
            version: header.version.unwrap_or(0),
            section_number: header.section_number.unwrap_or(0),
            last_section_number: header.last_section_number.unwrap_or(0),
            scope,
            service_id,
            transport_stream_id: tsid,
            original_network_id: onid,
            event_id,
            timing_state,
            raw_start_time,
            raw_duration,
            start_time_millis: start.unwrap_or(0),
            duration_millis: duration.unwrap_or(0),
            free_ca_mode,
            descriptors,
        });
        if descriptor_truncated {
            break;
        }
        cursor = desc_end;
    }
    out
}

fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes([bytes[offset], bytes[offset + 1]])
}
fn decode_bcd2(v: u8) -> Option<i32> {
    let hi = (v >> 4) & 0x0f;
    let lo = v & 0x0f;
    (hi <= 9 && lo <= 9).then_some((hi as i32) * 10 + lo as i32)
}
fn classify_timing(
    bytes: &[u8],
    start_offset: usize,
    duration_offset: usize,
) -> (EitTimingState, Option<i64>, Option<i64>) {
    let start_undefined = bytes[start_offset..start_offset + 5]
        .iter()
        .all(|byte| *byte == 0xff);
    let duration_undefined = bytes[duration_offset..duration_offset + 3]
        .iter()
        .all(|byte| *byte == 0xff);
    if start_undefined && duration_undefined {
        return (EitTimingState::BothTimingUndefined, None, None);
    }
    if start_undefined || duration_undefined {
        let start = (!start_undefined)
            .then(|| decode_mjd_bcd_millis(bytes, start_offset))
            .flatten();
        let duration = (!duration_undefined)
            .then(|| decode_duration_millis(bytes, duration_offset))
            .flatten();
        if (!start_undefined && start.is_none()) || (!duration_undefined && duration.is_none()) {
            return (EitTimingState::MalformedTiming, start, duration);
        }
        return (EitTimingState::UndefinedTime, start, duration);
    }
    let start = decode_mjd_bcd_millis(bytes, start_offset);
    let duration = decode_duration_millis(bytes, duration_offset);
    if start.is_none() || duration.is_none() {
        (EitTimingState::MalformedTiming, start, duration)
    } else {
        (EitTimingState::Defined, start, duration)
    }
}
fn decode_duration_millis(bytes: &[u8], offset: usize) -> Option<i64> {
    let h = decode_bcd2(bytes[offset])?;
    let m = decode_bcd2(bytes[offset + 1])?;
    let s = decode_bcd2(bytes[offset + 2])?;
    if m > 59 || s > 59 {
        return None;
    }
    Some(((h * 3600 + m * 60 + s) as i64) * 1000)
}
fn decode_mjd_bcd_millis(bytes: &[u8], offset: usize) -> Option<i64> {
    let mjd = u16_at(bytes, offset) as i32;
    if mjd == 0xffff {
        return None;
    }
    let (year, month, day) = mjd_to_ymd(mjd);
    let h = decode_bcd2(bytes[offset + 2])?;
    let m = decode_bcd2(bytes[offset + 3])?;
    let s = decode_bcd2(bytes[offset + 4])?;
    if h > 23 || m > 59 || s > 59 {
        return None;
    }
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
fn civil_to_unix_millis(
    year: i32,
    month: i32,
    day: i32,
    hour: i32,
    minute: i32,
    second: i32,
) -> i64 {
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
        // EIT header(14) + first event(12) + event_id(2) + start_time(5).
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
            event.stable_identity(),
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
mod eit_scope_contract_tests {
    use super::{classify_table_id, EitScope};

    #[test]
    fn classifies_eit_scope_from_arib_table_identity_only() {
        assert_eq!(classify_table_id(0x4e), EitScope::PresentFollowingActual);
        assert_eq!(classify_table_id(0x4f), EitScope::PresentFollowingOther);
        assert_eq!(classify_table_id(0x50), EitScope::ScheduleActual);
        assert_eq!(classify_table_id(0x5f), EitScope::ScheduleActual);
        assert_eq!(classify_table_id(0x60), EitScope::ScheduleOther);
        assert_eq!(classify_table_id(0x6f), EitScope::ScheduleOther);
        assert_eq!(classify_table_id(0x70), EitScope::Unknown);
    }
}

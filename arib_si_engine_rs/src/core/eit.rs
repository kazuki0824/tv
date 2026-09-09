use crate::descriptors::{
    event_descriptor_loop_truncated_diagnostic, parse_event_descriptors, DescriptorDiagnostic,
    DescriptorParseStatus, EventDescriptors, TruncatedDescriptorLoop,
};
use crate::sections::parse_section_header;

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
    pub fn has_stable_identity(self) -> bool {
        matches!(self, Self::Defined | Self::UndefinedTime)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Defined => "DEFINED",
            Self::UndefinedTime => "UNDEFINED_TIME",
            Self::BothTimingUndefined => "BOTH_TIMING_UNDEFINED",
            Self::MalformedTiming => "MALFORMED_TIMING",
        }
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

pub fn classify_table_id(table_id: u8) -> EitScope {
    match table_id {
        0x4e => EitScope::PresentFollowingActual,
        0x4f => EitScope::PresentFollowingOther,
        0x50..=0x5f => EitScope::ScheduleActual,
        0x60..=0x6f => EitScope::ScheduleOther,
        _ => EitScope::Unknown,
    }
}

#[derive(Default)]
pub struct EitSectionFacts {
    pub events: Vec<EitEvent>,
    pub event_loop_complete: bool,
}

pub fn parse_eit_section(section: &[u8]) -> Vec<EitEvent> {
    parse_eit_section_facts(section).events
}

pub fn parse_eit_section_facts(section: &[u8]) -> EitSectionFacts {
    let Some(header) = parse_section_header(section) else {
        return EitSectionFacts::default();
    };
    if !(0x4e..=0x6f).contains(&header.table_id)
        || header.total_length > section.len()
        || header.section_length < 4
    {
        return EitSectionFacts::default();
    }
    let body_end = 3 + header.section_length - 4;
    if section.len() < 14 || body_end < 14 {
        return EitSectionFacts::default();
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
        // CRC手前の受信範囲だけを共通parserへ渡し、読める記述子事実を保持する。
        let available = &section[desc_start..desc_end.min(body_end)];
        let mut descriptors = parse_event_descriptors(available);
        if descriptor_truncated {
            descriptors.truncated_loop = Some(TruncatedDescriptorLoop {
                declared_length: desc_len,
                raw_bytes: available.to_vec(),
            });
            descriptors
                .diagnostics
                .push(event_descriptor_loop_truncated_diagnostic(
                    desc_start,
                    desc_len,
                    body_end.saturating_sub(desc_start),
                    &section[desc_start..body_end],
                ));
        }
        let identity = timing_state.has_stable_identity().then_some(EitStableEventIdentity {
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
                reason: "EITの開始時刻または継続時間に不正なBCD・時刻欄があります".to_string(),
                malformed_descriptor_count: 0,
                descriptor_diagnostics: Vec::new(),
            });
        }
        for diagnostic in &descriptors.diagnostics {
            diagnostics.push(EitEventDiagnostic {
                event_identity: identity,
                parse_status: diagnostic.parse_status,
                reason: diagnostic.message.clone(),
                malformed_descriptor_count: usize::from(
                    diagnostic.parse_status.is_structural_error(),
                ),
                descriptor_diagnostics: vec![diagnostic.clone()],
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
    EitSectionFacts {
        events: out,
        event_loop_complete: cursor == body_end,
    }
}

pub(crate) fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes([bytes[offset], bytes[offset + 1]])
}
fn decode_bcd2(v: u8) -> Option<i32> {
    let hi = (v >> 4) & 0x0f;
    let lo = v & 0x0f;
    (hi <= 9 && lo <= 9).then_some((hi as i32) * 10 + lo as i32)
}
pub(crate) fn classify_timing(
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

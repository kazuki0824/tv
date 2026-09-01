use super::packet_pipeline::PacketPid;
use maleicacid_tuner_hal2_common::TS_PACKET_SIZE;
use std::collections::BTreeMap;

pub const MAX_PES_BUFFER_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContinuityOutcome {
    FirstPacket,
    InOrder,
    Duplicate,
    CounterCollision,
    Discontinuity,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ContinuityState {
    last_counter: Option<u8>,
    last_packet: Option<[u8; TS_PACKET_SIZE]>,
}

#[derive(Clone, Debug, Default)]
pub struct ContinuityTracker {
    states: BTreeMap<PacketPid, ContinuityState>,
}

impl ContinuityTracker {
    pub fn observe(
        &mut self,
        pid: PacketPid,
        continuity_counter: u8,
        has_payload: bool,
        packet: &[u8; TS_PACKET_SIZE],
    ) -> ContinuityOutcome {
        if !has_payload {
            return ContinuityOutcome::InOrder;
        }
        let state = self.states.entry(pid).or_default();
        let Some(last_counter) = state.last_counter else {
            state.last_counter = Some(continuity_counter);
            state.last_packet = Some(*packet);
            return ContinuityOutcome::FirstPacket;
        };
        if continuity_counter == last_counter {
            if state.last_packet.as_ref() == Some(packet) {
                return ContinuityOutcome::Duplicate;
            }
            state.last_packet = Some(*packet);
            return ContinuityOutcome::CounterCollision;
        }
        let expected = (last_counter + 1) & 0x0f;
        state.last_counter = Some(continuity_counter);
        state.last_packet = Some(*packet);
        if continuity_counter == expected {
            ContinuityOutcome::InOrder
        } else {
            ContinuityOutcome::Discontinuity
        }
    }

    pub fn reset_pid(&mut self, pid: PacketPid) {
        self.states.remove(&pid);
    }
}

// TS byte stream の分割・resync は common の TsPacketCompletionBuffer だけを正とする。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PesPacket {
    pub pid: PacketPid,
    pub stream_id: u8,
    pub pts_90khz: Option<u64>,
    pub dts_90khz: Option<u64>,
    pub is_pes_private_data: bool,
    pub data_alignment_indicator: bool,
    pub raw_bytes: Vec<u8>,
    pub payload: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PesHeaderSummary {
    pub stream_id: u8,
    pub payload_offset: usize,
    pub pts_90khz: Option<u64>,
    pub dts_90khz: Option<u64>,
    pub is_pes_private_data: bool,
    pub data_alignment_indicator: bool,
    pub expected_len: Option<usize>,
}

fn pes_stream_has_optional_header(stream_id: u8) -> bool {
    !matches!(
        stream_id,
        0xbc | 0xbe | 0xbf | 0xf0 | 0xf1 | 0xf2 | 0xf8 | 0xff
    )
}

fn pts_dts_field_value(field: &[u8], expected_prefix: u8) -> Option<u64> {
    if field.len() < 5 {
        return None;
    }
    if (field[0] >> 4) != expected_prefix {
        return None;
    }
    if (field[0] & 0x01) == 0 || (field[2] & 0x01) == 0 || (field[4] & 0x01) == 0 {
        return None;
    }
    let value = (((field[0] >> 1) as u64) & 0x07) << 30
        | ((field[1] as u64) << 22)
        | (((field[2] >> 1) as u64) << 15)
        | ((field[3] as u64) << 7)
        | ((field[4] >> 1) as u64);
    Some(value)
}

fn advance_optional_field(cursor: &mut usize, length: usize, header_len: usize) -> Option<()> {
    let end = cursor.checked_add(length)?;
    if end > header_len {
        return None;
    }
    *cursor = end;
    Some(())
}

fn pes_private_data_presence(flags2: u8, optional_header: &[u8]) -> Option<bool> {
    let mut cursor = match (flags2 >> 6) & 0x03 {
        0b00 => 0,
        0b10 => 5,
        0b11 => 10,
        _ => return None,
    };
    advance_optional_field(&mut cursor, 0, optional_header.len())?;

    for (flag, length) in [(0x20, 6), (0x10, 3), (0x08, 1), (0x04, 1), (0x02, 2)] {
        if (flags2 & flag) != 0 {
            advance_optional_field(&mut cursor, length, optional_header.len())?;
        }
    }
    if (flags2 & 0x01) == 0 {
        return Some(false);
    }

    let extension_flags = *optional_header.get(cursor)?;
    cursor = cursor.checked_add(1)?;
    if (extension_flags & 0x0e) != 0x0e {
        return None;
    }
    let is_private_data = (extension_flags & 0x80) != 0;
    if is_private_data {
        advance_optional_field(&mut cursor, 16, optional_header.len())?;
    }
    if (extension_flags & 0x40) != 0 {
        let pack_header_len = usize::from(*optional_header.get(cursor)?);
        cursor = cursor.checked_add(1)?;
        advance_optional_field(&mut cursor, pack_header_len, optional_header.len())?;
    }
    if (extension_flags & 0x20) != 0 {
        advance_optional_field(&mut cursor, 2, optional_header.len())?;
    }
    if (extension_flags & 0x10) != 0 {
        advance_optional_field(&mut cursor, 2, optional_header.len())?;
    }
    if (extension_flags & 0x01) != 0 {
        let extension_length = *optional_header.get(cursor)?;
        if (extension_length & 0x80) == 0 {
            return None;
        }
        cursor = cursor.checked_add(1)?;
        advance_optional_field(
            &mut cursor,
            usize::from(extension_length & 0x7f),
            optional_header.len(),
        )?;
    }
    Some(is_private_data)
}

enum PesHeaderParseStatus {
    Incomplete,
    Malformed,
    Complete(PesHeaderSummary),
}

fn parse_pes_header_status(bytes: &[u8]) -> PesHeaderParseStatus {
    if bytes.len() < 6 {
        return PesHeaderParseStatus::Incomplete;
    }
    if &bytes[..3] != [0x00, 0x00, 0x01] {
        return PesHeaderParseStatus::Malformed;
    }
    let stream_id = bytes[3];
    let packet_length = u16::from_be_bytes([bytes[4], bytes[5]]) as usize;
    if packet_length == 0 && !(0xe0..=0xef).contains(&stream_id) {
        return PesHeaderParseStatus::Malformed;
    }
    if !pes_stream_has_optional_header(stream_id) {
        let expected_len = if packet_length == 0 {
            None
        } else {
            Some(6 + packet_length)
        };
        return PesHeaderParseStatus::Complete(PesHeaderSummary {
            stream_id,
            payload_offset: 6,
            pts_90khz: None,
            dts_90khz: None,
            is_pes_private_data: false,
            data_alignment_indicator: false,
            expected_len,
        });
    }
    if bytes.len() < 9 {
        return PesHeaderParseStatus::Incomplete;
    }
    let flags1 = bytes[6];
    let flags2 = bytes[7];
    let header_data_len = bytes[8] as usize;
    let payload_offset = 9 + header_data_len;
    if (flags1 & 0xc0) != 0x80 {
        return PesHeaderParseStatus::Malformed;
    }
    if packet_length != 0 && packet_length < 3 + header_data_len {
        return PesHeaderParseStatus::Malformed;
    }
    let pts_dts_flags = (flags2 >> 6) & 0x03;
    if pts_dts_flags == 0b01 {
        return PesHeaderParseStatus::Malformed;
    }
    match pts_dts_flags {
        0b10 if header_data_len < 5 => return PesHeaderParseStatus::Malformed,
        0b11 if header_data_len < 10 => return PesHeaderParseStatus::Malformed,
        _ => {}
    }
    if bytes.len() < payload_offset {
        return PesHeaderParseStatus::Incomplete;
    }
    let data_alignment_indicator = (flags1 & 0x04) != 0;
    let pts_90khz = match pts_dts_flags {
        0b10 => match pts_dts_field_value(&bytes[9..14], 0b0010) {
            Some(value) => Some(value),
            None => return PesHeaderParseStatus::Malformed,
        },
        0b11 => match pts_dts_field_value(&bytes[9..14], 0b0011) {
            Some(value) => Some(value),
            None => return PesHeaderParseStatus::Malformed,
        },
        _ => None,
    };
    let dts_90khz = match pts_dts_flags {
        0b11 => match pts_dts_field_value(&bytes[14..19], 0b0001) {
            Some(value) => Some(value),
            None => return PesHeaderParseStatus::Malformed,
        },
        _ => None,
    };
    let is_pes_private_data = match pes_private_data_presence(flags2, &bytes[9..payload_offset]) {
        Some(value) => value,
        None => return PesHeaderParseStatus::Malformed,
    };
    let expected_len = if packet_length == 0 {
        None
    } else {
        Some(6 + packet_length)
    };
    PesHeaderParseStatus::Complete(PesHeaderSummary {
        stream_id,
        payload_offset,
        pts_90khz,
        dts_90khz,
        is_pes_private_data,
        data_alignment_indicator,
        expected_len,
    })
}

pub fn parse_pes_header_summary(bytes: &[u8]) -> Option<PesHeaderSummary> {
    match parse_pes_header_status(bytes) {
        PesHeaderParseStatus::Complete(summary) => Some(summary),
        PesHeaderParseStatus::Incomplete | PesHeaderParseStatus::Malformed => None,
    }
}

#[cfg(test)]
fn parse_pes_header(bytes: &[u8]) -> Option<PesHeaderSummary> {
    parse_pes_header_summary(bytes)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PesDropReason {
    ContinuationWithoutStart,
    MalformedPes,
    OversizedPes,
    FlushDiscard,
}

#[derive(Clone, Debug, Default)]
pub struct PesAssembler {
    pid: Option<PacketPid>,
    buf: Vec<u8>,
    expected_len: Option<usize>,
    unbounded_summary: Option<PesHeaderSummary>,
    overflow_drop_count: u64,
    overflow_generation: u64,
    overflow_drop_counter_saturated: bool,
    overflow_generation_counter_saturated: bool,
    last_drop_reason: Option<PesDropReason>,
}

impl PesAssembler {
    pub fn push(
        &mut self,
        pid: PacketPid,
        payload_unit_start: bool,
        payload: &[u8],
    ) -> Vec<PesPacket> {
        let mut out = Vec::new();
        if payload_unit_start {
            if self.unbounded_summary.is_some() {
                if let Some(packet) = self.take_completed() {
                    out.push(packet);
                }
            }
            self.reset_state_only();
            self.pid = Some(pid);
        } else if self.pid != Some(pid) {
            self.reset_with_drop(PesDropReason::ContinuationWithoutStart);
            return out;
        }

        self.buf.extend_from_slice(payload);
        if self.expected_len.is_none() {
            match parse_pes_header_status(&self.buf) {
                PesHeaderParseStatus::Complete(summary) => {
                    self.expected_len = summary.expected_len;
                    self.unbounded_summary = if summary.expected_len.is_none() {
                        Some(summary)
                    } else {
                        None
                    };
                }
                PesHeaderParseStatus::Incomplete => {}
                PesHeaderParseStatus::Malformed => {
                    self.reset_with_drop(PesDropReason::MalformedPes);
                    return out;
                }
            }
        }
        if self.buf.len() > MAX_PES_BUFFER_BYTES {
            self.reset_with_drop(PesDropReason::OversizedPes);
            return out;
        }
        if let Some(expected_len) = self.expected_len {
            if self.buf.len() >= expected_len {
                if let Some(packet) = self.take_completed() {
                    out.push(packet);
                }
            }
        }
        out
    }

    #[cfg(test)]
    fn flush(&mut self) -> Option<PesPacket> {
        self.reset_with_drop(PesDropReason::FlushDiscard);
        None
    }

    fn reset_state_only(&mut self) {
        self.pid = None;
        self.buf.clear();
        self.expected_len = None;
        self.unbounded_summary = None;
    }

    fn reset_with_drop(&mut self, reason: PesDropReason) {
        if self.pid.is_some()
            || !self.buf.is_empty()
            || self.expected_len.is_some()
            || self.unbounded_summary.is_some()
        {
            match self.overflow_drop_count.checked_add(1) {
                Some(next) => self.overflow_drop_count = next,
                None => self.overflow_drop_counter_saturated = true,
            }
            match self.overflow_generation.checked_add(1) {
                Some(next) => self.overflow_generation = next,
                None => self.overflow_generation_counter_saturated = true,
            }
            self.last_drop_reason = Some(reason);
        }
        self.reset_state_only();
    }

    pub fn take_drop_diagnostic(&mut self) -> Option<(PesDropReason, u64)> {
        self.last_drop_reason
            .take()
            .map(|reason| (reason, self.overflow_generation))
    }

    #[cfg(test)]
    fn overflow_drop_count(&self) -> u64 {
        self.overflow_drop_count
    }

    fn take_completed(&mut self) -> Option<PesPacket> {
        let pid = self.pid?;
        let summary = parse_pes_header_summary(&self.buf).or(self.unbounded_summary)?;
        if let Some(expected_len) = summary.expected_len {
            if self.buf.len() < expected_len {
                return None;
            }
        }
        let mut raw_bytes = std::mem::take(&mut self.buf);
        if let Some(expected_len) = summary.expected_len {
            raw_bytes.truncate(expected_len);
        }
        let payload = if raw_bytes.starts_with(&[0x00, 0x00, 0x01]) {
            raw_bytes[summary.payload_offset.min(raw_bytes.len())..].to_vec()
        } else {
            raw_bytes.clone()
        };
        self.expected_len = None;
        self.unbounded_summary = None;
        self.pid = None;
        Some(PesPacket {
            pid,
            stream_id: summary.stream_id,
            pts_90khz: summary.pts_90khz,
            dts_90khz: summary.dts_90khz,
            is_pes_private_data: summary.is_pes_private_data,
            data_alignment_indicator: summary.data_alignment_indicator,
            raw_bytes,
            payload,
        })
    }
}

#[cfg(test)]
fn packet_pid_for_test(pid: i32) -> PacketPid {
    PacketPid::from_config_pid(
        crate::config::ConfigInputPid::validate_tpid(pid).expect("valid test pid"),
    )
}

#[cfg(test)]
mod tests {

    use super::super::packet_pipeline::PacketPid;
    use super::{ContinuityOutcome, ContinuityTracker, PesAssembler};
    use crate::config::ConfigInputPid;
    use maleicacid_tuner_hal2_common::TsPacketCompletionBuffer;
    use maleicacid_tuner_hal2_common::TS_PACKET_SIZE;

    fn packet_pid(pid: i32) -> PacketPid {
        PacketPid::from_config_pid(ConfigInputPid::validate_tpid(pid).expect("valid test pid"))
    }

    fn make_packet(pid: u16, cc: u8) -> [u8; TS_PACKET_SIZE] {
        let mut packet = [0xff; TS_PACKET_SIZE];
        packet[0] = 0x47;
        packet[1] = ((pid >> 8) as u8) & 0x1f;
        packet[2] = pid as u8;
        packet[3] = 0x10 | (cc & 0x0f);
        packet
    }

    #[test]
    fn completion_buffer_requires_three_syncs_after_garbage_prefix() {
        let mut resync = TsPacketCompletionBuffer::default();
        let mut input = vec![0x00, 0x11, 0x22];
        input.extend_from_slice(&make_packet(0x30, 0));
        input.extend_from_slice(&make_packet(0x30, 1));
        let first = resync.push(&input);
        assert!(first.packets.is_empty());
        assert_eq!(first.malformed_bytes, 3);
        let tail = resync.push(&make_packet(0x30, 2));
        assert_eq!(tail.packets.len(), 3);
        assert_eq!(tail.malformed_bytes, 0);
    }

    #[test]
    fn completion_buffer_returns_complete_aligned_tail_without_next_sync() {
        let mut buffer = TsPacketCompletionBuffer::default();
        let packet = make_packet(0x0123, 7);
        let out = buffer.push(&packet);
        assert_eq!(out.packets, vec![packet]);
    }

    #[test]
    fn completion_buffer_does_not_sync_on_single_stray_sync_byte() {
        let mut resync = TsPacketCompletionBuffer::default();
        let mut input = vec![0x00, 0x47, 0x22];
        input.extend_from_slice(&make_packet(0x31, 0));
        input.extend_from_slice(&make_packet(0x31, 1));
        assert!(resync.push(&input).packets.is_empty());
    }

    #[test]
    fn continuity_tracker_flags_duplicate_and_gap() {
        let mut tracker = ContinuityTracker::default();
        let packet_0 = make_packet(256, 0);
        let packet_1 = make_packet(256, 1);
        let packet_3 = make_packet(256, 3);
        assert_eq!(
            tracker.observe(packet_pid(256), 0, true, &packet_0),
            ContinuityOutcome::FirstPacket
        );
        assert_eq!(
            tracker.observe(packet_pid(256), 1, true, &packet_1),
            ContinuityOutcome::InOrder
        );
        assert_eq!(
            tracker.observe(packet_pid(256), 1, true, &packet_1),
            ContinuityOutcome::Duplicate
        );
        assert_eq!(
            tracker.observe(packet_pid(256), 3, true, &packet_3),
            ContinuityOutcome::Discontinuity
        );
    }

    #[test]
    fn continuity_tracker_requires_full_packet_equality_for_duplicate() {
        let mut tracker = ContinuityTracker::default();
        let first = make_packet(256, 5);
        let mut changed = first;
        changed[20] ^= 0x01;

        assert_eq!(
            tracker.observe(packet_pid(256), 5, true, &first),
            ContinuityOutcome::FirstPacket
        );
        assert_eq!(
            tracker.observe(packet_pid(256), 5, true, &changed),
            ContinuityOutcome::CounterCollision
        );
    }

    #[test]
    fn continuity_tracker_ignores_adaptation_only_packets() {
        let mut tracker = ContinuityTracker::default();
        let packet_256_0 = make_packet(256, 0);
        let packet_256_1 = make_packet(256, 1);
        assert_eq!(
            tracker.observe(packet_pid(256), 0, true, &packet_256_0),
            ContinuityOutcome::FirstPacket
        );
        assert_eq!(
            tracker.observe(packet_pid(256), 1, false, &packet_256_1),
            ContinuityOutcome::InOrder
        );
        assert_eq!(
            tracker.observe(packet_pid(256), 1, true, &packet_256_1),
            ContinuityOutcome::InOrder
        );

        let mut tracker = ContinuityTracker::default();
        let packet_300_7 = make_packet(300, 7);
        let packet_300_0 = make_packet(300, 0);
        assert_eq!(
            tracker.observe(packet_pid(300), 7, false, &packet_300_7),
            ContinuityOutcome::InOrder
        );
        assert_eq!(
            tracker.observe(packet_pid(300), 0, true, &packet_300_0),
            ContinuityOutcome::FirstPacket
        );

        let mut tracker = ContinuityTracker::default();
        let packet_301_0 = make_packet(301, 0);
        let packet_301_1 = make_packet(301, 1);
        assert_eq!(
            tracker.observe(packet_pid(301), 0, true, &packet_301_0),
            ContinuityOutcome::FirstPacket
        );
        assert_eq!(
            tracker.observe(packet_pid(301), 0, false, &packet_301_0),
            ContinuityOutcome::InOrder
        );
        assert_eq!(
            tracker.observe(packet_pid(301), 1, true, &packet_301_1),
            ContinuityOutcome::InOrder
        );
    }

    #[test]
    fn pes_assembler_collects_complete_packet() {
        let mut assembler = PesAssembler::default();
        let pes = [
            0x00, 0x00, 0x01, 0xbd, 0x00, 0x08, 0x84, 0x00, 0x00, b'A', b'R', b'I', b'B', 0x24,
            0x01, 0x02,
        ];
        let packets = assembler.push(packet_pid(0x0123), true, &pes);
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].stream_id, 0xbd);
        assert_eq!(packets[0].payload, b"ARIB$\x01\x02".to_vec());
    }
}

#[cfg(test)]
mod pes_flush_tests {
    use super::{packet_pid_for_test, PesAssembler, PesDropReason};

    #[test]
    fn length_zero_video_pes_completes_at_the_next_start_boundary() {
        let mut assembler = PesAssembler::default();
        let first = [
            0x00, 0x00, 0x01, 0xe0, 0x00, 0x00, 0x80, 0x00, 0x00, 0xaa, 0xbb,
        ];
        assert!(assembler
            .push(packet_pid_for_test(0x0100), true, &first)
            .is_empty());

        let packets = assembler.push(
            packet_pid_for_test(0x0100),
            true,
            &[0x00, 0x00, 0x01, 0xe1],
        );
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].stream_id, 0xe0);
        assert_eq!(packets[0].payload, vec![0xaa, 0xbb]);
    }

    #[test]
    fn length_zero_non_video_pes_is_malformed() {
        let mut assembler = PesAssembler::default();
        let private_stream = [
            0x00, 0x00, 0x01, 0xbd, 0x00, 0x00, 0x80, 0x00, 0x00, 0xaa,
        ];

        assert!(assembler
            .push(packet_pid_for_test(0x0100), true, &private_stream)
            .is_empty());
        assert_eq!(
            assembler.take_drop_diagnostic(),
            Some((PesDropReason::MalformedPes, 1))
        );
    }

    #[test]
    fn length_zero_pes_is_discarded_on_lifecycle_boundary() {
        let mut assembler = PesAssembler::default();
        let mut payload = vec![0x00, 0x00, 0x01, 0xe0, 0x00, 0x00, 0x80, 0x00, 0x00];
        payload.extend_from_slice(&[0xaa, 0xbb, 0xcc]);
        let out = assembler.push(packet_pid_for_test(0x0100), true, &payload);
        assert!(out.is_empty());
        assert!(assembler.flush().is_none());
        assert_eq!(
            assembler.take_drop_diagnostic(),
            Some((PesDropReason::FlushDiscard, 1))
        );
    }
}

#[cfg(test)]
mod pes_optional_header_contract_tests {
    use super::{packet_pid_for_test, parse_pes_header, PesAssembler};

    fn pes_with_optional(
        stream_id: u8,
        flags1: u8,
        flags2: u8,
        header: &[u8],
        payload: &[u8],
    ) -> Vec<u8> {
        let packet_length = (3 + header.len() + payload.len()) as u16;
        let mut bytes = vec![
            0x00,
            0x00,
            0x01,
            stream_id,
            (packet_length >> 8) as u8,
            packet_length as u8,
            flags1,
            flags2,
            header.len() as u8,
        ];
        bytes.extend_from_slice(header);
        bytes.extend_from_slice(payload);
        bytes
    }

    fn pes_without_optional(stream_id: u8, payload: &[u8]) -> Vec<u8> {
        let packet_length = payload.len() as u16;
        let mut bytes = vec![
            0x00,
            0x00,
            0x01,
            stream_id,
            (packet_length >> 8) as u8,
            packet_length as u8,
        ];
        bytes.extend_from_slice(payload);
        bytes
    }

    fn pts_only_field() -> [u8; 5] {
        [0x21, 0x00, 0x01, 0x00, 0x01]
    }
    fn pts_dts_pts_field() -> [u8; 5] {
        [0x31, 0x00, 0x01, 0x00, 0x01]
    }
    fn dts_field() -> [u8; 5] {
        [0x11, 0x00, 0x01, 0x00, 0x01]
    }

    #[test]
    fn c06_accepts_optional_header_marker_pattern_for_video_stream() {
        let bytes = pes_with_optional(0xe0, 0x80, 0x80, &pts_only_field(), &[0xaa]);
        assert!(parse_pes_header(&bytes).is_some());
    }

    #[test]
    fn c06_rejects_invalid_optional_header_marker_pattern() {
        let bytes = pes_with_optional(0xe0, 0x40, 0x80, &pts_only_field(), &[0xaa]);
        assert!(parse_pes_header(&bytes).is_none());
    }

    #[test]
    fn c06_rejects_forbidden_pts_dts_flags() {
        let bytes = pes_with_optional(0xe0, 0x80, 0x40, &[], &[0xaa]);
        assert!(parse_pes_header(&bytes).is_none());
    }

    #[test]
    fn c09_optional_header_absent_stream_does_not_require_marker_bytes() {
        for stream_id in [0xbe, 0xbf, 0xf0, 0xf1, 0xf2, 0xf8, 0xff] {
            let bytes = pes_without_optional(stream_id, &[0x00, 0x11, 0x22]);
            let summary = parse_pes_header(&bytes)
                .expect("optional-header-absent stream must parse without marker bytes");
            assert_eq!(summary.stream_id, stream_id);
            assert_eq!(summary.payload_offset, 6);
        }
    }

    #[test]
    fn c09_optional_header_present_stream_rejects_missing_optional_header() {
        let bytes = pes_without_optional(0xe0, &[0x00, 0x11, 0x22]);
        assert!(parse_pes_header(&bytes).is_none());
    }

    #[test]
    fn c07_accepts_pts_only_prefix_and_marker_bits() {
        let bytes = pes_with_optional(0xe0, 0x80, 0x80, &pts_only_field(), &[0xaa]);
        assert!(parse_pes_header(&bytes).unwrap().pts_90khz.is_some());
    }

    #[test]
    fn c07_rejects_wrong_pts_only_prefix() {
        let bytes = pes_with_optional(0xe0, 0x80, 0x80, &[0x31, 0x00, 0x01, 0x00, 0x01], &[0xaa]);
        assert!(parse_pes_header(&bytes).is_none());
    }

    #[test]
    fn c07_accepts_pts_dts_prefixes() {
        let mut header = Vec::new();
        header.extend_from_slice(&pts_dts_pts_field());
        header.extend_from_slice(&dts_field());
        let bytes = pes_with_optional(0xe0, 0x80, 0xc0, &header, &[0xaa]);
        let summary = parse_pes_header(&bytes).unwrap();
        assert!(summary.pts_90khz.is_some());
        assert!(summary.dts_90khz.is_some());
    }

    #[test]
    fn c07_rejects_wrong_dts_prefix() {
        let mut header = Vec::new();
        header.extend_from_slice(&pts_dts_pts_field());
        header.extend_from_slice(&[0x21, 0x00, 0x01, 0x00, 0x01]);
        let bytes = pes_with_optional(0xe0, 0x80, 0xc0, &header, &[0xaa]);
        assert!(parse_pes_header(&bytes).is_none());
    }

    #[test]
    fn c07_rejects_marker_bit_failures() {
        for bad in [
            [0x20, 0x00, 0x01, 0x00, 0x01],
            [0x21, 0x00, 0x00, 0x00, 0x01],
            [0x21, 0x00, 0x01, 0x00, 0x00],
        ] {
            let bytes = pes_with_optional(0xe0, 0x80, 0x80, &bad, &[0xaa]);
            assert!(parse_pes_header(&bytes).is_none());
        }
    }

    #[test]
    fn pes_extension_private_data_presence_is_parsed_from_the_header() {
        let mut header = vec![0x8e];
        header.extend_from_slice(&[0x5a; 16]);
        let bytes = pes_with_optional(0xe0, 0x80, 0x01, &header, &[0xaa]);
        let summary = parse_pes_header(&bytes).expect("valid PES private data extension");
        assert!(summary.is_pes_private_data);

        let packets = PesAssembler::default().push(packet_pid_for_test(0x0100), true, &bytes);
        assert_eq!(packets.len(), 1);
        assert!(packets[0].is_pes_private_data);
    }

    #[test]
    fn private_stream_id_does_not_imply_pes_private_data() {
        let bytes = pes_with_optional(0xbd, 0x80, 0x00, &[], &[0xaa]);
        let summary = parse_pes_header(&bytes).expect("valid private_stream_1 PES");
        assert!(!summary.is_pes_private_data);
    }

    #[test]
    fn truncated_pes_private_data_extension_is_rejected() {
        let mut header = vec![0x8e];
        header.extend_from_slice(&[0x5a; 15]);
        let bytes = pes_with_optional(0xe0, 0x80, 0x01, &header, &[0xaa]);
        assert!(parse_pes_header(&bytes).is_none());

        let mut assembler = PesAssembler::default();
        assert!(assembler
            .push(packet_pid_for_test(0x0100), true, &bytes)
            .is_empty());
    }

    #[test]
    fn c09_padding_stream_is_not_rejected_by_optional_marker_in_assembler() {
        let bytes = pes_without_optional(0xbe, &[0x00, 0x11, 0x22]);
        let mut assembler = PesAssembler::default();
        let packets = assembler.push(packet_pid_for_test(0x0100), true, &bytes);
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].stream_id, 0xbe);
    }
}

#[cfg(test)]
mod pes_boundary_tests {
    use super::{packet_pid_for_test, PesAssembler, PesDropReason};

    #[test]
    fn unbounded_pes_is_discarded_on_flush_boundary() {
        let mut assembler = PesAssembler::default();
        let mut pes = vec![0x00, 0x00, 0x01, 0xe0, 0x00, 0x00, 0x80, 0x00, 0x00];
        pes.extend_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
        assert!(assembler
            .push(packet_pid_for_test(0x0100), true, &pes)
            .is_empty());
        assert!(assembler.flush().is_none());
        assert_eq!(
            assembler.take_drop_diagnostic(),
            Some((PesDropReason::FlushDiscard, 1))
        );
    }

    #[test]
    fn continuation_without_start_is_dropped_until_next_pusi() {
        let mut assembler = PesAssembler::default();
        assert!(assembler
            .push(packet_pid_for_test(0x0100), false, &[0xaa, 0xbb])
            .is_empty());
        assert_eq!(assembler.take_drop_diagnostic(), None);
        let pes = vec![0x00, 0x00, 0x01, 0xe0, 0x00, 0x04, 0x80, 0x00, 0x00, 0xde];
        let packets = assembler.push(packet_pid_for_test(0x0100), true, &pes);
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].stream_id, 0xe0);
    }
}

#[cfg(test)]
mod pes_oversized_tests {
    use super::{packet_pid_for_test, PesAssembler, PesDropReason, MAX_PES_BUFFER_BYTES};

    #[test]
    fn unbounded_pes_over_limit_is_dropped_and_next_pusi_recovers() {
        let mut assembler = PesAssembler::default();
        let mut oversized = vec![0x00, 0x00, 0x01, 0xe0, 0x00, 0x00, 0x80, 0x00, 0x00];
        oversized.resize(MAX_PES_BUFFER_BYTES + 1, 0xaa);
        assert!(assembler
            .push(packet_pid_for_test(0x0100), true, &oversized)
            .is_empty());
        assert_eq!(assembler.overflow_drop_count(), 1);
        assert_eq!(
            assembler.take_drop_diagnostic(),
            Some((PesDropReason::OversizedPes, 1))
        );

        let pes = vec![0x00, 0x00, 0x01, 0xe0, 0x00, 0x04, 0x80, 0x00, 0x00, 0xde];
        let packets = assembler.push(packet_pid_for_test(0x0100), true, &pes);
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].stream_id, 0xe0);
    }
}

#[cfg(test)]
mod pes_split_and_recovery_tests {
    use super::{packet_pid_for_test, PesAssembler, PesDropReason};

    fn bounded_video_pes(payload: &[u8]) -> Vec<u8> {
        let packet_length = (3 + payload.len()) as u16;
        let mut bytes = vec![
            0x00,
            0x00,
            0x01,
            0xe0,
            (packet_length >> 8) as u8,
            packet_length as u8,
            0x80,
            0x00,
            0x00,
        ];
        bytes.extend_from_slice(payload);
        bytes
    }

    #[test]
    fn pes_header_split_across_ts_payloads_completes_bounded_packet() {
        let mut assembler = PesAssembler::default();
        let pes = bounded_video_pes(&[0xaa, 0xbb, 0xcc]);
        assert!(assembler
            .push(packet_pid_for_test(0x0100), true, &pes[..5])
            .is_empty());
        assert!(assembler
            .push(packet_pid_for_test(0x0100), false, &pes[5..9])
            .is_empty());
        let out = assembler.push(packet_pid_for_test(0x0100), false, &pes[9..]);

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].payload, vec![0xaa, 0xbb, 0xcc]);
    }

    #[test]
    fn bounded_pes_excludes_trailing_ts_payload_stuffing() {
        let mut assembler = PesAssembler::default();
        let mut pes = bounded_video_pes(&[0xaa]);
        pes.extend_from_slice(&[0xff; 32]);

        let out = assembler.push(packet_pid_for_test(0x0100), true, &pes);

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].payload, vec![0xaa]);
        assert_eq!(out[0].raw_bytes, bounded_video_pes(&[0xaa]));
    }

    #[test]
    fn malformed_pes_resets_and_next_pusi_recovers() {
        let mut assembler = PesAssembler::default();
        let malformed = [0x00, 0x00, 0x02, 0xe0, 0x00, 0x04];
        assert!(assembler
            .push(packet_pid_for_test(0x0100), true, &malformed)
            .is_empty());
        assert_eq!(
            assembler.take_drop_diagnostic(),
            Some((PesDropReason::MalformedPes, 1))
        );

        let out = assembler.push(
            packet_pid_for_test(0x0100),
            true,
            &bounded_video_pes(&[0x55]),
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].payload, vec![0x55]);
    }
}

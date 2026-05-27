use std::collections::BTreeMap;

const MAX_PES_BUFFER_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContinuityOutcome {
    FirstPacket,
    InOrder,
    Duplicate,
    Discontinuity,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ContinuityState {
    last_counter: Option<u8>,
}

#[derive(Clone, Debug, Default)]
pub struct ContinuityTracker {
    states: BTreeMap<u16, ContinuityState>,
}

impl ContinuityTracker {
    pub fn observe(
        &mut self,
        pid: u16,
        continuity_counter: u8,
        has_payload: bool,
    ) -> ContinuityOutcome {
        if !has_payload {
            return ContinuityOutcome::InOrder;
        }
        let state = self.states.entry(pid).or_default();
        let Some(last_counter) = state.last_counter else {
            state.last_counter = Some(continuity_counter);
            return ContinuityOutcome::FirstPacket;
        };
        if continuity_counter == last_counter {
            return ContinuityOutcome::Duplicate;
        }
        let expected = (last_counter + 1) & 0x0f;
        state.last_counter = Some(continuity_counter);
        if continuity_counter == expected {
            ContinuityOutcome::InOrder
        } else {
            ContinuityOutcome::Discontinuity
        }
    }

    pub fn reset_pid(&mut self, pid: u16) {
        self.states.remove(&pid);
    }
}

// r50dz53/G1-20: soft_demux の旧 resync buffer は削除した。
// TS byte stream の分割・resync は common の TsPacketCompletionBuffer だけを正とする。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PesPacket {
    pub pid: u16,
    pub stream_id: u8,
    pub pts_90khz: Option<u64>,
    pub dts_90khz: Option<u64>,
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

pub fn parse_pes_header_summary(bytes: &[u8]) -> Option<PesHeaderSummary> {
    if bytes.len() < 6 || &bytes[..3] != [0x00, 0x00, 0x01] {
        return None;
    }
    let stream_id = bytes[3];
    let packet_length = u16::from_be_bytes([bytes[4], bytes[5]]) as usize;
    if !pes_stream_has_optional_header(stream_id) {
        let expected_len = if packet_length == 0 { None } else { Some(6 + packet_length) };
        return Some(PesHeaderSummary {
            stream_id,
            payload_offset: 6,
            pts_90khz: None,
            dts_90khz: None,
            data_alignment_indicator: false,
            expected_len,
        });
    }
    if bytes.len() < 9 {
        return None;
    }
    let flags1 = bytes[6];
    let flags2 = bytes[7];
    let header_data_len = bytes[8] as usize;
    let payload_offset = 9 + header_data_len;
    if bytes.len() < payload_offset || (flags1 & 0xc0) != 0x80 {
        return None;
    }
    if packet_length != 0 && packet_length < 3 + header_data_len {
        return None;
    }
    let data_alignment_indicator = (flags1 & 0x04) != 0;
    let pts_dts_flags = (flags2 >> 6) & 0x03;
    if pts_dts_flags == 0b01 {
        return None;
    }
    let pts_90khz = match pts_dts_flags {
        0b10 => {
            if header_data_len < 5 { return None; }
            Some(pts_dts_field_value(bytes.get(9..14)?, 0b0010)?)
        }
        0b11 => {
            if header_data_len < 10 { return None; }
            Some(pts_dts_field_value(bytes.get(9..14)?, 0b0011)?)
        }
        _ => None,
    };
    let dts_90khz = match pts_dts_flags {
        0b11 => Some(pts_dts_field_value(bytes.get(14..19)?, 0b0001)?),
        _ => None,
    };
    let expected_len = if packet_length == 0 {
        None
    } else {
        Some(6 + packet_length)
    };
    Some(PesHeaderSummary {
        stream_id,
        payload_offset,
        pts_90khz,
        dts_90khz,
        data_alignment_indicator,
        expected_len,
    })
}

#[cfg(test)]
fn parse_pes_header(bytes: &[u8]) -> Option<PesHeaderSummary> {
    parse_pes_header_summary(bytes)
}

#[derive(Clone, Debug, Default)]
pub struct PesAssembler {
    pid: Option<u16>,
    buf: Vec<u8>,
    expected_len: Option<usize>,
    overflow_drop_count: u64,
    overflow_generation: u64,
    last_drop_reason: Option<&'static str>,
}

impl PesAssembler {
    pub fn push(&mut self, pid: u16, payload_unit_start: bool, payload: &[u8]) -> Vec<PesPacket> {
        let mut out = Vec::new();
        if payload_unit_start {
            if let Some(packet) = self.take_completed() {
                out.push(packet);
            }
            self.pid = Some(pid);
            self.buf.clear();
            self.expected_len = None;
        }
        if self.pid != Some(pid) {
            self.pid = Some(pid);
            self.buf.clear();
            self.expected_len = None;
        }
        self.buf.extend_from_slice(payload);
        if self.buf.len() > MAX_PES_BUFFER_BYTES {
            self.reset_after_overflow_drop();
            return out;
        }
        if self.expected_len.is_none() {
            if let Some(summary) = parse_pes_header_summary(&self.buf) {
                self.expected_len = summary.expected_len;
            }
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

    pub fn flush(&mut self) -> Option<PesPacket> {
        // r50dz53/G2-06: PES_packet_length == 0 は unbounded PES として、
        // stop/flush/close などの lifecycle boundary で現在の buffer を finalize する。
        self.take_completed()
    }

    fn reset_after_overflow_drop(&mut self) {
        self.buf.clear();
        self.expected_len = None;
        self.overflow_drop_count = self.overflow_drop_count.saturating_add(1);
        self.overflow_generation = self.overflow_generation.saturating_add(1);
        self.last_drop_reason = Some("pes_assembler_buffer_overflow_drop");
    }

    pub fn take_drop_diagnostic(&mut self) -> Option<(&'static str, u64)> {
        self.last_drop_reason
            .take()
            .map(|reason| (reason, self.overflow_generation))
    }

    pub fn overflow_drop_count(&self) -> u64 {
        self.overflow_drop_count
    }


    fn take_completed(&mut self) -> Option<PesPacket> {
        let pid = self.pid?;
        let summary = parse_pes_header_summary(&self.buf)?;
        if let Some(expected_len) = summary.expected_len {
            if self.buf.len() < expected_len {
                return None;
            }
        }
        let payload = self.buf[summary.payload_offset..].to_vec();
        let raw_bytes = std::mem::take(&mut self.buf);
        self.expected_len = None;
        Some(PesPacket {
            pid,
            stream_id: summary.stream_id,
            pts_90khz: summary.pts_90khz,
            dts_90khz: summary.dts_90khz,
            data_alignment_indicator: summary.data_alignment_indicator,
            raw_bytes,
            payload,
        })
    }
}

#[cfg(test)]
mod tests {

    use super::{ContinuityOutcome, ContinuityTracker, PesAssembler};
    use maleicacid_tuner_hal_common::TsPacketCompletionBuffer;
    use maleicacid_tuner_hal_common::TS_PACKET_SIZE;

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
        assert!(resync.push(&input).packets.is_empty());
        let tail = resync.push(&make_packet(0x30, 2));
        assert_eq!(tail.packets.len(), 3);
        assert_eq!(tail.malformed_bytes, 3);
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
        assert_eq!(
            tracker.observe(256, 0, true),
            ContinuityOutcome::FirstPacket
        );
        assert_eq!(tracker.observe(256, 1, true), ContinuityOutcome::InOrder);
        assert_eq!(tracker.observe(256, 1, true), ContinuityOutcome::Duplicate);
        assert_eq!(
            tracker.observe(256, 3, true),
            ContinuityOutcome::Discontinuity
        );
    }

    #[test]
    fn continuity_tracker_ignores_adaptation_only_packets() {
        let mut tracker = ContinuityTracker::default();
        assert_eq!(
            tracker.observe(256, 0, true),
            ContinuityOutcome::FirstPacket
        );
        assert_eq!(tracker.observe(256, 1, false), ContinuityOutcome::InOrder);
        assert_eq!(tracker.observe(256, 1, true), ContinuityOutcome::InOrder);

        let mut tracker = ContinuityTracker::default();
        assert_eq!(tracker.observe(300, 7, false), ContinuityOutcome::InOrder);
        assert_eq!(
            tracker.observe(300, 0, true),
            ContinuityOutcome::FirstPacket
        );

        let mut tracker = ContinuityTracker::default();
        assert_eq!(
            tracker.observe(301, 0, true),
            ContinuityOutcome::FirstPacket
        );
        assert_eq!(tracker.observe(301, 0, false), ContinuityOutcome::InOrder);
        assert_eq!(tracker.observe(301, 1, true), ContinuityOutcome::InOrder);
    }

    #[test]
    fn pes_assembler_collects_complete_packet() {
        let mut assembler = PesAssembler::default();
        let pes = [
            0x00, 0x00, 0x01, 0xbd, 0x00, 0x08, 0x84, 0x00, 0x00, b'A', b'R', b'I', b'B', 0x24,
            0x01, 0x02,
        ];
        let packets = assembler.push(0x0123, true, &pes);
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].stream_id, 0xbd);
        assert_eq!(packets[0].payload, b"ARIB$\x01\x02".to_vec());
    }
}

#[cfg(test)]
mod pes_flush_tests {
    use super::PesAssembler;

    #[test]
    fn length_zero_pes_flushes_on_lifecycle_boundary() {
        let mut assembler = PesAssembler::default();
        let mut payload = vec![0x00, 0x00, 0x01, 0xe0, 0x00, 0x00, 0x80, 0x00, 0x00];
        payload.extend_from_slice(&[0xaa, 0xbb, 0xcc]);
        let out = assembler.push(0x100, true, &payload);
        assert!(out.is_empty());
        let flushed = assembler.flush().expect("length-zero PES should flush");
        assert_eq!(flushed.stream_id, 0xe0);
        assert_eq!(flushed.payload, vec![0xaa, 0xbb, 0xcc]);
    }
}

#[cfg(test)]
mod pes_optional_header_contract_tests {
    use super::{parse_pes_header, PesAssembler};

    fn pes_with_optional(stream_id: u8, flags1: u8, flags2: u8, header: &[u8], payload: &[u8]) -> Vec<u8> {
        let packet_length = (3 + header.len() + payload.len()) as u16;
        let mut bytes = vec![0x00, 0x00, 0x01, stream_id, (packet_length >> 8) as u8, packet_length as u8, flags1, flags2, header.len() as u8];
        bytes.extend_from_slice(header);
        bytes.extend_from_slice(payload);
        bytes
    }

    fn pes_without_optional(stream_id: u8, payload: &[u8]) -> Vec<u8> {
        let packet_length = payload.len() as u16;
        let mut bytes = vec![0x00, 0x00, 0x01, stream_id, (packet_length >> 8) as u8, packet_length as u8];
        bytes.extend_from_slice(payload);
        bytes
    }

    fn pts_only_field() -> [u8; 5] { [0x21, 0x00, 0x01, 0x00, 0x01] }
    fn pts_dts_pts_field() -> [u8; 5] { [0x31, 0x00, 0x01, 0x00, 0x01] }
    fn dts_field() -> [u8; 5] { [0x11, 0x00, 0x01, 0x00, 0x01] }

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
            let summary = parse_pes_header(&bytes).expect("optional-header-absent stream must parse without marker bytes");
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
        for bad in [[0x20, 0x00, 0x01, 0x00, 0x01], [0x21, 0x00, 0x00, 0x00, 0x01], [0x21, 0x00, 0x01, 0x00, 0x00]] {
            let bytes = pes_with_optional(0xe0, 0x80, 0x80, &bad, &[0xaa]);
            assert!(parse_pes_header(&bytes).is_none());
        }
    }

    #[test]
    fn c09_padding_stream_is_not_rejected_by_optional_marker_in_assembler() {
        let bytes = pes_without_optional(0xbe, &[0x00, 0x11, 0x22]);
        let mut assembler = PesAssembler::default();
        let packets = assembler.push(0x0100, true, &bytes);
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].stream_id, 0xbe);
    }
}


#[cfg(test)]
mod r50dz52_g2_06_tests {
    use super::PesAssembler;

    #[test]
    fn unbounded_pes_finalizes_on_flush_boundary() {
        let mut assembler = PesAssembler::default();
        let mut pes = vec![0x00, 0x00, 0x01, 0xe0, 0x00, 0x00, 0x80, 0x00, 0x00];
        pes.extend_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
        assert!(assembler.push(0x0100, true, &pes).is_empty());
        let flushed = assembler.flush();
        assert!(matches!(flushed.as_ref().map(|packet| packet.stream_id), Some(0xe0)));
        assert_eq!(flushed.map(|packet| packet.payload), Some(vec![0xde, 0xad, 0xbe, 0xef]));
    }
}

#[cfg(test)]
mod r50dz52_g2_07_tests {
    use super::{PesAssembler, MAX_PES_BUFFER_BYTES};

    #[test]
    fn pes_assembler_overflow_records_drop_diagnostic_and_resets_state() {
        let mut assembler = PesAssembler::default();
        let mut oversized = vec![0x00, 0x00, 0x01, 0xe0, 0x00, 0x00, 0x80, 0x00, 0x00];
        oversized.resize(MAX_PES_BUFFER_BYTES + 1, 0xaa);
        assert!(assembler.push(0x0100, true, &oversized).is_empty());
        assert_eq!(assembler.overflow_drop_count(), 1);
        assert_eq!(assembler.take_drop_diagnostic(), Some(("pes_assembler_buffer_overflow_drop", 1)));
        assert!(assembler.flush().is_none());
    }
}

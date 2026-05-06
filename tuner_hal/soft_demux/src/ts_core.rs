use maleicacid_tuner_hal_common::TS_PACKET_SIZE;
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

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TsPacketResyncBuffer {
    buf: Vec<u8>,
}

impl TsPacketResyncBuffer {
    pub fn push(&mut self, data: &[u8]) -> Vec<[u8; TS_PACKET_SIZE]> {
        if self.buf.is_empty() && Self::is_aligned_ts_stream(data) {
            return data
                .chunks_exact(TS_PACKET_SIZE)
                .map(|chunk| {
                    let mut packet = [0u8; TS_PACKET_SIZE];
                    packet.copy_from_slice(chunk);
                    packet
                })
                .collect();
        }

        self.buf.extend_from_slice(data);
        let mut out = Vec::new();
        loop {
            if self.buf.len() < TS_PACKET_SIZE {
                break;
            }
            let Some(offset) = self.find_sync_offset() else {
                let keep = self.buf.len().min(TS_PACKET_SIZE - 1);
                let drain = self.buf.len().saturating_sub(keep);
                self.buf.drain(..drain);
                break;
            };
            if offset > 0 {
                self.buf.drain(..offset);
                if self.buf.len() < TS_PACKET_SIZE {
                    break;
                }
            }
            if !self.has_next_sync() {
                break;
            }
            let mut packet = [0u8; TS_PACKET_SIZE];
            packet.copy_from_slice(&self.buf[..TS_PACKET_SIZE]);
            self.buf.drain(..TS_PACKET_SIZE);
            out.push(packet);
        }
        out
    }

    fn find_sync_offset(&self) -> Option<usize> {
        self.buf.iter().position(|byte| *byte == 0x47)
    }

    fn has_next_sync(&self) -> bool {
        self.buf
            .get(TS_PACKET_SIZE)
            .map_or(false, |byte| *byte == 0x47)
    }

    fn is_aligned_ts_stream(data: &[u8]) -> bool {
        !data.is_empty()
            && data.len() % TS_PACKET_SIZE == 0
            && data
                .chunks_exact(TS_PACKET_SIZE)
                .all(|packet| packet[0] == 0x47)
    }
}

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
struct PesHeaderSummary {
    stream_id: u8,
    payload_offset: usize,
    pts_90khz: Option<u64>,
    dts_90khz: Option<u64>,
    data_alignment_indicator: bool,
    expected_len: Option<usize>,
}

fn parse_pts(field: &[u8]) -> Option<u64> {
    if field.len() < 5 {
        return None;
    }
    let pts = (((field[0] >> 1) as u64) & 0x07) << 30
        | ((field[1] as u64) << 22)
        | (((field[2] >> 1) as u64) << 15)
        | ((field[3] as u64) << 7)
        | ((field[4] >> 1) as u64);
    Some(pts)
}

fn parse_pes_header(bytes: &[u8]) -> Option<PesHeaderSummary> {
    if bytes.len() < 9 || &bytes[..3] != [0x00, 0x00, 0x01] {
        return None;
    }
    let stream_id = bytes[3];
    let packet_length = u16::from_be_bytes([bytes[4], bytes[5]]) as usize;
    let flags1 = bytes[6];
    let flags2 = bytes[7];
    let header_data_len = bytes[8] as usize;
    let payload_offset = 9 + header_data_len;
    if bytes.len() < payload_offset {
        return None;
    }
    let data_alignment_indicator = (flags1 & 0x04) != 0;
    let pts_dts_flags = (flags2 >> 6) & 0x03;
    let pts_90khz = match pts_dts_flags {
        0b10 | 0b11 => parse_pts(bytes.get(9..14)?),
        _ => None,
    };
    let dts_90khz = match pts_dts_flags {
        0b11 => parse_pts(bytes.get(14..19)?),
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

#[derive(Clone, Debug, Default)]
pub struct PesAssembler {
    pid: Option<u16>,
    buf: Vec<u8>,
    expected_len: Option<usize>,
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
            self.buf.clear();
            self.expected_len = None;
            return out;
        }
        if self.expected_len.is_none() {
            if let Some(summary) = parse_pes_header(&self.buf) {
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
        self.take_completed()
    }

    fn take_completed(&mut self) -> Option<PesPacket> {
        let pid = self.pid?;
        let summary = parse_pes_header(&self.buf)?;
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

    use super::{ContinuityOutcome, ContinuityTracker, PesAssembler, TsPacketResyncBuffer};
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
    fn resync_buffer_discards_garbage_prefix() {
        let mut resync = TsPacketResyncBuffer::default();
        let mut input = vec![0x00, 0x11, 0x22];
        input.extend_from_slice(&make_packet(0x30, 0));
        input.extend_from_slice(&make_packet(0x30, 1));
        let packets = resync.push(&input);
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0][0], 0x47);
        let tail = resync.push(&make_packet(0x30, 2));
        assert_eq!(tail.len(), 1);
    }

    #[test]
    fn resync_buffer_accepts_clean_aligned_packets_without_lookahead() {
        let mut resync = TsPacketResyncBuffer::default();
        let packets = resync.push(&make_packet(0x31, 0));
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0][0], 0x47);
        assert_eq!(
            ((packets[0][1] as u16 & 0x1f) << 8) | packets[0][2] as u16,
            0x31
        );
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

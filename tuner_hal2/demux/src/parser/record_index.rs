//! record index event生成を統一する骨格。
//!
//! scrambling change、PES timestamp、H.264/H.265/VVC start code全走査をここへ集約する。

use crate::ts_core::parse_pes_header_summary;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum CodecKind {
    H264,
    H265,
    Vvc,
}

pub trait CodecStartCodeScanner {
    fn codec(&self) -> CodecKind;
    fn scan_start_codes(&self, payload: &[u8]) -> usize;
}

#[derive(Debug, Default)]
pub struct RecordIndexParser {
    processed_packets: u64,
}

impl RecordIndexParser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn processed_packets(&self) -> u64 {
        self.processed_packets
    }

    pub fn push_ts_packet(
        &mut self,
        packet: &[u8],
        cumulative_bytes: u64,
        configured_ts_index_mask: i32,
        sc_index_type: i32,
        configured_sc_index_mask_bits: i32,
        record_state: &mut RecordEventState,
    ) -> Option<TsRecordEventData> {
        self.processed_packets = self.processed_packets.saturating_add(1);
        build_ts_record_event_data(
            packet,
            cumulative_bytes,
            configured_ts_index_mask,
            sc_index_type,
            configured_sc_index_mask_bits,
            record_state,
        )
    }

    pub fn build_event(
        &mut self,
        packet: &[u8],
        cumulative_bytes: u64,
        configured_ts_index_mask: i32,
        sc_index_type: i32,
        configured_sc_index_mask_bits: i32,
        record_state: &mut RecordEventState,
    ) -> Option<TsRecordEventData> {
        self.push_ts_packet(
            packet,
            cumulative_bytes,
            configured_ts_index_mask,
            sc_index_type,
            configured_sc_index_mask_bits,
            record_state,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordStartCodeInfo {
    pub mask: i32,
    pub first_mb_in_slice: i32,
}

pub const INVALID_FIRST_MB_IN_SLICE: i32 = -1;
pub const RECORD_INDEX_PTS_ABSENT: i64 = -1;
pub const SC_INDEX_MASK_ABSENT: i32 = 0;
pub const AVC_SC_I_SLICE: i32 = 1 << 0;
pub const AVC_SC_P_SLICE: i32 = 1 << 1;
pub const AVC_SC_B_SLICE: i32 = 1 << 2;
pub const AVC_SC_SI_SLICE: i32 = 1 << 3;
pub const AVC_SC_SP_SLICE: i32 = 1 << 4;
pub const HEVC_SC_SPS: i32 = 1 << 0;
pub const HEVC_SC_AUD: i32 = 1 << 1;
pub const HEVC_SC_BLA_W_LP: i32 = 1 << 2;
pub const HEVC_SC_BLA_W_RADL: i32 = 1 << 3;
pub const HEVC_SC_BLA_N_LP: i32 = 1 << 4;
pub const HEVC_SC_IDR_W_RADL: i32 = 1 << 5;
pub const HEVC_SC_IDR_N_LP: i32 = 1 << 6;
pub const HEVC_SC_TRAIL_CRA: i32 = 1 << 7;
pub const VVC_SC_IDR_W_RADL: i32 = 1 << 0;
pub const VVC_SC_IDR_N_LP: i32 = 1 << 1;
pub const VVC_SC_CRA: i32 = 1 << 2;
pub const VVC_SC_GDR: i32 = 1 << 3;
pub const VVC_SC_VPS: i32 = 1 << 4;
pub const VVC_SC_SPS: i32 = 1 << 5;
pub const VVC_SC_AUD: i32 = 1 << 6;
pub const RECORD_SC_TYPE_NONE: i32 = 0;
pub const RECORD_SC_TYPE_SC: i32 = 1;
pub const RECORD_SC_TYPE_SC_HEVC: i32 = 2;
pub const RECORD_SC_TYPE_SC_AVC: i32 = 3;
pub const RECORD_SC_TYPE_SC_VVC: i32 = 4;

pub const DEMUX_TS_INDEX_FIRST_PACKET: i32 = 1 << 0;
pub const DEMUX_TS_INDEX_PAYLOAD_UNIT_START: i32 = 1 << 1;
pub const DEMUX_TS_INDEX_CHANGE_TO_NOT_SCRAMBLED: i32 = 1 << 2;
pub const DEMUX_TS_INDEX_CHANGE_TO_EVEN_SCRAMBLED: i32 = 1 << 3;
pub const DEMUX_TS_INDEX_CHANGE_TO_ODD_SCRAMBLED: i32 = 1 << 4;
pub const DEMUX_TS_INDEX_DISCONTINUITY: i32 = 1 << 5;
pub const DEMUX_TS_INDEX_RANDOM_ACCESS: i32 = 1 << 6;
pub const DEMUX_TS_INDEX_PRIORITY: i32 = 1 << 7;
pub const DEMUX_TS_INDEX_PCR: i32 = 1 << 8;
pub const DEMUX_TS_INDEX_OPCR: i32 = 1 << 9;
pub const DEMUX_TS_INDEX_SPLICING_POINT: i32 = 1 << 10;
pub const DEMUX_TS_INDEX_PRIVATE_DATA: i32 = 1 << 11;
pub const DEMUX_TS_INDEX_ADAPTATION_EXTENSION: i32 = 1 << 12;

#[derive(Clone, Debug, Default)]
pub struct RecordEventState {
    last_transport_scrambling_control: Option<u8>,
    // scanner/parserはTS payload境界をまたいで状態を保持する。
    sc_prefix_carry: Vec<u8>,
    pes_header_carry: Vec<u8>,
}

impl RecordEventState {
    pub fn reset_payload_state(&mut self) {
        self.sc_prefix_carry.clear();
        self.pes_header_carry.clear();
    }

    fn payload_with_sc_carry(&mut self, payload: &[u8]) -> Vec<u8> {
        let mut merged = Vec::with_capacity(self.sc_prefix_carry.len() + payload.len());
        merged.extend_from_slice(&self.sc_prefix_carry);
        merged.extend_from_slice(payload);
        let keep = payload.len().min(3);
        self.sc_prefix_carry.clear();
        if keep > 0 {
            self.sc_prefix_carry
                .extend_from_slice(&payload[payload.len() - keep..]);
        }
        merged
    }

    fn observe_pts(&mut self, payload: &[u8], payload_unit_start: bool) -> Option<i64> {
        if payload_unit_start || payload.starts_with(&[0x00, 0x00, 0x01]) {
            self.pes_header_carry.clear();
            if payload.starts_with(&[0x00, 0x00, 0x01]) {
                self.pes_header_carry
                    .extend_from_slice(&payload[..payload.len().min(19)]);
                if let Some(pts) = record_packet_pts(payload) {
                    self.pes_header_carry.clear();
                    return Some(pts);
                }
                return None;
            }
            if is_pes_start_prefix_fragment(payload) {
                self.pes_header_carry.extend_from_slice(payload);
            }
            return None;
        }
        if self.pes_header_carry.is_empty() {
            return None;
        }
        self.pes_header_carry
            .extend_from_slice(&payload[..payload.len().min(19)]);
        if !starts_with_complete_or_partial_pes_prefix(&self.pes_header_carry) {
            self.pes_header_carry.clear();
            return None;
        }
        if self.pes_header_carry.len() > 32 {
            self.pes_header_carry.truncate(32);
        }
        let pts = record_packet_pts(&self.pes_header_carry);
        if pts.is_some() || self.pes_header_carry.len() >= 19 {
            self.pes_header_carry.clear();
        }
        pts
    }
}

fn is_pes_start_prefix_fragment(payload: &[u8]) -> bool {
    matches!(payload, [0x00] | [0x00, 0x00])
}

fn starts_with_complete_or_partial_pes_prefix(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0x00, 0x00, 0x01]) || is_pes_start_prefix_fragment(bytes)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TsRecordEventData {
    pub pid: i32,
    pub ts_index_mask: i32,
    pub sc_index_type: i32,
    pub sc_index_mask_bits: i32,
    pub byte_number: i64,
    pub pts: i64,
    pub first_mb_in_slice: i32,
}

pub fn supported_record_ts_index_mask() -> i32 {
    DEMUX_TS_INDEX_FIRST_PACKET
        | DEMUX_TS_INDEX_PAYLOAD_UNIT_START
        | DEMUX_TS_INDEX_CHANGE_TO_NOT_SCRAMBLED
        | DEMUX_TS_INDEX_CHANGE_TO_EVEN_SCRAMBLED
        | DEMUX_TS_INDEX_CHANGE_TO_ODD_SCRAMBLED
        | DEMUX_TS_INDEX_DISCONTINUITY
        | DEMUX_TS_INDEX_RANDOM_ACCESS
        | DEMUX_TS_INDEX_PRIORITY
        | DEMUX_TS_INDEX_PCR
        | DEMUX_TS_INDEX_OPCR
        | DEMUX_TS_INDEX_SPLICING_POINT
        | DEMUX_TS_INDEX_PRIVATE_DATA
        | DEMUX_TS_INDEX_ADAPTATION_EXTENSION
}

pub fn supported_record_sc_index_mask(sc_index_type: i32) -> i32 {
    match sc_index_type {
        RECORD_SC_TYPE_NONE => 0,
        RECORD_SC_TYPE_SC => (1 << 0) | (1 << 1) | (1 << 2) | (1 << 3),
        RECORD_SC_TYPE_SC_AVC => {
            AVC_SC_I_SLICE | AVC_SC_P_SLICE | AVC_SC_B_SLICE | AVC_SC_SI_SLICE | AVC_SC_SP_SLICE
        }
        RECORD_SC_TYPE_SC_HEVC => {
            HEVC_SC_SPS
                | HEVC_SC_AUD
                | HEVC_SC_BLA_W_LP
                | HEVC_SC_BLA_W_RADL
                | HEVC_SC_BLA_N_LP
                | HEVC_SC_IDR_W_RADL
                | HEVC_SC_IDR_N_LP
                | HEVC_SC_TRAIL_CRA
        }
        RECORD_SC_TYPE_SC_VVC => {
            VVC_SC_IDR_W_RADL
                | VVC_SC_IDR_N_LP
                | VVC_SC_CRA
                | VVC_SC_GDR
                | VVC_SC_VPS
                | VVC_SC_SPS
                | VVC_SC_AUD
        }
        _ => 0,
    }
}

fn build_ts_record_event_data(
    packet: &[u8],
    cumulative_bytes: u64,
    configured_ts_index_mask: i32,
    sc_index_type: i32,
    configured_sc_index_mask_bits: i32,
    record_state: &mut RecordEventState,
) -> Option<TsRecordEventData> {
    let packet_view = crate::packet_pipeline::TsPacketView::validate(packet).ok()?;
    if packet_view.transport_error_indicator {
        return None;
    }
    let packet_payload = packet_view.payload.unwrap_or(&[]);
    let mut observed_ts_index = 0i32;
    if cumulative_bytes == 0 {
        observed_ts_index |= DEMUX_TS_INDEX_FIRST_PACKET;
    }
    if packet_view.payload_unit_start {
        observed_ts_index |= DEMUX_TS_INDEX_PAYLOAD_UNIT_START;
    }
    if packet_view.priority {
        observed_ts_index |= DEMUX_TS_INDEX_PRIORITY;
    }
    if packet_view.discontinuity_indicator {
        observed_ts_index |= DEMUX_TS_INDEX_DISCONTINUITY;
        record_state.reset_payload_state();
    }
    if packet_view.random_access_indicator {
        observed_ts_index |= DEMUX_TS_INDEX_RANDOM_ACCESS;
    }
    if packet_view.pcr_flag {
        observed_ts_index |= DEMUX_TS_INDEX_PCR;
    }
    if packet_view.opcr_flag {
        observed_ts_index |= DEMUX_TS_INDEX_OPCR;
    }
    if packet_view.splicing_point_flag {
        observed_ts_index |= DEMUX_TS_INDEX_SPLICING_POINT;
    }
    if packet_view.private_data_flag {
        observed_ts_index |= DEMUX_TS_INDEX_PRIVATE_DATA;
    }
    if packet_view.adaptation_extension_flag {
        observed_ts_index |= DEMUX_TS_INDEX_ADAPTATION_EXTENSION;
    }
    match record_state
        .last_transport_scrambling_control
        .replace(packet_view.scrambling_control)
    {
        Some(previous) if previous != packet_view.scrambling_control => {
            observed_ts_index |= match packet_view.scrambling_control {
                0 => DEMUX_TS_INDEX_CHANGE_TO_NOT_SCRAMBLED,
                2 => DEMUX_TS_INDEX_CHANGE_TO_EVEN_SCRAMBLED,
                3 => DEMUX_TS_INDEX_CHANGE_TO_ODD_SCRAMBLED,
                _ => 0,
            };
        }
        None if packet_view.scrambling_control != 0 => {
            observed_ts_index |= match packet_view.scrambling_control {
                2 => DEMUX_TS_INDEX_CHANGE_TO_EVEN_SCRAMBLED,
                3 => DEMUX_TS_INDEX_CHANGE_TO_ODD_SCRAMBLED,
                _ => 0,
            };
        }
        _ => {}
    }
    let (pts, sc_info) = if packet_view.scrambling_control == 0 {
        let pts = record_state
            .observe_pts(packet_payload, packet_view.payload_unit_start)
            .unwrap_or(RECORD_INDEX_PTS_ABSENT);
        let sc_payload = record_state.payload_with_sc_carry(packet_payload);
        (
            pts,
            record_sc_info(&sc_payload, sc_index_type, configured_sc_index_mask_bits),
        )
    } else {
        record_state.reset_payload_state();
        (RECORD_INDEX_PTS_ABSENT, None)
    };
    let first_mb_in_slice = sc_info
        .map(|info| info.first_mb_in_slice)
        .unwrap_or(INVALID_FIRST_MB_IN_SLICE);
    let ts_index_mask = observed_ts_index & configured_ts_index_mask;
    let sc_index_mask_bits = sc_info
        .map(|info| info.mask & configured_sc_index_mask_bits)
        .unwrap_or(SC_INDEX_MASK_ABSENT);
    if ts_index_mask == 0 && sc_index_mask_bits == 0 {
        return None;
    }
    Some(TsRecordEventData {
        pid: packet_view.pid,
        ts_index_mask,
        sc_index_type,
        sc_index_mask_bits,
        byte_number: i64::try_from(cumulative_bytes).ok()?,
        pts,
        first_mb_in_slice,
    })
}

pub fn pes_stream_id(payload: &[u8]) -> Option<i32> {
    if payload.len() >= 4 && payload[0] == 0x00 && payload[1] == 0x00 && payload[2] == 0x01 {
        Some(payload[3] as i32)
    } else {
        None
    }
}

pub fn record_packet_pts(payload: &[u8]) -> Option<i64> {
    if payload.starts_with(&[0x00, 0x00, 0x01]) {
        pes_time_fields(payload).0.map(|value| value as i64)
    } else {
        None
    }
}

pub fn record_sc_info(
    payload: &[u8],
    sc_index_type: i32,
    configured_mask: i32,
) -> Option<RecordStartCodeInfo> {
    if sc_index_type == RECORD_SC_TYPE_NONE || configured_mask == 0 {
        return None;
    }
    let es_payload = match pes_payload_kind(payload) {
        PesPayloadKind::ElementaryStream(bytes) => bytes,
        PesPayloadKind::NotPes => payload,
        PesPayloadKind::MalformedPes => return None,
    };
    let mut offset = 0usize;
    let mut observed_mask = 0i32;
    let mut first_mb = INVALID_FIRST_MB_IN_SLICE;
    while let Some((relative, prefix_len)) = find_sc_prefix(&es_payload[offset..]) {
        let nal_start = offset + relative + prefix_len;
        let nal = &es_payload[nal_start..];
        let parsed = match sc_index_type {
            RECORD_SC_TYPE_SC => parse_generic_sc_index(nal),
            RECORD_SC_TYPE_SC_AVC => parse_avc_sc_index(nal),
            RECORD_SC_TYPE_SC_HEVC => parse_hevc_sc_index(nal),
            RECORD_SC_TYPE_SC_VVC => parse_vvc_sc_index(nal),
            _ => None,
        };
        if let Some(info) = parsed {
            observed_mask |= info.mask;
            if first_mb == INVALID_FIRST_MB_IN_SLICE && (info.mask & configured_mask) != 0 {
                first_mb = info.first_mb_in_slice;
            }
        }
        offset = nal_start.saturating_add(1);
        if offset >= es_payload.len() {
            break;
        }
    }
    (observed_mask != 0).then_some(RecordStartCodeInfo {
        mask: observed_mask,
        first_mb_in_slice: first_mb,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PesPayloadKind<'a> {
    NotPes,
    MalformedPes,
    ElementaryStream(&'a [u8]),
}

fn looks_like_record_index_pes_stream(stream_id: u8) -> bool {
    matches!(
        stream_id,
        0xbc | 0xbd | 0xbe | 0xbf | 0xf0 | 0xf1 | 0xf2 | 0xf8 | 0xff | 0xc0..=0xdf | 0xe0..=0xef
    )
}

fn pes_payload_kind(payload: &[u8]) -> PesPayloadKind<'_> {
    if !payload.starts_with(&[0x00, 0x00, 0x01]) {
        return PesPayloadKind::NotPes;
    }
    let Some(stream_id) = payload.get(3).copied() else {
        return PesPayloadKind::NotPes;
    };
    if !looks_like_record_index_pes_stream(stream_id) {
        return PesPayloadKind::NotPes;
    }
    let Some(summary) = parse_pes_header_summary(payload) else {
        return PesPayloadKind::MalformedPes;
    };
    if let Some(expected_len) = summary.expected_len {
        if payload.len() < expected_len {
            return PesPayloadKind::MalformedPes;
        }
    }
    match payload.get(summary.payload_offset..) {
        Some(bytes) => PesPayloadKind::ElementaryStream(bytes),
        None => PesPayloadKind::MalformedPes,
    }
}

fn find_sc_prefix(bytes: &[u8]) -> Option<(usize, usize)> {
    let mut i = 0usize;
    while i + 3 < bytes.len() {
        if bytes[i] == 0x00 && bytes[i + 1] == 0x00 {
            if bytes[i + 2] == 0x01 {
                return Some((i, 3));
            }
            if i + 4 < bytes.len() && bytes[i + 2] == 0x00 && bytes[i + 3] == 0x01 {
                return Some((i, 4));
            }
        }
        i += 1;
    }
    None
}

fn parse_generic_sc_index(nal: &[u8]) -> Option<RecordStartCodeInfo> {
    let code = *nal.first()?;
    let mask = match code {
        0x00 if nal.len() >= 3 => {
            let picture_header = u16::from_be_bytes([nal[1], nal[2]]);
            match (picture_header >> 3) & 0x07 {
                1 => 1 << 0,
                2 => 1 << 1,
                3 => 1 << 2,
                _ => 0,
            }
        }
        0xb3 => 1 << 3,
        _ => 0,
    };
    (mask != 0).then_some(RecordStartCodeInfo {
        mask,
        first_mb_in_slice: INVALID_FIRST_MB_IN_SLICE,
    })
}

fn parse_avc_sc_index(nal: &[u8]) -> Option<RecordStartCodeInfo> {
    let header = *nal.first()?;
    let nal_type = header & 0x1f;
    if !(1..=5).contains(&nal_type) {
        return None;
    }
    let rbsp = nal_to_rbsp(&nal[1..]);
    let mut reader = BitReader::new(&rbsp);
    let first_mb = reader.read_ue()? as i32;
    let slice_type = (reader.read_ue()? % 5) as u8;
    let mask = match slice_type {
        0 => AVC_SC_P_SLICE,
        1 => AVC_SC_B_SLICE,
        2 => AVC_SC_I_SLICE,
        3 => AVC_SC_SP_SLICE,
        4 => AVC_SC_SI_SLICE,
        _ => 0,
    };
    (mask != 0).then_some(RecordStartCodeInfo {
        mask,
        first_mb_in_slice: first_mb,
    })
}

fn parse_hevc_sc_index(nal: &[u8]) -> Option<RecordStartCodeInfo> {
    if nal.len() < 2 {
        return None;
    }
    let nal_type = (nal[0] >> 1) & 0x3f;
    let mask = match nal_type {
        33 => HEVC_SC_SPS,
        35 => HEVC_SC_AUD,
        16 => HEVC_SC_BLA_W_LP,
        17 => HEVC_SC_BLA_W_RADL,
        18 => HEVC_SC_BLA_N_LP,
        19 => HEVC_SC_IDR_W_RADL,
        20 => HEVC_SC_IDR_N_LP,
        0..=9 | 21 => HEVC_SC_TRAIL_CRA,
        _ => 0,
    };
    (mask != 0).then_some(RecordStartCodeInfo {
        mask,
        first_mb_in_slice: INVALID_FIRST_MB_IN_SLICE,
    })
}

fn parse_vvc_sc_index(nal: &[u8]) -> Option<RecordStartCodeInfo> {
    if nal.len() < 2 {
        return None;
    }
    let nal_type = (nal[1] >> 3) & 0x1f;
    let mask = match nal_type {
        14 => VVC_SC_VPS,
        15 => VVC_SC_SPS,
        20 => VVC_SC_AUD,
        7 => VVC_SC_GDR,
        8 => VVC_SC_IDR_W_RADL,
        9 => VVC_SC_IDR_N_LP,
        10 => VVC_SC_CRA,
        _ => 0,
    };
    (mask != 0).then_some(RecordStartCodeInfo {
        mask,
        first_mb_in_slice: INVALID_FIRST_MB_IN_SLICE,
    })
}

fn nal_to_rbsp(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut zero_run = 0usize;
    for &byte in bytes {
        if zero_run >= 2 && byte == 0x03 {
            zero_run = 0;
            continue;
        }
        out.push(byte);
        if byte == 0x00 {
            zero_run += 1;
        } else {
            zero_run = 0;
        }
    }
    out
}

struct BitReader<'a> {
    bytes: &'a [u8],
    bit_offset: usize,
}

impl<'a> BitReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            bit_offset: 0,
        }
    }

    fn read_bit(&mut self) -> Option<u8> {
        let byte = *self.bytes.get(self.bit_offset / 8)?;
        let bit = 7 - (self.bit_offset % 8);
        self.bit_offset += 1;
        Some((byte >> bit) & 1)
    }

    fn read_bits(&mut self, count: usize) -> Option<u32> {
        let mut value = 0u32;
        for _ in 0..count {
            value = (value << 1) | u32::from(self.read_bit()?);
        }
        Some(value)
    }

    fn read_ue(&mut self) -> Option<u32> {
        let mut leading_zero_bits = 0usize;
        while self.read_bit()? == 0 {
            leading_zero_bits += 1;
            if leading_zero_bits > 31 {
                return None;
            }
        }
        let suffix = if leading_zero_bits == 0 {
            0
        } else {
            self.read_bits(leading_zero_bits)?
        };
        Some(((1u32 << leading_zero_bits) - 1) + suffix)
    }
}

pub fn pes_time_fields(payload: &[u8]) -> (Option<u64>, Option<u64>) {
    let Some(summary) = parse_pes_header_summary(payload) else {
        return (None, None);
    };
    if let Some(expected_len) = summary.expected_len {
        if payload.len() < expected_len {
            return (None, None);
        }
    }
    (summary.pts_90khz, summary.dts_90khz)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestScanner {
        codec: CodecKind,
    }

    impl CodecStartCodeScanner for TestScanner {
        fn codec(&self) -> CodecKind {
            self.codec
        }

        fn scan_start_codes(&self, _payload: &[u8]) -> usize {
            0
        }
    }

    #[test]
    fn codec_scanner_trait_preserves_codec_kind() {
        let scanner = TestScanner {
            codec: CodecKind::H264,
        };
        assert_eq!(scanner.codec(), CodecKind::H264);
        assert_eq!(scanner.scan_start_codes(&[]), 0);
    }
}

#[cfg(test)]
mod adaptation_field_contract_tests {
    use super::{
        RecordEventState, RecordIndexParser, DEMUX_TS_INDEX_ADAPTATION_EXTENSION,
        DEMUX_TS_INDEX_OPCR, DEMUX_TS_INDEX_PCR, DEMUX_TS_INDEX_PRIVATE_DATA,
        DEMUX_TS_INDEX_SPLICING_POINT, RECORD_SC_TYPE_NONE,
    };
    use maleicacid_tuner_hal2_common::TS_PACKET_SIZE;

    fn packet_with_adaptation(flags: u8, body: &[u8]) -> [u8; TS_PACKET_SIZE] {
        let mut packet = [0xffu8; TS_PACKET_SIZE];
        packet[0] = 0x47;
        packet[1] = 0x01;
        packet[2] = 0x00;
        packet[3] = 0x20;
        packet[4] = (1 + body.len()) as u8;
        packet[5] = flags;
        packet[6..6 + body.len()].copy_from_slice(body);
        packet
    }

    fn event_mask(packet: &[u8; TS_PACKET_SIZE], mask: i32) -> i32 {
        let mut parser = RecordIndexParser::new();
        let mut state = RecordEventState::default();
        parser
            .push_ts_packet(
                packet,
                TS_PACKET_SIZE as u64,
                mask,
                RECORD_SC_TYPE_NONE,
                0,
                &mut state,
            )
            .map(|event| event.ts_index_mask)
            .unwrap_or(0)
    }

    #[test]
    fn c03_pcr_flag_requires_complete_six_bytes() {
        let ok = packet_with_adaptation(0x10, &[0, 0, 0, 0, 0, 0]);
        let short = packet_with_adaptation(0x10, &[0, 0, 0, 0, 0]);
        assert_eq!(event_mask(&ok, DEMUX_TS_INDEX_PCR), DEMUX_TS_INDEX_PCR);
        assert_eq!(event_mask(&short, DEMUX_TS_INDEX_PCR), 0);
    }

    #[test]
    fn c03_opcr_flag_requires_complete_six_bytes_after_pcr() {
        let mut ok_body = vec![0; 6];
        ok_body.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
        let mut short_body = vec![0; 6];
        short_body.extend_from_slice(&[0, 0, 0, 0, 0]);
        let ok = packet_with_adaptation(0x18, &ok_body);
        let short = packet_with_adaptation(0x18, &short_body);
        assert_eq!(event_mask(&ok, DEMUX_TS_INDEX_OPCR), DEMUX_TS_INDEX_OPCR);
        assert_eq!(event_mask(&short, DEMUX_TS_INDEX_OPCR), 0);
    }

    #[test]
    fn c03_splicing_private_and_extension_require_declared_lengths() {
        let splice_short = packet_with_adaptation(0x04, &[]);
        assert_eq!(event_mask(&splice_short, DEMUX_TS_INDEX_SPLICING_POINT), 0);

        let private_short = packet_with_adaptation(0x02, &[3, 0xaa]);
        assert_eq!(event_mask(&private_short, DEMUX_TS_INDEX_PRIVATE_DATA), 0);
        let private_ok = packet_with_adaptation(0x02, &[1, 0xaa]);
        assert_eq!(
            event_mask(&private_ok, DEMUX_TS_INDEX_PRIVATE_DATA),
            DEMUX_TS_INDEX_PRIVATE_DATA
        );

        let extension_short = packet_with_adaptation(0x01, &[2, 0xaa]);
        assert_eq!(
            event_mask(&extension_short, DEMUX_TS_INDEX_ADAPTATION_EXTENSION),
            0
        );
        let extension_ok = packet_with_adaptation(0x01, &[1, 0xaa]);
        assert_eq!(
            event_mask(&extension_ok, DEMUX_TS_INDEX_ADAPTATION_EXTENSION),
            DEMUX_TS_INDEX_ADAPTATION_EXTENSION
        );
    }
}

#[cfg(test)]
mod record_start_code_boundary_tests {
    use super::*;
    use maleicacid_tuner_hal2_common::TS_PACKET_SIZE;

    fn payload_packet(pid: u16, cc: u8, payload: &[u8]) -> [u8; TS_PACKET_SIZE] {
        let mut packet = [0xffu8; TS_PACKET_SIZE];
        let adaptation_len = 183usize.saturating_sub(payload.len());
        let payload_offset = 4 + 1 + adaptation_len;
        packet[0] = 0x47;
        packet[1] = ((pid >> 8) as u8) & 0x1f;
        packet[2] = pid as u8;
        packet[3] = 0x30 | (cc & 0x0f);
        packet[4] = adaptation_len as u8;
        if adaptation_len > 0 {
            packet[5] = 0x00;
        }
        packet[payload_offset..payload_offset + payload.len()].copy_from_slice(payload);
        packet
    }

    #[test]
    fn start_code_prefix_carry_crosses_ts_payload_boundary() {
        let mut parser = RecordIndexParser::new();
        let mut state = RecordEventState::default();
        let first = payload_packet(0x0100, 0, &[0x00, 0x00]);
        assert!(parser
            .push_ts_packet(&first, 0, 0, RECORD_SC_TYPE_SC, 1 << 3, &mut state)
            .is_none());

        let second = payload_packet(0x0100, 1, &[0x01, 0xb3]);
        let event = parser.push_ts_packet(
            &second,
            TS_PACKET_SIZE as u64,
            0,
            RECORD_SC_TYPE_SC,
            1 << 3,
            &mut state,
        );
        assert!(
            matches!(event, Some(TsRecordEventData { sc_index_mask_bits, .. }) if sc_index_mask_bits == (1 << 3))
        );
    }
}

#[cfg(test)]
mod record_pts_boundary_tests {
    use super::*;

    #[test]
    fn pts_header_carry_crosses_ts_payload_boundary() {
        let mut state = RecordEventState::default();
        let first = [0x00, 0x00, 0x01, 0xe0, 0x00, 0x00, 0x80, 0x80];
        let second = [0x05, 0x21, 0x00, 0x01, 0x00, 0x01];

        assert_eq!(state.observe_pts(&first, true), None);
        assert_eq!(state.observe_pts(&second, false), Some(0));
    }

    #[test]
    fn pts_start_prefix_fragment_carry_crosses_ts_payload_boundary() {
        let mut state = RecordEventState::default();
        let first = [0x00, 0x00];
        let second = [
            0x01, 0xe0, 0x00, 0x00, 0x80, 0x80, 0x05, 0x21, 0x00, 0x01, 0x00, 0x01,
        ];

        assert_eq!(state.observe_pts(&first, true), None);
        assert_eq!(state.observe_pts(&second, false), Some(0));
    }

    #[test]
    fn malformed_prefix_fragment_is_cleared_without_pts() {
        let mut state = RecordEventState::default();
        assert_eq!(state.observe_pts(&[0x00, 0x00], true), None);
        assert_eq!(state.observe_pts(&[0x02, 0xe0, 0x00, 0x00], false), None);
        assert_eq!(
            state.observe_pts(
                &[0x01, 0xe0, 0x00, 0x00, 0x80, 0x80, 0x05, 0x21, 0x00, 0x01, 0x00, 0x01],
                false
            ),
            None
        );
    }
}

#[cfg(test)]
mod record_pes_payload_tests {
    use super::*;

    #[test]
    fn pes_payload_offset_uses_validated_header_layout() {
        let optional_header_missing = [0x00, 0x00, 0x01, 0xe0, 0x00, 0x00, 0x80, 0x80];
        assert!(matches!(
            pes_payload_kind(&optional_header_missing),
            PesPayloadKind::MalformedPes
        ));

        let malformed_flags = [0x00, 0x00, 0x01, 0xe0, 0x00, 0x03, 0x00, 0x80, 0x00];
        assert!(matches!(
            pes_payload_kind(&malformed_flags),
            PesPayloadKind::MalformedPes
        ));

        let optional_header_too_large = [0x00, 0x00, 0x01, 0xe0, 0x00, 0x03, 0x80, 0x80, 0x10];
        assert!(matches!(
            pes_payload_kind(&optional_header_too_large),
            PesPayloadKind::MalformedPes
        ));

        let private_stream_2 = [0x00, 0x00, 0x01, 0xbf, 0x00, 0x02, 0xaa, 0xbb];
        assert!(
            matches!(pes_payload_kind(&private_stream_2), PesPayloadKind::ElementaryStream(bytes) if bytes == &private_stream_2[6..])
        );
    }

    #[test]
    fn malformed_pes_does_not_fall_back_to_scanning_header_bytes() {
        let malformed_pes_with_embedded_avc_slice = [
            0x00, 0x00, 0x01, 0xe0, 0x00, 0x03, 0x00, 0x80, 0x00, 0x00, 0x00, 0x01, 0x65, 0x33,
        ];
        assert!(matches!(
            pes_payload_kind(&malformed_pes_with_embedded_avc_slice),
            PesPayloadKind::MalformedPes
        ));
        assert!(record_sc_info(
            &malformed_pes_with_embedded_avc_slice,
            RECORD_SC_TYPE_SC_AVC,
            AVC_SC_I_SLICE,
        )
        .is_none());
    }

    #[test]
    fn raw_elementary_stream_start_code_is_still_scanned() {
        let raw_avc_slice = [0x00, 0x00, 0x01, 0x65, 0x33];
        assert!(matches!(
            pes_payload_kind(&raw_avc_slice),
            PesPayloadKind::NotPes
        ));
        let info = record_sc_info(&raw_avc_slice, RECORD_SC_TYPE_SC_AVC, AVC_SC_I_SLICE)
            .expect("raw ES AVC slice should still be indexed");
        assert_eq!(info.mask & AVC_SC_I_SLICE, AVC_SC_I_SLICE);
    }
}

#[cfg(test)]
mod record_start_code_mask_tests {
    use super::*;

    #[test]
    fn first_mb_comes_from_slice_matching_configured_mask() {
        let unmatched_p_slice = [0x00, 0x00, 0x01, 0x41, 0x25];
        let matched_i_slice = [0x00, 0x00, 0x01, 0x41, 0x33];
        let mut payload = Vec::new();
        payload.extend_from_slice(&unmatched_p_slice);
        payload.extend_from_slice(&matched_i_slice);

        let info = record_sc_info(&payload, RECORD_SC_TYPE_SC_AVC, AVC_SC_I_SLICE);
        assert!(matches!(
            info,
            Some(RecordStartCodeInfo {
                first_mb_in_slice: 5,
                ..
            })
        ));
        if let Some(RecordStartCodeInfo { mask, .. }) = info {
            assert_eq!(mask & AVC_SC_I_SLICE, AVC_SC_I_SLICE);
        }
    }
    #[test]
    fn record_byte_number_never_negative() {
        let mut state = RecordEventState::default();
        let mut packet = [0xffu8; 188];
        packet[0] = 0x47;
        packet[1] = 0x41;
        packet[2] = 0x20;
        packet[3] = 0x10;
        packet[4..13].copy_from_slice(&[0x00, 0x00, 0x01, 0xe0, 0x00, 0x00, 0x80, 0x00, 0x00]);
        let event = build_ts_record_event_data(
            &packet,
            i64::MAX as u64,
            DEMUX_TS_INDEX_PAYLOAD_UNIT_START,
            RECORD_SC_TYPE_NONE,
            0,
            &mut state,
        )
        .unwrap();
        assert!(event.byte_number >= 0);
    }

    #[test]
    fn record_byte_number_overflow_stops_dvr() {
        let mut state = RecordEventState::default();
        let mut packet = [0xffu8; 188];
        packet[0] = 0x47;
        packet[1] = 0x41;
        packet[2] = 0x20;
        packet[3] = 0x10;
        packet[4..13].copy_from_slice(&[0x00, 0x00, 0x01, 0xe0, 0x00, 0x00, 0x80, 0x00, 0x00]);
        assert!(build_ts_record_event_data(
            &packet,
            (i64::MAX as u64).saturating_add(1),
            DEMUX_TS_INDEX_PAYLOAD_UNIT_START,
            RECORD_SC_TYPE_NONE,
            0,
            &mut state,
        )
        .is_none());
    }

    #[test]
    fn record_byte_number_overflow_does_not_emit_event() {
        let mut state = RecordEventState::default();
        let mut packet = [0xffu8; 188];
        packet[0] = 0x47;
        packet[1] = 0x41;
        packet[2] = 0x20;
        packet[3] = 0x10;
        packet[4..13].copy_from_slice(&[0x00, 0x00, 0x01, 0xe0, 0x00, 0x00, 0x80, 0x00, 0x00]);
        assert!(build_ts_record_event_data(
            &packet,
            (i64::MAX as u64).saturating_add(1),
            DEMUX_TS_INDEX_PAYLOAD_UNIT_START,
            RECORD_SC_TYPE_NONE,
            0,
            &mut state,
        )
        .is_none());
    }
}

#[cfg(test)]
mod scrambled_record_policy_tests {
    use super::*;

    fn payload_packet(pid: u16, scrambling_control: u8, pusi: bool, payload: &[u8]) -> [u8; 188] {
        let mut packet = [0xffu8; 188];
        packet[0] = 0x47;
        packet[1] = ((pid >> 8) as u8) & 0x1f;
        if pusi {
            packet[1] |= 0x40;
        }
        packet[2] = pid as u8;
        packet[3] = ((scrambling_control & 0x03) << 6) | 0x10;
        packet[4..4 + payload.len()].copy_from_slice(payload);
        packet
    }

    #[test]
    fn scrambled_record_packet_emits_only_ts_scrambling_metadata() {
        let mut state = RecordEventState::default();
        let mut parser = RecordIndexParser::new();
        let packet = payload_packet(
            0x0100,
            2,
            true,
            &[
                0x00, 0x00, 0x01, 0xe0, 0x00, 0x00, 0x80, 0x80, 0x05, 0x21, 0x00, 0x01, 0x00, 0x01,
            ],
        );
        let mask = DEMUX_TS_INDEX_CHANGE_TO_EVEN_SCRAMBLED | DEMUX_TS_INDEX_PAYLOAD_UNIT_START;
        let event = parser.push_ts_packet(
            &packet,
            0,
            mask,
            RECORD_SC_TYPE_SC_AVC,
            AVC_SC_I_SLICE,
            &mut state,
        );

        assert!(matches!(
            event,
            Some(TsRecordEventData {
                ts_index_mask,
                pts: RECORD_INDEX_PTS_ABSENT,
                sc_index_mask_bits: SC_INDEX_MASK_ABSENT,
                first_mb_in_slice: INVALID_FIRST_MB_IN_SLICE,
                ..
            }) if ts_index_mask == mask
        ));
    }

    #[test]
    fn scrambled_record_packet_clears_cross_packet_pes_and_sc_carry() {
        let mut state = RecordEventState::default();
        let first = payload_packet(0x0100, 0, true, &[0x00, 0x00]);
        let scrambled = payload_packet(0x0100, 2, false, &[0xaa, 0xbb]);
        let second = payload_packet(
            0x0100,
            0,
            false,
            &[
                0x01, 0xe0, 0x00, 0x00, 0x80, 0x80, 0x05, 0x21, 0x00, 0x01, 0x00, 0x01,
            ],
        );

        assert!(build_ts_record_event_data(
            &first,
            0,
            0,
            RECORD_SC_TYPE_SC_AVC,
            AVC_SC_I_SLICE,
            &mut state
        )
        .is_none());
        assert!(build_ts_record_event_data(
            &scrambled,
            188,
            DEMUX_TS_INDEX_CHANGE_TO_EVEN_SCRAMBLED,
            RECORD_SC_TYPE_SC_AVC,
            AVC_SC_I_SLICE,
            &mut state,
        )
        .is_some());
        let recovered = build_ts_record_event_data(
            &second,
            376,
            DEMUX_TS_INDEX_PAYLOAD_UNIT_START,
            RECORD_SC_TYPE_SC_AVC,
            AVC_SC_I_SLICE,
            &mut state,
        );
        assert!(recovered.is_none());
    }
}

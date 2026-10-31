use crate::parser::bitstream::{nal_to_rbsp, BitReader};
use super::video_timestamp::VideoTimestampFailure;

const PTS_MODULUS: i128 = 1_i128 << 33;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Hrd {
    count: u32,
    initial_bits: usize,
    cpb_bits: usize,
    dpb_bits: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Sps {
    tick: u32,
    scale: u32,
    nal_hrd: Option<Hrd>,
    vcl_hrd: Option<Hrd>,
    low_delay: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Anchor {
    sps: Sps,
    // 90 kHz時刻の有理数分子。AUごとの丸め誤差を累積させない。
    base: i128,
    previous_delay: u32,
    unwrapped_delay: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct AvcTiming {
    sps: Vec<Option<Sps>>,
    pps: Vec<Option<u32>>,
    anchor: Option<Anchor>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AvcTimestamp {
    pub pts: u64,
}

impl AvcTiming {
    pub(crate) fn timestamp(
        &mut self,
        payload: &[u8],
        pts: Option<u64>,
        dts: Option<u64>,
    ) -> Result<AvcTimestamp, VideoTimestampFailure> {
        let mut active = None;
        for nal in nals(payload) {
            let (&header, body) = nal.split_first().ok_or(VideoTimestampFailure::MissingAnchor)?;
            if header & 0x80 != 0 {
                return Err(VideoTimestampFailure::MissingAnchor);
            }
            let kind = header & 31;
            if matches!(kind, 6 | 9..=12) {
                continue;
            }
            if !matches!(kind, 1 | 5 | 7 | 8) {
                return Err(VideoTimestampFailure::MissingAnchor);
            }
            // sliceでは先頭3個のue(v)だけを読む。各最大63 bitとemulation prevention分を
            // 含む64 byteに限定し、圧縮picture本体をRBSPへ複製しない。
            let syntax = if matches!(header & 31, 1 | 5) {
                &body[..body.len().min(64)]
            } else {
                body
            };
            let rbsp = nal_to_rbsp(syntax)
                .map_err(|_| VideoTimestampFailure::AllocationFailed)?;
            let mut bits = BitReader::new(&rbsp);
            match header & 31 {
                7 => {
                    let (id, sps) = parse_sps(&mut bits).ok_or(VideoTimestampFailure::MissingAnchor)?;
                    let id = id as usize;
                    if self.sps.len() <= id {
                        self.sps.try_reserve(id + 1 - self.sps.len())
                            .map_err(|_| VideoTimestampFailure::AllocationFailed)?;
                        self.sps.resize(id + 1, None);
                    }
                    self.sps[id] = Some(sps);
                }
                8 => {
                    let pps = bits.read_ue().ok_or(VideoTimestampFailure::MissingAnchor)?;
                    let sps = bits.read_ue().ok_or(VideoTimestampFailure::MissingAnchor)?;
                    if pps > 255 || sps > 31 {
                        return Err(VideoTimestampFailure::MissingAnchor);
                    }
                    let pps = pps as usize;
                    if self.pps.len() <= pps {
                        self.pps.try_reserve(pps + 1 - self.pps.len())
                            .map_err(|_| VideoTimestampFailure::AllocationFailed)?;
                        self.pps.resize(pps + 1, None);
                    }
                    self.pps[pps] = Some(sps);
                }
                1 | 5 => {
                    bits.read_ue().ok_or(VideoTimestampFailure::MissingAnchor)?;
                    if bits.read_ue().ok_or(VideoTimestampFailure::MissingAnchor)? > 9 {
                        return Err(VideoTimestampFailure::MissingAnchor);
                    }
                    let pps = bits.read_ue().ok_or(VideoTimestampFailure::MissingAnchor)?;
                    let sps_id = self.pps.get(pps as usize).copied().flatten().ok_or(VideoTimestampFailure::MissingAnchor)?;
                    if active.is_some_and(|old| old != sps_id) {
                        return Err(VideoTimestampFailure::MissingAnchor);
                    }
                    active = Some(sps_id);
                }
                _ => return Err(VideoTimestampFailure::MissingAnchor),
            }
        }
        let sps_id = active.ok_or(VideoTimestampFailure::MissingAnchor)?;
        let sps = self.sps.get(sps_id as usize).copied().flatten().ok_or(VideoTimestampFailure::MissingAnchor)?;
        let hrd = sps.nal_hrd.or(sps.vcl_hrd);
        let mut buffering_period = false;
        let mut delays = None;
        // active SPS確定後にSEIを読む。NAL数に比例するRBSP保管配列は持たない。
        for nal in nals(payload).filter(|nal| nal.first().is_some_and(|header| header & 31 == 6)) {
            let rbsp = nal_to_rbsp(&nal[1..])
                .map_err(|_| VideoTimestampFailure::AllocationFailed)?;
            let mut rest = rbsp.as_slice();
            while !rest.is_empty() && rest != [0x80] {
                let kind = sei_number(&mut rest).ok_or(VideoTimestampFailure::MissingAnchor)?;
                let len = sei_number(&mut rest).ok_or(VideoTimestampFailure::MissingAnchor)?;
                let body = rest.get(..len).ok_or(VideoTimestampFailure::MissingAnchor)?;
                rest = rest.get(len..).ok_or(VideoTimestampFailure::MissingAnchor)?;
                let mut bits = BitReader::new(body);
                match kind {
                    0 => {
                        if buffering_period || bits.read_ue().ok_or(VideoTimestampFailure::MissingAnchor)? != sps_id {
                            return Err(VideoTimestampFailure::MissingAnchor);
                        }
                        for hrd in [sps.nal_hrd, sps.vcl_hrd].into_iter().flatten() {
                            for _ in 0..hrd.count {
                                bits.read_bits(hrd.initial_bits).ok_or(VideoTimestampFailure::MissingAnchor)?;
                                bits.read_bits(hrd.initial_bits).ok_or(VideoTimestampFailure::MissingAnchor)?;
                            }
                        }
                        buffering_period = true;
                    }
                    1 => {
                        if let Some(hrd) = hrd {
                            if delays.is_some() {
                                return Err(VideoTimestampFailure::MissingAnchor);
                            }
                            delays = Some((
                                bits.read_bits(hrd.cpb_bits).ok_or(VideoTimestampFailure::MissingAnchor)?,
                                bits.read_bits(hrd.dpb_bits).ok_or(VideoTimestampFailure::MissingAnchor)?,
                            ));
                        }
                    }
                    _ => {}
                }
            }
        }
        // HRDを持たないstreamは明示PTSを持つAUだけを配送する。
        let Some((cpb, dpb)) = delays else {
            self.anchor = None;
            return pts.map(|pts| AvcTimestamp { pts }).ok_or(VideoTimestampFailure::MissingAnchor);
        };
        let hrd = hrd.ok_or(VideoTimestampFailure::MissingAnchor)?;
        if sps.tick == 0 || sps.scale == 0 {
            return Err(VideoTimestampFailure::MissingAnchor);
        }
        let tick = i128::from(sps.tick) * 90_000;
        let scale = i128::from(sps.scale);
        let old = self.anchor.filter(|anchor| anchor.sps == sps);
        let elapsed = match old {
            Some(anchor) => {
                let modulus = 1_u64 << hrd.cpb_bits;
                let delta = (u64::from(cpb) + modulus - u64::from(anchor.previous_delay)) % modulus;
                anchor.unwrapped_delay.checked_add(delta).ok_or(VideoTimestampFailure::MissingAnchor)?
            }
            None => u64::from(cpb),
        };
        let (decode, output, base) = if let Some(pts) = pts {
            let output = i128::from(pts) * scale;
            let decode = output - i128::from(dpb) * tick;
            if let Some(dts) = dts {
                if (decode.div_euclid(scale)).rem_euclid(PTS_MODULUS) != i128::from(dts) {
                    return Err(VideoTimestampFailure::MissingAnchor);
                }
            }
            (decode, output, decode - i128::from(elapsed) * tick)
        } else {
            if sps.low_delay {
                return Err(VideoTimestampFailure::MissingAnchor);
            }
            let anchor = old.ok_or(VideoTimestampFailure::MissingAnchor)?;
            let decode = anchor.base + i128::from(elapsed) * tick;
            (decode, decode + i128::from(dpb) * tick, anchor.base)
        };
        // H.264 C.1.2: BPを含むAU自身は旧periodを参照し、その処理後に基準を更新する。
        self.anchor = Some(Anchor {
            sps,
            base: if buffering_period { decode } else { base },
            previous_delay: if buffering_period { 0 } else { cpb },
            unwrapped_delay: if buffering_period { 0 } else { elapsed },
        });
        Ok(AvcTimestamp {
            pts: output.div_euclid(scale).rem_euclid(PTS_MODULUS) as u64,
        })
    }
}

fn sei_number(bytes: &mut &[u8]) -> Option<usize> {
    let mut value = 0usize;
    loop {
        let (&byte, tail) = bytes.split_first()?;
        *bytes = tail;
        value = value.checked_add(usize::from(byte))?;
        if byte != 255 {
            return Some(value);
        }
    }
}

pub(crate) fn start_codes(bytes: &[u8]) -> impl Iterator<Item = (usize, usize)> + '_ {
    let mut index = 0usize;
    std::iter::from_fn(move || {
        while index + 3 < bytes.len() {
            if bytes[index..].starts_with(&[0, 0, 1]) {
                let start = if index > 0 && bytes[index - 1] == 0 {
                    index - 1
                } else {
                    index
                };
                index += 3;
                return Some((start, index));
            }
            index += 1;
        }
        None
    })
}

fn nals(bytes: &[u8]) -> impl Iterator<Item = &[u8]> {
    let mut codes = start_codes(bytes).peekable();
    std::iter::from_fn(move || {
        let (_, body) = codes.next()?;
        let end = codes.peek().map_or(bytes.len(), |(start, _)| *start);
        Some(&bytes[body..end])
    })
}

fn parse_hrd(bits: &mut BitReader<'_>) -> Option<Hrd> {
    let count = bits.read_ue()?.checked_add(1)?;
    if count > 32 {
        return None;
    }
    bits.read_bits(8)?;
    for _ in 0..count {
        bits.read_ue()?;
        bits.read_ue()?;
        bits.read_bit()?;
    }
    let initial_bits = bits.read_bits(5)? as usize + 1;
    let cpb_bits = bits.read_bits(5)? as usize + 1;
    let dpb_bits = bits.read_bits(5)? as usize + 1;
    bits.read_bits(5)?;
    Some(Hrd {
        count,
        initial_bits,
        cpb_bits,
        dpb_bits,
    })
}

fn parse_sps(bits: &mut BitReader<'_>) -> Option<(u32, Sps)> {
    let profile = bits.read_bits(8)?;
    bits.read_bits(16)?;
    let id = bits.read_ue()?;
    if id > 31 {
        return None;
    }
    if matches!(
        profile,
        100 | 110 | 122 | 244 | 44 | 83 | 86 | 118 | 128 | 138 | 139 | 134 | 135
    ) {
        let chroma = bits.read_ue()?;
        if chroma > 3 {
            return None;
        }
        if chroma == 3 {
            bits.read_bit()?;
        }
        if bits.read_ue()? > 6 || bits.read_ue()? > 6 {
            return None;
        }
        bits.read_bit()?;
        if bits.read_bit()? != 0 {
            for index in 0..if chroma == 3 { 12 } else { 8 } {
                if bits.read_bit()? == 0 {
                    continue;
                }
                let mut last = 8_i64;
                let mut next = 8_i64;
                for _ in 0..if index < 6 { 16 } else { 64 } {
                    if next != 0 {
                        let code = i64::from(bits.read_ue()?);
                        let delta = if code & 1 != 0 {
                            (code + 1) / 2
                        } else {
                            -(code / 2)
                        };
                        next = (last + delta).rem_euclid(256);
                    }
                    if next != 0 {
                        last = next;
                    }
                }
            }
        }
    }
    if bits.read_ue()? > 12 {
        return None;
    }
    match bits.read_ue()? {
        0 => {
            if bits.read_ue()? > 12 {
                return None;
            }
        }
        1 => {
            bits.read_bit()?;
            bits.read_ue()?;
            bits.read_ue()?;
            let count = bits.read_ue()?;
            if count > 255 {
                return None;
            }
            for _ in 0..count {
                bits.read_ue()?;
            }
        }
        2 => {}
        _ => return None,
    }
    bits.read_ue()?;
    bits.read_bit()?;
    bits.read_ue()?;
    bits.read_ue()?;
    if bits.read_bit()? == 0 {
        bits.read_bit()?;
    }
    bits.read_bit()?;
    if bits.read_bit()? != 0 {
        for _ in 0..4 {
            bits.read_ue()?;
        }
    }
    let mut sps = Sps {
        tick: 0,
        scale: 0,
        nal_hrd: None,
        vcl_hrd: None,
        low_delay: false,
    };
    if bits.read_bit()? == 0 {
        return Some((id, sps));
    }
    if bits.read_bit()? != 0 && bits.read_bits(8)? == 255 {
        bits.read_bits(32)?;
    }
    if bits.read_bit()? != 0 {
        bits.read_bit()?;
    }
    if bits.read_bit()? != 0 {
        bits.read_bits(4)?;
        if bits.read_bit()? != 0 {
            bits.read_bits(24)?;
        }
    }
    if bits.read_bit()? != 0 {
        bits.read_ue()?;
        bits.read_ue()?;
    }
    if bits.read_bit()? != 0 {
        sps.tick = bits.read_bits(32)?;
        sps.scale = bits.read_bits(32)?;
        bits.read_bit()?;
        if sps.tick == 0 || sps.scale == 0 {
            return None;
        }
    }
    if bits.read_bit()? != 0 {
        sps.nal_hrd = Some(parse_hrd(bits)?);
    }
    if bits.read_bit()? != 0 {
        sps.vcl_hrd = Some(parse_hrd(bits)?);
    }
    if let (Some(nal), Some(vcl)) = (sps.nal_hrd, sps.vcl_hrd) {
        if nal.cpb_bits != vcl.cpb_bits || nal.dpb_bits != vcl.dpb_bits {
            return None;
        }
    }
    if sps.nal_hrd.is_some() || sps.vcl_hrd.is_some() {
        sps.low_delay = bits.read_bit()? != 0;
    }
    bits.read_bit()?;
    if bits.read_bit()? != 0 {
        bits.read_bit()?;
        for _ in 0..6 {
            bits.read_ue()?;
        }
    }
    Some((id, sps))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn many_start_codes_preserve_prefixes_and_escaped_payload() {
        let mut bytes = Vec::new();
        let mut expected = Vec::new();
        for index in 0..16384 {
            let start = bytes.len();
            let prefix: &[u8] = if index % 2 == 0 {
                &[0, 0, 1]
            } else {
                &[0, 0, 0, 1]
            };
            bytes.extend_from_slice(prefix);
            expected.push((start, bytes.len()));
            bytes.extend_from_slice(&[12, 0, 0, 3, 1, 0x80]);
        }
        bytes.extend_from_slice(&[0, 0, 1]);
        assert!(start_codes(&bytes).eq(expected));
        let mut units = nals(&bytes);
        for _ in 0..16383 {
            assert_eq!(units.next(), Some([12, 0, 0, 3, 1, 0x80].as_slice()));
        }
        assert_eq!(
            units.next(),
            Some([12, 0, 0, 3, 1, 0x80, 0, 0, 1].as_slice())
        );
        assert_eq!(units.next(), None);
    }

    struct Bits(Vec<bool>);
    impl Bits {
        fn put(&mut self, value: u32, width: usize) {
            for bit in (0..width).rev() {
                self.0.push(value & (1 << bit) != 0);
            }
        }
        fn ue(&mut self, value: u32) {
            let code = value + 1;
            let width = (32 - code.leading_zeros()) as usize;
            self.put(0, width - 1);
            self.put(code, width);
        }
        fn finish(mut self) -> Vec<u8> {
            self.0.push(true);
            while self.0.len() % 8 != 0 {
                self.0.push(false);
            }
            self.0
                .chunks(8)
                .map(|chunk| {
                    chunk
                        .iter()
                        .fold(0u8, |byte, bit| (byte << 1) | u8::from(*bit))
                })
                .collect()
        }
    }

    fn nal(out: &mut Vec<u8>, header: u8, bytes: &[u8]) {
        out.extend_from_slice(&[0, 0, 0, 1, header]);
        let mut zeroes = 0;
        for &byte in bytes {
            if zeroes == 2 && byte <= 3 {
                out.push(3);
                zeroes = 0;
            }
            out.push(byte);
            zeroes = if byte == 0 { zeroes + 1 } else { 0 };
        }
    }

    pub(crate) fn au(config: bool, bp: bool, cpb: u32, dpb: u32, tick: u32, scale: u32) -> Vec<u8> {
        let mut out = Vec::new();
        nal(&mut out, 9, &[0x10]);
        if config {
            let mut bits = Bits(Vec::new());
            bits.put(66, 8);
            bits.put(0, 8);
            bits.put(30, 8);
            bits.ue(0);
            bits.ue(0);
            bits.ue(2);
            bits.ue(1);
            bits.put(0, 1);
            bits.ue(19);
            bits.ue(10);
            bits.put(1, 1);
            bits.put(1, 1);
            bits.put(0, 1);
            bits.put(1, 1);
            bits.put(0, 4);
            bits.put(1, 1);
            bits.put(tick, 32);
            bits.put(scale, 32);
            bits.put(0, 1);
            bits.put(1, 1);
            bits.ue(0);
            bits.put(0, 8);
            bits.ue(0);
            bits.ue(0);
            bits.put(1, 1);
            bits.put(16, 5);
            bits.put(7, 5);
            bits.put(3, 5);
            bits.put(0, 5);
            bits.put(0, 4);
            nal(&mut out, 0x67, &bits.finish());
            nal(&mut out, 0x68, &[0xc0]);
        }
        let mut sei = Vec::new();
        if bp {
            let mut bits = Bits(Vec::new());
            bits.ue(0);
            bits.put(1234, 17);
            bits.put(0, 17);
            let body = bits.finish();
            sei.extend_from_slice(&[0, body.len() as u8]);
            sei.extend(body);
        }
        let mut bits = Bits(Vec::new());
        bits.put(cpb, 8);
        bits.put(dpb, 4);
        let body = bits.finish();
        sei.extend_from_slice(&[1, body.len() as u8]);
        sei.extend(body);
        sei.push(0x80);
        nal(&mut out, 6, &sei);
        nal(&mut out, if bp { 0x65 } else { 0x41 }, &[0xb8, 0x80]);
        out
    }

    #[test]
    fn variable_picture_delay_and_buffering_period_are_not_fixed_frame_rate() {
        let mut timing = AvcTiming::default();
        assert_eq!(
            timing
                .timestamp(
                    &au(true, true, 60, 2, 1001, 30000),
                    Some(90000),
                    Some(83994)
                )
                .unwrap()
                .pts,
            90000
        );
        assert_eq!(
            timing
                .timestamp(&au(false, false, 2, 0, 1001, 30000), None, None)
                .unwrap()
                .pts,
            90000
        );
        assert_eq!(
            timing
                .timestamp(&au(false, false, 8, 3, 1001, 30000), None, None)
                .unwrap()
                .pts,
            117027
        );
        assert_eq!(
            timing
                .timestamp(&au(false, true, 10, 0, 1001, 30000), None, None)
                .unwrap()
                .pts,
            114024
        );
        assert_eq!(
            timing
                .timestamp(&au(false, false, 4, 0, 1001, 30000), None, None)
                .unwrap()
                .pts,
            126036
        );
    }

    #[test]
    fn delay_and_pts_wrap_and_fractional_ticks_keep_one_rational_anchor() {
        let mut timing = AvcTiming::default();
        let base = (1_u64 << 33) - 4;
        timing
            .timestamp(&au(true, false, 254, 0, 1, 60000), Some(base), None)
            .unwrap();
        assert_eq!(
            timing
                .timestamp(&au(false, false, 255, 0, 1, 60000), None, None)
                .unwrap()
                .pts,
            base + 1
        );
        assert_eq!(
            timing
                .timestamp(&au(false, false, 0, 0, 1, 60000), None, None)
                .unwrap()
                .pts,
            base + 3
        );
        assert_eq!(
            timing
                .timestamp(&au(false, false, 1, 0, 1, 60000), None, None)
                .unwrap()
                .pts,
            0
        );
    }

    #[test]
    fn large_filler_and_sei_keep_hrd_timestamp_association() {
        let mut timing = AvcTiming::default();
        let mut first = au(true, true, 60, 0, 1001, 30000);
        let filler = vec![0xff; 2 * 1024 * 1024];
        nal(&mut first, 12, &[filler.as_slice(), &[0x80]].concat());
        assert_eq!(timing.timestamp(&first, Some(90000), None).unwrap().pts, 90000);

        let mut second = au(false, false, 2, 0, 1001, 30000);
        // user_data_unregisteredのUUIDとpayloadをpicture timingと同じAUへ置く。
        let mut sei = vec![5];
        let size = 1024 * 1024 + 16;
        sei.extend(std::iter::repeat(255).take(size / 255));
        sei.push((size % 255) as u8);
        sei.extend(std::iter::repeat(0x55).take(size));
        sei.push(0x80);
        nal(&mut second, 6, &sei);
        assert_eq!(timing.timestamp(&second, None, None).unwrap().pts, 96006);
        assert_eq!(timing.timestamp(&au(false, false, 4, 0, 1001, 30000), None, None).unwrap().pts, 102012);
    }

    #[test]
    fn missing_configuration_anchor_or_invalid_timing_cannot_create_pts() {
        let mut timing = AvcTiming::default();
        assert!(timing
            .timestamp(&au(false, false, 2, 0, 1, 60000), Some(42), None)
            .is_err());
        assert!(timing
            .timestamp(&au(true, true, 2, 0, 1, 60000), None, None)
            .is_err());
        assert!(timing
            .timestamp(&au(true, true, 2, 0, 0, 60000), Some(42), None)
            .is_err());
    }
}

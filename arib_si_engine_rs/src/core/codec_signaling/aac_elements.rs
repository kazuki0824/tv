//! MPEG-2 AAC LCの構文要素を終端まで読む。伝送CRCの照合や係数・PCMの展開は行わない。
use super::{aac_tables::*, parse_program_config, AudioConfigBits};

#[derive(Debug, PartialEq, Eq)]
pub enum AacPresentation {
    DualMono,
    Other,
}

pub(super) struct ParsedAacFrame {
    pub(super) presentation: AacPresentation,
    pub(super) first_elements: [(u8, u8); 2],
    pub(super) has_program_config: bool,
}

/// 完全フレームで確認できたMPEG ID、profile、周波数、チャンネル構成の同一性。
pub(super) fn adts_frame_header(frame: &[u8]) -> Option<(usize, u16)> {
    let h = frame.get(..7)?;
    let size = ((h[3] as usize & 3) << 11) | ((h[4] as usize) << 3) | (h[5] as usize >> 5);
    if h[0] != 255
        || h[1] & 0xf6 != 0xf0
        || size <= if h[1] & 1 == 0 { 9 } else { 7 }
        || (h[2] >> 2) & 15 >= 13
    {
        return None;
    }
    let key = (u16::from(h[1] & 8) << 8) | (u16::from(h[2] & 0xfd) << 2) | u16::from(h[3] >> 6);
    Some((size, key))
}

#[cfg(test)]
fn adts_presentation(frame: &[u8]) -> Option<AacPresentation> {
    parse_frame(frame).map(|parsed| parsed.presentation)
}

pub(super) fn parse_frame(frame: &[u8]) -> Option<ParsedAacFrame> {
    let h = frame.get(..7)?;
    let frequency = (h[2] >> 2) & 15;
    let configuration = ((h[2] & 1) << 2) | (h[3] >> 6);
    let length = ((h[3] as usize & 3) << 11) | ((h[4] as usize) << 3) | (h[5] as usize >> 5);
    if h[0] != 255
        || h[1] & 0xfe != 0xf8
        || h[2] >> 6 != 1
        || frequency >= 12
        || length != frame.len()
        || h[6] & 3 != 0
    {
        return None;
    }
    let mut bits = AudioConfigBits {
        bytes: frame.get(if h[1] & 1 != 0 { 7 } else { 9 }..)?,
        offset: 0,
    };
    let mut tags = [0u16; 4];
    let mut first_elements = [(0, 0); 2];
    let mut next_element = first_elements.iter_mut();
    let mut channels = 0;
    let mut pce = None;
    loop {
        let element = bits.read(3)? as usize;
        match element {
            0 | 1 | 3 => {
                let instance = bits.read(4)?;
                let tag = 1 << instance;
                if let Some(slot) = next_element.next() {
                    *slot = (element as u8, instance as u8);
                }
                if tags[element] & tag != 0 {
                    return None;
                }
                tags[element] |= tag;
                let shared = if element == 1 && bits.read(1)? != 0 {
                    let window = Window::read(&mut bits, frequency)?;
                    match bits.read(2)? {
                        1 => bits.skip(window.groups.len() * window.bands)?,
                        3 => return None,
                        _ => {}
                    }
                    Some(window)
                } else {
                    None
                };
                for index in 0..if element == 1 { 2 } else { 1 } {
                    channel(
                        &mut bits,
                        frequency,
                        shared.as_ref(),
                        index == 1 && shared.is_some(),
                    )?;
                    channels += 1;
                }
            }
            4 => {
                bits.skip(4)?;
                let align = bits.read(1)? != 0;
                let mut size = bits.read(8)?;
                if size == 255 {
                    size += bits.read(8)?;
                }
                if align {
                    bits.align()?;
                }
                bits.skip(size as usize * 8)?;
            }
            5 => {
                let parsed = parse_program_config(&mut bits)?;
                if parsed.object_type != 2
                    || parsed.frequency_index != frequency
                    || pce.replace(parsed).is_some()
                {
                    return None;
                }
            }
            6 => {
                let mut count = bits.read(4)?;
                if count == 15 {
                    count = 14 + bits.read(8)?;
                }
                bits.skip(count as usize * 8)?;
            }
            7 => break,
            _ => return None,
        }
    }
    bits.align()?;
    if bits.offset != bits.bytes.len() * 8
        || channels == 0
        || pce.as_ref().is_some_and(|config| {
            config.element_tags != tags || usize::from(config.channel_count) != channels
        })
    {
        return None;
    }
    let sce = tags[0].count_ones();
    if sce == 2
        && channels == 2
        && pce
            .as_ref()
            .is_some_and(|config| config.front_pair != Some(first_elements.map(|(_, tag)| tag)))
    {
        return None;
    }
    let cpe = tags[1].count_ones();
    let lfe = tags[3].count_ones();
    if configuration == 0 {
        // ARIB STD-B32 第2部5.2.3(2) の送出順。
        match (first_elements[0].0, sce, cpe, lfe) {
            (0, 2, 0, 0) | (1, 1, 1, 0) | (1, 0, 2, 0) => {}
            _ => return None,
        }
    } else {
        let expected = [
            (0, 0, 0),
            (1, 0, 0),
            (0, 1, 0),
            (1, 1, 0),
            (2, 1, 0),
            (1, 2, 0),
            (1, 2, 1),
            (1, 3, 1),
        ];
        if expected[configuration as usize] != (sce, cpe, lfe) {
            return None;
        }
    }
    Some(ParsedAacFrame {
        presentation: if configuration == 0 && sce == 2 && channels == 2 {
            AacPresentation::DualMono
        } else {
            AacPresentation::Other
        },
        first_elements,
        has_program_config: configuration == 0 && pce.is_some(),
    })
}

impl AudioConfigBits<'_> {
    fn skip(&mut self, count: usize) -> Option<()> {
        let end = self.offset.checked_add(count)?;
        if end > self.bytes.len() * 8 {
            return None;
        }
        self.offset = end;
        Some(())
    }
    fn align(&mut self) -> Option<()> {
        self.skip((8 - self.offset % 8) % 8)
    }
    fn symbol(&mut self, book: usize) -> Option<usize> {
        let tree = HUFFMAN[book];
        let mut node = 0;
        loop {
            let next = tree[node][self.read(1)? as usize];
            if next < 0 {
                return Some((-1 - next) as usize);
            }
            node = next as usize;
        }
    }
}

struct Window {
    short: bool,
    bands: usize,
    offsets: &'static [usize],
    groups: Vec<usize>,
}

impl Window {
    fn read(bits: &mut AudioConfigBits<'_>, frequency: u8) -> Option<Self> {
        if bits.read(1)? != 0 {
            return None;
        }
        let short = bits.read(2)? == 2;
        bits.skip(1)?;
        let bands = bits.read(if short { 4 } else { 6 })? as usize;
        let mut groups = vec![1];
        if short {
            for _ in 1..8 {
                if bits.read(1)? != 0 {
                    *groups.last_mut()? += 1;
                } else {
                    groups.push(1);
                }
            }
        } else if bits.read(1)? != 0 {
            return None;
        }
        let (long, small) = match frequency {
            0 | 1 => (LONG_96K, SHORT_64K),
            2 => (LONG_64K, SHORT_64K),
            3 | 4 => (LONG_48K, SHORT_48K),
            5 => (LONG_32K, SHORT_48K),
            6 | 7 => (LONG_24K, SHORT_24K),
            8..=10 => (LONG_16K, SHORT_16K),
            11 => (LONG_8K, SHORT_8K),
            _ => return None,
        };
        let offsets = if short { small } else { long };
        offsets.get(bands)?;
        Some(Self {
            short,
            bands,
            offsets,
            groups,
        })
    }
}

fn channel(
    bits: &mut AudioConfigBits<'_>,
    frequency: u8,
    shared: Option<&Window>,
    intensity_allowed: bool,
) -> Option<()> {
    let gain = bits.read(8)? as i32;
    let own;
    let window = if let Some(w) = shared {
        w
    } else {
        own = Window::read(bits, frequency)?;
        &own
    };
    let width = if window.short { 3 } else { 5 };
    let escape = (1 << width) - 1;
    let mut books = Vec::with_capacity(window.groups.len() * window.bands);
    for _ in &window.groups {
        let mut band = 0;
        while band < window.bands {
            let book = bits.read(4)? as usize;
            if matches!(book, 12 | 13) || (book >= 14 && !intensity_allowed) {
                return None;
            }
            let mut length = 0;
            loop {
                let part = bits.read(width)?;
                length += part as usize;
                if band + length > window.bands {
                    return None;
                }
                if part != escape {
                    break;
                }
            }
            if length == 0 {
                return None;
            }
            books.resize(books.len() + length, book);
            band += length;
        }
    }
    let mut scale = gain;
    for &book in &books {
        if book != 0 {
            let delta = bits.symbol(0)? as i32 - 60;
            if book <= 11 {
                scale += delta;
                if !(0..=255).contains(&scale) {
                    return None;
                }
            }
        }
    }
    if bits.read(1)? != 0 {
        if window.short {
            return None;
        }
        let count = bits.read(2)? + 1;
        let start = bits.read(6)? as usize;
        let mut pos = *window.offsets.get(start)?;
        for _ in 0..count {
            pos += bits.read(5)? as usize;
            if pos >= 1024 {
                return None;
            }
            bits.skip(4)?;
        }
    }
    if bits.read(1)? != 0 {
        for _ in 0..if window.short { 8 } else { 1 } {
            let filters = bits.read(if window.short { 1 } else { 2 })?;
            if filters == 0 {
                continue;
            }
            let resolution = 3 + bits.read(1)?;
            for _ in 0..filters {
                bits.skip(if window.short { 4 } else { 6 })?;
                let order = bits.read(if window.short { 3 } else { 5 })?;
                if order > if window.short { 7 } else { 12 } {
                    return None;
                }
                if order > 0 {
                    bits.skip(1)?;
                    let compressed = bits.read(1)?;
                    bits.skip((order * (resolution - compressed)) as usize)?;
                }
            }
        }
    }
    if bits.read(1)? != 0 {
        return None;
    }
    let mut books = books.into_iter();
    for &windows in &window.groups {
        for band in 0..window.bands {
            let book = books.next()?;
            if !(1..=11).contains(&book) {
                continue;
            }
            let dimension = if book <= 4 { 4 } else { 2 };
            let lines = (window.offsets[band + 1] - window.offsets[band]) * windows;
            for _ in 0..lines / dimension {
                let mut symbol = bits.symbol(book)?;
                let radix = match book {
                    3 | 4 => 3,
                    7 | 8 => 8,
                    9 | 10 => 13,
                    11 => 17,
                    _ => continue,
                };
                let mut escapes = 0;
                for _ in 0..dimension {
                    let magnitude = symbol % radix;
                    symbol /= radix;
                    if magnitude != 0 {
                        bits.skip(1)?;
                    }
                    if magnitude == 16 {
                        escapes += 1;
                    }
                }
                for _ in 0..escapes {
                    let mut size = 4;
                    while bits.read(1)? != 0 {
                        size += 1;
                        if size > 12 {
                            return None;
                        }
                    }
                    bits.skip(size)?;
                }
            }
        }
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec_signaling::{probe_adts_configuration, AacConfigurationProbe};
    #[test]
    fn complete_elements_determine_presentation_and_decoder_config() {
        let dual = &[
            255, 248, 76, 0, 2, 32, 4, 94, 87, 0, 200, 0, 0, 22, 64, 0, 56,
        ];
        let stereo = &[
            255, 248, 76, 128, 2, 0, 4, 51, 180, 32, 100, 0, 1, 144, 0, 14,
        ];
        assert_eq!(adts_presentation(dual), Some(AacPresentation::DualMono));
        assert_eq!(adts_presentation(stereo), Some(AacPresentation::Other));
        let AacConfigurationProbe::Ready { configuration } =
            probe_adts_configuration(dual, None, false)
        else {
            panic!("2 SCEの構成が必要");
        };
        assert_eq!(configuration.channel_count, 2);
        let asc: Vec<_> = configuration
            .audio_specific_config_hex
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect();
        assert_eq!(
            crate::codec_signaling::configured_front_pair(&asc),
            crate::codec_signaling::frame_presentation_dto(dual).front_pair
        );
        let mut reversed = asc.clone();
        // 入力のタグ順に対応しないPCEを識別する。
        reversed[6] ^= 0x80;
        assert_ne!(
            crate::codec_signaling::configured_front_pair(&asc),
            crate::codec_signaling::configured_front_pair(&reversed)
        );
        assert!(matches!(
            probe_adts_configuration(dual, Some(&asc), false),
            AacConfigurationProbe::Ready { .. }
        ));
        for end in 0..dual.len() {
            assert_eq!(adts_presentation(&dual[..end]), None);
        }
        let mut invalid = *dual;
        *invalid.last_mut().unwrap() = 0;
        assert_eq!(adts_presentation(&invalid), None);
    }
    #[test]
    fn intensity_is_only_valid_on_common_window_right_channel() {
        let vectors: &[(&[u8], &[u8])] = &[
            (
                &[
                    255, 248, 76, 0, 2, 96, 4, 168, 4, 0, 200, 0, 184, 32, 5, 144, 1, 0, 71,
                ],
                &[
                    255, 248, 76, 128, 2, 64, 4, 106, 85, 33, 0, 67, 32, 4, 50, 112, 67, 128,
                ],
            ),
            (
                &[
                    255, 248, 76, 0, 2, 96, 4, 74, 68, 0, 200, 0, 188, 32, 5, 144, 1, 0, 71,
                ],
                &[
                    255, 248, 76, 128, 2, 64, 4, 250, 108, 33, 0, 67, 32, 4, 50, 120, 67, 128,
                ],
            ),
        ];
        for &(single, pair) in vectors {
            assert_eq!(adts_presentation(single), None);
            assert!(matches!(
                probe_adts_configuration(single, None, false),
                AacConfigurationProbe::Pending
            ));
            assert_eq!(adts_presentation(pair), Some(AacPresentation::Other));
        }
        // channel_configuration=0のCPEから2 SCEのASCを合成しない。
        let mut other = [
            255, 248, 76, 128, 2, 0, 4, 51, 180, 32, 100, 0, 1, 144, 0, 14,
        ];
        other[3] = 0;
        assert_eq!(adts_presentation(&other), None);
        assert!(matches!(
            probe_adts_configuration(&other, None, false),
            AacConfigurationProbe::Pending
        ));
    }

    // 48 kHz/36 kbit/sの440 Hz・1100 Hzを個別に符号化し、同じraw blockに収録した。
    #[test]
    fn coded_spectra_follow_element_boundaries_with_and_without_pce() {
        let text = concat!(
            "fff94c0014dffc0108328967a73cff7ffebfcfffa7f3492b833cf77372eb771cea94df91",
            "f29e749d8d9e7e69e79564aef9294b592c35588def5121a8b4ae88b10ce0bd848b245128",
            "c82608c84808ae80014a81000a02b400bd0ca1f993e3dfcfe97bf1e3fff57effffe7d9af",
            "1e78def5cc5e638e6eaa4b1487a21d612f4ebd695d72534a7354d78f5d208a0320b2d140",
            "4c69125222600924104c50009a62404000480b8b01c0"
        );
        let frame: Vec<_> = text
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect();
        assert_eq!(adts_presentation(&frame), Some(AacPresentation::DualMono));
        let mut with_pce = vec![255, 249, 76, 0, 21, 191, 252, 160, 153, 0, 0, 0, 2, 0];
        with_pce.extend_from_slice(&frame[7..]);
        assert_eq!(
            adts_presentation(&with_pce),
            Some(AacPresentation::DualMono)
        );
        let AacConfigurationProbe::Ready { configuration } =
            probe_adts_configuration(&with_pce, None, false)
        else {
            panic!("帯域内PCEを取得できません");
        };
        assert_eq!(configuration.channel_count, 2);
        // PCEが存在しないSCEを指定した場合は判定しない。
        with_pce[12] ^= 4;
        assert_eq!(adts_presentation(&with_pce), None);
    }
}

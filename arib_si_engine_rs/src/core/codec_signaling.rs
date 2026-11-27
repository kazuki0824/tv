use serde::Serialize;

/// PMT ES記述子から取得した放送事実。復号器の対応可否は含まない。
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodecDescriptorFacts {
    pub avc: Option<AvcSignaling>,
    pub mpeg4_audio_profile_and_level: Option<u8>,
    pub audio_extension: Option<Mpeg4AudioExtension>,
    pub malformed: bool,
    pub raw_descriptors_hex: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvcSignaling {
    pub profile_idc: u8,
    pub constraint_flags: u8,
    pub level_idc: u8,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Mpeg4AudioExtension {
    pub profile_level_indications: Vec<u8>,
    pub audio_specific_config_hex: Option<String>,
    pub header: Option<AudioSpecificConfigHeader>,
}

/// ASC共通先頭部だけの解釈。codec固有config全体の検証済み状態ではない。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioSpecificConfigHeader {
    pub audio_object_type: u8,
    pub sampling_frequency: u32,
    pub channel_configuration: u8,
    pub extension_sampling_frequency: Option<u32>,
    pub core_audio_object_type: Option<u8>,
    pub channel_count: Option<u8>,
}

fn hex(bytes: &[u8]) -> String {
    crate::ca_descriptor::hex_prefix(bytes, bytes.len())
}

impl CodecDescriptorFacts {
    pub fn observe(&mut self, tag: u8, body: &[u8]) {
        if !matches!(tag, 0x28 | 0x1c | 0x2e) {
            return;
        }
        self.raw_descriptors_hex
            .push_str(&hex(&[tag, body.len() as u8]));
        self.raw_descriptors_hex.push_str(&hex(body));
        let valid = match tag {
            0x28 if body.len() == 4 && body[1] & 3 == 0 && body[3] & 0x1f == 0x1f => install(
                &mut self.avc,
                AvcSignaling {
                    profile_idc: body[0],
                    constraint_flags: body[1],
                    level_idc: body[2],
                },
            ),
            0x1c if body.len() == 1 => install(&mut self.mpeg4_audio_profile_and_level, body[0]),
            0x2e => parse_audio_extension(body)
                .is_some_and(|value| install(&mut self.audio_extension, value)),
            _ => false,
        };
        self.malformed |= !valid;
    }

    pub fn is_resolved(&self) -> bool {
        !self.malformed
            && (self.mpeg4_audio_profile_and_level != Some(0xff) || self.audio_extension.is_some())
            && !matches!(self.audio_candidates(), (Some(left), Some(right))
                if (left == "MPEG-4-ALS") != (right == "MPEG-4-ALS"))
    }

    pub fn audio_codec(&self) -> Option<&'static str> {
        if !self.is_resolved() {
            return None;
        }
        let (base, extension) = self.audio_candidates();
        [base, extension]
            .into_iter()
            .flatten()
            .max_by_key(|codec| audio_codec_rank(codec))
    }

    fn audio_candidates(&self) -> (Option<&'static str>, Option<&'static str>) {
        let extension_codec = self.audio_extension.as_ref().and_then(|extension| {
            if extension
                .profile_level_indications
                .iter()
                .any(|value| matches!(value, 0x3c | 0x5a..=0x5c))
            {
                Some("MPEG-4-ALS")
            } else {
                let profile = extension
                    .profile_level_indications
                    .iter()
                    .filter_map(|value| match value {
                        0x28..=0x2b | 0x50..=0x51 => Some("AAC-LC"),
                        0x2c..=0x2f | 0x52..=0x53 => Some("HE-AAC"),
                        0x30..=0x33 | 0x54..=0x55 => Some("HE-AAC-v2"),
                        _ => None,
                    })
                    .max_by_key(|codec| audio_codec_rank(codec));
                let asc = extension
                    .header
                    .and_then(|header| match header.audio_object_type {
                        // 共通先頭部だけでは後続の暗黙SBR/PS拡張が無いと断定しない。
                        2 => Some("AAC"),
                        5 => Some("HE-AAC"),
                        29 => Some("HE-AAC-v2"),
                        36 => Some("MPEG-4-ALS"),
                        _ => None,
                    });
                [profile, asc]
                    .into_iter()
                    .flatten()
                    .max_by_key(|codec| audio_codec_rank(codec))
            }
        });
        let descriptor_codec = match self.mpeg4_audio_profile_and_level {
            Some(0x50..=0x55) => Some("AAC-LC"),
            Some(0x58..=0x5d) => Some("HE-AAC"),
            Some(0x60..=0x65) => Some("HE-AAC-v2"),
            Some(0x98) => Some("MPEG-4-ALS"),
            _ => None,
        };
        (descriptor_codec, extension_codec)
    }

    pub fn profile_level(&self) -> Option<String> {
        let mut facts = Vec::new();
        if let Some(avc) = self.avc {
            facts.push(format!(
                "AVC profile_idc={} constraint_flags={} level_idc={}",
                avc.profile_idc, avc.constraint_flags, avc.level_idc
            ));
        }
        if let Some(value) = self.mpeg4_audio_profile_and_level {
            facts.push(format!("MPEG4_audio_profile_and_level={value}"));
        }
        if let Some(extension) = &self.audio_extension {
            facts.push(format!(
                "audioProfileLevelIndication={:?}",
                extension.profile_level_indications
            ));
        }
        (!facts.is_empty()).then(|| facts.join(";"))
    }
}

fn audio_codec_rank(codec: &str) -> u8 {
    match codec {
        "AAC" => 0,
        "AAC-LC" => 1,
        "HE-AAC" => 2,
        "HE-AAC-v2" => 3,
        _ => 4,
    }
}

fn install<T: Eq>(slot: &mut Option<T>, value: T) -> bool {
    match slot {
        Some(current) => *current == value,
        None => {
            *slot = Some(value);
            true
        }
    }
}

fn parse_audio_extension(body: &[u8]) -> Option<Mpeg4AudioExtension> {
    let flags = *body.first()?;
    if flags & 0x70 != 0x70 {
        return None;
    }
    let end = 1 + usize::from(flags & 0x0f);
    let profiles = body.get(1..end)?.to_vec();
    let asc = if flags & 0x80 != 0 {
        let size = usize::from(*body.get(end)?);
        if size == 0 || end + 1 + size != body.len() {
            return None;
        }
        Some(body.get(end + 1..)?)
    } else {
        if end != body.len() {
            return None;
        }
        None
    };
    let header = match asc {
        Some(bytes) => Some(parse_asc_header(bytes)?),
        None => None,
    };
    let als_profile = profiles
        .iter()
        .any(|value| matches!(value, 0x3c | 0x5a..=0x5c));
    let aac_profile = profiles
        .iter()
        .any(|value| matches!(value, 0x28..=0x33 | 0x50..=0x55));
    if (als_profile && (aac_profile || header.is_some_and(|header| header.audio_object_type != 36)))
        || (aac_profile && header.is_some_and(|header| header.audio_object_type == 36))
    {
        return None;
    }
    Some(Mpeg4AudioExtension {
        profile_level_indications: profiles,
        audio_specific_config_hex: asc.map(hex),
        header,
    })
}

fn parse_asc_header(bytes: &[u8]) -> Option<AudioSpecificConfigHeader> {
    let mut bits = AudioConfigBits { bytes, offset: 0 };
    let audio_object_type = bits.object_type()?;
    let sampling_frequency = bits.frequency()?;
    let channel_configuration = bits.read(4)? as u8;
    let (extension_sampling_frequency, core_audio_object_type) =
        if matches!(audio_object_type, 5 | 29) {
            (Some(bits.frequency()?), Some(bits.object_type()?))
        } else {
            (None, None)
        };
    let core_type = core_audio_object_type.unwrap_or(audio_object_type);
    let channel_count = if core_type == 2 && channel_configuration == 0 {
        if bits.read(3) == Some(0) {
            parse_program_config(&mut bits)
                .filter(|config| {
                    config.object_type == core_type
                        && audio_frequency(config.frequency_index) == Some(sampling_frequency)
                })
                .map(|config| config.channel_count)
        } else {
            None
        }
    } else if matches!(core_type, 2 | 5 | 29) {
        channel_count_for_configuration(channel_configuration)
    } else {
        None
    };
    Some(AudioSpecificConfigHeader {
        audio_object_type,
        sampling_frequency,
        channel_configuration,
        extension_sampling_frequency,
        core_audio_object_type,
        channel_count,
    })
}

fn audio_frequency(index: u8) -> Option<u32> {
    const VALUES: [u32; 13] = [
        96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350,
    ];
    VALUES.get(usize::from(index)).copied()
}

fn channel_count_for_configuration(configuration: u8) -> Option<u8> {
    match configuration {
        1..=6 => Some(configuration),
        7 | 12 | 14 => Some(8),
        11 => Some(7),
        13 => Some(24),
        _ => None,
    }
}

struct ProgramConfig {
    element_tags: [u16; 4],
    arib_element_order: Option<[(u8, u8); 2]>,
    front_pair: Option<[u8; 2]>,
    object_type: u8,
    frequency_index: u8,
    channel_count: u8,
    fields_start: usize,
    fields_end: usize,
    comment_range: std::ops::Range<usize>,
}

/// ARIBで用いるPCE。ESとASCの双方が同じ構文処理を使う。
fn parse_program_config(bits: &mut AudioConfigBits<'_>) -> Option<ProgramConfig> {
    let fields_start = bits.offset;
    bits.read(4)?;
    let object_type = bits.read(2)? as u8 + 1;
    let frequency_index = bits.read(4)? as u8;
    audio_frequency(frequency_index)?;
    let front = bits.read(4)?;
    let side = bits.read(4)?;
    let back = bits.read(4)?;
    let lfe = bits.read(2)?;
    let associated = bits.read(3)?;
    if bits.read(4)? != 0 || bits.read(1)? != 0 || bits.read(1)? != 0 {
        return None;
    }
    if bits.read(1)? != 0 {
        bits.read(3)?;
    }
    let mut channel_count = 0u8;
    let mut tags = std::collections::BTreeSet::new();
    let mut first_elements = [(0, 0); 2];
    for index in 0..front + side + back {
        let pair = bits.read(1)?;
        let tag = bits.read(4)?;
        if let Some(slot) = first_elements.get_mut(index as usize) {
            *slot = (pair as u8, tag as u8);
        }
        if !tags.insert((pair, tag)) {
            return None;
        }
        channel_count += if pair == 1 { 2 } else { 1 };
    }
    for _ in 0..lfe {
        if !tags.insert((3, bits.read(4)?)) {
            return None;
        }
        channel_count += 1;
    }
    for _ in 0..associated {
        bits.read(4)?;
    }
    if channel_count == 0 {
        return None;
    }
    let fields_end = bits.offset;
    bits.offset = bits.offset.checked_add(7)? / 8 * 8;
    let comment_start = bits.offset / 8;
    let comment_count = bits.read(8)? as usize;
    let comment_end = comment_start.checked_add(1 + comment_count)?;
    bits.bytes.get(comment_start..comment_end)?;
    bits.offset = comment_end * 8;
    let mut element_tags = [0u16; 4];
    for (kind, tag) in tags {
        element_tags[kind as usize] |= 1 << tag;
    }
    let front_pair = (front == 2 && side + back + lfe == 0 && element_tags[0].count_ones() == 2)
        .then(|| first_elements.map(|(_, tag)| tag));
    // channel_configuration=0では、主副の前方2 SCEまたは前方CPE→後方要素を照合できる。
    let arib_element_order = (front_pair.is_some()
        || (front == 1 && side == 0 && back == 1 && lfe == 0 && first_elements[0].0 == 1))
        .then_some(first_elements);
    Some(ProgramConfig {
        element_tags,
        arib_element_order,
        front_pair,
        object_type,
        frequency_index,
        channel_count,
        fields_start,
        fields_end,
        comment_range: comment_start..comment_end,
    })
}

#[derive(Debug)]
pub enum AacConfigurationProbe {
    Pending,
    Invalid { reason: &'static str },
    Ready { configuration: AacAdtsConfiguration },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AacAdtsConfiguration {
    pub audio_object_type: u8,
    pub sampling_frequency: u32,
    pub extension_sampling_frequency: Option<u32>,
    pub channel_configuration: u8,
    pub channel_count: u8,
    pub audio_specific_config_hex: String,
}

/// 完全AUを再構成しない有限の構成probe。PCEがないframeは宣言長で送る。
pub fn probe_adts_configuration(
    bytes: &[u8],
    supplied_asc: Option<&[u8]>,
) -> AacConfigurationProbe {
    let mut cursor = 0usize;
    while let Some(header) = bytes.get(cursor..).and_then(|tail| tail.get(..7)) {
        if header[0] != 0xff || header[1] & 0xf6 != 0xf0 {
            cursor += 1;
            continue;
        }
        let header_size = if header[1] & 1 == 0 { 9 } else { 7 };
        let object_type = (header[2] >> 6) + 1;
        let frequency_index = (header[2] >> 2) & 15;
        let Some(frequency) = audio_frequency(frequency_index) else {
            cursor += 1;
            continue;
        };
        let configuration = ((header[2] & 1) << 2) | (header[3] >> 6);
        let length = (usize::from(header[3] & 3) << 11)
            | (usize::from(header[4]) << 3)
            | usize::from(header[5] >> 5);
        if length < header_size {
            cursor += 1;
            continue;
        }
        if object_type != 2 || header[6] & 3 != 0 {
            return AacConfigurationProbe::Invalid {
                reason: "ADTSのobject typeまたはraw data block数が未対応です",
            };
        }
        let frame_end = cursor + length;
        if cursor + header_size > bytes.len() {
            return AacConfigurationProbe::Pending;
        }
        if let Some(asc) = supplied_asc {
            let Some(parsed) = parse_asc_header(asc) else {
                return AacConfigurationProbe::Invalid {
                    reason: "AudioSpecificConfigが不正です",
                };
            };
            if !matches!(parsed.audio_object_type, 2 | 5 | 29)
                || parsed
                    .core_audio_object_type
                    .unwrap_or(parsed.audio_object_type)
                    != 2
                || parsed.sampling_frequency != frequency
                || parsed.channel_configuration != configuration
            {
                return AacConfigurationProbe::Invalid {
                    reason: "AudioSpecificConfigとADTSが不一致です",
                };
            }
            let Some(channel_count) = parsed.channel_count else {
                return AacConfigurationProbe::Invalid {
                    reason: "AudioSpecificConfigのPCEまたはchannel構成が不正です",
                };
            };
            return AacConfigurationProbe::Ready {
                configuration: AacAdtsConfiguration {
                    audio_object_type: parsed.audio_object_type,
                    sampling_frequency: frequency,
                    extension_sampling_frequency: parsed.extension_sampling_frequency,
                    channel_configuration: configuration,
                    channel_count,
                    audio_specific_config_hex: hex(asc),
                },
            };
        }
        let prefix = [
            (object_type << 3) | (frequency_index >> 1),
            ((frequency_index & 1) << 7) | (configuration << 3),
        ];
        let (channel_count, config_bytes) =
            if let Some(count) = channel_count_for_configuration(configuration) {
                (count, prefix.to_vec())
            } else {
                let payload = &bytes[cursor + header_size..frame_end.min(bytes.len())];
                let mut bits = AudioConfigBits {
                    bytes: payload,
                    offset: 0,
                };
                if bits.read(3) != Some(5) {
                    if frame_end > bytes.len() {
                        return AacConfigurationProbe::Pending;
                    }
                    if let Some(aac_elements::ParsedAacFrame {
                        presentation: AacPresentation::DualMono,
                        first_elements,
                        ..
                    }) = aac_elements::parse_frame(&bytes[cursor..frame_end])
                    {
                        let tags = first_elements.map(|(_, tag)| tag);
                        // 確認した順序の2 SCEを前方要素としてPCEに記録する。
                        let mut config = prefix.to_vec();
                        let mut index = 0;
                        for (value, width) in [
                            (0, 4),
                            (1, 2),
                            (frequency_index, 4),
                            (2, 4),
                            (0, 4),
                            (0, 4),
                            (0, 2),
                            (0, 3),
                            (0, 4),
                            (0, 3),
                            (tags[0], 5),
                            (tags[1], 5),
                        ] {
                            for bit in (0..width).rev() {
                                if index % 8 == 0 {
                                    config.push(0);
                                }
                                config[2 + index / 8] |= ((value >> bit) & 1) << (7 - index % 8);
                                index += 1;
                            }
                        }
                        config.push(0);
                        return AacConfigurationProbe::Ready {
                            configuration: AacAdtsConfiguration {
                                audio_object_type: object_type,
                                sampling_frequency: frequency,
                                extension_sampling_frequency: None,
                                channel_configuration: 0,
                                channel_count: 2,
                                audio_specific_config_hex: hex(&config),
                            },
                        };
                    }
                    cursor = frame_end;
                    continue;
                }
                let Some(pce) = parse_program_config(&mut bits) else {
                    return if frame_end > bytes.len() {
                        AacConfigurationProbe::Pending
                    } else {
                        AacConfigurationProbe::Invalid {
                            reason: "ADTSのPCEが不正です",
                        }
                    };
                };
                if pce.object_type != object_type || pce.frequency_index != frequency_index {
                    return AacConfigurationProbe::Invalid {
                        reason: "PCEのprofileまたは周波数がADTSと不一致です",
                    };
                }
                let mut config = prefix.to_vec();
                let mut output_bit = 16usize;
                for source_bit in pce.fields_start..pce.fields_end {
                    if output_bit / 8 == config.len() {
                        config.push(0);
                    }
                    let value = (payload[source_bit / 8] >> (7 - source_bit % 8)) & 1;
                    config[output_bit / 8] |= value << (7 - output_bit % 8);
                    output_bit += 1;
                }
                // ASC基準のbyte alignment後、元のcomment長とbytesを保つ。
                config.extend_from_slice(&payload[pce.comment_range]);
                (pce.channel_count, config)
            };
        return AacConfigurationProbe::Ready {
            configuration: AacAdtsConfiguration {
                audio_object_type: object_type,
                sampling_frequency: frequency,
                extension_sampling_frequency: None,
                channel_configuration: configuration,
                channel_count,
                audio_specific_config_hex: hex(&config_bytes),
            },
        };
    }
    AacConfigurationProbe::Pending
}

#[path = "codec_signaling/aac_elements.rs"]
mod aac_elements;
#[path = "codec_signaling/aac_tables.rs"]
mod aac_tables;
fn lc_asc_program_config(asc: &[u8]) -> Option<ProgramConfig> {
    let mut bits = AudioConfigBits {
        bytes: asc,
        offset: 0,
    };
    if bits.object_type()? != 2 {
        return None;
    }
    let frequency = bits.frequency()?;
    if bits.read(4)? != 0 || bits.read(3)? != 0 {
        return None;
    }
    let config = parse_program_config(&mut bits)?;
    if config.comment_range.end != asc.len()
        || config.object_type != 2
        || audio_frequency(config.frequency_index)? != frequency
    {
        return None;
    }
    Some(config)
}

pub fn configured_front_pair(asc: &[u8]) -> Option<i32> {
    let config = lc_asc_program_config(asc)?;
    Some(front_pair_key(config.frequency_index, config.front_pair?))
}

/// PCE非再送時も、完全なraw data blockの周波数と要素の配置が既存ASCに一致することを確認する。
pub fn matches_inherited_aac_configuration(frame: &[u8], asc: &[u8]) -> Option<bool> {
    // ADTSの通常probeで生成する2バイトの固定構成AAC LC ASC。
    if asc.len() == 2 && asc[1] & 0x07 == 0 {
        let fixed = parse_asc_header(asc)?;
        if fixed.audio_object_type != 2 || !(1..=7).contains(&fixed.channel_configuration) {
            return None;
        }
        aac_elements::parse_frame(frame)?;
        let channel_configuration = ((frame[2] & 1) << 2) | (frame[3] >> 6);
        let frequency = audio_frequency((frame[2] >> 2) & 15)?;
        return Some(
            channel_configuration == fixed.channel_configuration
                && frequency == fixed.sampling_frequency,
        );
    }
    let config = lc_asc_program_config(asc)?;
    let parsed = aac_elements::parse_frame(frame)?;
    let header = frame[2];
    if parsed.has_program_config
        || ((header & 1) | (frame[3] >> 6)) != 0
        || (header >> 2) & 15 != config.frequency_index
        || parsed.element_tags != config.element_tags
    {
        return Some(false);
    }
    Some(parsed.first_elements == config.arib_element_order?)
}

/// ARIB STD-B32 Part 2 5.2.3(3)(d): PCEコメントは構成の比較に使わない。
pub fn same_aac_decoder_configuration(first: &[u8], second: &[u8]) -> bool {
    if first == second {
        return true;
    }
    match (lc_asc_program_config(first), lc_asc_program_config(second)) {
        (Some(a), Some(b)) => first[..a.comment_range.start] == second[..b.comment_range.start],
        _ => false,
    }
}

fn front_pair_key(frequency: u8, tags: [u8; 2]) -> i32 {
    ((0x900 | (i32::from(frequency) << 4)) << 8) | (i32::from(tags[0]) << 4) | i32::from(tags[1])
}

pub fn frame_presentation_dto(frame: &[u8]) -> crate::codec_probe_dto::AacFramePresentationDto {
    use crate::codec_probe_dto::{AacFramePresentationDto, AacProbeStatusDto};
    if frame.len() < 7 {
        return AacFramePresentationDto::unresolved(AacProbeStatusDto::Pending, None);
    }
    let Some((declared_length, configuration)) = aac_elements::adts_frame_header(frame) else {
        return AacFramePresentationDto::unresolved(
            AacProbeStatusDto::Invalid,
            Some("ADTSヘッダが不正です"),
        );
    };
    if frame.len() < declared_length {
        return AacFramePresentationDto::unresolved(AacProbeStatusDto::Pending, None);
    }
    if frame.len() != declared_length {
        return AacFramePresentationDto::unresolved(
            AacProbeStatusDto::Invalid,
            Some("ADTSの宣言長が入力と一致しません"),
        );
    }
    let (dual_mono, front_pair, element_order, has_program_config) =
        match aac_elements::parse_frame(frame) {
            Some(parsed) => {
                let dual_mono = parsed.presentation == AacPresentation::DualMono;
                let front_pair = dual_mono.then(|| {
                    let tags = parsed.first_elements.map(|(_, tag)| tag);
                    front_pair_key((frame[2] >> 2) & 15, tags)
                });
                // 構成0の2要素を、型とタグを各4ビットで並べた比較値にする。
                let element_order = (((frame[2] & 1) | (frame[3] >> 6)) == 0).then(|| {
                    i32::from(u16::from_be_bytes(
                        parsed.first_elements.map(|(kind, tag)| (kind << 4) | tag),
                    ))
                });
                (
                    Some(dual_mono),
                    front_pair,
                    element_order,
                    parsed.has_program_config,
                )
            }
            None => {
                // 主副を確定できなくても、先頭PCEがあれば構成probeに照合を委ねる。
                let payload = if frame[1] & 1 == 0 { 9 } else { 7 };
                (None, None, None, frame[payload] >> 5 == 5)
            }
        };
    AacFramePresentationDto {
        status: AacProbeStatusDto::Ready,
        reason: None,
        configuration: Some(i32::from(configuration)),
        dual_mono,
        front_pair,
        element_order,
        has_program_config,
    }
}

pub use aac_elements::AacPresentation;

struct AudioConfigBits<'a> {
    bytes: &'a [u8],
    offset: usize,
}
impl AudioConfigBits<'_> {
    fn read(&mut self, count: usize) -> Option<u32> {
        let mut value = 0;
        for _ in 0..count {
            value = (value << 1)
                | u32::from((self.bytes.get(self.offset / 8)? >> (7 - self.offset % 8)) & 1);
            self.offset += 1;
        }
        Some(value)
    }
    fn object_type(&mut self) -> Option<u8> {
        let value = self.read(5)? as u8;
        if value == 31 {
            Some(32 + self.read(6)? as u8)
        } else {
            Some(value)
        }
    }
    fn frequency(&mut self) -> Option<u32> {
        let index = self.read(4)?;
        if index == 15 {
            self.read(24).filter(|value| *value != 0)
        } else {
            audio_frequency(index as u8)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DUAL_MONO_ADTS: &[u8] = &[
        0xff, 0xf1, 0x4c, 0, 2, 0x1f, 0xfc, 0xa0, 0x99, 0, 0, 0, 2, 1, 0xab, 0xe0,
    ];
    const DUAL_MONO_ASC: &[u8] = &[0x11, 0x80, 4, 0xc8, 0, 0, 0, 0x10, 1, 0xab];

    #[test]
    fn adts_pce_is_relocated_to_asc_alignment_with_exact_comment() {
        let presentation = frame_presentation_dto(DUAL_MONO_ADTS);
        assert_eq!(presentation.dual_mono, None);
        assert!(presentation.has_program_config);
        let AacConfigurationProbe::Ready { configuration } =
            probe_adts_configuration(DUAL_MONO_ADTS, None)
        else {
            panic!("PCEの構成を取得できません");
        };
        assert_eq!(configuration.channel_count, 2);
        assert_eq!(configuration.channel_configuration, 0);
        assert_eq!(
            configuration.audio_specific_config_hex,
            "118004c80000001001ab"
        );
        let parsed = parse_asc_header(DUAL_MONO_ASC).unwrap();
        assert_eq!(parsed.channel_count, Some(2));
        assert_eq!(parsed.sampling_frequency, 48000);
        let no_comment = &[0x11, 0x80, 4, 0xc8, 0, 0, 0, 0x10, 0];
        assert!(same_aac_decoder_configuration(DUAL_MONO_ASC, no_comment));
        assert_eq!(
            configured_front_pair(DUAL_MONO_ASC),
            Some(front_pair_key(3, [0, 1]))
        );
    }

    #[test]
    fn adts_waits_for_pce_and_accepts_out_of_band_config_without_inventing_channels() {
        let absent = &[0xff, 0xf1, 0x4c, 0, 1, 0x1f, 0xfc, 0xe0];
        assert!(matches!(
            probe_adts_configuration(absent, None),
            AacConfigurationProbe::Pending
        ));
        let mut later = absent.to_vec();
        later.extend_from_slice(DUAL_MONO_ADTS);
        assert!(matches!(
            probe_adts_configuration(&later, None),
            AacConfigurationProbe::Ready { .. }
        ));
        assert!(matches!(
            probe_adts_configuration(&DUAL_MONO_ADTS[..13], None),
            AacConfigurationProbe::Pending
        ));
        let AacConfigurationProbe::Ready { configuration } =
            probe_adts_configuration(absent, Some(DUAL_MONO_ASC))
        else {
            panic!("帯域外PCEの構成を取得できません");
        };
        assert_eq!(configuration.audio_specific_config_hex, hex(DUAL_MONO_ASC));
        assert_eq!(configuration.channel_count, 2);
    }

    #[test]
    fn inherited_aac_distinguishes_fixed_and_program_defined_layouts() {
        let asc = [0x11, 0x80, 0x04, 0xc4, 0x04, 0, 0x21, 0x10, 0];
        let reversed_asc = [0x11, 0x80, 0x04, 0xc4, 0x04, 0, 0x23, 0, 0];
        let both_front = [0x11, 0x80, 0x04, 0xc8, 0, 0, 0x21, 0x10, 0];
        let frame = [
            0xff, 0xf8, 0x4c, 0, 0x02, 0xe0, 0x04, 0, 0, 0x20, 0x64, 0, 0x01, 0x90, 0, 0x02, 0x26,
            0x40, 0, 0x19, 0, 0, 0xe0,
        ];
        let mut reversed = frame;
        reversed[9] = 0x22;
        reversed[16] = 0x06;
        assert_eq!(frame_presentation_dto(&frame).element_order, Some(0x1011));
        assert_eq!(
            frame_presentation_dto(&reversed).element_order,
            Some(0x1110)
        );
        assert_eq!(frame_presentation_dto(&reversed[..22]).element_order, None);
        let stereo_asc = [0x11, 0x90];
        let stereo = [
            0xff, 0xf8, 0x4c, 0x80, 0x02, 0, 0x04, 0x33, 0xb4, 0x20, 0x64, 0, 0x01, 0x90, 0, 0x0e,
        ];
        assert_eq!(
            matches_inherited_aac_configuration(&frame, &stereo_asc),
            Some(false)
        );
        assert_eq!(
            matches_inherited_aac_configuration(&frame[..22], &stereo_asc),
            None
        );
        assert_eq!(
            matches_inherited_aac_configuration(&stereo, &stereo_asc),
            Some(true)
        );
        assert_eq!(
            matches_inherited_aac_configuration(&frame, &[0x11, 0x92]),
            None
        );
        assert_eq!(
            matches_inherited_aac_configuration(&frame, &asc),
            Some(true)
        );
        assert_eq!(
            matches_inherited_aac_configuration(&reversed, &asc),
            Some(false)
        );
        assert_eq!(
            matches_inherited_aac_configuration(&reversed, &reversed_asc),
            Some(true)
        );
        assert_eq!(
            matches_inherited_aac_configuration(&reversed[..22], &asc),
            None
        );
        assert_eq!(
            matches_inherited_aac_configuration(&frame, &both_front),
            None
        );
    }

    #[test]
    fn conflicting_or_truncated_complete_pce_is_invalid() {
        let mut malformed = DUAL_MONO_ADTS.to_vec();
        malformed[8] ^= 0x08;
        assert!(matches!(
            probe_adts_configuration(&malformed, None),
            AacConfigurationProbe::Invalid { .. }
        ));
        let mut duplicate = DUAL_MONO_ADTS.to_vec();
        duplicate[12] = 0;
        assert!(matches!(
            probe_adts_configuration(&duplicate, None),
            AacConfigurationProbe::Invalid { .. }
        ));
        let mut truncated = DUAL_MONO_ADTS[..13].to_vec();
        truncated[4] = 1;
        truncated[5] = 0xbf;
        assert!(matches!(
            probe_adts_configuration(&truncated, None),
            AacConfigurationProbe::Invalid { .. }
        ));
    }

    #[test]
    fn avc_duplicates_preserve_conflict_and_raw_evidence() {
        let mut facts = CodecDescriptorFacts::default();
        facts.observe(0x28, &[100, 0, 40, 0x3f]);
        facts.observe(0x28, &[100, 0, 41, 0x3f]);
        facts.observe(0x28, &[100, 0, 40, 0x3f]);
        assert!(!facts.is_resolved());
        assert_eq!(facts.avc.unwrap().level_idc, 40);
        assert_eq!(
            facts.raw_descriptors_hex,
            "28046400283f28046400293f28046400283f"
        );
    }
    #[test]
    fn audio_profile_namespaces_are_not_interchangeable() {
        let mut base = CodecDescriptorFacts::default();
        base.observe(0x1c, &[0x5a]);
        assert_eq!(base.audio_codec(), Some("HE-AAC"));
        let mut extension = CodecDescriptorFacts::default();
        extension.observe(0x1c, &[0xff]);
        assert!(!extension.is_resolved());
        for profile in [0x3c, 0x5a, 0x5b, 0x5c] {
            let mut facts = extension.clone();
            facts.observe(0x2e, &[0x71, profile]);
            assert_eq!(facts.audio_codec(), Some("MPEG-4-ALS"));
        }
        for (profile, codec) in [
            (0x28, "AAC-LC"),
            (0x2c, "HE-AAC"),
            (0x30, "HE-AAC-v2"),
            (0x50, "AAC-LC"),
            (0x53, "HE-AAC"),
            (0x55, "HE-AAC-v2"),
        ] {
            let mut facts = extension.clone();
            facts.observe(0x2e, &[0x71, profile]);
            assert_eq!(facts.audio_codec(), Some(codec));
        }
        let mut conflict = extension;
        conflict.observe(0x2e, &[0x72, 0x3c, 0x2c]);
        assert!(!conflict.is_resolved());
    }
    #[test]
    fn asc_preserves_bytes_and_bounded_header_without_inventing_config() {
        let mut facts = CodecDescriptorFacts::default();
        facts.observe(0x2e, &[0xf0, 2, 0x11, 0x90]);
        assert_eq!(facts.audio_codec(), Some("AAC"));
        facts.observe(0x1c, &[0x58]);
        assert_eq!(facts.audio_codec(), Some("HE-AAC"));
        let extension = facts.audio_extension.unwrap();
        assert_eq!(extension.audio_specific_config_hex.as_deref(), Some("1190"));
        let header = extension.header.unwrap();
        assert_eq!(
            (header.sampling_frequency, header.channel_configuration),
            (48000, 2)
        );
        for bytes in [&[0xf0][..], &[0xf0, 2, 0x11], &[0x71], &[0x70, 0], &[0x00]] {
            assert!(parse_audio_extension(bytes).is_none());
        }
        assert!(parse_asc_header(&[0xff]).is_none());
        let he = parse_asc_header(&[0x2b, 0x11, 0x88, 0]).unwrap();
        assert_eq!(
            (
                he.audio_object_type,
                he.sampling_frequency,
                he.extension_sampling_frequency,
                he.core_audio_object_type
            ),
            (5, 24000, Some(48000), Some(2))
        );
    }
    #[test]
    fn unsupported_and_malformed_signaling_does_not_become_a_known_profile() {
        let mut facts = CodecDescriptorFacts::default();
        facts.observe(0x1c, &[0x3c]);
        assert_eq!(facts.audio_codec(), None);
        facts.observe(0x28, &[66, 3, 31, 0x3f]);
        assert!(!facts.is_resolved());
        assert!(facts.avc.is_none());
    }
}

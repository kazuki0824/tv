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
    for _ in 0..front + side + back {
        let pair = bits.read(1)?;
        let tag = bits.read(4)?;
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
    Some(ProgramConfig {
        object_type,
        frequency_index,
        channel_count,
        fields_start,
        fields_end,
        comment_range: comment_start..comment_end,
    })
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AacConfigurationProbe {
    Pending,
    Invalid { reason: &'static str },
    Ready { configuration: AacAdtsConfiguration },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
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

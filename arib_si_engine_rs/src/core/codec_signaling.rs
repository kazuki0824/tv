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
    Some(AudioSpecificConfigHeader {
        audio_object_type,
        sampling_frequency,
        channel_configuration,
        extension_sampling_frequency,
        core_audio_object_type,
    })
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
        const FREQUENCIES: [u32; 13] = [
            96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350,
        ];
        let index = self.read(4)?;
        if index == 15 {
            self.read(24).filter(|value| *value != 0)
        } else {
            FREQUENCIES.get(index as usize).copied()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

use crate::ts_core::PesPacket;
use crate::TsInputOrigin;

// This is a FilterRuntime-owned association value, not a second packet pipeline or clock.
// It accepts only complete supported frame sequences and never falls back to PCR/wallclock.
const PTS_MODULUS: u128 = 1_u128 << 33;
const PTS_CLOCK_HZ: u128 = 90_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AudioFrameSignature {
    Adts {
        mpeg_id: u8,
        profile: u8,
        sample_rate_hz: u32,
        channel_configuration: u8,
    },
    MpegAudio {
        version: MpegVersion,
        layer: MpegLayer,
        sample_rate_hz: u32,
        channel_mode: u8,
    },
}

impl AudioFrameSignature {
    const fn sample_rate_hz(self) -> u32 {
        match self {
            Self::Adts { sample_rate_hz, .. } | Self::MpegAudio { sample_rate_hz, .. } => {
                sample_rate_hz
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MpegVersion {
    One,
    Two,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MpegLayer {
    One,
    Two,
    Three,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AudioPayloadTiming {
    signature: AudioFrameSignature,
    sample_count: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AudioTimestampAnchor {
    base_pts_90khz: u64,
    signature: AudioFrameSignature,
    elapsed_samples: u64,
    origin: TsInputOrigin,
}

impl AudioTimestampAnchor {
    fn next_pts_90khz(self) -> u64 {
        let elapsed_ticks = u128::from(self.elapsed_samples) * PTS_CLOCK_HZ
            / u128::from(self.signature.sample_rate_hz());
        ((u128::from(self.base_pts_90khz) + elapsed_ticks) % PTS_MODULUS) as u64
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AudioTimestampAssociationFailure {
    MissingAnchor,
    UnsupportedOrIncompleteFrames,
    UnannouncedParameterChange,
    OriginChange,
    SampleCountOverflow,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct AudioTimestampAssociation {
    anchor: Option<AudioTimestampAnchor>,
}

impl AudioTimestampAssociation {
    pub(crate) fn reset(&mut self) {
        self.anchor = None;
    }

    pub(crate) fn associate(
        &mut self,
        packet: &PesPacket,
        origin: TsInputOrigin,
    ) -> Result<u64, AudioTimestampAssociationFailure> {
        let timing = parse_audio_payload_timing(&packet.payload);
        if let Some(explicit_pts) = packet.pts_90khz {
            self.anchor = timing.and_then(|timing| {
                (timing.sample_count > 0).then_some(AudioTimestampAnchor {
                    base_pts_90khz: explicit_pts,
                    signature: timing.signature,
                    elapsed_samples: timing.sample_count,
                    origin,
                })
            });
            return Ok(explicit_pts);
        }

        let timing = match timing {
            Some(timing) if timing.sample_count > 0 => timing,
            _ => {
                self.reset();
                return Err(AudioTimestampAssociationFailure::UnsupportedOrIncompleteFrames);
            }
        };
        let Some(mut anchor) = self.anchor else {
            return Err(AudioTimestampAssociationFailure::MissingAnchor);
        };
        if anchor.origin != origin {
            self.reset();
            return Err(AudioTimestampAssociationFailure::OriginChange);
        }
        if anchor.signature != timing.signature {
            self.reset();
            return Err(AudioTimestampAssociationFailure::UnannouncedParameterChange);
        }
        let pts_90khz = anchor.next_pts_90khz();
        let Some(elapsed_samples) = anchor.elapsed_samples.checked_add(timing.sample_count) else {
            self.reset();
            return Err(AudioTimestampAssociationFailure::SampleCountOverflow);
        };
        anchor.elapsed_samples = elapsed_samples;
        self.anchor = Some(anchor);
        Ok(pts_90khz)
    }

    pub(crate) fn reset_if_origin(&mut self, origin: TsInputOrigin) {
        if self.anchor.is_some_and(|anchor| anchor.origin == origin) {
            self.reset();
        }
    }
}

fn parse_audio_payload_timing(payload: &[u8]) -> Option<AudioPayloadTiming> {
    if payload.len() < 4 {
        return None;
    }
    if payload.len() >= 7 && payload[0] == 0xff && (payload[1] & 0xf6) == 0xf0 {
        parse_adts_payload_timing(payload)
    } else {
        parse_mpeg_audio_payload_timing(payload)
    }
}

fn parse_adts_payload_timing(payload: &[u8]) -> Option<AudioPayloadTiming> {
    let mut offset = 0usize;
    let mut signature = None;
    let mut sample_count = 0u64;
    while offset < payload.len() {
        let header = payload.get(offset..offset.checked_add(7)?)?;
        if header[0] != 0xff || (header[1] & 0xf6) != 0xf0 {
            return None;
        }
        let mpeg_id = (header[1] >> 3) & 0x01;
        let protection_absent = (header[1] & 0x01) != 0;
        let header_len = 9;
        let profile = (header[2] >> 6) & 0x03;
        if mpeg_id != 1 || protection_absent || profile != 1 {
            return None;
        }
        let frequency_index = usize::from((header[2] >> 2) & 0x0f);
        let sample_rate_hz = match frequency_index {
            0 => 96_000,
            3 => 48_000,
            4 => 44_100,
            5 => 32_000,
            6 => 24_000,
            7 => 22_050,
            8 => 16_000,
            _ => return None,
        };
        let channel_configuration = ((header[2] & 0x01) << 2) | (header[3] >> 6);
        let frame_len = (usize::from(header[3] & 0x03) << 11)
            | (usize::from(header[4]) << 3)
            | usize::from(header[5] >> 5);
        let buffer_fullness = (u16::from(header[5] & 0x1f) << 6) | u16::from(header[6] >> 2);
        if frame_len < header_len || buffer_fullness == 0x07ff || (header[6] & 0x03) != 0 {
            return None;
        }
        let frame_end = offset.checked_add(frame_len)?;
        if frame_end > payload.len() {
            return None;
        }
        let current_signature = AudioFrameSignature::Adts {
            mpeg_id,
            profile,
            sample_rate_hz,
            channel_configuration,
        };
        if signature.is_some_and(|stored| stored != current_signature) {
            return None;
        }
        signature.get_or_insert(current_signature);
        sample_count = sample_count.checked_add(1024)?;
        offset = frame_end;
    }
    Some(AudioPayloadTiming {
        signature: signature?,
        sample_count,
    })
}

fn parse_mpeg_audio_payload_timing(payload: &[u8]) -> Option<AudioPayloadTiming> {
    let mut offset = 0usize;
    let mut signature = None;
    let mut sample_count = 0u64;
    while offset < payload.len() {
        let header = payload.get(offset..offset.checked_add(4)?)?;
        if header[0] != 0xff || (header[1] & 0xe0) != 0xe0 {
            return None;
        }
        let version = match (header[1] >> 3) & 0x03 {
            0b11 => MpegVersion::One,
            0b10 => MpegVersion::Two,
            _ => return None,
        };
        let layer = match (header[1] >> 1) & 0x03 {
            0b11 => MpegLayer::One,
            0b10 => MpegLayer::Two,
            0b01 => MpegLayer::Three,
            _ => return None,
        };
        let bitrate_index = usize::from(header[2] >> 4);
        let sample_rate_index = usize::from((header[2] >> 2) & 0x03);
        if !(1..=14).contains(&bitrate_index) || sample_rate_index >= 3 {
            return None;
        }
        let bitrate_kbps = mpeg_audio_bitrate_kbps(version, layer, bitrate_index)?;
        let sample_rate_hz = mpeg_audio_sample_rate_hz(version, sample_rate_index)?;
        let padding = if (header[2] & 0x02) != 0 { 1usize } else { 0 };
        let (frame_len, samples_per_frame) = match layer {
            MpegLayer::One => (
                ((12usize.checked_mul(bitrate_kbps)?.checked_mul(1000)?
                    / usize::try_from(sample_rate_hz).ok()?)
                .checked_add(padding)?)
                .checked_mul(4)?,
                384u64,
            ),
            MpegLayer::Two => (
                (144usize.checked_mul(bitrate_kbps)?.checked_mul(1000)?
                    / usize::try_from(sample_rate_hz).ok()?)
                .checked_add(padding)?,
                1152u64,
            ),
            MpegLayer::Three => match version {
                MpegVersion::One => (
                    (144usize.checked_mul(bitrate_kbps)?.checked_mul(1000)?
                        / usize::try_from(sample_rate_hz).ok()?)
                    .checked_add(padding)?,
                    1152u64,
                ),
                MpegVersion::Two => (
                    (72usize.checked_mul(bitrate_kbps)?.checked_mul(1000)?
                        / usize::try_from(sample_rate_hz).ok()?)
                    .checked_add(padding)?,
                    576u64,
                ),
            },
        };
        if frame_len < 4 {
            return None;
        }
        let frame_end = offset.checked_add(frame_len)?;
        if frame_end > payload.len() {
            return None;
        }
        let current_signature = AudioFrameSignature::MpegAudio {
            version,
            layer,
            sample_rate_hz,
            channel_mode: header[3] >> 6,
        };
        if signature.is_some_and(|stored| stored != current_signature) {
            return None;
        }
        signature.get_or_insert(current_signature);
        sample_count = sample_count.checked_add(samples_per_frame)?;
        offset = frame_end;
    }
    Some(AudioPayloadTiming {
        signature: signature?,
        sample_count,
    })
}

fn mpeg_audio_sample_rate_hz(version: MpegVersion, index: usize) -> Option<u32> {
    let rates = match version {
        MpegVersion::One => [44_100, 48_000, 32_000],
        MpegVersion::Two => [22_050, 24_000, 16_000],
    };
    rates.get(index).copied()
}

fn mpeg_audio_bitrate_kbps(version: MpegVersion, layer: MpegLayer, index: usize) -> Option<usize> {
    const MPEG1_LAYER1: [usize; 14] = [
        32, 64, 96, 128, 160, 192, 224, 256, 288, 320, 352, 384, 416, 448,
    ];
    const MPEG1_LAYER2: [usize; 14] = [
        32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384,
    ];
    const MPEG1_LAYER3: [usize; 14] = [
        32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
    ];
    const MPEG2_LAYER1: [usize; 14] = [
        32, 48, 56, 64, 80, 96, 112, 128, 144, 160, 176, 192, 224, 256,
    ];
    const MPEG2_LAYER2_OR_3: [usize; 14] =
        [8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160];
    let table = match (version, layer) {
        (MpegVersion::One, MpegLayer::One) => &MPEG1_LAYER1,
        (MpegVersion::One, MpegLayer::Two) => &MPEG1_LAYER2,
        (MpegVersion::One, MpegLayer::Three) => &MPEG1_LAYER3,
        (MpegVersion::Two, MpegLayer::One) => &MPEG2_LAYER1,
        (MpegVersion::Two, MpegLayer::Two | MpegLayer::Three) => &MPEG2_LAYER2_OR_3,
    };
    table.get(index.checked_sub(1)?).copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConfigInputPid;
    use crate::packet_pipeline::PacketPid;

    const ORIGIN: TsInputOrigin = TsInputOrigin::Frontend {
        frontend_generation: 1,
    };

    fn packet(payload: Vec<u8>, pts_90khz: Option<u64>) -> PesPacket {
        PesPacket {
            pid: PacketPid::from_config_pid(
                ConfigInputPid::validate_tpid(0x0101).expect("valid test PID"),
            ),
            stream_id: 0xc0,
            pts_90khz,
            dts_90khz: None,
            data_alignment_indicator: true,
            raw_bytes: Vec::new(),
            payload,
        }
    }

    fn adts_frame(sample_rate_index: u8, body_len: usize) -> Vec<u8> {
        let frame_len = 9 + body_len;
        let mut frame = vec![0u8; frame_len];
        frame[0] = 0xff;
        frame[1] = 0xf8;
        frame[2] = 0x40 | (sample_rate_index << 2);
        frame[3] = 2 << 6 | ((frame_len >> 11) as u8 & 0x03);
        frame[4] = (frame_len >> 3) as u8;
        frame[5] = ((frame_len & 0x07) as u8) << 5;
        frame[6] = 0;
        frame
    }

    fn mpeg1_layer2_frame() -> Vec<u8> {
        let frame_len = 144 * 128_000 / 48_000;
        let mut frame = vec![0u8; frame_len];
        frame[..4].copy_from_slice(&[0xff, 0xfd, 0x84, 0x00]);
        frame
    }

    #[test]
    fn adts_explicit_anchor_associates_pts_sparse_events_without_changing_provenance() {
        let mut association = AudioTimestampAssociation::default();
        let first = packet(adts_frame(3, 4), Some(90_000));
        let second = packet(adts_frame(3, 4), None);

        assert_eq!(association.associate(&first, ORIGIN), Ok(90_000));
        assert_eq!(association.associate(&second, ORIGIN), Ok(91_920));
    }

    #[test]
    fn adts_rational_duration_accumulates_from_the_explicit_anchor() {
        let mut association = AudioTimestampAssociation::default();
        let first = packet(adts_frame(4, 4), Some((1_u64 << 33) - 1_000));
        let second = packet(adts_frame(4, 4), None);
        let third = packet(adts_frame(4, 4), None);

        assert_eq!(
            association.associate(&first, ORIGIN),
            Ok((1_u64 << 33) - 1_000)
        );
        assert_eq!(association.associate(&second, ORIGIN), Ok(1_089));
        assert_eq!(association.associate(&third, ORIGIN), Ok(3_179));
    }

    #[test]
    fn mpeg_audio_frame_sample_count_associates_the_next_event() {
        let mut association = AudioTimestampAssociation::default();
        let first = packet(mpeg1_layer2_frame(), Some(180_000));
        let second = packet(mpeg1_layer2_frame(), None);

        assert_eq!(association.associate(&first, ORIGIN), Ok(180_000));
        assert_eq!(association.associate(&second, ORIGIN), Ok(182_160));
    }

    #[test]
    fn missing_anchor_and_incomplete_payload_never_create_a_timestamp() {
        let mut association = AudioTimestampAssociation::default();
        assert_eq!(
            association.associate(&packet(adts_frame(3, 4), None), ORIGIN),
            Err(AudioTimestampAssociationFailure::MissingAnchor)
        );

        let explicit = packet(adts_frame(3, 4), Some(90_000));
        assert_eq!(association.associate(&explicit, ORIGIN), Ok(90_000));
        assert_eq!(
            association.associate(&packet(vec![0xff, 0xf8, 0x4c], None), ORIGIN),
            Err(AudioTimestampAssociationFailure::UnsupportedOrIncompleteFrames)
        );
    }

    #[test]
    fn unannounced_sample_rate_change_invalidates_the_anchor() {
        let mut association = AudioTimestampAssociation::default();
        assert_eq!(
            association.associate(&packet(adts_frame(3, 4), Some(90_000)), ORIGIN),
            Ok(90_000)
        );
        assert_eq!(
            association.associate(&packet(adts_frame(4, 4), None), ORIGIN),
            Err(AudioTimestampAssociationFailure::UnannouncedParameterChange)
        );
        assert_eq!(
            association.associate(&packet(adts_frame(3, 4), None), ORIGIN),
            Err(AudioTimestampAssociationFailure::MissingAnchor)
        );
    }

    #[test]
    fn input_origin_change_cannot_reuse_an_anchor() {
        let mut association = AudioTimestampAssociation::default();
        assert_eq!(
            association.associate(&packet(adts_frame(3, 4), Some(90_000)), ORIGIN),
            Ok(90_000)
        );
        assert_eq!(
            association.associate(
                &packet(adts_frame(3, 4), None),
                TsInputOrigin::PlaybackDvr {
                    dvr_id: 1,
                    queue_identity: 2,
                    queue_epoch: 3,
                },
            ),
            Err(AudioTimestampAssociationFailure::OriginChange)
        );
    }
}

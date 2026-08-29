use crate::ts_core::PesPacket;
use crate::TsInputOrigin;

// FilterRuntimeに従属する有限な関連付け状態であり、第二のpacket pipelineやclockではない。
// PES境界を跨ぐframeは最大1個だけ追跡し、PCR/wallclockへfallbackしない。
const PTS_MODULUS: u128 = 1_u128 << 33;
const PTS_CLOCK_HZ: u128 = 90_000;
const MAX_AUDIO_HEADER_BYTES: usize = 7;
const MAX_AUDIO_FRAME_BYTES: usize = (1usize << 13) - 1;

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
struct AudioFrameHeader {
    signature: AudioFrameSignature,
    sample_count: u64,
    frame_len: usize,
    parsed_header_len: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AudioTimestampAnchor {
    base_pts_90khz: u64,
    signature: AudioFrameSignature,
    elapsed_samples: u64,
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
    UnsupportedOrMalformedFrames,
    UnannouncedParameterChange,
    OriginChange,
    SampleCountOverflow,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum AudioFrameResidual {
    Boundary,
    Header {
        bytes: Vec<u8>,
        frame_pts_90khz: u64,
        reanchor: bool,
    },
    Frame {
        remaining_bytes: usize,
        frame_pts_90khz: u64,
    },
}

impl AudioFrameResidual {
    fn frame_pts_90khz(&self) -> Option<u64> {
        match self {
            Self::Boundary => None,
            Self::Header {
                frame_pts_90khz,
                ..
            }
            | Self::Frame {
                frame_pts_90khz,
                ..
            } => Some(*frame_pts_90khz),
        }
    }
}

impl Default for AudioFrameResidual {
    fn default() -> Self {
        Self::Boundary
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AudioFrameHeaderParse {
    NeedMore(usize),
    Complete(AudioFrameHeader),
    Invalid,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum AudioFrameHeaderRead {
    Partial(Vec<u8>),
    Complete(AudioFrameHeader),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct AudioTimestampAssociation {
    anchor: Option<AudioTimestampAnchor>,
    origin: Option<TsInputOrigin>,
    residual: AudioFrameResidual,
}

impl AudioTimestampAssociation {
    pub(crate) fn reset(&mut self) {
        self.anchor = None;
        self.origin = None;
        self.residual = AudioFrameResidual::Boundary;
    }

    pub(crate) fn associate(
        &mut self,
        packet: &PesPacket,
        origin: TsInputOrigin,
    ) -> Result<u64, AudioTimestampAssociationFailure> {
        let result = self.associate_inner(packet, origin);
        if result.is_err() {
            self.reset();
        }
        result
    }

    pub(crate) fn reset_if_origin(&mut self, origin: TsInputOrigin) {
        if self.origin == Some(origin) {
            self.reset();
        }
    }

    fn associate_inner(
        &mut self,
        packet: &PesPacket,
        origin: TsInputOrigin,
    ) -> Result<u64, AudioTimestampAssociationFailure> {
        if packet.payload.is_empty() {
            return Err(AudioTimestampAssociationFailure::UnsupportedOrMalformedFrames);
        }
        if self.origin.is_some_and(|stored| stored != origin) {
            if packet.pts_90khz.is_none() {
                return Err(AudioTimestampAssociationFailure::OriginChange);
            }
            self.reset();
        }
        if self.origin.is_none() {
            if packet.pts_90khz.is_none() {
                return Err(AudioTimestampAssociationFailure::MissingAnchor);
            }
            self.origin = Some(origin);
        }

        let continued_frame_pts_90khz = self.residual.frame_pts_90khz();
        let mut explicit_for_first_start = packet.pts_90khz;
        let mut first_started_frame_pts_90khz = None;
        let mut offset = 0usize;

        while offset < packet.payload.len() {
            match std::mem::take(&mut self.residual) {
                AudioFrameResidual::Boundary => {
                    let reanchor = explicit_for_first_start.is_some();
                    let frame_pts_90khz = match explicit_for_first_start.take() {
                        Some(explicit_pts_90khz) => explicit_pts_90khz,
                        None => self.next_frame_pts_90khz()?,
                    };
                    first_started_frame_pts_90khz.get_or_insert(frame_pts_90khz);
                    self.read_and_commit_frame(
                        Vec::new(),
                        frame_pts_90khz,
                        reanchor,
                        &packet.payload,
                        &mut offset,
                    )?;
                }
                AudioFrameResidual::Header {
                    bytes,
                    frame_pts_90khz,
                    reanchor,
                } => {
                    self.read_and_commit_frame(
                        bytes,
                        frame_pts_90khz,
                        reanchor,
                        &packet.payload,
                        &mut offset,
                    )?;
                }
                AudioFrameResidual::Frame {
                    remaining_bytes,
                    frame_pts_90khz,
                } => {
                    let available = packet.payload.len().checked_sub(offset).ok_or(
                        AudioTimestampAssociationFailure::UnsupportedOrMalformedFrames,
                    )?;
                    let consumed = remaining_bytes.min(available);
                    offset = offset.checked_add(consumed).ok_or(
                        AudioTimestampAssociationFailure::UnsupportedOrMalformedFrames,
                    )?;
                    let remaining_bytes = remaining_bytes.checked_sub(consumed).ok_or(
                        AudioTimestampAssociationFailure::UnsupportedOrMalformedFrames,
                    )?;
                    if remaining_bytes > 0 {
                        self.residual = AudioFrameResidual::Frame {
                            remaining_bytes,
                            frame_pts_90khz,
                        };
                    }
                }
            }
        }

        if explicit_for_first_start.is_some() {
            return Err(AudioTimestampAssociationFailure::UnsupportedOrMalformedFrames);
        }
        packet
            .pts_90khz
            .or(first_started_frame_pts_90khz)
            .or(continued_frame_pts_90khz)
            .ok_or(AudioTimestampAssociationFailure::MissingAnchor)
    }

    fn next_frame_pts_90khz(&self) -> Result<u64, AudioTimestampAssociationFailure> {
        self.anchor
            .map(AudioTimestampAnchor::next_pts_90khz)
            .ok_or(AudioTimestampAssociationFailure::MissingAnchor)
    }

    fn read_and_commit_frame(
        &mut self,
        bytes: Vec<u8>,
        frame_pts_90khz: u64,
        reanchor: bool,
        payload: &[u8],
        offset: &mut usize,
    ) -> Result<(), AudioTimestampAssociationFailure> {
        let header = match read_audio_frame_header(bytes, payload, offset)? {
            AudioFrameHeaderRead::Partial(bytes) => {
                self.residual = AudioFrameResidual::Header {
                    bytes,
                    frame_pts_90khz,
                    reanchor,
                };
                return Ok(());
            }
            AudioFrameHeaderRead::Complete(header) => header,
        };
        self.commit_frame_header(header, frame_pts_90khz, reanchor)?;

        let remaining_bytes = header
            .frame_len
            .checked_sub(header.parsed_header_len)
            .ok_or(AudioTimestampAssociationFailure::UnsupportedOrMalformedFrames)?;
        let available = payload
            .len()
            .checked_sub(*offset)
            .ok_or(AudioTimestampAssociationFailure::UnsupportedOrMalformedFrames)?;
        let consumed = remaining_bytes.min(available);
        *offset = (*offset)
            .checked_add(consumed)
            .ok_or(AudioTimestampAssociationFailure::UnsupportedOrMalformedFrames)?;
        let remaining_bytes = remaining_bytes
            .checked_sub(consumed)
            .ok_or(AudioTimestampAssociationFailure::UnsupportedOrMalformedFrames)?;
        if remaining_bytes > 0 {
            self.residual = AudioFrameResidual::Frame {
                remaining_bytes,
                frame_pts_90khz,
            };
        }
        Ok(())
    }

    fn commit_frame_header(
        &mut self,
        header: AudioFrameHeader,
        frame_pts_90khz: u64,
        reanchor: bool,
    ) -> Result<(), AudioTimestampAssociationFailure> {
        if reanchor {
            self.anchor = Some(AudioTimestampAnchor {
                base_pts_90khz: frame_pts_90khz,
                signature: header.signature,
                elapsed_samples: header.sample_count,
            });
            return Ok(());
        }

        let Some(mut anchor) = self.anchor else {
            return Err(AudioTimestampAssociationFailure::MissingAnchor);
        };
        if anchor.signature != header.signature {
            return Err(AudioTimestampAssociationFailure::UnannouncedParameterChange);
        }
        let Some(elapsed_samples) = anchor.elapsed_samples.checked_add(header.sample_count) else {
            return Err(AudioTimestampAssociationFailure::SampleCountOverflow);
        };
        anchor.elapsed_samples = elapsed_samples;
        self.anchor = Some(anchor);
        Ok(())
    }
}

fn read_audio_frame_header(
    mut bytes: Vec<u8>,
    payload: &[u8],
    offset: &mut usize,
) -> Result<AudioFrameHeaderRead, AudioTimestampAssociationFailure> {
    loop {
        match parse_audio_frame_header(&bytes) {
            AudioFrameHeaderParse::NeedMore(required_len) => {
                if required_len > MAX_AUDIO_HEADER_BYTES || required_len <= bytes.len() {
                    return Err(AudioTimestampAssociationFailure::UnsupportedOrMalformedFrames);
                }
                let available = payload
                    .len()
                    .checked_sub(*offset)
                    .ok_or(AudioTimestampAssociationFailure::UnsupportedOrMalformedFrames)?;
                let required = required_len
                    .checked_sub(bytes.len())
                    .ok_or(AudioTimestampAssociationFailure::UnsupportedOrMalformedFrames)?;
                let consumed = required.min(available);
                let end = (*offset)
                    .checked_add(consumed)
                    .ok_or(AudioTimestampAssociationFailure::UnsupportedOrMalformedFrames)?;
                let chunk = payload
                    .get(*offset..end)
                    .ok_or(AudioTimestampAssociationFailure::UnsupportedOrMalformedFrames)?;
                bytes.extend_from_slice(chunk);
                *offset = end;
                if bytes.len() < required_len {
                    return Ok(AudioFrameHeaderRead::Partial(bytes));
                }
            }
            AudioFrameHeaderParse::Complete(header) => {
                return Ok(AudioFrameHeaderRead::Complete(header));
            }
            AudioFrameHeaderParse::Invalid => {
                return Err(AudioTimestampAssociationFailure::UnsupportedOrMalformedFrames);
            }
        }
    }
}

fn parse_audio_frame_header(bytes: &[u8]) -> AudioFrameHeaderParse {
    if bytes.len() < 2 {
        return AudioFrameHeaderParse::NeedMore(2);
    }
    if bytes[0] != 0xff {
        return AudioFrameHeaderParse::Invalid;
    }
    if (bytes[1] & 0xf6) == 0xf0 {
        parse_adts_frame_header(bytes)
    } else if (bytes[1] & 0xe0) == 0xe0 {
        parse_mpeg_audio_frame_header(bytes)
    } else {
        AudioFrameHeaderParse::Invalid
    }
}

fn parse_adts_frame_header(bytes: &[u8]) -> AudioFrameHeaderParse {
    let Some(header) = bytes.get(..7) else {
        return AudioFrameHeaderParse::NeedMore(7);
    };
    let mpeg_id = (header[1] >> 3) & 0x01;
    let protection_absent = (header[1] & 0x01) != 0;
    let profile = (header[2] >> 6) & 0x03;
    if mpeg_id != 1 || protection_absent || profile != 1 {
        return AudioFrameHeaderParse::Invalid;
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
        _ => return AudioFrameHeaderParse::Invalid,
    };
    let channel_configuration = ((header[2] & 0x01) << 2) | (header[3] >> 6);
    let frame_len = (usize::from(header[3] & 0x03) << 11)
        | (usize::from(header[4]) << 3)
        | usize::from(header[5] >> 5);
    let buffer_fullness = (u16::from(header[5] & 0x1f) << 6) | u16::from(header[6] >> 2);
    if !(9..=MAX_AUDIO_FRAME_BYTES).contains(&frame_len)
        || buffer_fullness == 0x07ff
        || (header[6] & 0x03) != 0
    {
        return AudioFrameHeaderParse::Invalid;
    }
    AudioFrameHeaderParse::Complete(AudioFrameHeader {
        signature: AudioFrameSignature::Adts {
            mpeg_id,
            profile,
            sample_rate_hz,
            channel_configuration,
        },
        sample_count: 1024,
        frame_len,
        parsed_header_len: 7,
    })
}

fn parse_mpeg_audio_frame_header(bytes: &[u8]) -> AudioFrameHeaderParse {
    let Some(header) = bytes.get(..4) else {
        return AudioFrameHeaderParse::NeedMore(4);
    };
    let version = match (header[1] >> 3) & 0x03 {
        0b11 => MpegVersion::One,
        0b10 => MpegVersion::Two,
        _ => return AudioFrameHeaderParse::Invalid,
    };
    let layer = match (header[1] >> 1) & 0x03 {
        0b11 => MpegLayer::One,
        0b10 => MpegLayer::Two,
        0b01 => MpegLayer::Three,
        _ => return AudioFrameHeaderParse::Invalid,
    };
    let bitrate_index = usize::from(header[2] >> 4);
    let sample_rate_index = usize::from((header[2] >> 2) & 0x03);
    if !(1..=14).contains(&bitrate_index) || sample_rate_index >= 3 {
        return AudioFrameHeaderParse::Invalid;
    }
    let Some(bitrate_kbps) = mpeg_audio_bitrate_kbps(version, layer, bitrate_index) else {
        return AudioFrameHeaderParse::Invalid;
    };
    let Some(sample_rate_hz) = mpeg_audio_sample_rate_hz(version, sample_rate_index) else {
        return AudioFrameHeaderParse::Invalid;
    };
    let padding = if (header[2] & 0x02) != 0 { 1usize } else { 0 };
    let Some((frame_len, sample_count)) =
        mpeg_audio_frame_layout(version, layer, bitrate_kbps, sample_rate_hz, padding)
    else {
        return AudioFrameHeaderParse::Invalid;
    };
    if !(4..=MAX_AUDIO_FRAME_BYTES).contains(&frame_len) {
        return AudioFrameHeaderParse::Invalid;
    }
    AudioFrameHeaderParse::Complete(AudioFrameHeader {
        signature: AudioFrameSignature::MpegAudio {
            version,
            layer,
            sample_rate_hz,
            channel_mode: header[3] >> 6,
        },
        sample_count,
        frame_len,
        parsed_header_len: 4,
    })
}

fn mpeg_audio_frame_layout(
    version: MpegVersion,
    layer: MpegLayer,
    bitrate_kbps: usize,
    sample_rate_hz: u32,
    padding: usize,
) -> Option<(usize, u64)> {
    let sample_rate_hz = usize::try_from(sample_rate_hz).ok()?;
    let (coefficient, sample_count) = match (version, layer) {
        (_, MpegLayer::One) => (12usize, 384u64),
        (_, MpegLayer::Two) | (MpegVersion::One, MpegLayer::Three) => (144, 1152),
        (MpegVersion::Two, MpegLayer::Three) => (72, 576),
    };
    let slots = coefficient
        .checked_mul(bitrate_kbps)?
        .checked_mul(1000)?
        .checked_div(sample_rate_hz)?
        .checked_add(padding)?;
    let frame_len = if layer == MpegLayer::One {
        slots.checked_mul(4)?
    } else {
        slots
    };
    Some((frame_len, sample_count))
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
    fn adts_body_residual_crosses_a_pes_boundary_and_keeps_the_first_new_frame_pts() {
        let mut association = AudioTimestampAssociation::default();
        let first_frame = adts_frame(3, 8);
        let second_frame = adts_frame(3, 4);
        let split_at = 12;

        assert_eq!(
            association.associate(
                &packet(first_frame[..split_at].to_vec(), Some(90_000)),
                ORIGIN,
            ),
            Ok(90_000)
        );
        assert!(matches!(
            &association.residual,
            AudioFrameResidual::Frame {
                remaining_bytes: 5,
                frame_pts_90khz: 90_000,
            }
        ));

        let mut continuation = first_frame[split_at..].to_vec();
        continuation.extend_from_slice(&second_frame);
        assert_eq!(
            association.associate(&packet(continuation, None), ORIGIN),
            Ok(91_920)
        );
        assert_eq!(association.residual, AudioFrameResidual::Boundary);
    }

    #[test]
    fn adts_header_residual_is_bounded_and_preserves_exact_sample_count() {
        let mut association = AudioTimestampAssociation::default();
        let first_frame = adts_frame(3, 4);
        let second_frame = adts_frame(3, 8);
        let third_frame = adts_frame(3, 4);
        let mut first_payload = first_frame;
        first_payload.extend_from_slice(&second_frame[..3]);

        assert_eq!(
            association.associate(&packet(first_payload, Some(90_000)), ORIGIN),
            Ok(90_000)
        );
        assert!(matches!(
            &association.residual,
            AudioFrameResidual::Header {
                bytes,
                frame_pts_90khz: 91_920,
                reanchor: false,
            } if bytes.as_slice() == &second_frame[..3]
        ));

        let mut continuation = second_frame[3..].to_vec();
        continuation.extend_from_slice(&third_frame);
        assert_eq!(
            association.associate(&packet(continuation, None), ORIGIN),
            Ok(93_840)
        );
        assert_eq!(association.residual, AudioFrameResidual::Boundary);
    }

    #[test]
    fn a_continuation_only_pes_uses_the_containing_frame_pts() {
        let mut association = AudioTimestampAssociation::default();
        let frame = adts_frame(3, 8);
        let split_at = 12;

        assert_eq!(
            association.associate(
                &packet(frame[..split_at].to_vec(), Some(90_000)),
                ORIGIN,
            ),
            Ok(90_000)
        );
        assert_eq!(
            association.associate(&packet(frame[split_at..].to_vec(), None), ORIGIN),
            Ok(90_000)
        );
        assert_eq!(
            association.associate(&packet(adts_frame(3, 4), None), ORIGIN),
            Ok(91_920)
        );
    }

    #[test]
    fn explicit_pts_after_a_continuation_reanchors_the_first_frame_started_in_the_pes() {
        let mut association = AudioTimestampAssociation::default();
        let first_frame = adts_frame(3, 8);
        let second_frame = adts_frame(3, 4);
        let split_at = 12;

        assert_eq!(
            association.associate(
                &packet(first_frame[..split_at].to_vec(), Some(90_000)),
                ORIGIN,
            ),
            Ok(90_000)
        );
        let mut continuation = first_frame[split_at..].to_vec();
        continuation.extend_from_slice(&second_frame);
        assert_eq!(
            association.associate(&packet(continuation, Some(180_000)), ORIGIN),
            Ok(180_000)
        );
        assert_eq!(
            association.associate(&packet(adts_frame(3, 4), None), ORIGIN),
            Ok(181_920)
        );
    }

    #[test]
    fn mpeg_audio_body_residual_crosses_a_pes_boundary() {
        let mut association = AudioTimestampAssociation::default();
        let first_frame = mpeg1_layer2_frame();
        let second_frame = mpeg1_layer2_frame();
        let split_at = 100;

        assert_eq!(
            association.associate(
                &packet(first_frame[..split_at].to_vec(), Some(180_000)),
                ORIGIN,
            ),
            Ok(180_000)
        );
        let mut continuation = first_frame[split_at..].to_vec();
        continuation.extend_from_slice(&second_frame);
        assert_eq!(
            association.associate(&packet(continuation, None), ORIGIN),
            Ok(182_160)
        );
    }

    #[test]
    fn reset_discards_a_partial_frame_and_requires_a_new_anchor() {
        let mut association = AudioTimestampAssociation::default();
        let frame = adts_frame(3, 8);
        let split_at = 12;

        assert_eq!(
            association.associate(
                &packet(frame[..split_at].to_vec(), Some(90_000)),
                ORIGIN,
            ),
            Ok(90_000)
        );
        association.reset();
        assert_eq!(association.residual, AudioFrameResidual::Boundary);
        assert_eq!(
            association.associate(&packet(frame[split_at..].to_vec(), None), ORIGIN),
            Err(AudioTimestampAssociationFailure::MissingAnchor)
        );
    }

    #[test]
    fn missing_anchor_and_malformed_payload_never_create_a_timestamp() {
        let mut association = AudioTimestampAssociation::default();
        assert_eq!(
            association.associate(&packet(adts_frame(3, 4), None), ORIGIN),
            Err(AudioTimestampAssociationFailure::MissingAnchor)
        );

        let explicit = packet(adts_frame(3, 4), Some(90_000));
        assert_eq!(association.associate(&explicit, ORIGIN), Ok(90_000));
        assert_eq!(
            association.associate(&packet(vec![0x00, 0x00], None), ORIGIN),
            Err(AudioTimestampAssociationFailure::UnsupportedOrMalformedFrames)
        );
        assert_eq!(
            association.associate(&packet(adts_frame(3, 4), None), ORIGIN),
            Err(AudioTimestampAssociationFailure::MissingAnchor)
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

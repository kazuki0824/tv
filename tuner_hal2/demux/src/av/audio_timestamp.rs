use super::AvMediaEventMetadata;
use crate::ts_core::PesPacket;
use crate::TsInputOrigin;

// FilterRuntimeに従属する有限な関連付け状態であり、第二のpacket pipelineやclockではない。
// PES境界を跨ぐframeは最大1個だけ再構成し、PCR/wallclockへfallbackしない。
const PTS_MODULUS: u128 = 1_u128 << 33;
const PTS_CLOCK_HZ: u128 = 90_000;
const MAX_AUDIO_HEADER_BYTES: usize = 7;
const MAX_AUDIO_FRAME_BYTES: usize = (1usize << 13) - 1;
const MAX_COLD_START_BYTES: usize = MAX_AUDIO_FRAME_BYTES * 2 + MAX_AUDIO_HEADER_BYTES;

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
pub(crate) struct AudioMediaFrame {
    pub(crate) payload: Vec<u8>,
    pub(crate) metadata: AvMediaEventMetadata,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AudioFrameInProgress {
    bytes: Vec<u8>,
    expected_len: Option<usize>,
    frame_pts_90khz: u64,
    stream_id: u8,
    is_pts_present: bool,
    dts_90khz: Option<u64>,
    reanchor: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum AudioFrameAssembly {
    Boundary,
    Frame(AudioFrameInProgress),
}

impl Default for AudioFrameAssembly {
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
struct AudioPesChunk {
    stream_id: u8,
    pts_90khz: Option<u64>,
    dts_90khz: Option<u64>,
    data_alignment_indicator: bool,
    payload: Vec<u8>,
}

impl From<PesPacket> for AudioPesChunk {
    fn from(packet: PesPacket) -> Self {
        Self {
            stream_id: packet.stream_id,
            pts_90khz: packet.pts_90khz,
            dts_90khz: packet.dts_90khz,
            data_alignment_indicator: packet.data_alignment_indicator,
            payload: packet.payload,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ColdStartAcquisition {
    chunks: Vec<AudioPesChunk>,
    first_payload_len: usize,
    total_bytes: usize,
}

impl ColdStartAcquisition {
    fn new(chunk: AudioPesChunk) -> Result<Self, AudioTimestampAssociationFailure> {
        let total_bytes = chunk.payload.len();
        if total_bytes > MAX_COLD_START_BYTES {
            return Err(AudioTimestampAssociationFailure::UnsupportedOrMalformedFrames);
        }
        Ok(Self {
            chunks: vec![chunk],
            first_payload_len: total_bytes,
            total_bytes,
        })
    }

    fn probe_with(&self, chunk: &AudioPesChunk) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(
            MAX_COLD_START_BYTES.min(self.total_bytes.saturating_add(chunk.payload.len())),
        );
        for stored in &self.chunks {
            bytes.extend_from_slice(&stored.payload);
        }
        let remaining = MAX_COLD_START_BYTES.saturating_sub(bytes.len());
        bytes.extend_from_slice(&chunk.payload[..chunk.payload.len().min(remaining)]);
        bytes
    }

    fn push(&mut self, chunk: AudioPesChunk) -> Result<(), AudioTimestampAssociationFailure> {
        let total_bytes = self
            .total_bytes
            .checked_add(chunk.payload.len())
            .ok_or(AudioTimestampAssociationFailure::UnsupportedOrMalformedFrames)?;
        if total_bytes > MAX_COLD_START_BYTES {
            return Err(AudioTimestampAssociationFailure::UnsupportedOrMalformedFrames);
        }
        self.total_bytes = total_bytes;
        self.chunks.push(chunk);
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct AudioTimestampAssociation {
    anchor: Option<AudioTimestampAnchor>,
    origin: Option<TsInputOrigin>,
    assembly: AudioFrameAssembly,
    cold_start: Option<ColdStartAcquisition>,
}

impl AudioTimestampAssociation {
    pub(crate) fn reset(&mut self) {
        self.anchor = None;
        self.origin = None;
        self.assembly = AudioFrameAssembly::Boundary;
        self.cold_start = None;
    }

    pub(crate) fn extract(
        &mut self,
        packet: PesPacket,
        origin: TsInputOrigin,
    ) -> Result<Vec<AudioMediaFrame>, AudioTimestampAssociationFailure> {
        let result = self.extract_inner(packet, origin);
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

    fn extract_inner(
        &mut self,
        packet: PesPacket,
        origin: TsInputOrigin,
    ) -> Result<Vec<AudioMediaFrame>, AudioTimestampAssociationFailure> {
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

        let chunk = AudioPesChunk::from(packet);
        if let Some(cold_start) = self.cold_start.as_ref() {
            return self.continue_cold_start(chunk, cold_start.first_payload_len);
        }

        let acquiring_initial_boundary =
            self.anchor.is_none() && matches!(self.assembly, AudioFrameAssembly::Boundary);
        if acquiring_initial_boundary && !chunk.data_alignment_indicator {
            return self.start_cold_start(chunk);
        }
        self.process_chunk(chunk, 0)
    }

    fn next_frame_pts_90khz(&self) -> Result<u64, AudioTimestampAssociationFailure> {
        self.anchor
            .map(AudioTimestampAnchor::next_pts_90khz)
            .ok_or(AudioTimestampAssociationFailure::MissingAnchor)
    }

    fn start_cold_start(
        &mut self,
        chunk: AudioPesChunk,
    ) -> Result<Vec<AudioMediaFrame>, AudioTimestampAssociationFailure> {
        match cold_start_decision(&chunk.payload, chunk.payload.len()) {
            ColdStartDecision::Confirmed(offset) => self.process_chunk(chunk, offset),
            ColdStartDecision::Pending => {
                self.cold_start = Some(ColdStartAcquisition::new(chunk)?);
                Ok(Vec::new())
            }
            ColdStartDecision::Invalid => {
                Err(AudioTimestampAssociationFailure::UnsupportedOrMalformedFrames)
            }
        }
    }

    fn continue_cold_start(
        &mut self,
        chunk: AudioPesChunk,
        first_payload_len: usize,
    ) -> Result<Vec<AudioMediaFrame>, AudioTimestampAssociationFailure> {
        let decision = {
            let cold_start = self
                .cold_start
                .as_ref()
                .ok_or(AudioTimestampAssociationFailure::UnsupportedOrMalformedFrames)?;
            cold_start_decision(&cold_start.probe_with(&chunk), first_payload_len)
        };
        match decision {
            ColdStartDecision::Confirmed(offset) => {
                let cold_start = self
                    .cold_start
                    .take()
                    .ok_or(AudioTimestampAssociationFailure::UnsupportedOrMalformedFrames)?;
                let mut chunks = cold_start.chunks;
                chunks.push(chunk);
                let mut outputs = Vec::new();
                for (index, stored) in chunks.into_iter().enumerate() {
                    let start_offset = if index == 0 { offset } else { 0 };
                    outputs.extend(self.process_chunk(stored, start_offset)?);
                }
                Ok(outputs)
            }
            ColdStartDecision::Pending => {
                self.cold_start
                    .as_mut()
                    .ok_or(AudioTimestampAssociationFailure::UnsupportedOrMalformedFrames)?
                    .push(chunk)?;
                Ok(Vec::new())
            }
            ColdStartDecision::Invalid => {
                Err(AudioTimestampAssociationFailure::UnsupportedOrMalformedFrames)
            }
        }
    }

    fn process_chunk(
        &mut self,
        chunk: AudioPesChunk,
        mut offset: usize,
    ) -> Result<Vec<AudioMediaFrame>, AudioTimestampAssociationFailure> {
        if offset >= chunk.payload.len() {
            return Err(AudioTimestampAssociationFailure::UnsupportedOrMalformedFrames);
        }
        let mut outputs = Vec::new();
        let mut explicit_for_first_start = chunk.pts_90khz;
        let mut dts_for_first_start = chunk.dts_90khz;

        while offset < chunk.payload.len() {
            if matches!(self.assembly, AudioFrameAssembly::Boundary) {
                let explicit_pts_90khz = explicit_for_first_start.take();
                let reanchor = explicit_pts_90khz.is_some();
                let frame_pts_90khz = match explicit_pts_90khz {
                    Some(pts_90khz) => pts_90khz,
                    None => self.next_frame_pts_90khz()?,
                };
                self.assembly = AudioFrameAssembly::Frame(AudioFrameInProgress {
                    bytes: Vec::new(),
                    expected_len: None,
                    frame_pts_90khz,
                    stream_id: chunk.stream_id,
                    is_pts_present: reanchor,
                    dts_90khz: dts_for_first_start.take(),
                    reanchor,
                });
            }
            self.fill_frame(&chunk.payload, &mut offset, &mut outputs)?;
        }

        if explicit_for_first_start.is_some() {
            return Err(AudioTimestampAssociationFailure::UnsupportedOrMalformedFrames);
        }
        Ok(outputs)
    }

    fn fill_frame(
        &mut self,
        payload: &[u8],
        offset: &mut usize,
        outputs: &mut Vec<AudioMediaFrame>,
    ) -> Result<(), AudioTimestampAssociationFailure> {
        let mut frame = match std::mem::take(&mut self.assembly) {
            AudioFrameAssembly::Boundary => {
                return Err(AudioTimestampAssociationFailure::UnsupportedOrMalformedFrames);
            }
            AudioFrameAssembly::Frame(frame) => frame,
        };

        if frame.expected_len.is_none() {
            loop {
                match parse_audio_frame_header(&frame.bytes) {
                    AudioFrameHeaderParse::NeedMore(required_len) => {
                        if required_len > MAX_AUDIO_HEADER_BYTES
                            || required_len <= frame.bytes.len()
                        {
                            return Err(
                                AudioTimestampAssociationFailure::UnsupportedOrMalformedFrames,
                            );
                        }
                        let available = payload.len().checked_sub(*offset).ok_or(
                            AudioTimestampAssociationFailure::UnsupportedOrMalformedFrames,
                        )?;
                        let required = required_len.checked_sub(frame.bytes.len()).ok_or(
                            AudioTimestampAssociationFailure::UnsupportedOrMalformedFrames,
                        )?;
                        let consumed = required.min(available);
                        let end = (*offset).checked_add(consumed).ok_or(
                            AudioTimestampAssociationFailure::UnsupportedOrMalformedFrames,
                        )?;
                        frame
                            .bytes
                            .extend_from_slice(payload.get(*offset..end).ok_or(
                                AudioTimestampAssociationFailure::UnsupportedOrMalformedFrames,
                            )?);
                        *offset = end;
                        if frame.bytes.len() < required_len {
                            self.assembly = AudioFrameAssembly::Frame(frame);
                            return Ok(());
                        }
                    }
                    AudioFrameHeaderParse::Complete(header) => {
                        if header.parsed_header_len > frame.bytes.len() {
                            return Err(
                                AudioTimestampAssociationFailure::UnsupportedOrMalformedFrames,
                            );
                        }
                        self.commit_frame_header(header, frame.frame_pts_90khz, frame.reanchor)?;
                        frame.expected_len = Some(header.frame_len);
                        break;
                    }
                    AudioFrameHeaderParse::Invalid => {
                        return Err(AudioTimestampAssociationFailure::UnsupportedOrMalformedFrames);
                    }
                }
            }
        }

        let expected_len = frame
            .expected_len
            .ok_or(AudioTimestampAssociationFailure::UnsupportedOrMalformedFrames)?;
        let remaining = expected_len
            .checked_sub(frame.bytes.len())
            .ok_or(AudioTimestampAssociationFailure::UnsupportedOrMalformedFrames)?;
        let available = payload
            .len()
            .checked_sub(*offset)
            .ok_or(AudioTimestampAssociationFailure::UnsupportedOrMalformedFrames)?;
        let consumed = remaining.min(available);
        let end = (*offset)
            .checked_add(consumed)
            .ok_or(AudioTimestampAssociationFailure::UnsupportedOrMalformedFrames)?;
        frame.bytes.extend_from_slice(
            payload
                .get(*offset..end)
                .ok_or(AudioTimestampAssociationFailure::UnsupportedOrMalformedFrames)?,
        );
        *offset = end;
        if frame.bytes.len() < expected_len {
            self.assembly = AudioFrameAssembly::Frame(frame);
            return Ok(());
        }

        outputs.push(AudioMediaFrame {
            payload: frame.bytes,
            metadata: AvMediaEventMetadata {
                stream_id: frame.stream_id,
                is_pts_present: frame.is_pts_present,
                pts_90khz: Some(frame.frame_pts_90khz),
                is_dts_present: frame.dts_90khz.is_some(),
                dts_90khz: frame.dts_90khz,
            },
        });
        self.assembly = AudioFrameAssembly::Boundary;
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ColdStartCandidateValidation {
    Invalid,
    Pending,
    ConfirmedBoundary { next_offset: usize },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ColdStartDecision {
    Invalid,
    Pending,
    Confirmed(usize),
}

// data_alignment_indicator=falseのcold startではpayload先頭をsyncwordと仮定しない。
// 最初のPES内で開始する候補だけを有限探索し、後続の同一signature境界を
// 実際に観測するまでPTS anchorとMediaEvent payloadを公開しない。
fn cold_start_decision(bytes: &[u8], first_payload_len: usize) -> ColdStartDecision {
    let search_len = first_payload_len
        .min(MAX_AUDIO_FRAME_BYTES)
        .min(bytes.len());
    let mut confirmed = None;
    let mut saw_pending = false;
    let mut competing_pending = false;
    for offset in 0..search_len {
        match cold_start_candidate_validation(bytes, offset) {
            ColdStartCandidateValidation::ConfirmedBoundary { next_offset } => {
                if confirmed.replace((offset, next_offset)).is_some() {
                    return ColdStartDecision::Invalid;
                }
            }
            ColdStartCandidateValidation::Pending => {
                saw_pending = true;
                if let Some((confirmed_offset, next_offset)) = confirmed {
                    // 確定検証に使った直後headerは同一frame列であり、別候補ではない。
                    // その後方で開始する未確定候補だけは競合するため確定を保留する。
                    competing_pending |= offset > confirmed_offset && offset != next_offset;
                }
            }
            ColdStartCandidateValidation::Invalid => {}
        }
    }
    match confirmed {
        Some(_) if competing_pending => ColdStartDecision::Pending,
        Some((offset, _)) => ColdStartDecision::Confirmed(offset),
        None if saw_pending => ColdStartDecision::Pending,
        None => ColdStartDecision::Invalid,
    }
}

fn cold_start_candidate_validation(bytes: &[u8], offset: usize) -> ColdStartCandidateValidation {
    let Some(candidate) = bytes.get(offset..) else {
        return ColdStartCandidateValidation::Invalid;
    };
    let header = match parse_audio_frame_header(candidate) {
        AudioFrameHeaderParse::Complete(header) => header,
        AudioFrameHeaderParse::NeedMore(required_len)
            if candidate.first() == Some(&0xff)
                && required_len <= MAX_AUDIO_HEADER_BYTES
                && candidate.len() < required_len =>
        {
            return ColdStartCandidateValidation::Pending;
        }
        AudioFrameHeaderParse::NeedMore(_) | AudioFrameHeaderParse::Invalid => {
            return ColdStartCandidateValidation::Invalid;
        }
    };
    let Some(next_offset) = offset.checked_add(header.frame_len) else {
        return ColdStartCandidateValidation::Invalid;
    };
    if next_offset >= bytes.len() {
        return ColdStartCandidateValidation::Pending;
    }

    let Some(next_candidate) = bytes.get(next_offset..) else {
        return ColdStartCandidateValidation::Invalid;
    };
    match parse_audio_frame_header(next_candidate) {
        AudioFrameHeaderParse::Complete(next_header)
            if next_header.signature == header.signature =>
        {
            ColdStartCandidateValidation::ConfirmedBoundary { next_offset }
        }
        AudioFrameHeaderParse::NeedMore(required_len) => {
            if next_candidate.first() == Some(&0xff)
                && next_candidate.len() < required_len
                && required_len <= MAX_AUDIO_HEADER_BYTES
            {
                ColdStartCandidateValidation::Pending
            } else {
                ColdStartCandidateValidation::Invalid
            }
        }
        AudioFrameHeaderParse::Complete(_) | AudioFrameHeaderParse::Invalid => {
            ColdStartCandidateValidation::Invalid
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

    fn packet_with_alignment(
        payload: Vec<u8>,
        pts_90khz: Option<u64>,
        data_alignment_indicator: bool,
    ) -> PesPacket {
        PesPacket {
            pid: PacketPid::from_config_pid(
                ConfigInputPid::validate_tpid(0x0101).expect("valid test PID"),
            ),
            stream_id: 0xc0,
            pts_90khz,
            dts_90khz: None,
            data_alignment_indicator,
            raw_bytes: Vec::new(),
            payload,
        }
    }

    fn packet(payload: Vec<u8>, pts_90khz: Option<u64>) -> PesPacket {
        packet_with_alignment(payload, pts_90khz, true)
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

    fn pts(frames: &[AudioMediaFrame]) -> Vec<u64> {
        frames
            .iter()
            .map(|frame| frame.metadata.pts_90khz.expect("frame PTS"))
            .collect()
    }

    #[test]
    fn adts_explicit_anchor_associates_pts_sparse_frames_without_changing_provenance() {
        let mut association = AudioTimestampAssociation::default();
        let first = association
            .extract(packet(adts_frame(3, 4), Some(90_000)), ORIGIN)
            .unwrap();
        let second = association
            .extract(packet(adts_frame(3, 4), None), ORIGIN)
            .unwrap();

        assert_eq!(pts(&first), vec![90_000]);
        assert!(first[0].metadata.is_pts_present);
        assert_eq!(pts(&second), vec![91_920]);
        assert!(!second[0].metadata.is_pts_present);
    }

    #[test]
    fn adts_rational_duration_accumulates_from_the_explicit_anchor() {
        let mut association = AudioTimestampAssociation::default();
        let first = association
            .extract(
                packet(adts_frame(4, 4), Some((1_u64 << 33) - 1_000)),
                ORIGIN,
            )
            .unwrap();
        let second = association
            .extract(packet(adts_frame(4, 4), None), ORIGIN)
            .unwrap();
        let third = association
            .extract(packet(adts_frame(4, 4), None), ORIGIN)
            .unwrap();

        assert_eq!(pts(&first), vec![(1_u64 << 33) - 1_000]);
        assert_eq!(pts(&second), vec![1_089]);
        assert_eq!(pts(&third), vec![3_179]);
    }

    #[test]
    fn mpeg_audio_frame_sample_count_associates_the_next_frame() {
        let mut association = AudioTimestampAssociation::default();
        let first = association
            .extract(packet(mpeg1_layer2_frame(), Some(180_000)), ORIGIN)
            .unwrap();
        let second = association
            .extract(packet(mpeg1_layer2_frame(), None), ORIGIN)
            .unwrap();

        assert_eq!(pts(&first), vec![180_000]);
        assert_eq!(pts(&second), vec![182_160]);
    }

    #[test]
    fn cold_start_defers_until_the_next_adts_boundary_is_confirmed() {
        let mut association = AudioTimestampAssociation::default();
        let frame = adts_frame(3, 4);
        let mut first_payload = vec![0x11, 0x22, 0x33, 0x44];
        first_payload.extend_from_slice(&frame);

        let deferred = association
            .extract(
                packet_with_alignment(first_payload, Some(90_000), false),
                ORIGIN,
            )
            .unwrap();
        assert!(deferred.is_empty());

        let confirmed = association
            .extract(packet_with_alignment(frame.clone(), None, false), ORIGIN)
            .unwrap();
        assert_eq!(pts(&confirmed), vec![90_000, 91_920]);
        assert_eq!(confirmed[0].payload, frame);
    }

    #[test]
    fn cold_start_keeps_a_first_au_that_crosses_the_next_pes() {
        let mut association = AudioTimestampAssociation::default();
        let first_frame = adts_frame(3, 8);
        let second_frame = adts_frame(3, 4);
        let split_at = 12;
        let mut first_payload = vec![0x11, 0x22, 0x33, 0x44];
        first_payload.extend_from_slice(&first_frame[..split_at]);

        let deferred = association
            .extract(
                packet_with_alignment(first_payload, Some(90_000), false),
                ORIGIN,
            )
            .unwrap();
        assert!(deferred.is_empty());

        let mut continuation = first_frame[split_at..].to_vec();
        continuation.extend_from_slice(&second_frame);
        let confirmed = association
            .extract(packet_with_alignment(continuation, None, false), ORIGIN)
            .unwrap();
        assert_eq!(pts(&confirmed), vec![90_000, 91_920]);
        assert_eq!(confirmed[0].payload, first_frame);
        assert_eq!(confirmed[1].payload, second_frame);
    }

    #[test]
    fn cold_start_rejects_a_sync_like_prefix_without_a_valid_declared_boundary() {
        let mut association = AudioTimestampAssociation::default();
        let fake_frame = adts_frame(3, 0);
        let real_frame = adts_frame(3, 4);
        let mut payload = fake_frame;
        payload.push(0x00);
        payload.extend_from_slice(&real_frame);
        payload.extend_from_slice(&real_frame);

        let frames = association
            .extract(packet_with_alignment(payload, Some(90_000), false), ORIGIN)
            .unwrap();
        assert_eq!(pts(&frames), vec![90_000, 91_920]);
        assert_eq!(frames[0].payload, real_frame);
    }

    #[test]
    fn cold_start_defers_ambiguous_header_only_candidates_until_true_boundary() {
        let mut association = AudioTimestampAssociation::default();
        let false_candidate = adts_frame(3, 200);
        let first_frame = adts_frame(3, 8);
        let second_frame = adts_frame(3, 4);
        let split_at = 12;
        let mut first_payload = false_candidate[..7].to_vec();
        first_payload.extend_from_slice(&[0x11, 0x22, 0x33, 0x44]);
        first_payload.extend_from_slice(&first_frame[..split_at]);

        let deferred = association
            .extract(
                packet_with_alignment(first_payload, Some(90_000), false),
                ORIGIN,
            )
            .unwrap();
        assert!(deferred.is_empty());
        assert!(association.anchor.is_none());
        assert!(association.cold_start.is_some());

        let mut continuation = first_frame[split_at..].to_vec();
        continuation.extend_from_slice(&second_frame);
        let confirmed = association
            .extract(packet_with_alignment(continuation, None, false), ORIGIN)
            .unwrap();
        assert_eq!(pts(&confirmed), vec![90_000, 91_920]);
        assert_eq!(confirmed[0].payload, first_frame);
        assert_eq!(confirmed[1].payload, second_frame);
    }

    #[test]
    fn cold_start_never_commits_a_confirmed_candidate_while_another_is_pending() {
        let mut association = AudioTimestampAssociation::default();
        let false_first_frame = adts_frame(3, 0);
        let false_continuation_frame = adts_frame(3, 200);
        let real_first_frame = adts_frame(3, 8);
        let real_second_frame = adts_frame(3, 4);
        let split_at = 12;
        let mut first_payload = false_first_frame;
        first_payload.extend_from_slice(&false_continuation_frame[..7]);
        first_payload.extend_from_slice(&[0x11, 0x22, 0x33, 0x44]);
        first_payload.extend_from_slice(&real_first_frame[..split_at]);

        let deferred = association
            .extract(
                packet_with_alignment(first_payload, Some(90_000), false),
                ORIGIN,
            )
            .unwrap();
        assert!(deferred.is_empty());
        assert!(association.anchor.is_none());
        assert!(association.cold_start.is_some());

        let mut continuation = real_first_frame[split_at..].to_vec();
        continuation.extend_from_slice(&real_second_frame);
        assert_eq!(
            association.extract(packet_with_alignment(continuation, None, false), ORIGIN),
            Err(AudioTimestampAssociationFailure::UnsupportedOrMalformedFrames)
        );
        assert_eq!(association, AudioTimestampAssociation::default());
    }

    #[test]
    fn aligned_cold_start_never_scans_past_a_malformed_payload_start() {
        let mut association = AudioTimestampAssociation::default();
        let mut payload = vec![0x11, 0x22, 0x33, 0x44];
        payload.extend_from_slice(&adts_frame(3, 4));

        assert_eq!(
            association.extract(packet_with_alignment(payload, Some(90_000), true), ORIGIN,),
            Err(AudioTimestampAssociationFailure::UnsupportedOrMalformedFrames)
        );
        assert_eq!(association, AudioTimestampAssociation::default());
    }

    #[test]
    fn adts_body_is_reassembled_across_pes_into_exact_frame_ranges() {
        let mut association = AudioTimestampAssociation::default();
        let first_frame = adts_frame(3, 8);
        let second_frame = adts_frame(3, 4);
        let split_at = 12;

        let first = association
            .extract(
                packet(first_frame[..split_at].to_vec(), Some(90_000)),
                ORIGIN,
            )
            .unwrap();
        assert!(first.is_empty());
        assert!(matches!(
            &association.assembly,
            AudioFrameAssembly::Frame(frame)
                if frame.bytes.as_slice() == &first_frame[..split_at]
                    && frame.expected_len == Some(first_frame.len())
        ));

        let mut continuation = first_frame[split_at..].to_vec();
        continuation.extend_from_slice(&second_frame);
        let completed = association
            .extract(packet(continuation, None), ORIGIN)
            .unwrap();
        assert_eq!(pts(&completed), vec![90_000, 91_920]);
        assert_eq!(completed[0].payload, first_frame);
        assert_eq!(completed[1].payload, second_frame);
        assert!(completed[0].metadata.is_pts_present);
        assert!(!completed[1].metadata.is_pts_present);
        assert_eq!(association.assembly, AudioFrameAssembly::Boundary);
    }

    #[test]
    fn adts_header_residual_is_bounded_and_preserves_exact_sample_count() {
        let mut association = AudioTimestampAssociation::default();
        let first_frame = adts_frame(3, 4);
        let second_frame = adts_frame(3, 8);
        let third_frame = adts_frame(3, 4);
        let mut first_payload = first_frame.clone();
        first_payload.extend_from_slice(&second_frame[..3]);

        let first = association
            .extract(packet(first_payload, Some(90_000)), ORIGIN)
            .unwrap();
        assert_eq!(pts(&first), vec![90_000]);
        assert!(matches!(
            &association.assembly,
            AudioFrameAssembly::Frame(frame)
                if frame.bytes.as_slice() == &second_frame[..3]
                    && frame.expected_len.is_none()
                    && frame.frame_pts_90khz == 91_920
        ));

        let mut continuation = second_frame[3..].to_vec();
        continuation.extend_from_slice(&third_frame);
        let completed = association
            .extract(packet(continuation, None), ORIGIN)
            .unwrap();
        assert_eq!(pts(&completed), vec![91_920, 93_840]);
        assert_eq!(completed[0].payload, second_frame);
        assert_eq!(completed[1].payload, third_frame);
    }

    #[test]
    fn continuation_only_pes_completes_the_containing_frame_with_its_pts() {
        let mut association = AudioTimestampAssociation::default();
        let frame = adts_frame(3, 8);
        let split_at = 12;

        assert!(association
            .extract(packet(frame[..split_at].to_vec(), Some(90_000)), ORIGIN,)
            .unwrap()
            .is_empty());
        let completed = association
            .extract(packet(frame[split_at..].to_vec(), None), ORIGIN)
            .unwrap();
        assert_eq!(pts(&completed), vec![90_000]);
        assert_eq!(completed[0].payload, frame);

        let next = association
            .extract(packet(adts_frame(3, 4), None), ORIGIN)
            .unwrap();
        assert_eq!(pts(&next), vec![91_920]);
    }

    #[test]
    fn explicit_pts_after_a_continuation_reanchors_the_first_new_frame() {
        let mut association = AudioTimestampAssociation::default();
        let first_frame = adts_frame(3, 8);
        let second_frame = adts_frame(3, 4);
        let split_at = 12;

        assert!(association
            .extract(
                packet(first_frame[..split_at].to_vec(), Some(90_000)),
                ORIGIN,
            )
            .unwrap()
            .is_empty());
        let mut continuation = first_frame[split_at..].to_vec();
        continuation.extend_from_slice(&second_frame);
        let completed = association
            .extract(packet(continuation, Some(180_000)), ORIGIN)
            .unwrap();
        assert_eq!(pts(&completed), vec![90_000, 180_000]);
        assert!(completed[0].metadata.is_pts_present);
        assert!(completed[1].metadata.is_pts_present);

        let next = association
            .extract(packet(adts_frame(3, 4), None), ORIGIN)
            .unwrap();
        assert_eq!(pts(&next), vec![181_920]);
    }

    #[test]
    fn mpeg_audio_body_is_reassembled_across_a_pes_boundary() {
        let mut association = AudioTimestampAssociation::default();
        let first_frame = mpeg1_layer2_frame();
        let second_frame = mpeg1_layer2_frame();
        let split_at = 100;

        assert!(association
            .extract(
                packet(first_frame[..split_at].to_vec(), Some(180_000)),
                ORIGIN,
            )
            .unwrap()
            .is_empty());
        let mut continuation = first_frame[split_at..].to_vec();
        continuation.extend_from_slice(&second_frame);
        let completed = association
            .extract(packet(continuation, None), ORIGIN)
            .unwrap();
        assert_eq!(pts(&completed), vec![180_000, 182_160]);
        assert_eq!(completed[0].payload, first_frame);
        assert_eq!(completed[1].payload, second_frame);
    }

    #[test]
    fn reset_discards_a_partial_frame_and_requires_a_new_anchor() {
        let mut association = AudioTimestampAssociation::default();
        let frame = adts_frame(3, 8);
        let split_at = 12;

        assert!(association
            .extract(packet(frame[..split_at].to_vec(), Some(90_000)), ORIGIN,)
            .unwrap()
            .is_empty());
        association.reset();
        assert_eq!(association.assembly, AudioFrameAssembly::Boundary);
        assert_eq!(
            association.extract(packet(frame[split_at..].to_vec(), None), ORIGIN),
            Err(AudioTimestampAssociationFailure::MissingAnchor)
        );
    }

    #[test]
    fn missing_anchor_and_malformed_payload_never_create_a_timestamp() {
        let mut association = AudioTimestampAssociation::default();
        assert_eq!(
            association.extract(packet(adts_frame(3, 4), None), ORIGIN),
            Err(AudioTimestampAssociationFailure::MissingAnchor)
        );

        let explicit = association
            .extract(packet(adts_frame(3, 4), Some(90_000)), ORIGIN)
            .unwrap();
        assert_eq!(pts(&explicit), vec![90_000]);
        assert_eq!(
            association.extract(packet(vec![0x00, 0x00], None), ORIGIN),
            Err(AudioTimestampAssociationFailure::UnsupportedOrMalformedFrames)
        );
        assert_eq!(
            association.extract(packet(adts_frame(3, 4), None), ORIGIN),
            Err(AudioTimestampAssociationFailure::MissingAnchor)
        );
    }

    #[test]
    fn unannounced_sample_rate_change_invalidates_the_anchor() {
        let mut association = AudioTimestampAssociation::default();
        assert!(association
            .extract(packet(adts_frame(3, 4), Some(90_000)), ORIGIN)
            .is_ok());
        assert_eq!(
            association.extract(packet(adts_frame(4, 4), None), ORIGIN),
            Err(AudioTimestampAssociationFailure::UnannouncedParameterChange)
        );
        assert_eq!(
            association.extract(packet(adts_frame(3, 4), None), ORIGIN),
            Err(AudioTimestampAssociationFailure::MissingAnchor)
        );
    }

    #[test]
    fn input_origin_change_cannot_reuse_an_anchor() {
        let mut association = AudioTimestampAssociation::default();
        assert!(association
            .extract(packet(adts_frame(3, 4), Some(90_000)), ORIGIN)
            .is_ok());
        assert_eq!(
            association.extract(
                packet(adts_frame(3, 4), None),
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

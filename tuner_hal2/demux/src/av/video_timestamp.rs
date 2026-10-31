use super::avc_timing::{start_codes, AvcTiming};
use super::AvMediaEventMetadata;
use crate::ts_core::PesPacket;
use crate::TsInputOrigin;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum VideoTimestampFailure {
    MissingAnchor,
    AllocationFailed,
    OriginChange,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct VideoMediaFrame {
    pub payload: Vec<u8>,
    pub metadata: AvMediaEventMetadata,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PesAnchor {
    offset: usize,
    metadata: AvMediaEventMetadata,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct VideoTimestampAssociation {
    bytes: Vec<u8>,
    anchors: Vec<PesAnchor>,
    origin: Option<TsInputOrigin>,
    timing: AvcTiming,
    context: Option<AvMediaEventMetadata>,
}

impl VideoTimestampAssociation {
    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(crate) fn reset_if_origin(&mut self, origin: TsInputOrigin) {
        if self.origin == Some(origin) {
            self.reset();
        }
    }

    pub(crate) fn extract_avc(
        &mut self,
        packet: PesPacket,
        origin: TsInputOrigin,
    ) -> Result<Vec<VideoMediaFrame>, VideoTimestampFailure> {
        let result = self.extract_inner(packet, origin);
        if result.is_err() {
            self.reset();
        }
        result
    }

    fn extract_inner(
        &mut self,
        packet: PesPacket,
        origin: TsInputOrigin,
    ) -> Result<Vec<VideoMediaFrame>, VideoTimestampFailure> {
        if self.origin.is_some_and(|old| old != origin) {
            return Err(VideoTimestampFailure::OriginChange);
        }
        self.origin = Some(origin);
        if packet.payload.is_empty() {
            return Ok(Vec::new());
        }
        // AVには汎用PES filterの固定予算を適用せず、実割当失敗を上位へ返す。
        self.bytes
            .try_reserve(packet.payload.len())
            .map_err(|_| VideoTimestampFailure::AllocationFailed)?;
        self.anchors
            .try_reserve(1)
            .map_err(|_| VideoTimestampFailure::AllocationFailed)?;
        self.anchors.push(PesAnchor {
            offset: self.bytes.len(),
            metadata: AvMediaEventMetadata::from_pes(
                packet.stream_id,
                packet.pts_90khz,
                packet.dts_90khz,
                packet.is_pes_private_data,
            ),
        });
        self.bytes.extend_from_slice(&packet.payload);
        let mut frames = Vec::new();
        let mut consumed = 0;
        {
            let mut boundaries = start_codes(&self.bytes)
                .filter(|(_, header)| self.bytes[*header] & 31 == 9)
                .map(|(start, _)| start)
                .peekable();
            while let Some(start) = boundaries.next() {
                let Some(&end) = boundaries.peek() else {
                    break;
                };
                // PESの明示時刻はそのPES内で開始する最初のAUへ一度だけ対応させる。
                let anchor_count = self
                    .anchors
                    .iter()
                    .take_while(|anchor| anchor.offset <= start)
                    .count();
                let metadata = if anchor_count != 0 {
                    let anchor = self.anchors[anchor_count - 1].metadata;
                    self.anchors.drain(..anchor_count);
                    self.context = Some(anchor);
                    anchor
                } else {
                    let context = self.context.ok_or(VideoTimestampFailure::MissingAnchor)?;
                    AvMediaEventMetadata::from_pes(
                        context.stream_id,
                        None,
                        None,
                        context.is_pes_private_data,
                    )
                };
                let timestamp = self.timing.timestamp(
                    &self.bytes[start..end],
                    metadata.pts_90khz,
                    metadata.dts_90khz,
                )?;
                let mut payload = Vec::new();
                payload
                    .try_reserve_exact(end - start)
                    .map_err(|_| VideoTimestampFailure::AllocationFailed)?;
                payload.extend_from_slice(&self.bytes[start..end]);
                frames
                    .try_reserve(1)
                    .map_err(|_| VideoTimestampFailure::AllocationFailed)?;
                frames.push(VideoMediaFrame {
                    payload,
                    metadata: AvMediaEventMetadata::from_pes_with_authoritative_pts(
                        metadata.stream_id,
                        metadata.pts_90khz,
                        timestamp.pts,
                        metadata.dts_90khz,
                        metadata.is_pes_private_data,
                    ),
                });
                consumed = end;
            }
        }
        if consumed != 0 {
            self.bytes.drain(..consumed);
            for anchor in &mut self.anchors {
                anchor.offset = anchor.offset.saturating_sub(consumed);
            }
        }
        Ok(frames)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConfigInputPid;
    use crate::packet_pipeline::PacketPid;
    use crate::ts_core::PesAssembler;

    fn packet(payload: Vec<u8>, pts: Option<u64>) -> PesPacket {
        PesPacket {
            pid: PacketPid::from_config_pid(ConfigInputPid::validate_tpid(0x281).unwrap()),
            stream_id: 0xe0,
            pts_90khz: pts,
            dts_90khz: None,
            is_pes_private_data: false,
            data_alignment_indicator: false,
            raw_bytes: Vec::new(),
            payload,
        }
    }

    #[test]
    fn many_nals_preserve_au_and_malformed_input_resets_timing() {
        use crate::av::avc_timing::tests::au;
        let mut first = au(true, true, 60, 0, 1001, 30000);
        for _ in 0..16384 {
            first.extend_from_slice(&[0, 0, 1, 12, 0xff, 0x80]);
        }
        let second = au(false, false, 2, 0, 1001, 30000);
        let origin = TsInputOrigin::frontend(1);
        let mut video = VideoTimestampAssociation::default();
        let mut payload = first.clone();
        payload.extend_from_slice(&second);
        let frames = video
            .extract_avc(packet(payload, Some(90000)), origin)
            .unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].payload, first);
        assert_eq!(frames[0].metadata.pts_90khz, Some(90000));

        let mut malformed = first.clone();
        malformed.extend_from_slice(&[0, 0, 1, 0x8c, 0x80]);
        malformed.extend_from_slice(&second);
        assert_eq!(
            video.extract_avc(packet(malformed, Some(180000)), origin),
            Err(VideoTimestampFailure::MissingAnchor)
        );
        let mut unanchored = second.clone();
        unanchored.extend_from_slice(&second);
        assert_eq!(
            video.extract_avc(packet(unanchored, None), origin),
            Err(VideoTimestampFailure::MissingAnchor)
        );
        let mut recovery = first.clone();
        recovery.extend_from_slice(&second);
        let frames = video
            .extract_avc(packet(recovery, Some(270000)), origin)
            .unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].payload, first);
        assert_eq!(frames[0].metadata.pts_90khz, Some(270000));
    }

    #[test]
    fn split_start_code_and_continuation_pts_apply_only_to_first_new_au() {
        use crate::av::avc_timing::tests::au;
        let first = au(true, true, 60, 0, 1001, 30000);
        let second = au(false, false, 2, 0, 1001, 30000);
        let third = au(false, false, 4, 0, 1001, 30000);
        let mut video = VideoTimestampAssociation::default();
        let origin = TsInputOrigin::frontend(1);
        assert!(video
            .extract_avc(
                packet(first[..first.len() - 2].to_vec(), Some(90000)),
                origin
            )
            .unwrap()
            .is_empty());
        let mut continuation = first[first.len() - 2..].to_vec();
        continuation.extend_from_slice(&second);
        continuation.extend_from_slice(&third[..2]);
        let output = video
            .extract_avc(packet(continuation, Some(96006)), origin)
            .unwrap();
        assert_eq!(output.len(), 1);
        assert_eq!(output[0].payload, first);
        assert_eq!(output[0].metadata.pts_90khz, Some(90000));
        let output = video
            .extract_avc(packet(third[2..].to_vec(), None), origin)
            .unwrap();
        assert_eq!(output.len(), 1);
        assert_eq!(output[0].payload, second);
        assert_eq!(output[0].metadata.pts_90khz, Some(96006));
        assert!(output[0].metadata.is_pts_present);
        video.reset();
        let mut no_config = third.clone();
        no_config.extend(third);
        assert!(video
            .extract_avc(packet(no_config, Some(42)), origin)
            .is_err());
    }

    #[test]
    #[ignore = "放送captureをAVC_CAPTURE_PATHで明示して実行する"]
    fn captured_transport_frames_have_individual_timestamps() {
        let path = std::env::var("AVC_CAPTURE_PATH").expect("capture path");
        let capture = std::fs::read(path).unwrap();
        let pid = PacketPid::from_config_pid(ConfigInputPid::validate_tpid(0x281).unwrap());
        let mut pes = PesAssembler::default();
        let mut video = VideoTimestampAssociation::default();
        let mut frames = Vec::new();
        let mut failures = 0;
        for packet in capture.chunks_exact(188) {
            assert_eq!(packet[0], 0x47);
            if (u16::from(packet[1] & 31) << 8 | u16::from(packet[2])) != 0x281 {
                continue;
            }
            if packet[3] & 0x10 == 0 {
                continue;
            }
            let offset = if packet[3] & 0x20 != 0 {
                5 + usize::from(packet[4])
            } else {
                4
            };
            for packet in pes.push(pid, packet[1] & 0x40 != 0, &packet[offset..]) {
                match video.extract_avc(packet, TsInputOrigin::frontend(1)) {
                    Ok(output) => frames.extend(output),
                    Err(_) => failures += 1,
                }
            }
        }
        eprintln!("frames={} failures={}", frames.len(), failures);
        assert!(frames.len() > 80);
        assert!(frames.iter().any(|frame| !frame.metadata.is_pts_present));
        for frame in &frames {
            assert_eq!(
                start_codes(&frame.payload)
                    .filter(|(_, h)| frame.payload[*h] & 31 == 9)
                    .count(),
                1
            );
        }
        // 添付captureのbitstream timingが規定する間隔。実装へ固定fpsは持ち込まない。
        for pair in frames.windows(2) {
            assert_eq!(
                (pair[1].metadata.pts_90khz.unwrap() + (1 << 33)
                    - pair[0].metadata.pts_90khz.unwrap())
                    % (1 << 33),
                6006
            );
        }
    }
}

use std::collections::VecDeque;

use maleicacid_tuner_hal2_common::{TsPacketCompletionBuffer, TS_PACKET_SIZE};
use maleicacid_tuner_hal2_demux::{
    DemuxRuntime, DemuxRuntimeError, DvrRuntimeState, PipelineReport, PlaybackConsumeReport,
    TsInputOrigin, ValidatedTsPacket,
};

const PLAYBACK_CONSUME_CHUNK_PACKETS: usize = 256;
const PLAYBACK_CONSUME_CHUNK_BYTES: usize = TS_PACKET_SIZE * PLAYBACK_CONSUME_CHUNK_PACKETS;

pub(crate) const fn required_playback_processing_bytes(queue_capacity: usize) -> usize {
    if queue_capacity < PLAYBACK_CONSUME_CHUNK_BYTES {
        queue_capacity
    } else {
        PLAYBACK_CONSUME_CHUNK_BYTES
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PlaybackConsumeTxn {
    dvr_id: i32,
    queue_capacity: usize,
    processing_buffer: Vec<u8>,
    completion: TsPacketCompletionBuffer,
    parse_inject_cursor: VecDeque<[u8; TS_PACKET_SIZE]>,
    cursor_origin: Option<TsInputOrigin>,
}

#[must_use]
pub(crate) struct PlaybackPacket {
    packet: [u8; TS_PACKET_SIZE],
    origin: TsInputOrigin,
}

impl PlaybackPacket {
    pub(crate) fn bytes(&self) -> &[u8; TS_PACKET_SIZE] {
        &self.packet
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PlaybackConsumeTxnPrepareError {
    InvalidCapacity,
    OutOfMemory,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PlaybackConsumeTxnError {
    primary: Box<DemuxRuntimeError>,
    cleanup: Option<Box<DemuxRuntimeError>>,
}

impl PlaybackConsumeTxnError {
    fn new(primary: DemuxRuntimeError, cleanup: Option<DemuxRuntimeError>) -> Self {
        Self { primary: Box::new(primary), cleanup: cleanup.map(Box::new) }
    }

    pub(crate) fn primary(&self) -> DemuxRuntimeError {
        *self.primary
    }

    pub(crate) fn cleanup(&self) -> Option<DemuxRuntimeError> {
        self.cleanup.as_deref().copied()
    }
}

impl From<DemuxRuntimeError> for PlaybackConsumeTxnError {
    fn from(primary: DemuxRuntimeError) -> Self {
        Self::new(primary, None)
    }
}

impl PlaybackConsumeTxn {
    pub(crate) fn prepare(
        dvr_id: i32,
        buffer_size: i32,
    ) -> Result<Self, PlaybackConsumeTxnPrepareError> {
        let capacity = usize::try_from(buffer_size)
            .ok()
            .filter(|capacity| *capacity > 0)
            .ok_or(PlaybackConsumeTxnPrepareError::InvalidCapacity)?;
        let processing_capacity = required_playback_processing_bytes(capacity);
        let mut processing_buffer = Vec::new();
        processing_buffer
            .try_reserve_exact(processing_capacity)
            .map_err(|_| PlaybackConsumeTxnPrepareError::OutOfMemory)?;
        processing_buffer.resize(processing_capacity, 0);
        Ok(Self {
            dvr_id,
            queue_capacity: capacity,
            processing_buffer,
            completion: TsPacketCompletionBuffer::default(),
            parse_inject_cursor: VecDeque::new(),
            cursor_origin: None,
        })
    }

    pub(crate) fn capacity_matches(&self, buffer_size: i32) -> bool {
        usize::try_from(buffer_size).ok() == Some(self.queue_capacity)
    }

    pub(crate) fn begin_consume(
        &mut self,
        demux: &mut DemuxRuntime,
    ) -> Result<PlaybackConsumeReport, PlaybackConsumeTxnError> {
        let mut report = PlaybackConsumeReport::default();
        if self.parse_inject_cursor.is_empty() {
            let Some(read_txn) =
                demux.begin_playback_queue_read(self.dvr_id, self.processing_buffer.len())?
            else {
                return Ok(report);
            };
            let read_limit = read_txn.read_limit();
            let read = match demux
                .read_playback_queue(&read_txn, &mut self.processing_buffer[..read_limit])
            {
                Ok(read) => read,
                Err(primary) => {
                    let cleanup = demux.abort_playback_queue_read(read_txn).err();
                    return Err(PlaybackConsumeTxnError::new(primary, cleanup));
                }
            };
            if read == 0 {
                demux.abort_playback_queue_read(read_txn)?;
                return Err(DemuxRuntimeError::queue_runtime_failure(self.dvr_id).into());
            }
            let origin = demux.commit_playback_queue_read(read_txn)?;
            let drain = self.completion.push(&self.processing_buffer[..read]);
            self.parse_inject_cursor.extend(drain.packets);
            self.cursor_origin = Some(origin);
            report.bytes_read = read;
            report.malformed_bytes = drain.malformed_bytes;
        }

        report.dropped_bytes = report.malformed_bytes;
        let stats =
            demux.note_playback_consume_result(self.dvr_id, 0, 0, report.malformed_bytes)?;
        if report.dropped_bytes > 0 {
            eprintln!(
                "maleicacid-tuner-hal2-dvr-playback-diagnostic: dvr_id={} malformed_packets={} malformed_bytes={} dropped_bytes={} total_dropped_bytes={}",
                self.dvr_id,
                report.malformed_packets,
                report.malformed_bytes,
                report.dropped_bytes,
                stats.dropped_bytes,
            );
        }
        Ok(report)
    }

    pub(crate) fn pending_packet(
        &self,
        demux: &DemuxRuntime,
    ) -> Result<Option<PlaybackPacket>, DemuxRuntimeError> {
        if demux.dvr_snapshot(self.dvr_id)?.state != DvrRuntimeState::Started {
            return Ok(None);
        }
        let Some(packet) = self.parse_inject_cursor.front() else {
            return Ok(None);
        };
        let origin = self
            .cursor_origin
            .ok_or(DemuxRuntimeError::queue_runtime_failure(self.dvr_id))?;
        Ok(Some(PlaybackPacket {
            packet: *packet,
            origin,
        }))
    }

    pub(crate) fn is_current_packet(
        &self,
        demux: &DemuxRuntime,
        packet: &PlaybackPacket,
    ) -> Result<bool, DemuxRuntimeError> {
        Ok(
            demux.dvr_snapshot(self.dvr_id)?.state == DvrRuntimeState::Started
                && self.cursor_origin == Some(packet.origin)
                && self.parse_inject_cursor.front() == Some(&packet.packet),
        )
    }

    pub(crate) fn consume(
        &mut self,
        demux: &mut DemuxRuntime,
        packet: PlaybackPacket,
        output: Option<&ValidatedTsPacket<'_>>,
    ) -> Result<PlaybackConsumeReport, PlaybackConsumeTxnError> {
        if !self.is_current_packet(demux, &packet)? {
            return Ok(PlaybackConsumeReport::default());
        }
        self.parse_inject_cursor.pop_front();
        if self.parse_inject_cursor.is_empty() {
            self.cursor_origin = None;
        }
        let (packet_report, malformed_packets) = match ValidatedTsPacket::validate(&packet.packet) {
            Ok(_) => (
                output.map_or_else(PipelineReport::default, |validated| {
                    demux.inject_playback_packet(validated, packet.origin)
                }),
                0,
            ),
            Err(reason) => (demux.note_malformed_playback_packet(reason), 1),
        };
        demux.note_playback_consume_result(
            self.dvr_id,
            1 - malformed_packets,
            malformed_packets,
            0,
        )?;
        Ok(PlaybackConsumeReport {
            completed_packets: 1,
            malformed_packets,
            dropped_bytes: malformed_packets * TS_PACKET_SIZE,
            packet_reports: vec![packet_report],
            ..PlaybackConsumeReport::default()
        })
    }

    pub(crate) fn discard_for_boundary(&mut self) -> usize {
        let pending_packet_bytes = self
            .parse_inject_cursor
            .len()
            .saturating_mul(TS_PACKET_SIZE);
        self.parse_inject_cursor.clear();
        self.cursor_origin = None;
        let drain = self.completion.drain_for_boundary();
        pending_packet_bytes
            .saturating_add(drain.packets.len().saturating_mul(TS_PACKET_SIZE))
            .saturating_add(drain.malformed_bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn started_demux() -> DemuxRuntime {
        use maleicacid_tuner_hal2_demux::{
            DvrKind, DvrRuntimeConfigureRequest, DvrRuntimeOperationRequest,
            DvrRuntimeRegistrationRequest,
        };
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_dvr_from_typed_request(DvrRuntimeRegistrationRequest::new(
                7,
                DvrKind::Playback,
                4096,
                true,
            ))
            .unwrap();
        demux
            .configure_dvr_runtime_with_typed_request(DvrRuntimeConfigureRequest::new(7))
            .1
            .unwrap();
        demux
            .start_dvr_runtime_from_typed_request(DvrRuntimeOperationRequest::new(7))
            .unwrap();
        demux
    }

    fn pending_txn(packet: [u8; TS_PACKET_SIZE]) -> PlaybackConsumeTxn {
        let mut txn = PlaybackConsumeTxn::prepare(7, 4096).unwrap();
        txn.parse_inject_cursor.push_back(packet);
        txn.cursor_origin = Some(TsInputOrigin::PlaybackDvr {
            dvr_id: 7,
            queue_identity: 1,
            queue_epoch: 1,
        });
        txn
    }

    fn clear_packet() -> [u8; TS_PACKET_SIZE] {
        let mut packet = [0xff; TS_PACKET_SIZE];
        packet[..4].copy_from_slice(&[0x47, 0x00, 0x64, 0x10]);
        packet
    }

    #[test]
    fn stop_retains_pending_packet_until_restart() {
        use maleicacid_tuner_hal2_demux::DvrRuntimeOperationRequest;
        let mut demux = started_demux();
        let original = clear_packet();
        let mut txn = pending_txn(original);
        let pending = txn.pending_packet(&demux).unwrap().unwrap();
        demux
            .stop_dvr_runtime_from_typed_request(DvrRuntimeOperationRequest::new(7))
            .unwrap();
        assert!(txn.pending_packet(&demux).unwrap().is_none());
        assert_eq!(
            txn.consume(&mut demux, pending, None)
                .unwrap()
                .completed_packets,
            0
        );
        assert_eq!(txn.parse_inject_cursor.len(), 1);
        demux
            .start_dvr_runtime_from_typed_request(DvrRuntimeOperationRequest::new(7))
            .unwrap();
        let pending = txn.pending_packet(&demux).unwrap().unwrap();
        let validated = ValidatedTsPacket::validate(&original).unwrap();
        assert_eq!(
            txn.consume(&mut demux, pending, Some(&validated))
                .unwrap()
                .completed_packets,
            1
        );
        assert!(txn.pending_packet(&demux).unwrap().is_none());
    }

    #[test]
    fn boundary_discards_pending_packet_and_rejects_delayed_result() {
        let mut demux = started_demux();
        let mut txn = pending_txn(clear_packet());
        let pending = txn.pending_packet(&demux).unwrap().unwrap();
        assert_eq!(txn.discard_for_boundary(), TS_PACKET_SIZE);
        assert_eq!(
            txn.consume(&mut demux, pending, None)
                .unwrap()
                .completed_packets,
            0
        );
    }

    #[test]
    fn malformed_packet_is_accounted_without_stopping_following_packet() {
        let mut demux = started_demux();
        let mut malformed = clear_packet();
        malformed[3] = 0;
        let mut txn = pending_txn(malformed);
        txn.parse_inject_cursor.push_back(clear_packet());
        let pending = txn.pending_packet(&demux).unwrap().unwrap();
        let report = txn.consume(&mut demux, pending, None).unwrap();
        assert_eq!(report.malformed_packets, 1);
        assert_eq!(report.dropped_bytes, TS_PACKET_SIZE);
        assert!(txn.pending_packet(&demux).unwrap().is_some());
    }

    #[test]
    fn playback_uses_resolved_keys_and_preserves_ciphertext_without_a_key() {
        use crate::descrambler_key_table::tests::TestKeyReference;
        use crate::registry::{ResolvedDescramblerPacketFlow, RuntimeRegistry};
        use maleicacid_tuner_hal2_demux::{
            FilterConfig, FilterConfigKind, FilterOpenType, FilterRuntimeConfigureRequest,
            FilterRuntimeOperationRequest, FilterRuntimeRegistrationRequest, OpenFilterRequest,
            PesSettings,
        };
        use maleicacid_tuner_hal2_descrambler::{
            multi2_encrypt_payload, DescramblerKeySlot, DescramblerKeyToken, DescramblerPidClaim,
            KeyParity, Multi2KeyMaterial,
        };
        let slot = DescramblerKeySlot::empty()
            .try_with_even(Multi2KeyMaterial::new([0x12; 32], [0x34; 8], [0x56; 8]))
            .unwrap();
        let clear = clear_packet();
        let mut encrypted = clear;
        multi2_encrypt_payload(&mut encrypted[4..], slot.key_for(KeyParity::Even).unwrap());
        encrypted[3] = 0x90;
        for (has_key, concurrent_failure) in [(true, false), (false, false), (false, true)] {
            let mut registry = RuntimeRegistry::default();
            let descrambler = registry.allocate_descrambler().unwrap();
            registry
                .begin_descrambler_demux_source_call_use_case(descrambler.id)
                .unwrap();
            registry
                .bind_descrambler_demux_use_case(descrambler.id, 1, 1)
                .unwrap();
            registry
                .add_descrambler_pid_claim_use_case(
                    descrambler.id,
                    DescramblerPidClaim::from_demux_input(100).unwrap(),
                )
                .unwrap();
            let token = DescramblerKeyToken::try_from_bytes(vec![0x71; 8]).unwrap();
            let reference =
                std::sync::Arc::new(TestKeyReference(std::sync::Mutex::new(Some(slot.clone()))));
            if has_key || concurrent_failure {
                registry
                    .publish_descrambler_key_resolution(token.clone(), reference.clone())
                    .unwrap();
                registry
                    .replace_descrambler_key_use_case(descrambler.id, token.clone())
                    .unwrap();
            }
            let mut demux = started_demux();
            for (filter_id, open_type, kind) in [
                (8, FilterOpenType::TsRaw, FilterConfigKind::TsRaw),
                (
                    9,
                    FilterOpenType::TsPes,
                    FilterConfigKind::TsPes(PesSettings {
                        stream_id: 0xffff,
                        raw: false,
                    }),
                ),
            ] {
                demux
                    .register_filter_from_typed_request(FilterRuntimeRegistrationRequest::new(
                        filter_id,
                        &OpenFilterRequest {
                            open_type,
                            buffer_size: 4096,
                            callback_present: true,
                        },
                        8,
                    ))
                    .unwrap();
                demux
                    .configure_filter_runtime_with_typed_request(
                        FilterRuntimeConfigureRequest::new(
                            filter_id,
                            FilterConfig {
                                open_type,
                                tpid: 100,
                                kind,
                            },
                        ),
                    )
                    .1
                    .unwrap();
                demux
                    .start_filter_runtime_from_typed_request(FilterRuntimeOperationRequest::new(
                        filter_id,
                    ))
                    .unwrap();
            }
            let mut txn = pending_txn(encrypted);
            let pending = txn.pending_packet(&demux).unwrap().unwrap();
            let pid = ValidatedTsPacket::validate(pending.bytes()).unwrap().pid();
            let requests = registry.descrambler_key_refresh_requests_for_demuxes(
                &[(crate::registry::DemuxRuntimeId(1), 1)]
                    .into_iter()
                    .collect(),
            );
            if concurrent_failure {
                assert_eq!(requests.len(), 1);
                *reference.0.lock().unwrap() = None;
            }
            let packet_keys = registry.snapshot_descrambler_packet_keys(requests);
            let decision = registry
                .resolved_descrambler_packet_material_for_demux(1, 1, pid, &packet_keys)
                .decide_descrambled_packet(1, pid, pending.bytes());
            assert_eq!(decision.packet, if has_key { clear } else { encrypted });
            assert_eq!(
                decision.flow,
                if has_key {
                    ResolvedDescramblerPacketFlow::Descrambled
                } else {
                    ResolvedDescramblerPacketFlow::RecordPassThroughAndDropAssembly
                }
            );
            let output = ValidatedTsPacket::validate(&decision.packet).unwrap();
            let report = txn.consume(&mut demux, pending, Some(&output)).unwrap();
            assert_eq!(report.completed_packets, 1);
            assert_eq!(report.packet_reports[0].accepted_packets, 1);
            assert_eq!(
                report.packet_reports[0]
                    .assembly_suppression_reasons
                    .is_empty(),
                has_key
            );
            assert!(txn.pending_packet(&demux).unwrap().is_none());
        }
    }

    #[test]
    fn large_fmq_uses_bounded_processing_chunk_without_losing_capacity_identity() {
        let queue_capacity = 4 * 1024 * 1024;
        let txn = PlaybackConsumeTxn::prepare(7, queue_capacity).expect("prepare");

        assert_eq!(
            txn.processing_buffer.len(),
            required_playback_processing_bytes(queue_capacity as usize)
        );
        assert_eq!(txn.processing_buffer.len(), PLAYBACK_CONSUME_CHUNK_BYTES);
        assert!(txn.capacity_matches(queue_capacity));
        assert!(!txn.capacity_matches(queue_capacity / 2));
    }

    #[test]
    fn small_fmq_does_not_allocate_beyond_its_capacity() {
        let txn = PlaybackConsumeTxn::prepare(7, 100).expect("prepare");

        assert_eq!(
            txn.processing_buffer.len(),
            required_playback_processing_bytes(100)
        );
        assert_eq!(txn.processing_buffer.len(), 100);
        assert!(txn.capacity_matches(100));
    }
}

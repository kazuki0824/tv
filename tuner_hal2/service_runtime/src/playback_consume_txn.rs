use std::collections::VecDeque;

use maleicacid_tuner_hal2_common::{TsPacketCompletionBuffer, TS_PACKET_SIZE};
use maleicacid_tuner_hal2_demux::{
    DemuxRuntime, DemuxRuntimeError, PlaybackConsumeReport, TsInputOrigin, ValidatedTsPacket,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PlaybackConsumeTxnPrepareError {
    InvalidCapacity,
    OutOfMemory,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PlaybackConsumeTxnError {
    primary: DemuxRuntimeError,
    cleanup: Option<DemuxRuntimeError>,
}

impl PlaybackConsumeTxnError {
    const fn new(primary: DemuxRuntimeError, cleanup: Option<DemuxRuntimeError>) -> Self {
        Self { primary, cleanup }
    }

    pub(crate) const fn primary(self) -> DemuxRuntimeError {
        self.primary
    }

    pub(crate) const fn cleanup(self) -> Option<DemuxRuntimeError> {
        self.cleanup
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

    pub(crate) fn consume(
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

        let origin = match self.cursor_origin {
            Some(origin) => origin,
            None if self.parse_inject_cursor.is_empty() => return Ok(report),
            None => return Err(DemuxRuntimeError::queue_runtime_failure(self.dvr_id).into()),
        };
        let mut injected_packets = 0usize;
        let mut malformed_packets = 0usize;
        while let Some(packet) = self.parse_inject_cursor.pop_front() {
            let packet_report = match ValidatedTsPacket::validate(&packet) {
                Ok(validated) => {
                    injected_packets = injected_packets.saturating_add(1);
                    demux.inject_playback_packet(&validated, origin)
                }
                Err(reason) => {
                    malformed_packets = malformed_packets.saturating_add(1);
                    demux.note_malformed_playback_packet(reason)
                }
            };
            report.packet_reports.push(packet_report);
        }
        self.cursor_origin = None;
        report.completed_packets = injected_packets.saturating_add(malformed_packets);
        report.malformed_packets = malformed_packets;
        report.dropped_bytes = report
            .malformed_bytes
            .saturating_add(malformed_packets.saturating_mul(TS_PACKET_SIZE));
        let stats = demux.note_playback_consume_result(
            self.dvr_id,
            injected_packets,
            malformed_packets,
            report.malformed_bytes,
        )?;
        if report.dropped_bytes > 0 {
            eprintln!(
                "maleicacid-tuner-hal2-dvr-playback-diagnostic: dvr_id={} malformed_packets={} malformed_bytes={} dropped_bytes={} total_dropped_bytes={}",
                self.dvr_id,
                malformed_packets,
                report.malformed_bytes,
                report.dropped_bytes,
                stats.dropped_bytes,
            );
        }
        Ok(report)
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

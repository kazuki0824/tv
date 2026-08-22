use std::collections::VecDeque;

use maleicacid_tuner_hal2_common::{TsPacketCompletionBuffer, TS_PACKET_SIZE};
use maleicacid_tuner_hal2_demux::{
    DemuxRuntime, DemuxRuntimeError, PlaybackConsumeReport, TsInputOrigin,
    ValidatedTsPacket,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PlaybackConsumeTxn {
    dvr_id: i32,
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

impl PlaybackConsumeTxn {
    pub(crate) fn prepare(
        dvr_id: i32,
        buffer_size: i32,
    ) -> Result<Self, PlaybackConsumeTxnPrepareError> {
        let capacity = usize::try_from(buffer_size)
            .ok()
            .filter(|capacity| *capacity > 0)
            .ok_or(PlaybackConsumeTxnPrepareError::InvalidCapacity)?;
        let mut processing_buffer = Vec::new();
        processing_buffer
            .try_reserve_exact(capacity)
            .map_err(|_| PlaybackConsumeTxnPrepareError::OutOfMemory)?;
        processing_buffer.resize(capacity, 0);
        Ok(Self {
            dvr_id,
            processing_buffer,
            completion: TsPacketCompletionBuffer::default(),
            parse_inject_cursor: VecDeque::new(),
            cursor_origin: None,
        })
    }

    pub(crate) fn capacity_matches(&self, buffer_size: i32) -> bool {
        usize::try_from(buffer_size).ok() == Some(self.processing_buffer.len())
    }

    pub(crate) fn consume(
        &mut self,
        demux: &mut DemuxRuntime,
    ) -> Result<PlaybackConsumeReport, DemuxRuntimeError> {
        let mut report = PlaybackConsumeReport::default();
        if self.parse_inject_cursor.is_empty() {
            let Some(read_txn) = demux.begin_playback_queue_read(
                self.dvr_id,
                self.processing_buffer.len(),
            )? else {
                return Ok(report);
            };
            let read_limit = read_txn.read_limit();
            let read = demux.read_playback_queue(
                &read_txn,
                &mut self.processing_buffer[..read_limit],
            )?;
            if read == 0 {
                return Err(DemuxRuntimeError::queue_runtime_failure(self.dvr_id));
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
            None => return Err(DemuxRuntimeError::queue_runtime_failure(self.dvr_id)),
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

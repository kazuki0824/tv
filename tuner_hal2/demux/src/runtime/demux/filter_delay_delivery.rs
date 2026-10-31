use super::{DemuxRuntime, DemuxRuntimeError};
use crate::config::FilterDelayReadiness;
use crate::packet_pipeline::PipelineGeneratedEvent;

impl DemuxRuntime {
    /// 既存のFilterDelayHint状態で配送可能になったcallback eventだけを取り出す。
    /// 遅延状態の正本はFilterRuntime、保留eventの正本はFilterProducerDrainGateに置く。
    pub fn take_ready_filter_events(
        &mut self,
    ) -> Result<Vec<PipelineGeneratedEvent>, DemuxRuntimeError> {
        let ready_filter_ids: Vec<i32> = self
            .filters
            .iter()
            .filter_map(|(filter_id, filter)| {
                (filter.state().is_started()
                    && filter.delivery_readiness() == FilterDelayReadiness::Ready)
                    .then_some(*filter_id)
            })
            .collect();

        let mut events = Vec::new();
        for filter_id in ready_filter_ids {
            let pending = self
                .filter_producer_gates
                .get(&filter_id)
                .ok_or(DemuxRuntimeError::filter_missing(filter_id))?
                .take_pending_events()
                .map_err(|_| DemuxRuntimeError::queue_runtime_failure(filter_id))?;
            self.filters
                .get_mut(&filter_id)
                .ok_or(DemuxRuntimeError::filter_missing(filter_id))?
                .commit_delivery_batch();
            events.extend(pending);
        }
        Ok(events)
    }

    /// FilterRuntimeが保持する正本deadlineのうち、最も早い時刻を返す。
    /// 独立したscheduler状態は生成せず、呼出し元は正本を再評価するまでに
    /// 配送executorが待機できる時間の判断にのみ使用する。
    pub fn next_filter_delivery_deadline(&self) -> Option<std::time::Instant> {
        self.filters
            .values()
            .filter_map(|filter| {
                if !filter.state().is_started() {
                    return None;
                }
                let snapshot = filter.snapshot();
                if snapshot.queued_bytes == 0 {
                    return None;
                }
                snapshot.delivery_not_before
            })
            .min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{FilterDelayHint, FilterOpenType};
    use crate::runtime::filter::{FilterRuntime, FilterStatusEvent};
    use crate::runtime::filter_producer_drain_gate::FilterProducerDrainGate;
    use std::time::Duration;

    #[test]
    fn time_delay_releases_pending_event_without_another_packet() {
        let mut demux = DemuxRuntime::new(1, 1);
        let mut filter = FilterRuntime::new_typed(7, 1, FilterOpenType::TsSection);
        filter.mark_started();
        filter.set_delay_hint(FilterDelayHint::TimeDelayMs(5));
        filter.note_payload_queued(3);

        let gate = FilterProducerDrainGate::new(4).expect("gate");
        let mut permit = gate.begin_producer().expect("producer");
        permit
            .enqueue_event(PipelineGeneratedEvent::FilterStatus {
                filter_id: 7,
                status: FilterStatusEvent::DataReady,
            })
            .expect("pending event");
        permit.commit().expect("producer commit");

        demux.filters.insert(7, filter);
        demux.filter_producer_gates.insert(7, gate);

        assert!(demux.take_ready_filter_events().unwrap().is_empty());
        assert!(demux.next_filter_delivery_deadline().is_some());

        std::thread::sleep(Duration::from_millis(10));

        let events = demux.take_ready_filter_events().unwrap();
        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0],
            PipelineGeneratedEvent::FilterStatus {
                filter_id: 7,
                status: FilterStatusEvent::DataReady,
            }
        ));
        assert_eq!(demux.next_filter_delivery_deadline(), None);
    }
}

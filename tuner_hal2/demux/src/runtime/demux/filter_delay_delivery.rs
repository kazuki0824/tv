use super::*;

impl DemuxRuntime {
    /// Releases only callback events whose existing FilterDelayHint state says
    /// they are ready. The FilterRuntime and FilterProducerDrainGate remain the
    /// canonical owners of delay state and pending events respectively.
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

    /// Returns the earliest canonical FilterRuntime deadline. No independent
    /// scheduler state is created here; callers only use this to decide how
    /// long the delivery executor may sleep before re-evaluating the owners.
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

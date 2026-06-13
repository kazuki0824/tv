//! tuner_hal2 demux層。
//!
//! parser断片は `parser/` 配下に置く。runtime所有は `runtime/` 配下で再構築し、AV shared memory処理は `av/` 配下へ分ける。

#[path = "parser/packet_pipeline.rs"]
pub mod packet_pipeline;
#[path = "parser/record_index.rs"]
pub mod record_index;
#[path = "parser/sections.rs"]
pub mod sections;
#[path = "parser/ts_core.rs"]
pub mod ts_core;

pub mod parser;
pub mod runtime;
pub mod av;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum TsInputOrigin {
    Frontend,
    Playback,
    SourceFilter { source_filter_id: i32, source_filter_generation: u64 },
}

impl TsInputOrigin {
    pub const fn allows_record_mirror(self) -> bool { matches!(self, TsInputOrigin::Frontend) }
}

pub use runtime::{DemuxRuntime, DvrRuntime, FilterRuntime, RuntimeIoRegistry};
pub use av::{AvDataId, AvDataIdState, AvFilterReleaseState, AvHandleReleaseInput, AvHandleReleaseOutcome, AvHandleReleaseTxn, AvPayloadDeliveryOutcome, AvSharedBacking, AvSlotId, ClientHandleState};


#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet_pipeline::{FilterPipelineConfig, PipelineOpenKind};

    #[test]
    fn frontend_origin_allows_record_mirror() {
        assert!(TsInputOrigin::Frontend.allows_record_mirror());
        assert!(!TsInputOrigin::Playback.allows_record_mirror());
    }

    #[test]
    fn source_boundary_missing_queue_does_not_create_queue() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux.register_filter(DemuxRuntime::open_filter_runtime(10, 1, PipelineOpenKind::Raw, None)).unwrap();
        assert!(!demux.queue_exists(10));
        let (txn, result) = SourceBoundaryTxn::new(10).apply(&mut demux);
        assert!(result.is_err());
        assert_eq!(txn.outcome(), Some(SourceBoundaryOutcome::Failed { step: SourceBoundaryStep::ClearQueue }));
        assert!(!demux.queue_exists(10));
    }

    #[test]
    fn filter_configure_failure_rolls_back_runtime_snapshot() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux.register_filter(DemuxRuntime::open_filter_runtime(11, 1, PipelineOpenKind::Raw, Some(FilterPipelineConfig { tpid: Some(100), raw: false }))).unwrap();
        demux.create_filter_queue(11).unwrap();
        let before = demux.filter(11).unwrap().snapshot();
        let (txn, result) = FilterConfigureTxn::new(11).configure(&mut demux, PipelineOpenKind::Pes, FilterPipelineConfig { tpid: Some(101), raw: true });
        assert!(result.is_err());
        assert_eq!(txn.outcome(), Some(FilterConfigureOutcome::RolledBack { failed_step: FilterConfigureStep::ValidateSettings }));
        assert_eq!(demux.filter(11).unwrap().snapshot(), before);
    }

    #[test]
    fn generation_boundary_resets_pipeline_and_bumps_generation() {
        let mut demux = DemuxRuntime::new(1, 7);
        let (_, report) = GenerationBoundaryTxn::new(DemuxStreamGeneration(7)).apply(&mut demux);
        assert_eq!(report.next_generation, DemuxStreamGeneration(8));
        assert_eq!(demux.generation(), 8);
    }
}

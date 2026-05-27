//! stream境界処理を統一する。
//!
//! 境界理由ごとに同じ 8 段階を実行し、失敗時は boundary_failed として
//! 呼び出し側が同じ plan を再実行できる結果を返す。closure による no-op
//! 埋め込みは禁止し、具象 resource trait で操作する。

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum StreamBoundaryReason { TuneStart, FrontendClose, FrontendUnbind, SourceFilterChange }
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum StreamBoundaryStep { AdvanceGeneration, NotifyWorkerBoundary, FlushRuntimeIo, ClearFmq, ResetPacketPipeline, DiscardAvPayloads, ResetDvrPlayback, CommitGeneration }

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct StreamBoundaryResetPlan { pub reason: StreamBoundaryReason, pub demux_id: i32, pub generation: u64 }

pub trait StreamBoundaryResources {
    type Error;
    fn advance_generation(&mut self, plan: &StreamBoundaryResetPlan) -> Result<(), Self::Error>;
    fn notify_worker_boundary(&mut self, plan: &StreamBoundaryResetPlan) -> Result<(), Self::Error>;
    fn flush_runtime_io(&mut self, plan: &StreamBoundaryResetPlan) -> Result<(), Self::Error>;
    fn clear_fmq(&mut self, plan: &StreamBoundaryResetPlan) -> Result<(), Self::Error>;
    fn reset_packet_pipeline(&mut self, plan: &StreamBoundaryResetPlan) -> Result<(), Self::Error>;
    fn discard_av_payloads(&mut self, plan: &StreamBoundaryResetPlan) -> Result<(), Self::Error>;
    fn reset_dvr_playback(&mut self, plan: &StreamBoundaryResetPlan) -> Result<(), Self::Error>;
    fn commit_generation(&mut self, plan: &StreamBoundaryResetPlan) -> Result<(), Self::Error>;
}

impl StreamBoundaryResetPlan {
    pub fn for_demux(reason: StreamBoundaryReason, demux_id: i32, generation: u64) -> Self { Self { reason, demux_id, generation } }

    #[cfg(test)]
    pub fn execute<R: StreamBoundaryResources>(&self, resources: &mut R) -> Result<StreamBoundaryResetResult, R::Error> {
        self.execute_from_step(resources, None)
    }

    pub fn execute_from_step<R: StreamBoundaryResources>(&self, resources: &mut R, resume_from: Option<StreamBoundaryStep>) -> Result<StreamBoundaryResetResult, R::Error> {
        self.try_execute_from_step(resources, resume_from).map_err(|failure| failure.error)
    }

    pub fn try_execute_from_step<R: StreamBoundaryResources>(&self, resources: &mut R, resume_from: Option<StreamBoundaryStep>) -> Result<StreamBoundaryResetResult, StreamBoundaryExecutionError<R::Error>> {
        let steps: &[(StreamBoundaryStep, fn(&mut R, &StreamBoundaryResetPlan) -> Result<(), R::Error>)] = &[
            (StreamBoundaryStep::AdvanceGeneration, R::advance_generation),
            (StreamBoundaryStep::NotifyWorkerBoundary, R::notify_worker_boundary),
            (StreamBoundaryStep::FlushRuntimeIo, R::flush_runtime_io),
            (StreamBoundaryStep::ClearFmq, R::clear_fmq),
            (StreamBoundaryStep::ResetPacketPipeline, R::reset_packet_pipeline),
            (StreamBoundaryStep::DiscardAvPayloads, R::discard_av_payloads),
            (StreamBoundaryStep::ResetDvrPlayback, R::reset_dvr_playback),
            (StreamBoundaryStep::CommitGeneration, R::commit_generation),
        ];
        let mut started = resume_from.is_none();
        let mut attempted = 0usize;
        for (step, f) in steps {
            if !started {
                started = Some(*step) == resume_from;
            }
            if !started {
                continue;
            }
            attempted = attempted.saturating_add(1);
            if let Err(error) = f(resources, self) {
                let result = StreamBoundaryResetResult {
                    reason: self.reason,
                    demux_id: Some(self.demux_id),
                    generation: Some(self.generation),
                    attempted_steps: attempted,
                    boundary_failed: true,
                    failed_step: Some(*step),
                };
                return Err(StreamBoundaryExecutionError { result, error });
            }
        }
        Ok(StreamBoundaryResetResult { reason: self.reason, demux_id: Some(self.demux_id), generation: Some(self.generation), attempted_steps: attempted, boundary_failed: false, failed_step: None })
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct StreamBoundaryResetResult { pub reason: StreamBoundaryReason, pub demux_id: Option<i32>, pub generation: Option<u64>, pub attempted_steps: usize, pub boundary_failed: bool, pub failed_step: Option<StreamBoundaryStep> }

#[derive(Debug)]
pub struct StreamBoundaryExecutionError<E> {
    pub result: StreamBoundaryResetResult,
    pub error: E,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PendingStreamBoundaryPlan {
    pub plan: StreamBoundaryResetPlan,
    pub failed_step: Option<StreamBoundaryStep>,
}

impl PendingStreamBoundaryPlan {
    pub fn new(plan: StreamBoundaryResetPlan, failed_step: Option<StreamBoundaryStep>) -> Self {
        Self { plan, failed_step }
    }

    pub fn execute<R: StreamBoundaryResources>(&self, resources: &mut R) -> Result<StreamBoundaryResetResult, R::Error> {
        self.plan.execute_from_step(resources, self.failed_step)
    }

    pub fn try_execute<R: StreamBoundaryResources>(&self, resources: &mut R) -> Result<StreamBoundaryResetResult, StreamBoundaryExecutionError<R::Error>> {
        self.plan.try_execute_from_step(resources, self.failed_step)
    }
}

#[cfg(test)]
#[derive(Debug, Default)]
pub struct TestStreamBoundaryResources;
#[cfg(test)]
impl StreamBoundaryResources for TestStreamBoundaryResources { type Error = (); fn advance_generation(&mut self,_:&StreamBoundaryResetPlan)->Result<(),Self::Error>{Ok(())} fn notify_worker_boundary(&mut self,_:&StreamBoundaryResetPlan)->Result<(),Self::Error>{Ok(())} fn flush_runtime_io(&mut self,_:&StreamBoundaryResetPlan)->Result<(),Self::Error>{Ok(())} fn clear_fmq(&mut self,_:&StreamBoundaryResetPlan)->Result<(),Self::Error>{Ok(())} fn reset_packet_pipeline(&mut self,_:&StreamBoundaryResetPlan)->Result<(),Self::Error>{Ok(())} fn discard_av_payloads(&mut self,_:&StreamBoundaryResetPlan)->Result<(),Self::Error>{Ok(())} fn reset_dvr_playback(&mut self,_:&StreamBoundaryResetPlan)->Result<(),Self::Error>{Ok(())} fn commit_generation(&mut self,_:&StreamBoundaryResetPlan)->Result<(),Self::Error>{Ok(())} }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_boundary_plan_preserves_demux_generation() {
        let plan=StreamBoundaryResetPlan::for_demux(StreamBoundaryReason::FrontendUnbind,7,11);
        let mut resources=TestStreamBoundaryResources;
        let result=plan.execute(&mut resources).unwrap();
        assert_eq!(result.demux_id,Some(7));
        assert_eq!(result.generation,Some(11));
        assert_eq!(result.attempted_steps,8);
    }

    #[derive(Debug)]
    struct FailingBoundaryResources {
        fail_once_at: Option<StreamBoundaryStep>,
        steps: Vec<StreamBoundaryStep>,
    }

    impl StreamBoundaryResources for FailingBoundaryResources {
        type Error = StreamBoundaryStep;
        fn advance_generation(&mut self, _: &StreamBoundaryResetPlan) -> Result<(), Self::Error> { self.step(StreamBoundaryStep::AdvanceGeneration) }
        fn notify_worker_boundary(&mut self, _: &StreamBoundaryResetPlan) -> Result<(), Self::Error> { self.step(StreamBoundaryStep::NotifyWorkerBoundary) }
        fn flush_runtime_io(&mut self, _: &StreamBoundaryResetPlan) -> Result<(), Self::Error> { self.step(StreamBoundaryStep::FlushRuntimeIo) }
        fn clear_fmq(&mut self, _: &StreamBoundaryResetPlan) -> Result<(), Self::Error> { self.step(StreamBoundaryStep::ClearFmq) }
        fn reset_packet_pipeline(&mut self, _: &StreamBoundaryResetPlan) -> Result<(), Self::Error> { self.step(StreamBoundaryStep::ResetPacketPipeline) }
        fn discard_av_payloads(&mut self, _: &StreamBoundaryResetPlan) -> Result<(), Self::Error> { self.step(StreamBoundaryStep::DiscardAvPayloads) }
        fn reset_dvr_playback(&mut self, _: &StreamBoundaryResetPlan) -> Result<(), Self::Error> { self.step(StreamBoundaryStep::ResetDvrPlayback) }
        fn commit_generation(&mut self, _: &StreamBoundaryResetPlan) -> Result<(), Self::Error> { self.step(StreamBoundaryStep::CommitGeneration) }
    }

    impl FailingBoundaryResources {
        fn step(&mut self, step: StreamBoundaryStep) -> Result<(), StreamBoundaryStep> {
            self.steps.push(step);
            if self.fail_once_at == Some(step) {
                self.fail_once_at = None;
                Err(step)
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn pending_stream_boundary_retries_from_failed_step() {
        let plan = StreamBoundaryResetPlan::for_demux(StreamBoundaryReason::TuneStart, 3, 4);
        let mut first = FailingBoundaryResources { fail_once_at: Some(StreamBoundaryStep::ResetPacketPipeline), steps: Vec::new() };
        let failure = plan.try_execute_from_step(&mut first, None).unwrap_err();
        assert_eq!(failure.result.failed_step, Some(StreamBoundaryStep::ResetPacketPipeline));
        let pending = PendingStreamBoundaryPlan::new(plan, failure.result.failed_step);
        let mut retry = FailingBoundaryResources { fail_once_at: None, steps: Vec::new() };
        let retry_result = pending.try_execute(&mut retry).unwrap();
        assert!(!retry_result.boundary_failed);
        assert_eq!(retry.steps.first().copied(), Some(StreamBoundaryStep::ResetPacketPipeline));
        assert_eq!(retry.steps.last().copied(), Some(StreamBoundaryStep::CommitGeneration));
    }
}


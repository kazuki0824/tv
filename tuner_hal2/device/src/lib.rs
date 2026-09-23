//! tuner_hal2 device層。
//!
//! このcrateはdriver ABI断片とfrontend runtime transactionを所有する。AIDL objectやdemux lifecycleは所有しない。

pub mod dvb;
pub mod px4;
mod runtime;

pub use runtime::{
    apply_frontend_backend_lnb_voltage, apply_frontend_backend_lnb_voltage_classified,
    run_frontend_backend_tune_worker, run_frontend_backend_tune_worker_with_previous,
    BackendTuneRollbackFailure, BackendTuneRollbackStep, BackendTuneStep,
    FrontendBackendFailureDiagnostic, FrontendBackendLnbApplyOutcome, FrontendBackendLnbApplyPlan,
    FrontendBackendSession, FrontendBackendSessionKind, FrontendBackendSubmitFailure,
    FrontendBackendTunePlan, FrontendDiagnosticWriteFailure, FrontendLivePacketSink,
    FrontendLivePumpJoinOutcome, FrontendLivePumpOwner, FrontendLivePumpReport,
    FrontendLiveReaderDescriptor, FrontendLiveReaderDescriptorKind, FrontendLnbVoltage,
    FrontendRuntime, FrontendRuntimeSnapshot, FrontendRuntimeState, FrontendScanPhase,
    FrontendScanSession, FrontendScanTerminalReason, FrontendSignalState, FrontendTerminalEvent,
    FrontendTerminalEventKind, FrontendTerminalEventReason,
    FrontendTmccPartialReceptionObservation, FrontendTmccTsidListObservation,
    FrontendWorkerCancelReason, FrontendWorkerContext, FrontendWorkerKind, FrontendWorkerRegistry,
    FrontendWorkerStartError, FrontendWorkerStopOutcome, FrontendWorkerStopPoll,
    FrontendWorkerStopTicket, FrontendWorkerSubmitWait, Px4ChannelApplyResult,
};

//! tuner_hal2 device層。
//!
//! このcrateはdriver ABI断片とfrontend runtime transactionを所有する。AIDL objectやdemux lifecycleは所有しない。

pub mod dvb;
pub mod px4;
pub mod runtime;

pub use runtime::{
    run_frontend_backend_tune_worker, run_frontend_backend_tune_worker_with_previous,
    run_frontend_live_pump, run_frontend_live_pump_limited, BackendTuneCommit, BackendTuneOps,
    BackendTuneOutcome, BackendTuneRollbackFailure, BackendTuneRollbackReport,
    BackendTuneRollbackStep, BackendTuneStep, BackendTuneTxn, FrontendBackendSession,
    FrontendBackendSessionKind, FrontendBackendTunePlan, FrontendLivePacketSink,
    FrontendLivePumpJoinOutcome, FrontendLivePumpOwner, FrontendLivePumpReport,
    FrontendLiveReaderDescriptor, FrontendLiveReaderDescriptorKind, FrontendRuntime,
    FrontendRuntimeSnapshot, FrontendRuntimeState, FrontendScanPhase, FrontendScanSession,
    FrontendScanTerminalReason, FrontendSignalState, FrontendTerminalEvent,
    FrontendTerminalEventKind, FrontendTerminalEventReason, FrontendTuneOutcome, FrontendTuneTxn,
    FrontendWorkerCancelReason, FrontendWorkerContext, FrontendWorkerKind, FrontendWorkerRegistry,
    FrontendWorkerStartError, FrontendWorkerStopOutcome, TuneWorkerStart,
};

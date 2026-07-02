//! frontend device runtime。
//!
//! frontend backend runtime状態とtune transaction境界を所有する。AIDL objectは公開せず、demux ledgerを直接更新しない。

pub mod backend_worker;
pub mod frontend_runtime;
pub mod frontend_worker;
pub mod live_pump;
pub mod reader;
pub mod scan_session;
pub(crate) mod thread_result_owner;
pub mod tune_txn;

pub use backend_worker::{
    apply_frontend_backend_lnb_voltage, run_frontend_backend_tune_worker,
    run_frontend_backend_tune_worker_with_previous, FrontendBackendLnbApplyPlan,
    FrontendBackendSession, FrontendBackendSessionKind, FrontendBackendTunePlan,
    FrontendLnbVoltage,
};
pub use frontend_runtime::{
    FrontendDiagnosticWriteFailure, FrontendLivePumpDiagnostic, FrontendLivePumpJoinResult,
    FrontendLivePumpTerminalReason, FrontendRuntime, FrontendRuntimeSnapshot, FrontendRuntimeState,
    FrontendSignalState, FrontendTerminalEvent, FrontendTerminalEventKind,
    FrontendTerminalEventReason,
};
pub use frontend_worker::{
    FrontendWorkerCancelReason, FrontendWorkerContext, FrontendWorkerDetachedJoin,
    FrontendWorkerKind, FrontendWorkerRegistry, FrontendWorkerStartError,
    FrontendWorkerStopOutcome, FrontendWorkerStopTicket,
};
pub use live_pump::{
    run_frontend_live_pump, run_frontend_live_pump_limited, FrontendLivePacketSink,
    FrontendLivePumpJoinOutcome, FrontendLivePumpOwner, FrontendLivePumpReport,
};
pub use reader::{FrontendLiveReaderDescriptor, FrontendLiveReaderDescriptorKind};
pub use scan_session::{FrontendScanPhase, FrontendScanSession, FrontendScanTerminalReason};
pub use tune_txn::{
    BackendTuneCommit, BackendTuneOps, BackendTuneOutcome, BackendTuneRollbackFailure,
    BackendTuneRollbackReport, BackendTuneRollbackStep, BackendTuneStep, BackendTuneTxn,
    FrontendTuneOutcome, FrontendTuneTxn, TuneWorkerStart,
};

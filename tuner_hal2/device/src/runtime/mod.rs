//! frontend device runtime。
//!
//! frontend backend runtime状態とtune transaction境界を所有する。AIDL objectは公開せず、demux ledgerを直接更新しない。

mod backend_worker;
mod frontend_runtime;
mod frontend_worker;
mod live_pump;
mod reader;
mod scan_session;
pub(crate) mod thread_result_owner;
pub(crate) mod tune_txn;

pub use backend_worker::{
    apply_frontend_backend_lnb_voltage, run_frontend_backend_tune_worker,
    run_frontend_backend_tune_worker_with_previous, FrontendBackendLnbApplyPlan,
    FrontendBackendSession, FrontendBackendSessionKind, FrontendBackendSubmitFailure,
    FrontendBackendSubmitTicket, FrontendBackendSubmitWait, FrontendBackendTunePlan,
    FrontendLnbVoltage,
};
pub use frontend_runtime::{
    FrontendRuntime, FrontendRuntimeSnapshot, FrontendRuntimeState, FrontendSignalState,
    FrontendTerminalEvent, FrontendTerminalEventKind, FrontendTerminalEventReason,
};
pub use frontend_worker::{
    FrontendWorkerCancelReason, FrontendWorkerContext, FrontendWorkerDetachedJoin,
    FrontendWorkerKind, FrontendWorkerRegistry, FrontendWorkerStartError,
    FrontendWorkerStopOutcome, FrontendWorkerStopPoll, FrontendWorkerStopTicket,
};
pub use live_pump::{
    run_frontend_live_pump, run_frontend_live_pump_limited, FrontendLivePacketSink,
    FrontendLivePumpJoinOutcome, FrontendLivePumpOwner, FrontendLivePumpReport,
};
pub use reader::{FrontendLiveReaderDescriptor, FrontendLiveReaderDescriptorKind};
pub use scan_session::{FrontendScanPhase, FrontendScanSession, FrontendScanTerminalReason};

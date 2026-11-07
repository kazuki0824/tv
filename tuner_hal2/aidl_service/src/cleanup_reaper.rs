use maleicacid_tuner_hal2_common::{PoisonTrackedMutex, RuntimeLockKind};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use maleicacid_tuner_hal2_common::{HalError, HalInternalKind};
use maleicacid_tuner_hal2_domain_request::AidlMethodCall;
use maleicacid_tuner_hal2_resource_ledger::CleanupStep;
use maleicacid_tuner_hal2_service_runtime::CapabilitySnapshot;

use crate::object_handle::{AidlObjectHandle, AidlObjectKind};
use crate::service_context::AidlServiceContext;

#[derive(Clone, Copy, Debug)]
struct CleanupReaperPolicy {
    max_jobs: usize,
    terminal_deadline: Duration,
    retry_delays: [Duration; 4],
}

impl CleanupReaperPolicy {
    fn from_snapshot(snapshot: CapabilitySnapshot) -> Self {
        Self {
            max_jobs: snapshot.cleanup_reaper_capacity,
            terminal_deadline: Duration::from_millis(snapshot.cleanup_terminal_deadline_ms),
            retry_delays: snapshot
                .cleanup_retry_schedule_ms
                .map(Duration::from_millis),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct CleanupJobKey {
    kind: u8,
    object_id: i64,
    generation: u64,
}

impl CleanupJobKey {
    fn from_handle(handle: AidlObjectHandle) -> Self {
        let kind = match handle.object_kind() {
            AidlObjectKind::Tuner => 0,
            AidlObjectKind::Frontend => 1,
            AidlObjectKind::Demux => 2,
            AidlObjectKind::Filter => 3,
            AidlObjectKind::Dvr => 4,
            AidlObjectKind::Descrambler => 5,
            AidlObjectKind::Lnb => 6,
        };
        Self {
            kind,
            object_id: handle.object_id().0,
            generation: handle.generation().0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct CleanupJob {
    handle: AidlObjectHandle,
    registered_at: Instant,
}

pub(crate) struct CleanupReaperQueue {
    policy: CleanupReaperPolicy,
    runtime: PoisonTrackedMutex<
        Option<
            maleicacid_tuner_hal2_service_runtime::WorkerRuntimeReaperQueue<
                CleanupJobKey,
                CleanupStep,
                CleanupJob,
            >,
        >,
    >,
}

impl core::fmt::Debug for CleanupReaperQueue {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CleanupReaperQueue")
            .field("policy", &self.policy)
            .finish_non_exhaustive()
    }
}

impl CleanupReaperQueue {
    pub(crate) fn from_snapshot(snapshot: CapabilitySnapshot) -> Self {
        Self {
            policy: CleanupReaperPolicy::from_snapshot(snapshot),
            runtime: PoisonTrackedMutex::new(None, RuntimeLockKind::CleanupReaperOwner),
        }
    }

    fn install(
        &self,
        runtime: maleicacid_tuner_hal2_service_runtime::WorkerRuntimeReaperQueue<
            CleanupJobKey,
            CleanupStep,
            CleanupJob,
        >,
    ) -> Result<(), HalError> {
        let mut slot = self.runtime.lock().map_err(HalError::LockPoisoned)?;
        if slot.is_some() {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "cleanup reaperの正規ownerが重複して設定されました",
            ));
        }
        *slot = Some(runtime);
        Ok(())
    }

    fn classify_enqueue_error(error: HalError) -> HalError {
        if maleicacid_tuner_hal2_service_runtime::WorkerFailureClassifier::reaper_enqueue_worker_lock_poisoned(&error) {
            HalError::composed_failure(
                "cleanup reaper worker lock失陥",
                HalError::WorkerReaperUnavailable,
                error,
            )
        } else {
            error
        }
    }

    pub(crate) fn enqueue(
        &self,
        handle: AidlObjectHandle,
        dependency: CleanupStep,
    ) -> Result<(), HalError> {
        let key = CleanupJobKey::from_handle(handle);
        let slot = self.runtime.lock().map_err(HalError::LockPoisoned)?;
        let runtime = slot.as_ref().ok_or_else(|| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "cleanup reaperの正規ownerが設定されていません",
            )
        })?;
        if runtime
            .pending_value(&key)
            .map_err(Self::classify_enqueue_error)?
            .is_some()
        {
            return Ok(());
        }
        runtime
            .enqueue_reserved(
                CleanupJob {
                    handle,
                    registered_at: Instant::now(),
                },
                [(key, dependency)],
            )
            .map_err(Self::classify_enqueue_error)
    }
}

fn close_method(kind: AidlObjectKind) -> Result<AidlMethodCall, HalError> {
    match kind {
        AidlObjectKind::Frontend => Ok(AidlMethodCall::FrontendClose),
        AidlObjectKind::Demux => Ok(AidlMethodCall::DemuxClose),
        AidlObjectKind::Filter => Ok(AidlMethodCall::FilterClose),
        AidlObjectKind::Dvr => Ok(AidlMethodCall::DvrClose),
        AidlObjectKind::Descrambler => Ok(AidlMethodCall::DescramblerClose),
        AidlObjectKind::Lnb => Ok(AidlMethodCall::LnbClose),
        AidlObjectKind::Tuner => Err(HalError::internal(
            HalInternalKind::InvariantViolation,
            "root tuner objectがcleanup reaper queueに入りました",
        )),
    }
}

fn mark_cleanup_reaper_critical(context: &AidlServiceContext) {
    let shared_runtime = context.runtime();
    maleicacid_tuner_hal2_service_runtime::TunerServiceRuntime::mark_shared_service_critical(
        &shared_runtime,
    );
}

fn run_cleanup_job(
    context: Weak<AidlServiceContext>,
    policy: CleanupReaperPolicy,
    job: CleanupJob,
    pending: maleicacid_tuner_hal2_service_runtime::WorkerRuntimeReaperPending<
        CleanupJobKey,
        CleanupStep,
    >,
    worker: maleicacid_tuner_hal2_service_runtime::WorkerContext,
) {
    let key = CleanupJobKey::from_handle(job.handle);
    let mut attempt = 0usize;
    loop {
        if worker.stop_requested() {
            return;
        }
        let Some(context) = context.upgrade() else {
            return;
        };
        use maleicacid_tuner_hal2_service_runtime::AidlObjectCleanupDisposition;
        // 期限前後で同じ状態・所有権判定を使う。期限は物理隔離試行の契機に限る。
        match context.cleanup_retry_disposition(job.handle) {
            Ok(
                AidlObjectCleanupDisposition::Stale
                | AidlObjectCleanupDisposition::Closed
                | AidlObjectCleanupDisposition::NoCleanupPending,
            ) => return,
            Ok(AidlObjectCleanupDisposition::Quarantined) => {
                if context.quarantine_cleanup_has_owner(job.handle) == Ok(true) {
                    return;
                }
                mark_cleanup_reaper_critical(&context);
                return;
            }
            Ok(AidlObjectCleanupDisposition::RequiresIsolation) => {}
            Err(error) => {
                log::error!(
                    "cleanup reaper対象の状態を確認できません: handle={:?} error={error:?}",
                    job.handle
                );
                mark_cleanup_reaper_critical(&context);
                return;
            }
        }
        if job.registered_at.elapsed() >= policy.terminal_deadline {
            match crate::object_runtime::quarantine_drop_leak_object(&context, job.handle) {
                Ok(crate::object_runtime::DropLeakQuarantineOutcome::Completed)
                | Ok(crate::object_runtime::DropLeakQuarantineOutcome::SupervisedPending) => {
                    // ObjectCloseTxnが完了義務を保持し、Supervisorからjoin通知を受ける。
                    // cleanup reaperの実行レーンは占有し続けない。
                    return;
                }
                Ok(crate::object_runtime::DropLeakQuarantineOutcome::OwnedByExistingCleanup) => {
                    // 親の未開始権限を見ただけで子ジョブを終了してはならない。
                    // 既存親ownerが実行中、または正規reaperに登録された場合だけ移管する。
                    match context.ensure_related_cleanup_progress(job.handle) {
                        Ok(true) => {
                            log::debug!(
                                "親ObjectCloseTxnの正規回収経路へ委譲: handle={:?}",
                                job.handle
                            );
                            return;
                        }
                        Ok(false) => {
                            log::error!(
                                "親cleanupへの進行委譲を証明できません: handle={:?}",
                                job.handle
                            );
                        }
                        Err(error) => {
                            log::error!(
                                "親cleanupの回収登録に失敗: handle={:?} error={error:?}",
                                job.handle
                            );
                        }
                    }
                    // 実行中または予約済みという証拠なしに成功終了させない。
                    mark_cleanup_reaper_critical(&context);
                    return;
                }
                Err(error) => {
                    log::error!(
                        "cleanup reaper期限での対象隔離に失敗: handle={:?} error={error:?}",
                        job.handle
                    );
                    mark_cleanup_reaper_critical(&context);
                    return;
                }
            }
        }
        let dependency = match context.cleanup_dependency_for_handle(job.handle) {
            Ok(dependency) => dependency,
            Err(_) => {
                // 初回状態確認後に別のownerが隔離した可能性がある。
                // 同じcleanup正本で所有権を再確認する。
                if matches!(
                    context.cleanup_retry_disposition(job.handle),
                    Ok(AidlObjectCleanupDisposition::Stale
                        | AidlObjectCleanupDisposition::Closed
                        | AidlObjectCleanupDisposition::NoCleanupPending)
                ) || context.quarantine_cleanup_has_owner(job.handle) == Ok(true)
                {
                    return;
                }
                mark_cleanup_reaper_critical(&context);
                return;
            }
        };
        if pending.update_value(&key, dependency).is_err() {
            mark_cleanup_reaper_critical(&context);
            return;
        }
        let result = close_method(job.handle.object_kind()).and_then(|method| {
            crate::object_runtime::retry_cleanup_from_reaper(&context, job.handle, method)
        });
        if result.is_ok() {
            return;
        }
        attempt = attempt.saturating_add(1);
        let delay = policy
            .retry_delays
            .get(attempt)
            .copied()
            .unwrap_or(Duration::from_millis(1_000));
        let remaining = policy
            .terminal_deadline
            .saturating_sub(job.registered_at.elapsed());
        let deadline = Instant::now().checked_add(delay.min(remaining));
        worker.wait_until(deadline);
    }
}

pub(crate) fn start_cleanup_reaper(
    context: Weak<AidlServiceContext>,
    queue: Arc<CleanupReaperQueue>,
) -> Result<(), HalError> {
    let policy = queue.policy;
    let runner_context = context;
    let runner = Arc::new(move |job, pending, worker| {
        run_cleanup_job(runner_context.clone(), policy, job, pending, worker);
    });
    let owner = maleicacid_tuner_hal2_service_runtime::WorkerRuntime::start_reaper_queue(
        policy.max_jobs,
        "tuner-hal2-cleanup-reaper",
        runner,
    )?;
    queue.install(owner)
}

#[cfg(test)]
mod tests {

    // 競合の検査対象は実ObjectCloseTxn。物理操作の成功は既存executor境界から注入する。
    fn simulated_cleanup_completion(
        attempt: maleicacid_tuner_hal2_service_runtime::ObjectCloseCleanupAttempt,
    ) -> maleicacid_tuner_hal2_service_runtime::CloseCleanupAttemptCompletion {
        use maleicacid_tuner_hal2_service_runtime::{
            ObjectArtifactCleanupCommand, ObjectArtifactCleanupExecutor, ObjectCloseCleanupFailure,
            ObjectCloseRuntimeExecutor, ObjectDomainCleanupCommand, ObjectDomainCleanupExecutor,
            ObjectRuntimeCleanupCommand,
        };

        struct SuccessfulCleanup;
        impl ObjectCloseRuntimeExecutor for SuccessfulCleanup {
            fn execute_runtime_cleanup(
                &mut self,
                _: ObjectRuntimeCleanupCommand,
            ) -> Result<(), ObjectCloseCleanupFailure> {
                Ok(())
            }
        }
        impl ObjectDomainCleanupExecutor for SuccessfulCleanup {
            fn execute_frontend_cleanup(
                &mut self,
                _: ObjectDomainCleanupCommand,
            ) -> Result<(), HalError> {
                Ok(())
            }
            fn execute_lnb_cleanup(
                &mut self,
                _: ObjectDomainCleanupCommand,
            ) -> Result<(), HalError> {
                Ok(())
            }
            fn execute_lnb_drop_leak_record(
                &mut self,
                _: ObjectDomainCleanupCommand,
            ) -> Result<(), HalError> {
                Ok(())
            }
        }
        impl ObjectArtifactCleanupExecutor for SuccessfulCleanup {
            fn execute_owner_callback_cleanup(
                &mut self,
                _: ObjectArtifactCleanupCommand,
            ) -> Result<(), ObjectCloseCleanupFailure> {
                Ok(())
            }
            fn execute_descendant_callback_cleanup(
                &mut self,
                _: ObjectArtifactCleanupCommand,
            ) -> Result<(), ObjectCloseCleanupFailure> {
                Ok(())
            }
            fn execute_lnb_owner_loss_callback_cleanup(
                &mut self,
                _: ObjectArtifactCleanupCommand,
            ) -> Result<(), ObjectCloseCleanupFailure> {
                Ok(())
            }
            fn execute_dvr_status_notifier_cleanup(
                &mut self,
                _: ObjectArtifactCleanupCommand,
            ) -> Result<(), ObjectCloseCleanupFailure> {
                Ok(())
            }
        }
        let (completion, report) = attempt.execute_cleanup_report_with_executor(
            &mut SuccessfulCleanup,
            &mut SuccessfulCleanup,
            &mut SuccessfulCleanup,
        );
        assert!(report.into_result().is_ok());
        completion
    }

    #[test]
    fn concurrent_close_cleanup_defers_quarantine_without_service_shutdown() {
        use maleicacid_tuner_hal2_domain_request::{AidlApi, AidlMethodCall};
        use maleicacid_tuner_hal2_service_runtime::{
            close_object_use_case, AidlObjectCleanupDisposition, TunerServiceRuntime,
        };
        use std::sync::Mutex;

        let mut runtime = TunerServiceRuntime::new();
        let demux = runtime
            .root_open_txn()
            .open_demux_root_object(AidlMethodCall::PublicApi {
                object: AidlObjectKind::Tuner,
                api: AidlApi::TunerOpenDemux,
            })
            .expect("Demuxを公開する");
        let close = close_object_use_case(
            &mut runtime,
            demux.object_id(),
            demux.generation(),
            AidlObjectKind::Demux,
            AidlMethodCall::DemuxClose,
        )
        .expect("既存close開始");
        let _active_attempt = close
            .begin_cleanup_attempt(&mut runtime)
            .expect("既存closeの物理cleanup実行権限を取得する");
        let failure_state = runtime.failure_state();
        let handle =
            AidlObjectHandle::new(AidlObjectKind::Demux, demux.object_id(), demux.generation());
        let context =
            AidlServiceContext::from_shared_runtime_for_test(Arc::new(Mutex::new(runtime)));

        let result = crate::object_runtime::quarantine_drop_leak_object(&context, handle)
            .expect("既存cleanup権限との競合を型付きで返す");
        assert_eq!(
            result,
            crate::object_runtime::DropLeakQuarantineOutcome::OwnedByExistingCleanup
        );
        assert_eq!(
            context.cleanup_retry_disposition(handle),
            Ok(AidlObjectCleanupDisposition::RequiresIsolation)
        );
        assert!(!failure_state.snapshot().service_critical);
    }

    #[test]
    fn issued_but_unstarted_parent_cleanup_progresses_without_public_retry() {
        use maleicacid_tuner_hal2_domain_request::{AidlApi, AidlMethodCall};
        use maleicacid_tuner_hal2_resource_ledger::{LedgerGeneration, LedgerId};
        use maleicacid_tuner_hal2_service_runtime::{
            close_object_use_case, RuntimeObjectEntry, RuntimeOwnerRelation,
            TunerServiceRuntime,
        };
        use std::sync::Mutex;

        let mut runtime = TunerServiceRuntime::new();
        let parent = runtime
            .root_open_txn()
            .open_demux_root_object(AidlMethodCall::PublicApi {
                object: AidlObjectKind::Tuner,
                api: AidlApi::TunerOpenDemux,
            })
            .expect("親Demuxを開く");
        let child_id = AidlObjectId(977);
        let generation = parent.generation();
        runtime
            .object_table_mut()
            .insert(RuntimeObjectEntry {
                last_reference_ended: false,
                object_kind: AidlObjectKind::Filter,
                object_id: child_id,
                generation,
                ledger_id: LedgerId(977),
                ledger_generation: LedgerGeneration(1),
                owner: RuntimeOwnerRelation::Demux {
                    demux: parent.object_id(),
                    generation,
                },
                lifecycle: runtime
                    .object_table()
                    .entry(parent.object_id())
                    .expect("親はLive")
                    .lifecycle,
            })
            .expect("子の公開登録");
        // 子は隔離済みで親closeが物理処理すべき対象は親だけとする。
        runtime
            .object_table_mut()
            .quarantine_cascade(child_id, generation)
            .expect("子を隔離");
        let _unstarted = close_object_use_case(
            &mut runtime,
            parent.object_id(),
            generation,
            AidlObjectKind::Demux,
            AidlMethodCall::DemuxClose,
        )
        .expect("親close権限は発行したが物理cleanupは未開始");
        let parent_handle = AidlObjectHandle::new(
            AidlObjectKind::Demux,
            parent.object_id(),
            generation,
        );
        let child_handle = AidlObjectHandle::new(AidlObjectKind::Filter, child_id, generation);
        let context =
            AidlServiceContext::from_shared_runtime_for_test(Arc::new(Mutex::new(runtime)));
        assert_eq!(
            crate::object_runtime::quarantine_drop_leak_object(&context, child_handle)
                .expect("親ownerへの委譲"),
            crate::object_runtime::DropLeakQuarantineOutcome::OwnedByExistingCleanup
        );
        assert!(context
            .ensure_related_cleanup_progress(child_handle)
            .expect("親のreaper投入を保証"));
        for _ in 0..200 {
            if context.cleanup_retry_disposition(parent_handle)
                == Ok(maleicacid_tuner_hal2_service_runtime::AidlObjectCleanupDisposition::Closed)
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("公開closeを再呼出しせず親の未開始cleanupを自律回収できませんでした");
    }

    #[test]
    fn poisoned_owner_slot_rejects_enqueue_with_identity() {
        let queue = CleanupReaperQueue::from_snapshot(
            maleicacid_tuner_hal2_service_runtime::TunerServiceRuntime::default()
                .capability_snapshot(),
        );
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = queue.runtime.lock().unwrap();
            panic!("汚染を注入");
        }));
        // インストール状態に依存せず、汚染した所有者格納領域を読まない。
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Filter,
            AidlObjectId(7),
            AidlObjectGeneration(3),
        );
        let error = queue.enqueue(handle, CleanupStep::StopWorker).unwrap_err();
        assert!(
            matches!(error, HalError::LockPoisoned(poison) if poison.lock == RuntimeLockKind::CleanupReaperOwner && poison.poison_count == 1)
        );
    }

    use super::*;
    use maleicacid_tuner_hal2_domain_request::{AidlObjectGeneration, AidlObjectId};

    #[test]
    fn cleanup_pending_frontend_reaper_reaches_closed_and_allows_reopen() {
        use maleicacid_tuner_hal2_common::{FrontendBackendKind, FrontendSystem};
        use maleicacid_tuner_hal2_domain_request::AidlApi;
        use maleicacid_tuner_hal2_service_runtime::{
            close_object_use_case, finish_object_close_use_case, FrontendCapabilitySnapshot,
            FrontendProbeOutcome, FrontendRuntimeId, FrontendScalarCapability,
            ObjectCloseCleanupFailure, SatellitePowerTopology, ServiceBootOutcome,
            TunerServiceRuntime,
        };
        use std::sync::Mutex;

        let frontend_id = 7;
        let mut runtime = TunerServiceRuntime::new();
        assert_eq!(
            runtime.boot_from_probe_results([FrontendProbeOutcome::Available {
                id: FrontendRuntimeId(frontend_id),
                backend: FrontendBackendKind::Px4CharDevice,
                system: FrontendSystem::IsdbT,
                path: "/dev/null".into(),
                lnb_profile: None,
                satellite_power_topology: SatellitePowerTopology::UnknownOrDisabled,
                capability: FrontendCapabilitySnapshot {
                    scalar: FrontendScalarCapability {
                        min_frequency_hz: 473_142_857,
                        max_frequency_hz: 473_142_857,
                        min_symbol_rate: 0,
                        max_symbol_rate: 0,
                        acquire_range_hz: 0,
                    },
                    exclusive_group_id: 7,
                    isdbt_segment: None,
                },
            }]),
            ServiceBootOutcome::Ready
        );
        let first = runtime
            .root_open_txn()
            .open_frontend_root_object_for_id(
                frontend_id,
                AidlMethodCall::PublicApi {
                    object: AidlObjectKind::Tuner,
                    api: AidlApi::TunerOpenFrontendById,
                },
            )
            .expect("最初のfrontend openが成功する");
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Frontend,
            first.object_id(),
            first.generation(),
        );

        let close = close_object_use_case(
            &mut runtime,
            first.object_id(),
            first.generation(),
            AidlObjectKind::Frontend,
            AidlMethodCall::FrontendClose,
        )
        .expect("frontend close plan開始が成功する");
        let attempt = close
            .begin_cleanup_attempt(&mut runtime)
            .expect("最初のcleanup試行開始が成功する");
        finish_object_close_use_case(
            &mut runtime,
            simulated_cleanup_completion(attempt),
            Err(ObjectCloseCleanupFailure::new(
                CleanupStep::ReleaseBackend,
                HalError::cleanup_failed(
                    "Issue #181の遅延cleanup",
                    "worker cleanupは引き続きreaperが所有しています",
                ),
            )),
        )
        .expect_err("最初のcleanupがpendingのまま残る");

        let runtime = Arc::new(Mutex::new(runtime));
        let context = AidlServiceContext::from_shared_runtime_for_test(Arc::clone(&runtime));
        context
            .enqueue_cleanup_retry(handle)
            .expect("cleanup retryがqueueへ登録される");

        for _ in 0..200 {
            if context
                .cleanup_is_terminal_for_handle(handle)
                .expect("cleanup terminal stateを引き続き読める")
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            context
                .cleanup_is_terminal_for_handle(handle)
                .expect("cleanupがterminal stateへ到達する"),
            "cleanup reaperがterminal object stateへ到達しませんでした"
        );

        let mut runtime = runtime.lock().expect("runtime lockが健全なままである");
        assert!(
            runtime
                .runtime_object_diagnostic_snapshots()
                .iter()
                .all(|snapshot| snapshot.object_id() != first.object_id()),
            "cleanup成功時はQuarantinedではなくClosedへ到達しなければなりません"
        );
        runtime
            .root_open_txn()
            .open_frontend_root_object_for_id(
                frontend_id,
                AidlMethodCall::PublicApi {
                    object: AidlObjectKind::Tuner,
                    api: AidlApi::TunerOpenFrontendById,
                },
            )
            .expect("cleanup reaperがClosedへ到達した後にfrontendを再openできる");
    }

    #[test]
    fn cleanup_job_key_is_identity_only() {
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Filter,
            AidlObjectId(7),
            AidlObjectGeneration(3),
        );
        assert_eq!(
            CleanupJobKey::from_handle(handle),
            CleanupJobKey {
                kind: 3,
                object_id: 7,
                generation: 3
            }
        );
    }
}

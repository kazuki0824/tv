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

/// 物理cleanupの正本はObjectCloseTxn。ここでは既存reaperと、
/// 満杯時の受付継続を監視する従属WorkerRuntimeのhandleだけを所有する。
#[derive(Clone, Copy)]
struct CleanupAdmission {
    handle: AidlObjectHandle,
    registered_at: Instant,
}

struct CleanupReaperQueueOwner {
    queue: Option<
        maleicacid_tuner_hal2_service_runtime::WorkerRuntimeReaperQueue<
            CleanupJobKey,
            CleanupStep,
            CleanupJob,
        >,
    >,
    /// reaperが満杯でも回収が未完のownerを列挙する受付予定。義務正本はObjectCloseTxn。
    admission_pending: std::collections::BTreeMap<CleanupJobKey, CleanupAdmission>,
    /// 受付の空き待ちは１本の従属WorkerRuntimeで監視し、別の容量枠を作らない。
    admission_worker: Option<maleicacid_tuner_hal2_service_runtime::WorkerRuntime<()>>,
}

pub(crate) struct CleanupReaperQueue {
    policy: CleanupReaperPolicy,
    runtime: PoisonTrackedMutex<CleanupReaperQueueOwner>,
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
            runtime: PoisonTrackedMutex::new(
                CleanupReaperQueueOwner {
                    queue: None,
                    admission_pending: std::collections::BTreeMap::new(),
                    admission_worker: None,
                },
                RuntimeLockKind::CleanupReaperOwner,
            ),
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
        if slot.queue.is_some() {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "cleanup reaperの正規ownerが重複して設定されました",
            ));
        }
        slot.queue = Some(runtime);
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

    /// reaper容量はbounded受付数であり、全体runtime破損ではない。
    /// 既存retry schedule内だけ受付を待ち、容量回復時に同じ義務を投入する。
    pub(crate) fn enqueue(
        &self,
        handle: AidlObjectHandle,
        dependency: CleanupStep,
    ) -> Result<(), HalError> {
        let key = CleanupJobKey::from_handle(handle);
        let budget = self.policy.retry_delays[2];
        let started = Instant::now();
        loop {
            let runtime = {
                let slot = self.runtime.lock().map_err(HalError::LockPoisoned)?;
                slot.queue
                    .as_ref()
                    .ok_or_else(|| {
                        HalError::internal(
                            HalInternalKind::InvariantViolation,
                            "cleanup reaperの正規ownerが設定されていません",
                        )
                    })?
                    .clone()
            };
            if runtime
                .pending_value(&key)
                .map_err(Self::classify_enqueue_error)?
                .is_some()
            {
                return Ok(());
            }
            match runtime
                .enqueue_reserved(
                    CleanupJob {
                        handle,
                        registered_at: Instant::now(),
                    },
                    [(key, dependency)],
                )
                .map_err(Self::classify_enqueue_error)
            {
                Err(error @ HalError::Busy { .. }) if started.elapsed() < budget => {
                    std::thread::sleep(
                        budget
                            .saturating_sub(started.elapsed())
                            .min(Duration::from_millis(10)),
                    );
                    if started.elapsed() >= budget {
                        return Err(error);
                    }
                }
                result => return result,
            }
        }
    }

    /// 正本ObjectCloseTxnの世代付き未完義務を、容量不足の間も1本のWorkerRuntimeで監督する。
    /// pendingは受付スケジュールであり、物理cleanupの新たな台帳・ownerではない。
    pub(crate) fn supervise_unqueued_owner(
        self: &Arc<Self>,
        context: Weak<AidlServiceContext>,
        handle: AidlObjectHandle,
    ) -> Result<(), HalError> {
        use maleicacid_tuner_hal2_service_runtime::{WorkerRuntime, WorkerTerminalResult};

        let key = CleanupJobKey::from_handle(handle);
        let mut owner = self.runtime.lock().map_err(HalError::LockPoisoned)?;
        if owner.admission_worker.as_ref().is_some_and(WorkerRuntime::is_finished) {
            if let Some(finished) = owner.admission_worker.take() {
                match finished.join() {
                    WorkerTerminalResult::Normal(()) | WorkerTerminalResult::StopRequested => {}
                    WorkerTerminalResult::RuntimeFailure(error) => {
                        log::error!("cleanup受付監督workerのjoinが異常: {error:?}");
                        if let Some(context) = context.upgrade() {
                            maleicacid_tuner_hal2_service_runtime::TunerServiceRuntime::mark_shared_service_critical(
                                &context.runtime(),
                            );
                        }
                        return Err(error);
                    }
                    WorkerTerminalResult::PanicOrJoinFailure => {
                        if let Some(context) = context.upgrade() {
                            maleicacid_tuner_hal2_service_runtime::TunerServiceRuntime::mark_shared_service_critical(
                                &context.runtime(),
                            );
                        }
                        return Err(HalError::WorkerReaperPanicked);
                    }
                }
            }
        }
        owner.admission_pending.entry(key).or_insert(CleanupAdmission {
            handle,
            registered_at: Instant::now(),
        });
        if let Some(worker) = owner.admission_worker.as_ref() {
            // 新しいentryの登録は既存WorkerRuntimeのwakeで通知する。
            return worker.wake();
        }
        let queue = owner
            .queue
            .as_ref()
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "cleanup reaper ownerなし",
                )
            })?
            .clone();
        let weak_queue = Arc::downgrade(self);
        let observer_context = context.clone();
        let budget = self.policy.terminal_deadline;
        let worker = WorkerRuntime::spawn_observed(
            "tuner-hal2-cleanup-admission".to_string(),
            0,
            1,
            move |control| -> Result<(), HalError> {
                use maleicacid_tuner_hal2_service_runtime::AidlObjectCleanupDisposition;
                loop {
                    if control.stop_requested() {
                        return Ok(());
                    }
                    let Some(queue_owner) = weak_queue.upgrade() else {
                        return Ok(());
                    };
                    let Some(context) = context.upgrade() else {
                        return Ok(());
                    };
                    let tasks: Vec<(CleanupJobKey, CleanupAdmission)> = {
                        let state = queue_owner.runtime.lock().map_err(HalError::LockPoisoned)?;
                        state
                            .admission_pending
                            .iter()
                            .map(|(key, task)| (*key, *task))
                            .collect()
                    };
                    for (key, task) in tasks {
                        let handle = task.handle;
                        let disposition = context.cleanup_retry_disposition(handle)?;
                        let terminal = match disposition {
                            AidlObjectCleanupDisposition::Stale
                            | AidlObjectCleanupDisposition::Closed
                            | AidlObjectCleanupDisposition::NoCleanupPending => true,
                            AidlObjectCleanupDisposition::Quarantined => {
                                context.quarantine_cleanup_has_owner(handle)?
                            }
                            AidlObjectCleanupDisposition::RequiresIsolation => false,
                        };
                        let managed = if terminal {
                            true
                        } else {
                            queue
                                .pending_value(&key)
                                .map_err(CleanupReaperQueue::classify_enqueue_error)?
                                .is_some()
                        };
                        if managed {
                            queue_owner
                                .runtime
                                .lock()
                                .map_err(HalError::LockPoisoned)?
                                .admission_pending
                                .remove(&key);
                            continue;
                        }
                        // 既存closeが物理cleanup中ならその実行権限を奪って再実行しない。
                        let active = context
                            .related_cleanup_recovery_owners(handle)?
                            .iter()
                            .any(|(_, active)| *active);
                        if !active {
                            if let Ok(dependency) = context.cleanup_dependency_for_handle(handle) {
                                match queue
                                    .enqueue_reserved(
                                        CleanupJob {
                                            handle,
                                            registered_at: Instant::now(),
                                        },
                                        [(key, dependency)],
                                    )
                                    .map_err(CleanupReaperQueue::classify_enqueue_error)
                                {
                                    Ok(()) => {
                                        queue_owner
                                            .runtime
                                            .lock()
                                            .map_err(HalError::LockPoisoned)?
                                            .admission_pending
                                            .remove(&key);
                                        continue;
                                    }
                                    Err(HalError::Busy { .. }) => {}
                                    Err(error) => {
                                        // pending照会後に別の実行主体が予約した競合は成功。
                                        if queue
                                            .pending_value(&key)
                                            .map_err(CleanupReaperQueue::classify_enqueue_error)?
                                            .is_some()
                                        {
                                            queue_owner
                                                .runtime
                                                .lock()
                                                .map_err(HalError::LockPoisoned)?
                                                .admission_pending
                                                .remove(&key);
                                            continue;
                                        }
                                        return Err(error);
                                    }
                                }
                            }
                        }
                        if task.registered_at.elapsed() >= budget {
                            // 期限は局所隔離の再確認を開始する条件であって、
                            // 旧ownerの物理cleanupを重ねて良い根拠にはならない。
                            let isolated = crate::object_runtime::quarantine_drop_leak_object(
                                &context,
                                handle,
                            );
                            if matches!(
                                isolated,
                                Ok(crate::object_runtime::DropLeakQuarantineOutcome::Completed)
                                    | Ok(crate::object_runtime::DropLeakQuarantineOutcome::SupervisedPending)
                            ) || matches!(
                                context.cleanup_retry_disposition(handle)?,
                                AidlObjectCleanupDisposition::Stale
                                    | AidlObjectCleanupDisposition::Closed
                                    | AidlObjectCleanupDisposition::NoCleanupPending
                            ) || context.quarantine_cleanup_has_owner(handle) == Ok(true)
                            {
                                queue_owner
                                    .runtime
                                    .lock()
                                    .map_err(HalError::LockPoisoned)?
                                    .admission_pending
                                    .remove(&key);
                                continue;
                            }
                            return Err(HalError::cleanup_failed(
                                "cleanup reaper受付の監督期限",
                                "既存ownerが終端せず、世代fence付きの局所隔離も確定できません",
                            ));
                        }
                    }
                    // 空き待ちも停止要求で解除でき、追加ownerは同じworkerへwakeする。
                    control.wait_until(Instant::now().checked_add(Duration::from_millis(10)));
                }
            },
            move |terminal| {
                if !matches!(
                    terminal,
                    WorkerTerminalResult::Normal(()) | WorkerTerminalResult::StopRequested
                ) {
                    log::error!("cleanup受付監督workerが失敗しました: result={terminal:?}");
                    if let Some(context) = observer_context.upgrade() {
                        maleicacid_tuner_hal2_service_runtime::TunerServiceRuntime::mark_shared_service_critical(
                            &context.runtime(),
                        );
                    }
                }
            },
        )
        .map_err(|error| HalError::Io {
            backend: "cleanup-admission",
            operation: "受付監督worker起動",
            path: None,
            errno: error.raw_os_error(),
            detail: maleicacid_tuner_hal2_common::HalErrorDetail::new(error.to_string()),
        })?;
        owner.admission_worker = Some(worker);
        Ok(())
    }

    /// 同じ正本reaperが関連ownerの未完cleanupを既に受付済みか確認する。
    pub(crate) fn has_pending(&self, handle: AidlObjectHandle) -> Result<bool, HalError> {
        let slot = self.runtime.lock().map_err(HalError::LockPoisoned)?;
        let runtime = slot.queue.as_ref().ok_or_else(|| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "cleanup reaper ownerなし",
            )
        })?;
        runtime
            .pending_value(&CleanupJobKey::from_handle(handle))
            .map(|entry| entry.is_some())
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
    #[cfg(test)] deadline_checkpoint: Option<&std::sync::mpsc::Sender<()>>,
) {
    use maleicacid_tuner_hal2_service_runtime::AidlObjectCleanupDisposition;
    let origin = job;
    let origin_key = CleanupJobKey::from_handle(origin.handle);
    let mut target = origin;
    let mut attempt = 0usize;
    let mut delegated_wait = None::<Instant>;
    'job: loop {
        if worker.stop_requested() {
            return;
        }
        let Some(context) = context.upgrade() else {
            return;
        };
        let disposition = context.cleanup_retry_disposition(target.handle);
        match disposition {
            Ok(
                AidlObjectCleanupDisposition::Stale
                | AidlObjectCleanupDisposition::Closed
                | AidlObjectCleanupDisposition::NoCleanupPending,
            ) => {
                if target.handle == origin.handle {
                    return;
                }
                target = origin;
                attempt = 0;
                delegated_wait = None;
                continue;
            }
            Ok(AidlObjectCleanupDisposition::Quarantined) => {
                if context.quarantine_cleanup_has_owner(target.handle) == Ok(true) {
                    if target.handle == origin.handle {
                        return;
                    }
                    target = origin;
                    attempt = 0;
                    delegated_wait = None;
                    continue;
                }
                mark_cleanup_reaper_critical(&context);
                return;
            }
            Ok(AidlObjectCleanupDisposition::RequiresIsolation) => {}
            Err(error) => {
                log::error!(
                    "cleanup reaper対象の状態確認失敗: handle={:?} error={error:?}",
                    target.handle
                );
                mark_cleanup_reaper_critical(&context);
                return;
            }
        }
        if target.registered_at.elapsed() >= policy.terminal_deadline {
            match crate::object_runtime::quarantine_drop_leak_object(&context, target.handle) {
                Ok(crate::object_runtime::DropLeakQuarantineOutcome::Completed)
                | Ok(crate::object_runtime::DropLeakQuarantineOutcome::SupervisedPending) => {
                    if target.handle == origin.handle {
                        return;
                    }
                    target = origin;
                    attempt = 0;
                    delegated_wait = None;
                    continue;
                }
                Ok(crate::object_runtime::DropLeakQuarantineOutcome::OwnedByExistingCleanup) => {
                    let owners = match context.related_cleanup_recovery_owners(target.handle) {
                        Ok(owners) => owners,
                        Err(error) => {
                            log::error!("関連cleanup ownerの列挙失敗: {error:?}");
                            mark_cleanup_reaper_critical(&context);
                            return;
                        }
                    };
                    // 未開始のownerは、現在のreaper予約グループを使って回収する。
                    // 同一・祖先・子孫いずれのownerでも別の予約枠を必要としない。
                    for (owner, active) in owners {
                        if active || owner == target.handle {
                            continue;
                        }
                        let key = CleanupJobKey::from_handle(owner);
                        match pending.pending_value(&key) {
                            Ok(Some(_)) => continue,
                            Ok(None) => {}
                            Err(error) => {
                                log::error!("関連cleanupの予約照会失敗: {error:?}");
                                mark_cleanup_reaper_critical(&context);
                                return;
                            }
                        }
                        let dependency = match context.cleanup_dependency_for_handle(owner) {
                            Ok(dependency) => dependency,
                            Err(error) => {
                                // owner確認後に別スレッドがcleanupを完了した場合は正常競合。
                                if matches!(
                                    context.cleanup_retry_disposition(owner),
                                    Ok(AidlObjectCleanupDisposition::Stale
                                        | AidlObjectCleanupDisposition::Closed
                                        | AidlObjectCleanupDisposition::NoCleanupPending)
                                ) || context.quarantine_cleanup_has_owner(owner) == Ok(true)
                                {
                                    continue;
                                }
                                log::debug!("関連cleanupの依存関係が変化しました: owner={owner:?} error={error:?}");
                                continue;
                            }
                        };
                        match pending.attach_to_group(&origin_key, key, dependency) {
                            Ok(true) => {
                                target = CleanupJob {
                                    handle: owner,
                                    registered_at: Instant::now(),
                                };
                                attempt = 0;
                                delegated_wait = None;
                                continue 'job;
                            }
                            Ok(false) => {}
                            Err(error) => {
                                log::error!("関連cleanupの予約移管失敗: {error:?}");
                                mark_cleanup_reaper_critical(&context);
                                return;
                            }
                        }
                    }
                    // 別の既存ownerが処理中または予約済み。重複cleanupを行わず
                    // その結果を期限付きで再照会する。
                    if matches!(
                        context.cleanup_retry_disposition(target.handle),
                        Ok(AidlObjectCleanupDisposition::Stale
                            | AidlObjectCleanupDisposition::Closed
                            | AidlObjectCleanupDisposition::NoCleanupPending)
                    ) {
                        continue;
                    }
                    let waited = delegated_wait.get_or_insert_with(Instant::now).elapsed();
                    if waited >= policy.terminal_deadline {
                        // 判定後のowner変更・正常完了も同一ID/世代で再照会する。
                        let owners = match context.related_cleanup_recovery_owners(target.handle) {
                            Ok(owners) => owners,
                            Err(error) => {
                                log::error!("期限後の関連cleanup owner確認失敗: {error:?}");
                                mark_cleanup_reaper_critical(&context);
                                return;
                            }
                        };
                        let mut other_owner_progressing = false;
                        for (owner, active) in owners {
                            if active {
                                other_owner_progressing = true;
                                break;
                            }
                            match pending.pending_value(&CleanupJobKey::from_handle(owner)) {
                                Ok(Some(_)) => {
                                    other_owner_progressing = true;
                                    break;
                                }
                                Ok(None) => {}
                                Err(error) => {
                                    log::error!("期限後のreaper予約確認失敗: {error:?}");
                                    mark_cleanup_reaper_critical(&context);
                                    return;
                                }
                            }
                        }
                        if !other_owner_progressing {
                            // 有効な回収ownerが消滅した場合は局所隔離を試す。
                            match crate::object_runtime::quarantine_drop_leak_object(
                                &context,
                                target.handle,
                            ) {
                                Ok(crate::object_runtime::DropLeakQuarantineOutcome::Completed)
                                | Ok(crate::object_runtime::DropLeakQuarantineOutcome::SupervisedPending) => {
                                    if target.handle == origin.handle {
                                        return;
                                    }
                                    target = origin;
                                    attempt = 0;
                                    delegated_wait = None;
                                    continue;
                                }
                                Ok(crate::object_runtime::DropLeakQuarantineOutcome::OwnedByExistingCleanup) => {
                                    // owner移管の競合。残りの回復期限内で再検査する。
                                }
                                Err(error) => {
                                    log::error!("関連cleanupの局所隔離不能: handle={:?} error={error:?}", target.handle);
                                    mark_cleanup_reaper_critical(&context);
                                    return;
                                }
                            }
                        }
                        // 元のcleanup期限に続く回復猶予は同じproduct期限1回分だけ。
                        // 無応答ownerのactive権限を取り上げて二重cleanupすることはできない。
                        // 本reaper予約はその間保持し、猶予を過ぎても局所fence不能なら
                        // 既存ServiceFailureStateをfail-stopしてinitによる再起動を要求する。
                        if waited >= policy.terminal_deadline.saturating_mul(2) {
                            // 時間だけでは昇格しない。直前のQuarantineが
                            // OwnedByExistingCleanupを返し、同一世代の未完義務が残る。
                            match context.cleanup_retry_disposition(target.handle) {
                                Ok(
                                    AidlObjectCleanupDisposition::Stale
                                    | AidlObjectCleanupDisposition::Closed
                                    | AidlObjectCleanupDisposition::NoCleanupPending,
                                ) => continue,
                                Ok(AidlObjectCleanupDisposition::Quarantined)
                                    if context.quarantine_cleanup_has_owner(target.handle)
                                        == Ok(true) =>
                                {
                                    return;
                                }
                                Ok(AidlObjectCleanupDisposition::RequiresIsolation)
                                | Ok(AidlObjectCleanupDisposition::Quarantined) => {
                                    log::error!(
                                        "既存ownerの未完cleanupが有界回復期限を超え、世代fence付き局所隔離を確定できません: handle={:?}",
                                        target.handle
                                    );
                                    mark_cleanup_reaper_critical(&context);
                                    return;
                                }
                                Err(error) => {
                                    log::error!("cleanup終端再確認に失敗: {error:?}");
                                    mark_cleanup_reaper_critical(&context);
                                    return;
                                }
                            }
                        }
                    }
                    // 別ownerの進行確認が済んでからテスト用同期点を通知する。
                    #[cfg(test)]
                    if waited >= policy.terminal_deadline {
                        if let Some(checkpoint) = deadline_checkpoint {
                            let _ = checkpoint.send(());
                        }
                    }
                    worker.wait_until(Instant::now().checked_add(Duration::from_millis(20)));
                    continue;
                }
                Err(error) => {
                    if matches!(
                        context.cleanup_retry_disposition(target.handle),
                        Ok(AidlObjectCleanupDisposition::Stale
                            | AidlObjectCleanupDisposition::Closed
                            | AidlObjectCleanupDisposition::NoCleanupPending)
                    ) {
                        continue;
                    }
                    log::error!(
                        "期限到達後のcleanup隔離失敗: handle={:?} error={error:?}",
                        target.handle
                    );
                    mark_cleanup_reaper_critical(&context);
                    return;
                }
            }
        }
        let dependency = match context.cleanup_dependency_for_handle(target.handle) {
            Ok(dependency) => dependency,
            Err(error) => {
                if matches!(
                    context.cleanup_retry_disposition(target.handle),
                    Ok(AidlObjectCleanupDisposition::Stale
                        | AidlObjectCleanupDisposition::Closed
                        | AidlObjectCleanupDisposition::NoCleanupPending)
                ) || context.quarantine_cleanup_has_owner(target.handle) == Ok(true)
                {
                    continue;
                }
                log::error!(
                    "cleanup依存関係を取得できません: handle={:?} error={error:?}",
                    target.handle
                );
                mark_cleanup_reaper_critical(&context);
                return;
            }
        };
        let key = CleanupJobKey::from_handle(target.handle);
        if let Err(error) = pending.update_value(&key, dependency) {
            log::error!("cleanup reaper予約更新失敗: {error:?}");
            mark_cleanup_reaper_critical(&context);
            return;
        }
        let result = close_method(target.handle.object_kind()).and_then(|method| {
            crate::object_runtime::retry_cleanup_from_reaper(&context, target.handle, method)
        });
        if result.is_ok() {
            if target.handle == origin.handle {
                return;
            }
            target = origin;
            attempt = 0;
            delegated_wait = None;
            continue;
        }
        attempt = attempt.saturating_add(1);
        let delay = policy
            .retry_delays
            .get(attempt)
            .copied()
            .unwrap_or(Duration::from_millis(1_000));
        let remaining = policy
            .terminal_deadline
            .saturating_sub(target.registered_at.elapsed());
        worker.wait_until(Instant::now().checked_add(delay.min(remaining)));
    }
}

pub(crate) fn start_cleanup_reaper(
    context: Weak<AidlServiceContext>,
    queue: Arc<CleanupReaperQueue>,
) -> Result<(), HalError> {
    let policy = queue.policy;
    let runner_context = context;
    let runner = Arc::new(move |job, pending, worker| {
        #[cfg(test)]
        run_cleanup_job(runner_context.clone(), policy, job, pending, worker, None);
        #[cfg(not(test))]
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
        use maleicacid_tuner_hal2_domain_request::{
            AidlApi, AidlMethodCall, DvrOpenKind, OpenDvrRequest,
        };
        use maleicacid_tuner_hal2_service_runtime::{
            close_object_use_case, ObjectMethodUseCase, TunerServiceRuntime,
        };
        use std::sync::Mutex;

        let runtime = Arc::new(Mutex::new(TunerServiceRuntime::new()));
        let parent = runtime
            .lock()
            .unwrap()
            .root_open_txn()
            .open_demux_root_object(AidlMethodCall::PublicApi {
                object: AidlObjectKind::Tuner,
                api: AidlApi::TunerOpenDemux,
            })
            .expect("親Demuxを開く");
        let child = ObjectMethodUseCase::execute_after_live(
            &runtime,
            parent.object_id(),
            parent.generation(),
            AidlObjectKind::Demux,
            || -> Result<_, HalError> {
                let request = OpenDvrRequest {
                    kind: DvrOpenKind::Playback,
                    buffer_size: 188,
                };
                Ok((AidlMethodCall::DemuxOpenDvr(request.clone()), request))
            },
            |runtime, dispatch, request| {
                runtime
                    .child_open_txn()
                    .open_dvr_child_runtime_for_demux_object(
                        parent.object_id(),
                        parent.generation(),
                        request,
                        dispatch,
                    )
            },
        )
        .expect("子DVRを正規open");
        let child_id = child.runtime_entry.object_id();
        let generation = child.runtime_entry.generation();
        runtime
            .lock()
            .unwrap()
            .commit_prepared_child_object(child_id, generation)
            .expect("子DVRを公開確定");

        let _unstarted = close_object_use_case(
            &mut runtime.lock().unwrap(),
            parent.object_id(),
            parent.generation(),
            AidlObjectKind::Demux,
            AidlMethodCall::DemuxClose,
        )
        .expect("親close権限だけを発行し、物理cleanupは開始しない");
        let parent_handle = AidlObjectHandle::new(
            AidlObjectKind::Demux,
            parent.object_id(),
            parent.generation(),
        );
        let child_handle = AidlObjectHandle::new(AidlObjectKind::Dvr, child_id, generation);
        let context = AidlServiceContext::from_shared_runtime_for_test(Arc::clone(&runtime));
        assert_eq!(
            crate::object_runtime::quarantine_drop_leak_object(&context, child_handle)
                .expect("親ownerとの競合拒否"),
            crate::object_runtime::DropLeakQuarantineOutcome::OwnedByExistingCleanup
        );
        // 容量1の同一予約groupで子の既存cleanup委譲を開始する。
        // 別のreaper予約を要求すると、子が容量を保持中のため必ず失敗する。
        let policy = CleanupReaperPolicy {
            max_jobs: 1,
            terminal_deadline: Duration::from_millis(20),
            retry_delays: [Duration::ZERO; 4],
        };
        let weak = Arc::downgrade(&context);
        let runner = Arc::new(move |job, pending, worker| {
            run_cleanup_job(weak.clone(), policy, job, pending, worker, None);
        });
        let queue = maleicacid_tuner_hal2_service_runtime::WorkerRuntime::start_reaper_queue(
            1,
            "test-related-cleanup",
            runner,
        )
        .expect("容量1のreaperが起動する");
        let dependency = context
            .cleanup_dependency_for_handle(child_handle)
            .expect("子のcleanup依存関係");
        queue
            .enqueue_reserved(
                CleanupJob {
                    handle: child_handle,
                    registered_at: Instant::now() - Duration::from_secs(1),
                },
                [(CleanupJobKey::from_handle(child_handle), dependency)],
            )
            .expect("子reaperが唯一の予約枠を保持する");

        for _ in 0..200 {
            if context.cleanup_retry_disposition(parent_handle)
                == Ok(maleicacid_tuner_hal2_service_runtime::AidlObjectCleanupDisposition::Closed)
                && context.cleanup_retry_disposition(child_handle)
                    == Ok(
                        maleicacid_tuner_hal2_service_runtime::AidlObjectCleanupDisposition::Closed,
                    )
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("外部APIの再呼出しなしに親の未開始cleanupを自律完了できませんでした");
    }

    #[test]
    fn unrelated_second_cleanup_capacity_full_is_retryable_busy() {
        use maleicacid_tuner_hal2_service_runtime::WorkerRuntime;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::mpsc;

        let mut snapshot =
            maleicacid_tuner_hal2_service_runtime::TunerServiceRuntime::new().capability_snapshot();
        snapshot.cleanup_reaper_capacity = 1;
        snapshot.cleanup_retry_schedule_ms = [0, 10, 20, 40];
        let queue = CleanupReaperQueue::from_snapshot(snapshot);
        let release = Arc::new(AtomicBool::new(false));
        let release_worker = Arc::clone(&release);
        let (started_tx, started_rx) = mpsc::channel();
        let runner = Arc::new(
            move |_: CleanupJob, _: _, _: maleicacid_tuner_hal2_service_runtime::WorkerContext| {
                started_tx.send(()).unwrap();
                while !release_worker.load(Ordering::Acquire) {
                    std::thread::yield_now();
                }
            },
        );
        let owner = WorkerRuntime::start_reaper_queue(1, "test-capacity-reaper", runner).unwrap();
        queue.install(owner).unwrap();
        let first = AidlObjectHandle::new(
            AidlObjectKind::Filter,
            AidlObjectId(100),
            AidlObjectGeneration(1),
        );
        let second = AidlObjectHandle::new(
            AidlObjectKind::Filter,
            AidlObjectId(101),
            AidlObjectGeneration(1),
        );
        queue.enqueue(first, CleanupStep::StopWorker).unwrap();
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let busy = queue.enqueue(second, CleanupStep::StopWorker).unwrap_err();
        assert!(matches!(busy, HalError::Busy { .. }));
        assert!(queue.has_pending(first).unwrap());
        assert!(!queue.has_pending(second).unwrap());
        release.store(true, Ordering::Release);
        for _ in 0..100 {
            if queue.enqueue(second, CleanupStep::StopWorker).is_ok() {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("容量解放後も次のcleanup予約を受け付けられませんでした");
    }

    /// reaper満杯時の未予約active ownerを、外部APIの再呼出しなしで監督する。
    fn admission_monitor_case(complete_owner: bool) {
        use maleicacid_tuner_hal2_domain_request::{AidlApi, AidlMethodCall};
        use maleicacid_tuner_hal2_service_runtime::{
            close_object_use_case, finish_object_close_use_case, TunerServiceRuntime, WorkerRuntime,
        };
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::{mpsc, Mutex};

        let runtime = Arc::new(Mutex::new(TunerServiceRuntime::new()));
        let demux = runtime
            .lock()
            .unwrap()
            .root_open_txn()
            .open_demux_root_object(AidlMethodCall::PublicApi {
                object: AidlObjectKind::Tuner,
                api: AidlApi::TunerOpenDemux,
            })
            .unwrap();
        let handle =
            AidlObjectHandle::new(AidlObjectKind::Demux, demux.object_id(), demux.generation());
        let close = close_object_use_case(
            &mut runtime.lock().unwrap(),
            demux.object_id(),
            demux.generation(),
            AidlObjectKind::Demux,
            AidlMethodCall::DemuxClose,
        )
        .unwrap();
        let attempt = close
            .begin_cleanup_attempt(&mut runtime.lock().unwrap())
            .unwrap();
        let state = runtime.lock().unwrap().failure_state();
        let mut snapshot = runtime.lock().unwrap().capability_snapshot();
        snapshot.cleanup_reaper_capacity = 1;
        snapshot.cleanup_terminal_deadline_ms = 80;
        snapshot.cleanup_retry_schedule_ms = [0, 5, 10, 20];
        let queue = CleanupReaperQueue::from_snapshot(snapshot);
        let release = Arc::new(AtomicBool::new(false));
        let release_worker = Arc::clone(&release);
        let (started_tx, started_rx) = mpsc::channel();
        let runner = Arc::new(
            move |_: CleanupJob, _: _, _: maleicacid_tuner_hal2_service_runtime::WorkerContext| {
                started_tx.send(()).unwrap();
                while !release_worker.load(Ordering::Acquire) {
                    std::thread::yield_now();
                }
            },
        );
        queue
            .install(
                WorkerRuntime::start_reaper_queue(1, "test-admission-monitor", runner).unwrap(),
            )
            .unwrap();
        let unrelated = AidlObjectHandle::new(
            AidlObjectKind::Filter,
            AidlObjectId(99990),
            AidlObjectGeneration(1),
        );
        queue.enqueue(unrelated, CleanupStep::StopWorker).unwrap();
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        // 実際のAIDL enqueue_cleanup_retry経路へ同じ満杯queueを注入する。
        let queue = Arc::new(queue);
        let context = AidlServiceContext::from_shared_runtime_with_cleanup_reaper_for_test(
            Arc::clone(&runtime),
            Some(Arc::clone(&queue)),
        );
        assert!(context.enqueue_cleanup_retry(handle).is_ok());
        queue
            .supervise_unqueued_owner(Arc::downgrade(&context), handle)
            .expect("同一世代の監督workerは重複生成しない");
        assert_eq!(queue.runtime.lock().unwrap().admission_watchers.len(), 1);
        if complete_owner {
            finish_object_close_use_case(&mut runtime.lock().unwrap(), attempt.completion, Ok(()))
                .unwrap();
        }
        let key = CleanupJobKey::from_handle(handle);
        let mut finished = false;
        for _ in 0..500 {
            if queue
                .runtime
                .lock()
                .unwrap()
                .admission_watchers
                .get(&key)
                .is_some_and(|worker| worker.is_finished())
            {
                finished = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        release.store(true, Ordering::Release);
        assert!(finished, "監督workerが有界期限内に終了しませんでした");
        assert_eq!(
            state.snapshot().service_critical,
            !complete_owner,
            "正常完了時は全体停止せず、無応答時はfail-stopする"
        );
        assert!(
            matches!(
                context.cleanup_retry_disposition(handle),
                Ok(maleicacid_tuner_hal2_service_runtime::AidlObjectCleanupDisposition::Closed)
            ) == complete_owner
        );
    }

    #[test]
    fn admission_busy_active_close_finishes_without_global_shutdown() {
        admission_monitor_case(true);
    }

    #[test]
    fn admission_busy_stalled_active_close_is_not_left_unmonitored() {
        admission_monitor_case(false);
    }

    #[test]
    fn active_owner_may_finish_after_delegation_deadline_without_hal_shutdown() {
        use maleicacid_tuner_hal2_domain_request::{AidlApi, AidlMethodCall};
        use maleicacid_tuner_hal2_service_runtime::{
            close_object_use_case, finish_object_close_use_case, TunerServiceRuntime, WorkerRuntime,
        };
        use std::sync::{mpsc, Mutex};

        let runtime = Arc::new(Mutex::new(TunerServiceRuntime::new()));
        let demux = runtime
            .lock()
            .unwrap()
            .root_open_txn()
            .open_demux_root_object(AidlMethodCall::PublicApi {
                object: AidlObjectKind::Tuner,
                api: AidlApi::TunerOpenDemux,
            })
            .unwrap();
        let handle =
            AidlObjectHandle::new(AidlObjectKind::Demux, demux.object_id(), demux.generation());
        let close = close_object_use_case(
            &mut runtime.lock().unwrap(),
            demux.object_id(),
            demux.generation(),
            AidlObjectKind::Demux,
            AidlMethodCall::DemuxClose,
        )
        .unwrap();
        let attempt = close
            .begin_cleanup_attempt(&mut runtime.lock().unwrap())
            .unwrap();
        let failure = runtime.lock().unwrap().failure_state();
        let context = AidlServiceContext::from_shared_runtime_for_test(Arc::clone(&runtime));
        let weak = Arc::downgrade(&context);
        let policy = CleanupReaperPolicy {
            max_jobs: 1,
            terminal_deadline: Duration::from_millis(10),
            retry_delays: [Duration::ZERO; 4],
        };
        let (done_tx, done_rx) = mpsc::channel();
        let (checkpoint_tx, checkpoint_rx) = mpsc::channel();
        let runner = Arc::new(move |job, pending, worker| {
            run_cleanup_job(
                weak.clone(),
                policy,
                job,
                pending,
                worker,
                Some(&checkpoint_tx),
            );
            done_tx.send(()).unwrap();
        });
        let queue =
            WorkerRuntime::start_reaper_queue(1, "test-delegation-deadline", runner).unwrap();
        queue
            .enqueue_reserved(
                CleanupJob {
                    handle,
                    registered_at: Instant::now() - Duration::from_millis(50),
                },
                [(
                    CleanupJobKey::from_handle(handle),
                    CleanupStep::ReleaseBackend,
                )],
            )
            .unwrap();
        // 実際に期限再評価分岐へ入るまで、別ownerを終了させない。
        checkpoint_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("期限超過分岐を通過する");
        assert!(!failure.snapshot().service_critical);
        let release = std::thread::spawn(move || {
            finish_object_close_use_case(&mut runtime.lock().unwrap(), attempt.completion, Ok(()))
                .unwrap();
        });
        done_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("別ownerの正常完了後にreaper終了");
        release.join().unwrap();
        assert!(!failure.snapshot().service_critical);
    }

    #[test]
    fn permanently_stalled_owner_has_bounded_fail_stop_and_reaper_release() {
        use maleicacid_tuner_hal2_domain_request::{AidlApi, AidlMethodCall};
        use maleicacid_tuner_hal2_service_runtime::{
            close_object_use_case, TunerServiceRuntime, WorkerRuntime,
        };
        use std::sync::{mpsc, Mutex};

        let runtime = Arc::new(Mutex::new(TunerServiceRuntime::new()));
        let demux = runtime
            .lock()
            .unwrap()
            .root_open_txn()
            .open_demux_root_object(AidlMethodCall::PublicApi {
                object: AidlObjectKind::Tuner,
                api: AidlApi::TunerOpenDemux,
            })
            .unwrap();
        let handle =
            AidlObjectHandle::new(AidlObjectKind::Demux, demux.object_id(), demux.generation());
        let close = close_object_use_case(
            &mut runtime.lock().unwrap(),
            demux.object_id(),
            demux.generation(),
            AidlObjectKind::Demux,
            AidlMethodCall::DemuxClose,
        )
        .unwrap();
        // 実行権限だけ確保し、物理cleanupの完了は通知しない。
        let _stalled_attempt = close
            .begin_cleanup_attempt(&mut runtime.lock().unwrap())
            .unwrap();
        let failure_state = runtime.lock().unwrap().failure_state();
        let context = AidlServiceContext::from_shared_runtime_for_test(Arc::clone(&runtime));
        let weak = Arc::downgrade(&context);
        let policy = CleanupReaperPolicy {
            max_jobs: 1,
            terminal_deadline: Duration::from_millis(10),
            retry_delays: [Duration::ZERO; 4],
        };
        let (done_tx, done_rx) = mpsc::channel();
        let (checkpoint_tx, checkpoint_rx) = mpsc::channel();
        let runner = Arc::new(move |job, pending, worker| {
            run_cleanup_job(
                weak.clone(),
                policy,
                job,
                pending,
                worker,
                Some(&checkpoint_tx),
            );
            done_tx.send(()).unwrap();
        });
        let queue = WorkerRuntime::start_reaper_queue(1, "test-stalled-owner", runner).unwrap();
        queue
            .enqueue_reserved(
                CleanupJob {
                    handle,
                    registered_at: Instant::now() - Duration::from_millis(50),
                },
                [(
                    CleanupJobKey::from_handle(handle),
                    CleanupStep::ReleaseBackend,
                )],
            )
            .unwrap();
        checkpoint_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("期限到達後にownerを確認してなお待機した");
        assert!(!failure_state.snapshot().service_critical);
        done_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("回復期限後にreaperは終了し、予約グループを解放する");
        assert!(failure_state.snapshot().service_critical);
        for _ in 0..100 {
            if queue
                .pending_value(&CleanupJobKey::from_handle(handle))
                .unwrap()
                .is_none()
            {
                return;
            }
            std::thread::yield_now();
        }
        panic!("停止不能ownerがreaperの保留予約を占有し続けています");
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
            attempt.completion,
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

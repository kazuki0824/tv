use maleicacid_tuner_hal2_common::{PoisonTrackedMutex, RuntimeLockKind};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use maleicacid_tuner_hal2_common::{HalError, HalInternalKind};
use maleicacid_tuner_hal2_domain_request::AidlMethodCall;
use maleicacid_tuner_hal2_resource_ledger::CleanupStep;
use maleicacid_tuner_hal2_service_runtime::{AidlObjectCleanupDisposition, CapabilitySnapshot};

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

struct CleanupReaperQueueOwner {
    queue: Option<
        maleicacid_tuner_hal2_service_runtime::WorkerRuntimeReaperQueue<
            CleanupJobKey,
            CleanupStep,
            CleanupJob,
        >,
    >,
    /// reaperが満杯でも回収が未完のownerを列挙する受付予定。義務正本はObjectCloseTxn。
    admission_pending: std::collections::BTreeMap<CleanupJobKey, CleanupJob>,
    /// 受付の空き待ちは１本の従属WorkerRuntimeで監視し、別の容量枠を作らない。
    admission_worker: Option<maleicacid_tuner_hal2_service_runtime::WorkerRuntime<()>>,
}

impl CleanupReaperQueueOwner {
    fn track_admission(
        &mut self,
        key: CleanupJobKey,
        handle: AidlObjectHandle,
        limit: usize,
    ) -> Result<(), HalError> {
        if self.admission_pending.contains_key(&key) {
            return Ok(());
        }
        if self.admission_pending.len() >= limit {
            return Err(HalError::cleanup_failed(
                "cleanup reaper受付監督",
                "受付待ち上限に達し、未完義務を監督へ引き渡せません",
            ));
        }
        self.admission_pending.insert(
            key,
            CleanupJob {
                handle,
                registered_at: Instant::now(),
            },
        );
        Ok(())
    }
}

pub(crate) struct CleanupReaperQueue {
    policy: CleanupReaperPolicy,
    runtime: PoisonTrackedMutex<CleanupReaperQueueOwner>,
    /// 元サービス状態のWeak参照。Drop中のtyped worker異常を第二正本を作らず通知する。
    service_runtime: std::sync::OnceLock<
        std::sync::Weak<
            std::sync::Mutex<maleicacid_tuner_hal2_service_runtime::TunerServiceRuntime>,
        >,
    >,
    #[cfg(test)]
    idle_park_notice: std::sync::Mutex<Option<std::sync::mpsc::Sender<()>>>,
    #[cfg(test)]
    admission_grace_notice: std::sync::OnceLock<std::sync::mpsc::SyncSender<()>>,
}

impl Drop for CleanupReaperQueue {
    fn drop(&mut self) {
        let worker = match self.runtime.lock() {
            Ok(mut owner) => owner.admission_worker.take(),
            Err(error) => {
                log::error!("cleanup受付監督ownerの破棄中にロック汚染: {error:?}");
                None
            }
        };
        if let Some(worker) = worker {
            worker.request_stop();
            // 無期限joinは禁止。完了済みの場合はtyped resultを必ず検査する。
            if worker.is_finished() {
                match worker.join() {
                    maleicacid_tuner_hal2_service_runtime::WorkerTerminalResult::Normal(())
                    | maleicacid_tuner_hal2_service_runtime::WorkerTerminalResult::StopRequested => {}
                    maleicacid_tuner_hal2_service_runtime::WorkerTerminalResult::RuntimeFailure(error) => {
                        log::error!("cleanup受付監督workerの終了失敗: {error:?}");
                        self.record_terminal_failure("runtime failure during Drop");
                    }
                    maleicacid_tuner_hal2_service_runtime::WorkerTerminalResult::PanicOrJoinFailure => {
                        log::error!("cleanup受付監督workerのjoin/panic異常");
                        self.record_terminal_failure("panic or join failure during Drop");
                    }
                }
            }
        }
    }
}

/// 未受付entryがない場合は新規登録のwake/stopのみ待機する。
/// entryがあれば最も早い物理回収期限と再受付の監視周期を比較する。
fn next_admission_wake(
    pending: &std::collections::BTreeMap<CleanupJobKey, CleanupJob>,
    budget: Duration,
    retry_delay: Duration,
    now: Instant,
) -> Option<Instant> {
    pending
        .values()
        .map(|task| {
            let budget = if now.saturating_duration_since(task.registered_at) >= budget {
                budget.saturating_mul(2)
            } else {
                budget
            };
            task.registered_at.checked_add(budget).unwrap_or(now)
        })
        .min()
        .map(|deadline| now.checked_add(retry_delay).unwrap_or(now).min(deadline))
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
            service_runtime: std::sync::OnceLock::new(),
            #[cfg(test)]
            idle_park_notice: std::sync::Mutex::new(None),
            #[cfg(test)]
            admission_grace_notice: std::sync::OnceLock::new(),
        }
    }

    #[cfg(test)]
    fn notify_admission_grace(&self) {
        if let Some(notice) = self.admission_grace_notice.get() {
            // 試験側の未受信・切断で監督の進行や終端理由を変えない。
            let _ = notice.try_send(());
        }
    }

    pub(crate) fn bind_service_runtime(
        &self,
        runtime: &std::sync::Arc<
            std::sync::Mutex<maleicacid_tuner_hal2_service_runtime::TunerServiceRuntime>,
        >,
    ) {
        // 既存ServiceFailureStateへの唯一のmutation入口を参照するだけ。
        if self
            .service_runtime
            .set(std::sync::Arc::downgrade(runtime))
            .is_err()
        {
            log::error!("cleanup受付監督ownerのservice runtime参照が重複登録されました");
            maleicacid_tuner_hal2_service_runtime::TunerServiceRuntime::mark_shared_service_critical(runtime);
        }
    }

    fn record_terminal_failure(&self, reason: &'static str) {
        if let Some(runtime) = self
            .service_runtime
            .get()
            .and_then(std::sync::Weak::upgrade)
        {
            maleicacid_tuner_hal2_service_runtime::TunerServiceRuntime::mark_shared_service_critical(&runtime);
        } else {
            // service owner消滅後は、公開中のHALを停止する根拠が残っていない。
            log::error!(
                "cleanup受付監督workerの異常を通知するservice runtimeが存在しません: {reason}"
            );
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

    /// 既存reaperへ一度だけ受付を試み、満杯は呼出元の回復判断へ返す。
    pub(crate) fn enqueue(
        &self,
        handle: AidlObjectHandle,
        dependency: CleanupStep,
    ) -> Result<(), HalError> {
        let key = CleanupJobKey::from_handle(handle);
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
        if owner
            .admission_worker
            .as_ref()
            .is_some_and(WorkerRuntime::is_finished)
        {
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
        owner.track_admission(key, handle, self.policy.max_jobs)?;
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
        let retry_delay = self.policy.retry_delays[1];
        let worker = WorkerRuntime::spawn_observed(
            "tuner-hal2-cleanup-admission".to_string(),
            0,
            1,
            move |control| -> Result<(), HalError> {
                use crate::object_runtime::DropLeakQuarantineOutcome;

                let recovery_deadline = budget.saturating_mul(2);
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
                    let tasks: Vec<(CleanupJobKey, CleanupJob)> = {
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
                        let managed = disposition.recovery_settled()
                            || queue
                                .pending_value(&key)
                                .map_err(CleanupReaperQueue::classify_enqueue_error)?
                                .is_some();
                        if managed {
                            queue_owner
                                .runtime
                                .lock()
                                .map_err(HalError::LockPoisoned)?
                                .admission_pending
                                .remove(&key);
                            continue;
                        }
                        match disposition {
                            AidlObjectCleanupDisposition::QuarantineAwaitingReport
                                if task.registered_at.elapsed() < recovery_deadline =>
                            {
                                continue;
                            }
                            AidlObjectCleanupDisposition::QuarantineAwaitingReport
                            | AidlObjectCleanupDisposition::Quarantined => {
                                if context.cleanup_retry_disposition(handle)?.recovery_settled() {
                                    continue;
                                }
                                return Err(HalError::cleanup_failed(
                                    "cleanup reaper受付の監督期限",
                                    "隔離の物理cleanup報告を確定できません",
                                ));
                            }
                            _ => {}
                        }
                        // 既存closeが物理cleanup中ならその実行権限を奪って再実行しない。
                        let active = context
                            .related_cleanup_recovery_owners(handle)?
                            .iter()
                            .any(|(_, active)| *active);
                        if !active {
                            if let Ok(dependency) = context.cleanup_dependency_for_handle(handle) {
                                match queue
                                    .enqueue_reserved(task, [(key, dependency)])
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
                                Ok(DropLeakQuarantineOutcome::Completed
                                    | DropLeakQuarantineOutcome::SupervisedPending)
                            ) || context.cleanup_retry_disposition(handle)?.recovery_settled()
                                || queue
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
                            if matches!(isolated, Ok(DropLeakQuarantineOutcome::OwnedByExistingCleanup))
                                && task.registered_at.elapsed() < recovery_deadline
                            {
                                // 既存closeに元の期限1回分だけ回復猶予を与える。
                                // 予約・実行権限は既存ownerに残し、登録時刻も更新しない。
                                #[cfg(test)]
                                queue_owner.notify_admission_grace();
                                continue;
                            }
                            return Err(HalError::cleanup_failed(
                                "cleanup reaper受付の監督期限",
                                "既存ownerが終端せず、世代fence付きの局所隔離も確定できません",
                            ));
                        }
                    }
                    // 受付待ちが空ならstop/wakeを無期限で待つ。
                    // 新規登録のwakeはWorkerWakeのatomic pending bitで保持されるため、
                    // 空集合確認直後に登録されても起床を取りこぼさない。
                    let next_wait = {
                        let state = queue_owner.runtime.lock().map_err(HalError::LockPoisoned)?;
                        next_admission_wake(
                            &state.admission_pending,
                            budget,
                            retry_delay,
                            Instant::now(),
                        )
                    };
                    #[cfg(test)]
                    let idle_notice = if next_wait.is_none() {
                        queue_owner
                            .idle_park_notice
                            .lock()
                            .ok()
                            .and_then(|mut signal| signal.take())
                    } else {
                        None
                    };
                    // WorkerContextのwakeはatomic pending bitを持つため、
                    // 強参照を解放してから無期限駐機しても通知は取りこぼさない。
                    // context -> queue -> worker の強参照循環を作らない。
                    drop(context);
                    drop(queue_owner);
                    #[cfg(test)]
                    if let Some(signal) = idle_notice {
                        if signal.send(()).is_err() {
                            log::debug!("idle駐機通知のテスト受信側が終了しました");
                        }
                    }
                    control.wait_until(next_wait);
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
    use crate::object_runtime::DropLeakQuarantineOutcome;

    let origin = job;
    let origin_key = CleanupJobKey::from_handle(origin.handle);
    let recovery_deadline = policy.terminal_deadline.saturating_mul(2);
    let mut target = origin;
    let mut attempt = 0usize;
    'job: loop {
        if worker.stop_requested() {
            return;
        }
        let Some(context) = context.upgrade() else {
            return;
        };
        let elapsed = origin.registered_at.elapsed();
        match context.cleanup_retry_disposition(target.handle) {
            Ok(disposition) if disposition.recovery_settled() => {
                if target.handle == origin.handle {
                    return;
                }
                target = origin;
                attempt = 0;
                continue;
            }
            Ok(AidlObjectCleanupDisposition::QuarantineAwaitingReport)
                if elapsed < recovery_deadline =>
            {
                let delay = policy.retry_delays[1].min(recovery_deadline.saturating_sub(elapsed));
                worker.wait_until(Instant::now().checked_add(delay));
                continue;
            }
            Ok(AidlObjectCleanupDisposition::RequiresIsolation) => {}
            disposition => {
                log::error!(
                    "cleanup reaper対象の隔離報告を確定できません: handle={:?} disposition={disposition:?}",
                    target.handle
                );
                mark_cleanup_reaper_critical(&context);
                return;
            }
        }
        // 関連ownerの通常closeは、元の予約から測った残りの回復猶予だけ試す。
        let deadline = if target.handle == origin.handle {
            policy.terminal_deadline
        } else {
            recovery_deadline
        };
        if elapsed >= deadline {
            let isolation = crate::object_runtime::quarantine_drop_leak_object(&context, target.handle);
            if matches!(
                isolation,
                Ok(DropLeakQuarantineOutcome::Completed | DropLeakQuarantineOutcome::SupervisedPending)
            ) || context.cleanup_retry_disposition(target.handle)
                .is_ok_and(AidlObjectCleanupDisposition::recovery_settled)
            {
                if target.handle == origin.handle {
                    return;
                }
                target = origin;
                attempt = 0;
                continue;
            }
            if matches!(isolation, Ok(DropLeakQuarantineOutcome::OwnedByExistingCleanup))
                && origin.registered_at.elapsed() < recovery_deadline
            {
                let owners = match context.related_cleanup_recovery_owners(target.handle) {
                    Ok(owners) => owners,
                    Err(error) => {
                        log::error!("関連cleanup ownerの列挙失敗: {error:?}");
                        mark_cleanup_reaper_critical(&context);
                        return;
                    }
                };
                // 追加の予約枠を取らず、同じgroupへ未開始ownerを引き継ぐ。
                for (owner, active) in owners {
                    if active || owner == target.handle {
                        continue;
                    }
                    let Ok(dependency) = context.cleanup_dependency_for_handle(owner) else {
                        // 並行closeや隔離開始で依存関係が消えることがある。
                        continue;
                    };
                    let key = CleanupJobKey::from_handle(owner);
                    match pending.attach_to_group(&origin_key, key, dependency) {
                        Ok(true) => {
                            target.handle = owner;
                            attempt = 0;
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
                #[cfg(test)]
                if let Some(checkpoint) = deadline_checkpoint {
                    let _ = checkpoint.send(());
                }
                let delay = policy.retry_delays[1]
                    .min(recovery_deadline.saturating_sub(origin.registered_at.elapsed()));
                worker.wait_until(Instant::now().checked_add(delay));
                continue;
            }
            log::error!(
                "cleanup reaper期限までに局所回収を確定できません: handle={:?} result={isolation:?}",
                target.handle
            );
            mark_cleanup_reaper_critical(&context);
            return;
        }
        let dependency = match context.cleanup_dependency_for_handle(target.handle) {
            Ok(dependency) => dependency,
            Err(error) => {
                if context.cleanup_retry_disposition(target.handle).is_ok_and(|state| {
                    state.recovery_settled()
                        || state == AidlObjectCleanupDisposition::QuarantineAwaitingReport
                }) {
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
            continue;
        }
        attempt = attempt.saturating_add(1);
        let delay = policy
            .retry_delays
            .get(attempt)
            .copied()
            .unwrap_or(Duration::from_millis(1_000));
        let remaining = deadline.saturating_sub(origin.registered_at.elapsed());
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
            terminal_deadline: Duration::from_millis(250),
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
                    registered_at: Instant::now() - Duration::from_millis(275),
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

    #[test]
    fn failed_admission_worker_join_during_drop_marks_existing_service_critical() {
        use maleicacid_tuner_hal2_service_runtime::{TunerServiceRuntime, WorkerRuntime};
        use std::sync::Mutex;

        let runtime = Arc::new(Mutex::new(TunerServiceRuntime::new()));
        let failure = runtime.lock().unwrap().failure_state();
        let queue =
            CleanupReaperQueue::from_snapshot(runtime.lock().unwrap().capability_snapshot());
        queue.bind_service_runtime(&runtime);
        let failed = WorkerRuntime::spawn_observed(
            "test-admission-join-failure".to_string(),
            0,
            1,
            |_| -> Result<(), HalError> {
                Err(HalError::cleanup_failed(
                    "admission join",
                    "injected runtime failure",
                ))
            },
            |_| {},
        )
        .unwrap();
        for _ in 0..1000 {
            if failed.is_finished() {
                break;
            }
            std::thread::yield_now();
        }
        assert!(failed.is_finished());
        queue.runtime.lock().unwrap().admission_worker = Some(failed);
        drop(queue);
        assert!(
            failure.snapshot().service_critical,
            "Drop中の異常join結果はSSOTへ伝播する"
        );
    }

    #[test]
    fn idle_admission_worker_does_not_retain_service_or_queue() {
        use maleicacid_tuner_hal2_service_runtime::TunerServiceRuntime;
        use std::sync::{mpsc, Mutex};

        let runtime = Arc::new(Mutex::new(TunerServiceRuntime::new()));
        let context = AidlServiceContext::from_shared_runtime_for_test(runtime);
        let queue = Arc::clone(&context.cleanup_reaper_queue);
        let (signal, reached_idle) = mpsc::channel();
        *queue.idle_park_notice.lock().unwrap() = Some(signal);
        let context_weak = Arc::downgrade(&context);
        let queue_weak = Arc::downgrade(&queue);
        // 未発行IDは直ちにStaleになり、監督リストが空になる。
        let stale = AidlObjectHandle::new(
            AidlObjectKind::Filter,
            AidlObjectId(999_991),
            AidlObjectGeneration(1),
        );
        queue
            .supervise_unqueued_owner(context_weak.clone(), stale)
            .expect("既存の監督workerを起動する");
        reached_idle
            .recv_timeout(Duration::from_secs(2))
            .expect("監督workerが待機対象0件のpark箇所へ到達する");
        drop(context);
        drop(queue);

        for _ in 0..500 {
            if context_weak.upgrade().is_none() && queue_weak.upgrade().is_none() {
                return;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        panic!("idle監督workerがcontext/queueの強参照を保持したままです");
    }

    #[test]
    fn idle_admission_worker_has_no_polling_deadline_and_new_owner_wakes_it() {
        let pending = std::collections::BTreeMap::new();
        let now = Instant::now();
        let budget = Duration::from_millis(20);
        let retry_delay = Duration::from_millis(10);
        assert_eq!(
            next_admission_wake(&pending, budget, retry_delay, now),
            None
        );
        let mut pending = pending;
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Filter,
            AidlObjectId(111),
            AidlObjectGeneration(3),
        );
        pending.insert(
            CleanupJobKey::from_handle(handle),
            CleanupJob {
                handle,
                registered_at: now,
            },
        );
        assert_eq!(
            next_admission_wake(&pending, budget, retry_delay, now + budget),
            Some(now + budget + retry_delay),
        );
        pending.clear();
        assert_eq!(
            next_admission_wake(&pending, budget, retry_delay, now),
            None
        );
        // 新規owner登録後のwake未着はWorkerWakeの原子的保留bitで防止する。
        // 既存WorkerRuntimeのwake/stop回帰はcontrolのworker_wakeテストで確認する。
    }

    #[test]
    fn admission_failure_after_service_owner_drop_does_not_abort() {
        let runtime = Arc::new(std::sync::Mutex::new(
            maleicacid_tuner_hal2_service_runtime::TunerServiceRuntime::new(),
        ));
        let queue =
            CleanupReaperQueue::from_snapshot(runtime.lock().unwrap().capability_snapshot());
        queue.bind_service_runtime(&runtime);
        drop(runtime);
        queue.record_terminal_failure("test join failure");
    }

    #[test]
    fn admission_bound_preserves_existing_generations_and_deadlines() {
        for limit in [1, CapabilitySnapshot::product_default().cleanup_reaper_capacity] {
            let queue = CleanupReaperQueue::from_snapshot(CapabilitySnapshot::product_default());
            let mut owner = queue.runtime.lock().unwrap();
            for generation in 1..=limit {
                let handle = AidlObjectHandle::new(
                    AidlObjectKind::Demux,
                    AidlObjectId(1),
                    AidlObjectGeneration(generation as u64),
                );
                owner.track_admission(CleanupJobKey::from_handle(handle), handle, limit).unwrap();
            }
            let first = AidlObjectHandle::new(
                AidlObjectKind::Demux, AidlObjectId(1), AidlObjectGeneration(1),
            );
            let key = CleanupJobKey::from_handle(first);
            let original = owner.admission_pending[&key].registered_at;
            owner.track_admission(key, first, limit).unwrap();
            assert_eq!(owner.admission_pending[&key].registered_at, original);
            let next = AidlObjectHandle::new(
                AidlObjectKind::Demux,
                AidlObjectId(1),
                AidlObjectGeneration(limit as u64 + 1),
            );
            assert!(owner.track_admission(CleanupJobKey::from_handle(next), next, limit).is_err());
            assert_eq!(owner.admission_pending.len(), limit);
        }
    }

    #[test]
    fn admission_grace_notice_does_not_wait_for_full_or_disconnected_receiver() {
        use std::sync::mpsc::{sync_channel, TryRecvError};

        let queue = CleanupReaperQueue::from_snapshot(CapabilitySnapshot::product_default());
        let (signal, receiver) = sync_channel(1);
        queue.admission_grace_notice.set(signal).unwrap();
        queue.notify_admission_grace();
        queue.notify_admission_grace();
        assert_eq!(receiver.try_recv(), Ok(()));
        assert_eq!(receiver.try_recv(), Err(TryRecvError::Empty));
        drop(receiver);
        queue.notify_admission_grace();
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
        snapshot.cleanup_terminal_deadline_ms = 200;
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
        let (grace_tx, grace_rx) = mpsc::sync_channel(1);
        queue.admission_grace_notice.set(grace_tx).unwrap();
        assert!(context.enqueue_cleanup_retry(handle).is_ok());
        queue
            .supervise_unqueued_owner(Arc::downgrade(&context), handle)
            .expect("同一世代の監督workerは重複生成しない");
        assert_eq!(queue.runtime.lock().unwrap().admission_pending.len(), 1);
        assert!(queue.runtime.lock().unwrap().admission_worker.is_some());
        grace_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("元の監督期限後に既存ownerの回復猶予へ入る");
        assert!(!state.snapshot().service_critical);
        if complete_owner {
            finish_object_close_use_case(
                &mut runtime.lock().unwrap(),
                simulated_cleanup_completion(attempt),
                Ok(()),
            )
            .unwrap();
        }
        let key = CleanupJobKey::from_handle(handle);
        let mut finished = false;
        for _ in 0..500 {
            let (pending, worker_finished) = {
                let owner = queue.runtime.lock().unwrap();
                (
                    owner.admission_pending.contains_key(&key),
                    owner
                        .admission_worker
                        .as_ref()
                        .is_some_and(|worker| worker.is_finished()),
                )
            };
            if (complete_owner && !pending)
                || (!complete_owner && worker_finished && state.snapshot().service_critical)
            {
                finished = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(
            finished,
            "受付監督が正常完了またはfail-stopを有界時間内に確定できませんでした"
        );
        if complete_owner {
            // 待ち0件で監督workerがparkした後、別のcleanupが登録された場合も
            // WorkerWakeのpending bitにより起床を取りこぼさない。
            let second = runtime
                .lock()
                .unwrap()
                .root_open_txn()
                .open_demux_root_object(AidlMethodCall::PublicApi {
                    object: AidlObjectKind::Tuner,
                    api: AidlApi::TunerOpenDemux,
                })
                .unwrap();
            let second_handle = AidlObjectHandle::new(
                AidlObjectKind::Demux,
                second.object_id(),
                second.generation(),
            );
            let close = close_object_use_case(
                &mut runtime.lock().unwrap(),
                second.object_id(),
                second.generation(),
                AidlObjectKind::Demux,
                AidlMethodCall::DemuxClose,
            )
            .unwrap();
            let second_attempt = close
                .begin_cleanup_attempt(&mut runtime.lock().unwrap())
                .unwrap();
            assert!(context.enqueue_cleanup_retry(second_handle).is_ok());
            assert!(queue
                .runtime
                .lock()
                .unwrap()
                .admission_pending
                .contains_key(&CleanupJobKey::from_handle(second_handle)));
            finish_object_close_use_case(
                &mut runtime.lock().unwrap(),
                simulated_cleanup_completion(second_attempt),
                Ok(()),
            )
            .unwrap();
            let mut woken = false;
            for _ in 0..500 {
                if !queue
                    .runtime
                    .lock()
                    .unwrap()
                    .admission_pending
                    .contains_key(&CleanupJobKey::from_handle(second_handle))
                {
                    woken = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            assert!(woken, "idle状態からの新規owner通知を取りこぼしました");
            assert!(!state.snapshot().service_critical);
        }
        release.store(true, Ordering::Release);
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
    fn three_owners_share_one_admission_monitor_without_global_stop() {
        use maleicacid_tuner_hal2_domain_request::{AidlApi, AidlMethodCall};
        use maleicacid_tuner_hal2_service_runtime::{
            close_object_use_case, finish_object_close_use_case, TunerServiceRuntime, WorkerRuntime,
        };
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::{mpsc, Mutex};

        let runtime = Arc::new(Mutex::new(TunerServiceRuntime::new()));
        let mut attempts = Vec::new();
        let mut handles = Vec::new();
        for _ in 0..2 {
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
            handles.push(handle);
            attempts.push(
                close
                    .begin_cleanup_attempt(&mut runtime.lock().unwrap())
                    .unwrap(),
            );
        }
        let failure = runtime.lock().unwrap().failure_state();
        let mut snapshot = runtime.lock().unwrap().capability_snapshot();
        snapshot.cleanup_reaper_capacity = 2;
        snapshot.cleanup_terminal_deadline_ms = 500;
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
                WorkerRuntime::start_reaper_queue(1, "test-three-owner-admission", runner).unwrap(),
            )
            .unwrap();
        let unrelated = AidlObjectHandle::new(
            AidlObjectKind::Filter,
            AidlObjectId(99990),
            AidlObjectGeneration(1),
        );
        queue.enqueue(unrelated, CleanupStep::StopWorker).unwrap();
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let queue = Arc::new(queue);
        let context = AidlServiceContext::from_shared_runtime_with_cleanup_reaper_for_test(
            Arc::clone(&runtime),
            Some(Arc::clone(&queue)),
        );
        for handle in &handles {
            assert!(context.enqueue_cleanup_retry(*handle).is_ok());
        }
        {
            let owner = queue.runtime.lock().unwrap();
            assert_eq!(owner.admission_pending.len(), 2);
            assert!(owner.admission_worker.is_some());
        }
        // Aのreaper予約は保持したまま、B/Cの正規closeだけを完了させる。
        for attempt in attempts {
            finish_object_close_use_case(
                &mut runtime.lock().unwrap(),
                simulated_cleanup_completion(attempt),
                Ok(()),
            )
            .unwrap();
        }
        let mut completed = false;
        for _ in 0..300 {
            if queue.runtime.lock().unwrap().admission_pending.is_empty() {
                completed = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        release.store(true, Ordering::Release);
        assert!(completed, "B/Cが独立した監視容量不足を起こさず終了する");
        assert!(!failure.snapshot().service_critical);
        for handle in handles {
            assert!(matches!(
                context.cleanup_retry_disposition(handle),
                Ok(maleicacid_tuner_hal2_service_runtime::AidlObjectCleanupDisposition::Closed)
            ));
        }
    }

    #[test]
    fn active_owner_may_finish_during_recovery_grace_without_hal_shutdown() {
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
            terminal_deadline: Duration::from_millis(250),
            retry_delays: [Duration::from_millis(5); 4],
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
                    registered_at: Instant::now() - Duration::from_millis(275),
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
            finish_object_close_use_case(
                &mut runtime.lock().unwrap(),
                simulated_cleanup_completion(attempt),
                Ok(()),
            )
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
            terminal_deadline: Duration::from_millis(250),
            retry_delays: [Duration::from_millis(5); 4],
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
                    registered_at: Instant::now() - Duration::from_millis(275),
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

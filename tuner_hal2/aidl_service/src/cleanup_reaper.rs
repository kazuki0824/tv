use maleicacid_tuner_hal2_common::{PoisonTrackedMutex, RuntimeLockKind};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use maleicacid_tuner_hal2_common::{HalError, HalInternalKind};
use maleicacid_tuner_hal2_domain_request::AidlMethodCall;
#[cfg(test)]
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
    execution_started_at: Option<Instant>,
}

struct CleanupReaperQueueOwner {
    queue: Option<
        maleicacid_tuner_hal2_service_runtime::WorkerRuntimeReaperQueue<
            CleanupJobKey,
            CleanupJob,
            CleanupJob,
        >,
    >,
    /// 受付の空き待ちは１本の従属WorkerRuntimeで監視し、別の容量枠を作らない。
    admission_worker: Option<maleicacid_tuner_hal2_service_runtime::WorkerRuntime<()>>,
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

/// 未完義務がなければ新規登録のwake/stopだけを待つ。
fn next_admission_wake(
    pending: &[CleanupJob],
    retry_delay: Duration,
    now: Instant,
) -> Option<Instant> {
    if pending.is_empty() {
        None
    } else {
        now.checked_add(retry_delay)
    }
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
            CleanupJob,
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
    #[cfg(test)]
    fn enqueue(&self, handle: AidlObjectHandle) -> Result<(), HalError> {
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
        let job = CleanupJob {
            handle,
            registered_at: Instant::now(),
            execution_started_at: None,
        };
        runtime
            .enqueue_reserved(job, [(key, job)])
            .map_err(Self::classify_enqueue_error)?;
        self.wake_monitor()
    }

    fn wake_monitor(&self) -> Result<(), HalError> {
        let owner = self.runtime.lock().map_err(HalError::LockPoisoned)?;
        if let Some(worker) = owner.admission_worker.as_ref() {
            worker.wake()?;
        }
        Ok(())
    }

    /// 受付の空き待ちは既存義務に委譲し、呼出元では待たない。
    pub(crate) fn supervise_unqueued_owner(
        self: &Arc<Self>,
        context: Weak<AidlServiceContext>,
        handle: AidlObjectHandle,
    ) -> Result<(), HalError> {
        let Some(service) = context.upgrade() else {
            return Ok(());
        };
        self.start_monitor(context)?;
        // 予約が残る間にlaneから引き渡された要求も、同じ義務へ保持する。
        service.request_cleanup_recovery(handle)?;
        self.wake_monitor()
    }

    fn start_monitor(self: &Arc<Self>, context: Weak<AidlServiceContext>) -> Result<(), HalError> {
        use maleicacid_tuner_hal2_service_runtime::{WorkerRuntime, WorkerTerminalResult};

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
        if owner.admission_worker.is_some() {
            return Ok(());
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
        #[cfg(test)]
        let budget = self.policy.terminal_deadline;
        let retry_delay = self.policy.retry_delays[1];
        let worker = WorkerRuntime::spawn_observed(
            "tuner-hal2-cleanup-admission".to_string(),
            0,
            1,
            move |control| -> Result<(), HalError> {
                #[cfg(test)]
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
                    // 時間超過とは独立に、共有reaperの異常終端・予約正本の破損は検出する。
                    queue
                        .check_health()
                        .map_err(CleanupReaperQueue::classify_enqueue_error)?;
                    let mut handed_off = false;
                    let tasks: Vec<CleanupJob> = context.cleanup_recovery_requests()?
                        .into_iter()
                        .map(|request| CleanupJob {
                            handle: AidlObjectHandle::new(
                                request.object_kind, request.object_id, request.generation,
                            ),
                            registered_at: request.requested_at,
                            execution_started_at: None,
                        })
                        .collect();
                    for &task in &tasks {
                        #[cfg(test)]
                        if task.registered_at.elapsed() >= recovery_deadline {
                            queue_owner.notify_admission_grace();
                        }
                        let handle = task.handle;
                        let key = CleanupJobKey::from_handle(handle);
                        let disposition = context.cleanup_retry_disposition(handle)?;
                        let reservation = queue
                            .pending_value(&key)
                            .map_err(CleanupReaperQueue::classify_enqueue_error)?;
                        if disposition.recovery_settled() {
                            context.finish_cleanup_recovery_admission(handle)?;
                            continue;
                        }
                        // enqueueとlaneからの再監督要求は並行する。予約の存在だけで
                        // 要求を消すと、laneが返した未完義務の再受付を失う。
                        if reservation.is_some() {
                            continue;
                        }
                        match disposition {
                            AidlObjectCleanupDisposition::QuarantineAwaitingReport => {
                                continue;
                            }
                            AidlObjectCleanupDisposition::Quarantined => {
                                if !context.fail_cleanup_if_unsettled(handle)? {
                                    continue;
                                }
                                return Err(HalError::cleanup_failed(
                                    "cleanup reaper受付の状態確認",
                                    "隔離対象の回収義務がありません",
                                ));
                            }
                            _ => {}
                        }
                        // 既存closeが物理cleanup中ならその実行権限を奪って再実行しない。
                        let owners = match context.related_cleanup_recovery_owners(handle) {
                            Ok(owners) => owners,
                            Err(_) if context.cleanup_retry_disposition(handle)?.recovery_settled() => {
                                context.finish_cleanup_recovery_admission(handle)?;
                                continue;
                            }
                            Err(error) => return Err(error),
                        };
                        if !owners.iter().any(|(_, active)| *active) {
                            if context.cleanup_dependency_for_handle(handle).is_ok() {
                                match queue
                                    .enqueue_reserved(task, [(key, task)])
                                    .map_err(CleanupReaperQueue::classify_enqueue_error)
                                {
                                    Ok(()) => {
                                        handed_off = true;
                                        continue;
                                    }
                                    Err(HalError::Busy { .. }) => {
                                        // 未実行の受付待ちは物理回収の失敗ではない。
                                        continue;
                                    }
                                    Err(error) => {
                                        // pending照会後に別の実行主体が予約した競合は成功。
                                        if queue
                                            .pending_value(&key)
                                            .map_err(CleanupReaperQueue::classify_enqueue_error)?
                                            .is_some()
                                        {
                                            handed_off = true;
                                            continue;
                                        }
                                        return Err(error);
                                    }
                                }
                            }
                        }
                    }
                    if handed_off {
                        continue;
                    }
                    // 受付待ちが空ならstop/wakeを無期限で待つ。
                    // 新規登録のwakeはWorkerWakeのatomic pending bitで保持されるため、
                    // 空集合確認直後に登録されても起床を取りこぼさない。
                    let next_wait = next_admission_wake(&tasks, retry_delay, Instant::now());
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
    #[cfg(test)]
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
        CleanupJob,
    >,
    worker: maleicacid_tuner_hal2_service_runtime::WorkerContext,
) {
    use crate::object_runtime::DropLeakQuarantineOutcome;

    let origin = job;
    let started = origin.execution_started_at.unwrap_or(origin.registered_at);
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
        let elapsed = started.elapsed();
        match context.cleanup_retry_disposition(target.handle) {
            Ok(disposition) if disposition.recovery_settled() => {
                if target.handle == origin.handle {
                    return;
                }
                target = origin;
                attempt = 0;
                continue;
            }
            Ok(AidlObjectCleanupDisposition::QuarantineAwaitingReport) => {
                context.defer_cleanup_to_monitor(origin.handle, target.handle);
                return;
            }
            Ok(AidlObjectCleanupDisposition::RequiresIsolation) => {}
            disposition => {
                log::error!(
                    "cleanup reaper対象の隔離報告を確定できません: handle={:?} disposition={disposition:?}",
                    target.handle
                );
                if context.fail_cleanup_if_unsettled(target.handle) == Ok(false) {
                    continue;
                }
                mark_cleanup_reaper_critical(&context);
                return;
            }
        }
        match context.related_cleanup_recovery_owners(target.handle) {
            Ok(owners) if owners.iter().any(|(_, active)| *active) => {
                context.defer_cleanup_to_monitor(origin.handle, target.handle);
                return;
            }
            Ok(_) => {}
            Err(_)
                if context
                    .cleanup_retry_disposition(target.handle)
                    .is_ok_and(AidlObjectCleanupDisposition::recovery_settled) =>
            {
                continue
            }
            Err(error) => {
                log::error!("関連cleanup ownerの照会失敗: {error:?}");
                mark_cleanup_reaper_critical(&context);
                return;
            }
        }
        // 関連ownerの通常closeにも既存の再評価期限を適用する。
        let deadline = if target.handle == origin.handle {
            policy.terminal_deadline
        } else {
            recovery_deadline
        };
        if elapsed >= deadline {
            let isolation =
                crate::object_runtime::quarantine_drop_leak_object(&context, target.handle);
            if matches!(
                isolation,
                Ok(DropLeakQuarantineOutcome::Completed
                    | DropLeakQuarantineOutcome::SupervisedPending)
            ) || context
                .cleanup_retry_disposition(target.handle)
                .is_ok_and(AidlObjectCleanupDisposition::recovery_settled)
            {
                if target.handle == origin.handle {
                    return;
                }
                target = origin;
                attempt = 0;
                continue;
            }
            if matches!(
                isolation,
                Ok(DropLeakQuarantineOutcome::OwnedByExistingCleanup)
            ) {
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
                    if context.cleanup_dependency_for_handle(owner).is_err() {
                        // 並行closeや隔離開始で依存関係が消えることがある。
                        continue;
                    }
                    let key = CleanupJobKey::from_handle(owner);
                    let related = CleanupJob {
                        handle: owner,
                        ..origin
                    };
                    match pending.attach_to_group(&origin_key, key, related) {
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
                context.defer_cleanup_to_monitor(origin.handle, target.handle);
                return;
            }
            if context.fail_cleanup_if_unsettled(target.handle) == Ok(false) {
                continue;
            }
            log::error!(
                "cleanup局所隔離の実行に失敗しました: handle={:?} result={isolation:?}",
                target.handle
            );
            mark_cleanup_reaper_critical(&context);
            return;
        }
        if let Err(error) = context.cleanup_dependency_for_handle(target.handle) {
            if context
                .cleanup_retry_disposition(target.handle)
                .is_ok_and(|state| {
                    state.recovery_settled()
                        || state == AidlObjectCleanupDisposition::QuarantineAwaitingReport
                })
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
        let remaining = deadline.saturating_sub(started.elapsed());
        worker.wait_until(Instant::now().checked_add(delay.min(remaining)));
    }
}

pub(crate) fn start_cleanup_reaper(
    context: Weak<AidlServiceContext>,
    queue: Arc<CleanupReaperQueue>,
) -> Result<(), HalError> {
    let policy = queue.policy;
    let runner_context = context.clone();
    let runner_queue = Arc::downgrade(&queue);
    let runner = Arc::new(
        move |mut job: CleanupJob,
              pending: maleicacid_tuner_hal2_service_runtime::WorkerRuntimeReaperPending<
            CleanupJobKey,
            CleanupJob,
        >,
              worker| {
            job.execution_started_at = Some(Instant::now());
            let start = pending
                .update_value(&CleanupJobKey::from_handle(job.handle), job)
                .and_then(|()| {
                    runner_queue
                        .upgrade()
                        .map_or(Ok(()), |queue| queue.wake_monitor())
                });
            if let Err(error) = start {
                log::error!("cleanup reaper実行開始を記録できません: {error:?}");
                if let Some(context) = runner_context.upgrade() {
                    mark_cleanup_reaper_critical(&context);
                }
                return;
            }
            run_cleanup_job(runner_context.clone(), policy, job, pending, worker);
        },
    );
    let owner = maleicacid_tuner_hal2_service_runtime::WorkerRuntime::start_reaper_queue(
        policy.max_jobs,
        "tuner-hal2-cleanup-reaper",
        runner,
    )?;
    queue.install(owner)?;
    queue.start_monitor(context)
}

#[cfg(test)]
mod tests {

    // 競合の検査対象は実ObjectCloseTxn。物理操作の成功は既存executor境界から注入する。
    fn recovery_requested(context: &AidlServiceContext, handle: AidlObjectHandle) -> bool {
        context
            .cleanup_recovery_requests()
            .unwrap()
            .iter()
            .any(|request| {
                request.object_id == handle.object_id() && request.generation == handle.generation()
            })
    }

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
        parent_cleanup_delegation_case(false);
    }

    #[test]
    fn expired_origin_does_not_grant_a_new_deadline_to_unstarted_parent() {
        parent_cleanup_delegation_case(true);
    }

    fn parent_cleanup_delegation_case(expired: bool) {
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
        let failure = runtime.lock().unwrap().failure_state();
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
            run_cleanup_job(weak.clone(), policy, job, pending, worker);
        });
        let queue = maleicacid_tuner_hal2_service_runtime::WorkerRuntime::start_reaper_queue(
            1,
            "test-related-cleanup",
            runner,
        )
        .expect("容量1のreaperが起動する");
        let job = CleanupJob {
            handle: child_handle,
            registered_at: Instant::now() - Duration::from_millis(if expired { 550 } else { 275 }),
            execution_started_at: None,
        };
        queue
            .enqueue_reserved(job, [(CleanupJobKey::from_handle(child_handle), job)])
            .expect("子reaperが唯一の予約枠を保持する");

        for _ in 0..200 {
            assert!(!failure.snapshot().service_critical);
            if context
                .cleanup_retry_disposition(parent_handle)
                .is_ok_and(AidlObjectCleanupDisposition::recovery_settled)
                && context
                    .cleanup_retry_disposition(child_handle)
                    .is_ok_and(AidlObjectCleanupDisposition::recovery_settled)
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("親cleanupの回収または隔離移管が完了しませんでした");
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
        queue.enqueue(first).unwrap();
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let busy = queue.enqueue(second).unwrap_err();
        assert!(matches!(busy, HalError::Busy { .. }));
        assert!(queue.has_pending(first).unwrap());
        assert!(!queue.has_pending(second).unwrap());
        release.store(true, Ordering::Release);
        for _ in 0..100 {
            if queue.enqueue(second).is_ok() {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("容量解放後も次のcleanup予約を受け付けられませんでした");
    }

    #[test]
    fn expired_admission_busy_retries_after_the_slot_is_released() {
        use maleicacid_tuner_hal2_domain_request::AidlApi;
        use maleicacid_tuner_hal2_service_runtime::{
            close_object_use_case, TunerServiceRuntime, WorkerRuntime,
        };
        use std::sync::{mpsc, Mutex};

        let mut runtime = TunerServiceRuntime::new();
        let entry = runtime
            .root_open_txn()
            .open_demux_root_object(AidlMethodCall::PublicApi {
                object: AidlObjectKind::Tuner,
                api: AidlApi::TunerOpenDemux,
            })
            .unwrap();
        let handle =
            AidlObjectHandle::new(AidlObjectKind::Demux, entry.object_id(), entry.generation());
        let _plan = close_object_use_case(
            &mut runtime,
            handle.object_id(),
            handle.generation(),
            handle.object_kind(),
            AidlMethodCall::DemuxClose,
        )
        .unwrap();
        let failure = runtime.failure_state();
        let mut snapshot = runtime.capability_snapshot();
        snapshot.cleanup_reaper_capacity = 1;
        snapshot.cleanup_terminal_deadline_ms = 30;
        snapshot.cleanup_retry_schedule_ms = [0, 5, 10, 20];
        let queue = Arc::new(CleanupReaperQueue::from_snapshot(snapshot));
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let release_rx = Mutex::new(release_rx);
        let owner = WorkerRuntime::start_reaper_queue(
            1,
            "test-expired-admission",
            Arc::new(move |job: CleanupJob, _, _| {
                started_tx.send(job.handle).unwrap();
                release_rx
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(2))
                    .unwrap();
            }),
        )
        .unwrap();
        let stale = AidlObjectHandle::new(
            AidlObjectKind::Filter,
            AidlObjectId(-1),
            AidlObjectGeneration(1),
        );
        let occupied = owner
            .reserve_pending([(
                CleanupJobKey::from_handle(stale),
                CleanupJob {
                    handle: stale,
                    registered_at: Instant::now(),
                    execution_started_at: None,
                },
            )])
            .unwrap();
        queue.install(owner.clone()).unwrap();
        let context = AidlServiceContext::from_shared_runtime_with_cleanup_reaper_for_test(
            Arc::new(Mutex::new(runtime)),
            Some(Arc::clone(&queue)),
        );
        let (expired_tx, expired_rx) = mpsc::sync_channel(1);
        queue.admission_grace_notice.set(expired_tx).unwrap();

        context.enqueue_cleanup_retry(handle).unwrap();
        expired_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(recovery_requested(&context, handle));
        assert!(!failure.snapshot().service_critical);

        owner.release_reservation(occupied).unwrap();
        assert_eq!(
            started_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            handle
        );
        assert!(!failure.snapshot().service_critical);
        crate::object_runtime::retry_cleanup_from_reaper(
            &context,
            handle,
            AidlMethodCall::DemuxClose,
        )
        .unwrap();
        release_tx.send(()).unwrap();
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
        let queue = Arc::new(CleanupReaperQueue::from_snapshot(
            runtime.lock().unwrap().capability_snapshot(),
        ));
        let context = AidlServiceContext::from_shared_runtime_with_cleanup_reaper_for_test(
            runtime,
            Some(Arc::clone(&queue)),
        );
        start_cleanup_reaper(Arc::downgrade(&context), Arc::clone(&queue)).unwrap();
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
        let pending = Vec::new();
        let now = Instant::now();
        let budget = Duration::from_millis(20);
        let retry_delay = Duration::from_millis(10);
        assert_eq!(next_admission_wake(&pending, retry_delay, now), None);
        let mut pending = pending;
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Filter,
            AidlObjectId(111),
            AidlObjectGeneration(3),
        );
        pending.push(CleanupJob {
            handle,
            registered_at: now,
            execution_started_at: None,
        });
        assert_eq!(
            next_admission_wake(&pending, retry_delay, now + budget),
            Some(now + budget + retry_delay),
        );
        pending.clear();
        assert_eq!(next_admission_wake(&pending, retry_delay, now), None);
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
    fn pending_quarantine_report_does_not_stop_the_service() {
        use maleicacid_tuner_hal2_domain_request::AidlApi;
        use maleicacid_tuner_hal2_service_runtime::{
            quarantine_object_drop_leak_use_case, TunerServiceRuntime, WorkerRuntime,
        };
        use std::sync::{mpsc, Mutex};

        for (reserved, blocked) in [(false, false), (true, false), (true, true)] {
            let mut runtime = TunerServiceRuntime::new();
            let demux = runtime
                .root_open_txn()
                .open_demux_root_object(AidlMethodCall::PublicApi {
                    object: AidlObjectKind::Tuner,
                    api: AidlApi::TunerOpenDemux,
                })
                .unwrap();
            let handle =
                AidlObjectHandle::new(AidlObjectKind::Demux, demux.object_id(), demux.generation());
            let _held_report = quarantine_object_drop_leak_use_case(
                &mut runtime,
                handle.object_id(),
                handle.generation(),
            )
            .unwrap()
            .unwrap();
            let failure = runtime.failure_state();
            let mut snapshot = runtime.capability_snapshot();
            snapshot.cleanup_reaper_capacity = 1;
            snapshot.cleanup_terminal_deadline_ms = 80;
            snapshot.cleanup_retry_schedule_ms = [0, 5, 10, 20];
            let runtime = Arc::new(Mutex::new(runtime));
            let queue = Arc::new(CleanupReaperQueue::from_snapshot(snapshot));
            let context = AidlServiceContext::from_shared_runtime_with_cleanup_reaper_for_test(
                runtime,
                Some(Arc::clone(&queue)),
            );
            let weak = Arc::downgrade(&context);
            let policy = queue.policy;
            let (release, waiter) = mpsc::channel();
            let waiter = Mutex::new(waiter);
            let runner = Arc::new(move |job: CleanupJob, pending, worker| {
                if reserved && !blocked {
                    run_cleanup_job(weak.clone(), policy, job, pending, worker);
                } else if let Ok(waiter) = waiter.lock() {
                    let _ = waiter.recv_timeout(Duration::from_secs(2));
                }
            });
            queue
                .install(WorkerRuntime::start_reaper_queue(1, "隔離報告待ち", runner).unwrap())
                .unwrap();
            let first = if reserved {
                handle
            } else {
                AidlObjectHandle::new(
                    AidlObjectKind::Demux,
                    AidlObjectId(99990),
                    AidlObjectGeneration(1),
                )
            };
            if blocked {
                queue.start_monitor(Arc::downgrade(&context)).unwrap();
            }
            queue.enqueue(first).unwrap();
            if !reserved {
                assert!(queue.has_pending(first).unwrap());
                context.enqueue_cleanup_retry(handle).unwrap();
                assert!(recovery_requested(&context, handle));
            }
            // 旧最終期限を超えても未返却reportを完了扱いせず、HALを維持する。
            std::thread::sleep(Duration::from_millis(250));
            assert!(!failure.snapshot().service_critical);
            drop(release);
            assert_eq!(
                context.cleanup_retry_disposition(handle),
                Ok(AidlObjectCleanupDisposition::QuarantineAwaitingReport)
            );
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
        queue.enqueue(unrelated).unwrap();
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
        assert_eq!(context.cleanup_recovery_requests().unwrap().len(), 1);
        assert!(queue.runtime.lock().unwrap().admission_worker.is_some());
        grace_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("旧最終期限を越えて既存ownerを監督する");
        assert!(!state.snapshot().service_critical);
        if complete_owner {
            finish_object_close_use_case(
                &mut runtime.lock().unwrap(),
                simulated_cleanup_completion(attempt),
                Ok(()),
            )
            .unwrap();
        }
        let mut finished = false;
        for _ in 0..500 {
            let pending = recovery_requested(&context, handle);
            let worker_finished = queue
                .runtime
                .lock()
                .unwrap()
                .admission_worker
                .as_ref()
                .is_some_and(|worker| worker.is_finished());
            assert!(!state.snapshot().service_critical);
            if (complete_owner && !pending) || (!complete_owner && pending && !worker_finished) {
                finished = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(
            finished,
            "受付監督が完了または既存ownerの継続監督を確認できませんでした"
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
            assert!(recovery_requested(&context, second_handle));
            finish_object_close_use_case(
                &mut runtime.lock().unwrap(),
                simulated_cleanup_completion(second_attempt),
                Ok(()),
            )
            .unwrap();
            let mut woken = false;
            for _ in 0..500 {
                if !recovery_requested(&context, second_handle) {
                    woken = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            assert!(woken, "idle状態からの新規owner通知を取りこぼしました");
            assert!(!state.snapshot().service_critical);
        }
        release.store(true, Ordering::Release);
        assert!(!state.snapshot().service_critical);
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
    fn full_reaper_admits_concurrent_recovery_without_blocking_callers() {
        use maleicacid_tuner_hal2_domain_request::{AidlApi, AidlMethodCall};
        use maleicacid_tuner_hal2_service_runtime::{
            close_object_use_case, finish_object_close_use_case, TunerServiceRuntime, WorkerRuntime,
        };
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::{mpsc, Mutex};

        let runtime = Arc::new(Mutex::new(TunerServiceRuntime::new()));
        let mut attempts = Vec::new();
        let mut handles = Vec::new();
        for _ in 0..4 {
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
        snapshot.cleanup_reaper_capacity = 1;
        snapshot.cleanup_terminal_deadline_ms = 1_000;
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
        queue.enqueue(unrelated).unwrap();
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let queue = Arc::new(queue);
        let context = AidlServiceContext::from_shared_runtime_with_cleanup_reaper_for_test(
            Arc::clone(&runtime),
            Some(Arc::clone(&queue)),
        );
        context.enqueue_cleanup_retry(handles[0]).unwrap();
        let (admitted_tx, admitted_rx) = mpsc::channel();
        let callers: Vec<_> = handles[1..]
            .iter()
            .copied()
            .map(|handle| {
                let context = Arc::clone(&context);
                let admitted_tx = admitted_tx.clone();
                std::thread::spawn(move || {
                    let _ = admitted_tx.send(context.enqueue_cleanup_retry(handle));
                })
            })
            .collect();
        // いずれの所有者もまだ完了できず、reaperにも空きはない。
        for _ in &callers {
            admitted_rx
                .recv_timeout(Duration::from_millis(500))
                .unwrap()
                .unwrap();
        }
        for caller in callers {
            caller.join().unwrap();
        }
        assert_eq!(
            context.cleanup_recovery_requests().unwrap().len(),
            handles.len()
        );
        let original = context
            .cleanup_recovery_requests()
            .unwrap()
            .into_iter()
            .find(|request| request.object_id == handles[0].object_id())
            .unwrap()
            .requested_at;
        context.enqueue_cleanup_retry(handles[0]).unwrap();
        assert!(context
            .cleanup_recovery_requests()
            .unwrap()
            .iter()
            .any(|request| request.object_id == handles[0].object_id()
                && request.requested_at == original));
        assert!(!failure.snapshot().service_critical);
        let last_attempt = attempts.pop().unwrap();
        let last_handle = *handles.last().unwrap();
        for attempt in attempts {
            finish_object_close_use_case(
                &mut runtime.lock().unwrap(),
                simulated_cleanup_completion(attempt),
                Ok(()),
            )
            .unwrap();
        }
        assert_eq!(context.cleanup_recovery_requests().unwrap().len(), 1);
        assert!(recovery_requested(&context, last_handle));
        let monitor = queue
            .runtime
            .lock()
            .unwrap()
            .admission_worker
            .take()
            .unwrap();
        monitor.request_stop();
        for _ in 0..250 {
            if monitor.is_finished() {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(monitor.is_finished(), "空き待ち監督はstopで終了する");
        assert!(matches!(
            monitor.join(),
            maleicacid_tuner_hal2_service_runtime::WorkerTerminalResult::Normal(())
                | maleicacid_tuner_hal2_service_runtime::WorkerTerminalResult::StopRequested
        ));
        assert!(recovery_requested(&context, last_handle));
        finish_object_close_use_case(
            &mut runtime.lock().unwrap(),
            simulated_cleanup_completion(last_attempt),
            Ok(()),
        )
        .unwrap();
        release.store(true, Ordering::Release);
        assert!(context.cleanup_recovery_requests().unwrap().is_empty());
        assert!(!failure.snapshot().service_critical);
        for handle in handles {
            assert!(matches!(
                context.cleanup_retry_disposition(handle),
                Ok(maleicacid_tuner_hal2_service_runtime::AidlObjectCleanupDisposition::Closed)
            ));
        }
    }

    #[test]
    fn product_reaper_with_stale_reservations_allows_active_close_to_finish() {
        use maleicacid_tuner_hal2_domain_request::AidlApi;
        use maleicacid_tuner_hal2_service_runtime::{
            close_object_use_case, finish_object_close_use_case, TunerServiceRuntime, WorkerRuntime,
        };
        use std::sync::Mutex;

        let mut runtime = TunerServiceRuntime::new();
        let object = runtime
            .root_open_txn()
            .open_demux_root_object(AidlMethodCall::PublicApi {
                object: AidlObjectKind::Tuner,
                api: AidlApi::TunerOpenDemux,
            })
            .unwrap();
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Demux,
            object.object_id(),
            object.generation(),
        );
        let attempt = close_object_use_case(
            &mut runtime,
            handle.object_id(),
            handle.generation(),
            handle.object_kind(),
            AidlMethodCall::DemuxClose,
        )
        .unwrap()
        .begin_cleanup_attempt(&mut runtime)
        .unwrap();
        let failure = runtime.failure_state();
        let snapshot = runtime.capability_snapshot();
        let capacity = snapshot.cleanup_reaper_capacity;
        let queue = Arc::new(CleanupReaperQueue::from_snapshot(snapshot));
        let reaper = WorkerRuntime::start_reaper_queue(
            capacity,
            "test-stale-reservations",
            Arc::new(|_: CleanupJob, _, _| {}),
        )
        .unwrap();
        let mut old_groups = Vec::new();
        for generation in 1..=capacity {
            let stale = AidlObjectHandle::new(
                AidlObjectKind::Demux,
                AidlObjectId(-1),
                AidlObjectGeneration(generation as u64),
            );
            let job = CleanupJob {
                handle: stale,
                registered_at: Instant::now() - queue.policy.terminal_deadline.saturating_mul(3),
                execution_started_at: None,
            };
            old_groups.push(
                reaper
                    .reserve_pending([(CleanupJobKey::from_handle(stale), job)])
                    .unwrap(),
            );
        }
        queue.install(reaper.clone()).unwrap();
        let runtime = Arc::new(Mutex::new(runtime));
        let context = AidlServiceContext::from_shared_runtime_with_cleanup_reaper_for_test(
            Arc::clone(&runtime),
            Some(Arc::clone(&queue)),
        );
        context.enqueue_cleanup_retry(handle).unwrap();
        assert!(!failure.snapshot().service_critical);
        finish_object_close_use_case(
            &mut runtime.lock().unwrap(),
            simulated_cleanup_completion(attempt),
            Ok(()),
        )
        .unwrap();
        assert_eq!(
            context.cleanup_retry_disposition(handle),
            Ok(AidlObjectCleanupDisposition::Closed)
        );
        for generation in 1..=capacity {
            let stale = AidlObjectHandle::new(
                AidlObjectKind::Demux,
                AidlObjectId(-1),
                AidlObjectGeneration(generation as u64),
            );
            assert!(reaper
                .any_pending([&CleanupJobKey::from_handle(stale)])
                .unwrap());
        }
        assert!(!failure.snapshot().service_critical);
        drop(old_groups);
    }

    fn active_owner_retains_obligation_and_releases_lane(finish_owner: bool) {
        use maleicacid_tuner_hal2_domain_request::AidlApi;
        use maleicacid_tuner_hal2_service_runtime::{
            close_object_use_case, finish_object_close_use_case, TunerServiceRuntime,
        };
        use std::sync::{mpsc, Mutex};

        let mut runtime = TunerServiceRuntime::new();
        let open = AidlMethodCall::PublicApi {
            object: AidlObjectKind::Tuner,
            api: AidlApi::TunerOpenDemux,
        };
        let first = runtime
            .root_open_txn()
            .open_demux_root_object(open.clone())
            .unwrap();
        let second = runtime
            .root_open_txn()
            .open_demux_root_object(open)
            .unwrap();
        let handle =
            AidlObjectHandle::new(AidlObjectKind::Demux, first.object_id(), first.generation());
        let healthy = AidlObjectHandle::new(
            AidlObjectKind::Demux,
            second.object_id(),
            second.generation(),
        );
        let attempt = close_object_use_case(
            &mut runtime,
            handle.object_id(),
            handle.generation(),
            handle.object_kind(),
            AidlMethodCall::DemuxClose,
        )
        .unwrap()
        .begin_cleanup_attempt(&mut runtime)
        .unwrap();
        let failure = runtime.failure_state();
        let mut snapshot = runtime.capability_snapshot();
        snapshot.cleanup_reaper_capacity = 1;
        snapshot.cleanup_terminal_deadline_ms = 30;
        snapshot.cleanup_retry_schedule_ms = [0, 5, 10, 20];
        let queue = Arc::new(CleanupReaperQueue::from_snapshot(snapshot));
        let runtime = Arc::new(Mutex::new(runtime));
        let context = AidlServiceContext::from_shared_runtime_with_cleanup_reaper_for_test(
            Arc::clone(&runtime),
            Some(Arc::clone(&queue)),
        );
        let (signal, passed_old_deadline) = mpsc::sync_channel(1);
        queue.admission_grace_notice.set(signal).unwrap();
        start_cleanup_reaper(Arc::downgrade(&context), Arc::clone(&queue)).unwrap();
        // laneへ直接配送し、実runnerから監督への引渡しを通す。
        queue.enqueue(handle).unwrap();
        passed_old_deadline
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        assert!(!failure.snapshot().service_critical);
        assert!(recovery_requested(&context, handle));
        assert!(!queue.has_pending(handle).unwrap());
        assert_eq!(
            context.cleanup_retry_disposition(handle),
            Ok(AidlObjectCleanupDisposition::RequiresIsolation)
        );
        // 影響を受けないDemuxはLiveで、同じ容量1のlaneから回収できる。
        assert_eq!(
            context.cleanup_retry_disposition(healthy),
            Ok(AidlObjectCleanupDisposition::NoCleanupPending)
        );
        let _plan = close_object_use_case(
            &mut runtime.lock().unwrap(),
            healthy.object_id(),
            healthy.generation(),
            healthy.object_kind(),
            AidlMethodCall::DemuxClose,
        )
        .unwrap();
        queue.enqueue(healthy).unwrap();
        for _ in 0..500 {
            if context
                .cleanup_retry_disposition(healthy)
                .is_ok_and(AidlObjectCleanupDisposition::recovery_settled)
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(context
            .cleanup_retry_disposition(healthy)
            .is_ok_and(AidlObjectCleanupDisposition::recovery_settled));
        if finish_owner {
            finish_object_close_use_case(
                &mut runtime.lock().unwrap(),
                simulated_cleanup_completion(attempt),
                Ok(()),
            )
            .unwrap();
            for _ in 0..500 {
                if !recovery_requested(&context, handle) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            assert!(!recovery_requested(&context, handle));
            assert_eq!(
                context.cleanup_retry_disposition(handle),
                Ok(AidlObjectCleanupDisposition::Closed)
            );
        } else {
            assert!(recovery_requested(&context, handle));
            assert_eq!(
                context.related_cleanup_recovery_owners(handle).unwrap(),
                vec![(handle, true)]
            );
            // 既存ownerが失敗を返した後も、監督要求から自律再受付できる。
            assert!(finish_object_close_use_case(
                &mut runtime.lock().unwrap(),
                simulated_cleanup_completion(attempt),
                Err(
                    maleicacid_tuner_hal2_service_runtime::ObjectCloseCleanupFailure::new(
                        CleanupStep::ReleaseBackend,
                        HalError::cleanup_failed("既存ownerの回収", "遅延後の失敗を注入"),
                    )
                ),
            )
            .is_err());
            for _ in 0..500 {
                if !recovery_requested(&context, handle) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            assert!(!recovery_requested(&context, handle));
            assert_eq!(
                context.cleanup_retry_disposition(handle),
                Ok(AidlObjectCleanupDisposition::Closed)
            );
        }
        assert!(!failure.snapshot().service_critical);
    }

    #[test]
    fn delayed_owner_finishes_after_old_final_deadline_without_hal_shutdown() {
        active_owner_retains_obligation_and_releases_lane(true);
    }

    #[test]
    fn stalled_owner_keeps_fence_without_occupying_another_cleanup_lane() {
        active_owner_retains_obligation_and_releases_lane(false);
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
        let error = queue.enqueue(handle).unwrap_err();
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

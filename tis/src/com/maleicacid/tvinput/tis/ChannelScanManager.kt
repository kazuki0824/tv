package com.maleicacid.tvinput.tis

import android.content.Context
import android.util.Log
import com.maleicacid.tvinput.aribsi.AribSiEngine
import com.maleicacid.tvinput.common.LogTags
import com.maleicacid.tvinput.db.ChannelRecord
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.CopyOnWriteArraySet
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference

// 同じ状態・境界を扱う操作群を一つの所有者に保つ。
@Suppress("TooManyFunctions", "LargeClass")
object ChannelScanManager {
    private const val TUNER_RESOURCE_LOST = "TUNER_RESOURCE_LOST"
    private const val SCAN_GENERATION_EXHAUSTED = "SCAN_GENERATION_EXHAUSTED"
    private const val EXHAUSTED_SCAN_GENERATION = -1

    interface Listener {
        fun onScanStateChanged(state: ScanState)
    }

    data class BackgroundMaintenanceStartDecision(
        val allowed: Boolean,
        val reason: String?,
    )

    data class BootEpgSyncStartDecision(
        val allowed: Boolean,
        val reason: String?,
    )

    data class LiveSessionPreemptDecision(
        val shouldCancel: Boolean,
        val deferBootEpgSync: Boolean,
        val diagnosticReason: String?,
    )

    private val executor: ExecutorService =
        Executors.newSingleThreadExecutor { runnable ->
            Thread(runnable, "maleicacid-channel-scan-manager").apply { isDaemon = true }
        }

    internal class ActiveScanTask(
        val generation: Int,
        val purpose: ScanPurpose,
        val context: Context,
    ) {
        @Volatile var closing = false
        val cancelRequested = AtomicBoolean(false)
        val publicationLock = Any()

        fun requestCancel(): Boolean =
            synchronized(cancelRequested) {
                val scan = controller as? ChannelScanController
                if (scan != null) {
                    scan.requestCancelScan()
                } else {
                    cancelRequested.set(true)
                    true
                }
            }

        @Volatile var controller: AutoCloseable? = null

        @Volatile var engine: AutoCloseable? = null
    }

    // 呼出しごとの受付結果。taskなし・failureなしは既存scanによる拒否であり、状態として保持しない。
    private data class ScanAdmission(
        val task: ActiveScanTask?,
        val failure: ScanState.Failed? = null,
    )

    private val listeners = CopyOnWriteArrayList<Listener>()
    private val activeLiveSessions = CopyOnWriteArraySet<MaleicacidLiveSession>()
    private val activePlaybackPipelines = AtomicInteger(0)
    private val sessionCreationsInProgress = AtomicInteger(0)
    private val nextGeneration = AtomicInteger(0)
    private val activeTask = AtomicReference<ActiveScanTask?>(null)

    @Volatile private var state: ScanState = ScanState.Idle

    fun currentState(): ScanState = state

    fun addListener(listener: Listener) {
        listeners += listener
        listener.onScanStateChanged(state)
    }

    fun removeListener(listener: Listener) {
        listeners -= listener
    }

    fun registerLiveSession(session: MaleicacidLiveSession) {
        activeLiveSessions += session
    }

    fun unregisterLiveSession(
        context: Context,
        session: MaleicacidLiveSession,
    ) {
        if (activeLiveSessions.remove(session) && activeLiveSessions.isEmpty()) {
            drainPendingBootEpgSyncIfIdle(context, "LIVE_SESSION_RELEASED")
        }
    }

    /** 新しいschedulerを設けず、既存のscan/live受付時に未完了の所有だけを再試行する。 */
    private fun retryPendingRelease() {
        executor.execute {
            activeLiveSessions.forEach { it.retryReleaseIfPending() }
            activeTask.get()?.takeIf { it.closing }?.let { finishScanIfCurrent(it.generation) }
        }
    }

    internal fun registerPlaybackPipeline() {
        activePlaybackPipelines.updateAndGet { Math.addExact(it, 1) }
    }

    internal fun unregisterPlaybackPipeline(context: Context?) {
        val remaining =
            activePlaybackPipelines.updateAndGet {
                check(it > 0) { "再生中pipelineの計数が不正です" }
                it - 1
            }
        if (remaining == 0 && context != null) drainPendingBootEpgSyncIfIdle(context, "PLAYBACK_PIPELINE_STOPPED")
    }

    fun beginLiveSessionCreation() {
        retryPendingRelease()
        sessionCreationsInProgress.updateAndGet { Math.addExact(it, 1) }
        preemptBootOrBackgroundScanForLiveSessionCreation()
    }

    fun finishLiveSessionCreation(context: Context) {
        while (true) {
            val current = sessionCreationsInProgress.get()
            if (current <= 0) return
            if (sessionCreationsInProgress.compareAndSet(current, current - 1)) {
                if (current - 1 == 0) drainPendingBootEpgSyncIfIdle(context, "LIVE_SESSION_CREATION_FINISHED")
                return
            }
        }
    }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    // setup terminalのtyped分岐は同一scan ownerで完結させ、状態写像だけを別ownerへ分散しない。
    @Suppress("LongMethod", "MaxLineLength", "ReturnCount", "CyclomaticComplexMethod")
    fun startIfIdle(
        context: Context,
        inputId: String,
        onRejected: (ScanState.Failed?) -> Unit = {},
    ): Int? {
        retryPendingRelease()
        val appContext = context.applicationContext
        val admission = beginScan(ScanPurpose.SETUP_SCAN, appContext)
        val task =
            admission.task ?: run {
                onRejected(admission.failure)
                return null
            }
        val generation = task.generation
        val scanAction =
            Runnable {
                val result =
                    runCatching {
                        val createdEngine = AribSiEngine(appContext)
                        task.engine = createdEngine
                        val createdController =
                            ChannelScanController(
                                appContext,
                                inputId,
                                createdEngine,
                                task.purpose,
                                task.cancelRequested,
                                task.publicationLock,
                            )
                        task.controller = createdController
                        if (!isCurrentGeneration(generation)) return@runCatching null
                        if (isCancelledGeneration(generation)) createdController.cancelScan()
                        createdController.startInitialScan()
                    }
                result
                    .onSuccess { scanResult ->
                        if (scanResult != null) {
                            when {
                                scanResult.terminal.outcome == ChannelScanController.ScanTerminalOutcome.CANCELLED -> {
                                    setTerminalStateIfCurrent(generation, ScanState.Cancelled(generation, ScanPurpose.SETUP_SCAN))
                                }

                                scanResult.terminal.outcome == ChannelScanController.ScanTerminalOutcome.RESOURCE_LOST -> {
                                    setTerminalStateIfCurrent(
                                        generation,
                                        ScanState.Failed(
                                            TUNER_RESOURCE_LOST,
                                            generation,
                                            ScanPurpose.SETUP_SCAN,
                                        ),
                                    )
                                }

                                scanResult.terminal.outcome == ChannelScanController.ScanTerminalOutcome.TUNE_REJECTED ||
                                    scanResult.terminal.outcome == ChannelScanController.ScanTerminalOutcome.INTERNAL_FAILURE -> {
                                    Log.w(
                                        LogTags.TIS,
                                        "setup scan failed generation=$generation outcome=${scanResult.terminal.outcome} " +
                                            "detail=${scanResult.terminal.detail} scanned=${scanResult.scanned} " +
                                            "diagnostics=${scanResult.diagnostics}",
                                    )
                                    setTerminalStateIfCurrent(
                                        generation,
                                        ScanState.Failed(scanResult.terminal.detail, generation, ScanPurpose.SETUP_SCAN),
                                    )
                                }

                                else -> {
                                    setTerminalStateIfCurrent(
                                        generation,
                                        ScanState.Completed(
                                            scanResult,
                                            generation,
                                            ScanPurpose.SETUP_SCAN,
                                        ),
                                    )
                                }
                            }
                        }
                    }.onFailure { e ->
                        Log.w(LogTags.TIS, "チャンネル scan に失敗しました inputId=$inputId", e)
                        if (isCancelledGeneration(generation)) {
                            setTerminalStateIfCurrent(generation, ScanState.Cancelled(generation, ScanPurpose.SETUP_SCAN))
                        } else {
                            setTerminalStateIfCurrent(
                                generation,
                                ScanState.Failed(e.message ?: "不明な例外", generation, ScanPurpose.SETUP_SCAN),
                            )
                        }
                    }
                finishScanIfCurrent(generation)
                drainPendingBootEpgSyncIfIdle(appContext, "SCAN_FINISHED")
            }
        ProgramUpgradeCleanup.ensure(appContext) { success ->
            executor.execute {
                if (success) {
                    scanAction.run()
                } else {
                    setTerminalStateIfCurrent(
                        generation,
                        if (isCancelledGeneration(generation)) {
                            ScanState.Cancelled(generation, ScanPurpose.SETUP_SCAN)
                        } else {
                            ScanState.Failed("Program cleanupに失敗しました", generation, ScanPurpose.SETUP_SCAN)
                        },
                    )
                    finishScanIfCurrent(generation)
                }
            }
        }
        return generation
    }

    // 同じ入力に対する分岐・項目写像を保持し、処理分割による状態の受け渡しを増やさない。
    // 同じ入力と資源寿命を扱う手順を一続きに確認できる形に保つ。
    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    // 入力拒否・未準備・失敗を発生点で返し、成功経路を深い入れ子にしない。
    @Suppress("CyclomaticComplexMethod", "LongMethod", "MaxLineLength", "ReturnCount")
    fun startBootEpgSyncIfIdle(
        context: Context,
        inputId: String,
        targetChannels: List<ChannelRecord>,
        onFinished: ((generation: Int, needsReschedule: Boolean) -> Unit)? = null,
    ): Int? {
        retryPendingRelease()
        val targetSnapshot = targetChannels.toList()
        val requiredServiceKeys = targetSnapshot.map { it.serviceKey }.toSet()
        val appContext = context.applicationContext
        if (!ProgramUpgradeCleanup.ensure(appContext)) {
            markBootEpgSyncDeferred(appContext, "PROGRAM_UPGRADE_CLEANUP_PENDING")
            return null
        }
        val precheck =
            bootEpgSyncStartDecision(
                activeLiveSessions.size,
                isScanRunning(),
                sessionCreationsInProgress.get() > 0,
                activePlaybackPipelines.get() > 0,
            )
        if (!precheck.allowed) {
            markBootEpgSyncDeferred(appContext, precheck.reason ?: "UNKNOWN")
            return null
        }
        val admission = beginScan(ScanPurpose.BOOT_EPG_SYNC, appContext)
        val task = admission.task
        if (task == null) {
            markBootEpgSyncDeferred(
                appContext,
                admission.failure?.message ?: "SCAN_RUNNING",
            )
            return null
        }
        val generation = task.generation
        if (activeLiveSessions.isNotEmpty() || sessionCreationsInProgress.get() > 0 || activePlaybackPipelines.get() > 0) {
            setTerminalStateIfCurrent(generation, ScanState.Idle)
            finishScanIfCurrent(generation)
            markBootEpgSyncDeferred(appContext, "LIVE_SESSION_STARTING_OR_ACTIVE")
            return null
        }
        executor.execute {
            if (activeLiveSessions.isNotEmpty() || sessionCreationsInProgress.get() > 0 || activePlaybackPipelines.get() > 0) {
                setTerminalStateIfCurrent(generation, ScanState.Idle)
                finishScanIfCurrent(generation)
                markBootEpgSyncDeferred(appContext, "LIVE_SESSION_STARTING_OR_ACTIVE")
                onFinished?.invoke(generation, true)
                return@execute
            }
            var shouldScheduleBackgroundMaintenance = false
            var needsReschedule = true
            val result =
                runCatching {
                    val createdEngine = AribSiEngine(appContext)
                    task.engine = createdEngine
                    val createdController =
                        ChannelScanController(
                            appContext,
                            inputId,
                            createdEngine,
                            task.purpose,
                            task.cancelRequested,
                            task.publicationLock,
                        )
                    task.controller = createdController
                    if (!isCurrentGeneration(generation)) return@runCatching null
                    if (isCancelledGeneration(generation)) createdController.cancelScan()
                    createdController.startBootEpgSync(targetSnapshot)
                }
            result
                .onSuccess { scanResult ->
                    if (scanResult != null) {
                        val terminalCancel =
                            scanResult.terminal.outcome == ChannelScanController.ScanTerminalOutcome.CANCELLED
                        val terminalResourceLost =
                            scanResult.terminal.outcome == ChannelScanController.ScanTerminalOutcome.RESOURCE_LOST
                        val allRequiredTargetsCommitted =
                            requiredServiceKeys.isNotEmpty() &&
                                scanResult.committedServiceKeys.containsAll(requiredServiceKeys) &&
                                !terminalCancel &&
                                !terminalResourceLost
                        if (allRequiredTargetsCommitted) {
                            DirectBootGuard.clearPending(appContext)
                            shouldScheduleBackgroundMaintenance = true
                            needsReschedule = false
                        } else {
                            DirectBootGuard.deferPending(
                                appContext,
                                when {
                                    terminalCancel -> "BOOT_EPG_CANCELLED"
                                    terminalResourceLost -> TUNER_RESOURCE_LOST
                                    else -> "BOOT_EPG_INCOMPLETE_TARGET_SET"
                                },
                            )
                        }
                        if (terminalCancel) {
                            setTerminalStateIfCurrent(generation, ScanState.Cancelled(generation, ScanPurpose.BOOT_EPG_SYNC))
                        } else if (terminalResourceLost) {
                            setTerminalStateIfCurrent(
                                generation,
                                ScanState.Failed(TUNER_RESOURCE_LOST, generation, ScanPurpose.BOOT_EPG_SYNC),
                            )
                        } else {
                            setTerminalStateIfCurrent(generation, ScanState.Completed(scanResult, generation, ScanPurpose.BOOT_EPG_SYNC))
                        }
                    }
                }.onFailure { e ->
                    DirectBootGuard.markTunerUnavailable(appContext)
                    Log.w(LogTags.TIS, "boot後EPG同期に失敗しました inputId=$inputId", e)
                    if (isCancelledGeneration(generation)) {
                        setTerminalStateIfCurrent(generation, ScanState.Cancelled(generation, ScanPurpose.BOOT_EPG_SYNC))
                    } else {
                        setTerminalStateIfCurrent(generation, ScanState.Failed(e.message ?: "不明な例外", generation, ScanPurpose.BOOT_EPG_SYNC))
                    }
                }
            if (!finishScanIfCurrent(generation)) {
                needsReschedule = true
                shouldScheduleBackgroundMaintenance = false
                DirectBootGuard.deferPending(appContext, "SCAN_RELEASE_PENDING")
            }
            onFinished?.invoke(generation, needsReschedule)
            if (shouldScheduleBackgroundMaintenance) {
                startBackgroundChannelMaintenanceIfIdle(appContext, inputId, source = "BOOT_EPG_SYNC_COMPLETED")
            }
        }
        return generation
    }

    // 同じ入力に対する分岐・項目写像を保持し、処理分割による状態の受け渡しを増やさない。
    // 同じ入力と資源寿命を扱う手順を一続きに確認できる形に保つ。
    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    // 入力拒否・未準備・失敗を発生点で返し、成功経路を深い入れ子にしない。
    @Suppress("CyclomaticComplexMethod", "LongMethod", "MaxLineLength", "ReturnCount")
    fun startBackgroundChannelMaintenanceIfIdle(
        context: Context,
        inputId: String,
        source: String = "MANUAL",
    ): Boolean {
        retryPendingRelease()
        val precheck =
            backgroundMaintenanceStartDecision(
                activeLiveSessions.size,
                isScanRunning(),
                sessionCreationsInProgress.get() > 0,
                activePlaybackPipelines.get() > 0,
            )
        if (!precheck.allowed) {
            markBackgroundMaintenanceSkipped(precheck.reason ?: "UNKNOWN", source)
            return false
        }
        val appContext = context.applicationContext
        if (!ProgramUpgradeCleanup.ensure(appContext)) {
            markBackgroundMaintenanceSkipped("PROGRAM_UPGRADE_CLEANUP_PENDING", source)
            return false
        }
        val admission = beginScan(ScanPurpose.BACKGROUND_MAINTENANCE, appContext)
        val task = admission.task
        if (task == null) {
            markBackgroundMaintenanceSkipped(
                admission.failure?.message ?: "SCAN_RUNNING",
                source,
            )
            return false
        }
        val generation = task.generation
        if (activeLiveSessions.isNotEmpty() || sessionCreationsInProgress.get() > 0 || activePlaybackPipelines.get() > 0) {
            setTerminalStateIfCurrent(generation, ScanState.Idle)
            finishScanIfCurrent(generation)
            markBackgroundMaintenanceSkipped("LIVE_SESSION_STARTING_OR_ACTIVE", source)
            return false
        }
        executor.execute {
            if (activeLiveSessions.isNotEmpty() || sessionCreationsInProgress.get() > 0 || activePlaybackPipelines.get() > 0) {
                setTerminalStateIfCurrent(generation, ScanState.Idle)
                finishScanIfCurrent(generation)
                markBackgroundMaintenanceSkipped("LIVE_SESSION_STARTING_OR_ACTIVE", source)
                return@execute
            }
            val result =
                runCatching {
                    val createdEngine = AribSiEngine(appContext)
                    task.engine = createdEngine
                    val createdController =
                        ChannelScanController(
                            appContext,
                            inputId,
                            createdEngine,
                            task.purpose,
                            task.cancelRequested,
                            task.publicationLock,
                        )
                    task.controller = createdController
                    if (!isCurrentGeneration(generation)) return@runCatching null
                    if (isCancelledGeneration(generation)) createdController.cancelScan()
                    createdController.startBackgroundChannelMaintenance()
                }
            result
                .onSuccess { scanResult ->
                    if (scanResult != null) {
                        if (
                            scanResult.terminal.outcome == ChannelScanController.ScanTerminalOutcome.CANCELLED
                        ) {
                            setTerminalStateIfCurrent(generation, ScanState.Cancelled(generation, ScanPurpose.BACKGROUND_MAINTENANCE))
                        } else if (scanResult.terminal.outcome == ChannelScanController.ScanTerminalOutcome.RESOURCE_LOST) {
                            setTerminalStateIfCurrent(
                                generation,
                                ScanState.Failed(TUNER_RESOURCE_LOST, generation, ScanPurpose.BACKGROUND_MAINTENANCE),
                            )
                        } else {
                            setTerminalStateIfCurrent(
                                generation,
                                ScanState.Completed(scanResult, generation, ScanPurpose.BACKGROUND_MAINTENANCE),
                            )
                        }
                    }
                }.onFailure { e ->
                    Log.w(LogTags.TIS, "background channel maintenance に失敗しました inputId=$inputId source=$source", e)
                    if (isCancelledGeneration(generation)) {
                        setTerminalStateIfCurrent(generation, ScanState.Cancelled(generation, ScanPurpose.BACKGROUND_MAINTENANCE))
                    } else {
                        setTerminalStateIfCurrent(
                            generation,
                            ScanState.Failed(e.message ?: "不明な例外", generation, ScanPurpose.BACKGROUND_MAINTENANCE),
                        )
                    }
                }
            finishScanIfCurrent(generation)
            drainPendingBootEpgSyncIfIdle(appContext, "BACKGROUND_MAINTENANCE_FINISHED")
        }
        return true
    }

    fun cancel() {
        val task = activeTask.get() ?: return
        if (!task.requestCancel()) return
        setTerminalStateIfCurrent(
            task.generation,
            ScanState.Cancelled(task.generation, task.purpose),
        )
    }

    // 入力拒否・未準備・失敗を発生点で返し、成功経路を深い入れ子にしない。
    @Suppress("ReturnCount")
    fun cancelIfCurrent(
        generation: Int,
        purpose: ScanPurpose,
    ): Boolean {
        val task = activeTask.get() ?: return false
        if (task.generation != generation || task.purpose != purpose) return false
        if (!task.requestCancel()) return false
        setTerminalStateIfCurrent(generation, ScanState.Cancelled(generation, purpose))
        return true
    }

    // 入力拒否・未準備・失敗を発生点で返し、成功経路を深い入れ子にしない。
    @Suppress("ReturnCount")
    private fun preemptBootOrBackgroundScanForLiveSessionCreation() {
        val task = activeTask.get()
        val decision = liveSessionPreemptDecision(task != null, task?.purpose)
        if (!decision.shouldCancel) return
        task ?: return
        if (task.cancelRequested.get()) return
        if (decision.deferBootEpgSync) {
            markBootEpgSyncDeferred(
                task.context,
                decision.diagnosticReason ?: "LIVE_SESSION_PREEMPTED_RUNNING_BOOT_EPG_SYNC",
            )
        }
        if (!task.requestCancel()) return
        setTerminalStateIfCurrent(
            task.generation,
            ScanState.Cancelled(task.generation, task.purpose),
        )
    }

    internal fun liveSessionPreemptDecision(
        scanRunning: Boolean,
        purpose: ScanPurpose?,
    ): LiveSessionPreemptDecision {
        if (!scanRunning || purpose == null) {
            return LiveSessionPreemptDecision(
                shouldCancel = false,
                deferBootEpgSync = false,
                diagnosticReason = null,
            )
        }
        return when (purpose) {
            ScanPurpose.BOOT_EPG_SYNC -> {
                LiveSessionPreemptDecision(
                    shouldCancel = true,
                    deferBootEpgSync = true,
                    diagnosticReason = "LIVE_SESSION_PREEMPTED_RUNNING_BOOT_EPG_SYNC",
                )
            }

            ScanPurpose.BACKGROUND_MAINTENANCE -> {
                LiveSessionPreemptDecision(
                    shouldCancel = true,
                    deferBootEpgSync = false,
                    diagnosticReason = "LIVE_SESSION_PREEMPTED_RUNNING_BACKGROUND_MAINTENANCE",
                )
            }

            ScanPurpose.SETUP_SCAN -> {
                LiveSessionPreemptDecision(shouldCancel = false, deferBootEpgSync = false, diagnosticReason = null)
            }
        }
    }

    private fun allocateScanGeneration(): Int? {
        while (true) {
            val current = nextGeneration.get()
            val next =
                runCatching { RuntimeIdentity.nextInt(current, "channel scan世代") }
                    .getOrNull()
                    ?: return null
            if (nextGeneration.compareAndSet(current, next)) return next
        }
    }

    @Synchronized
    @Suppress("ReturnCount")
    private fun beginScan(
        purpose: ScanPurpose,
        context: Context,
    ): ScanAdmission {
        if (activeTask.get() != null) return ScanAdmission(null)
        val generation = allocateScanGeneration()
        if (generation == null) {
            val failure = ScanState.Failed(SCAN_GENERATION_EXHAUSTED, EXHAUSTED_SCAN_GENERATION, purpose)
            setState(failure)
            return ScanAdmission(null, failure)
        }
        val task = ActiveScanTask(generation, purpose, context.applicationContext)
        if (!activeTask.compareAndSet(null, task)) return ScanAdmission(null)
        setState(ScanState.Running(System.currentTimeMillis(), generation, purpose))
        return ScanAdmission(task)
    }

    private fun isScanRunning(): Boolean = activeTask.get() != null

    private fun isCurrentGeneration(generation: Int): Boolean = activeTask.get()?.generation == generation

    private fun isCancelledGeneration(generation: Int): Boolean =
        activeTask.get()?.let { it.generation == generation && it.cancelRequested.get() } == true

    private fun setTerminalStateIfCurrent(
        generation: Int,
        terminalState: ScanState,
    ) {
        if (isCurrentGeneration(generation)) {
            setState(terminalState)
        }
    }

    private fun finishScanIfCurrent(generation: Int): Boolean {
        val task = activeTask.get()?.takeIf { it.generation == generation } ?: return true
        return finishScanRelease(task, activeTask) { error ->
            Log.w(
                LogTags.TIS,
                if (error is TunerOneShotCloseFailure) {
                    "scan Tuner解放のone-shot失敗 generation=$generation（同じSDKへの再試行は不可）"
                } else {
                    "scan解放の所有を再試行まで保持します generation=$generation"
                },
                error,
            )
        }
    }

    // 境界呼出しの失敗を漏らさず扱い、既存の診断・解放・失敗伝播へ渡す。
    @Suppress("TooGenericExceptionCaught")
    internal fun finishScanRelease(
        task: ActiveScanTask,
        owner: AtomicReference<ActiveScanTask?>,
        reportFailure: (Exception) -> Unit,
    ): Boolean {
        task.closing = true
        return try {
            closeController(task)
            owner.compareAndSet(task, null)
            true
        } catch (error: Exception) {
            // semantic終端理由は維持する。未解放資源はtask.closingと所有参照で表す。
            reportFailure(error)
            false
        }
    }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    internal fun bootEpgSyncStartDecision(
        activeLiveSessionCount: Int,
        scanRunning: Boolean,
        sessionCreationInProgress: Boolean = false,
        playbackPipelineRunning: Boolean = false,
    ): BootEpgSyncStartDecision =
        when {
            activeLiveSessionCount > 0 || sessionCreationInProgress -> BootEpgSyncStartDecision(false, "LIVE_SESSION_STARTING_OR_ACTIVE")
            playbackPipelineRunning -> BootEpgSyncStartDecision(false, "PLAYBACK_PIPELINE_RUNNING")
            scanRunning -> BootEpgSyncStartDecision(false, "SCAN_RUNNING")
            else -> BootEpgSyncStartDecision(true, null)
        }

    private fun markBootEpgSyncDeferred(
        context: Context,
        reason: String,
    ) {
        DirectBootGuard.deferPending(context.applicationContext, reason)
        Log.i(
            LogTags.TIS,
            "boot EPG 同期を開始しません reason=$reason activeLiveSessions=" +
                "${activeLiveSessions.size} sessionCreationsInProgress=" +
                "${sessionCreationsInProgress.get()} scanRunning=${isScanRunning()}",
        )
    }

    private fun drainPendingBootEpgSyncIfIdle(
        context: Context,
        source: String,
    ) {
        val backgroundWorkBlocked =
            activeLiveSessions.isNotEmpty() ||
                sessionCreationsInProgress.get() > 0 ||
                activePlaybackPipelines.get() > 0 ||
                isScanRunning()
        if (backgroundWorkBlocked) {
            return
        }
        Log.i(LogTags.TIS, "pending boot EPG 同期ジョブを再登録します source=$source")
        BootEpgSyncScheduler.scheduleIfEligible(context.applicationContext, source)
    }

    private fun markBackgroundMaintenanceSkipped(
        reason: String,
        source: String,
    ) {
        Log.i(
            LogTags.TIS,
            "background channel maintenance を開始しません source=$source reason=$reason " +
                "activeLiveSessions=${activeLiveSessions.size} sessionCreationsInProgress=" +
                "${sessionCreationsInProgress.get()} scanRunning=${isScanRunning()}",
        )
    }

    private fun closeController(task: ActiveScanTask) {
        SectionFilterPolicy.completeCleanup(
            {
                task.controller?.close()
                task.controller = null
            },
            {
                task.engine?.close()
                task.engine = null
            },
        )
    }

    private fun setState(newState: ScanState) {
        state = newState
        listeners.forEach { listener -> runCatching { listener.onScanStateChanged(newState) } }
    }

    internal fun backgroundMaintenanceStartDecision(
        activeLiveSessionCount: Int,
        scanRunning: Boolean,
        sessionCreationInProgress: Boolean = false,
        playbackPipelineRunning: Boolean = false,
    ): BackgroundMaintenanceStartDecision =
        when {
            activeLiveSessionCount > 0 || sessionCreationInProgress -> {
                BackgroundMaintenanceStartDecision(
                    false,
                    "LIVE_SESSION_STARTING_OR_ACTIVE",
                )
            }

            playbackPipelineRunning -> {
                BackgroundMaintenanceStartDecision(false, "PLAYBACK_PIPELINE_RUNNING")
            }

            scanRunning -> {
                BackgroundMaintenanceStartDecision(false, "SCAN_RUNNING")
            }

            else -> {
                BackgroundMaintenanceStartDecision(true, null)
            }
        }
}

package com.maleicacid.tvinput.tis

import android.content.Context
import android.media.tv.TvInputService
import android.util.Log
import com.maleicacid.tvinput.aribsi.AribSiEngine
import com.maleicacid.tvinput.common.LogTags
import com.maleicacid.tvinput.db.ChannelRecord
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference

object ChannelScanManager {
    interface Listener { fun onScanStateChanged(state: ScanState) }

    data class BackgroundMaintenanceStartDecision(val allowed: Boolean, val reason: String?)
    data class BootEpgSyncStartDecision(val allowed: Boolean, val reason: String?)
    data class LiveSessionPreemptDecision(val shouldCancel: Boolean, val deferBootEpgSync: Boolean, val diagnosticReason: String?)

    private val executor: ExecutorService = Executors.newSingleThreadExecutor { runnable ->
        Thread(runnable, "maleicacid-channel-scan-manager").apply { isDaemon = true }
    }
    private class ActiveScanTask(
        val generation: Int,
        val purpose: ScanPurpose,
        val context: Context,
    ) {
        val cancelRequested = AtomicBoolean(false)
        @Volatile var controller: ChannelScanController? = null
        @Volatile var engine: AribSiEngine? = null
    }

    private val listeners = CopyOnWriteArrayList<Listener>()
    private val activeLiveSessions = AtomicInteger(0)
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

    fun registerLiveSession() {
        activeLiveSessions.incrementAndGet()
    }

    fun unregisterLiveSession(context: Context) {
        while (true) {
            val current = activeLiveSessions.get()
            if (current <= 0) return
            if (activeLiveSessions.compareAndSet(current, current - 1)) {
                if (current - 1 == 0) drainPendingBootEpgSyncIfIdle(context, "LIVE_SESSION_RELEASED")
                return
            }
        }
    }

    fun activeLiveSessionCountForTest(): Int = activeLiveSessions.get()
    fun sessionCreationInProgressCountForTest(): Int = sessionCreationsInProgress.get()

    fun beginLiveSessionCreation() {
        sessionCreationsInProgress.incrementAndGet()
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

    fun bootEpgSyncStartDecisionForTest(activeLiveSessionCount: Int, scanRunning: Boolean, sessionCreationInProgress: Boolean = false): BootEpgSyncStartDecision =
        bootEpgSyncStartDecision(activeLiveSessionCount, scanRunning, sessionCreationInProgress)

    fun liveSessionPreemptDecisionForTest(scanRunning: Boolean, purpose: ScanPurpose?): LiveSessionPreemptDecision =
        liveSessionPreemptDecision(scanRunning, purpose)

    fun startIfIdle(context: Context, inputId: String): Int? {
        val appContext = context.applicationContext
        val task = beginScan(ScanPurpose.SETUP_SCAN, appContext) ?: return null
        val generation = task.generation
        executor.execute {
            val result = runCatching {
                val createdEngine = AribSiEngine(appContext)
                val createdController = ChannelScanController(
                    appContext,
                    inputId,
                    createdEngine,
                    TvInputService.PRIORITY_HINT_USE_CASE_TYPE_SCAN,
                    task.cancelRequested,
                )
                if (!isCurrentGeneration(generation)) {
                    createdController.close()
                    createdEngine.close()
                    return@runCatching null
                }
                task.engine = createdEngine
                task.controller = createdController
                if (isCancelledGeneration(generation)) createdController.cancelScan()
                createdController.startInitialScan()
            }
            result.onSuccess { scanResult ->
                if (scanResult != null) {
                    if (scanResult.terminalCancelObserved || isCancelledGeneration(generation)) {
                        setTerminalStateIfCurrent(generation, ScanState.Cancelled(generation, ScanPurpose.SETUP_SCAN))
                    } else {
                        setTerminalStateIfCurrent(generation, ScanState.Completed(scanResult, generation, ScanPurpose.SETUP_SCAN))
                    }
                }
            }.onFailure { e ->
                Log.w(LogTags.TIS, "チャンネル scan に失敗しました inputId=$inputId", e)
                if (isCancelledGeneration(generation)) {
                    setTerminalStateIfCurrent(generation, ScanState.Cancelled(generation, ScanPurpose.SETUP_SCAN))
                } else {
                    setTerminalStateIfCurrent(generation, ScanState.Failed(e.message ?: "不明な例外", generation, ScanPurpose.SETUP_SCAN))
                }
            }
            finishScanIfCurrent(generation)
            drainPendingBootEpgSyncIfIdle(appContext, "SCAN_FINISHED")
        }
        return generation
    }

    fun startBootEpgSyncIfIdle(
        context: Context,
        inputId: String,
        targetChannels: List<ChannelRecord>,
        onFinished: ((generation: Int, needsReschedule: Boolean) -> Unit)? = null,
    ): Int? {
        val targetSnapshot = targetChannels.toList()
        val appContext = context.applicationContext
        val precheck = bootEpgSyncStartDecision(activeLiveSessions.get(), isScanRunning(), sessionCreationsInProgress.get() > 0)
        if (!precheck.allowed) {
            markBootEpgSyncDeferred(appContext, precheck.reason ?: "UNKNOWN")
            return null
        }
        val task = beginScan(ScanPurpose.BOOT_EPG_SYNC, appContext)
        if (task == null) {
            markBootEpgSyncDeferred(appContext, "SCAN_RUNNING")
            return null
        }
        val generation = task.generation
        if (activeLiveSessions.get() > 0 || sessionCreationsInProgress.get() > 0) {
            setTerminalStateIfCurrent(generation, ScanState.Idle)
            finishScanIfCurrent(generation)
            markBootEpgSyncDeferred(appContext, "LIVE_SESSION_STARTING_OR_ACTIVE")
            return null
        }
        executor.execute {
            if (activeLiveSessions.get() > 0 || sessionCreationsInProgress.get() > 0) {
                setTerminalStateIfCurrent(generation, ScanState.Idle)
                finishScanIfCurrent(generation)
                markBootEpgSyncDeferred(appContext, "LIVE_SESSION_STARTING_OR_ACTIVE")
                onFinished?.invoke(generation, true)
                return@execute
            }
            var shouldScheduleBackgroundMaintenance = false
            var needsReschedule = true
            val result = runCatching {
                val createdEngine = AribSiEngine(appContext)
                val createdController = ChannelScanController(
                    appContext,
                    inputId,
                    createdEngine,
                    TvInputService.PRIORITY_HINT_USE_CASE_TYPE_BACKGROUND,
                    task.cancelRequested,
                )
                if (!isCurrentGeneration(generation)) {
                    createdController.close()
                    createdEngine.close()
                    return@runCatching null
                }
                task.engine = createdEngine
                task.controller = createdController
                if (isCancelledGeneration(generation)) createdController.cancelScan()
                createdController.startBootEpgSync(targetSnapshot)
            }
            result.onSuccess { scanResult ->
                if (scanResult != null) {
                    val terminalCancel = scanResult.terminalCancelObserved || isCancelledGeneration(generation)
                    val successCandidateObserved = scanResult.successfulCandidates > 0 && !terminalCancel
                    if (successCandidateObserved) {
                        DirectBootGuard.clearPending(appContext)
                        shouldScheduleBackgroundMaintenance = true
                        needsReschedule = false
                    } else {
                        DirectBootGuard.deferPending(appContext, if (terminalCancel) "BOOT_EPG_CANCELLED" else "BOOT_EPG_NO_SUCCESSFUL_CANDIDATE")
                    }
                    if (terminalCancel) {
                        setTerminalStateIfCurrent(generation, ScanState.Cancelled(generation, ScanPurpose.BOOT_EPG_SYNC))
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
            finishScanIfCurrent(generation)
            onFinished?.invoke(generation, needsReschedule)
            if (shouldScheduleBackgroundMaintenance) {
                BackgroundChannelMaintenanceDiagnostics.scheduledAfterBootSyncCount.incrementAndGet()
                startBackgroundChannelMaintenanceIfIdle(appContext, inputId, source = "BOOT_EPG_SYNC_COMPLETED")
            }
        }
        return generation
    }

    fun startBackgroundChannelMaintenanceIfIdle(
        context: Context,
        inputId: String,
        source: String = "MANUAL",
    ): Boolean {
        val precheck = backgroundMaintenanceStartDecision(activeLiveSessions.get(), isScanRunning(), sessionCreationsInProgress.get() > 0)
        if (!precheck.allowed) {
            markBackgroundMaintenanceSkipped(precheck.reason ?: "UNKNOWN", source)
            return false
        }
        val appContext = context.applicationContext
        val task = beginScan(ScanPurpose.BACKGROUND_MAINTENANCE, appContext)
        if (task == null) {
            markBackgroundMaintenanceSkipped("SCAN_RUNNING", source)
            return false
        }
        val generation = task.generation
        if (activeLiveSessions.get() > 0 || sessionCreationsInProgress.get() > 0) {
            setTerminalStateIfCurrent(generation, ScanState.Idle)
            finishScanIfCurrent(generation)
            markBackgroundMaintenanceSkipped("LIVE_SESSION_STARTING_OR_ACTIVE", source)
            return false
        }
        executor.execute {
            if (activeLiveSessions.get() > 0 || sessionCreationsInProgress.get() > 0) {
                setTerminalStateIfCurrent(generation, ScanState.Idle)
                finishScanIfCurrent(generation)
                markBackgroundMaintenanceSkipped("LIVE_SESSION_STARTING_OR_ACTIVE", source)
                return@execute
            }
            BackgroundChannelMaintenanceDiagnostics.startedCount.incrementAndGet()
            val result = runCatching {
                val createdEngine = AribSiEngine(appContext)
                val createdController = ChannelScanController(
                    appContext,
                    inputId,
                    createdEngine,
                    TvInputService.PRIORITY_HINT_USE_CASE_TYPE_BACKGROUND,
                    task.cancelRequested,
                )
                if (!isCurrentGeneration(generation)) {
                    createdController.close()
                    createdEngine.close()
                    return@runCatching null
                }
                task.engine = createdEngine
                task.controller = createdController
                if (isCancelledGeneration(generation)) createdController.cancelScan()
                createdController.startBackgroundChannelMaintenance()
            }
            result.onSuccess { scanResult ->
                if (scanResult != null) {
                    if (scanResult.terminalCancelObserved || isCancelledGeneration(generation)) {
                        setTerminalStateIfCurrent(generation, ScanState.Cancelled(generation, ScanPurpose.BACKGROUND_MAINTENANCE))
                    } else {
                        setTerminalStateIfCurrent(generation, ScanState.Completed(scanResult, generation, ScanPurpose.BACKGROUND_MAINTENANCE))
                    }
                }
            }.onFailure { e ->
                Log.w(LogTags.TIS, "background channel maintenance に失敗しました inputId=$inputId source=$source", e)
                if (isCancelledGeneration(generation)) {
                    setTerminalStateIfCurrent(generation, ScanState.Cancelled(generation, ScanPurpose.BACKGROUND_MAINTENANCE))
                } else {
                    setTerminalStateIfCurrent(generation, ScanState.Failed(e.message ?: "不明な例外", generation, ScanPurpose.BACKGROUND_MAINTENANCE))
                }
            }
            finishScanIfCurrent(generation)
            drainPendingBootEpgSyncIfIdle(appContext, "BACKGROUND_MAINTENANCE_FINISHED")
        }
        return true
    }

    fun cancel() {
        val task = activeTask.get() ?: return
        task.cancelRequested.set(true)
        setTerminalStateIfCurrent(
            task.generation,
            ScanState.Cancelled(task.generation, task.purpose),
        )
    }

    fun cancelIfCurrent(generation: Int, purpose: ScanPurpose): Boolean {
        val task = activeTask.get() ?: return false
        if (task.generation != generation || task.purpose != purpose) return false
        task.cancelRequested.set(true)
        setTerminalStateIfCurrent(generation, ScanState.Cancelled(generation, purpose))
        return true
    }

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
        if (task.purpose == ScanPurpose.BACKGROUND_MAINTENANCE) {
            BackgroundChannelMaintenanceDiagnostics.cancelledByLiveSessionCount.incrementAndGet()
            BackgroundChannelMaintenanceDiagnostics.lastSkippedReason = decision.diagnosticReason ?: "LIVE_SESSION_PREEMPTED_RUNNING_BACKGROUND_MAINTENANCE"
        }
        task.cancelRequested.set(true)
        setTerminalStateIfCurrent(
            task.generation,
            ScanState.Cancelled(task.generation, task.purpose),
        )
    }

    private fun liveSessionPreemptDecision(scanRunning: Boolean, purpose: ScanPurpose?): LiveSessionPreemptDecision {
        if (!scanRunning || purpose == null) return LiveSessionPreemptDecision(false, false, null)
        return when (purpose) {
            ScanPurpose.BOOT_EPG_SYNC -> LiveSessionPreemptDecision(true, true, "LIVE_SESSION_PREEMPTED_RUNNING_BOOT_EPG_SYNC")
            ScanPurpose.BACKGROUND_MAINTENANCE -> LiveSessionPreemptDecision(true, false, "LIVE_SESSION_PREEMPTED_RUNNING_BACKGROUND_MAINTENANCE")
            ScanPurpose.SETUP_SCAN -> LiveSessionPreemptDecision(false, false, null)
        }
    }

    private fun beginScan(purpose: ScanPurpose, context: Context): ActiveScanTask? {
        val generation = nextGeneration.incrementAndGet()
        val task = ActiveScanTask(generation, purpose, context.applicationContext)
        if (!activeTask.compareAndSet(null, task)) return null
        setState(ScanState.Running(System.currentTimeMillis(), generation, purpose))
        return task
    }

    private fun isScanRunning(): Boolean = activeTask.get() != null

    private fun isCurrentGeneration(generation: Int): Boolean =
        activeTask.get()?.generation == generation

    private fun isCancelledGeneration(generation: Int): Boolean =
        activeTask.get()?.let { it.generation == generation && it.cancelRequested.get() } == true

    private fun setTerminalStateIfCurrent(generation: Int, terminalState: ScanState) {
        if (isCurrentGeneration(generation)) {
            setState(terminalState)
        }
    }

    private fun finishScanIfCurrent(generation: Int) {
        val task = activeTask.get()?.takeIf { it.generation == generation } ?: return
        closeController(task)
        activeTask.compareAndSet(task, null)
    }

    private fun bootEpgSyncStartDecision(activeLiveSessionCount: Int, scanRunning: Boolean, sessionCreationInProgress: Boolean): BootEpgSyncStartDecision = when {
        activeLiveSessionCount > 0 || sessionCreationInProgress -> BootEpgSyncStartDecision(false, "LIVE_SESSION_STARTING_OR_ACTIVE")
        scanRunning -> BootEpgSyncStartDecision(false, "SCAN_RUNNING")
        else -> BootEpgSyncStartDecision(true, null)
    }

    private fun markBootEpgSyncDeferred(context: Context, reason: String) {
        DirectBootGuard.deferPending(context.applicationContext, reason)
        Log.i(LogTags.TIS, "boot EPG 同期を開始しません reason=$reason activeLiveSessions=${activeLiveSessions.get()} sessionCreationsInProgress=${sessionCreationsInProgress.get()} scanRunning=${isScanRunning()}")
    }

    private fun drainPendingBootEpgSyncIfIdle(context: Context, source: String) {
        if (activeLiveSessions.get() > 0 || sessionCreationsInProgress.get() > 0 || isScanRunning()) return
        Log.i(LogTags.TIS, "pending boot EPG 同期ジョブを再登録します source=$source")
        BootEpgSyncScheduler.scheduleIfEligible(context.applicationContext, source)
    }

    private fun markBackgroundMaintenanceSkipped(reason: String, source: String) {
        BackgroundChannelMaintenanceDiagnostics.lastSkippedReason = reason
        when (reason) {
            "ACTIVE_LIVE_SESSION", "LIVE_SESSION_STARTING_OR_ACTIVE" -> BackgroundChannelMaintenanceDiagnostics.skippedActiveLiveSessionCount.incrementAndGet()
            "SCAN_RUNNING" -> BackgroundChannelMaintenanceDiagnostics.skippedScanRunningCount.incrementAndGet()
            else -> BackgroundChannelMaintenanceDiagnostics.skippedOtherCount.incrementAndGet()
        }
        Log.i(LogTags.TIS, "background channel maintenance を開始しません source=$source reason=$reason activeLiveSessions=${activeLiveSessions.get()} sessionCreationsInProgress=${sessionCreationsInProgress.get()} scanRunning=${isScanRunning()}")
    }

    private fun closeController(task: ActiveScanTask) {
        runCatching { task.controller?.close() }
        runCatching { task.engine?.close() }
        task.controller = null
        task.engine = null
    }

    private fun setState(newState: ScanState) {
        state = newState
        listeners.forEach { listener -> runCatching { listener.onScanStateChanged(newState) } }
    }

    private fun backgroundMaintenanceStartDecision(activeLiveSessionCount: Int, scanRunning: Boolean, sessionCreationInProgress: Boolean): BackgroundMaintenanceStartDecision = when {
        activeLiveSessionCount > 0 || sessionCreationInProgress -> BackgroundMaintenanceStartDecision(false, "LIVE_SESSION_STARTING_OR_ACTIVE")
        scanRunning -> BackgroundMaintenanceStartDecision(false, "SCAN_RUNNING")
        else -> BackgroundMaintenanceStartDecision(true, null)
    }

    fun backgroundMaintenanceStartDecisionForTest(activeLiveSessionCount: Int, scanRunning: Boolean, sessionCreationInProgress: Boolean = false): BackgroundMaintenanceStartDecision =
        backgroundMaintenanceStartDecision(activeLiveSessionCount, scanRunning, sessionCreationInProgress)
}

package com.maleicacid.tvinput.tis

import android.content.Context
import android.util.Log
import com.maleicacid.tvinput.aribsi.AribSiEngine
import com.maleicacid.tvinput.common.LogTags
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger

object ChannelScanManager {
    interface Listener { fun onScanStateChanged(state: ScanState) }

    data class BackgroundMaintenanceStartDecision(val allowed: Boolean, val reason: String?)
    data class BootEpgSyncStartDecision(val allowed: Boolean, val reason: String?)

    private val executor: ExecutorService = Executors.newSingleThreadExecutor { runnable ->
        Thread(runnable, "maleicacid-channel-scan-manager").apply { isDaemon = true }
    }
    private val listeners = CopyOnWriteArrayList<Listener>()
    private val running = AtomicBoolean(false)
    private val activeLiveSessions = AtomicInteger(0)
    @Volatile private var state: ScanState = ScanState.Idle
    @Volatile private var controller: ChannelScanController? = null
    @Volatile private var engine: AribSiEngine? = null
    @Volatile private var pendingBootEpgContext: Context? = null
    @Volatile private var pendingBootEpgInputId: String? = null

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

    fun unregisterLiveSession() {
        while (true) {
            val current = activeLiveSessions.get()
            if (current <= 0) return
            if (activeLiveSessions.compareAndSet(current, current - 1)) {
                if (current - 1 == 0) drainPendingBootEpgSyncAfterLiveSessionRelease()
                return
            }
        }
    }

    fun activeLiveSessionCountForTest(): Int = activeLiveSessions.get()

    fun bootEpgSyncStartDecisionForTest(activeLiveSessionCount: Int, scanRunning: Boolean): BootEpgSyncStartDecision =
        bootEpgSyncStartDecision(activeLiveSessionCount, scanRunning)

    fun startIfIdle(context: Context, inputId: String) {
        if (!running.compareAndSet(false, true)) return
        val appContext = context.applicationContext
        setState(ScanState.Running(System.currentTimeMillis()))
        executor.execute {
            val result = runCatching {
                val createdEngine = AribSiEngine(appContext)
                val createdController = ChannelScanController(appContext, inputId, createdEngine)
                engine = createdEngine
                controller = createdController
                createdController.startInitialScan()
            }
            result.onSuccess { scanResult ->
                setState(ScanState.Completed(scanResult))
            }.onFailure { e ->
                Log.w(LogTags.TIS, "チャンネル scan に失敗しました inputId=$inputId", e)
                setState(ScanState.Failed(e.message ?: "不明な例外"))
            }
            closeController()
            running.set(false)
            drainPendingBootEpgSyncIfIdle("SCAN_FINISHED")
        }
    }

    fun startBootEpgSyncIfIdle(context: Context, inputId: String): Boolean {
        val appContext = context.applicationContext
        val precheck = bootEpgSyncStartDecision(activeLiveSessions.get(), running.get())
        if (!precheck.allowed) {
            markBootEpgSyncDeferred(appContext, inputId, precheck.reason ?: "UNKNOWN")
            return false
        }
        if (!running.compareAndSet(false, true)) {
            markBootEpgSyncDeferred(appContext, inputId, "SCAN_RUNNING")
            return false
        }
        if (activeLiveSessions.get() > 0) {
            running.set(false)
            markBootEpgSyncDeferred(appContext, inputId, "ACTIVE_LIVE_SESSION")
            return false
        }
        setState(ScanState.Running(System.currentTimeMillis()))
        executor.execute {
            if (activeLiveSessions.get() > 0) {
                closeController()
                running.set(false)
                setState(ScanState.Idle)
                markBootEpgSyncDeferred(appContext, inputId, "ACTIVE_LIVE_SESSION")
                return@execute
            }
            var shouldScheduleBackgroundMaintenance = false
            val result = runCatching {
                val createdEngine = AribSiEngine(appContext)
                val createdController = ChannelScanController(appContext, inputId, createdEngine)
                engine = createdEngine
                controller = createdController
                createdController.startBootEpgSync()
            }
            result.onSuccess { scanResult ->
                val terminalCancel = scanResult.terminalCancelObserved
                val successCandidateObserved = scanResult.successfulCandidates > 0 && !terminalCancel
                if (successCandidateObserved) {
                    DirectBootGuard.clearPending(appContext)
                    shouldScheduleBackgroundMaintenance = true
                } else {
                    DirectBootGuard.deferPending(appContext, if (terminalCancel) "BOOT_EPG_CANCELLED" else "BOOT_EPG_NO_SUCCESSFUL_CANDIDATE")
                }
                setState(ScanState.Completed(scanResult))
            }.onFailure { e ->
                DirectBootGuard.markTunerUnavailable(appContext)
                Log.w(LogTags.TIS, "boot後EPG同期に失敗しました inputId=$inputId", e)
                setState(ScanState.Failed(e.message ?: "不明な例外"))
            }
            closeController()
            running.set(false)
            if (shouldScheduleBackgroundMaintenance) {
                BackgroundChannelMaintenanceDiagnostics.scheduledAfterBootSyncCount.incrementAndGet()
                startBackgroundChannelMaintenanceIfIdle(appContext, inputId, source = "BOOT_EPG_SYNC_COMPLETED")
            }
        }
        return true
    }

    fun startBackgroundChannelMaintenanceIfIdle(
        context: Context,
        inputId: String,
        source: String = "MANUAL",
    ): Boolean {
        val precheck = backgroundMaintenanceStartDecision(activeLiveSessions.get(), running.get())
        if (!precheck.allowed) {
            markBackgroundMaintenanceSkipped(precheck.reason ?: "UNKNOWN", source)
            return false
        }
        if (!running.compareAndSet(false, true)) {
            markBackgroundMaintenanceSkipped("SCAN_RUNNING", source)
            return false
        }
        if (activeLiveSessions.get() > 0) {
            running.set(false)
            markBackgroundMaintenanceSkipped("ACTIVE_LIVE_SESSION", source)
            return false
        }
        val appContext = context.applicationContext
        setState(ScanState.Running(System.currentTimeMillis()))
        executor.execute {
            if (activeLiveSessions.get() > 0) {
                closeController()
                running.set(false)
                setState(ScanState.Idle)
                markBackgroundMaintenanceSkipped("ACTIVE_LIVE_SESSION", source)
                return@execute
            }
            BackgroundChannelMaintenanceDiagnostics.startedCount.incrementAndGet()
            val result = runCatching {
                val createdEngine = AribSiEngine(appContext)
                val createdController = ChannelScanController(appContext, inputId, createdEngine)
                engine = createdEngine
                controller = createdController
                createdController.startBackgroundChannelMaintenance()
            }
            result.onSuccess { scanResult ->
                setState(ScanState.Completed(scanResult))
            }.onFailure { e ->
                Log.w(LogTags.TIS, "background channel maintenance に失敗しました inputId=$inputId source=$source", e)
                setState(ScanState.Failed(e.message ?: "不明な例外"))
            }
            closeController()
            running.set(false)
            drainPendingBootEpgSyncIfIdle("BACKGROUND_MAINTENANCE_FINISHED")
        }
        return true
    }

    fun cancel() {
        // B-14: executor 外から controller / engine を close しない。cancel request も scan executor 上で順序化する。
        setState(ScanState.Cancelled)
        executor.execute {
            controller?.cancelScan()
            closeController()
            running.set(false)
            drainPendingBootEpgSyncIfIdle("SCAN_CANCELLED")
        }
    }


    private fun bootEpgSyncStartDecision(activeLiveSessionCount: Int, scanRunning: Boolean): BootEpgSyncStartDecision = when {
        activeLiveSessionCount > 0 -> BootEpgSyncStartDecision(false, "ACTIVE_LIVE_SESSION")
        scanRunning -> BootEpgSyncStartDecision(false, "SCAN_RUNNING")
        else -> BootEpgSyncStartDecision(true, null)
    }

    private fun markBootEpgSyncDeferred(context: Context, inputId: String, reason: String) {
        pendingBootEpgContext = context.applicationContext
        pendingBootEpgInputId = inputId
        DirectBootGuard.deferPending(context.applicationContext, reason)
        Log.i(LogTags.TIS, "boot EPG 同期を開始しません reason=$reason activeLiveSessions=${activeLiveSessions.get()} scanRunning=${running.get()}")
    }

    private fun drainPendingBootEpgSyncAfterLiveSessionRelease() {
        drainPendingBootEpgSyncIfIdle("LIVE_SESSION_RELEASED")
    }

    private fun drainPendingBootEpgSyncIfIdle(source: String) {
        val context = pendingBootEpgContext ?: return
        val inputId = pendingBootEpgInputId ?: return
        if (activeLiveSessions.get() > 0 || running.get()) return
        pendingBootEpgContext = null
        pendingBootEpgInputId = null
        Log.i(LogTags.TIS, "pending boot EPG 同期を再試行します source=$source")
        startBootEpgSyncIfIdle(context, inputId)
    }

    private fun markBackgroundMaintenanceSkipped(reason: String, source: String) {
        BackgroundChannelMaintenanceDiagnostics.lastSkippedReason = reason
        when (reason) {
            "ACTIVE_LIVE_SESSION" -> BackgroundChannelMaintenanceDiagnostics.skippedActiveLiveSessionCount.incrementAndGet()
            "SCAN_RUNNING" -> BackgroundChannelMaintenanceDiagnostics.skippedScanRunningCount.incrementAndGet()
            else -> BackgroundChannelMaintenanceDiagnostics.skippedOtherCount.incrementAndGet()
        }
        Log.i(LogTags.TIS, "background channel maintenance を開始しません source=$source reason=$reason activeLiveSessions=${activeLiveSessions.get()} scanRunning=${running.get()}")
    }

    private fun closeController() {
        runCatching { controller?.close() }
        runCatching { engine?.close() }
        controller = null
        engine = null
    }

    private fun setState(newState: ScanState) {
        state = newState
        listeners.forEach { listener -> runCatching { listener.onScanStateChanged(newState) } }
    }

    private fun backgroundMaintenanceStartDecision(activeLiveSessionCount: Int, scanRunning: Boolean): BackgroundMaintenanceStartDecision = when {
        activeLiveSessionCount > 0 -> BackgroundMaintenanceStartDecision(false, "ACTIVE_LIVE_SESSION")
        scanRunning -> BackgroundMaintenanceStartDecision(false, "SCAN_RUNNING")
        else -> BackgroundMaintenanceStartDecision(true, null)
    }

    fun backgroundMaintenanceStartDecisionForTest(activeLiveSessionCount: Int, scanRunning: Boolean): BackgroundMaintenanceStartDecision =
        backgroundMaintenanceStartDecision(activeLiveSessionCount, scanRunning)
}

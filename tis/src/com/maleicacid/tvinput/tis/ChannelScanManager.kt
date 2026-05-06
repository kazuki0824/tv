package com.maleicacid.tvinput.tis

import android.content.Context
import android.util.Log
import com.maleicacid.tvinput.aribsi.AribSiEngine
import com.maleicacid.tvinput.common.LogTags
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean

object ChannelScanManager {
    interface Listener { fun onScanStateChanged(state: ScanState) }

    private val executor: ExecutorService = Executors.newSingleThreadExecutor { runnable ->
        Thread(runnable, "maleicacid-channel-scan-manager").apply { isDaemon = true }
    }
    private val listeners = CopyOnWriteArrayList<Listener>()
    private val running = AtomicBoolean(false)
    @Volatile private var state: ScanState = ScanState.Idle
    @Volatile private var controller: ChannelScanController? = null
    @Volatile private var engine: AribSiEngine? = null

    fun currentState(): ScanState = state

    fun addListener(listener: Listener) {
        listeners += listener
        listener.onScanStateChanged(state)
    }

    fun removeListener(listener: Listener) {
        listeners -= listener
    }

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
        }
    }

    fun cancel() {
        controller?.cancelScan()
        setState(ScanState.Cancelled)
        closeController()
        running.set(false)
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
}

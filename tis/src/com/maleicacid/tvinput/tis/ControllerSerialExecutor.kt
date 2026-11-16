package com.maleicacid.tvinput.tis

import java.util.concurrent.Callable
import java.util.concurrent.Future
import java.util.concurrent.FutureTask
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicLong
import java.util.concurrent.atomic.AtomicReference

/**
 * TunerControllerの単一所有threadを維持したまま、未実行のsection callbackより
 * lifecycle/control境界を先に実行するserial executor。
 */
// Section受付の実行・診断を同一ownerに維持するため、関数数だけで別ownerへ分割しない。
@Suppress("TooManyFunctions")
internal class ControllerSerialExecutor(
    threadName: String,
    maxPendingDataTasks: Int = DEFAULT_MAX_PENDING_DATA_TASKS,
) : PrioritySerialExecutor(threadName, maxPendingDataTasks) {
    private val inFlightPeak = AtomicInteger()
    private val dataCapacityRejections = AtomicLong()
    private val dataTasksFinished = AtomicLong()
    private val dataOwnerNanos = AtomicLong()
    private val dataOwnerMaxNanos = AtomicLong()
    private val controlTasksFinished = AtomicLong()
    private val controlOwnerNanos = AtomicLong()
    private val controlOwnerMaxNanos = AtomicLong()
    private val cleanupTasksFinished = AtomicLong()
    private val cleanupOwnerNanos = AtomicLong()
    private val cleanupOwnerMaxNanos = AtomicLong()
    private val activeMetricsWindow = AtomicReference<MetricsWindow?>()

    internal data class MetricsTotals(
        val rejected: Long,
        val dataFinished: Long,
        val dataNanos: Long,
        val controlFinished: Long,
        val controlNanos: Long,
        val cleanupFinished: Long,
        val cleanupNanos: Long,
    )

    internal class MetricsWindow(
        val baseline: MetricsTotals,
    ) {
        val sampledPeak = AtomicInteger()
    }

    private fun totals(): MetricsTotals =
        MetricsTotals(
            rejected = dataCapacityRejections.get(),
            dataFinished = dataTasksFinished.get(),
            dataNanos = dataOwnerNanos.get(),
            controlFinished = controlTasksFinished.get(),
            controlNanos = controlOwnerNanos.get(),
            cleanupFinished = cleanupTasksFinished.get(),
            cleanupNanos = cleanupOwnerNanos.get(),
        )

    // 候補別に累積差分と標本ピークを分ける。旧世代task・Semaphore取得競合は除外できない。
    internal fun beginMetricsWindow(): MetricsWindow {
        val window = MetricsWindow(totals())
        check(activeMetricsWindow.compareAndSet(null, window)) { "Section計測区間が重複しています" }
        return window
    }

    internal fun finishMetricsWindow(window: MetricsWindow): String {
        check(activeMetricsWindow.compareAndSet(window, null)) { "Section計測区間が一致しません" }
        val end = totals()
        val start = window.baseline
        return "scope=candidateWorkInterval peakPrecision=sampledNonAtomic " +
            "inFlightPeakSampled=${window.sampledPeak.get()} " +
            "capacityRejectionsDelta=${end.rejected - start.rejected} " +
            "dataFinishedDelta=${end.dataFinished - start.dataFinished} " +
            "dataOwnerTotalUsDelta=${(end.dataNanos - start.dataNanos) / NANOS_PER_MICROSECOND} " +
            "controlFinishedDelta=${end.controlFinished - start.controlFinished} " +
            "controlOwnerTotalUsDelta=${(end.controlNanos - start.controlNanos) / NANOS_PER_MICROSECOND} " +
            "cleanupFinishedDelta=${end.cleanupFinished - start.cleanupFinished} " +
            "cleanupOwnerTotalUsDelta=${(end.cleanupNanos - start.cleanupNanos) / NANOS_PER_MICROSECOND}"
    }

    // 単一owner workerだけが参照する。snapshot集計値はAtomicで取得する。
    private var ownerStartedAtNanos = 0L

    override fun onDataSlotsAcquired(inFlight: Int) {
        inFlightPeak.accumulateAndGet(inFlight) { previous, current -> maxOf(previous, current) }
        activeMetricsWindow.get()?.sampledPeak?.accumulateAndGet(inFlight) { previous, current ->
            maxOf(previous, current)
        }
    }

    override fun onDataSlotsRejected(inFlight: Int) {
        dataCapacityRejections.incrementAndGet()
        inFlightPeak.accumulateAndGet(inFlight) { previous, current -> maxOf(previous, current) }
        activeMetricsWindow.get()?.sampledPeak?.accumulateAndGet(inFlight) { previous, current ->
            maxOf(previous, current)
        }
    }

    override fun beforeExecute(
        thread: Thread,
        runnable: Runnable,
    ) {
        super.beforeExecute(thread, runnable)
        ownerStartedAtNanos = System.nanoTime()
    }

    override fun afterExecute(
        runnable: Runnable,
        error: Throwable?,
    ) {
        val elapsedNanos = (System.nanoTime() - ownerStartedAtNanos).coerceAtLeast(0L)
        val queueClass = (runnable as QueuedTask).queueClass
        when (queueClass) {
            DATA_QUEUE_CLASS -> {
                dataTasksFinished.incrementAndGet()
                dataOwnerNanos.addAndGet(elapsedNanos)
                dataOwnerMaxNanos.accumulateAndGet(elapsedNanos) { previous, current -> maxOf(previous, current) }
            }

            CONTROL_QUEUE_CLASS -> {
                controlTasksFinished.incrementAndGet()
                controlOwnerNanos.addAndGet(elapsedNanos)
                controlOwnerMaxNanos.accumulateAndGet(elapsedNanos) { previous, current -> maxOf(previous, current) }
            }

            CLEANUP_QUEUE_CLASS -> {
                cleanupTasksFinished.incrementAndGet()
                cleanupOwnerNanos.addAndGet(elapsedNanos)
                cleanupOwnerMaxNanos.accumulateAndGet(elapsedNanos) { previous, current -> maxOf(previous, current) }
            }
        }
        super.afterExecute(runnable, error)
    }

    // 生涯累積の診断。Semaphore残数の後読みは厳密な瞬間ピークではない。
    fun metricsSnapshot(): String =
        "scope=executorLifetime dataInFlightPeakSampled=${inFlightPeak.get()} " +
            "dataCapacityRejections=${dataCapacityRejections.get()} dataTasksFinished=${dataTasksFinished.get()} " +
            "dataOwnerTotalUs=${dataOwnerNanos.get() / NANOS_PER_MICROSECOND} " +
            "dataOwnerMaxUs=${dataOwnerMaxNanos.get() / NANOS_PER_MICROSECOND} " +
            "controlTasksFinished=${controlTasksFinished.get()} " +
            "controlOwnerTotalUs=${controlOwnerNanos.get() / NANOS_PER_MICROSECOND} " +
            "controlOwnerMaxUs=${controlOwnerMaxNanos.get() / NANOS_PER_MICROSECOND} " +
            "cleanupTasksFinished=${cleanupTasksFinished.get()} " +
            "cleanupOwnerTotalUs=${cleanupOwnerNanos.get() / NANOS_PER_MICROSECOND} " +
            "cleanupOwnerMaxUs=${cleanupOwnerMaxNanos.get() / NANOS_PER_MICROSECOND}"

    override fun execute(command: Runnable) = executeData(command)

    fun executeControl(command: Runnable) {
        enqueue(CONTROL_QUEUE_CLASS, command)
    }

    fun executeData(command: Runnable) = acquireDataSlotAndEnqueue(command)

    fun <T> submitControl(
        cleanup: Boolean = false,
        block: () -> T,
    ): Future<T> {
        val task = FutureTask(Callable(block))
        val onDiscarded: () -> Unit = { task.cancel(false) }
        if (cleanup) {
            enqueueCleanup(CLEANUP_QUEUE_CLASS, task, onDiscarded = onDiscarded)
        } else {
            enqueue(CONTROL_QUEUE_CLASS, task, onDiscarded = onDiscarded)
        }
        return task
    }

    private companion object {
        const val NANOS_PER_MICROSECOND = 1_000L
        const val DEFAULT_MAX_PENDING_DATA_TASKS = 1
    }
}

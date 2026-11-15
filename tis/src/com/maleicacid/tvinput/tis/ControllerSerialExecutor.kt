package com.maleicacid.tvinput.tis

import java.util.concurrent.Callable
import java.util.concurrent.Future
import java.util.concurrent.FutureTask
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicLong

/**
 * TunerControllerの単一所有threadを維持したまま、未実行のsection callbackより
 * lifecycle/control境界を先に実行するserial executor。
 */
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

    // 単一owner workerだけが参照する。snapshot集計値はAtomicで取得する。
    private var ownerStartedAtNanos = 0L

    override fun onDataSlotsAcquired(inFlight: Int) {
        inFlightPeak.accumulateAndGet(inFlight) { previous, current -> maxOf(previous, current) }
    }

    override fun onDataSlotsRejected(inFlight: Int) {
        dataCapacityRejections.incrementAndGet()
        inFlightPeak.accumulateAndGet(inFlight) { previous, current -> maxOf(previous, current) }
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
        if (queueClass == DATA_QUEUE_CLASS) {
            dataTasksFinished.incrementAndGet()
            dataOwnerNanos.addAndGet(elapsedNanos)
            dataOwnerMaxNanos.accumulateAndGet(elapsedNanos) { previous, current -> maxOf(previous, current) }
        } else {
            controlTasksFinished.incrementAndGet()
            controlOwnerNanos.addAndGet(elapsedNanos)
            controlOwnerMaxNanos.accumulateAndGet(elapsedNanos) { previous, current -> maxOf(previous, current) }
        }
        super.afterExecute(runnable, error)
    }

    // 累積値。peakは実行中taskを含む予約枠数であり、queue内task数ではない。
    fun metricsSnapshot(): String =
        "dataInFlightPeak=${inFlightPeak.get()} dataCapacityRejections=${dataCapacityRejections.get()} " +
            "dataTasksFinished=${dataTasksFinished.get()} dataOwnerTotalUs=${dataOwnerNanos.get() / NANOS_PER_MICROSECOND} " +
            "dataOwnerMaxUs=${dataOwnerMaxNanos.get() / NANOS_PER_MICROSECOND} " +
            "controlTasksFinished=${controlTasksFinished.get()} " +
            "controlOwnerTotalUs=${controlOwnerNanos.get() / NANOS_PER_MICROSECOND} " +
            "controlOwnerMaxUs=${controlOwnerMaxNanos.get() / NANOS_PER_MICROSECOND}"

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

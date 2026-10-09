package com.maleicacid.tvinput.tis

import java.util.Comparator
import java.util.concurrent.PriorityBlockingQueue
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.Semaphore
import java.util.concurrent.ThreadFactory
import java.util.concurrent.ThreadPoolExecutor
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicLong
import java.util.concurrent.atomic.AtomicReference

// 二つのownerで共通のqueue順序・thread識別・permit寿命だけを実装する。
// control待機、callback破棄、owner再投入と上限値は各executorの方針として残す。
internal abstract class PrioritySerialExecutor(
    private val ownerName: String,
    maxPendingDataTasks: Int,
) : ThreadPoolExecutor(
        1,
        1,
        0L,
        TimeUnit.MILLISECONDS,
        PriorityBlockingQueue(INITIAL_QUEUE_CAPACITY, TASK_ORDER),
        ThreadFactory { runnable -> Thread(runnable, ownerName).apply { isDaemon = true } },
    ) {
    protected data class QueuedTask(
        val queueClass: Int,
        val sequence: Long,
        val delegate: Runnable,
        val onFinished: (() -> Unit)? = null,
        val onDiscarded: (() -> Unit)? = null,
    ) : Runnable {
        override fun run() {
            try {
                delegate.run()
            } finally {
                onFinished?.invoke()
            }
        }

        fun discardBeforeRun() {
            onDiscarded?.invoke()
        }
    }

    private val ownerThread = AtomicReference<Thread?>()
    private val nextSequence = AtomicLong()
    private val nextCleanupSequence = AtomicLong()
    protected val pendingDataSlots =
        Semaphore(maxPendingDataTasks.also { require(it > 0) { "maxPendingDataTasksは正でなければなりません" } })

    override fun beforeExecute(
        thread: Thread,
        runnable: Runnable,
    ) {
        ownerThread.set(thread)
        super.beforeExecute(thread, runnable)
    }

    fun isOwnerThread(): Boolean = Thread.currentThread() === ownerThread.get()

    override fun shutdownNow(): MutableList<Runnable> {
        val dropped = super.shutdownNow()
        dropped.filterIsInstance<QueuedTask>().forEach(QueuedTask::discardBeforeRun)
        return dropped
    }

    @Suppress("ThrowsCount")
    protected fun acquireDataSlotAndEnqueue(
        command: Runnable,
        onFinished: () -> Unit = { pendingDataSlots.release() },
        onDiscarded: () -> Unit = { pendingDataSlots.release() },
    ) {
        if (!pendingDataSlots.tryAcquire()) {
            throw RejectedExecutionException("$ownerName のdata未処理数が上限に達しました")
        }
        try {
            enqueue(DATA_QUEUE_CLASS, command, onFinished, onDiscarded)
        } catch (error: RejectedExecutionException) {
            pendingDataSlots.release()
            throw error
        } catch (error: IllegalStateException) {
            pendingDataSlots.release()
            throw error
        }
    }

    protected fun enqueue(
        queueClass: Int,
        command: Runnable,
        onFinished: (() -> Unit)? = null,
        onDiscarded: (() -> Unit)? = null,
    ) {
        val sequence = nextSequence.getAndIncrement()
        check(sequence >= 0L) { "$ownerName task sequenceが枯渇しました" }
        super.execute(QueuedTask(queueClass, sequence, command, onFinished, onDiscarded))
    }

    // cleanupの方針は呼出元が決め、通常sequenceを消費しない投入機構だけを共有する。
    protected fun enqueueCleanup(
        queueClass: Int,
        command: Runnable,
    ) {
        val sequence =
            nextCleanupSequence.getAndUpdate {
                check(it < Long.MAX_VALUE) { "$ownerName cleanup sequenceが枯渇しました" }
                it + 1L
            }
        super.execute(QueuedTask(queueClass, sequence, command))
    }

    protected companion object {
        const val CLEANUP_QUEUE_CLASS = -1
        const val CONTROL_QUEUE_CLASS = 0
        const val DATA_QUEUE_CLASS = 1
        private const val INITIAL_QUEUE_CAPACITY = 11
        private val TASK_ORDER: Comparator<Runnable> =
            Comparator { left, right ->
                val leftTask = left as QueuedTask
                val rightTask = right as QueuedTask
                val classOrder = leftTask.queueClass.compareTo(rightTask.queueClass)
                if (classOrder != 0) classOrder else leftTask.sequence.compareTo(rightTask.sequence)
            }
    }
}

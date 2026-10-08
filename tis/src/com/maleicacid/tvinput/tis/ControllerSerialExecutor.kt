package com.maleicacid.tvinput.tis

import java.util.Comparator
import java.util.concurrent.Callable
import java.util.concurrent.Future
import java.util.concurrent.FutureTask
import java.util.concurrent.PriorityBlockingQueue
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.Semaphore
import java.util.concurrent.ThreadFactory
import java.util.concurrent.ThreadPoolExecutor
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicLong
import java.util.concurrent.atomic.AtomicReference

/**
 * TunerControllerの単一所有threadを維持したまま、未実行のsection callbackより
 * lifecycle/control境界を先に実行するserial executor。
 */
internal class ControllerSerialExecutor(
    threadName: String,
    maxPendingDataTasks: Int = DEFAULT_MAX_PENDING_DATA_TASKS,
) : ThreadPoolExecutor(
        1,
        1,
        0L,
        TimeUnit.MILLISECONDS,
        PriorityBlockingQueue(INITIAL_QUEUE_CAPACITY, TASK_ORDER),
        ThreadFactory { runnable ->
            Thread(runnable, threadName).apply { isDaemon = true }
        },
    ) {
    private interface ControlTask

    private class ControlFutureTask<T>(
        callable: Callable<T>,
    ) : FutureTask<T>(callable),
        ControlTask

    private data class QueuedTask(
        val queueClass: Int,
        val sequence: Long,
        val delegate: Runnable,
        val onFinished: (() -> Unit)? = null,
    ) : Runnable {
        override fun run() {
            try {
                delegate.run()
            } finally {
                onFinished?.invoke()
            }
        }

        fun discardBeforeRun() {
            onFinished?.invoke()
        }
    }

    private val ownerThread = AtomicReference<Thread?>()
    private val nextSequence = AtomicLong()
    private val pendingDataSlots =
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

    override fun execute(command: Runnable) {
        if (command is ControlTask) {
            executeControl(command)
        } else {
            executeData(command)
        }
    }

    fun executeControl(command: Runnable) {
        enqueue(CONTROL_QUEUE_CLASS, command)
    }

    @Suppress("ThrowsCount")
    fun executeData(command: Runnable) {
        if (!pendingDataSlots.tryAcquire()) {
            throw RejectedExecutionException("controller dataの未処理数が上限に達しました")
        }
        try {
            enqueue(DATA_QUEUE_CLASS, command) { pendingDataSlots.release() }
        } catch (error: RejectedExecutionException) {
            pendingDataSlots.release()
            throw error
        } catch (error: IllegalStateException) {
            pendingDataSlots.release()
            throw error
        }
    }

    private fun enqueue(
        queueClass: Int,
        command: Runnable,
        onFinished: (() -> Unit)? = null,
    ) {
        val sequence = nextSequence.getAndIncrement()
        check(sequence >= 0L) { "TunerControllerのtask sequenceが枯渇しました" }
        super.execute(
            QueuedTask(
                queueClass = queueClass,
                sequence = sequence,
                delegate = command,
                onFinished = onFinished,
            ),
        )
    }

    fun <T> submitControl(block: () -> T): Future<T> {
        val task = ControlFutureTask(Callable(block))
        executeControl(task)
        return task
    }

    fun <T> submitData(block: () -> T): Future<T> {
        val task = FutureTask(Callable(block))
        executeData(task)
        return task
    }

    private companion object {
        const val INITIAL_QUEUE_CAPACITY = 11
        const val DEFAULT_MAX_PENDING_DATA_TASKS = 1
        const val CONTROL_QUEUE_CLASS = 0
        const val DATA_QUEUE_CLASS = 1

        val TASK_ORDER: Comparator<Runnable> =
            Comparator { left, right ->
                val leftTask = left as QueuedTask
                val rightTask = right as QueuedTask
                val classOrder = leftTask.queueClass.compareTo(rightTask.queueClass)
                if (classOrder != 0) {
                    classOrder
                } else {
                    leftTask.sequence.compareTo(rightTask.sequence)
                }
            }
    }
}

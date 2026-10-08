package com.maleicacid.tvinput.tis

import java.util.Comparator
import java.util.concurrent.Callable
import java.util.concurrent.ExecutionException
import java.util.concurrent.FutureTask
import java.util.concurrent.PriorityBlockingQueue
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.Semaphore
import java.util.concurrent.ThreadFactory
import java.util.concurrent.ThreadPoolExecutor
import java.util.concurrent.TimeUnit
import java.util.concurrent.TimeoutException
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicLong
import java.util.concurrent.atomic.AtomicReference

/** 開始済み操作の結果は未確定であり、同じownerによる後片付けが必要。 */
internal class ControlResultUnknownException(
    threadName: String,
    cause: Exception,
) : IllegalStateException("$threadName の開始済みcontrol結果が未確定です", cause)

// lifecycle/controlをdata callbackより優先し、data未処理数を有限化する単一owner executor。
// queue・permit・control待機を同じ所有者で閉じ、関数数だけを理由に所有者を分割しない。
@Suppress("TooManyFunctions")
internal class LifecycleSerialExecutor(
    private val threadName: String,
    private val maxPendingDataTasks: Int = DEFAULT_MAX_PENDING_DATA_TASKS,
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
        ControlTask {
        private val phase = AtomicInteger(CONTROL_QUEUED)

        override fun run() {
            // 取消しと開始が競合するため、取消し済みtaskの遅延実行をCASで拒否する。
            @Suppress("RedundantIf")
            if (!phase.compareAndSet(CONTROL_QUEUED, CONTROL_RUNNING)) return
            try {
                super.run()
            } finally {
                phase.set(CONTROL_DONE)
            }
        }

        fun cancelBeforeStart(): Boolean = phase.compareAndSet(CONTROL_QUEUED, CONTROL_CANCELLED) && super.cancel(false)
    }

    private data class QueuedTask(
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
    private val deferredOwnerData = ArrayDeque<Runnable>()
    private var stoppingData = false
    private val pendingDataSlots =
        Semaphore(maxPendingDataTasks.also { require(it > 0) { "maxPendingDataTasksは正でなければなりません" } })

    override fun beforeExecute(
        thread: Thread,
        runnable: Runnable,
    ) {
        ownerThread.compareAndSet(null, thread)
        super.beforeExecute(thread, runnable)
    }

    fun isOwnerThread(): Boolean = Thread.currentThread() === ownerThread.get()

    override fun shutdownNow(): MutableList<Runnable> {
        val deferred =
            synchronized(deferredOwnerData) {
                stoppingData = true
                deferredOwnerData.toList().also { deferredOwnerData.clear() }
            }
        val dropped = super.shutdownNow()
        dropped.filterIsInstance<QueuedTask>().forEach(QueuedTask::discardBeforeRun)
        dropped.addAll(deferred)
        return dropped
    }

    override fun execute(command: Runnable) {
        if (command is ControlTask) {
            executeControl(command)
        } else {
            executeData(command)
        }
    }

    fun executeControl(command: Runnable) = enqueue(CONTROL_QUEUE_CLASS, command)

    fun executeData(command: Runnable) {
        if (isOwnerThread()) {
            synchronized(deferredOwnerData) {
                if (stoppingData || isShutdown) throw RejectedExecutionException("$threadName はshutdown済みです")
                check(deferredOwnerData.size < maxPendingDataTasks) {
                    "$threadName のowner-thread data再投入数が上限に達しました"
                }
                deferredOwnerData.addLast(command)
            }
            return
        }
        acquireDataSlotAndEnqueue(command)
    }

    // 割込み・投入拒否・識別子枯渇でpermitを返し、それぞれの失敗を保持する。
    @Suppress("ThrowsCount")
    private fun acquireDataSlotAndEnqueue(command: Runnable) {
        try {
            pendingDataSlots.acquire()
        } catch (error: InterruptedException) {
            Thread.currentThread().interrupt()
            throw RejectedExecutionException("$threadName data enqueue待機が割り込まれました", error)
        }
        try {
            enqueue(DATA_QUEUE_CLASS, command, ::finishDataTask, pendingDataSlots::release)
        } catch (error: RejectedExecutionException) {
            pendingDataSlots.release()
            throw error
        } catch (error: IllegalStateException) {
            pendingDataSlots.release()
            throw error
        }
    }

    private fun finishDataTask() =
        synchronized(deferredOwnerData) {
            val deferred =
                if (stoppingData || isShutdown) {
                    deferredOwnerData.clear()
                    null
                } else {
                    deferredOwnerData.removeFirstOrNull()
                }
            if (deferred == null) {
                pendingDataSlots.release()
                return@synchronized
            }
            try {
                enqueue(DATA_QUEUE_CLASS, deferred, ::finishDataTask, pendingDataSlots::release)
            } catch (error: RejectedExecutionException) {
                deferredOwnerData.clear()
                pendingDataSlots.release()
                if (!isShutdown) throw error
            } catch (error: IllegalStateException) {
                deferredOwnerData.clear()
                pendingDataSlots.release()
                throw error
            }
        }

    // 未開始の取消しと開始済みの結果不明を混同せず、有限待機の終了理由を保持する。
    @Suppress("ThrowsCount")
    fun <T> callControl(
        timeoutMs: Long,
        block: () -> T,
    ): T {
        if (isOwnerThread()) return block()
        val task = ControlFutureTask(Callable(block))
        executeControl(task)
        return try {
            task.get(timeoutMs, TimeUnit.MILLISECONDS)
        } catch (error: TimeoutException) {
            if (task.cancelBeforeStart()) {
                throw IllegalStateException("$threadName control待機が${timeoutMs}msを超過しました", error)
            }
            throw ControlResultUnknownException(threadName, error)
        } catch (error: InterruptedException) {
            Thread.currentThread().interrupt()
            if (!task.cancelBeforeStart()) throw ControlResultUnknownException(threadName, error)
            throw IllegalStateException("$threadName control待機が割り込まれました", error)
        } catch (error: ExecutionException) {
            propagateExecutionFailure(error)
        }
    }

    private fun propagateExecutionFailure(error: ExecutionException): Nothing {
        val cause = error.cause ?: error
        throw when (cause) {
            is Error, is ControlResultUnknownException -> cause
            else -> IllegalStateException("$threadName control taskが失敗しました", cause)
        }
    }

    private fun enqueue(
        queueClass: Int,
        command: Runnable,
        onFinished: (() -> Unit)? = null,
        onDiscarded: (() -> Unit)? = null,
    ) {
        val sequence = nextSequence.getAndIncrement()
        check(sequence >= 0L) { "$threadName task sequenceが枯渇しました" }
        super.execute(
            QueuedTask(
                queueClass = queueClass,
                sequence = sequence,
                delegate = command,
                onFinished = onFinished,
                onDiscarded = onDiscarded,
            ),
        )
    }

    private companion object {
        const val INITIAL_QUEUE_CAPACITY = 11
        const val DEFAULT_MAX_PENDING_DATA_TASKS = 64
        const val CONTROL_QUEUE_CLASS = 0
        const val DATA_QUEUE_CLASS = 1
        const val CONTROL_QUEUED = 0
        const val CONTROL_RUNNING = 1
        const val CONTROL_DONE = 2
        const val CONTROL_CANCELLED = 3

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

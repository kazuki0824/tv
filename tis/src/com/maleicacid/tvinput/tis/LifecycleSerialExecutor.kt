package com.maleicacid.tvinput.tis

import java.util.concurrent.Callable
import java.util.concurrent.ExecutionException
import java.util.concurrent.FutureTask
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.TimeUnit
import java.util.concurrent.TimeoutException
import java.util.concurrent.atomic.AtomicInteger

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
) : PrioritySerialExecutor(threadName, maxPendingDataTasks) {
    private class ControlFutureTask<T>(
        callable: Callable<T>,
    ) : FutureTask<T>(callable) {
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

    private class CallbackTask(
        private val isReleased: () -> Boolean,
        private val onFailure: (RuntimeException) -> Unit,
        private val action: () -> Unit,
        private val onDiscard: () -> Unit,
    ) : Runnable {
        private val discarded =
            java.util.concurrent.atomic
                .AtomicBoolean(false)

        // 開始済み入力の所有権はaction側にある。失敗通知でもonDiscardを重ねない。
        @Suppress("TooGenericExceptionCaught")
        override fun run() {
            if (isReleased()) {
                discard()
                return
            }
            try {
                action()
            } catch (error: RuntimeException) {
                if (!isReleased()) onFailure(error)
            }
        }

        fun discard() {
            if (discarded.compareAndSet(false, true)) onDiscard()
        }

        fun fail(error: RuntimeException) {
            try {
                if (!isReleased()) onFailure(error)
            } finally {
                discard()
            }
        }
    }

    private val deferredOwnerData = ArrayDeque<Runnable>()
    private var stoppingData = false

    override fun shutdownNow(): MutableList<Runnable> {
        val deferred =
            synchronized(deferredOwnerData) {
                stoppingData = true
                deferredOwnerData.toList().also { deferredOwnerData.clear() }
            }
        val dropped = super.shutdownNow()
        deferred.forEach(::discardCallback)
        dropped.addAll(deferred)
        return dropped
    }

    // Filterを閉じたownerが、未実行eventの解放を確認してからexecutorを停止する。
    fun discardDataCallbacks() {
        synchronized(deferredOwnerData) {
            deferredOwnerData.filterIsInstance<CallbackTask>().forEach(CallbackTask::discard)
            deferredOwnerData.removeAll { it is CallbackTask }
        }
        queue.filterIsInstance<QueuedTask>().forEach { task ->
            if (task.queueClass == DATA_QUEUE_CLASS && task.delegate is CallbackTask && remove(task)) {
                task.discardBeforeRun()
            }
        }
    }

    override fun execute(command: Runnable) = executeData(command)

    // terminal cleanupだけは通常task identityを消費しない。同じownerで枯渇後も解放を完了する。
    fun executeCleanupControl(command: Runnable) {
        enqueueCleanup(CLEANUP_QUEUE_CLASS, command)
    }

    fun executeTerminalCleanup(command: Runnable) {
        try {
            executeCleanupControl(command)
        } catch (error: RejectedExecutionException) {
            if (!isShutdown) throw error
        }
    }

    fun executeCallback(
        control: Boolean = false,
        isReleased: () -> Boolean,
        onFailure: (RuntimeException) -> Unit,
        onDiscard: () -> Unit = {},
        action: () -> Unit,
    ) {
        if (isReleased()) {
            onDiscard()
            return
        }
        val task = CallbackTask(isReleased, onFailure, action, onDiscard)
        try {
            if (control) executeControl(task) else executeData(task)
        } catch (error: RejectedExecutionException) {
            if (isShutdown) task.discard() else task.fail(error)
        } catch (error: IllegalStateException) {
            if (isShutdown) task.discard() else task.fail(error)
        }
    }

    fun executeControl(command: Runnable) {
        enqueue(CONTROL_QUEUE_CLASS, command, { finishOwnerTask(hasDataSlot = false) })
    }

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
        acquireDataSlotAndEnqueue(command, { finishOwnerTask(hasDataSlot = true) }, {
            pendingDataSlots.release()
            discardCallback(command)
        })
    }

    private fun finishOwnerTask(hasDataSlot: Boolean): Unit =
        synchronized(deferredOwnerData) {
            if (stoppingData || isShutdown) discardDeferredCallbacks()
            if (deferredOwnerData.isEmpty()) {
                if (hasDataSlot) pendingDataSlots.release()
                return@synchronized
            }
            // control完了はpermitを待たない。dataが満杯なら既存dataの完了が同じqueueをdrainする。
            if (!hasDataSlot && !pendingDataSlots.tryAcquire()) return@synchronized
            val deferred = deferredOwnerData.removeFirst()
            try {
                enqueue(DATA_QUEUE_CLASS, deferred, { finishOwnerTask(hasDataSlot = true) }, {
                    pendingDataSlots.release()
                    discardCallback(deferred)
                })
            } catch (error: RejectedExecutionException) {
                discardDeferredCallbacks()
                pendingDataSlots.release()
                if (!isShutdown) reportDeferredFailure(deferred, error)
            } catch (error: IllegalStateException) {
                discardDeferredCallbacks()
                pendingDataSlots.release()
                if (!isShutdown) reportDeferredFailure(deferred, error)
            }
        }

    private fun discardCallback(task: Runnable) {
        if (task is CallbackTask) task.discard()
    }

    private fun discardDeferredCallbacks() {
        val discarded = deferredOwnerData.toList()
        deferredOwnerData.clear()
        discarded.forEach(::discardCallback)
    }

    private fun reportDeferredFailure(
        task: Runnable,
        error: RuntimeException,
    ) {
        if (task is CallbackTask) task.fail(error) else throw error
    }

    // 未開始の取消しと開始済みの結果不明を混同せず、有限待機の終了理由を保持する。
    @Suppress("ThrowsCount")
    fun <T> callControl(
        timeoutMs: Long,
        cleanup: Boolean = false,
        block: () -> T,
    ): T {
        if (isOwnerThread()) return block()
        val task = ControlFutureTask(Callable(block))
        val onDiscarded: () -> Unit = { task.cancelBeforeStart() }
        if (cleanup) {
            enqueueCleanup(CLEANUP_QUEUE_CLASS, task, onDiscarded = onDiscarded)
        } else {
            enqueue(CONTROL_QUEUE_CLASS, task, { finishOwnerTask(hasDataSlot = false) }, onDiscarded)
        }
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

    private companion object {
        const val DEFAULT_MAX_PENDING_DATA_TASKS = 64
        const val CONTROL_QUEUED = 0
        const val CONTROL_RUNNING = 1
        const val CONTROL_DONE = 2
        const val CONTROL_CANCELLED = 3
    }
}

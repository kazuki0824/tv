package com.maleicacid.tvinput.tis

import java.util.concurrent.Callable
import java.util.concurrent.Future
import java.util.concurrent.FutureTask
import java.util.concurrent.Semaphore

/**
 * TunerControllerの単一所有threadを維持したまま、未実行のsection callbackより
 * lifecycle/control境界を先に実行するserial executor。
 */
internal class ControllerSerialExecutor(
    threadName: String,
    maxPendingDataTasks: Int = DEFAULT_MAX_PENDING_DATA_TASKS,
    reservedDataSlots: Int = 0,
) : PrioritySerialExecutor(threadName, maxPendingDataTasks - reservedDataSlots) {
    init {
        require(reservedDataSlots >= 0) { "予約data枠は負にできません" }
    }

    // SDT枠を通常dataから予約するが、総未処理数と実行ownerは変更しない。
    private val reservedPendingDataSlots = Semaphore(reservedDataSlots)
    override fun execute(command: Runnable) = executeData(command)

    fun executeControl(command: Runnable) {
        enqueue(CONTROL_QUEUE_CLASS, command)
    }

    fun executeData(command: Runnable) = acquireDataSlotAndEnqueue(command)

    fun executeReservedData(command: Runnable) {
        if (!reservedPendingDataSlots.tryAcquire()) {
            throw DataCapacityExceededException("controllerのSDT予約data未処理数が上限に達しました")
        }
        try {
            enqueue(
                DATA_QUEUE_CLASS,
                command,
                onFinished = { reservedPendingDataSlots.release() },
                onDiscarded = { reservedPendingDataSlots.release() },
            )
        } catch (error: RuntimeException) {
            reservedPendingDataSlots.release()
            throw error
        }
    }

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
        const val DEFAULT_MAX_PENDING_DATA_TASKS = 1
    }
}

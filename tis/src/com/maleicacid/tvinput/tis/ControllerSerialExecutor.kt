package com.maleicacid.tvinput.tis

import java.util.concurrent.Callable
import java.util.concurrent.Future
import java.util.concurrent.FutureTask

/**
 * TunerControllerの単一所有threadを維持したまま、未実行のsection callbackより
 * lifecycle/control境界を先に実行するserial executor。
 */
internal class ControllerSerialExecutor(
    threadName: String,
    maxPendingDataTasks: Int = DEFAULT_MAX_PENDING_DATA_TASKS,
) : PrioritySerialExecutor(threadName, maxPendingDataTasks) {
    override fun execute(command: Runnable) = executeData(command)

    fun executeControl(command: Runnable) {
        enqueue(CONTROL_QUEUE_CLASS, command)
    }

    fun executeData(command: Runnable) = acquireDataSlotAndEnqueue(command)

    fun <T> submitControl(block: () -> T): Future<T> {
        val task = FutureTask(Callable(block))
        executeControl(task)
        return task
    }

    private companion object {
        const val DEFAULT_MAX_PENDING_DATA_TASKS = 1
    }
}

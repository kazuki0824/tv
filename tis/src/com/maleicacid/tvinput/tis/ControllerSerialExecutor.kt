package com.maleicacid.tvinput.tis

import java.util.Comparator
import java.util.concurrent.Callable
import java.util.concurrent.Future
import java.util.concurrent.FutureTask
import java.util.concurrent.PriorityBlockingQueue
import java.util.concurrent.ThreadFactory
import java.util.concurrent.ThreadPoolExecutor
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicLong

/**
 * TunerControllerの単一所有threadを維持したまま、未実行のsection callbackより
 * lifecycle/control境界を先に実行するserial executor。
 */
internal class ControllerSerialExecutor(
    threadName: String,
) : ThreadPoolExecutor(
        1,
        1,
        0L,
        TimeUnit.MILLISECONDS,
        PriorityBlockingQueue(11, TASK_ORDER),
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
    ) : Runnable {
        override fun run() = delegate.run()
    }

    private val nextSequence = AtomicLong()

    override fun execute(command: Runnable) {
        val sequence = nextSequence.getAndIncrement()
        check(sequence >= 0L) { "TunerController task sequence exhausted" }
        super.execute(
            QueuedTask(
                queueClass = if (command is ControlTask) CONTROL_QUEUE_CLASS else DATA_QUEUE_CLASS,
                sequence = sequence,
                delegate = command,
            ),
        )
    }

    fun <T> submitControl(block: () -> T): Future<T> {
        val task = ControlFutureTask(Callable(block))
        execute(task)
        return task
    }

    private companion object {
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

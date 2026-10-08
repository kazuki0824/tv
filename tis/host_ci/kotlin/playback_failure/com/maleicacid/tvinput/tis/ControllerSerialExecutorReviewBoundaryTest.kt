// 試験の入力・期待値を本体定数から独立した具体値で固定する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import org.junit.Test
import sun.misc.Unsafe
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference

class ControllerSerialExecutorReviewBoundaryTest {
    @Test
    fun replacementWorkerReentersControllerWithoutSelfQueueWait() {
        val executor = ControllerSerialExecutor("交換worker試験")
        val previous = AtomicReference<Thread>()
        val unsafe =
            Unsafe::class.java
                .getDeclaredField("theUnsafe")
                .apply { isAccessible = true }
                .get(null) as Unsafe
        val controller = unsafe.allocateInstance(TunerController::class.java) as TunerController
        TunerController::class.java
            .getDeclaredField("sectionExecutor")
            .apply { isAccessible = true }
            .set(controller, executor)
        val dataCall =
            TunerController::class.java
                .getDeclaredMethod("callOnControllerData", Function0::class.java)
                .apply { isAccessible = true }
        try {
            executor.executeControl {
                previous.set(Thread.currentThread())
                error("worker交換を起こす試験用例外")
            }
            val result =
                executor
                    .submitControl {
                        check(Thread.currentThread() !== previous.get())
                        check(executor.isOwnerThread())
                        check(controller.currentGeneration() == 0L)
                        dataCall.invoke(controller, { 7 })
                    }.get(WAIT_SECONDS, TimeUnit.SECONDS)
            check(result == 7)
        } finally {
            executor.shutdownNow()
        }
    }

    @Test
    fun controllerExecutorOwnerUsesThreadIdentityNotName() {
        val executor = ControllerSerialExecutor("owner-identity-test")
        try {
            check(executor.submitControl { executor.isOwnerThread() }.get(WAIT_SECONDS, TimeUnit.SECONDS))
            val sameNameOtherThreadResult = arrayOf(false)
            val thread =
                Thread(
                    { sameNameOtherThreadResult[0] = executor.isOwnerThread() },
                    "owner-identity-test",
                )
            thread.start()
            thread.join(JOIN_WAIT_MS)
            check(!sameNameOtherThreadResult[0])
        } finally {
            executor.shutdownNow()
        }
    }

    @Test
    fun controllerExecutorShutdownReturnsDiscardedDataPermit() {
        val executor = ControllerSerialExecutor("shutdown-permit-test", maxPendingDataTasks = 1)
        val controlStarted = CountDownLatch(1)
        val releaseControl = CountDownLatch(1)
        val secondReturned = CountDownLatch(1)
        val producer = Executors.newSingleThreadExecutor()
        try {
            executor.executeControl {
                controlStarted.countDown()
                runCatching { releaseControl.await() }
            }
            check(controlStarted.await(WAIT_SECONDS, TimeUnit.SECONDS))
            executor.executeData { error("破棄対象data taskが実行されました") }
            producer.submit {
                runCatching { executor.executeData {} }
                secondReturned.countDown()
            }
            check(!secondReturned.await(BLOCKED_PROBE_MS, TimeUnit.MILLISECONDS))
            executor.shutdownNow()
            check(secondReturned.await(WAIT_SECONDS, TimeUnit.SECONDS))
        } finally {
            releaseControl.countDown()
            producer.shutdownNow()
            executor.shutdownNow()
        }
    }

    private companion object {
        const val WAIT_SECONDS = 1L
        const val JOIN_WAIT_MS = 1_000L
        const val BLOCKED_PROBE_MS = 50L
    }
}

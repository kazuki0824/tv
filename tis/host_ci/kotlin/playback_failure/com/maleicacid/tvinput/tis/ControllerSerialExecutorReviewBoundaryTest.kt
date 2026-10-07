package com.maleicacid.tvinput.tis

import org.junit.Test
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

class ControllerSerialExecutorReviewBoundaryTest {
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

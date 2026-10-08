// 待機境界を本体budgetと独立した短い値で検証する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import org.junit.Test
import java.util.Collections
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

private fun assertLifecycleSerialExecutorBoundsDataAndPrioritizesControl() {
    val executor = LifecycleSerialExecutor("lifecycle-test", maxPendingDataTasks = 1)
    val firstStarted = CountDownLatch(1)
    val releaseFirst = CountDownLatch(1)
    val secondReturned = CountDownLatch(1)
    val order = Collections.synchronizedList(mutableListOf<String>())
    val producer = Executors.newSingleThreadExecutor()
    try {
        executor.executeData {
            order += "data-running"
            firstStarted.countDown()
            check(releaseFirst.await(1, TimeUnit.SECONDS))
        }
        check(firstStarted.await(1, TimeUnit.SECONDS))
        val producerFuture =
            producer.submit {
                executor.executeData { order += "data-second" }
                secondReturned.countDown()
            }
        val control = CountDownLatch(1)
        executor.executeControl {
            order += "control"
            control.countDown()
        }
        check(!secondReturned.await(50, TimeUnit.MILLISECONDS))
        releaseFirst.countDown()
        check(control.await(1, TimeUnit.SECONDS))
        producerFuture.get(1, TimeUnit.SECONDS)
        executor.shutdown()
        check(executor.awaitTermination(1, TimeUnit.SECONDS))
        check(order == listOf("data-running", "control", "data-second")) { order.toString() }
    } finally {
        releaseFirst.countDown()
        producer.shutdownNow()
        executor.shutdownNow()
    }
}

class LifecycleControlDeadlineTest {
    @Test
    fun shutdownFromOtherThreadDiscardsDeferredDataAndReturnsAllPermits() {
        val executor = LifecycleSerialExecutor("並行shutdown試験", maxPendingDataTasks = 2)
        val deferredReady = CountDownLatch(1)
        val releaseOwner = CountDownLatch(1)
        val shutdownDone = CountDownLatch(1)
        val failure =
            java.util.concurrent.atomic
                .AtomicReference<Throwable?>()
        try {
            executor.executeData {
                executor.executeData { error("deferred dataが実行されました") }
                executor.executeData { error("deferred dataが実行されました") }
                deferredReady.countDown()
                while (releaseOwner.count > 0) {
                    runCatching { releaseOwner.await() }
                }
            }
            check(deferredReady.await(1, TimeUnit.SECONDS))
            executor.executeData { error("queued dataが実行されました") }
            val shutdownThread =
                Thread {
                    try {
                        check(executor.shutdownNow().size == 3)
                    } catch (error: Throwable) {
                        failure.set(error)
                    } finally {
                        shutdownDone.countDown()
                    }
                }
            shutdownThread.start()
            check(shutdownDone.await(1, TimeUnit.SECONDS))
            check(failure.get() == null) { failure.get().toString() }
            releaseOwner.countDown()
            check(executor.awaitTermination(1, TimeUnit.SECONDS))
            val field = LifecycleSerialExecutor::class.java.getDeclaredField("pendingDataSlots").apply { isAccessible = true }
            val slots = field.get(executor) as java.util.concurrent.Semaphore
            check(slots.availablePermits() == 2)
            check(runCatching { executor.executeData {} }.exceptionOrNull() is java.util.concurrent.RejectedExecutionException)
            check(slots.availablePermits() == 2)
        } finally {
            releaseOwner.countDown()
            executor.shutdownNow()
        }
    }

    @Test
    fun lifecycleShutdownReturnsDiscardedDataPermit() {
        val executor = LifecycleSerialExecutor("lifecycle-shutdown試験", maxPendingDataTasks = 1)
        val controlStarted = CountDownLatch(1)
        val releaseControl = CountDownLatch(1)
        val secondReturned = CountDownLatch(1)
        val producer = Executors.newSingleThreadExecutor()
        try {
            executor.executeControl {
                controlStarted.countDown()
                runCatching { releaseControl.await() }
            }
            check(controlStarted.await(1, TimeUnit.SECONDS))
            executor.executeData { error("破棄対象data taskが実行されました") }
            producer.submit {
                runCatching { executor.executeData {} }
                secondReturned.countDown()
            }
            check(!secondReturned.await(50, TimeUnit.MILLISECONDS))
            executor.shutdownNow()
            check(secondReturned.await(1, TimeUnit.SECONDS))
        } finally {
            releaseControl.countDown()
            producer.shutdownNow()
            executor.shutdownNow()
        }
    }

    @Test fun lifecycleOwnerDataReentryReturnsToTailQueue() {
        val executor = LifecycleSerialExecutor("lifecycle再入試験", maxPendingDataTasks = 1)
        val completed = CountDownLatch(1)
        val order = Collections.synchronizedList(mutableListOf<String>())
        try {
            executor.executeData {
                order += "外側開始"
                executor.executeData {
                    order += "再評価"
                    completed.countDown()
                }
                order += "外側終了"
            }
            check(completed.await(1, TimeUnit.SECONDS))
            check(order == listOf("外側開始", "外側終了", "再評価")) { order.toString() }
        } finally {
            executor.shutdownNow()
        }
    }

    @Test
    fun boundedDataRetainsControlPriority() = assertLifecycleSerialExecutorBoundsDataAndPrioritizesControl()

    // 期待する例外型と原因を検証してから、後片付けの順序を観測する試験境界。
    @Suppress("SwallowedException")
    @Test
    fun startedControlReturnsUnknownBeforeCompletionAndRetainsOwnerCleanup() {
        val executor = LifecycleSerialExecutor("開始済み期限試験")
        val caller = Executors.newSingleThreadExecutor()
        val started = CountDownLatch(1)
        val release = CountDownLatch(1)
        val cleaned = CountDownLatch(1)
        val order = Collections.synchronizedList(mutableListOf<String>())
        try {
            val result =
                caller.submit<Boolean> {
                    try {
                        executor.callControl(100L) {
                            started.countDown()
                            check(release.await(2, TimeUnit.SECONDS))
                            order += "操作完了"
                        }
                        false
                    } catch (error: ControlResultUnknownException) {
                        executor.executeControl {
                            order += "後片付け"
                            cleaned.countDown()
                        }
                        true
                    }
                }
            check(started.await(1, TimeUnit.SECONDS))
            check(result.get(1, TimeUnit.SECONDS))
            check(order.isEmpty())
            release.countDown()
            check(cleaned.await(1, TimeUnit.SECONDS))
            check(order == listOf("操作完了", "後片付け"))
        } finally {
            release.countDown()
            caller.shutdownNow()
            executor.shutdownNow()
        }
    }

    @Test
    fun nestedOwnerPreservesUnknownResultForCallerCleanup() {
        val outer = LifecycleSerialExecutor("外側owner")
        val inner = LifecycleSerialExecutor("内側owner")
        val release = CountDownLatch(1)
        try {
            val failure =
                runCatching {
                    outer.callControl(1_000L) {
                        inner.callControl(100L) { check(release.await(2, TimeUnit.SECONDS)) }
                    }
                }.exceptionOrNull()
            check(failure is ControlResultUnknownException)
            check(failure.cause is java.util.concurrent.TimeoutException)
        } finally {
            release.countDown()
            inner.shutdownNow()
            outer.shutdownNow()
        }
    }

    @Test
    fun queuedControlIsCancelledWithoutExecutingAfterTimeout() {
        val executor = LifecycleSerialExecutor("未開始期限試験")
        val started = CountDownLatch(1)
        val release = CountDownLatch(1)
        val executed = AtomicBoolean(false)
        try {
            executor.executeControl {
                started.countDown()
                check(release.await(2, TimeUnit.SECONDS))
            }
            check(started.await(1, TimeUnit.SECONDS))
            val failure = runCatching { executor.callControl(50L) { executed.set(true) } }.exceptionOrNull()
            check(failure is IllegalStateException && failure !is ControlResultUnknownException)
            release.countDown()
            executor.callControl(1_000L) {}
            check(!executed.get())
        } finally {
            release.countDown()
            executor.shutdownNow()
        }
    }

    @Test
    fun interruptedStartedControlRetainsUnknownResultAndInterruptStatus() {
        val executor = LifecycleSerialExecutor("開始済み割込み試験")
        val started = CountDownLatch(1)
        val release = CountDownLatch(1)
        val observed = AtomicBoolean(false)
        val caller =
            Thread {
                try {
                    executor.callControl(2_000L) {
                        started.countDown()
                        check(release.await(2, TimeUnit.SECONDS))
                    }
                } catch (error: ControlResultUnknownException) {
                    observed.set(error.cause is InterruptedException && Thread.currentThread().isInterrupted)
                }
            }
        try {
            caller.start()
            check(started.await(1, TimeUnit.SECONDS))
            caller.interrupt()
            caller.join(1_000L)
            check(!caller.isAlive && observed.get())
        } finally {
            release.countDown()
            caller.interrupt()
            executor.shutdownNow()
        }
    }
}

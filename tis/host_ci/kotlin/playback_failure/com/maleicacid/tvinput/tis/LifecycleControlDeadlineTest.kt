// 待機境界を本体budgetと独立した短い値で検証する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import org.junit.Test
import java.util.Collections
import java.util.concurrent.CancellationException
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.Semaphore
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong
import java.util.concurrent.atomic.AtomicReference

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
                val failure = runCatching { executor.executeData { order += "data-second" } }.exceptionOrNull()
                check(failure is RejectedExecutionException)
                secondReturned.countDown()
            }
        val control = CountDownLatch(1)
        executor.executeControl {
            order += "control"
            control.countDown()
        }
        check(secondReturned.await(1, TimeUnit.SECONDS))
        releaseFirst.countDown()
        check(control.await(1, TimeUnit.SECONDS))
        producerFuture.get(1, TimeUnit.SECONDS)
        executor.shutdown()
        check(executor.awaitTermination(1, TimeUnit.SECONDS))
        check(order == listOf("data-running", "control")) { order.toString() }
    } finally {
        releaseFirst.countDown()
        producer.shutdownNow()
        executor.shutdownNow()
    }
}

// 同じexecutorの投入・期限・shutdown・cleanup境界を一緒に試験する。
@Suppress("TooManyFunctions")
class LifecycleControlDeadlineTest {
    @Test
    fun terminalCleanupCancelsUnstartedNormalControlWithoutTimeout() = assertTerminalCleanupCancelsQueuedControl(false)

    @Test
    fun terminalCleanupCancelsUnstartedCleanupControlWithoutTimeout() = assertTerminalCleanupCancelsQueuedControl(true)

    private fun assertTerminalCleanupCancelsQueuedControl(cleanup: Boolean) {
        val executor = LifecycleSerialExecutor("terminal破棄試験")
        val caller = Executors.newSingleThreadExecutor()
        val entered = CountDownLatch(1)
        val resume = CountDownLatch(1)
        val executed = AtomicBoolean(false)
        try {
            executor.executeControl {
                entered.countDown()
                resume.await()
            }
            check(entered.await(1, TimeUnit.SECONDS))
            executor.executeTerminalCleanup { executor.shutdownNow() }
            val failure =
                caller.submit<Throwable?> {
                    runCatching { executor.callControl(5_000L, cleanup) { executed.set(true) } }.exceptionOrNull()
                }
            val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(1)
            while (executor.queue.size != 2 && System.nanoTime() < deadline) Thread.yield()
            check(executor.queue.size == 2)
            resume.countDown()
            check(executor.awaitTermination(1, TimeUnit.SECONDS))
            check(executor.isShutdown && executor.queue.isEmpty())
            check(failure.get(1, TimeUnit.SECONDS) is CancellationException)
            check(!executed.get())
        } finally {
            resume.countDown()
            executor.shutdownNow()
            caller.shutdownNow()
            check(caller.awaitTermination(1, TimeUnit.SECONDS))
        }
    }

    @Test
    fun cleanupCommandsRemainFifoAfterNormalIdentityExhaustion() {
        val executor = LifecycleSerialExecutor("cleanup FIFO試験")
        val entered = CountDownLatch(1)
        val unblock = CountDownLatch(1)
        val completed = CountDownLatch(3)
        val order = Collections.synchronizedList(mutableListOf<String>())
        try {
            executor.executeControl {
                entered.countDown()
                check(unblock.await(5, TimeUnit.SECONDS))
            }
            check(entered.await(5, TimeUnit.SECONDS))
            val sequence =
                PrioritySerialExecutor::class.java
                    .getDeclaredField("nextSequence")
                    .apply {
                        isAccessible = true
                    }.get(executor) as AtomicLong
            sequence.set(Long.MIN_VALUE)
            for (label in listOf("A", "B", "C")) {
                executor.executeCleanupControl {
                    check(executor.isOwnerThread())
                    order += label
                    completed.countDown()
                }
            }
            unblock.countDown()
            check(completed.await(5, TimeUnit.SECONDS))
            check(order == listOf("A", "B", "C")) { order.toString() }
            check(sequence.get() == Long.MIN_VALUE)
        } finally {
            unblock.countDown()
            executor.shutdownNow()
        }
    }

    @Test
    fun rejectedCallbackDisposesInputAndRetainsFailedReleaseForOwnerRetry() {
        val executor = LifecycleSerialExecutor("拒否資源試験", maxPendingDataTasks = 1)
        val started = CountDownLatch(1)
        val unblock = CountDownLatch(1)
        val released = AtomicBoolean(false)
        val cleanup = ResourceCleanup()
        var attempts = 0
        try {
            executor.executeControl {
                started.countDown()
                unblock.await()
            }
            check(started.await(1, TimeUnit.SECONDS))
            executor.executeData {}
            executor.executeCallback(
                isReleased = released::get,
                onFailure = { released.set(true) },
                onDiscard = {
                    cleanup.release("拒否event") {
                        attempts++
                        check(attempts > 1)
                    }
                },
                action = { error("拒否eventを受理しました") },
            )
            check(released.get() && attempts == 1 && cleanup.hasPending)
            unblock.countDown()
            executor.callControl(1_000) {
                cleanup.retry()
                cleanup.requireComplete()
            }
            check(attempts == 2 && !cleanup.hasPending)
            released.set(false)
            val failed = CountDownLatch(1)
            var notifications = 0
            executor.executeCallback(
                isReleased = released::get,
                onFailure = {
                    check(executor.isOwnerThread())
                    notifications++
                    released.set(true)
                    executor.executeTerminalCleanup {
                        cleanup.release("開始済みevent") { attempts++ }
                        failed.countDown()
                    }
                },
                onDiscard = { error("開始済み入力を再び破棄しました") },
            ) { error("実行開始後のcallback失敗") }
            check(failed.await(1, TimeUnit.SECONDS))
            check(released.get() && notifications == 1 && attempts == 3 && !cleanup.hasPending)
        } finally {
            unblock.countDown()
            executor.shutdownNow()
        }
    }

    @Test
    fun ownerDiscardsAcceptedInputsOnceBeforeShutdownAndKeepsFailedCleanup() {
        val executor = LifecycleSerialExecutor("未実行event回収試験")
        val started = CountDownLatch(1)
        val unblock = CountDownLatch(1)
        val released = AtomicBoolean(false)
        val cleanup = ResourceCleanup()
        val cleanupDone = CountDownLatch(1)
        var attempts = 0
        try {
            executor.executeControl {
                started.countDown()
                unblock.await()
            }
            check(started.await(1, TimeUnit.SECONDS))
            executor.executeCallback(
                isReleased = released::get,
                onFailure = { throw IllegalStateException("受理済みeventの投入が失敗しました", it) },
                onDiscard = {
                    cleanup.release("未実行event") {
                        attempts++
                        check(attempts > 1)
                    }
                },
                action = { error("閉鎖後のeventを実行しました") },
            )
            released.set(true)
            executor.executeCleanupControl {
                executor.discardDataCallbacks()
                check(cleanup.hasPending && !executor.isShutdown && attempts == 1)
                executor.discardDataCallbacks()
                check(attempts == 1)
                cleanup.retry()
                cleanup.requireComplete()
                cleanupDone.countDown()
            }
            unblock.countDown()
            check(cleanupDone.await(1, TimeUnit.SECONDS))
            check(attempts == 2 && !cleanup.hasPending)
        } finally {
            unblock.countDown()
            executor.shutdownNow()
        }
    }

    @Test
    fun exhaustedCallbackIdentityFencesBothDataAndControlAndRetainsOwnerCleanup() {
        listOf(false, true).forEach { control ->
            val executor = LifecycleSerialExecutor("投入失敗試験")
            val released = AtomicBoolean(false)
            val cause = AtomicReference<RuntimeException>()
            val cleaned = CountDownLatch(1)
            try {
                val sequence =
                    PrioritySerialExecutor::class.java
                        .getDeclaredField("nextSequence")
                        .apply { isAccessible = true }
                (sequence.get(executor) as AtomicLong).set(Long.MIN_VALUE)
                executor.executeCallback(control, released::get, { error ->
                    cause.set(error)
                    released.set(true)
                    executor.executeTerminalCleanup {
                        check(executor.isOwnerThread())
                        cleaned.countDown()
                    }
                }) { error("投入失敗した操作を実行しました") }
                check(cleaned.await(1, TimeUnit.SECONDS))
                check(released.get() && cause.get() is IllegalStateException)
                // 通常identityは再利用せず、失敗したcleanupも同じownerで再試行できる。
                check(runCatching { executor.callControl(1_000L, cleanup = true) { error("解放失敗") } }.isFailure)
                check(executor.callControl(1_000L, cleanup = true) { executor.isOwnerThread() })
                check(runCatching { executor.executeControl {} }.isFailure)
            } finally {
                executor.shutdownNow()
            }
        }
    }

    @Test
    fun deferredCallbackDrainFailureRetainsCauseAndOwnerCleanup() {
        val executor = LifecycleSerialExecutor("再入投入失敗試験", maxPendingDataTasks = 1)
        val released = AtomicBoolean(false)
        val cause = AtomicReference<RuntimeException>()
        val cleaned = CountDownLatch(1)
        try {
            executor.executeControl {
                executor.executeCallback(isReleased = released::get, onFailure = { error ->
                    cause.set(error)
                    released.set(true)
                    executor.executeTerminalCleanup {
                        check(executor.isOwnerThread())
                        cleaned.countDown()
                    }
                }) { error("枯渇後のdeferred callbackを実行しました") }
                val sequence =
                    PrioritySerialExecutor::class.java
                        .getDeclaredField("nextSequence")
                        .apply { isAccessible = true }
                (sequence.get(executor) as AtomicLong).set(Long.MIN_VALUE)
            }
            check(cleaned.await(1, TimeUnit.SECONDS))
            check(cause.get() is IllegalStateException && released.get())
        } finally {
            executor.shutdownNow()
        }
    }

    @Test
    fun ownerCallbackOverflowIsTerminalRatherThanSilentDrop() {
        val executor = LifecycleSerialExecutor("再入上限試験", maxPendingDataTasks = 1)
        val released = AtomicBoolean(false)
        val cleaned = CountDownLatch(1)
        try {
            executor.executeControl {
                executor.executeData { error("terminal後のdataを実行しました") }
                executor.executeCallback(isReleased = released::get, onFailure = { error ->
                    check(error is IllegalStateException)
                    released.set(true)
                    executor.executeTerminalCleanup {
                        executor.shutdownNow()
                        cleaned.countDown()
                    }
                }) { error("上限超過callbackを実行しました") }
            }
            check(cleaned.await(1, TimeUnit.SECONDS))
            check(released.get())
        } finally {
            executor.shutdownNow()
        }
    }

    @Test
    fun shutdownAndReleasedCallbacksAreTheOnlyIgnoredSubmissionFailures() {
        val executor = LifecycleSerialExecutor("shutdown投入試験")
        try {
            executor.executeCallback(
                isReleased = { true },
                onFailure = { throw IllegalStateException("release後を失敗通知しました", it) },
            ) {
                error("release後のcallbackを実行しました")
            }
            executor.shutdownNow()
            listOf(false, true).forEach { control ->
                executor.executeCallback(
                    control,
                    { false },
                    { throw IllegalStateException("shutdown競合を失敗通知しました", it) },
                ) {
                    error("shutdown後のcallbackを実行しました")
                }
            }
        } finally {
            executor.shutdownNow()
        }
    }

    @Test
    fun replacementLifecycleWorkerReentersControlAndData() {
        val executor = LifecycleSerialExecutor("交換lifecycle worker試験", maxPendingDataTasks = 1)
        val previous = AtomicReference<Thread>()
        val completed = CountDownLatch(1)
        val order = Collections.synchronizedList(mutableListOf<String>())
        try {
            executor.executeControl {
                previous.set(Thread.currentThread())
                error("worker交換を起こす試験用例外")
            }
            executor.executeControl {
                check(Thread.currentThread() !== previous.get() && executor.isOwnerThread())
                check(executor.callControl(100L) { 7 } == 7)
                executor.executeData {
                    order += "data"
                    completed.countDown()
                }
                order += "control終了"
            }
            check(completed.await(1, TimeUnit.SECONDS))
            check(order == listOf("control終了", "data"))
        } finally {
            executor.shutdownNow()
        }
    }

    @Test
    fun controlDataReentryDrainsWithoutAnotherExternalDataTask() {
        val executor = LifecycleSerialExecutor("control data再入試験", maxPendingDataTasks = 1)
        val completed = CountDownLatch(1)
        val order = Collections.synchronizedList(mutableListOf<String>())
        try {
            executor.executeControl {
                order += "control開始"
                executor.executeData {
                    order += "data"
                    completed.countDown()
                }
                order += "control終了"
            }
            check(completed.await(1, TimeUnit.SECONDS))
            check(order == listOf("control開始", "control終了", "data"))
        } finally {
            executor.shutdownNow()
        }
    }

    @Test
    fun shutdownFromOtherThreadDiscardsDeferredDataAndReturnsAllPermits() {
        val executor = LifecycleSerialExecutor("並行shutdown試験", maxPendingDataTasks = 2)
        val deferredReady = CountDownLatch(1)
        val releaseOwner = CountDownLatch(1)
        val shutdownDone = CountDownLatch(1)
        val failure = AtomicReference<Throwable?>()
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
                    failure.set(runCatching { check(executor.shutdownNow().size == 3) }.exceptionOrNull())
                    shutdownDone.countDown()
                }
            shutdownThread.start()
            check(shutdownDone.await(1, TimeUnit.SECONDS))
            check(failure.get() == null) { failure.get().toString() }
            releaseOwner.countDown()
            check(executor.awaitTermination(1, TimeUnit.SECONDS))
            val field =
                PrioritySerialExecutor::class.java
                    .getDeclaredField("pendingDataSlots")
                    .apply { isAccessible = true }
            val slots = field.get(executor) as Semaphore
            check(slots.availablePermits() == 2)
            val rejected = runCatching { executor.executeData {} }.exceptionOrNull()
            check(rejected is RejectedExecutionException)
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
            check(secondReturned.await(1, TimeUnit.SECONDS))
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

// 試験の入力・期待値を本体定数から独立した具体値で固定する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import com.maleicacid.tvinput.aribsi.NativeAribCaptionFactParser
import com.maleicacid.tvinput.common.TsPid
import org.junit.Test
import sun.misc.Unsafe
import java.util.concurrent.CancellationException
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Semaphore
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicLong
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
                .getDeclaredMethod("postOnControllerData", Function0::class.java)
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
                        dataCall.invoke(controller, { Unit })
                        7
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
    fun controllerExecutorExpandedBudgetAdmitsMultipleFilterBurstsButRemainsBounded() {
        val executor = ControllerSerialExecutor("multi-filter-section-budget", maxPendingDataTasks = 16)
        val started = CountDownLatch(1)
        val releaseControl = CountDownLatch(1)
        val slots =
            generateSequence<Class<*>>(executor.javaClass) { it.superclass }
                .mapNotNull { type -> type.declaredFields.singleOrNull { it.name == "pendingDataSlots" } }
                .first()
                .apply { isAccessible = true }
                .get(executor) as Semaphore
        try {
            executor.expandDataTaskCapacity(32)
            executor.executeControl {
                started.countDown()
                runCatching { releaseControl.await() }
            }
            check(started.await(WAIT_SECONDS, TimeUnit.SECONDS))
            repeat(32) { executor.executeData { error("未実行data taskが実行されました") } }
            check(slots.availablePermits() == 0)
            check(runCatching { executor.executeData {} }.exceptionOrNull() is DataCapacityExceededException)
            check(executor.shutdownNow().size == 32)
            check(slots.availablePermits() == 32)
        } finally {
            releaseControl.countDown()
            executor.shutdownNow()
        }
    }

    @Test
    fun controllerExecutorShutdownReturnsDiscardedDataPermit() {
        val executor = ControllerSerialExecutor("shutdown-permit-test", maxPendingDataTasks = 1)
        val controlStarted = CountDownLatch(1)
        val releaseControl = CountDownLatch(1)
        val slots =
            generateSequence<Class<*>>(executor.javaClass) { it.superclass }
                .mapNotNull { type -> type.declaredFields.singleOrNull { it.name == "pendingDataSlots" } }
                .first()
                .apply { isAccessible = true }
                .get(executor) as Semaphore
        try {
            executor.executeControl {
                controlStarted.countDown()
                runCatching { releaseControl.await() }
            }
            check(controlStarted.await(WAIT_SECONDS, TimeUnit.SECONDS))
            executor.executeData { error("破棄対象data taskが実行されました") }
            check(slots.availablePermits() == 0)
            check(runCatching { executor.executeData {} }.isFailure)
            check(slots.availablePermits() == 0)
            check(executor.shutdownNow().size == 1)
            check(slots.availablePermits() == 1)
        } finally {
            releaseControl.countDown()
            executor.shutdownNow()
        }
    }

    // 採番境界と本番releaseの失敗・再試行を同じownerで検査する。
    @Suppress("LongMethod")
    @Test
    fun controllerReleaseRetriesCleanupAfterNormalSequenceExhaustion() {
        val executor = ControllerSerialExecutor("枯渇後controller解放試験")
        var cleanupCalls = 0
        val controller =
            controllerForRelease(executor) {
                cleanupCalls++
                check(cleanupCalls != 1) { "解放境界の試験用失敗" }
            }
        val sequence =
            PrioritySerialExecutor::class.java
                .getDeclaredField("nextSequence")
                .apply { isAccessible = true }
                .get(executor) as AtomicLong
        val released = TunerController::class.java.getDeclaredField("released").apply { isAccessible = true }
        try {
            sequence.set(Long.MAX_VALUE)
            executor.submitControl {}.get(WAIT_SECONDS, TimeUnit.SECONDS)
            check(sequence.get() == Long.MIN_VALUE)
            check(runCatching { executor.submitControl {} }.isFailure)
            val failure = runCatching { controller.release() }.exceptionOrNull()
            check(failure?.message == "解放境界の試験用失敗")
            check(cleanupCalls == 1 && !released.getBoolean(controller) && !executor.isShutdown)
            controller.release()
            check(cleanupCalls == 3 && released.getBoolean(controller) && executor.isShutdown)
            controller.release()
            check(cleanupCalls == 3)
        } finally {
            executor.shutdownNow()
        }
    }

    // 優先releaseが受理済みcontrolを破棄したとき、同期callerを待機中のまま残さない。
    @Suppress("LongMethod")
    @Test
    fun priorityReleaseFinishesDiscardedCurrentGenerationCaller() {
        val executor = ControllerSerialExecutor("優先release終了通知試験")
        val controller = controllerForRelease(executor)
        val ownerStarted = CountDownLatch(1)
        val resumeOwner = CountDownLatch(1)
        val resumeNormal = CountDownLatch(1)
        val currentDone = CountDownLatch(1)
        val releaseDone = CountDownLatch(1)
        val currentFailure = AtomicReference<Throwable>()
        val releaseFailure = AtomicReference<Throwable>()
        val currentCaller =
            Thread {
                currentFailure.set(runCatching { controller.currentGeneration() }.exceptionOrNull())
                currentDone.countDown()
            }
        val releaseCaller =
            Thread {
                releaseFailure.set(runCatching { controller.release() }.exceptionOrNull())
                releaseDone.countDown()
            }
        try {
            executor.executeControl {
                ownerStarted.countDown()
                runCatching { resumeOwner.await() }
            }
            check(ownerStarted.await(WAIT_SECONDS, TimeUnit.SECONDS))
            executor.submitControl {
                runCatching { resumeNormal.await() }
                Unit
            }
            currentCaller.start()
            awaitQueuedTasks(executor, 2)
            releaseCaller.start()
            awaitQueuedTasks(executor, 3)
            resumeOwner.countDown()
            check(releaseDone.await(WAIT_SECONDS, TimeUnit.SECONDS))
            check(releaseFailure.get() == null && executor.isShutdown)
            check(currentDone.await(WAIT_SECONDS, TimeUnit.SECONDS))
            check(currentFailure.get() is CancellationException)
        } finally {
            resumeOwner.countDown()
            resumeNormal.countDown()
            executor.shutdownNow()
            currentCaller.interrupt()
            releaseCaller.interrupt()
            currentCaller.join(JOIN_WAIT_MS)
            releaseCaller.join(JOIN_WAIT_MS)
        }
    }

    @Test
    fun controllerShutdownCompletesUnstartedNormalAndCleanupFutures() {
        val executor = ControllerSerialExecutor("control Future破棄試験")
        val started = CountDownLatch(1)
        val resume = CountDownLatch(1)
        try {
            executor.executeControl {
                started.countDown()
                runCatching { resume.await() }
            }
            check(started.await(WAIT_SECONDS, TimeUnit.SECONDS))
            val normal = executor.submitControl<Unit> { error("破棄対象の通常controlが実行されました") }
            val cleanup = executor.submitControl<Unit>(cleanup = true) { error("破棄対象のcleanupが実行されました") }
            check(executor.shutdownNow().size == 2)
            listOf(normal, cleanup).forEach { task ->
                check(task.isDone && task.isCancelled)
                val failure = runCatching { task.get(WAIT_SECONDS, TimeUnit.SECONDS) }.exceptionOrNull()
                check(failure is CancellationException)
            }
        } finally {
            resume.countDown()
            executor.shutdownNow()
        }
    }

    private fun awaitQueuedTasks(
        executor: ControllerSerialExecutor,
        count: Int,
    ) {
        val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(WAIT_SECONDS)
        while (executor.queue.size != count && System.nanoTime() < deadline) Thread.yield()
        check(executor.queue.size == count)
    }

    // Android資源なしで、本番releaseのcontroller所有と投入・終了境界を検査する共通fixture。
    private fun controllerForRelease(
        executor: ControllerSerialExecutor,
        onClear: () -> Unit = {},
    ): TunerController {
        val playbackExecutor = LifecycleSerialExecutor("解放済みplayback試験")
        playbackExecutor.shutdownNow()
        val unsafe =
            Unsafe::class.java
                .getDeclaredField("theUnsafe")
                .apply { isAccessible = true }
                .get(null) as Unsafe
        val controller = unsafe.allocateInstance(TunerController::class.java) as TunerController
        val playback = unsafe.allocateInstance(PlaybackPipeline::class.java) as PlaybackPipeline

        fun set(
            target: Any,
            name: String,
            value: Any,
        ) {
            target.javaClass
                .getDeclaredField(name)
                .apply { isAccessible = true }
                .set(target, value)
        }
        set(controller, "sectionExecutor", executor)
        set(controller, "playbackPipeline", playback)
        set(playback, "executor", playbackExecutor)
        listOf(
            "failedDynamicPmtPids",
            "failedDynamicEcmPids",
            "failedDynamicEmmPids",
            "dynamicPmtPids",
            "dynamicEcmPids",
            "dynamicEmmPids",
        ).forEach { set(controller, it, linkedSetOf<TsPid>()) }
        listOf("sectionFilters", "sectionFilterHandles").forEach { set(controller, it, LinkedHashMap<TsPid, Any>()) }
        set(controller, "captionFactParsers", ConcurrentHashMap<TsPid, NativeAribCaptionFactParser>())
        set(controller, "superimposeTimingByPid", ConcurrentHashMap<TsPid, Int>())
        val languages =
            object : ConcurrentHashMap<TsPid, List<NativeAribCaptionFactParser.Language>>() {
                override fun clear() {
                    onClear()
                    super.clear()
                }
            }
        set(controller, "captionLanguagesByPid", languages)
        return controller
    }

    private companion object {
        const val WAIT_SECONDS = 1L
        const val JOIN_WAIT_MS = 1_000L
    }
}

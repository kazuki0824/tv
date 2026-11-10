// Android 15の実Filterを使用し、callback lockとowner closeの競合を固定する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import android.media.tv.tuner.filter.Filter
import android.media.tv.tuner.filter.FilterCallback
import android.media.tv.tuner.filter.FilterEvent
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.util.ReflectionHelpers
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executor
import java.util.concurrent.TimeUnit

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class FrameworkFilterCallbackLockTest {
    @Test
    fun sectionHandoffReturnsBeforeOwnerCompletesAndCloseCanAcquireFrameworkLock() {
        val executor = ControllerSerialExecutor("Framework section配送試験")
        val controller = TunerController(RuntimeEnvironment.getApplication(), "framework-lock-test")
        ReflectionHelpers.getField<ControllerSerialExecutor>(controller, "sectionExecutor").shutdownNow()
        TunerController::class.java
            .getDeclaredField("sectionExecutor")
            .apply { isAccessible = true }
            .set(controller, executor)
        val post =
            TunerController::class.java
                .getDeclaredMethod("postOnControllerData", Function0::class.java)
                .apply { isAccessible = true }
        val ownerStarted = CountDownLatch(1)
        val releaseOwner = CountDownLatch(1)
        val callbackStarted = CountDownLatch(1)
        val callbackReturned = CountDownLatch(1)
        val filter =
            frameworkFilter(
                object : FilterCallback {
                    override fun onFilterEvent(
                        filter: Filter,
                        events: Array<FilterEvent>,
                    ) {
                        callbackStarted.countDown()
                        post.invoke(controller, { Unit })
                    }

                    override fun onFilterStatusChanged(
                        filter: Filter,
                        status: Int,
                    ) = Unit
                },
                Executor { it.run() },
            )
        val delivery =
            Filter::class.java
                .getDeclaredMethod("onFilterEvent", Array<FilterEvent>::class.java)
                .apply { isAccessible = true }
        val producer =
            Thread {
                try {
                    delivery.invoke(filter, emptyArray<FilterEvent>())
                } finally {
                    callbackReturned.countDown()
                }
            }
        try {
            executor.executeControl {
                ownerStarted.countDown()
                releaseOwner.await()
            }
            check(ownerStarted.await(1, TimeUnit.SECONDS))
            producer.start()
            check(callbackStarted.await(1, TimeUnit.SECONDS))
            val close = executor.submitControl { filter.close() }
            releaseOwner.countDown()
            check(callbackReturned.await(1, TimeUnit.SECONDS))
            close.get(1, TimeUnit.SECONDS)
        } finally {
            releaseOwner.countDown()
            producer.interrupt()
            controller.close()
            executor.shutdownNow()
            producer.join(1_000)
        }
    }

    @Test
    fun sixtyFourQueuedDataTasksDoNotBlockFrameworkCallbackOrPriorityClose() {
        val executor = LifecycleSerialExecutor("Framework容量試験")
        val ownerStarted = CountDownLatch(1)
        val releaseOwner = CountDownLatch(1)
        val returned = CountDownLatch(1)
        val rejected =
            java.util.concurrent.atomic
                .AtomicReference<Throwable>()
        val filter =
            frameworkFilter(
                object : FilterCallback {
                    override fun onFilterEvent(
                        filter: Filter,
                        events: Array<FilterEvent>,
                    ) = Unit

                    override fun onFilterStatusChanged(
                        filter: Filter,
                        status: Int,
                    ) = Unit
                },
                Executor { task ->
                    runCatching { executor.execute(task) }.onFailure(rejected::set)
                },
            )
        val delivery =
            Filter::class.java
                .getDeclaredMethod("onFilterEvent", Array<FilterEvent>::class.java)
                .apply { isAccessible = true }
        val producer =
            Thread {
                try {
                    delivery.invoke(filter, emptyArray<FilterEvent>())
                } finally {
                    returned.countDown()
                }
            }
        try {
            executor.executeControl {
                ownerStarted.countDown()
                releaseOwner.await()
            }
            check(ownerStarted.await(1, TimeUnit.SECONDS))
            repeat(64) { executor.executeData {} }
            producer.start()
            check(returned.await(1, TimeUnit.SECONDS))
            check(rejected.get() is java.util.concurrent.RejectedExecutionException)
            val close = CountDownLatch(1)
            executor.executeControl {
                filter.close()
                close.countDown()
            }
            releaseOwner.countDown()
            check(close.await(1, TimeUnit.SECONDS))
        } finally {
            releaseOwner.countDown()
            executor.shutdownNow()
            producer.interrupt()
            producer.join(1_000)
        }
    }

    private fun frameworkFilter(
        callback: FilterCallback,
        executor: Executor,
    ): Filter {
        val filter =
            ReflectionHelpers.callConstructor(
                Filter::class.java,
                ReflectionHelpers.ClassParameter.from(Long::class.javaPrimitiveType, 1L),
            )
        // Javaの実close境界まで検証し、native handleの作成は行わない。
        Filter::class.java
            .getDeclaredField("mIsClosed")
            .apply { isAccessible = true }
            .setBoolean(filter, true)
        filter.setCallback(callback, executor)
        return filter
    }
}

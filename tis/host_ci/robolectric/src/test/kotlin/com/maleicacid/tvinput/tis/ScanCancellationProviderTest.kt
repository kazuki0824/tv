@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import android.app.job.JobParameters
import android.content.ComponentName
import android.content.Context
import android.content.pm.ProviderInfo
import android.database.MatrixCursor
import android.media.tv.ITvInputManager
import android.media.tv.TvContract
import android.media.tv.TvInputInfo
import android.media.tv.TvInputManager
import android.net.Uri
import android.os.Looper
import android.os.UserManager
import com.maleicacid.tvinput.aribsi.AribSiEngine
import com.maleicacid.tvinput.aribsi.ProviderDataBridge
import com.maleicacid.tvinput.common.FrequencyHz
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.db.ChannelRecord
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.shadow.api.Shadow
import org.robolectric.shadows.ShadowContentResolver
import org.robolectric.shadows.ShadowJobService
import org.robolectric.util.ReflectionHelpers
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

@RunWith(RobolectricTestRunner::class)
@Config(
    sdk = [35],
    instrumentedPackages = ["com.maleicacid.tvinput.aribsi.AribSiEngine"],
    shadows = [TunerFrontendLeaseTransitionTest.SiEngineBoundary::class],
)
class ScanCancellationProviderTest {
    @Test
    fun stopWithoutPendingBootWorkDoesNotReschedule() {
        val service = Robolectric.buildService(BootEpgSyncJobService::class.java).create().get()
        DirectBootGuard.clearPending(service)
        val entered = CountDownLatch(1)
        val release = CountDownLatch(1)
        ChannelScanManager.prepareEpgRefresh {
            entered.countDown()
            check(release.await(5, TimeUnit.SECONDS))
        }
        try {
            assertTrue(entered.await(5, TimeUnit.SECONDS))
            val params = Shadow.newInstanceOf(JobParameters::class.java)
            assertFalse(service.onStartJob(params))
            assertFalse(service.onStopJob(params))
            assertFalse(DirectBootGuard.isPending(service))
        } finally {
            release.countDown()
        }
    }

    @Test
    fun stoppedBootJobDiscardsProviderReadinessResult() {
        for (available in listOf(true, false)) verifyStoppedReadiness(available)
    }

    private fun verifyStoppedReadiness(available: Boolean) {
        val service = Robolectric.buildService(BootEpgSyncJobService::class.java).create().get()
        shadowOf(checkNotNull(service.getSystemService(UserManager::class.java))).setUserUnlocked(true)
        DirectBootGuard.markBootEpgSyncRequested(service, "test")
        val entered = CountDownLatch(1)
        val release = CountDownLatch(1)
        val provider =
            object : TvProviderVisibilityTransactionTest.VisibilityProvider() {
                override fun query(
                    uri: Uri,
                    projection: Array<out String>?,
                    selection: String?,
                    selectionArgs: Array<out String>?,
                    sortOrder: String?,
                ): MatrixCursor {
                    check(Looper.myLooper() !== Looper.getMainLooper())
                    entered.countDown()
                    check(release.await(5, TimeUnit.SECONDS))
                    check(available) { "TvProvider unavailable" }
                    return MatrixCursor(checkNotNull(projection))
                }
            }
        provider.attachInfo(service, ProviderInfo().apply { authority = TvContract.AUTHORITY })
        ShadowContentResolver.registerProviderInternal(TvContract.AUTHORITY, provider)
        val params = Shadow.newInstanceOf(JobParameters::class.java)
        try {
            assertTrue(service.onStartJob(params))
            assertTrue(entered.await(5, TimeUnit.SECONDS))
            assertTrue(service.onStopJob(params))
        } finally {
            release.countDown()
        }
        val drained = CountDownLatch(1)
        ChannelScanManager.prepareEpgRefresh { drained.countDown() }
        assertTrue(drained.await(5, TimeUnit.SECONDS))
        shadowOf(Looper.getMainLooper()).idle()
        assertTrue(DirectBootGuard.isPending(service))
        assertEquals("BOOT_EPG_JOB_STOPPED", DirectBootGuard.pendingState(service).lastSkippedReason)
        assertFalse(Shadow.extract<ShadowJobService>(service).getIsJobFinished())
        provider.database.close()
    }

    @Test
    fun dailyRequestStartsOnceAfterPendingCleanupSucceeds() {
        val service = Robolectric.buildService(PeriodicEpgRefreshJobService::class.java).create().get()
        val context = service.applicationContext
        val input = TvInputInfo.Builder(context, ComponentName(context, MaleicacidTvInputService::class.java)).build()
        val manager =
            TvInputManager(
                object : ITvInputManager.Default() {
                    override fun getTvInputList(userId: Int): List<TvInputInfo> = listOf(input)
                },
                0,
            )
        val app = shadowOf(RuntimeEnvironment.getApplication())
        val ready = ReflectionHelpers.getField<AtomicBoolean>(ProgramUpgradeCleanup, "ready")
        val previousReady = ready.getAndSet(false)
        val release = CountDownLatch(1)
        val complete = CountDownLatch(1)
        val queries = AtomicInteger()
        val provider =
            object : TvProviderVisibilityTransactionTest.VisibilityProvider() {
                override fun query(
                    uri: Uri,
                    projection: Array<out String>?,
                    selection: String?,
                    selectionArgs: Array<out String>?,
                    sortOrder: String?,
                ): MatrixCursor {
                    queries.incrementAndGet()
                    val channel =
                        ChannelRecord(
                            ServiceKey(1, 2, 3),
                            1,
                            "011",
                            "試験局",
                            FrequencyHz(473142857),
                            casFactsCanonicalJson = testCasFacts(false),
                        )
                    val data = ProviderDataBridge.buildChannelProviderData(channel)
                    check(data is ProviderDataBridge.Success) { data.toString() }
                    val bytes = data.bytes
                    return MatrixCursor(checkNotNull(projection)).apply {
                        addRow(arrayOf(7L, 1, 2, 3, "011", "試験局", "1", TvContract.Channels.TYPE_ISDB_T, bytes))
                    }
                }
            }
        provider.attachInfo(context, ProviderInfo().apply { authority = TvContract.AUTHORITY })
        ShadowContentResolver.registerProviderInternal(TvContract.AUTHORITY, provider)
        val generation = ReflectionHelpers.getField<AtomicInteger>(ChannelScanManager, "nextGeneration")
        val before = generation.get()
        val params = Shadow.newInstanceOf(JobParameters::class.java)
        try {
            app.setSystemService(Context.TV_INPUT_SERVICE, manager)
            assertFalse(ProgramUpgradeCleanup.ensure(cleanup = { release.await(5, TimeUnit.SECONDS) }))
            assertTrue(service.onStartJob(params))
            assertFalse(ProgramUpgradeCleanup.ensure(cleanup = { error("重複起動") }) { complete.countDown() })
            shadowOf(Looper.getMainLooper()).idle()
            assertEquals(0, queries.get())
            release.countDown()
            assertTrue(complete.await(5, TimeUnit.SECONDS))
            shadowOf(Looper.getMainLooper()).idle()
            val read = CountDownLatch(1)
            ChannelScanManager.prepareEpgRefresh { read.countDown() }
            assertTrue(read.await(5, TimeUnit.SECONDS))
            shadowOf(Looper.getMainLooper()).idle()
            assertEquals(1, queries.get())
            assertEquals(before + 1, generation.get())
        } finally {
            service.onStopJob(params)
            release.countDown()
            ready.set(previousReady)
            app.removeSystemService(Context.TV_INPUT_SERVICE)
            provider.database.close()
        }
    }

    @Test
    fun stopWithRedeliveredParametersCancelsOnlyTheCurrentPeriodicRun() {
        val service = Robolectric.buildService(PeriodicEpgRefreshJobService::class.java).create().get()

        fun params(id: Int): JobParameters {
            val value = Shadow.newInstanceOf(JobParameters::class.java)
            ReflectionHelpers.setField(value, "jobId", id)
            return value
        }
        val original = params(17)
        val stop = params(17)
        check(original !== stop)
        val task = ChannelScanManager.ActiveScanTask(9, ScanPurpose.PERIODIC_EPG_REFRESH, service)
        val taskOwner =
            ReflectionHelpers.getField<AtomicReference<ChannelScanManager.ActiveScanTask?>>(
                ChannelScanManager,
                "activeTask",
            )
        val previousState = ChannelScanManager.currentState()
        val runClass = BootEpgSyncJobService::class.java.declaredClasses.single { it.simpleName == "RunContext" }
        val ctor = runClass.declaredConstructors.single { it.parameterCount == 5 }.apply { isAccessible = true }
        val stopped = AtomicBoolean()
        val run =
            ctor.newInstance(
                original,
                AtomicBoolean(),
                stopped,
                AtomicInteger(task.generation),
                AtomicReference<Any?>(),
            )
        val owner = ReflectionHelpers.getField<AtomicReference<Any?>>(service, "activeRun")
        owner.set(run)
        check(taskOwner.compareAndSet(null, task))
        try {
            service.onStopJob(params(18))
            check(owner.get() === run && !task.cancelRequested.get())
            // 完了通知がメインスレッドに届く前に、同じジョブの停止が届く。
            val finish =
                BootEpgSyncJobService::class.java.getDeclaredMethod(
                    "finish",
                    runClass,
                    Boolean::class.javaPrimitiveType,
                )
            finish.isAccessible = true
            finish.invoke(service, run, false)
            stopDuringChannelRead(service, run, stop)
            check(stopped.get() && task.cancelRequested.get() && owner.get() == null)
            val next = ChannelScanManager.ActiveScanTask(10, ScanPurpose.PERIODIC_EPG_REFRESH, service)
            taskOwner.set(next)
            service.onStopJob(stop)
            shadowOf(Looper.getMainLooper()).idle()
            assertFalse(next.cancelRequested.get())
            assertFalse(Shadow.extract<ShadowJobService>(service).getIsJobFinished())
        } finally {
            taskOwner.set(null)
            ReflectionHelpers.setField(ChannelScanManager, "state", previousState)
        }
    }

    private fun stopDuringChannelRead(
        service: PeriodicEpgRefreshJobService,
        run: Any,
        stop: JobParameters,
    ) {
        val entered = CountDownLatch(1)
        val release = CountDownLatch(1)
        val drained = CountDownLatch(1)
        val consumed = AtomicBoolean()
        val channels =
            object : AbstractList<ChannelRecord>() {
                override val size: Int
                    get() {
                        consumed.set(true)
                        return 0
                    }

                override fun get(index: Int): ChannelRecord = throw IndexOutOfBoundsException()
            }
        val query: () -> Result<List<ChannelRecord>> = {
            check(Looper.myLooper() !== Looper.getMainLooper())
            entered.countDown()
            check(release.await(5, TimeUnit.SECONDS))
            Result.success(channels)
        }
        val readTargets = BootEpgSyncJobService::class.java.declaredMethods.single { it.name == "readTargets" }
        readTargets.isAccessible = true
        readTargets.invoke(service, run, "input.test", query)
        try {
            assertTrue(entered.await(1, TimeUnit.SECONDS))
            assertFalse(service.onStopJob(stop))
        } finally {
            release.countDown()
        }
        ChannelScanManager.prepareEpgRefresh { drained.countDown() }
        assertTrue(drained.await(1, TimeUnit.SECONDS))
        shadowOf(Looper.getMainLooper()).idle()
        assertFalse(consumed.get())
    }

    @Test
    fun uiCancellationReturnsWhileProviderPublicationOrFinalCommitIsBlocked() {
        for (finalizing in listOf(false, true)) {
            val context = RuntimeEnvironment.getApplication()
            val task = ChannelScanManager.ActiveScanTask(1, ScanPurpose.SETUP_SCAN, context)
            val engine = AribSiEngine(context)
            val controller =
                ChannelScanController(
                    context,
                    "scan-cancellation-test",
                    engine,
                    ScanPurpose.SETUP_SCAN,
                    task.cancelRequested,
                    task.publicationLock,
                )
            val fence =
                ReflectionHelpers.getField<ChannelScanController.ScanGenerationFence>(controller, "scanGenerationFence")
            task.controller = controller
            val entered = CountDownLatch(1)
            val release = CountDownLatch(1)
            val workers = Executors.newFixedThreadPool(2)
            try {
                val publish =
                    workers.submit {
                        val blockedProvider = {
                            entered.countDown()
                            check(release.await(5, TimeUnit.SECONDS))
                        }
                        if (finalizing) {
                            fence.finishScan(blockedProvider)
                        } else {
                            fence.publishIfCurrent(1L, blockedProvider)
                        }
                    }
                assertTrue(entered.await(1, TimeUnit.SECONDS))
                val cancel = workers.submit<Boolean> { task.requestCancel() }
                assertEquals(!finalizing, cancel.get(1, TimeUnit.SECONDS))
                assertEquals(!finalizing, task.cancelRequested.get())
                release.countDown()
                publish.get(1, TimeUnit.SECONDS)
                if (!finalizing) {
                    assertEquals(null, fence.publishIfCurrent(1L) { error("取消し後に公開しました") })
                }
                fence.finishScan { Unit }
                assertFalse(task.requestCancel())
            } finally {
                release.countDown()
                workers.shutdownNow()
                controller.close()
                engine.close()
            }
        }
    }
}

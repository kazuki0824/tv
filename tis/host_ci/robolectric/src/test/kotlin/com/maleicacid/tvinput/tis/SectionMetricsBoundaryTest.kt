// 境界の競合と起動時burstを固定する。時間値は端末性能の期待値ではない。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import android.media.tv.tuner.filter.FilterEvent
import android.media.tv.tuner.filter.SectionEvent
import com.maleicacid.tvinput.aribsi.AribSiEngine
import com.maleicacid.tvinput.common.LogTags
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.db.ChannelRecord
import com.maleicacid.tvinput.tis.TunerFrontendLeaseTransitionTest.Companion.terrestrial
import com.maleicacid.tvinput.tis.TunerFrontendLeaseTransitionTest.NativeFilter
import com.maleicacid.tvinput.tis.TunerFrontendLeaseTransitionTest.NativeTuner
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.annotation.Implementation
import org.robolectric.annotation.Implements
import org.robolectric.annotation.RealObject
import org.robolectric.shadow.api.Shadow
import org.robolectric.shadows.ShadowLog
import org.robolectric.util.ReflectionHelpers
import java.util.concurrent.CountDownLatch
import java.util.concurrent.FutureTask
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference

@RunWith(RobolectricTestRunner::class)
@Config(
    sdk = [35],
    instrumentedPackages = [
        "com.maleicacid.tvinput.aribsi",
        "com.maleicacid.tvinput.tis.ControllerSerialExecutor",
    ],
    shadows = [
        SectionMetricsBoundaryTest.TotalsBoundary::class,
        TunerFrontendLeaseTransitionTest.NativeTuner::class,
        TunerFrontendLeaseTransitionTest.NativeFilter::class,
        TunerFrontendLeaseTransitionTest.NativeTrm::class,
        TunerFrontendLeaseTransitionTest.SiEngineBoundary::class,
    ],
)
class SectionMetricsBoundaryTest {
    @Test
    fun beginAndFinishGapsCanCountWorkWithoutSamplingItsPeak() {
        for (atBegin in listOf(true, false)) verifyBoundaryGap(atBegin)
    }

    // Windowの本体処理を実行し、totals読取りの前後だけを試験側で停止する。
    @Suppress("LongMethod")
    private fun verifyBoundaryGap(atBegin: Boolean) {
        val executor = ControllerSerialExecutor("Section区間境界試験", maxPendingDataTasks = 2)
        val ownerStarted = CountDownLatch(1)
        val releaseOwner = CountDownLatch(1)
        val boundaryReached = CountDownLatch(1)
        val resumeBoundary = CountDownLatch(1)
        val dataFinished = CountDownLatch(2)
        val pauseOnce = AtomicBoolean(true)
        val shadow = Shadow.extract<TotalsBoundary>(executor)
        val window = if (atBegin) null else executor.beginMetricsWindow()
        val pause = {
            if (pauseOnce.compareAndSet(true, false)) {
                boundaryReached.countDown()
                check(resumeBoundary.await(5, TimeUnit.SECONDS))
            }
            Unit
        }
        if (atBegin) shadow.afterTotals = pause else shadow.beforeTotals = pause
        val result = FutureTask { executor.finishMetricsWindow(window ?: executor.beginMetricsWindow()) }
        val reader = Thread(result)
        try {
            executor.executeControl {
                ownerStarted.countDown()
                releaseOwner.await()
            }
            check(ownerStarted.await(5, TimeUnit.SECONDS))
            reader.start()
            if (!boundaryReached.await(5, TimeUnit.SECONDS)) {
                result.get(1, TimeUnit.SECONDS)
                error("本体の累積読取り境界へ接続できませんでした")
            }
            repeat(2) { executor.executeData { dataFinished.countDown() } }
            check(runCatching { executor.executeData {} }.exceptionOrNull() is DataCapacityExceededException)
            releaseOwner.countDown()
            check(dataFinished.await(5, TimeUnit.SECONDS))
            executor.submitControl {}.get(5, TimeUnit.SECONDS)
            resumeBoundary.countDown()
            val metrics = result.get(5, TimeUnit.SECONDS)
            check("inFlightPeakSampled=0 capacityRejectionsDelta=1 dataFinishedDelta=2" in metrics)
            check("deltaPrecision=perCounterNonAtomic boundaryAlignment=notGuaranteed" in metrics)
            val next = executor.beginMetricsWindow()
            check("capacityRejectionsDelta=0" in executor.finishMetricsWindow(next))
        } finally {
            releaseOwner.countDown()
            resumeBoundary.countDown()
            reader.join(5_000)
            executor.shutdownNow()
        }
    }

    @Test
    fun firstFilterBurstIsObservedForSetupAndMaintenance() {
        for (maintenance in listOf(false, true)) verifyScanAttempt(maintenance, rejectFirst = false)
    }

    @Test
    fun failedTuneClosesWindowBeforeTheNextAttempt() {
        for (maintenance in listOf(false, true)) verifyScanAttempt(maintenance, rejectFirst = true)
    }

    // 同じscan ownerで失敗→成功、初期Filter開始→取消し・解放までを通す。
    @Suppress("LongMethod")
    private fun verifyScanAttempt(
        maintenance: Boolean,
        rejectFirst: Boolean,
    ) {
        NativeTuner.instances.clear()
        NativeTuner.current = null
        NativeTuner.successfulTunes = !rejectFirst
        NativeTuner.failNextConstruction = false
        val context = RuntimeEnvironment.getApplication()
        val engine = AribSiEngine(context)
        val cancelled = AtomicBoolean(false)
        val scan = ChannelScanController(context, "Section起動計測試験", engine, ScanPurpose.SETUP_SCAN, cancelled)
        val controller = ReflectionHelpers.getField<TunerController>(scan, "tunerController")
        val executor = ReflectionHelpers.getField<ControllerSerialExecutor>(controller, "sectionExecutor")
        val injected = AtomicBoolean(false)
        NativeFilter.onStart = {
            val sdk = checkNotNull(NativeTuner.current)
            // PAT開始済み、次Filterのstart中に別threadからPAT callbackを配送する。
            if (sdk.filters.size == 2 && injected.compareAndSet(false, true)) {
                injectStartupBurst(sdk)
                cancelled.set(true)
            }
        }
        try {
            ShadowLog.clear()
            if (rejectFirst) {
                val failed = runScan(scan, maintenance)
                check(failed.diagnostics.any { "選局に失敗" in it.message })
                val rejectedMetrics = scanMetrics().single()
                check("scope=candidateAttemptWorkInterval" in rejectedMetrics)
                check("capacityRejectionsDelta=0" in rejectedMetrics)
                check(!injected.get())
                NativeTuner.successfulTunes = true
                ShadowLog.clear()
            }
            val result = runScan(scan, maintenance)
            check(injected.get())
            check(result.terminal.outcome == ChannelScanController.ScanTerminalOutcome.CANCELLED)
            val metrics = scanMetrics().single()
            check("inFlightPeakSampled=16 capacityRejectionsDelta=1" in metrics)
            check("generationAttribution=mixed" in metrics)
            // 選局・取消しのfinallyで閉じたことを、次の計測受付でも確認する。
            val next = executor.beginMetricsWindow()
            executor.finishMetricsWindow(next)
            check(checkNotNull(NativeTuner.current).filters.all { Shadow.extract<NativeFilter>(it).closes == 1 })
        } finally {
            NativeFilter.onStart = null
            scan.close()
            engine.close()
        }
    }

    private fun runScan(
        scan: ChannelScanController,
        maintenance: Boolean,
    ): ChannelScanController.ScanResult =
        if (maintenance) {
            scan.startBootEpgSync(
                listOf(
                    ChannelRecord(
                        serviceKey = ServiceKey(1, 1, 1),
                        serviceType = 1,
                        displayName = "試験局",
                        displayNumber = "13",
                        frequencyHz = terrestrial().frequencyHz,
                    ),
                ),
            )
        } else {
            scan.startInitialScan(listOf(terrestrial()))
        }

    private fun scanMetrics(): List<String> =
        ShadowLog.getLogsForTag(LogTags.TIS).map { it.msg }.filter { it.startsWith("SECTION_EXECUTOR_METRICS ") }

    private fun injectStartupBurst(sdk: NativeTuner) {
        val pat = sdk.filters.first()
        val payload =
            byteArrayOf(
                0,
                0xb0.toByte(),
                13,
                0,
                0x11,
                0xc1.toByte(),
                0,
                0,
                0,
                1,
                0xe1.toByte(),
                0,
                0x45,
                0x21,
                0xf9.toByte(),
                0xb6.toByte(),
            )
        val event =
            ReflectionHelpers.callConstructor(
                SectionEvent::class.java,
                ReflectionHelpers.ClassParameter.from(Int::class.javaPrimitiveType, 0),
                ReflectionHelpers.ClassParameter.from(Int::class.javaPrimitiveType, 0),
                ReflectionHelpers.ClassParameter.from(Int::class.javaPrimitiveType, 0),
                ReflectionHelpers.ClassParameter.from(Long::class.javaPrimitiveType, payload.size.toLong()),
            )
        val failure = AtomicReference<Throwable>()
        val producer =
            Thread {
                failure.set(
                    runCatching {
                        repeat(17) {
                            Shadow.extract<NativeFilter>(pat).offer(payload)
                            sdk.filterCallbacks.getValue(pat).onFilterEvent(pat, arrayOf<FilterEvent>(event))
                        }
                    }.exceptionOrNull(),
                )
            }
        producer.start()
        producer.join(5_000)
        check(!producer.isAlive && failure.get() == null) { "起動時Section配送失敗: ${failure.get()}" }
    }

    @Implements(ControllerSerialExecutor::class)
    class TotalsBoundary {
        @RealObject private lateinit var actual: ControllerSerialExecutor
        var beforeTotals: (() -> Unit)? = null
        var afterTotals: (() -> Unit)? = null

        // Robolectricの明示名検索を使い、privateのまま本体呼出しへ接続する。
        @Implementation(methodName = "totals")
        @Suppress("UnusedPrivateMember")
        private fun totals(): ControllerSerialExecutor.MetricsTotals {
            beforeTotals?.invoke()
            val result =
                Shadow.directlyOn<ControllerSerialExecutor.MetricsTotals, ControllerSerialExecutor>(
                    actual,
                    ControllerSerialExecutor::class.java,
                    "totals",
                )
            afterTotals?.invoke()
            return result
        }
    }
}

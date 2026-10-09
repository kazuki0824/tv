// SDKのscan登録寿命を実メソッドで検査し、native境界だけを代替する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import android.content.Context
import android.media.tv.tuner.Tuner
import android.media.tv.tuner.frontend.Atsc3PlpInfo
import android.media.tv.tuner.frontend.FrontendSettings
import android.media.tv.tuner.frontend.IsdbsFrontendSettings
import android.media.tv.tuner.frontend.ScanCallback
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.annotation.Implementation
import org.robolectric.annotation.Implements
import org.robolectric.shadow.api.Shadow
import org.robolectric.util.ReflectionHelpers
import java.util.concurrent.Executor
import java.util.concurrent.locks.ReentrantLock
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertNull
import kotlin.test.assertSame
import kotlin.test.assertTrue

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], shadows = [FrameworkTunerScanBoundaryTest.NativeScanBoundary::class])
class FrameworkTunerScanBoundaryTest {
    private val executor = Executor { it.run() }
    private val firstRf = IsdbsFrontendSettings.builder().setFrequencyLong(1_049_480_000L).build()
    private val secondRf = IsdbsFrontendSettings.builder().setFrequencyLong(1_087_840_000L).build()

    @Test
    fun stoppedNotificationAloneRejectsNextCallbackBeforeNativeScan() {
        val tuner = frameworkTuner()
        val native = Shadow.extract<NativeScanBoundary>(tuner)
        val first = TunerController.StreamIdDiscoveryOperation(1L)
        val firstCallback = callback(first)
        assertEquals(Tuner.RESULT_SUCCESS, tuner.scan(firstRf, Tuner.SCAN_TYPE_AUTO, executor, firstCallback))
        ReflectionHelpers.callInstanceMethod<Unit>(tuner, "onScanStopped")
        assertTrue(first.await(1))
        assertSame(firstCallback, ReflectionHelpers.getField<ScanCallback>(tuner, "mScanCallback"))
        val nextCallback = callback(TunerController.StreamIdDiscoveryOperation(2L))
        assertFailsWith<IllegalStateException> {
            tuner.scan(secondRf, Tuner.SCAN_TYPE_AUTO, executor, nextCallback)
        }
        assertEquals(listOf(firstRf.frequencyLong), native.scannedFrequencies)
        assertEquals(0, native.stopCalls)
    }

    @Test
    fun productionCleanupReleasesSdkRegistrationAndKeepsLeaseAndStoppedResult() {
        for (stopResult in listOf(Tuner.RESULT_SUCCESS, Tuner.RESULT_INVALID_STATE)) {
            val tuner = frameworkTuner()
            val native = Shadow.extract<NativeScanBoundary>(tuner)
            native.stopResult = stopResult
            val first = TunerController.StreamIdDiscoveryOperation(3L)
            first.start { tuner.scan(firstRf, Tuner.SCAN_TYPE_AUTO, executor, callback(first)) }
            ReflectionHelpers.callInstanceMethod<Unit>(
                tuner,
                "onInputStreamIds",
                ReflectionHelpers.ClassParameter.from(IntArray::class.java, intArrayOf(16400, 16401)),
            )
            ReflectionHelpers.callInstanceMethod<Unit>(tuner, "onScanStopped")
            val stopped = first.result(true)
            val cleaned =
                first.resultWithCleanup(
                    first.await(1),
                    cleanup = { first.cancel { tuner.cancelScanning() } },
                    diagnose = { throw it },
                )
            assertEquals(stopped, cleaned)
            assertTrue(cleaned.success)
            assertEquals(setOf(16400, 16401), cleaned.streamIds)
            assertNull(ReflectionHelpers.getField<ScanCallback?>(tuner, "mScanCallback"))
            assertNull(ReflectionHelpers.getField<Executor?>(tuner, "mScanCallbackExecutor"))
            assertEquals(1, native.stopCalls)
            assertEquals(17, ReflectionHelpers.getField<Int>(tuner, "mFrontendHandle"))

            val next = TunerController.StreamIdDiscoveryOperation(4L)
            val nextCallback = callback(next)
            assertEquals(Tuner.RESULT_SUCCESS, tuner.scan(secondRf, Tuner.SCAN_TYPE_AUTO, executor, nextCallback))
            assertSame(nextCallback, ReflectionHelpers.getField<ScanCallback>(tuner, "mScanCallback"))
            assertEquals(listOf(firstRf.frequencyLong, secondRf.frequencyLong), native.scannedFrequencies)
            assertEquals(17, ReflectionHelpers.getField<Int>(tuner, "mFrontendHandle"))
            assertEquals(stopped, first.result(true))
        }
    }

    private fun frameworkTuner(): Tuner {
        val tuner = Tuner(RuntimeEnvironment.getApplication(), null, 0)
        ReflectionHelpers.setField(tuner, "mFrontendLock", ReentrantLock())
        ReflectionHelpers.setField(tuner, "mScanCallbackLock", Any())
        // TRMで確保済みの同一leaseを与え、SDKの資源照合も実処理を通す。
        ReflectionHelpers.setField(tuner, "mFrontendHandle", 17)
        return tuner
    }

    @Implements(Tuner::class)
    class NativeScanBoundary {
        val scannedFrequencies = mutableListOf<Long>()
        var stopResult = Tuner.RESULT_SUCCESS
        var stopCalls = 0

        // constructorのnative/TRM接続を省略し、下の試験fixtureで確保済みleaseを与える。
        @Implementation(methodName = "__constructor__")
        @Suppress("UNUSED_PARAMETER")
        private fun constructTuner(
            context: Context,
            tvInputSessionId: String?,
            useCase: Int,
        ) = Unit

        @Implementation
        private fun nativeScan(
            settingsType: Int,
            settings: FrontendSettings,
            scanType: Int,
        ): Int {
            assertEquals(FrontendSettings.TYPE_ISDBS, settingsType)
            assertEquals(Tuner.SCAN_TYPE_AUTO, scanType)
            scannedFrequencies += settings.frequencyLong
            return Tuner.RESULT_SUCCESS
        }

        @Implementation
        private fun nativeStopScan(): Int {
            stopCalls++
            return stopResult
        }
    }

    private fun callback(operation: TunerController.StreamIdDiscoveryOperation): ScanCallback =
        object : ScanCallback {
            override fun onLocked() = Unit

            override fun onUnlocked() = Unit

            override fun onScanStopped() {
                operation.complete()
            }

            override fun onProgress(percent: Int) {
                operation.reportProgress(percent)
            }

            @Suppress("DEPRECATION")
            override fun onFrequenciesReported(frequencies: IntArray) = Unit

            override fun onFrequenciesLongReported(frequencies: LongArray) = Unit

            override fun onSymbolRatesReported(rate: IntArray) = Unit

            override fun onPlpIdsReported(plpIds: IntArray) = Unit

            override fun onGroupIdsReported(groupIds: IntArray) = Unit

            override fun onInputStreamIdsReported(inputStreamIds: IntArray) {
                operation.reportIds(inputStreamIds)
            }

            override fun onDvbsStandardReported(dvbsStandard: Int) = Unit

            override fun onDvbtStandardReported(dvbtStandard: Int) = Unit

            override fun onAnalogSifStandardReported(sif: Int) = Unit

            override fun onAtsc3PlpInfosReported(atsc3PlpInfos: Array<Atsc3PlpInfo>) = Unit

            override fun onHierarchyReported(hierarchy: Int) = Unit

            override fun onSignalTypeReported(signalType: Int) = Unit

            override fun onModulationReported(modulation: Int) = Unit

            override fun onPriorityReported(isHighPriority: Boolean) = Unit

            override fun onDvbcAnnexReported(dvbcAnnex: Int) = Unit

            override fun onDvbtCellIdsReported(dvbtCellIds: IntArray) = Unit
        }
}

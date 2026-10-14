// TunerController公開選局入口を通す。Frameworkのnative/TRM接続だけをShadowで代替する。
package com.maleicacid.tvinput.tis

import android.content.Context
import android.media.tv.tuner.Tuner
import android.media.tv.tuner.frontend.FrontendSettings
import com.maleicacid.tvinput.common.FrequencyHz
import com.maleicacid.tvinput.db.ChannelRecord
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.annotation.Implementation
import org.robolectric.annotation.Implements
import org.robolectric.annotation.RealObject
import org.robolectric.util.ReflectionHelpers
import java.util.concurrent.Executor

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], shadows = [TunerFrontendLeaseTransitionTest.NativeTuner::class])
class TunerFrontendLeaseTransitionTest {
    @Test
    fun tuneReusesSameTypeAndReturnsSatelliteLeaseBeforeTerrestrialRequest() {
        val controller = controller()
        val sdk = checkNotNull(NativeTuner.current)
        try {
            check(!controller.tuneForScan(terrestrial()).success)
            check(!controller.tuneForScan(terrestrial()).success)
            check(sdk.fullClosed == 0)
            check(!controller.tuneForScan(satellite()).success)
            check(sdk.fullClosed == 1)
            val satelliteSdk = checkNotNull(NativeTuner.current)
            check(satelliteSdk !== sdk)
            check(!controller.tuneForScan(terrestrial()).success)
            check(satelliteSdk.fullClosed == 1)
            check(
                NativeTuner.instances.flatMap { it.requested } ==
                    listOf(
                        FrontendSettings.TYPE_ISDBT,
                        FrontendSettings.TYPE_ISDBT,
                        FrontendSettings.TYPE_ISDBS,
                        FrontendSettings.TYPE_ISDBT,
                    ),
            )
        } finally {
            controller.release()
        }
    }

    @Test
    fun directSatelliteScanAlsoTracksFrontendForNextTerrestrialTune() {
        val controller = controller()
        val sdk = checkNotNull(NativeTuner.current)
        try {
            val result = controller.discoverIsdbsStreamIds(JapanIsdbScanPlan.isdbsBsBands().first())
            check(!result.success)
            check(sdk.scanned == listOf(FrontendSettings.TYPE_ISDBS))
            check(!controller.tuneForScan(terrestrial()).success)
            check(sdk.fullClosed == 1)
            check(NativeTuner.instances.flatMap { it.requested } == listOf(FrontendSettings.TYPE_ISDBT))
        } finally {
            controller.release()
        }
    }

    @Test
    fun failedTunerClosePreventsTuneAndAllowsLaterRetry() {
        val controller = controller()
        val sdk = checkNotNull(NativeTuner.current)
        try {
            check(!controller.tuneForScan(satellite()).success)
            sdk.failNextFullClose = true
            val failure = controller.tuneForScan(terrestrial())
            check(!failure.success && failure.resultCode == Tuner.RESULT_UNKNOWN_ERROR)
            check(sdk.requested == listOf(FrontendSettings.TYPE_ISDBS))
            check(!controller.tuneForScan(terrestrial()).success)
            check(sdk.fullClosed == 1)
            check(NativeTuner.instances.flatMap { it.requested } == listOf(FrontendSettings.TYPE_ISDBS, FrontendSettings.TYPE_ISDBT))
        } finally {
            controller.release()
        }
    }

    @Test
    fun failedTunerClosePreventsSatelliteDiscoveryAndReturnsFailure() {
        val controller = controller()
        val sdk = checkNotNull(NativeTuner.current)
        try {
            check(!controller.tuneForScan(terrestrial()).success)
            sdk.failNextFullClose = true
            val outcome = controller.discoverIsdbsStreamIds(JapanIsdbScanPlan.isdbsBsBands().first())
            check(!outcome.success && outcome.resultCode == Tuner.RESULT_UNKNOWN_ERROR)
            check(outcome.message.contains("frontend解放"))
            check(sdk.scanned.isEmpty())
            check(sdk.requested == listOf(FrontendSettings.TYPE_ISDBT))
        } finally {
            controller.release()
        }
    }

    private fun controller(): TunerController {
        NativeTuner.instances.clear()
        NativeTuner.current = null
        return TunerController(
            RuntimeEnvironment.getApplication(),
            "lease-integration-test",
        )
    }

    private fun terrestrial(): ScanCandidate =
        ScanCandidate(
            ChannelRecord.DELIVERY_SYSTEM_ISDB_T,
            FrequencyHz(473_142_857L),
            displayChannel = "13",
        )

    private fun satellite(): ScanCandidate = JapanIsdbScanPlan.isdbs110CsBands().first()

    @Implements(Tuner::class)
    class NativeTuner {
        @RealObject private lateinit var actual: Tuner

        val requested = mutableListOf<Int>()
        val scanned = mutableListOf<Int>()
        var closed = 0
        var fullClosed = 0
        var failNextFullClose = false
        private var heldType: Int? = null

        companion object {
            var current: NativeTuner? = null
            val instances = mutableListOf<NativeTuner>()
        }

        @Implementation(methodName = "__constructor__")
        @Suppress("UNUSED_PARAMETER")
        private fun construct(
            context: Context,
            sessionId: String?,
            useCase: Int,
        ) {
            // Shadow constructorはJavaのfield initializerを飛ばすため、
            // clearOnTuneEventListener()が使うSDK既存のlockだけをテスト側で復元する。
            ReflectionHelpers.setField(actual, "mOnTuneEventLock", Any())
            current = this
            instances += this
        }

        @Implementation
        @Suppress("UNUSED_PARAMETER")
        fun setResourceLostListener(
            executor: Executor,
            listener: Tuner.OnResourceLostListener,
        ) = Unit

        @Implementation
        fun tune(settings: FrontendSettings): Int {
            check(heldType == null || heldType == settings.type) { "Tuner SDK would reject a cross-type tune" }
            requested += settings.type
            heldType = settings.type
            return Tuner.RESULT_UNAVAILABLE
        }

        @Implementation
        // SDKの固定signatureにexecutor/callbackが必要だが、開始失敗fixtureではcallbackを発火しない。
        // 未使用引数を理由に余計なcallback状態・fake eventを保持しない。
        @Suppress("UNUSED_PARAMETER")
        fun scan(
            settings: FrontendSettings,
            scanType: Int,
            executor: Executor,
            callback: android.media.tv.tuner.frontend.ScanCallback,
        ): Int {
            check(scanType == Tuner.SCAN_TYPE_AUTO)
            check(heldType == null || heldType == settings.type) { "Tuner SDK would reject a cross-type scan" }
            scanned += settings.type
            heldType = settings.type
            return Tuner.RESULT_UNAVAILABLE
        }

        @Implementation
        fun cancelScanning(): Int = Tuner.RESULT_SUCCESS

        @Implementation
        fun closeFrontend() {
            closed++
            heldType = null
        }

        @Implementation
        fun close() {
            if (failNextFullClose) {
                failNextFullClose = false
                error("tuner close failed")
            }
            fullClosed++
            heldType = null
        }
    }
}

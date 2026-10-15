// TunerController公開選局入口を通す。Frameworkのnative/TRM接続だけをShadowで代替する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import android.content.Context
import android.media.tv.tuner.DemuxCapabilities
import android.media.tv.tuner.Tuner
import android.media.tv.tuner.TunerUtils
import android.media.tv.tuner.filter.Filter
import android.media.tv.tuner.filter.FilterCallback
import android.media.tv.tuner.filter.FilterConfiguration
import android.media.tv.tuner.filter.FilterEvent
import android.media.tv.tuner.filter.SectionEvent
import android.media.tv.tuner.frontend.FrontendInfo
import android.media.tv.tuner.frontend.FrontendSettings
import com.maleicacid.tvinput.aribsi.WellKnownSectionPid
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
import org.robolectric.shadow.api.Shadow
import org.robolectric.util.ReflectionHelpers
import java.util.concurrent.Executor
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit

@RunWith(RobolectricTestRunner::class)
@Config(
    sdk = [35],
    shadows = [TunerFrontendLeaseTransitionTest.NativeTuner::class, TunerFrontendLeaseTransitionTest.NativeFilter::class],
)
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
            check(
                NativeTuner.instances.flatMap { it.requested } ==
                    listOf(FrontendSettings.TYPE_ISDBS, FrontendSettings.TYPE_ISDBT),
            )
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


    @Test
    fun successfulRetunesOpenFreshFiltersAndDeliverSections() {
        val controller = controller()
        val received = LinkedBlockingQueue<com.maleicacid.tvinput.common.TsPid>()
        NativeTuner.successfulTunes = true
        controller.setOnSectionIngestedCallback { pid -> if (pid != null) received.offer(pid) }
        try {
            fun select(candidate: ScanCandidate): NativeTuner {
                check(controller.tuneForScan(candidate).success)
                val sdk = checkNotNull(NativeTuner.current)
                val filter = sdk.filters.first()
                deliverPat(filter)
                check(received.poll(5, TimeUnit.SECONDS) == WellKnownSectionPid.PAT)
                return sdk
            }

            val first = select(terrestrial())
            check(select(terrestrial()) === first)
            check(first.fullClosed == 0)
            val second = select(satellite())
            check(second !== first && first.fullClosed == 1)
            check(first.filters.all { Shadow.extract<NativeFilter>(it).closes == 1 })
            val third = select(terrestrial())
            check(third !== second && second.fullClosed == 1)
            check(NativeTuner.instances.size == 3)
            check(NativeTuner.instances.flatMap { it.requested } ==
                listOf(
                    FrontendSettings.TYPE_ISDBT,
                    FrontendSettings.TYPE_ISDBT,
                    FrontendSettings.TYPE_ISDBS,
                    FrontendSettings.TYPE_ISDBT,
                ))
        } finally {
            controller.release()
        }
    }

    @Test
    fun bsSourceReselectionDisposesExistingDemuxEvenForSameFrontendType() {
        val controller = controller()
        NativeTuner.successfulTunes = true
        try {
            check(controller.tuneForScan(satellite()).success)
            val oldSdk = checkNotNull(NativeTuner.current)
            check(oldSdk.filters.isNotEmpty())
            val selected = controller.prepareBsCandidateSource()
            check(selected.success)
            check(oldSdk.fullClosed == 1)
            check(oldSdk.closed == 0)
            check(oldSdk.filters.all { Shadow.extract<NativeFilter>(it).closes == 1 })
            val newSdk = checkNotNull(NativeTuner.current)
            check(newSdk !== oldSdk)
            check(newSdk.applied == listOf(FrontendSettings.TYPE_ISDBS))
            check(controller.tuneForScan(satellite()).success)
            check(newSdk.filters.isNotEmpty())
        } finally {
            controller.release()
        }
    }

    @Test
    fun successfulCloseFollowedByFailedConstructionRetriesOnNextTune() {
        val controller = controller()
        val oldSdk = checkNotNull(NativeTuner.current)
        try {
            check(!controller.tuneForScan(satellite()).success)
            NativeTuner.failNextConstruction = true
            val failed = controller.tuneForScan(terrestrial())
            check(!failed.success && failed.resultCode == Tuner.RESULT_UNKNOWN_ERROR)
            check(oldSdk.fullClosed == 1)
            check(oldSdk.requested == listOf(FrontendSettings.TYPE_ISDBS))
            check(NativeTuner.instances.size == 1)
            val retry = controller.tuneForScan(terrestrial())
            check(!retry.success && retry.resultCode == Tuner.RESULT_UNAVAILABLE)
            check(NativeTuner.instances.size == 2)
            val newSdk = checkNotNull(NativeTuner.current)
            check(newSdk !== oldSdk)
            check(newSdk.requested == listOf(FrontendSettings.TYPE_ISDBT))
        } finally {
            controller.release()
        }
    }

    private fun deliverPat(filter: Filter) {
        val payload = byteArrayOf(0x00, 0x01, 0x02)
        Shadow.extract<NativeFilter>(filter).offer(payload)
        val event =
            ReflectionHelpers.callConstructor(
                SectionEvent::class.java,
                ReflectionHelpers.ClassParameter.from(Int::class.javaPrimitiveType, 0),
                ReflectionHelpers.ClassParameter.from(Int::class.javaPrimitiveType, 0),
                ReflectionHelpers.ClassParameter.from(Int::class.javaPrimitiveType, 0),
                ReflectionHelpers.ClassParameter.from(Long::class.javaPrimitiveType, payload.size.toLong()),
            )
        ReflectionHelpers.callInstanceMethod<Unit>(
            filter,
            "onFilterEvent",
            ReflectionHelpers.ClassParameter.from(Array<FilterEvent>::class.java, arrayOf<FilterEvent>(event)),
        )
    }

    private fun controller(): TunerController {
        NativeTuner.instances.clear()
        NativeTuner.current = null
        NativeTuner.successfulTunes = false
        NativeTuner.failNextConstruction = false
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
        val filters = mutableListOf<Filter>()
        val applied = mutableListOf<Int>()
        private var heldType: Int? = null

        companion object {
            var current: NativeTuner? = null
            val instances = mutableListOf<NativeTuner>()
            var successfulTunes = false
            var failNextConstruction = false
        }

        @Implementation(methodName = "__constructor__")
        @Suppress("UNUSED_PARAMETER")
        private fun construct(
            context: Context,
            sessionId: String?,
            useCase: Int,
        ) {
            if (failNextConstruction) {
                failNextConstruction = false
                error("injected Tuner construction failure")
            }
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
            check(fullClosed == 0) { "Closed Tuner instance reused" }
            check(heldType == null || heldType == settings.type) { "Tuner SDK would reject a cross-type tune" }
            requested += settings.type
            heldType = settings.type
            return if (successfulTunes) Tuner.RESULT_SUCCESS else Tuner.RESULT_UNAVAILABLE
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
        fun getDemuxCapabilities(): DemuxCapabilities {
            val constructor = DemuxCapabilities::class.java.declaredConstructors.single()
            constructor.isAccessible = true
            return constructor.newInstance(
                1, 0, 0, 0, 32, 0, 0, 0, 0, 4096L,
                Filter.TYPE_TS, intArrayOf(Filter.TYPE_TS), intArrayOf(Filter.TYPE_TS), false,
            ) as DemuxCapabilities
        }

        @Implementation
        @Suppress("UNUSED_PARAMETER")
        fun openFilter(
            type: Int,
            subtype: Int,
            bufferSize: Long,
            executor: Executor,
            callback: FilterCallback,
        ): Filter {
            check(fullClosed == 0 && bufferSize > 0)
            val filter =
                ReflectionHelpers.callConstructor(
                    Filter::class.java,
                    ReflectionHelpers.ClassParameter.from(Long::class.javaPrimitiveType, 1L),
                )
            ReflectionHelpers.setField(filter, "mMainType", type)
            val sdkSubtype =
                ReflectionHelpers.callStaticMethod<Int>(
                    TunerUtils::class.java,
                    "getFilterSubtype",
                    ReflectionHelpers.ClassParameter.from(Int::class.javaPrimitiveType, type),
                    ReflectionHelpers.ClassParameter.from(Int::class.javaPrimitiveType, subtype),
                )
            ReflectionHelpers.setField(filter, "mSubtype", sdkSubtype)
            filter.setCallback(callback, executor)
            filters += filter
            return filter
        }

        @Implementation
        fun getAvailableFrontendInfos(): List<FrontendInfo> {
            val constructor = FrontendInfo::class.java.declaredConstructors.single()
            constructor.isAccessible = true
            val info = constructor.newInstance(
                7, FrontendSettings.TYPE_ISDBS, 1_000_000_000L, 2_000_000_000L,
                0, 0, 0L, 0, intArrayOf(), null,
            ) as FrontendInfo
            return listOf(info)
        }

        @Implementation
        fun applyFrontend(info: FrontendInfo): Int {
            check(fullClosed == 0 && heldType == null)
            heldType = info.type
            applied += info.type
            return Tuner.RESULT_SUCCESS
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

    @Implements(Filter::class)
    class NativeFilter {
        var closes = 0
        private val pending = ArrayDeque<Byte>()

        fun offer(bytes: ByteArray) = bytes.forEach(pending::addLast)

        @Implementation
        @Suppress("UNUSED_PARAMETER")
        fun nativeConfigureFilter(
            type: Int,
            subtype: Int,
            configuration: FilterConfiguration,
        ): Int = Tuner.RESULT_SUCCESS

        @Implementation
        fun nativeStartFilter(): Int = Tuner.RESULT_SUCCESS

        @Implementation
        fun nativeStopFilter(): Int = Tuner.RESULT_SUCCESS

        @Implementation
        fun nativeRead(
            buffer: ByteArray,
            offset: Long,
            size: Long,
        ): Int {
            val count = minOf(size.toInt(), pending.size)
            repeat(count) { index -> buffer[offset.toInt() + index] = pending.removeFirst() }
            return count
        }

        @Implementation
        fun nativeClose(): Int {
            closes++
            pending.clear()
            return Tuner.RESULT_SUCCESS
        }
    }
}

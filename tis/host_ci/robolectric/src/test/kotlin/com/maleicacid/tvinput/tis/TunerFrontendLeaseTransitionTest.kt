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
import android.net.Uri
import com.maleicacid.tvinput.aribsi.AribSiEngine
import com.maleicacid.tvinput.aribsi.SectionIngestController
import com.maleicacid.tvinput.aribsi.SectionTransportLossSnapshot
import com.maleicacid.tvinput.aribsi.SiIngestResult
import com.maleicacid.tvinput.aribsi.SiStatus
import com.maleicacid.tvinput.aribsi.WellKnownSectionPid
import com.maleicacid.tvinput.common.FrequencyHz
import com.maleicacid.tvinput.common.TsPid
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
    instrumentedPackages = ["com.maleicacid.tvinput.aribsi"],
    shadows = [
        TunerFrontendLeaseTransitionTest.NativeTuner::class,
        TunerFrontendLeaseTransitionTest.NativeFilter::class,
        TunerFrontendLeaseTransitionTest.SiEngineBoundary::class,
    ],
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
    fun clearingTuneListenerPreservesFrameworkSdkCleanup() {
        val controller = controller()
        val sdk = checkNotNull(NativeTuner.current)
        try {
            val tuner = ReflectionHelpers.getField<Tuner>(controller, "tuner")
            tuner.setOnTuneEventListener(Executor { task -> task.run() }) { }
            check(ReflectionHelpers.getField<Any?>(tuner, "mOnTuneEventListener") != null)
            check(ReflectionHelpers.getField<Any?>(tuner, "mOnTuneEventExecutor") != null)

            val before = sdk.clearTuneListenerCalls
            check(!controller.tuneForScan(terrestrial()).success)
            check(sdk.clearTuneListenerCalls > before)
            check(ReflectionHelpers.getField<Any?>(tuner, "mOnTuneEventListener") == null)
            check(ReflectionHelpers.getField<Any?>(tuner, "mOnTuneEventExecutor") == null)
        } finally {
            controller.release()
        }
    }

    @Test
    fun failedOneShotTunerCloseCannotBeMisreadAsSuccessfulRetry() {
        val controller = controller()
        val sdk = checkNotNull(NativeTuner.current)
        try {
            check(!controller.tuneForScan(satellite()).success)
            sdk.failNextFullClose = true
            val first = controller.tuneForScan(terrestrial())
            check(!first.success && first.resultCode == Tuner.RESULT_UNKNOWN_ERROR)
            check(sdk.oneShotClosed && sdk.fullClosed == 0 && sdk.closeCalls == 1)
            val again = controller.tuneForScan(terrestrial())
            check(!again.success && again.resultCode == Tuner.RESULT_UNKNOWN_ERROR)
            check(again.message.contains("資源解放未確定"))
            check(sdk.closeCalls == 1 && sdk.requested == listOf(FrontendSettings.TYPE_ISDBS))
            check(NativeTuner.instances.size == 1)
            val bs = controller.prepareBsCandidateSource()
            check(!bs.success && bs.resultCode == Tuner.RESULT_UNKNOWN_ERROR)
            check(sdk.closeCalls == 1 && sdk.applied.isEmpty())
            // terminal releaseもAOSPのclose再呼出しno-opを成功判定せず、実行器のみ閉じる。
            check(runCatching { controller.release() }.isFailure)
            check(runCatching { controller.release() }.isFailure)
            check(sdk.closeCalls == 1 && sdk.fullClosed == 0)
        } finally {
            runCatching { controller.release() }
        }
    }

    @Test
    fun failedCloseRejectsInvalidLiveChannelWithoutUsingOldSdk() {
        val controller = controller()
        val sdk = checkNotNull(NativeTuner.current)
        try {
            check(!controller.tuneForScan(satellite()).success)
            sdk.failNextFullClose = true
            check(!controller.tuneForScan(terrestrial()).success)
            val beforeClose = sdk.closeCalls
            val beforeClear = sdk.clearTuneListenerCalls
            val result = controller.tuneForLive(Uri.parse("content://android.media.tv/channel/999999"))
            check(!result.success && result.resultCode == Tuner.RESULT_UNKNOWN_ERROR)
            check(result.message.contains("資源解放未確定"))
            check(sdk.closeCalls == beforeClose && sdk.clearTuneListenerCalls == beforeClear)
            check(sdk.requested == listOf(FrontendSettings.TYPE_ISDBS))
            check(NativeTuner.instances.size == 1)
        } finally {
            runCatching { controller.release() }
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
            check(sdk.oneShotClosed && sdk.closeCalls == 1)
            val retry = controller.discoverIsdbsStreamIds(JapanIsdbScanPlan.isdbsBsBands().first())
            check(!retry.success && retry.resultCode == Tuner.RESULT_UNKNOWN_ERROR)
            check(sdk.closeCalls == 1 && sdk.scanned.isEmpty())
        } finally {
            runCatching { controller.release() }
        }
    }

    @Test
    fun sectionOverflowCountsOnlyCurrentSdkFilterAndTuneGeneration() {
        val controller = controller()
        val engine = AribSiEngine(RuntimeEnvironment.getApplication())
        val ingest = SectionIngestController(engine)
        controller.setSectionIngestController(ingest)
        NativeTuner.successfulTunes = true
        try {
            check(controller.tuneForScan(terrestrial()).success)
            val sdk = checkNotNull(NativeTuner.current)
            val pat = sdk.filters.first()
            val other = sdk.filters.first { it !== pat }
            val callback = sdk.filterCallbacks.getValue(pat)
            callback.onFilterStatusChanged(pat, Filter.STATUS_DATA_READY)
            callback.onFilterStatusChanged(pat, Filter.STATUS_HIGH_WATER)
            controller.currentGeneration()
            check(ingest.filterOverflowCount == 0)

            // 同世代でも別Filterの実体を渡したstatusは計上しない。
            callback.onFilterStatusChanged(other, Filter.STATUS_OVERFLOW)
            controller.currentGeneration()
            check(ingest.filterOverflowCount == 0)

            callback.onFilterStatusChanged(pat, Filter.STATUS_OVERFLOW)
            callback.onFilterStatusChanged(pat, Filter.STATUS_OVERFLOW or Filter.STATUS_DATA_READY)
            controller.currentGeneration()
            check(ingest.filterOverflowCount == 2)
            check("filterOverflow=2" in ingest.diagnosticSummary())
            check(ingest.inputDeliveryLossCount == 0)

            // 同世代のclose後に遅延したcallbackも失効させる。
            controller.closeSectionFilter(WellKnownSectionPid.PAT)
            callback.onFilterStatusChanged(pat, Filter.STATUS_OVERFLOW)
            controller.currentGeneration()
            check(ingest.filterOverflowCount == 2)

            val priorCount = sdk.filters.size
            check(controller.tuneForScan(terrestrial()).success)
            callback.onFilterStatusChanged(pat, Filter.STATUS_OVERFLOW)
            val nextPat = sdk.filters[priorCount]
            sdk.filterCallbacks.getValue(nextPat).onFilterStatusChanged(nextPat, Filter.STATUS_OVERFLOW)
            controller.currentGeneration()
            check(ingest.filterOverflowCount == 3)
        } finally {
            controller.release()
            engine.close()
        }
    }

    @Test
    fun oldSectionCallbacksCannotPolluteNewCandidateBetweenParserResetAndRetune() {
        val controller = controller()
        val engine = AribSiEngine(RuntimeEnvironment.getApplication())
        val ingest = SectionIngestController(engine)
        controller.setSectionIngestController(ingest)
        NativeTuner.successfulTunes = true
        try {
            check(controller.tuneForScan(terrestrial()).success)
            val sdk = checkNotNull(NativeTuner.current)
            val stalePat = sdk.filters.first()
            val staleCallback = sdk.filterCallbacks.getValue(stalePat)
            var candidateBaseline: SectionTransportLossSnapshot? = null
            val retune =
                controller.tuneForScan(terrestrial()) {
                    // resetは旧Filterの解放後、新Filterの生成前に実行する。
                    check(Shadow.extract<NativeFilter>(stalePat).closes == 1)
                    check(sdk.filters.all { Shadow.extract<NativeFilter>(it).closes == 1 })
                    ingest.resetForTune()
                    candidateBaseline = ingest.transportLossSnapshot()
                    // parser resetと次tuneの間に入った旧世代callbackを再現する。
                    staleCallback.onFilterStatusChanged(stalePat, Filter.STATUS_OVERFLOW)
                    deliverPat(stalePat)
                }
            check(retune.success)
            controller.currentGeneration()
            check(ingest.transportLossSnapshot() == candidateBaseline)
            check(ingest.diagnostics().isEmpty())
            val currentSdk = checkNotNull(NativeTuner.current)
            val currentPat = currentSdk.filters.last { Shadow.extract<NativeFilter>(it).closes == 0 }
            currentSdk.filterCallbacks.getValue(currentPat).onFilterStatusChanged(currentPat, Filter.STATUS_OVERFLOW)
            controller.currentGeneration()
            check(ingest.filterOverflowCount == 1)
            deliverPat(currentPat)
            check(ingest.diagnostics().single { it.pid == WellKnownSectionPid.PAT }.acceptedCount == 1)
        } finally {
            controller.release()
            engine.close()
        }
    }

    @Test
    fun rejectedSectionReadCountsCurrentFilterButNotClosedFilterOfSameGeneration() {
        val controller = controller()
        val engine = AribSiEngine(RuntimeEnvironment.getApplication())
        val ingest = SectionIngestController(engine)
        controller.setSectionIngestController(ingest)
        NativeTuner.successfulTunes = true
        try {
            check(controller.tuneForScan(terrestrial()).success)
            val sdk = checkNotNull(NativeTuner.current)
            val oldPat = sdk.filters.first()
            val owner = ReflectionHelpers.getField<ControllerSerialExecutor>(controller, "sectionExecutor")
            val slotsField = PrioritySerialExecutor::class.java.getDeclaredField("pendingDataSlots")
            slotsField.isAccessible = true
            val slots = slotsField.get(owner) as java.util.concurrent.Semaphore
            val permits = slots.availablePermits()
            check(permits > 0 && slots.tryAcquire(permits))
            try {
                val deliver =
                    TunerController::class.java.declaredMethods.single { it.name.startsWith("deliverSectionRead") }
                deliver.isAccessible = true
                val generation = controller.currentGeneration()
                val section = byteArrayOf(0x00, 0x00, 0x00)
                controller.closeSectionFilter(WellKnownSectionPid.PAT)
                deliver.invoke(controller, oldPat, WellKnownSectionPid.PAT.value, generation, section, section.size)
                controller.currentGeneration()
                check(ingest.inputDeliveryLossCount == 0)

                val nextPat = sdk.filters.first { it !== oldPat && sdk.filterCallbacks.containsKey(it) }
                // 現行PATが必要なので、closeされたPIDを同じgenerationで再登録する。
                val newPat = controller.openSectionFilter(WellKnownSectionPid.PAT)
                check(newPat.isOpen)
                val activePat = sdk.filters.last()
                check(activePat !== oldPat && activePat !== nextPat)
                deliver.invoke(controller, activePat, WellKnownSectionPid.PAT.value, generation, section, section.size)
                controller.currentGeneration()
                check(ingest.inputDeliveryLossCount == 1)
            } finally {
                slots.release(permits)
            }
        } finally {
            controller.release()
            engine.close()
        }
    }

    @Test
    fun successfulRetunesOpenFreshFiltersAndDeliverSections() {
        val controller = controller()
        val received = LinkedBlockingQueue<TsPid>()
        val engine = AribSiEngine(RuntimeEnvironment.getApplication())
        val ingest = SectionIngestController(engine)
        controller.setSectionIngestController(ingest)
        NativeTuner.successfulTunes = true
        controller.setOnSectionIngestedCallback { pid -> if (pid != null) received.offer(pid) }
        try {
            var previous: Filter? = null
            var receivedCount = 0

            fun select(candidate: ScanCandidate): NativeTuner {
                check(controller.tuneForScan(candidate).success)
                val sdk = checkNotNull(NativeTuner.current)
                // 旧generationのFilter callbackは実Filter identity照合で棄却される。
                previous?.let { stale ->
                    check(Shadow.extract<NativeFilter>(stale).closes == 1)
                    deliverPat(stale)
                    check(received.poll(100, TimeUnit.MILLISECONDS) == null)
                }
                val filter = sdk.filters.first { Shadow.extract<NativeFilter>(it).closes == 0 }
                deliverPat(filter)
                check(received.poll(5, TimeUnit.SECONDS) == WellKnownSectionPid.PAT)
                receivedCount++
                check(ingest.diagnostics().single { it.pid == WellKnownSectionPid.PAT }.acceptedCount == receivedCount)
                previous = filter
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
        // 既存native PAT受理試験と同じ有効section。section_length=13、CRC-32/MPEG=4521f9b6。
        val payload = VALID_PAT
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
        var oneShotClosed = false
        var closeCalls = 0
        var clearTuneListenerCalls = 0
        val filters = mutableListOf<Filter>()
        val filterCallbacks = mutableMapOf<Filter, FilterCallback>()
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
                error("Tuner生成失敗を注入")
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
        fun clearOnTuneEventListener() {
            clearTuneListenerCalls++
            // Java側のリスナ解除は元のSDK実装で行い、監視による振舞いの変更を避ける。
            Shadow.directlyOn<Void, Tuner>(actual, Tuner::class.java, "clearOnTuneEventListener")
        }

        @Implementation
        fun tune(settings: FrontendSettings): Int {
            check(fullClosed == 0) { "閉鎖済みTunerを再利用しました" }
            check(heldType == null || heldType == settings.type) { "SDKでは異種frontendを同一Tunerで選局できません" }
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
            check(heldType == null || heldType == settings.type) { "SDKでは異種frontendを同一Tunerで探索できません" }
            scanned += settings.type
            heldType = settings.type
            return Tuner.RESULT_UNAVAILABLE
        }

        @Implementation
        fun getDemuxCapabilities(): DemuxCapabilities {
            val constructor = DemuxCapabilities::class.java.declaredConstructors.single { it.parameterCount == 14 }
            constructor.isAccessible = true
            return constructor.newInstance(
                1,
                0,
                0,
                0,
                32,
                0,
                0,
                0,
                0,
                4096L,
                Filter.TYPE_TS,
                intArrayOf(Filter.TYPE_TS),
                intArrayOf(Filter.TYPE_TS),
                false,
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
            filterCallbacks[filter] = callback
            return filter
        }

        @Implementation
        fun getAvailableFrontendInfos(): List<FrontendInfo> {
            val constructor = FrontendInfo::class.java.declaredConstructors.single { it.parameterCount == 10 }
            constructor.isAccessible = true
            val info =
                constructor.newInstance(
                    7,
                    FrontendSettings.TYPE_ISDBS,
                    1_000_000_000L,
                    2_000_000_000L,
                    0,
                    0,
                    0L,
                    0,
                    intArrayOf(),
                    null,
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
            closeCalls++
            if (oneShotClosed) return // Android 15: 2回目以降のcloseはno-op
            oneShotClosed = true
            if (failNextFullClose) {
                failNextFullClose = false
                error("Tunerの解放途中で失敗しました")
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

    // SectionIngestController本体を実行する。Native SI parserだけは既存host回帰と分離。
    @Implements(AribSiEngine::class)
    class SiEngineBoundary {
        @Implementation(methodName = "__constructor__")
        @Suppress("UNUSED_PARAMETER")
        fun construct(context: Context) = Unit

        @Implementation
        fun ingestSection(
            pid: TsPid,
            section: ByteArray,
        ): SiIngestResult {
            check(pid == WellKnownSectionPid.PAT && section.contentEquals(VALID_PAT))
            return SiIngestResult(pid, SiStatus.OK)
        }

        @Implementation
        fun close() = Unit
    }

    companion object {
        // NativeAribSiParserCasDiscoveryTest.PAT_BODYと同一の16-byte PAT/CRC-32/MPEG検査ベクトル。
        private val VALID_PAT =
            byteArrayOf(
                0x00,
                0xb0.toByte(),
                0x0d,
                0x00,
                0x11,
                0xc1.toByte(),
                0x00,
                0x00,
                0x00,
                0x01,
                0xe1.toByte(),
                0x00,
                0x45,
                0x21,
                0xf9.toByte(),
                0xb6.toByte(),
            )
    }
}

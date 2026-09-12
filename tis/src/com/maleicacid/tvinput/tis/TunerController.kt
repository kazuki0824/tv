package com.maleicacid.tvinput.tis

import android.content.Context
import android.media.tv.TvContract
import android.media.tv.TvInputService
import android.media.tv.tuner.Tuner
import android.media.tv.tuner.filter.Filter
import android.media.tv.tuner.filter.FilterCallback
import android.media.tv.tuner.filter.FilterEvent
import android.media.tv.tuner.filter.SectionEvent
import android.media.tv.tuner.filter.SectionSettingsWithSectionBits
import android.media.tv.tuner.filter.TsFilterConfiguration
import android.media.tv.tuner.frontend.Atsc3PlpInfo
import android.media.tv.tuner.frontend.FrontendInfo
import android.media.tv.tuner.frontend.FrontendSettings
import android.media.tv.tuner.frontend.FrontendStatus
import android.media.tv.tuner.frontend.IsdbsFrontendSettings
import android.media.tv.tuner.frontend.IsdbtFrontendSettings
import android.media.tv.tuner.frontend.OnTuneEventListener
import android.media.tv.tuner.frontend.ScanCallback
import android.net.Uri
import android.util.Log
import android.view.Surface
import com.maleicacid.tvinput.aribsi.AribElementaryStream
import com.maleicacid.tvinput.aribsi.CaMetadata
import com.maleicacid.tvinput.aribsi.NativeAribCaptionFactParser
import com.maleicacid.tvinput.aribsi.ProviderDataBridge
import com.maleicacid.tvinput.aribsi.SectionIngestController
import com.maleicacid.tvinput.aribsi.WellKnownSectionPid
import com.maleicacid.tvinput.common.CaptionTimestamp
import com.maleicacid.tvinput.common.FrequencyHz
import com.maleicacid.tvinput.common.LogTags
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.StreamSelector
import com.maleicacid.tvinput.common.StreamSelectorType
import com.maleicacid.tvinput.common.TsPid
import com.maleicacid.tvinput.db.ChannelRecord
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.CountDownLatch
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.TimeUnit

/** ライブ視聴、スキャン、録画向けの Tuner 制御層。 */
@Suppress(
    // Android Tuner資源の単一ownerとして状態遷移を直列化するため、機械的な責務分割は行わない。
    "LargeClass",
    "TooManyFunctions",
)
class TunerController(
    private val context: Context,
    private val inputId: String,
    private val useCase: Int = TvInputService.PRIORITY_HINT_USE_CASE_TYPE_LIVE,
    private val sessionId: String? = null,
    private val sessionContext: Context? = null,
) : AutoCloseable {
    interface SectionFilterHandle : AutoCloseable {
        val pid: TsPid
        val isOpen: Boolean
    }

    data class ResolvedChannel(
        val uri: Uri?,
        val inputId: String,
        val serviceKey: ServiceKey,
        val serviceType: Int,
        val displayName: String,
        val displayNumber: String,
        val deliverySystem: String,
        val frequencyHz: FrequencyHz,
        val streamSelector: StreamSelector,
        val physicalChannel: Int?,
        val backendHint: String?,
        val satelliteBand: String? = null,
    )

    data class TuneOutcome(
        val success: Boolean,
        val resultCode: Int,
        val channel: ResolvedChannel?,
        val generation: Long,
        val message: String = "",
    )

    data class AvStreamSelection(
        val serviceKey: ServiceKey,
        val pcrPid: TsPid?,
        val video: AribElementaryStream?,
        val audio: AribElementaryStream?,
        val subtitle: AribElementaryStream? = null,
        val subtitleLanguageId: Int? = null,
        val superimpose: AribElementaryStream? = null,
        val audioComponentType: Int? = null,
        val dualMonoPresentation: PlaybackPipeline.DualMonoPresentation = PlaybackPipeline.DualMonoPresentation.MAIN,
    )

    data class TisTrack(
        val id: String,
        val type: Int,
        val pid: TsPid,
        val streamType: Int,
        val componentTag: Int?,
        val componentType: Int?,
        val language: String?,
        val dataComponentId: Int? = null,
        val captionServiceKind: String? = null,
        val captionLanguageId: Int? = null,
        val automaticPresentationOnReception: Boolean? = null,
    )

    private data class SectionFilterArtifact(
        val filter: Filter,
        var started: Boolean = false,
    )

    @Suppress("TooGenericExceptionCaught", "MaxLineLength")
    private inner class TunerSectionFilterHandle(
        override val pid: TsPid,
        private val artifacts: MutableList<SectionFilterArtifact>,
        private val generation: Long,
    ) : SectionFilterHandle {
        private var closing = false
        val isClosed: Boolean get() = artifacts.isEmpty()
        override val isOpen: Boolean get() = !closing && artifacts.isNotEmpty() && artifacts.all { it.started }

        override fun close(): Unit =
            callOnController {
                closing = true
                val remainingSources = sectionFilters[pid].orEmpty().filterNot { source -> artifacts.any { it.filter === source } }
                if (remainingSources.isEmpty()) sectionFilters.remove(pid) else sectionFilters[pid] = remainingSources
                var failure: RuntimeException? = null

                fun retain(error: RuntimeException) {
                    val primary = failure
                    if (primary == null) {
                        failure = error
                    } else if (primary !== error) {
                        primary.addSuppressed(error)
                    }
                }
                val iterator = artifacts.iterator()
                while (iterator.hasNext()) {
                    val artifact = iterator.next()
                    if (artifact.started) {
                        try {
                            val result = artifact.filter.stop()
                            check(result == Tuner.RESULT_SUCCESS) { "section filter stopに失敗しました pid=$pid result=$result" }
                            artifact.started = false
                        } catch (error: RuntimeException) {
                            retain(error)
                        }
                    }
                    try {
                        artifact.filter.close()
                        iterator.remove()
                    } catch (error: RuntimeException) {
                        retain(error)
                    }
                }
                failure?.let { throw it }
            }

        override fun toString(): String =
            "TunerSectionFilterHandle(pid=$pid, generation=$generation, filters=${artifacts.size}, closing=$closing)"
    }

    private inner class UnavailableSectionFilterHandle(
        override val pid: TsPid,
        private val reason: String,
    ) : SectionFilterHandle {
        override val isOpen: Boolean get() = false

        override fun close() = Unit

        override fun toString(): String = "UnavailableSectionFilterHandle(pid=$pid, reason=$reason)"
    }

    private val sectionExecutor: ExecutorService =
        Executors.newSingleThreadExecutor { runnable ->
            Thread(runnable, "maleicacid-tis-controller-$inputId").apply { isDaemon = true }
        }

    @Volatile private var released = false

    private fun <T> callOnController(block: () -> T): T {
        if (Thread.currentThread().name.startsWith("maleicacid-tis-controller-$inputId")) return block()
        check(!released) { "TunerController は解放済みです inputId=$inputId" }
        return try {
            sectionExecutor.submit<T> { block() }.get()
        } catch (e: RejectedExecutionException) {
            throw IllegalStateException("TunerController executor は停止済みです inputId=$inputId", e)
        }
    }

    private val sectionFilterHandles = LinkedHashMap<TsPid, SectionFilterHandle>()

    // 配送許可sourceだけを保持する。closing中の資源所有はsectionFilterHandlesに残る。
    private val sectionFilters = LinkedHashMap<TsPid, List<Filter>>()
    private val dynamicPmtPids = linkedSetOf<TsPid>()
    private val dynamicEcmPids = linkedSetOf<TsPid>()
    private val dynamicEmmPids = linkedSetOf<TsPid>()
    private val captionLanguagesByPid = ConcurrentHashMap<TsPid, List<NativeAribCaptionFactParser.Language>>()
    private val captionFactParsers = ConcurrentHashMap<TsPid, NativeAribCaptionFactParser>()
    private val superimposeTimingByPid = ConcurrentHashMap<TsPid, Int>()

    @Volatile private var latestBroadcastClockAuthority: AribBroadcastClock.AuthoritySample? = null
    private var sectionIngestController: SectionIngestController? = null
    private var casController: CasController? = null
    private var onSectionIngestedCallback: (() -> Unit)? = null
    private var onTunerResourceLostCallback: ((Long) -> Unit)? = null
    private var onTuneEventCallback: ((Long, Int) -> Unit)? = null
    private var onBroadcastClockUpdatedCallback: (() -> Unit)? = null
    private val tvInputSessionId: String? = normalizedTvInputSessionId(sessionId)
    private var tuner: Tuner? = createTuner()
    private var currentTune: ResolvedChannel? = null
    private var tuneAccepted = false
    private var tuneGeneration: Long = 0L
    private var streamIdDiscovery: StreamIdDiscoveryOperation? = null
    private val sectionShortReadCounters = linkedMapOf<TsPid, Int>()
    private val sectionReadErrorCounters = linkedMapOf<TsPid, Int>()
    private val sectionMalformedCounters = linkedMapOf<TsPid, Int>()
    private val sectionOversizedCounters = linkedMapOf<TsPid, Int>()
    private val playbackPipeline = PlaybackPipeline(inputId, tvInputSessionId, sessionContext)

    @Suppress("TooGenericExceptionCaught", "MaxLineLength")
    private fun createTuner(): Tuner? {
        var created: Tuner? = null
        return try {
            created = Tuner(context, tvInputSessionId, useCase)
            created.setResourceLostListener(sectionExecutor) { callbackTuner ->
                if (callbackTuner === tuner && !released) handleTunerResourceLostOnController()
            }
            created
        } catch (error: RuntimeException) {
            runCatching { created?.close() }.exceptionOrNull()?.let { cleanup ->
                if (cleanup !== error) error.addSuppressed(cleanup)
            }
            Log.w(LogTags.TIS, "Tuner を利用できません inputId=$inputId tvInputSessionId=$tvInputSessionId useCase=$useCase", error)
            null
        }
    }

    @Suppress("MaxLineLength")
    fun setSectionIngestController(controller: SectionIngestController?) = callOnController { sectionIngestController = controller }

    fun setCasController(controller: CasController?) =
        callOnController {
            casController = controller
        }

    @Suppress("MaxLineLength")
    fun setOnSectionIngestedCallback(callback: (() -> Unit)?) = callOnController { onSectionIngestedCallback = callback }

    @Suppress("MaxLineLength")
    fun setOnTunerResourceLostCallback(callback: ((Long) -> Unit)?) = callOnController { onTunerResourceLostCallback = callback }

    @Suppress("MaxLineLength")
    fun setOnTuneEventCallback(callback: ((Long, Int) -> Unit)?) = callOnController { onTuneEventCallback = callback }

    @Suppress("MaxLineLength")
    fun setOnBroadcastClockUpdatedCallback(callback: (() -> Unit)?) = callOnController { onBroadcastClockUpdatedCallback = callback }

    fun setPlaybackCallbacks(
        onVideoAvailable: (Long) -> Unit,
        onVideoUnavailable: (PlaybackPipeline.PlaybackUnavailable) -> Unit,
    ) {
        playbackPipeline.setCallbacks(onVideoAvailable, onVideoUnavailable)
    }

    fun setOnVideoFormatDiscoveredCallback(callback: (Long, PlaybackPipeline.VideoFormatInfo) -> Unit) {
        playbackPipeline.setOnVideoFormatDiscoveredCallback(callback)
    }

    fun setOnPlaybackGenerationRestartedCallback(callback: (PlaybackPipeline.PlaybackGenerationRestart) -> Unit) {
        playbackPipeline.setOnPlaybackGenerationRestartedCallback(callback)
    }

    @Suppress("SpreadOperator", "TooGenericExceptionCaught", "MaxLineLength")
    private fun handleTunerResourceLostOnController() {
        val lostGeneration = streamIdDiscovery?.generation ?: tuneGeneration
        try {
            completeResourceLoss(
                invalidate = {
                    if (!tuneAccepted && streamIdDiscovery?.acceptsResourceLoss != true) {
                        false
                    } else {
                        streamIdDiscovery?.loseResources()
                        invalidateTuneOnController()
                        true
                    }
                },
                cleanup = {
                    SectionFilterPolicy.completeCleanup(
                        { playbackPipeline.stop() },
                        { cancelStreamIdDiscoveryOnController() },
                        { closeSectionFiltersOnController() },
                        { casController?.clearForResourceLoss() },
                        {
                            SectionFilterPolicy.completeCleanup(
                                *captionFactParsers.entries
                                    .map { (pid, parser) ->
                                        {
                                            parser.close()
                                            captionFactParsers.remove(pid, parser)
                                            Unit
                                        }
                                    }.toTypedArray(),
                            )
                        },
                    )
                },
                notifyLost = { onTunerResourceLostCallback?.invoke(lostGeneration) },
            )
        } catch (failure: Exception) {
            // callback配送後にprimary/suppressedを診断へ残し、controller executorを維持する。
            Log.w(LogTags.TIS, "resource-lost cleanupに失敗しました inputId=$inputId generation=$lostGeneration", failure)
        }
    }

    private fun armTuneEventListener(
        tunerInstance: Tuner,
        generation: Long,
    ): Boolean {
        if (onTuneEventCallback == null) return true
        return runCatching {
            tunerInstance.setOnTuneEventListener(sectionExecutor) { event ->
                if (tunerInstance === tuner && !released) handleTuneEventOnController(generation, event)
            }
        }.onFailure { error ->
            Log.w(LogTags.TIS, "frontend tune event listener 登録に失敗しました inputId=$inputId generation=$generation", error)
        }.isSuccess
    }

    private fun handleTuneEventOnController(
        generation: Long,
        event: Int,
    ) {
        if (!tuneAccepted || generation != tuneGeneration || currentTune == null) return
        when (event) {
            OnTuneEventListener.SIGNAL_NO_SIGNAL, OnTuneEventListener.SIGNAL_LOST_LOCK -> playbackPipeline.stop()
            OnTuneEventListener.SIGNAL_LOCKED -> Unit
            else -> return
        }
        onTuneEventCallback?.invoke(generation, event)
    }

    fun setSurface(surface: Surface?) {
        playbackPipeline.setSurface(surface)
        if (surface == null) Log.d(LogTags.TIS, "Surface が解除されました inputId=$inputId sessionId=$tvInputSessionId")
    }

    fun setStreamVolume(volume: Float) {
        playbackPipeline.setVolume(volume)
    }

    fun tuneForLive(channelUri: Uri): TuneOutcome = callOnController { tuneForLiveOnController(channelUri) }

    private fun tuneForLiveOnController(channelUri: Uri): TuneOutcome {
        val resolved =
            resolveChannel(channelUri).getOrElse { e ->
                resetBeforeTune()
                Log.w(LogTags.TIS, "channel 解決に失敗しました inputId=$inputId uri=$channelUri", e)
                return TuneOutcome(false, Tuner.RESULT_INVALID_ARGUMENT, null, tuneGeneration, e.message.orEmpty())
            }
        return tuneResolvedChannel(resolved, startPlayback = true)
    }

    internal data class BsFrontendSelectionResult(
        val source: BsCandidateSource?,
        val resultCode: Int,
        val message: String = "",
    ) {
        val success: Boolean get() = source != null && resultCode == Tuner.RESULT_SUCCESS
    }

    internal fun prepareBsCandidateSource(): BsFrontendSelectionResult =
        callOnController {
            prepareBsCandidateSourceOnController()
        }

    @Suppress("LongMethod", "ReturnCount")
    private fun prepareBsCandidateSourceOnController(): BsFrontendSelectionResult {
        resetBeforeTune()
        val tunerInstance =
            tuner
                ?: return BsFrontendSelectionResult(null, Tuner.RESULT_UNAVAILABLE, "Tunerを利用できません")
        val closeFailure = runCatching { tunerInstance.closeFrontend() }.exceptionOrNull()
        if (closeFailure != null) {
            return BsFrontendSelectionResult(
                null,
                Tuner.RESULT_UNKNOWN_ERROR,
                "既存frontendの解放に失敗しました: ${closeFailure.message}",
            )
        }
        val infos =
            runCatching { tunerInstance.availableFrontendInfos.orEmpty() }.getOrElse { error ->
                return BsFrontendSelectionResult(
                    null,
                    Tuner.RESULT_UNKNOWN_ERROR,
                    "利用可能frontend一覧の取得に失敗しました: ${error.message}",
                )
            }
        val capabilities =
            infos.map { info ->
                BsFrontendCapability(
                    frontendId = info.id,
                    isIsdbs = info.type == FrontendSettings.TYPE_ISDBS,
                    supportsStreamIdList =
                        info.statusCapabilities.any {
                            it == FrontendStatus.FRONTEND_STATUS_TYPE_STREAM_IDS
                        },
                )
            }
        val byId = infos.associateBy(FrontendInfo::getId)
        val ordered = BsFrontendSelectionPolicy.orderedCandidates(capabilities)
        if (ordered.isEmpty()) {
            return BsFrontendSelectionResult(null, Tuner.RESULT_UNAVAILABLE, "ISDB-S frontendがありません")
        }
        for (candidate in ordered) {
            val info =
                byId[candidate.frontendId]
                    ?: return BsFrontendSelectionResult(
                        null,
                        Tuner.RESULT_UNKNOWN_ERROR,
                        "frontend一覧と選択候補が一致しません id=${candidate.frontendId}",
                    )
            val result =
                runCatching { tunerInstance.applyFrontend(info) }.getOrElse { error ->
                    return BsFrontendSelectionResult(
                        null,
                        Tuner.RESULT_UNKNOWN_ERROR,
                        "frontend適用中に例外が発生しました id=${candidate.frontendId}: ${error.message}",
                    )
                }
            if (result == Tuner.RESULT_UNAVAILABLE) continue
            if (result != Tuner.RESULT_SUCCESS) {
                return BsFrontendSelectionResult(
                    null,
                    result,
                    "frontend適用に失敗しました id=${candidate.frontendId} result=$result",
                )
            }
            val source = BsFrontendSelectionPolicy.sourceFor(candidate)
            return BsFrontendSelectionResult(source, Tuner.RESULT_SUCCESS)
        }
        return BsFrontendSelectionResult(
            null,
            Tuner.RESULT_UNAVAILABLE,
            "利用可能なISDB-S frontendを確保できません",
        )
    }

    data class StreamIdDiscoveryResult(
        val success: Boolean,
        val streamIds: Set<Int>,
        val resultCode: Int,
        val message: String = "",
        val generation: Long? = null,
        val resourceLost: Boolean = false,
    ) {
        // runtime errorはfrontend capabilityを表さない。成功した現在のscan報告だけを採用する。
        fun candidatesFor(seed: ScanCandidate): List<ScanCandidate> =
            if (success) JapanIsdbScanPlan.explicitBsCandidatesFromScan(seed, streamIds) else emptyList()
    }

    @Suppress("TooGenericExceptionCaught", "MaxLineLength")
    fun discoverIsdbsStreamIds(
        seed: ScanCandidate,
        timeoutMs: Long = BS_STREAM_ID_SCAN_TIMEOUT_MS,
    ): StreamIdDiscoveryResult {
        check(!Thread.currentThread().name.startsWith("maleicacid-tis-controller-$inputId")) {
            "BS探索の待機はcontroller executor外で実行する必要があります"
        }
        val operation = callOnController { startStreamIdDiscoveryOnController(seed) }
        val completed =
            try {
                operation.await(timeoutMs)
            } catch (failure: InterruptedException) {
                try {
                    callOnController { if (streamIdDiscovery === operation) cancelStreamIdDiscoveryOnController() }
                } catch (cleanup: Exception) {
                    if (cleanup !== failure) failure.addSuppressed(cleanup)
                } finally {
                    Thread.currentThread().interrupt()
                }
                throw failure
            }
        return callOnController {
            operation.resultWithCleanup(
                completed,
                cleanup = { if (streamIdDiscovery === operation) cancelStreamIdDiscoveryOnController() },
                diagnose = { Log.w(LogTags.TIS, "BS探索失敗後のscan解放を再試行まで保持します", it) },
            )
        }
    }

    private fun startStreamIdDiscoveryOnController(seed: ScanCandidate): StreamIdDiscoveryOperation {
        require(seed.kind == ScanCandidateKind.ISDB_S_BS && seed.streamSelector == StreamSelector.NONE)
        resetBeforeTune()
        val operation = StreamIdDiscoveryOperation(++tuneGeneration)
        val tunerInstance = tuner
        if (tunerInstance == null) {
            operation.startFailed(Tuner.RESULT_UNAVAILABLE, "Tunerを利用できません")
            return operation
        }
        streamIdDiscovery = operation
        val settings = IsdbsFrontendSettings.builder().setFrequencyLong(seed.frequencyHz.value).build()
        val callback =
            object : ScanCallback {
                override fun onLocked() {
                    if (streamIdDiscovery === operation) {
                        operation.continueAfterLock {
                            tunerInstance.scan(settings, Tuner.SCAN_TYPE_AUTO, sectionExecutor, this)
                        }
                    }
                }

                override fun onUnlocked() = Unit

                override fun onScanStopped() {
                    if (streamIdDiscovery === operation) operation.complete()
                }

                override fun onProgress(percent: Int) {
                    if (streamIdDiscovery === operation) operation.reportProgress(percent)
                }

                @Suppress("DEPRECATION")
                override fun onFrequenciesReported(frequencies: IntArray) = Unit

                override fun onFrequenciesLongReported(frequencies: LongArray) = Unit

                override fun onSymbolRatesReported(rate: IntArray) = Unit

                override fun onPlpIdsReported(plpIds: IntArray) = Unit

                override fun onGroupIdsReported(groupIds: IntArray) = Unit

                override fun onInputStreamIdsReported(inputStreamIds: IntArray) {
                    if (streamIdDiscovery === operation) operation.reportIds(inputStreamIds)
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
        operation.start { tunerInstance.scan(settings, Tuner.SCAN_TYPE_AUTO, sectionExecutor, callback) }
        return operation
    }

    private fun cancelStreamIdDiscoveryOnController() {
        val operation = streamIdDiscovery ?: return
        operation.cancel { tuner?.cancelScanning() ?: Tuner.RESULT_SUCCESS }
        // RFごとのscan終了ではapplyFrontend()で選んだBS frontendを保持する。
        // ここでcloseすると次のRFでTRMが別ISDB-S frontendを割り当て得る。
        streamIdDiscovery = null
    }

    /** 世代と待機結果を一つに保持する。状態変更はcontroller executor、awaitだけ呼出元。 */
    @Suppress("TooManyFunctions", "MaxLineLength")
    internal class StreamIdDiscoveryOperation(
        val generation: Long,
    ) {
        private val terminal = CountDownLatch(1)
        private val ids = linkedSetOf<Int>()

        private enum class Outcome { SCANNING, STOPPED, TIMED_OUT, START_FAILED, CANCELLED, LOST }

        private var outcome = Outcome.SCANNING

        // operationの受信結果と、未解放ownerに対する資源喪失通知は別の寿命を持つ。
        private var resourceLossObserved = false
        private var resultCode = Tuner.RESULT_SUCCESS
        private var message = ""
        private var continuationStarted = false
        val active: Boolean get() = outcome == Outcome.SCANNING
        val acceptsResourceLoss: Boolean get() = !resourceLossObserved && outcome != Outcome.CANCELLED

        @Suppress("MagicNumber")
        fun reportIds(values: IntArray) {
            if (active) values.filterTo(ids) { it in 0..0xfffe }
        }

        fun reportProgress(
            @Suppress("UNUSED_PARAMETER") percent: Int,
        ) = Unit // 進捗は停止通知ではない。

        private fun finish(next: Outcome) {
            if (!active) return
            outcome = next
            terminal.countDown()
        }

        fun complete() {
            finish(Outcome.STOPPED)
        }

        fun continueAfterLock(scan: () -> Int) {
            if (!active || continuationStarted) return
            continuationStarted = true
            start(scan)
        }

        fun start(scan: () -> Int) {
            val result = runCatching(scan)
            val code = result.getOrDefault(Tuner.RESULT_UNKNOWN_ERROR)
            if (result.isFailure || code != Tuner.RESULT_SUCCESS) {
                startFailed(code, result.exceptionOrNull()?.message ?: "Tuner.scanに失敗しました result=$code")
            }
        }

        fun startFailed(
            code: Int,
            detail: String,
        ) {
            if (!active) return
            resultCode = code
            message = detail
            finish(Outcome.START_FAILED)
        }

        fun loseResources() {
            resourceLossObserved = true
            finish(Outcome.LOST)
        }

        fun cancel(stopScan: () -> Int) {
            // 非SUCCESS/例外では結果もownerも解放済みにしない。
            val result = stopScan()
            check(result == Tuner.RESULT_SUCCESS) { "BS scanの解放に失敗しました result=$result" }
            finish(Outcome.CANCELLED)
        }

        fun await(timeoutMs: Long): Boolean = terminal.await(timeoutMs.coerceAtLeast(1L), TimeUnit.MILLISECONDS)

        @Suppress("TooGenericExceptionCaught")
        fun resultWithCleanup(
            completed: Boolean,
            cleanup: () -> Unit,
            diagnose: (Exception) -> Unit,
        ): StreamIdDiscoveryResult {
            val result = result(completed)
            try {
                cleanup()
            } catch (failure: Exception) {
                if (!result.resourceLost && outcome != Outcome.START_FAILED) throw failure
                diagnose(failure)
            }
            return result
        }

        fun result(completed: Boolean): StreamIdDiscoveryResult {
            if (!completed) finish(Outcome.TIMED_OUT)
            check(!active) { "BS探索の終端前に結果を取得できません" }
            return when (outcome) {
                Outcome.LOST -> {
                    StreamIdDiscoveryResult(
                        false,
                        emptySet(),
                        Tuner.RESULT_UNAVAILABLE,
                        "TUNER_RESOURCE_LOST",
                        generation,
                        true,
                    )
                }

                Outcome.CANCELLED -> {
                    StreamIdDiscoveryResult(false, emptySet(), Tuner.RESULT_UNAVAILABLE, "BS scan cancelled", generation)
                }

                Outcome.START_FAILED -> {
                    StreamIdDiscoveryResult(false, emptySet(), resultCode, message, generation)
                }

                Outcome.STOPPED -> {
                    StreamIdDiscoveryResult(
                        ids.isNotEmpty(),
                        ids.toSet(),
                        resultCode,
                        if (ids.isEmpty()) "stream ID報告なし" else "",
                        generation,
                    )
                }

                Outcome.TIMED_OUT -> {
                    StreamIdDiscoveryResult(false, emptySet(), resultCode, "scan callback timeout", generation)
                }

                Outcome.SCANNING -> {
                    error("unreachable")
                }
            }
        }
    }

    @Suppress("MaxLineLength")
    fun tuneForScan(candidate: ScanCandidate): TuneOutcome = callOnController { tuneForScanOnController(candidate) }

    private fun tuneForScanOnController(candidate: ScanCandidate): TuneOutcome {
        val synthetic =
            ResolvedChannel(
                uri = null,
                inputId = inputId,
                serviceKey = ServiceKey(0, 0, 0),
                serviceType = 0x01,
                displayName = candidate.displayChannel,
                displayNumber = candidate.displayChannel,
                deliverySystem = candidate.deliverySystem,
                frequencyHz = candidate.frequencyHz,
                streamSelector = candidate.streamSelector,
                physicalChannel = candidate.physicalChannel,
                backendHint = candidate.backendHint,
                satelliteBand = candidate.satelliteBand,
            )
        return tuneResolvedChannel(synthetic, startPlayback = false)
    }

    @Suppress("MaxLineLength")
    fun tuneAndBeginSiIngest(settings: FrontendSettings): Int = callOnController { tuneAndBeginSiIngestOnController(settings) }

    private fun tuneAndBeginSiIngestOnController(settings: FrontendSettings): Int {
        val tunerInstance = tuner ?: return Tuner.RESULT_UNAVAILABLE
        resetBeforeTune()
        val result = tunerInstance.tune(settings)
        if (result == Tuner.RESULT_SUCCESS) {
            initializeAcceptedTune(null, tuneGeneration + 1L)
        }
        return result
    }

    @Suppress("ReturnCount", "MaxLineLength")
    private fun tuneResolvedChannel(
        channel: ResolvedChannel,
        startPlayback: Boolean,
    ): TuneOutcome {
        resetBeforeTune()
        val tunerInstance = tuner ?: return TuneOutcome(false, Tuner.RESULT_UNAVAILABLE, channel, tuneGeneration, "Tuner を利用できません")
        val settings =
            buildFrontendSettings(channel).getOrElse { e ->
                Log.w(LogTags.TIS, "frontend settings 構築に失敗しました channel=$channel", e)
                return TuneOutcome(false, Tuner.RESULT_INVALID_ARGUMENT, channel, tuneGeneration, e.message.orEmpty())
            }
        val nextGeneration = tuneGeneration + 1L
        if (!armTuneEventListener(tunerInstance, nextGeneration)) {
            return TuneOutcome(false, Tuner.RESULT_UNAVAILABLE, channel, tuneGeneration, "frontend tune event listenerを登録できません")
        }
        val result =
            runCatching { tunerInstance.tune(settings) }.getOrElse { e ->
                runCatching { tunerInstance.clearOnTuneEventListener() }
                Log.w(LogTags.TIS, "Tuner.tune が例外を返しました inputId=$inputId channel=$channel", e)
                return TuneOutcome(false, Tuner.RESULT_UNAVAILABLE, channel, tuneGeneration, e.message.orEmpty())
            }
        return if (result == Tuner.RESULT_SUCCESS) {
            initializeAcceptedTune(channel, nextGeneration)
            TuneOutcome(true, result, channel, tuneGeneration)
        } else {
            runCatching { tunerInstance.clearOnTuneEventListener() }
            currentTune = null
            tuneAccepted = false
            playbackPipeline.stop()
            TuneOutcome(false, result, channel, tuneGeneration, "Tuner.tune に失敗しました result=$result")
        }
    }

    private fun invalidateTuneOnController() {
        currentTune = null
        tuneAccepted = false
        captionLanguagesByPid.clear()
        superimposeTimingByPid.clear()
        latestBroadcastClockAuthority = null
    }

    @Suppress("SpreadOperator")
    private fun closeCaptionParsersOnController() {
        SectionFilterPolicy.completeCleanup(
            *captionFactParsers.entries
                .map { (pid, parser) ->
                    {
                        parser.close()
                        captionFactParsers.remove(pid, parser)
                        Unit
                    }
                }.toTypedArray(),
        )
    }

    private fun resetBeforeTune() =
        completeRetuneReset(
            invalidate = { invalidateTuneOnController() },
            { playbackPipeline.stop() },
            { tuner?.clearOnTuneEventListener() },
            { cancelStreamIdDiscoveryOnController() },
            { closeSectionFiltersOnController() },
            { casController?.clearForResourceLoss() },
            { closeCaptionParsersOnController() },
        )

    private fun initializeAcceptedTune(
        channel: ResolvedChannel?,
        generation: Long,
    ) {
        // 準備に失敗したgenerationも再使用しない。配送・CAS受付はcommitまでfalse。
        tuneGeneration = generation
        completeTuneInitialization(
            prepare = { prepareInitialSectionFiltersOnController(generation) },
            commit = {
                currentTune = channel
                tuneAccepted = true
            },
            rollback = { resetBeforeTune() },
        )
    }

    fun beginSiIngestAfterTune(): Boolean = callOnController { beginSiIngestAfterTuneOnController() }

    private fun beginSiIngestAfterTuneOnController(): Boolean {
        if (!tuneAccepted) {
            Log.w(LogTags.TIS, "tune 要求未受付のため SI 取得を開始しません inputId=$inputId")
            return false
        }
        openInitialSectionFilters(tuneGeneration)
        return true
    }

    fun openInitialSectionFilters(generation: Long = tuneGeneration): Unit =
        callOnController {
            openInitialSectionFiltersOnController(generation)
        }

    private fun openInitialSectionFiltersOnController(generation: Long = tuneGeneration) {
        if (!tuneAccepted) return
        prepareInitialSectionFiltersOnController(generation)
    }

    @Suppress("MaxLineLength")
    private fun prepareInitialSectionFiltersOnController(generation: Long) {
        listOf(
            WellKnownSectionPid.PAT,
            WellKnownSectionPid.CAT,
            WellKnownSectionPid.NIT,
            WellKnownSectionPid.SDT_BAT,
            WellKnownSectionPid.EIT,
            WellKnownSectionPid.TDT,
        ).forEach { pid ->
            val handle = SectionFilterPolicy.openOwnedFilter(pid, sectionFilterHandles) { createSectionFilter(pid, generation) }
            check(handle.isOpen) { "初期section filterを開始できません pid=$pid" }
        }
        Log.d(LogTags.TIS, "初期 section filter を開きます inputId=$inputId pids=${sectionFilterHandles.keys} generation=$generation")
    }

    fun openSectionFilters() = openInitialSectionFilters()

    fun openProgramMapFilter(pmtPid: TsPid): SectionFilterHandle = openSectionFilter(pmtPid)

    fun openEcmFilter(ecmPid: TsPid): SectionFilterHandle = openSectionFilter(ecmPid)

    fun openEmmFilter(emmPid: TsPid): SectionFilterHandle = openSectionFilter(emmPid)

    fun openSectionFilter(
        pid: TsPid,
        generation: Long = tuneGeneration,
    ): SectionFilterHandle =
        callOnController {
            openSectionFilterOnController(pid, generation)
        }

    @Suppress("MaxLineLength")
    private fun openSectionFilterOnController(
        pid: TsPid,
        generation: Long = tuneGeneration,
    ): SectionFilterHandle {
        if (!tuneAccepted) return UnavailableSectionFilterHandle(pid, "tune未受付")
        return SectionFilterPolicy.openOwnedFilter(pid, sectionFilterHandles) { createSectionFilter(pid, generation) }
    }

    @Suppress("ReturnCount", "TooGenericExceptionCaught", "MaxLineLength")
    private fun createSectionFilter(
        pid: TsPid,
        generation: Long,
    ): SectionFilterHandle {
        val tunerInstance = tuner ?: return UnavailableSectionFilterHandle(pid, "Tuner利用不可")
        val callback =
            object : FilterCallback {
                override fun onFilterEvent(
                    filter: Filter,
                    events: Array<FilterEvent>,
                ) {
                    if (!isCurrentSectionFilter(pid, generation, filter)) return
                    events.filterIsInstance<SectionEvent>().forEach { event ->
                        val length = event.dataLength.toLong()
                        when (SectionFilterPolicy.dataLengthDecision(length)) {
                            SectionFilterPolicy.DataLengthDecision.MALFORMED -> {
                                recordSectionMalformedDrop(pid, "dataLength=$length")
                                return@forEach
                            }

                            SectionFilterPolicy.DataLengthDecision.OVERSIZED -> {
                                recordSectionOversizedDrop(pid, length)
                                return@forEach
                            }

                            SectionFilterPolicy.DataLengthDecision.ACCEPT -> {
                                Unit
                            }
                        }
                        val section = ByteArray(length.toInt())
                        val readResult = runCatching { filter.read(section, 0, section.size.toLong()) }
                        if (readResult.isFailure) {
                            recordSectionReadError(pid, "exception=${readResult.exceptionOrNull()?.message}")
                            return@forEach
                        }
                        val read = readResult.getOrThrow()
                        val sourceIsCurrent = isCurrentSectionFilter(pid, generation, filter)
                        when (SectionFilterPolicy.readDecision(expected = section.size, actual = read, sourceIsCurrent = sourceIsCurrent)) {
                            SectionFilterPolicy.ReadDecision.INGEST -> {
                                onSectionFromFilter(pid, section, generation, filter)
                            }

                            SectionFilterPolicy.ReadDecision.SHORT_READ -> {
                                recordSectionShortRead(
                                    pid,
                                    expected = section.size,
                                    actual = read,
                                )
                            }

                            SectionFilterPolicy.ReadDecision.READ_ERROR -> {
                                recordSectionReadError(
                                    pid,
                                    "read=$read expected=${section.size}",
                                )
                            }

                            SectionFilterPolicy.ReadDecision.STALE_SOURCE -> {
                                Unit
                            }
                        }
                    }
                }

                override fun onFilterStatusChanged(
                    filter: Filter,
                    status: Int,
                ) {
                    Log.d(LogTags.TIS, "section filter 状態 inputId=$inputId pid=$pid status=$status generation=$generation")
                }
            }
        val artifacts = mutableListOf<SectionFilterArtifact>()
        val handle = TunerSectionFilterHandle(pid, artifacts, generation)
        sectionFilterHandles[pid] = handle
        try {
            for (settings in sectionSettingsForPid(pid)) {
                val filter =
                    tunerInstance.openFilter(Filter.TYPE_TS, Filter.SUBTYPE_SECTION, SECTION_FILTER_BUFFER_BYTES, sectionExecutor, callback)
                        ?: error("section openFilterがnullを返しました pid=$pid")
                artifacts += SectionFilterArtifact(filter)
                val config =
                    TsFilterConfiguration
                        .builder()
                        .setTpid(pid.value)
                        .setSettings(settings)
                        .build()
                val configured = filter.configure(config)
                check(configured == Tuner.RESULT_SUCCESS) { "section configureに失敗しました pid=$pid result=$configured" }
            }
            sectionFilters[pid] = artifacts.map { it.filter }
            for (artifact in artifacts) {
                val started = artifact.filter.start()
                check(started == Tuner.RESULT_SUCCESS) { "section startに失敗しました pid=$pid result=$started" }
                artifact.started = true
            }
            return handle
        } catch (error: RuntimeException) {
            try {
                handle.close()
            } catch (cleanup: RuntimeException) {
                if (cleanup !== error) error.addSuppressed(cleanup)
            }
            if (!handle.isClosed) throw error
            sectionFilterHandles.remove(pid)
            Log.w(LogTags.TIS, "section filter群の準備に失敗しました inputId=$inputId pid=$pid", error)
            return UnavailableSectionFilterHandle(pid, error.message.orEmpty())
        }
    }

    private fun isCurrentSectionFilter(
        pid: TsPid,
        generation: Long,
        filter: Filter,
    ): Boolean = tuneAccepted && generation == tuneGeneration && sectionFilters[pid].orEmpty().any { it === filter }

    @Suppress("MaxLineLength")
    private fun recordSectionShortRead(
        pid: TsPid,
        expected: Int,
        actual: Int,
    ) {
        sectionShortReadCounters[pid] = (sectionShortReadCounters[pid] ?: 0) + 1
        Log.w(
            LogTags.TIS,
            "section short read を破棄します inputId=$inputId pid=$pid expected=$expected actual=$actual count=${sectionShortReadCounters[pid]}",
        )
    }

    @Suppress("MaxLineLength")
    private fun recordSectionReadError(
        pid: TsPid,
        detail: String,
    ) {
        sectionReadErrorCounters[pid] = (sectionReadErrorCounters[pid] ?: 0) + 1
        Log.w(LogTags.TIS, "section read 失敗を破棄します inputId=$inputId pid=$pid detail=$detail count=${sectionReadErrorCounters[pid]}")
    }

    @Suppress("MaxLineLength")
    private fun recordSectionMalformedDrop(
        pid: TsPid,
        detail: String,
    ) {
        sectionMalformedCounters[pid] = (sectionMalformedCounters[pid] ?: 0) + 1
        Log.w(
            LogTags.TIS,
            "malformed section を allocation 前に破棄します inputId=$inputId pid=$pid detail=$detail count=${sectionMalformedCounters[pid]}",
        )
    }

    @Suppress("MaxLineLength")
    private fun recordSectionOversizedDrop(
        pid: TsPid,
        dataLength: Long,
    ) {
        sectionOversizedCounters[pid] = (sectionOversizedCounters[pid] ?: 0) + 1
        Log.w(
            LogTags.TIS,
            "oversized section を allocation 前に破棄します inputId=$inputId pid=$pid dataLength" +
                "=$dataLength max=${SectionFilterPolicy.MAX_SECTION_EVENT_BYTES} count=" +
                "${sectionOversizedCounters[pid]}",
        )
    }

    fun closeSectionFilter(pid: TsPid): Unit = callOnController { closeSectionFilterOnController(pid) }

    private fun closeSectionFilterOnController(pid: TsPid) {
        sectionFilters.remove(pid)
        val handle = sectionFilterHandles[pid] ?: return
        try {
            handle.close()
        } finally {
            if (handle is TunerSectionFilterHandle && handle.isClosed) sectionFilterHandles.remove(pid)
        }
    }

    fun closeSectionFilters(): Unit = callOnController { closeSectionFiltersOnController() }

    @Suppress("TooGenericExceptionCaught")
    private fun closeSectionFiltersOnController() {
        sectionFilters.clear()
        var failure: RuntimeException? = null
        for (pid in sectionFilterHandles.keys.toList()) {
            try {
                closeSectionFilterOnController(pid)
            } catch (error: RuntimeException) {
                val primary = failure
                if (primary == null) {
                    failure = error
                } else if (primary !== error) {
                    primary.addSuppressed(error)
                }
            }
        }
        dynamicPmtPids.retainAll(sectionFilterHandles.keys)
        dynamicEcmPids.retainAll(sectionFilterHandles.keys)
        dynamicEmmPids.retainAll(sectionFilterHandles.keys)
        failure?.let { throw it }
    }

    private fun updateDynamicSectionFiltersOnController(
        pmtPids: Set<TsPid>,
        ecmPids: Set<TsPid>,
        emmPids: Set<TsPid>,
        generation: Long = tuneGeneration,
    ) {
        if (!tuneAccepted || generation != tuneGeneration) return
        SectionFilterPolicy.completeCleanup(
            { replaceDynamicPidSet(dynamicPmtPids, pmtPids) { openProgramMapFilter(it) } },
            { replaceDynamicPidSet(dynamicEcmPids, ecmPids) { openEcmFilter(it) } },
            { replaceDynamicPidSet(dynamicEmmPids, emmPids) { openEmmFilter(it) } },
        )
    }

    @Suppress("MaxLineLength")
    fun updateCasMetadataAndFilters(
        metadata: List<CaMetadata>,
        pmtPids: Set<TsPid>,
        generation: Long,
        casDecisionReady: Boolean,
    ): CasController.UpdateResult? =
        callOnController {
            val controller = casController ?: return@callOnController null
            updateCasIfCurrent(generation, tuneGeneration, tuneAccepted) {
                SectionFilterPolicy.commitCasAndFilters(
                    updateCas = {
                        val acceptedMetadata = SectionFilterPolicy.metadataForCasDecision(casDecisionReady, metadata)
                        val needsDescrambler =
                            acceptedMetadata.any {
                                it.serviceKey != null &&
                                    it.source != com.maleicacid.tvinput.aribsi.CaMetadataSource.CAT
                            }
                        controller.updateFromCaMetadata(
                            acceptedMetadata,
                            if (needsDescrambler) ({ DirectTunerDescramblerBridge(tuner) }) else null,
                        )
                    },
                    commitFilters = { result ->
                        updateDynamicSectionFiltersOnController(pmtPids, result.ecmPids, result.emmPids, generation)
                        check((pmtPids + result.ecmPids + result.emmPids).all { sectionFilterHandles[it]?.isOpen == true }) {
                            "CAS/SI filter集合を開始できません"
                        }
                        if (!casDecisionReady) playbackPipeline.stop()
                    },
                    reject = {
                        SectionFilterPolicy.completeCleanup(
                            { controller.clearForResourceLoss() },
                            { updateDynamicSectionFiltersOnController(pmtPids, emptySet(), emptySet(), generation) },
                            { playbackPipeline.stop() },
                        )
                    },
                )
            }
        }

    private fun replaceDynamicPidSet(
        current: MutableSet<TsPid>,
        next: Set<TsPid>,
        opener: (TsPid) -> SectionFilterHandle,
    ) {
        SectionFilterPolicy.replaceDynamicPids(
            current,
            next,
            close = { pid -> if (pid !in initialPids()) closeSectionFilter(pid) },
            open = { pid -> opener(pid).isOpen },
            isOpen = { pid -> sectionFilterHandles[pid]?.isOpen == true },
        )
    }

    private fun initialPids(): Set<TsPid> =
        setOf(
            WellKnownSectionPid.PAT,
            WellKnownSectionPid.CAT,
            WellKnownSectionPid.NIT,
            WellKnownSectionPid.SDT_BAT,
            WellKnownSectionPid.EIT,
            WellKnownSectionPid.TDT,
        )

    fun onSection(
        pid: TsPid,
        section: ByteArray,
        generation: Long = tuneGeneration,
    ): Unit = callOnController { onSectionOnController(pid, section, generation) }

    private fun onSectionFromFilter(
        pid: TsPid,
        section: ByteArray,
        generation: Long,
        filter: Filter,
    ) {
        if (!isCurrentSectionFilter(pid, generation, filter)) return
        onSectionOnController(pid, section, generation)
    }

    @Suppress("MaxLineLength")
    private fun onSectionOnController(
        pid: TsPid,
        section: ByteArray,
        generation: Long = tuneGeneration,
    ) {
        if (!tuneAccepted || generation != tuneGeneration) return
        SectionFilterPolicy.dispatchSection(
            pid,
            initialPids() + dynamicPmtPids,
            dynamicEcmPids.filterTo(linkedSetOf()) { sectionFilterHandles[it]?.isOpen == true },
            dynamicEmmPids.filterTo(linkedSetOf()) { sectionFilterHandles[it]?.isOpen == true },
            onSi = {
                val receivedNanoTime = if (pid == WellKnownSectionPid.TDT) System.nanoTime() else 0L
                val result = sectionIngestController?.onSection(pid, section)
                if (pid == WellKnownSectionPid.TDT && result?.status == com.maleicacid.tvinput.aribsi.SiStatus.OK) {
                    sectionIngestController?.broadcastClockSnapshot()?.let { fact ->
                        val update =
                            AribBroadcastClock.updateAuthority(
                                latestBroadcastClockAuthority,
                                AribBroadcastClock.SourceSample(
                                    tableId = fact.tableId,
                                    mjd = fact.mjd,
                                    millisOfDay = fact.millisOfDay,
                                    receivedNanoTime = receivedNanoTime,
                                ),
                            )
                        if (update == null) {
                            latestBroadcastClockAuthority = null
                            Log.w(LogTags.TIS, "TDT/TOT clock factをauthorityへ昇格できないためfail-closedにします inputId=$inputId")
                        } else {
                            latestBroadcastClockAuthority = update.authority
                            if (update.discontinuity) {
                                Log.w(
                                    LogTags.TIS,
                                    "TDT/TOT clock discontinuityを検出しました inputId=$inputId generation=${update.authority.generation}",
                                )
                            }
                        }
                        onBroadcastClockUpdatedCallback?.invoke()
                    }
                }
                onSectionIngestedCallback?.invoke()
                startPlaybackIfStreamsKnown()
            },
            onEcm = {
                val diagnostics = casController?.onEcmSection(pid, section).orEmpty()
                diagnostics.forEach { Log.w(LogTags.TIS, "ECM 処理診断 $it") }
                if (diagnostics.any { it.state == CasController.State.ERROR }) {
                    playbackPipeline.reportUnavailable(PlaybackPipeline.PlaybackUnavailableReason.CAS_NO_KEY, diagnostics.joinToString())
                }
                // ECMの成功/失敗も同じ視聴可否gateへ即時に通知する。
                onSectionIngestedCallback?.invoke()
            },
            onEmm = {
                val diagnostics = casController?.onEmmSection(pid, section).orEmpty()
                diagnostics.forEach { Log.w(LogTags.TIS, "EMM 処理診断 $it") }
            },
        )
    }

    private fun startPlaybackIfStreamsKnown() {
        // 実際の サービススナップショット は MaleicacidLiveSession と AribSiEngine が管理する。
        // この hook は section 取り込み後の コールバック 用であり、視聴可能状態を主張しない。
    }

    @Suppress("LongParameterList", "MaxLineLength")
    fun selectAvStreams(
        serviceKey: ServiceKey,
        pcrPid: TsPid?,
        streams: List<AribElementaryStream>,
        preferredAudioTrackId: String? = null,
        preferredSubtitleTrackId: String? = null,
        audioExplicitlyDisabled: Boolean = false,
        subtitleExplicitlyDisabled: Boolean = false,
        defaultComponentGroupTags: Set<Int>? = null,
        dualMonoPresentation: PlaybackPipeline.DualMonoPresentation = PlaybackPipeline.DualMonoPresentation.MAIN,
    ): AvStreamSelection {
        val video = TunerSelectionPolicy.selectVideo(streams, defaultComponentGroupTags)
        val audioCandidates = streams.filter(TunerSelectionPolicy::isSupportedAudioStream)
        val audio =
            if (audioExplicitlyDisabled) {
                null
            } else {
                preferredAudioTrackId?.let { wanted ->
                    audioCandidates.firstOrNull { TunerSelectionPolicy.trackIdForAudio(it) == wanted }
                } ?: TunerSelectionPolicy.selectAudio(streams, defaultComponentGroupTags)
            }
        val captionTracks = captionTracksFor(streams, defaultComponentGroupTags)
        val selectedCaptionTrack =
            if (subtitleExplicitlyDisabled) {
                null
            } else {
                preferredSubtitleTrackId?.let { wanted ->
                    captionTracks.firstOrNull { it.id == wanted }
                }
                    ?: TunerSelectionPolicy.selectCaption(streams, defaultComponentGroupTags)?.let { defaultStream ->
                        captionTracks.firstOrNull {
                            it.pid ==
                                defaultStream.elementaryPid
                        }
                    }
            }
        val subtitle =
            selectedCaptionTrack?.let { track ->
                streams.firstOrNull {
                    it.elementaryPid == track.pid &&
                        TunerSelectionPolicy.isCaptionStream(it)
                }
            }
        val superimpose = TunerSelectionPolicy.selectSuperimpose(streams, defaultComponentGroupTags)
        return AvStreamSelection(
            serviceKey,
            pcrPid,
            video,
            audio,
            subtitle,
            selectedCaptionTrack?.captionLanguageId,
            superimpose,
            audio?.componentType,
            dualMonoPresentation,
        )
    }

    fun tracksFor(
        streams: List<AribElementaryStream>,
        defaultComponentGroupTags: Set<Int>? = null,
    ): List<TisTrack> =
        buildList {
            TunerSelectionPolicy.selectVideo(streams, defaultComponentGroupTags)?.let { stream ->
                add(
                    TisTrack(
                        TunerSelectionPolicy.trackIdForVideo(stream),
                        android.media.tv.TvTrackInfo.TYPE_VIDEO,
                        stream.elementaryPid,
                        stream.streamType,
                        stream.componentTag,
                        stream.componentType,
                        stream.languageCodes.firstOrNull(),
                    ),
                )
            }
            TunerSelectionPolicy.orderedAudioStreams(streams, defaultComponentGroupTags).forEach { stream ->
                add(
                    TisTrack(
                        TunerSelectionPolicy.trackIdForAudio(stream),
                        android.media.tv.TvTrackInfo.TYPE_AUDIO,
                        stream.elementaryPid,
                        stream.streamType,
                        stream.componentTag,
                        stream.componentType,
                        stream.languageCodes.firstOrNull(),
                    ),
                )
            }
            addAll(captionTracksFor(streams, defaultComponentGroupTags))
        }

    private fun captionTracksFor(
        streams: List<AribElementaryStream>,
        defaultComponentGroupTags: Set<Int>? = null,
    ): List<TisTrack> =
        buildList {
            TunerSelectionPolicy.orderedCaptionStreams(streams, defaultComponentGroupTags).forEach { stream ->
                val languages = captionLanguagesByPid[stream.elementaryPid].orEmpty()
                if (languages.isEmpty()) {
                    add(
                        TisTrack(
                            TunerSelectionPolicy.trackIdForSubtitle(stream, 1),
                            android.media.tv.TvTrackInfo.TYPE_SUBTITLE,
                            stream.elementaryPid,
                            stream.streamType,
                            stream.componentTag,
                            stream.componentType,
                            null,
                            stream.dataComponentId,
                            TunerSelectionPolicy.captionKind(stream),
                            1,
                        ),
                    )
                } else {
                    languages.filter { it.languageTag in 0..1 }.forEach { language ->
                        val languageId = language.languageTag + 1
                        add(
                            TisTrack(
                                TunerSelectionPolicy.trackIdForSubtitle(stream, languageId),
                                android.media.tv.TvTrackInfo.TYPE_SUBTITLE,
                                stream.elementaryPid,
                                stream.streamType,
                                stream.componentTag,
                                stream.componentType,
                                language.iso639LanguageCode,
                                stream.dataComponentId,
                                TunerSelectionPolicy.captionKind(stream),
                                languageId,
                            ),
                        )
                    }
                }
            }
        }

    fun superimposeTrackFor(
        streams: List<AribElementaryStream>,
        defaultComponentGroupTags: Set<Int>? = null,
    ): TisTrack? =
        TunerSelectionPolicy.selectSuperimpose(streams, defaultComponentGroupTags)?.let { stream ->
            val language =
                captionLanguagesByPid[stream.elementaryPid]
                    .orEmpty()
                    .filter { it.languageTag in 0..1 }
                    .minByOrNull { it.languageTag }
            TisTrack(
                TunerSelectionPolicy.trackIdForSuperimpose(stream),
                android.media.tv.TvTrackInfo.TYPE_SUBTITLE,
                stream.elementaryPid,
                stream.streamType,
                stream.componentTag,
                stream.componentType,
                language?.iso639LanguageCode,
                stream.dataComponentId,
                "superimpose",
                language?.languageTag?.plus(1) ?: 1,
                language?.automaticPresentationOnReception ?: stream.automaticPresentationOnReception,
            )
        }

    @Suppress("ReturnCount")
    fun startPlayback(selection: AvStreamSelection): PlaybackPipeline.StartResult? {
        val channel = currentTune ?: return null
        val tunerInstance = tuner ?: return null
        superimposeTimingByPid.clear()
        selection.superimpose?.let { stream ->
            stream.captionTiming?.let { timing -> superimposeTimingByPid[stream.elementaryPid] = timing }
        }
        return playbackPipeline.start(tunerInstance, channel, selection)
    }

    fun setOnSubtitleContinuityLostCallback(callback: (Long, String) -> Unit) {
        playbackPipeline.setOnSubtitleContinuityLostCallback { generation, trackId ->
            val pid =
                trackId
                    .substringAfter(':', "")
                    .substringBefore(':')
                    .toIntOrNull()
                    ?.let(TsPid::fromOrNull)
            if (pid != null) {
                captionFactParsers[pid]?.reset()
                captionLanguagesByPid.remove(pid)
            }
            callback(generation, trackId)
        }
    }

    @Suppress("MagicNumber", "MaxLineLength")
    fun setOnSubtitlePesCallback(callback: (Long, String, ByteArray, CaptionTimestamp, AribBroadcastClock.StatementTime?) -> Unit) {
        playbackPipeline.setOnSubtitlePesCallback { generation, trackId, pesData, timestamp ->
            val pid =
                trackId
                    .substringAfter(':', "")
                    .substringBefore(':')
                    .toIntOrNull()
                    ?.let(TsPid::fromOrNull)
            if (pid == null) {
                callback(generation, trackId, pesData, timestamp, null)
                return@setOnSubtitlePesCallback
            }
            val isSuperimpose = trackId.startsWith("superimpose:")
            val factParser = captionFactParsers.computeIfAbsent(pid) { NativeAribCaptionFactParser(isSuperimpose) }
            val facts = factParser.ingest(pesData)
            facts?.management?.let { management -> captionLanguagesByPid[pid] = management.languages }

            if (isSuperimpose && superimposeTimingByPid[pid] == 0x02) {
                when (facts?.disposition) {
                    NativeAribCaptionFactParser.Disposition.STATEMENT_TIMED -> {
                        val statement = facts.statementTime ?: return@setOnSubtitlePesCallback
                        callback(
                            generation,
                            trackId,
                            pesData,
                            CaptionTimestamp.NoPts,
                            AribBroadcastClock.StatementTime(statement.millisOfDay),
                        )
                    }

                    NativeAribCaptionFactParser.Disposition.MANAGEMENT,
                    NativeAribCaptionFactParser.Disposition.FRAGMENT_PENDING,
                    -> {
                        // management data / linked途中fragmentはrenderer continuityだけを進め、表示deadlineは作らない。
                        callback(generation, trackId, pesData, CaptionTimestamp.NoPts, null)
                    }

                    NativeAribCaptionFactParser.Disposition.STATEMENT_INVALID,
                    NativeAribCaptionFactParser.Disposition.INVALID,
                    NativeAribCaptionFactParser.Disposition.NONE,
                    null,
                    -> {
                        Log.w(
                            LogTags.TIS,
                            "Timing=10 superimposeのinvalid/未分類data-groupをfail-closedで破棄します pid=$pid gene" +
                                "ration=$generation disposition=${facts?.disposition}",
                        )
                    }
                }
                return@setOnSubtitlePesCallback
            }
            callback(generation, trackId, pesData, timestamp, null)
        }
    }

    @Suppress("ReturnCount")
    fun switchAudioTrack(selection: AvStreamSelection): PlaybackPipeline.AudioSwitchResult? {
        if (currentTune == null) return null
        val tunerInstance = tuner ?: return null
        return playbackPipeline.switchAudio(tunerInstance, selection)
    }

    fun setDualMonoPresentation(presentation: PlaybackPipeline.DualMonoPresentation): Boolean =
        playbackPipeline.setDualMonoPresentation(presentation)

    fun stopPlayback() {
        playbackPipeline.stop()
    }

    fun currentMediaClockSnapshot(): PlaybackPipeline.MediaClockSnapshot? = playbackPipeline.currentMediaClockSnapshot()

    fun broadcastDeadlineUntil(
        statementTime: AribBroadcastClock.StatementTime,
        expectedClockGeneration: Long? = null,
    ): AribBroadcastClock.Deadline? =
        AribBroadcastClock.deadlineUntil(
            statementTime,
            latestBroadcastClockAuthority,
            expectedClockGeneration,
        )

    fun currentResolvedChannel(): ResolvedChannel? = callOnController { currentTune }

    fun currentGeneration(): Long = callOnController { tuneGeneration }

    fun isTuneRequestAccepted(): Boolean = callOnController { tuneAccepted }

    @Suppress("MagicNumber", "MaxLineLength")
    private fun resolveChannel(channelUri: Uri): Result<ResolvedChannel> =
        runCatching {
            val projection =
                arrayOf(
                    TvContract.Channels.COLUMN_INPUT_ID,
                    TvContract.Channels.COLUMN_ORIGINAL_NETWORK_ID,
                    TvContract.Channels.COLUMN_TRANSPORT_STREAM_ID,
                    TvContract.Channels.COLUMN_SERVICE_ID,
                    TvContract.Channels.COLUMN_DISPLAY_NAME,
                    TvContract.Channels.COLUMN_DISPLAY_NUMBER,
                    TvContract.Channels.COLUMN_SERVICE_TYPE,
                    TvContract.Channels.COLUMN_INTERNAL_PROVIDER_DATA,
                )
            context.contentResolver.query(channelUri, projection, null, null, null)?.use { cursor ->
                if (!cursor.moveToFirst()) error("channel 行が見つかりません uri=$channelUri")
                val rowInputId = cursor.getString(0)
                require(rowInputId == inputId) { "inputId が一致しません row=$rowInputId session=$inputId" }
                val key = ServiceKey(cursor.getInt(1), cursor.getInt(2), cursor.getInt(3))
                val displayName = cursor.getString(4) ?: "service-${key.serviceId}"
                val displayNumber = cursor.getString(5) ?: key.serviceId.toString()
                val serviceType =
                    cursor.getString(6)?.toIntOrNull()?.takeIf { it in 0..0xff }
                        ?: error("channelのARIB service_typeが不正です")
                val providerData = cursor.getBlob(7)
                val decoded =
                    ProviderDataBridge.decodeChannelProviderData(providerData)
                        ?: error("channel provider data JSON v1を復元できません")
                require(decoded.serviceKey == key) { "channel rowとprovider dataのservice keyが一致しません" }
                ResolvedChannel(
                    uri = channelUri,
                    inputId = rowInputId,
                    serviceKey = key,
                    serviceType = serviceType,
                    displayName = displayName,
                    displayNumber = displayNumber,
                    deliverySystem = decoded.tune.deliverySystem,
                    frequencyHz = decoded.tune.frequencyHz,
                    streamSelector = decoded.tune.streamSelector,
                    physicalChannel = decoded.tune.physicalChannel,
                    backendHint = null,
                    satelliteBand = decoded.tune.satelliteBand,
                )
            } ?: error("query が null cursor を返しました uri=$channelUri")
        }

    @Suppress("MaxLineLength")
    private fun buildFrontendSettings(channel: ResolvedChannel): Result<FrontendSettings> =
        runCatching {
            when (channel.deliverySystem) {
                ChannelRecord.DELIVERY_SYSTEM_ISDB_T -> {
                    IsdbtFrontendSettings
                        .builder()
                        .setFrequencyLong(channel.frequencyHz.value)
                        .setBandwidth(IsdbtFrontendSettings.BANDWIDTH_6MHZ)
                        .build()
                }

                ChannelRecord.DELIVERY_SYSTEM_ISDB_S -> {
                    require(channel.satelliteBand != "110CS" || channel.streamSelector.type == StreamSelectorType.NONE) {
                        "CS110 は TSID/relative stream selector による frontend 選局を行いません"
                    }
                    IsdbsFrontendSettings
                        .builder()
                        .setFrequencyLong(channel.frequencyHz.value)
                        .apply {
                            when (channel.streamSelector.type) {
                                StreamSelectorType.NONE -> {
                                    Unit
                                }

                                StreamSelectorType.TSID -> {
                                    setStreamId(requireNotNull(channel.streamSelector.value))
                                    setStreamIdType(IsdbsFrontendSettings.STREAM_ID_TYPE_ID)
                                }

                                StreamSelectorType.RELATIVE -> {
                                    setStreamId(requireNotNull(channel.streamSelector.value))
                                    setStreamIdType(IsdbsFrontendSettings.STREAM_ID_TYPE_RELATIVE_NUMBER)
                                }
                            }
                        }.build()
                }

                else -> {
                    error("対象外の delivery system です: ${channel.deliverySystem}")
                }
            }
        }

    fun release() {
        if (released) return
        if (Thread.currentThread().name.startsWith("maleicacid-tis-controller-$inputId")) {
            releaseOnController()
            sectionExecutor.shutdownNow()
            return
        }
        callOnController { releaseOnController() }
        sectionExecutor.shutdownNow()
        Log.i(LogTags.TIS, "Tuner を解放します inputId=$inputId sessionId=$tvInputSessionId")
    }

    @Suppress("TooGenericExceptionCaught")
    private fun releaseOnController() {
        if (released) return
        invalidateTuneOnController()
        var failure: Throwable? = null

        fun release(action: () -> Unit) {
            try {
                action()
            } catch (error: Throwable) {
                val primary = failure
                if (primary == null) {
                    failure = error
                } else if (primary !== error) {
                    primary.addSuppressed(error)
                }
            }
        }
        release { playbackPipeline.release() }
        release { cancelStreamIdDiscoveryOnController() }
        release { closeSectionFiltersOnController() }
        captionLanguagesByPid.clear()
        captionFactParsers.entries.toList().forEach { (pid, parser) ->
            release {
                parser.close()
                captionFactParsers.remove(pid, parser)
            }
        }
        superimposeTimingByPid.clear()
        latestBroadcastClockAuthority = null
        release {
            casController?.close()
            casController = null
        }
        sectionIngestController = null
        onSectionIngestedCallback = null
        onTunerResourceLostCallback = null
        onTuneEventCallback = null
        release { tuner?.clearOnTuneEventListener() }
        currentTune = null
        tuneAccepted = false
        release {
            tuner?.close()
            tuner = null
        }
        failure?.let { throw it }
        released = true
    }

    override fun close() = release()

    companion object {
        /** 既存controller executorで失効を先に確定し、解放失敗なら呼出元の新tuneへ進まない。 */
        internal fun completeRetuneReset(
            invalidate: () -> Unit,
            vararg cleanup: () -> Unit,
        ) {
            invalidate()
            SectionFilterPolicy.completeCleanup(*cleanup)
        }

        /** 初期化が終わるまで成功を公開しない。rollbackの失敗も元の例外へ添える。 */
        @Suppress("TooGenericExceptionCaught")
        internal fun completeTuneInitialization(
            prepare: () -> Unit,
            commit: () -> Unit,
            rollback: () -> Unit,
        ) {
            try {
                prepare()
                commit()
            } catch (failure: Exception) {
                try {
                    rollback()
                } catch (cleanup: Exception) {
                    if (cleanup !== failure) failure.addSuppressed(cleanup)
                }
                throw failure
            }
        }

        /** 呼出しからCAS attach完了まで同一controller executorを占有する。 */
        @Suppress("MaxLineLength")
        internal fun updateCasIfCurrent(
            requestedGeneration: Long,
            currentGeneration: Long,
            tuneAccepted: Boolean,
            update: () -> CasController.UpdateResult,
        ): CasController.UpdateResult? = if (tuneAccepted && requestedGeneration == currentGeneration) update() else null

        /** 同一controller executorで失効を確定し、cleanup失敗でもlost通知を一度試行する。 */
        internal fun completeResourceLoss(
            invalidate: () -> Boolean,
            cleanup: () -> Unit,
            notifyLost: () -> Unit,
        ) {
            if (!invalidate()) return
            SectionFilterPolicy.completeCleanup(cleanup, notifyLost)
        }

        private const val SECTION_FILTER_BUFFER_BYTES = 64 * 1024L
        private const val BS_STREAM_ID_SCAN_TIMEOUT_MS = 2_500L

        @Suppress("MagicNumber", "MaxLineLength")
        internal fun sectionSettingsForPid(pid: TsPid): List<SectionSettingsWithSectionBits> {
            val tableIds: List<Int?> = if (pid == WellKnownSectionPid.TDT) listOf(0x70, 0x73) else listOf(null)
            return tableIds.map { tableId ->
                SectionSettingsWithSectionBits
                    .builder(Filter.TYPE_TS)
                    .setCrcEnabled(tableId != 0x70)
                    .setRepeat(true)
                    .setRaw(false)
                    .setBitWidthOfLengthField(12)
                    .apply {
                        if (tableId != null) {
                            setFilter(byteArrayOf(tableId.toByte()))
                            setMask(byteArrayOf(0xff.toByte()))
                            setMode(byteArrayOf(0))
                        }
                    }.build()
            }
        }

        internal fun normalizedTvInputSessionId(sessionId: String?): String? = sessionId?.takeIf { it.isNotBlank() }

        internal fun isSignalUnavailableTuneEventForTest(event: Int): Boolean =
            event == OnTuneEventListener.SIGNAL_NO_SIGNAL || event == OnTuneEventListener.SIGNAL_LOST_LOCK
    }
}

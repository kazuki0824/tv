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
import android.media.tv.tuner.frontend.FrontendSettings
import android.media.tv.tuner.frontend.OnTuneEventListener
import android.media.tv.tuner.frontend.ScanCallback
import android.media.tv.tuner.frontend.Atsc3PlpInfo
import android.media.tv.tuner.frontend.IsdbsFrontendSettings
import android.media.tv.tuner.frontend.IsdbtFrontendSettings
import android.net.Uri
import android.util.Log
import android.view.Surface
import com.maleicacid.tvinput.aribsi.AribElementaryStream
import com.maleicacid.tvinput.aribsi.NativeAribCaptionFactParser
import com.maleicacid.tvinput.aribsi.CaMetadata
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
import java.util.concurrent.TimeUnit
import java.util.concurrent.Executors
import java.util.concurrent.RejectedExecutionException

/** ライブ視聴、スキャン、録画向けの Tuner 制御層。 */
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

    private inner class TunerSectionFilterHandle(
        override val pid: TsPid,
        private val filter: Filter,
        private val generation: Long,
    ) : SectionFilterHandle {
        private var closed = false
        override val isOpen: Boolean get() = !closed
        override fun close() {
            if (closed) return
            closed = true
            runCatching { filter.stop() }.onFailure { Log.w(LogTags.TIS, "section filter stop に失敗しました pid=$pid", it) }
            runCatching { filter.close() }.onFailure { Log.w(LogTags.TIS, "section filter close に失敗しました pid=$pid", it) }
        }
        override fun toString(): String = "TunerSectionFilterHandle(pid=$pid, generation=$generation, closed=$closed)"
    }

    private inner class UnavailableSectionFilterHandle(
        override val pid: TsPid,
        private val reason: String,
    ) : SectionFilterHandle {
        override val isOpen: Boolean get() = false
        override fun close() = Unit
        override fun toString(): String = "UnavailableSectionFilterHandle(pid=$pid, reason=$reason)"
    }

    private val sectionExecutor: ExecutorService = Executors.newSingleThreadExecutor { runnable ->
        Thread(runnable, "maleicacid-tis-controller-$inputId").apply { isDaemon = true }
    }
    @Volatile private var released = false

    private fun <T> callOnController(block: () -> T): T {
        if (Thread.currentThread().name.startsWith("maleicacid-tis-controller-$inputId")) return block()
        if (released) throw IllegalStateException("TunerController は解放済みです inputId=$inputId")
        return try {
            sectionExecutor.submit<T> { block() }.get()
        } catch (e: RejectedExecutionException) {
            throw IllegalStateException("TunerController executor は停止済みです inputId=$inputId", e)
        }
    }
    private val sectionFilterHandles = LinkedHashMap<TsPid, SectionFilterHandle>()
    private val sectionFilters = LinkedHashMap<TsPid, Filter>()
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
    private var descramblerBridge: CasController.TunerDescramblerBridge? = null
    private var currentTune: ResolvedChannel? = null
    private var tuneAccepted = false
    private var tuneGeneration: Long = 0L
    private val sectionShortReadCounters = linkedMapOf<TsPid, Int>()
    private val sectionReadErrorCounters = linkedMapOf<TsPid, Int>()
    private val sectionMalformedCounters = linkedMapOf<TsPid, Int>()
    private val sectionOversizedCounters = linkedMapOf<TsPid, Int>()
    private val playbackPipeline = PlaybackPipeline(inputId, tvInputSessionId, sessionContext)

    private fun createTuner(): Tuner? = try {
        Tuner(context, tvInputSessionId, useCase).also { created ->
            created.setResourceLostListener(sectionExecutor) { callbackTuner ->
                if (callbackTuner === tuner && !released) handleTunerResourceLostOnController()
            }
        }
    } catch (e: RuntimeException) {
        Log.w(LogTags.TIS, "Tuner を利用できません inputId=$inputId tvInputSessionId=$tvInputSessionId useCase=$useCase", e)
        null
    }

    fun setSectionIngestController(controller: SectionIngestController?) = callOnController { sectionIngestController = controller }

    fun setCasController(controller: CasController?) = callOnController {
        casController = controller
    }

    fun setOnSectionIngestedCallback(callback: (() -> Unit)?) = callOnController { onSectionIngestedCallback = callback }

    fun setOnTunerResourceLostCallback(callback: ((Long) -> Unit)?) = callOnController { onTunerResourceLostCallback = callback }

    fun setOnTuneEventCallback(callback: ((Long, Int) -> Unit)?) = callOnController { onTuneEventCallback = callback }

    fun setOnBroadcastClockUpdatedCallback(callback: (() -> Unit)?) = callOnController { onBroadcastClockUpdatedCallback = callback }

    fun setPlaybackCallbacks(onVideoAvailable: (Long) -> Unit, onVideoUnavailable: (PlaybackPipeline.PlaybackUnavailable) -> Unit) {
        playbackPipeline.setCallbacks(onVideoAvailable, onVideoUnavailable)
    }

    fun setOnVideoFormatDiscoveredCallback(callback: (Long, PlaybackPipeline.VideoFormatInfo) -> Unit) {
        playbackPipeline.setOnVideoFormatDiscoveredCallback(callback)
    }

    fun setOnVideoOnlyFallbackRestartedCallback(callback: (PlaybackPipeline.VideoOnlyFallbackRestart) -> Unit) {
        playbackPipeline.setOnVideoOnlyFallbackRestartedCallback(callback)
    }

    private fun handleTunerResourceLostOnController() {
        val lostGeneration = tuneGeneration
        playbackPipeline.stop()
        closeSectionFiltersOnController()
        captionLanguagesByPid.clear()
        captionFactParsers.values.forEach { it.close() }
        captionFactParsers.clear()
        superimposeTimingByPid.clear()
        latestBroadcastClockAuthority = null
        currentTune = null
        tuneAccepted = false
        descramblerBridge = null
        onTunerResourceLostCallback?.invoke(lostGeneration)
    }

    private fun armTuneEventListener(tunerInstance: Tuner, generation: Long): Boolean {
        if (onTuneEventCallback == null) return true
        return runCatching {
            tunerInstance.setOnTuneEventListener(sectionExecutor) { event ->
                if (tunerInstance === tuner && !released) handleTuneEventOnController(generation, event)
            }
        }.onFailure { error ->
            Log.w(LogTags.TIS, "frontend tune event listener 登録に失敗しました inputId=$inputId generation=$generation", error)
        }.isSuccess
    }

    private fun handleTuneEventOnController(generation: Long, event: Int) {
        if (!tuneAccepted || generation != tuneGeneration || currentTune == null) return
        when (event) {
            OnTuneEventListener.SIGNAL_NO_SIGNAL, OnTuneEventListener.SIGNAL_LOST_LOCK -> playbackPipeline.stop()
            OnTuneEventListener.SIGNAL_LOCKED -> Unit
            else -> return
        }
        onTuneEventCallback?.invoke(generation, event)
    }

    fun createDescramblerBridge(): CasController.TunerDescramblerBridge = callOnController {
        val existing = descramblerBridge
        if (existing != null) return@callOnController existing
        val created = DirectTunerDescramblerBridge(tuner)
        descramblerBridge = created
        created
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
        val resolved = resolveChannel(channelUri).getOrElse { e ->
            resetBeforeTune()
            Log.w(LogTags.TIS, "channel 解決に失敗しました inputId=$inputId uri=$channelUri", e)
            return TuneOutcome(false, Tuner.RESULT_INVALID_ARGUMENT, null, tuneGeneration, e.message.orEmpty())
        }
        return tuneResolvedChannel(resolved, startPlayback = true)
    }

    data class StreamIdDiscoveryResult(
        val success: Boolean,
        val streamIds: Set<Int>,
        val resultCode: Int,
        val message: String = "",
    )

    fun discoverIsdbsStreamIds(seed: ScanCandidate, timeoutMs: Long = BS_STREAM_ID_SCAN_TIMEOUT_MS): StreamIdDiscoveryResult =
        callOnController { discoverIsdbsStreamIdsOnController(seed, timeoutMs) }

    private fun discoverIsdbsStreamIdsOnController(seed: ScanCandidate, timeoutMs: Long): StreamIdDiscoveryResult {
        require(seed.kind == ScanCandidateKind.ISDB_S_BS && seed.streamSelector == StreamSelector.NONE)
        val tunerInstance = tuner ?: return StreamIdDiscoveryResult(false, emptySet(), Tuner.RESULT_UNAVAILABLE, "Tunerを利用できません")
        resetBeforeTune()
        val settings = IsdbsFrontendSettings.builder()
            .setFrequencyLong(seed.frequencyHz.value)
            .build()
        val terminal = CountDownLatch(1)
        val ids = linkedSetOf<Int>()
        val callback = object : ScanCallback {
            override fun onLocked() = Unit
            override fun onUnlocked() = Unit
            override fun onScanStopped() { terminal.countDown() }
            override fun onProgress(percent: Int) { if (percent >= 100) terminal.countDown() }
            @Suppress("DEPRECATION")
            override fun onFrequenciesReported(frequencies: IntArray) = Unit
            override fun onFrequenciesLongReported(frequencies: LongArray) = Unit
            override fun onSymbolRatesReported(rate: IntArray) = Unit
            override fun onPlpIdsReported(plpIds: IntArray) = Unit
            override fun onGroupIdsReported(groupIds: IntArray) = Unit
            override fun onInputStreamIdsReported(inputStreamIds: IntArray) {
                synchronized(ids) { inputStreamIds.filterTo(ids) { it in 0..0xfffe } }
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
        val directExecutor = java.util.concurrent.Executor { command -> command.run() }
        val result = runCatching { tunerInstance.scan(settings, Tuner.SCAN_TYPE_AUTO, directExecutor, callback) }
            .getOrElse { error ->
                return StreamIdDiscoveryResult(false, emptySet(), Tuner.RESULT_UNKNOWN_ERROR, error.message.orEmpty())
            }
        if (result != Tuner.RESULT_SUCCESS) {
            runCatching { tunerInstance.cancelScanning() }
            return StreamIdDiscoveryResult(false, emptySet(), result, "Tuner.scanに失敗しました result=$result")
        }
        val completed = runCatching { terminal.await(timeoutMs.coerceAtLeast(1L), TimeUnit.MILLISECONDS) }.getOrDefault(false)
        runCatching { tunerInstance.cancelScanning() }
        val snapshot = synchronized(ids) { ids.toSet() }
        return if (snapshot.isNotEmpty()) {
            StreamIdDiscoveryResult(true, snapshot, result, if (completed) "" else "scan callback timeout後に報告済みstream IDを採用")
        } else {
            StreamIdDiscoveryResult(false, emptySet(), result, if (completed) "stream ID報告なし" else "scan callback timeout")
        }
    }

    fun tuneForScan(candidate: ScanCandidate): TuneOutcome = callOnController { tuneForScanOnController(candidate) }

    private fun tuneForScanOnController(candidate: ScanCandidate): TuneOutcome {
        val synthetic = ResolvedChannel(
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

    fun tuneAndBeginSiIngest(settings: FrontendSettings): Int = callOnController { tuneAndBeginSiIngestOnController(settings) }

    private fun tuneAndBeginSiIngestOnController(settings: FrontendSettings): Int {
        val tunerInstance = tuner ?: return Tuner.RESULT_UNAVAILABLE
        resetBeforeTune()
        val result = tunerInstance.tune(settings)
        if (result == Tuner.RESULT_SUCCESS) {
            tuneAccepted = true
            tuneGeneration++
            beginSiIngestAfterTune()
        }
        return result
    }

    private fun tuneResolvedChannel(channel: ResolvedChannel, startPlayback: Boolean): TuneOutcome {
        resetBeforeTune()
        val tunerInstance = tuner ?: return TuneOutcome(false, Tuner.RESULT_UNAVAILABLE, channel, tuneGeneration, "Tuner を利用できません")
        val settings = buildFrontendSettings(channel).getOrElse { e ->
            Log.w(LogTags.TIS, "frontend settings 構築に失敗しました channel=$channel", e)
            return TuneOutcome(false, Tuner.RESULT_INVALID_ARGUMENT, channel, tuneGeneration, e.message.orEmpty())
        }
        val nextGeneration = tuneGeneration + 1L
        if (!armTuneEventListener(tunerInstance, nextGeneration)) {
            return TuneOutcome(false, Tuner.RESULT_UNAVAILABLE, channel, tuneGeneration, "frontend tune event listenerを登録できません")
        }
        val result = runCatching { tunerInstance.tune(settings) }.getOrElse { e ->
            runCatching { tunerInstance.clearOnTuneEventListener() }
            Log.w(LogTags.TIS, "Tuner.tune が例外を返しました inputId=$inputId channel=$channel", e)
            return TuneOutcome(false, Tuner.RESULT_UNAVAILABLE, channel, tuneGeneration, e.message.orEmpty())
        }
        return if (result == Tuner.RESULT_SUCCESS) {
            currentTune = channel
            tuneAccepted = true
            tuneGeneration = nextGeneration
            beginSiIngestAfterTune()
            TuneOutcome(true, result, channel, tuneGeneration)
        } else {
            runCatching { tunerInstance.clearOnTuneEventListener() }
            currentTune = null
            tuneAccepted = false
            playbackPipeline.stop()
            TuneOutcome(false, result, channel, tuneGeneration, "Tuner.tune に失敗しました result=$result")
        }
    }

    private fun resetBeforeTune() {
        playbackPipeline.stop()
        runCatching { tuner?.clearOnTuneEventListener() }
        closeSectionFilters()
        casController?.clearForClearService()
        captionLanguagesByPid.clear()
        captionFactParsers.values.forEach { it.close() }
        captionFactParsers.clear()
        superimposeTimingByPid.clear()
        latestBroadcastClockAuthority = null
        currentTune = null
        tuneAccepted = false
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

    fun openInitialSectionFilters(generation: Long = tuneGeneration): Unit = callOnController { openInitialSectionFiltersOnController(generation) }

    private fun openInitialSectionFiltersOnController(generation: Long = tuneGeneration) {
        if (!tuneAccepted) return
        listOf(WellKnownSectionPid.PAT, WellKnownSectionPid.CAT, WellKnownSectionPid.NIT, WellKnownSectionPid.SDT_BAT, WellKnownSectionPid.EIT, WellKnownSectionPid.TDT)
            .forEach { openSectionFilterOnController(it, generation) }
        Log.d(LogTags.TIS, "初期 section filter を開きます inputId=$inputId pids=${sectionFilterHandles.keys} generation=$generation")
    }

    fun openSectionFilters() = openInitialSectionFilters()
    fun openProgramMapFilter(pmtPid: TsPid): SectionFilterHandle = openSectionFilter(pmtPid)
    fun openEcmFilter(ecmPid: TsPid): SectionFilterHandle = openSectionFilter(ecmPid)
    fun openEmmFilter(emmPid: TsPid): SectionFilterHandle = openSectionFilter(emmPid)

    fun openSectionFilter(pid: TsPid, generation: Long = tuneGeneration): SectionFilterHandle = callOnController { openSectionFilterOnController(pid, generation) }

    private fun openSectionFilterOnController(pid: TsPid, generation: Long = tuneGeneration): SectionFilterHandle {
        if (!tuneAccepted) return UnavailableSectionFilterHandle(pid, "tune未受付")
        sectionFilterHandles[pid]?.let { existing ->
            if (existing.isOpen) return existing
            sectionFilterHandles.remove(pid)
        }
        val handle = createSectionFilter(pid, generation)
        if (handle.isOpen) {
            sectionFilterHandles[pid] = handle
        }
        return handle
    }

    private fun createSectionFilter(pid: TsPid, generation: Long): SectionFilterHandle {
        val tunerInstance = tuner ?: return UnavailableSectionFilterHandle(pid, "Tuner利用不可")
        val callback = object : FilterCallback {
            override fun onFilterEvent(filter: Filter, events: Array<FilterEvent>) {
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
                        SectionFilterPolicy.DataLengthDecision.ACCEPT -> Unit
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
                        SectionFilterPolicy.ReadDecision.INGEST -> onSectionFromFilter(pid, section, generation, filter)
                        SectionFilterPolicy.ReadDecision.SHORT_READ -> recordSectionShortRead(pid, expected = section.size, actual = read)
                        SectionFilterPolicy.ReadDecision.READ_ERROR -> recordSectionReadError(pid, "read=$read expected=${section.size}")
                        SectionFilterPolicy.ReadDecision.STALE_SOURCE -> Unit
                    }
                }
            }
            override fun onFilterStatusChanged(filter: Filter, status: Int) {
                Log.d(LogTags.TIS, "section filter 状態 inputId=$inputId pid=$pid status=$status generation=$generation")
            }
        }
        val filter = tunerInstance.openFilter(Filter.TYPE_TS, Filter.SUBTYPE_SECTION, SECTION_FILTER_BUFFER_BYTES, sectionExecutor, callback)
            ?: return UnavailableSectionFilterHandle(pid, "openFilterがnullを返しました")
        val settings = SectionSettingsWithSectionBits.builder(Filter.TYPE_TS)
            .setCrcEnabled(pid != WellKnownSectionPid.TDT)
            .setRepeat(true)
            .setRaw(false)
            .setBitWidthOfLengthField(12)
            .build()
        val config = TsFilterConfiguration.builder().setTpid(pid.value).setSettings(settings).build()
        val configureResult = filter.configure(config)
        if (configureResult != Tuner.RESULT_SUCCESS) {
            runCatching { filter.close() }
            return UnavailableSectionFilterHandle(pid, "configure失敗-$configureResult")
        }
        sectionFilters[pid] = filter
        val startResult = filter.start()
        if (startResult != Tuner.RESULT_SUCCESS) {
            if (sectionFilters[pid] === filter) sectionFilters.remove(pid)
            runCatching { filter.close() }
            return UnavailableSectionFilterHandle(pid, "start失敗-$startResult")
        }
        return TunerSectionFilterHandle(pid, filter, generation)
    }

    private fun isCurrentSectionFilter(pid: TsPid, generation: Long, filter: Filter): Boolean =
        generation == tuneGeneration && sectionFilters[pid] === filter

    private fun recordSectionShortRead(pid: TsPid, expected: Int, actual: Int) {
        sectionShortReadCounters[pid] = (sectionShortReadCounters[pid] ?: 0) + 1
        Log.w(LogTags.TIS, "section short read を破棄します inputId=$inputId pid=$pid expected=$expected actual=$actual count=${sectionShortReadCounters[pid]}")
    }

    private fun recordSectionReadError(pid: TsPid, detail: String) {
        sectionReadErrorCounters[pid] = (sectionReadErrorCounters[pid] ?: 0) + 1
        Log.w(LogTags.TIS, "section read 失敗を破棄します inputId=$inputId pid=$pid detail=$detail count=${sectionReadErrorCounters[pid]}")
    }

    private fun recordSectionMalformedDrop(pid: TsPid, detail: String) {
        sectionMalformedCounters[pid] = (sectionMalformedCounters[pid] ?: 0) + 1
        Log.w(LogTags.TIS, "malformed section を allocation 前に破棄します inputId=$inputId pid=$pid detail=$detail count=${sectionMalformedCounters[pid]}")
    }

    private fun recordSectionOversizedDrop(pid: TsPid, dataLength: Long) {
        sectionOversizedCounters[pid] = (sectionOversizedCounters[pid] ?: 0) + 1
        Log.w(LogTags.TIS, "oversized section を allocation 前に破棄します inputId=$inputId pid=$pid dataLength=$dataLength max=${SectionFilterPolicy.MAX_SECTION_EVENT_BYTES} count=${sectionOversizedCounters[pid]}")
    }

    fun closeSectionFilter(pid: TsPid): Unit = callOnController { closeSectionFilterOnController(pid) }

    private fun closeSectionFilterOnController(pid: TsPid) {
        sectionFilters.remove(pid)
        sectionFilterHandles.remove(pid)?.close()
    }

    fun closeSectionFilters(): Unit = callOnController { closeSectionFiltersOnController() }

    private fun closeSectionFiltersOnController() {
        val handles = sectionFilterHandles.values.toList()
        sectionFilterHandles.clear()
        sectionFilters.clear()
        dynamicPmtPids.clear()
        dynamicEcmPids.clear()
        dynamicEmmPids.clear()
        handles.forEach { it.close() }
    }

    fun openDynamicFiltersFromCurrentSi(pmtPids: Iterable<TsPid>, ecmPids: Iterable<TsPid>, emmPids: Iterable<TsPid>) =
        updateDynamicSectionFilters(pmtPids.toSet(), ecmPids.toSet(), emmPids.toSet(), tuneGeneration)

    fun updateDynamicSectionFiltersForService(
        serviceKey: ServiceKey,
        pmtPids: Set<TsPid>,
        ecmPids: Set<TsPid>,
        emmPids: Set<TsPid>,
        generation: Long,
    ): Unit = callOnController {
        if (generation != tuneGeneration || currentTune?.serviceKey != serviceKey) return@callOnController
        updateDynamicSectionFiltersOnController(pmtPids, ecmPids, emmPids, generation)
    }

    fun updateDynamicSectionFilters(pmtPids: Set<TsPid>, ecmPids: Set<TsPid>, emmPids: Set<TsPid>, generation: Long = tuneGeneration): Unit =
        callOnController { updateDynamicSectionFiltersOnController(pmtPids, ecmPids, emmPids, generation) }

    private fun updateDynamicSectionFiltersOnController(pmtPids: Set<TsPid>, ecmPids: Set<TsPid>, emmPids: Set<TsPid>, generation: Long = tuneGeneration) {
        if (!tuneAccepted || generation != tuneGeneration) return
        replaceDynamicPidSet(dynamicPmtPids, pmtPids) { openProgramMapFilter(it) }
        replaceDynamicPidSet(dynamicEcmPids, ecmPids) { openEcmFilter(it) }
        replaceDynamicPidSet(dynamicEmmPids, emmPids) { openEmmFilter(it) }
    }

    fun updateCasMetadata(metadata: List<CaMetadata>): CasController.UpdateResult? = callOnController {
        val controller = casController ?: return@callOnController null
        if (metadata.isEmpty()) {
            controller.clearForClearService()
            CasController.UpdateResult(emptyList(), emptySet(), emptySet())
        } else {
            controller.updateFromCaMetadata(metadata, createDescramblerBridge())
        }
    }

    private fun replaceDynamicPidSet(current: MutableSet<TsPid>, next: Set<TsPid>, opener: (TsPid) -> SectionFilterHandle) {
        val sanitized = next
        (current - sanitized).toList().forEach { pid ->
            current.remove(pid)
            if (pid !in initialPids()) closeSectionFilter(pid)
        }
        (sanitized - current).forEach { pid ->
            val handle = opener(pid)
            if (handle.isOpen) current += pid
        }
    }

    private fun initialPids(): Set<TsPid> = setOf(WellKnownSectionPid.PAT, WellKnownSectionPid.CAT, WellKnownSectionPid.NIT, WellKnownSectionPid.SDT_BAT, WellKnownSectionPid.EIT, WellKnownSectionPid.TDT)

    fun onSection(pid: TsPid, section: ByteArray, generation: Long = tuneGeneration): Unit =
        callOnController { onSectionOnController(pid, section, generation) }

    private fun onSectionFromFilter(pid: TsPid, section: ByteArray, generation: Long, filter: Filter) {
        if (!isCurrentSectionFilter(pid, generation, filter)) return
        onSectionOnController(pid, section, generation)
    }

    private fun onSectionOnController(pid: TsPid, section: ByteArray, generation: Long = tuneGeneration) {
        if (generation != tuneGeneration) return
        val receivedNanoTime = if (pid == WellKnownSectionPid.TDT) System.nanoTime() else 0L
        val result = sectionIngestController?.onSection(pid, section)
        if (pid == WellKnownSectionPid.TDT && result?.status == com.maleicacid.tvinput.aribsi.SiStatus.OK) {
            sectionIngestController?.broadcastClockSnapshot()?.let { fact ->
                val update = AribBroadcastClock.updateAuthority(
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
        if (pid in dynamicEcmPids) {
            val diagnostics = casController?.onEcmSection(pid, section).orEmpty()
            diagnostics.forEach { Log.w(LogTags.TIS, "ECM 処理診断 $it") }
            if (diagnostics.any { it.state == CasController.State.ERROR }) {
                playbackPipeline.reportUnavailable(PlaybackPipeline.PlaybackUnavailableReason.CAS_NO_KEY, diagnostics.joinToString())
            }
        }
        if (pid in dynamicEmmPids) {
            val diagnostics = casController?.onEmmSection(pid, section).orEmpty()
            diagnostics.forEach { Log.w(LogTags.TIS, "EMM 処理診断 $it") }
        }
        onSectionIngestedCallback?.let { it() }
        startPlaybackIfStreamsKnown()
        Log.d(LogTags.TIS, "section 入力 inputId=$inputId pid=$pid size=${section.size} result=$result generation=$generation")
    }

    private fun startPlaybackIfStreamsKnown() {
        // 実際の サービススナップショット は MaleicacidLiveSession と AribSiEngine が管理する。
        // この hook は section 取り込み後の コールバック 用であり、視聴可能状態を主張しない。
    }

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
        val audioCandidates = streams.filter { TunerSelectionPolicy.isSupportedAudioStreamType(it.streamType) }
        val audio = if (audioExplicitlyDisabled) null else preferredAudioTrackId?.let { wanted ->
            audioCandidates.firstOrNull { TunerSelectionPolicy.trackIdForAudio(it) == wanted }
        } ?: TunerSelectionPolicy.selectAudio(streams, defaultComponentGroupTags)
        val captionTracks = captionTracksFor(streams, defaultComponentGroupTags)
        val selectedCaptionTrack = if (subtitleExplicitlyDisabled) null else preferredSubtitleTrackId?.let { wanted ->
            captionTracks.firstOrNull { it.id == wanted }
        } ?: TunerSelectionPolicy.selectCaption(streams, defaultComponentGroupTags)?.let { defaultStream -> captionTracks.firstOrNull { it.pid == defaultStream.elementaryPid } }
        val subtitle = selectedCaptionTrack?.let { track -> streams.firstOrNull { it.elementaryPid == track.pid && TunerSelectionPolicy.isCaptionStream(it) } }
        val superimpose = TunerSelectionPolicy.selectSuperimpose(streams, defaultComponentGroupTags)
        return AvStreamSelection(serviceKey, pcrPid, video, audio, subtitle, selectedCaptionTrack?.captionLanguageId, superimpose, audio?.componentType, dualMonoPresentation)
    }

    fun tracksFor(streams: List<AribElementaryStream>, defaultComponentGroupTags: Set<Int>? = null): List<TisTrack> = buildList {
        TunerSelectionPolicy.selectVideo(streams, defaultComponentGroupTags)?.let { stream ->
            add(TisTrack(TunerSelectionPolicy.trackIdForVideo(stream), android.media.tv.TvTrackInfo.TYPE_VIDEO, stream.elementaryPid, stream.streamType, stream.componentTag, stream.componentType, stream.languageCodes.firstOrNull()))
        }
        TunerSelectionPolicy.orderedAudioStreams(streams, defaultComponentGroupTags).forEach { stream ->
            add(TisTrack(TunerSelectionPolicy.trackIdForAudio(stream), android.media.tv.TvTrackInfo.TYPE_AUDIO, stream.elementaryPid, stream.streamType, stream.componentTag, stream.componentType, stream.languageCodes.firstOrNull()))
        }
        addAll(captionTracksFor(streams, defaultComponentGroupTags))
    }

    private fun captionTracksFor(streams: List<AribElementaryStream>, defaultComponentGroupTags: Set<Int>? = null): List<TisTrack> = buildList {
        TunerSelectionPolicy.orderedCaptionStreams(streams, defaultComponentGroupTags).forEach { stream ->
            val languages = captionLanguagesByPid[stream.elementaryPid].orEmpty()
            if (languages.isEmpty()) {
                add(TisTrack(TunerSelectionPolicy.trackIdForSubtitle(stream, 1), android.media.tv.TvTrackInfo.TYPE_SUBTITLE, stream.elementaryPid, stream.streamType, stream.componentTag, stream.componentType, null, stream.dataComponentId, TunerSelectionPolicy.captionKind(stream), 1))
            } else {
                languages.filter { it.languageTag in 0..1 }.forEach { language ->
                    val languageId = language.languageTag + 1
                    add(TisTrack(TunerSelectionPolicy.trackIdForSubtitle(stream, languageId), android.media.tv.TvTrackInfo.TYPE_SUBTITLE, stream.elementaryPid, stream.streamType, stream.componentTag, stream.componentType, language.iso639LanguageCode, stream.dataComponentId, TunerSelectionPolicy.captionKind(stream), languageId))
                }
            }
        }
    }

    fun superimposeTrackFor(streams: List<AribElementaryStream>, defaultComponentGroupTags: Set<Int>? = null): TisTrack? =
        TunerSelectionPolicy.selectSuperimpose(streams, defaultComponentGroupTags)?.let { stream ->
            val language = captionLanguagesByPid[stream.elementaryPid].orEmpty()
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

    fun startPlayback(selection: AvStreamSelection): PlaybackPipeline.StartResult? {
        val channel = currentTune ?: return null
        val tunerInstance = tuner ?: return null
        superimposeTimingByPid.clear()
        selection.superimpose?.let { stream ->
            stream.captionTiming?.let { timing -> superimposeTimingByPid[stream.elementaryPid] = timing }
        }
        return playbackPipeline.start(tunerInstance, channel, selection)
    }

    fun setOnSubtitlePesCallback(callback: (Long, String, ByteArray, CaptionTimestamp, AribBroadcastClock.StatementTime?) -> Unit) {
        playbackPipeline.setOnSubtitlePesCallback { generation, trackId, pesData, timestamp ->
            val pid = trackId.substringAfter(':', "").substringBefore(':').toIntOrNull()?.let(TsPid::fromOrNull)
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
                    NativeAribCaptionFactParser.Disposition.FRAGMENT_PENDING -> {
                        // management data / linked途中fragmentはrenderer continuityだけを進め、表示deadlineは作らない。
                        callback(generation, trackId, pesData, CaptionTimestamp.NoPts, null)
                    }
                    NativeAribCaptionFactParser.Disposition.STATEMENT_INVALID,
                    NativeAribCaptionFactParser.Disposition.INVALID,
                    NativeAribCaptionFactParser.Disposition.NONE,
                    null -> {
                        Log.w(
                            LogTags.TIS,
                            "Timing=10 superimposeのinvalid/未分類data-groupをfail-closedで破棄します pid=$pid generation=$generation disposition=${facts?.disposition}",
                        )
                    }
                }
                return@setOnSubtitlePesCallback
            }
            callback(generation, trackId, pesData, timestamp, null)
        }
    }

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

    fun currentMediaClockSnapshot(): PlaybackPipeline.MediaClockSnapshot? =
        playbackPipeline.currentMediaClockSnapshot()

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

    private fun resolveChannel(channelUri: Uri): Result<ResolvedChannel> = runCatching {
        val projection = arrayOf(
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
            val serviceType = cursor.getString(6)?.toIntOrNull()?.takeIf { it in 0..0xff }
                ?: error("channelのARIB service_typeが不正です")
            val providerData = providerDataBytes(cursor, 7)
            val decoded = ProviderDataBridge.decodeChannelProviderData(providerData)
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

    private fun buildFrontendSettings(channel: ResolvedChannel): Result<FrontendSettings> = runCatching {
        when (channel.deliverySystem) {
            ChannelRecord.DELIVERY_SYSTEM_ISDB_T -> IsdbtFrontendSettings.builder()
                .setFrequencyLong(channel.frequencyHz.value)
                .setBandwidth(IsdbtFrontendSettings.BANDWIDTH_6MHZ)
                .build()
            ChannelRecord.DELIVERY_SYSTEM_ISDB_S -> {
                require(channel.backendHint != "earth_pt1" || channel.streamSelector.type != StreamSelectorType.RELATIVE) { "earth_pt1 BS では相対 TS 番号を使えません" }
                require(channel.satelliteBand != "110CS" || channel.streamSelector.type == StreamSelectorType.NONE) { "CS110 は TSID/relative stream selector による frontend 選局を行いません" }
                IsdbsFrontendSettings.builder()
                .setFrequencyLong(channel.frequencyHz.value)
                .apply {
                    when (channel.streamSelector.type) {
                        StreamSelectorType.NONE -> Unit
                        StreamSelectorType.TSID -> {
                            setStreamId(requireNotNull(channel.streamSelector.value))
                            setStreamIdType(IsdbsFrontendSettings.STREAM_ID_TYPE_ID)
                        }
                        StreamSelectorType.RELATIVE -> {
                            setStreamId(requireNotNull(channel.streamSelector.value))
                            setStreamIdType(IsdbsFrontendSettings.STREAM_ID_TYPE_RELATIVE_NUMBER)
                        }
                    }
                }
                .build()
            }
            else -> error("対象外の delivery system です: ${channel.deliverySystem}")
        }
    }

    private fun providerDataBytes(cursor: android.database.Cursor, index: Int): ByteArray? =
        runCatching { cursor.getBlob(index) }.getOrNull()
            ?: runCatching { cursor.getString(index)?.toByteArray(Charsets.UTF_8) }.getOrNull()

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

    private fun releaseOnController() {
        if (released) return
        playbackPipeline.release()
        closeSectionFiltersOnController()
        captionLanguagesByPid.clear()
        captionFactParsers.values.forEach { it.close() }
        captionFactParsers.clear()
        superimposeTimingByPid.clear()
        latestBroadcastClockAuthority = null
        casController?.close()
        casController = null
        descramblerBridge = null
        sectionIngestController = null
        onSectionIngestedCallback = null
        onTuneEventCallback = null
        runCatching { tuner?.clearOnTuneEventListener() }
        currentTune = null
        tuneAccepted = false
        tuner?.close()
        tuner = null
        released = true
    }

    override fun close() = release()

    companion object {
        private const val SECTION_FILTER_BUFFER_BYTES = 64 * 1024L
        private const val BS_STREAM_ID_SCAN_TIMEOUT_MS = 2_500L

        internal fun normalizedTvInputSessionId(sessionId: String?): String? =
            sessionId?.takeIf { it.isNotBlank() }

        internal fun isSignalUnavailableTuneEventForTest(event: Int): Boolean =
            event == OnTuneEventListener.SIGNAL_NO_SIGNAL || event == OnTuneEventListener.SIGNAL_LOST_LOCK
    }
}

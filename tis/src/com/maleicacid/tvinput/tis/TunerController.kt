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
import android.media.tv.tuner.frontend.IsdbsFrontendSettings
import android.media.tv.tuner.frontend.IsdbtFrontendSettings
import android.net.Uri
import android.util.Log
import android.view.Surface
import com.maleicacid.tvinput.aribsi.AribElementaryStream
import com.maleicacid.tvinput.aribsi.CaMetadata
import com.maleicacid.tvinput.aribsi.SectionIngestController
import com.maleicacid.tvinput.aribsi.WellKnownSectionPid
import com.maleicacid.tvinput.common.LogTags
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.StreamSelector
import com.maleicacid.tvinput.common.StreamSelectorType
import android.content.AttributionSource
import com.maleicacid.tvinput.db.ChannelRecord
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors

/** ライブ視聴、スキャン、録画向けの Tuner 制御層。 */
class TunerController(
    private val context: Context,
    private val inputId: String,
    private val useCase: Int = TvInputService.PRIORITY_HINT_USE_CASE_TYPE_LIVE,
    private val sessionId: String? = null,
    private val attributionSource: AttributionSource? = null,
) : AutoCloseable {
    interface SectionFilterHandle : AutoCloseable {
        val pid: Int
        val isOpen: Boolean
    }

    data class ResolvedChannel(
        val uri: Uri?,
        val inputId: String,
        val serviceKey: ServiceKey,
        val displayName: String,
        val displayNumber: String,
        val deliverySystem: String,
        val frequencyHz: Long,
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
        val pcrPid: Int?,
        val video: AribElementaryStream?,
        val audio: AribElementaryStream?,
    )

    data class TisTrack(
        val id: String,
        val type: Int,
        val pid: Int,
        val streamType: Int,
        val componentTag: Int?,
        val language: String?,
    )

    enum class SectionReadDecision { INGEST, SHORT_READ, READ_ERROR, STALE_GENERATION }

    private inner class TunerSectionFilterHandle(
        override val pid: Int,
        private val filter: Filter,
        private val generation: Long,
        private val filterToken: Long,
    ) : SectionFilterHandle {
        private var closed = false
        override val isOpen: Boolean get() = !closed
        override fun close() {
            if (closed) return
            closed = true
            runCatching { filter.stop() }.onFailure { Log.w(LogTags.TIS, "section filter stop に失敗しました pid=$pid", it) }
            runCatching { filter.close() }.onFailure { Log.w(LogTags.TIS, "section filter close に失敗しました pid=$pid", it) }
        }
        override fun toString(): String = "TunerSectionFilterHandle(pid=$pid, generation=$generation, token=$filterToken, closed=$closed)"
    }

    private inner class UnavailableSectionFilterHandle(
        override val pid: Int,
        private val reason: String,
    ) : SectionFilterHandle {
        override val isOpen: Boolean get() = false
        override fun close() = Unit
        override fun toString(): String = "UnavailableSectionFilterHandle(pid=$pid, reason=$reason)"
    }

    private val sectionExecutor: ExecutorService = Executors.newSingleThreadExecutor { runnable ->
        Thread(runnable, "maleicacid-tis-controller-$inputId").apply { isDaemon = true }
    }
    private fun <T> callOnController(block: () -> T): T {
        if (Thread.currentThread().name.startsWith("maleicacid-tis-controller-$inputId")) return block()
        return sectionExecutor.submit<T> { block() }.get()
    }
    private val sectionFilterHandles = LinkedHashMap<Int, SectionFilterHandle>()
    private val sectionFilterTokens = LinkedHashMap<Int, Long>()
    private var nextSectionFilterToken: Long = 1L
    private val dynamicPmtPids = linkedSetOf<Int>()
    private val dynamicEcmPids = linkedSetOf<Int>()
    private val dynamicEmmPids = linkedSetOf<Int>()
    private var sectionIngestController: SectionIngestController? = null
    private var casController: CasController? = null
    private var onSectionIngestedCallback: (() -> Unit)? = null
    private val tvInputSessionId: String = sessionId?.takeIf { it.isNotBlank() } ?: "maleicacid-$inputId-${System.nanoTime()}"
    private var tuner: Tuner? = createTuner()
    private var descramblerBridge: CasController.TunerDescramblerBridge? = null
    private var currentTune: ResolvedChannel? = null
    private var tuneAccepted = false
    private var tuneGeneration: Long = 0L
    private val sectionShortReadCounters = linkedMapOf<Int, Int>()
    private val sectionReadErrorCounters = linkedMapOf<Int, Int>()
    private val playbackPipeline = PlaybackPipeline(inputId, tvInputSessionId, attributionSource)

    private fun createTuner(): Tuner? = try {
        Tuner(context, tvInputSessionId, useCase)
    } catch (e: RuntimeException) {
        Log.w(LogTags.TIS, "Tuner を利用できません inputId=$inputId tvInputSessionId=$tvInputSessionId useCase=$useCase", e)
        null
    }

    fun setSectionIngestController(controller: SectionIngestController?) { sectionIngestController = controller }

    fun setCasController(controller: CasController?) {
        casController = controller
    }

    fun setOnSectionIngestedCallback(callback: (() -> Unit)?) { onSectionIngestedCallback = callback }

    fun setPlaybackCallbacks(onVideoAvailable: () -> Unit, onVideoUnavailable: (PlaybackPipeline.PlaybackUnavailable) -> Unit) {
        playbackPipeline.setCallbacks(onVideoAvailable, onVideoUnavailable)
    }

    fun setOnVideoFormatDiscoveredCallback(callback: (PlaybackPipeline.VideoFormatInfo) -> Unit) {
        playbackPipeline.setOnVideoFormatDiscoveredCallback(callback)
    }

    fun createDescramblerBridge(): CasController.TunerDescramblerBridge {
        val existing = descramblerBridge
        if (existing != null) return existing
        val created = DirectTunerDescramblerBridge(tuner)
        descramblerBridge = created
        return created
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
        // B-03: channel 解決に失敗する場合も旧 live state を先に破棄する。
        resetBeforeTune()
        val resolved = resolveChannel(channelUri).getOrElse { e ->
            Log.w(LogTags.TIS, "channel 解決に失敗しました inputId=$inputId uri=$channelUri", e)
            return TuneOutcome(false, Tuner.RESULT_INVALID_ARGUMENT, null, tuneGeneration, e.message.orEmpty())
        }
        return tuneResolvedChannel(resolved, startPlayback = true)
    }

    fun tuneForScan(candidate: ScanCandidate): TuneOutcome = callOnController { tuneForScanOnController(candidate) }

    private fun tuneForScanOnController(candidate: ScanCandidate): TuneOutcome {
        val synthetic = ResolvedChannel(
            uri = null,
            inputId = inputId,
            serviceKey = ServiceKey(0, 0, 0),
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
        val tunerInstance = tuner ?: return TuneOutcome(false, Tuner.RESULT_UNAVAILABLE, channel, tuneGeneration, "Tuner を利用できません")
        val settings = buildFrontendSettings(channel).getOrElse { e ->
            Log.w(LogTags.TIS, "frontend settings 構築に失敗しました channel=$channel", e)
            return TuneOutcome(false, Tuner.RESULT_INVALID_ARGUMENT, channel, tuneGeneration, e.message.orEmpty())
        }
        resetBeforeTune()
        val result = runCatching { tunerInstance.tune(settings) }.getOrElse { e ->
            Log.w(LogTags.TIS, "Tuner.tune が例外を返しました inputId=$inputId channel=$channel", e)
            return TuneOutcome(false, Tuner.RESULT_UNAVAILABLE, channel, tuneGeneration, e.message.orEmpty())
        }
        return if (result == Tuner.RESULT_SUCCESS) {
            currentTune = channel
            tuneAccepted = true
            tuneGeneration++
            beginSiIngestAfterTune()
            TuneOutcome(true, result, channel, tuneGeneration)
        } else {
            currentTune = null
            tuneAccepted = false
            playbackPipeline.stop()
            TuneOutcome(false, result, channel, tuneGeneration, "Tuner.tune に失敗しました result=$result")
        }
    }

    private fun resetBeforeTune() {
        playbackPipeline.stop()
        closeSectionFilters()
        casController?.clearForClearService()
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
        listOf(WellKnownSectionPid.PAT, WellKnownSectionPid.CAT, WellKnownSectionPid.NIT, WellKnownSectionPid.SDT_BAT, WellKnownSectionPid.EIT)
            .forEach { openSectionFilterOnController(it, generation) }
        Log.d(LogTags.TIS, "初期 section filter を開きます inputId=$inputId pids=${sectionFilterHandles.keys} generation=$generation")
    }

    fun openSectionFilters() = openInitialSectionFilters()
    fun openProgramMapFilter(pmtPid: Int): SectionFilterHandle = openSectionFilter(pmtPid)
    fun openEcmFilter(ecmPid: Int): SectionFilterHandle = openSectionFilter(ecmPid)
    fun openEmmFilter(emmPid: Int): SectionFilterHandle = openSectionFilter(emmPid)

    fun openSectionFilter(pid: Int, generation: Long = tuneGeneration): SectionFilterHandle = callOnController { openSectionFilterOnController(pid, generation) }

    private fun openSectionFilterOnController(pid: Int, generation: Long = tuneGeneration): SectionFilterHandle {
        require(pid in 0..0x1fff) { "PID が範囲外です: $pid" }
        if (!tuneAccepted) return UnavailableSectionFilterHandle(pid, "tune未受付")
        sectionFilterHandles[pid]?.let { existing ->
            if (existing.isOpen) return existing
            sectionFilterHandles.remove(pid)
        }
        val token = nextSectionFilterToken++
        val handle = createSectionFilter(pid, generation, token)
        if (handle.isOpen) {
            sectionFilterHandles[pid] = handle
            sectionFilterTokens[pid] = token
        }
        return handle
    }

    private fun createSectionFilter(pid: Int, generation: Long, filterToken: Long): SectionFilterHandle {
        val tunerInstance = tuner ?: return UnavailableSectionFilterHandle(pid, "Tuner利用不可")
        val callback = object : FilterCallback {
            override fun onFilterEvent(filter: Filter, events: Array<FilterEvent>) {
                if (generation != tuneGeneration || sectionFilterTokens[pid] != filterToken) return
                events.filterIsInstance<SectionEvent>().forEach { event ->
                    val length = event.dataLength.toLong()
                    if (length <= 0 || length > Int.MAX_VALUE) return@forEach
                    val section = ByteArray(length.toInt())
                    val readResult = runCatching { filter.read(section, 0, section.size.toLong()) }
                    if (readResult.isFailure) {
                        recordSectionReadError(pid, "exception=${readResult.exceptionOrNull()?.message}")
                        return@forEach
                    }
                    val read = readResult.getOrThrow()
                    val generationStillMatches = generation == tuneGeneration && sectionFilterTokens[pid] == filterToken
                    when (sectionReadDecisionForTest(expected = section.size, actual = read, generationMatches = generationStillMatches)) {
                        SectionReadDecision.INGEST -> onSection(pid, section, generation, filterToken)
                        SectionReadDecision.SHORT_READ -> recordSectionShortRead(pid, expected = section.size, actual = read)
                        SectionReadDecision.READ_ERROR -> recordSectionReadError(pid, "read=$read expected=${section.size}")
                        SectionReadDecision.STALE_GENERATION -> Unit
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
            .setCrcEnabled(true)
            .setRepeat(true)
            .setRaw(false)
            .setBitWidthOfLengthField(12)
            .build()
        val config = TsFilterConfiguration.builder().setTpid(pid).setSettings(settings).build()
        val configureResult = filter.configure(config)
        if (configureResult != Tuner.RESULT_SUCCESS) {
            runCatching { filter.close() }
            return UnavailableSectionFilterHandle(pid, "configure失敗-$configureResult")
        }
        val startResult = filter.start()
        if (startResult != Tuner.RESULT_SUCCESS) {
            runCatching { filter.close() }
            return UnavailableSectionFilterHandle(pid, "start失敗-$startResult")
        }
        return TunerSectionFilterHandle(pid, filter, generation, filterToken)
    }

    private fun recordSectionShortRead(pid: Int, expected: Int, actual: Int) {
        sectionShortReadCounters[pid] = (sectionShortReadCounters[pid] ?: 0) + 1
        Log.w(LogTags.TIS, "section short read を破棄します inputId=$inputId pid=$pid expected=$expected actual=$actual count=${sectionShortReadCounters[pid]}")
    }

    private fun recordSectionReadError(pid: Int, detail: String) {
        sectionReadErrorCounters[pid] = (sectionReadErrorCounters[pid] ?: 0) + 1
        Log.w(LogTags.TIS, "section read 失敗を破棄します inputId=$inputId pid=$pid detail=$detail count=${sectionReadErrorCounters[pid]}")
    }

    fun sectionReadDiagnosticsForTest(): Map<Int, Pair<Int, Int>> =
        (sectionShortReadCounters.keys + sectionReadErrorCounters.keys).associateWith { pid ->
            (sectionShortReadCounters[pid] ?: 0) to (sectionReadErrorCounters[pid] ?: 0)
        }

    fun closeSectionFilter(pid: Int): Unit = callOnController { closeSectionFilterOnController(pid) }

    private fun closeSectionFilterOnController(pid: Int) {
        sectionFilterTokens.remove(pid)
        sectionFilterHandles.remove(pid)?.close()
    }

    fun closeSectionFilters(): Unit = callOnController { closeSectionFiltersOnController() }

    private fun closeSectionFiltersOnController() {
        val handles = sectionFilterHandles.values.toList()
        sectionFilterHandles.clear()
        sectionFilterTokens.clear()
        dynamicPmtPids.clear()
        dynamicEcmPids.clear()
        dynamicEmmPids.clear()
        handles.forEach { it.close() }
    }

    fun openDynamicFiltersFromCurrentSi(pmtPids: Iterable<Int>, ecmPids: Iterable<Int>, emmPids: Iterable<Int>) =
        updateDynamicSectionFilters(pmtPids.toSet(), ecmPids.toSet(), emmPids.toSet(), tuneGeneration)

    fun updateDynamicSectionFiltersForService(
        serviceKey: ServiceKey,
        pmtPids: Set<Int>,
        ecmPids: Set<Int>,
        emmPids: Set<Int>,
        generation: Long,
    ): Unit = callOnController {
        if (generation != tuneGeneration || currentTune?.serviceKey != serviceKey) return@callOnController
        updateDynamicSectionFiltersOnController(pmtPids, ecmPids, emmPids, generation)
    }

    fun updateDynamicSectionFilters(pmtPids: Set<Int>, ecmPids: Set<Int>, emmPids: Set<Int>, generation: Long = tuneGeneration): Unit =
        callOnController { updateDynamicSectionFiltersOnController(pmtPids, ecmPids, emmPids, generation) }

    private fun updateDynamicSectionFiltersOnController(pmtPids: Set<Int>, ecmPids: Set<Int>, emmPids: Set<Int>, generation: Long = tuneGeneration) {
        if (!tuneAccepted || generation != tuneGeneration) return
        replaceDynamicPidSet(dynamicPmtPids, pmtPids) { openProgramMapFilter(it) }
        replaceDynamicPidSet(dynamicEcmPids, ecmPids) { openEcmFilter(it) }
        replaceDynamicPidSet(dynamicEmmPids, emmPids) { openEmmFilter(it) }
    }

    fun updateCasMetadata(metadata: List<CaMetadata>): CasController.UpdateResult? = casController?.let { controller ->
        if (metadata.isEmpty()) {
            controller.clearForClearService()
            CasController.UpdateResult(emptyList(), emptySet(), emptySet())
        } else {
            controller.updateFromCaMetadata(metadata, createDescramblerBridge())
        }
    }

    private fun replaceDynamicPidSet(current: MutableSet<Int>, next: Set<Int>, opener: (Int) -> SectionFilterHandle) {
        val sanitized = next.filter { it in 0..0x1fff }.toSet()
        (current - sanitized).toList().forEach { pid ->
            current.remove(pid)
            if (pid !in initialPids()) closeSectionFilter(pid)
        }
        (sanitized - current).forEach { pid ->
            val handle = opener(pid)
            if (handle.isOpen) current += pid
        }
    }

    private fun initialPids(): Set<Int> = setOf(WellKnownSectionPid.PAT, WellKnownSectionPid.CAT, WellKnownSectionPid.NIT, WellKnownSectionPid.SDT_BAT, WellKnownSectionPid.EIT)

    fun onSection(pid: Int, section: ByteArray, generation: Long = tuneGeneration, filterToken: Long? = sectionFilterTokens[pid]): Unit =
        callOnController { onSectionOnController(pid, section, generation, filterToken) }

    private fun onSectionOnController(pid: Int, section: ByteArray, generation: Long = tuneGeneration, filterToken: Long? = sectionFilterTokens[pid]) {
        require(pid in 0..0x1fff) { "PID が範囲外です: $pid" }
        if (generation != tuneGeneration || sectionFilterTokens[pid] != filterToken) return
        val result = sectionIngestController?.onSection(pid, section)
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
        // 実際の service snapshot は MaleicacidLiveSession と AribSiEngine が管理する。
        // この hook は section 取り込み後の callback 用であり、視聴可能状態を主張しない。
    }

    fun selectAvStreams(
        serviceKey: ServiceKey,
        pcrPid: Int?,
        streams: List<AribElementaryStream>,
        preferredAudioTrackId: String? = null,
    ): AvStreamSelection {
        val video = streams.firstOrNull { it.streamType in VIDEO_STREAM_TYPES }
        val audioCandidates = streams.filter { it.streamType in AUDIO_STREAM_TYPES }
        val audio = preferredAudioTrackId?.let { wanted -> audioCandidates.firstOrNull { trackIdForAudio(it) == wanted } } ?: audioCandidates.firstOrNull()
        return AvStreamSelection(serviceKey, pcrPid, video, audio)
    }

    fun tracksFor(streams: List<AribElementaryStream>): List<TisTrack> = buildList {
        streams.filter { it.streamType in VIDEO_STREAM_TYPES }.forEach { stream ->
            add(TisTrack(trackIdForVideo(stream), android.media.tv.TvTrackInfo.TYPE_VIDEO, stream.elementaryPid, stream.streamType, stream.componentTag, stream.languageCodes.firstOrNull()))
        }
        streams.filter { it.streamType in AUDIO_STREAM_TYPES }.forEach { stream ->
            add(TisTrack(trackIdForAudio(stream), android.media.tv.TvTrackInfo.TYPE_AUDIO, stream.elementaryPid, stream.streamType, stream.componentTag, stream.languageCodes.firstOrNull()))
        }
    }

    fun trackIdForVideo(stream: AribElementaryStream): String = trackIdForVideoStream(stream)
    fun trackIdForAudio(stream: AribElementaryStream): String = trackIdForAudioStream(stream)

    fun startPlayback(selection: AvStreamSelection): PlaybackPipeline.StartResult? {
        val channel = currentTune ?: return null
        val tunerInstance = tuner ?: return null
        return playbackPipeline.start(tunerInstance, channel, selection)
    }

    fun switchAudioTrack(selection: AvStreamSelection): PlaybackPipeline.AudioSwitchResult? {
        if (currentTune == null) return null
        val tunerInstance = tuner ?: return null
        return playbackPipeline.switchAudio(tunerInstance, selection)
    }

    fun stopPlayback() {
        playbackPipeline.stop()
    }

    fun currentResolvedChannel(): ResolvedChannel? = currentTune
    fun currentGeneration(): Long = tuneGeneration
    fun isTuneRequestAccepted(): Boolean = tuneAccepted

    private fun resolveChannel(channelUri: Uri): Result<ResolvedChannel> = runCatching {
        val projection = arrayOf(
            TvContract.Channels.COLUMN_INPUT_ID,
            TvContract.Channels.COLUMN_ORIGINAL_NETWORK_ID,
            TvContract.Channels.COLUMN_TRANSPORT_STREAM_ID,
            TvContract.Channels.COLUMN_SERVICE_ID,
            TvContract.Channels.COLUMN_DISPLAY_NAME,
            TvContract.Channels.COLUMN_DISPLAY_NUMBER,
            TvContract.Channels.COLUMN_INTERNAL_PROVIDER_DATA,
        )
        context.contentResolver.query(channelUri, projection, null, null, null)?.use { cursor ->
            if (!cursor.moveToFirst()) error("channel 行が見つかりません uri=$channelUri")
            val rowInputId = cursor.getString(0)
            require(rowInputId == inputId) { "inputId が一致しません row=$rowInputId session=$inputId" }
            val key = ServiceKey(cursor.getInt(1), cursor.getInt(2), cursor.getInt(3))
            val displayName = cursor.getString(4) ?: "service-${key.serviceId}"
            val displayNumber = cursor.getString(5) ?: key.serviceId.toString()
            val providerData = cursor.getBlob(6)?.let { String(it, Charsets.UTF_8) }.orEmpty()
            val map = parseInternalProviderData(providerData)
            val deliverySystem = map["system"] ?: error("channel provider data に delivery system がありません")
            val frequencyHz = map["frequencyHz"]?.toLongOrNull() ?: error("channel provider data に frequencyHz がありません")
            require(frequencyHz > 0) { "frequencyHz が不正です: $frequencyHz" }
            ResolvedChannel(
                uri = channelUri,
                inputId = rowInputId,
                serviceKey = key,
                displayName = displayName,
                displayNumber = displayNumber,
                deliverySystem = deliverySystem,
                frequencyHz = frequencyHz,
                streamSelector = StreamSelector.fromStored(map["streamSelectorType"], map["streamSelectorValue"]),
                physicalChannel = map["physicalChannel"]?.toIntOrNull(),
                backendHint = map["backendHint"],
                satelliteBand = map["satelliteBand"],
            )
        } ?: error("query が null cursor を返しました uri=$channelUri")
    }

    private fun buildFrontendSettings(channel: ResolvedChannel): Result<FrontendSettings> = runCatching {
        when (channel.deliverySystem) {
            ChannelRecord.DELIVERY_SYSTEM_ISDB_T -> IsdbtFrontendSettings.builder()
                .setFrequencyLong(channel.frequencyHz)
                .setBandwidth(IsdbtFrontendSettings.BANDWIDTH_6MHZ)
                .build()
            ChannelRecord.DELIVERY_SYSTEM_ISDB_S -> {
                require(channel.backendHint != "earth_pt1" || channel.streamSelector.type != StreamSelectorType.RELATIVE) { "earth_pt1 BS では相対 TS 番号を使えません" }
                require(channel.satelliteBand != "110CS" || channel.streamSelector.type == StreamSelectorType.NONE) { "CS110 は TSID/relative stream selector による frontend 選局を行いません" }
                IsdbsFrontendSettings.builder()
                .setFrequencyLong(channel.frequencyHz)
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

    private fun parseInternalProviderData(data: String): Map<String, String> = data.split(';')
        .mapNotNull { part ->
            val i = part.indexOf('=')
            if (i <= 0) null else part.substring(0, i) to part.substring(i + 1)
        }.toMap()

    fun release() {
        playbackPipeline.release()
        closeSectionFilters()
        casController?.close()
        casController = null
        descramblerBridge = null
        sectionIngestController = null
        tuner?.close()
        tuner = null
        sectionExecutor.shutdownNow()
        Log.i(LogTags.TIS, "Tuner を解放します inputId=$inputId sessionId=$tvInputSessionId")
    }

    override fun close() = release()

    companion object {
        private const val SECTION_FILTER_BUFFER_BYTES = 64 * 1024L
        private val VIDEO_STREAM_TYPES = setOf(0x02, 0x1b)
        private val AUDIO_STREAM_TYPES = setOf(0x03, 0x04, 0x0f)

        fun isR51SupportedVideoStreamTypeForTest(streamType: Int): Boolean = streamType in VIDEO_STREAM_TYPES

        fun selectVideoForTest(streams: List<AribElementaryStream>): AribElementaryStream? =
            streams.firstOrNull { it.streamType in VIDEO_STREAM_TYPES }

        fun sectionReadDecisionForTest(expected: Int, actual: Int, generationMatches: Boolean): SectionReadDecision = when {
            !generationMatches -> SectionReadDecision.STALE_GENERATION
            expected <= 0 -> SectionReadDecision.READ_ERROR
            actual == expected -> SectionReadDecision.INGEST
            actual > 0 -> SectionReadDecision.SHORT_READ
            else -> SectionReadDecision.READ_ERROR
        }

        fun trackIdForVideoStream(stream: AribElementaryStream): String = "video:${stream.elementaryPid}"

        fun trackIdForAudioStream(stream: AribElementaryStream): String =
            stream.componentTag?.let { "audio:${stream.elementaryPid}:$it" } ?: "audio:${stream.elementaryPid}"

        fun isCs110SelectorAllowedForTest(satelliteBand: String?, selector: StreamSelector): Boolean =
            satelliteBand != "110CS" || selector.type == StreamSelectorType.NONE

        fun isSelectableTrackForTest(type: Int, trackId: String?, tracks: List<TisTrack>): Boolean = when (type) {
            android.media.tv.TvTrackInfo.TYPE_AUDIO -> trackId != null && tracks.any { it.type == type && it.id == trackId }
            android.media.tv.TvTrackInfo.TYPE_VIDEO -> trackId != null && tracks.firstOrNull { it.type == type }?.id == trackId
            else -> false
        }
    }
}

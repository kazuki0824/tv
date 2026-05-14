package com.maleicacid.tvinput.tis

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.media.tv.TvContentRating
import android.media.tv.TvInputService
import android.media.tv.TvInputManager
import android.media.tv.TvTrackInfo
import android.net.Uri
import android.os.Build
import android.view.Surface
import android.view.View
import java.util.concurrent.Callable
import java.util.concurrent.ExecutionException
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean
import com.maleicacid.tvinput.aribsi.AribService
import com.maleicacid.tvinput.aribsi.AribSiEngine
import com.maleicacid.tvinput.aribsi.PmtCatCaMetadataMapper
import com.maleicacid.tvinput.aribsi.SectionIngestController
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.db.ProgramRecord

class MaleicacidLiveSession(
    context: Context,
    private val inputId: String,
    private val sessionId: String? = null,
    private val attributionSource: android.content.AttributionSource? = null,
) : TvInputService.Session(context) {
    private val appContext = context.applicationContext
    private val tvInputManager: TvInputManager? = appContext.getSystemService(TvInputManager::class.java)
    private val aribSiEngine = AribSiEngine(context)
    private val sectionIngestController = SectionIngestController(aribSiEngine)
    private val tunerController = TunerController(context, inputId, attributionSource = attributionSource, sessionId = sessionId)
    private val casController = CasController()
    private val caMapper = PmtCatCaMetadataMapper()
    private val eventModelMapper = com.maleicacid.tvinput.aribsi.EventModelMapper()
    private val tvProviderWriter = TvProviderWriter(context, inputId)
    private val currentProgramRatingResolver = CurrentProgramRatingResolver(appContext)
    private val programPublishCoordinator = ProgramPublishCoordinator(tvProviderWriter)
    private val releaseOnce = AtomicBoolean(false)
    @Volatile private var sessionExecutorThread: Thread? = null
    private val sessionExecutor: ExecutorService = Executors.newSingleThreadExecutor { runnable ->
        Thread(runnable, "maleicacid-live-session-${sessionId ?: "legacy"}").also { thread ->
            thread.isDaemon = true
            sessionExecutorThread = thread
        }
    }
    private var surface: Surface? = null
    private var currentChannelUri: Uri? = null
    private var currentService: ServiceKey? = null
    private var currentGeneration: Long = 0L
    private var captionEnabled: Boolean = false
    private var streamVolume: Float = 1.0f
    private val playbackStartGate = PlaybackStartGate()
    private var currentPlaybackSignature: AvPlaybackSignature? = null
    private var pendingPlaybackSignature: AvPlaybackSignature? = null
    private var latestService: AribService? = null
    private val latestVideoMetadataByProgramKey = linkedMapOf<String, PlaybackPipeline.VideoFormatInfo>()
    private var preferredAudioTrackId: String? = null
    private var selectedSubtitleTrackId: String? = null
    private var currentTrackSignature: Set<String> = emptySet()
    private val captionOverlayView = CaptionOverlayView(appContext)
    private val captionController = AribCaptionController(captionOverlayView)
    private val unblockedContentKeys = linkedSetOf<String>()
    private var currentUnblockProgramIdentityKey: String? = null
    private data class BlockedContent(val rating: TvContentRating, val unblockKey: String)
    private val parentalControlReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context?, intent: Intent?) {
            enqueueSessionAction { reevaluateParentalControls() }
        }
    }

    init {
        tunerController.setSectionIngestController(sectionIngestController)
        tunerController.setCasController(casController)
        tunerController.setOnSectionIngestedCallback { enqueueSessionAction { refreshDynamicSiAndCasFilters() } }
        tunerController.setPlaybackCallbacks(
            onVideoAvailable = { enqueueSessionAction { handleFirstFrameAvailable() } },
            onVideoUnavailable = { reason -> enqueueSessionAction { handlePlaybackUnavailable(reason) } },
        )
        tunerController.setOnVideoFormatDiscoveredCallback { info -> enqueueSessionAction { updateCurrentProgramVideoMetadata(info) } }
        tunerController.setOnSubtitlePesCallback { trackId, pesData, ptsMillis ->
            captionController.onPesData(trackId, pesData, ptsMillis)
        }
        runCatching { setOverlayViewEnabled(true) }
        ChannelScanManager.registerLiveSession()
        registerParentalControlReceiver()
    }

    private fun <T> runOnSessionExecutorBlocking(action: () -> T): T {
        if (Thread.currentThread() == sessionExecutorThread) return action()
        val future = sessionExecutor.submit(Callable<T> { action() })
        return try {
            future.get()
        } catch (e: InterruptedException) {
            Thread.currentThread().interrupt()
            throw RuntimeException("session executor interrupted", e)
        } catch (e: ExecutionException) {
            val cause = e.cause ?: e
            when (cause) {
                is RuntimeException -> throw cause
                is Error -> throw cause
                else -> throw RuntimeException(cause)
            }
        }
    }

    private fun enqueueSessionAction(action: () -> Unit) {
        if (releaseOnce.get()) return
        if (Thread.currentThread() == sessionExecutorThread) {
            action()
            return
        }
        runCatching {
            sessionExecutor.execute {
                if (!releaseOnce.get()) action()
            }
        }
    }

    override fun onSetSurface(surface: Surface?): Boolean = runOnSessionExecutorBlocking {
        onSetSurfaceOnSessionExecutor(surface)
    }

    private fun onSetSurfaceOnSessionExecutor(surface: Surface?): Boolean {
        this.surface = surface
        tunerController.setSurface(surface)
        if (surface == null) {
            currentPlaybackSignature = null
            pendingPlaybackSignature = null
            playbackStartGate.reset()
            tunerController.stopPlayback()
            notifyVideoUnavailable(TvInputManager.VIDEO_UNAVAILABLE_REASON_UNKNOWN)
        } else {
            playbackStartGate.allowRetry()
            refreshDynamicSiAndCasFilters()
        }
        return true
    }

    override fun onCreateOverlayView(): View? = captionOverlayView

    override fun onSetStreamVolume(volume: Float) {
        enqueueSessionAction { onSetStreamVolumeOnSessionExecutor(volume) }
    }

    private fun onSetStreamVolumeOnSessionExecutor(volume: Float) {
        streamVolume = volume.coerceIn(0.0f, 1.0f)
        tunerController.setStreamVolume(streamVolume)
    }

    override fun onSetCaptionEnabled(enabled: Boolean) {
        enqueueSessionAction {
            captionEnabled = enabled
            captionController.setEnabled(enabled)
            latestService?.let { updateSubtitleSelection(tunerController.tracksFor(it.streams)) }
        }
    }

    override fun onTune(channelUri: Uri?): Boolean = runOnSessionExecutorBlocking {
        onTuneOnSessionExecutor(channelUri)
    }

    private fun onTuneOnSessionExecutor(channelUri: Uri?): Boolean {
        if (channelUri == null) return false
        notifyVideoUnavailable(TvInputManager.VIDEO_UNAVAILABLE_REASON_TUNING)
        aribSiEngine.reset()
        currentPlaybackSignature = null
        pendingPlaybackSignature = null
        currentService = null
        currentGeneration = 0L
        currentChannelUri = channelUri
        latestService = null
        latestVideoMetadataByProgramKey.clear()
        unblockedContentKeys.clear()
        currentUnblockProgramIdentityKey = null
        programPublishCoordinator.reset()
        preferredAudioTrackId = null
        selectedSubtitleTrackId = null
        captionController.setEnabled(captionEnabled)
        captionController.selectTrack(null)
        currentTrackSignature = emptySet()
        playbackStartGate.reset()
        val outcome = tunerController.tuneForLive(channelUri)
        if (!outcome.success || outcome.channel == null) {
            notifyVideoUnavailable(TvInputManager.VIDEO_UNAVAILABLE_REASON_UNKNOWN)
            return false
        }
        currentService = outcome.channel.serviceKey
        currentGeneration = outcome.generation
        refreshDynamicSiAndCasFilters()
        return true
    }

    private fun mapUnavailableReason(unavailable: PlaybackPipeline.PlaybackUnavailable): Int = when (unavailable.reason) {
        PlaybackPipeline.PlaybackUnavailableReason.SURFACE_NOT_SET,
        PlaybackPipeline.PlaybackUnavailableReason.SURFACE_DETACHED,
        PlaybackPipeline.PlaybackUnavailableReason.FIRST_FRAME_TIMEOUT -> TvInputManager.VIDEO_UNAVAILABLE_REASON_BUFFERING
        PlaybackPipeline.PlaybackUnavailableReason.CAS_NO_KEY -> TvInputManager.VIDEO_UNAVAILABLE_REASON_CAS_UNKNOWN
        else -> TvInputManager.VIDEO_UNAVAILABLE_REASON_UNKNOWN
    }

    private fun refreshDynamicSiAndCasFilters() {
        val serviceKey = currentService ?: return
        val transaction = aribSiEngine.takeProgramPublishSnapshot(takeUpdateWindows = false)
        val service = transaction.services.firstOrNull { it.serviceKey == serviceKey }
        val pmtPids = transaction.pmtPidsForSectionFilters.filter { it in 0..0x1fff }.toSet()
        val allCaMetadata = if (ENABLE_CAS_ORCHESTRATION) transaction.caMetadataForCasDiscovery else emptyList()
        val serviceScopedCa = allCaMetadata.filter {
            it.serviceKey == serviceKey && it.source != com.maleicacid.tvinput.aribsi.CaMetadataSource.CAT
        }
        val catCa = allCaMetadata.filter { it.source == com.maleicacid.tvinput.aribsi.CaMetadataSource.CAT }
        val expanded = caMapper.expandProgramLevelToElementaryStreams(
            serviceScopedCa + catCa,
            transaction.servicesForCasDiscovery,
        )
        val serviceCaMetadata = expanded.filter { it.serviceKey == serviceKey }
        val caMetadata = expanded.filter { it.serviceKey == null || it.serviceKey == serviceKey }
        val ecmPids = caMetadata.mapNotNull { it.ecmPid }.filter { it in 0..0x1fff }.toSet()
        val emmPids = caMetadata.mapNotNull { it.emmPid }.filter { it in 0..0x1fff }.toSet()
        tunerController.updateDynamicSectionFiltersForService(serviceKey, pmtPids, ecmPids, emmPids, currentGeneration)

        publishLiveProgramsForCurrentService()
        refreshCurrentProgramRatingState()
        if (caMetadata.isEmpty()) {
            casController.clearForClearService()
        } else {
            val bridge = if (serviceScopedCa.isEmpty()) null else tunerController.createDescramblerBridge()
            val casResult = casController.updateFromCaMetadata(caMetadata, bridge)
            val blockingCasError = serviceCaMetadata.isNotEmpty() && casResult.diagnostics.any { it.state == CasController.State.ERROR }
            if (blockingCasError) {
                currentPlaybackSignature = null
                pendingPlaybackSignature = null
                playbackStartGate.reset()
                tunerController.stopPlayback()
                notifyVideoUnavailable(TvInputManager.VIDEO_UNAVAILABLE_REASON_CAS_UNKNOWN)
                return
            }
            if (serviceCaMetadata.isNotEmpty()) {
                currentPlaybackSignature = null
                pendingPlaybackSignature = null
                playbackStartGate.reset()
                tunerController.stopPlayback()
                notifyVideoUnavailable(mapUnavailableReason(PlaybackPipeline.PlaybackUnavailable(PlaybackPipeline.PlaybackUnavailableReason.CAS_NO_KEY, "r51 CAS placeholder cannot provide real key token")))
                return
            }
        }
        if (service != null) {
            latestService = service
            updateTracks(service)
            maybeStartPlayback(service)
        }
    }

    private fun maybeStartPlayback(service: AribService): Boolean {
        val blocked = blockedContentRating()
        if (blocked != null) {
            stopPlaybackForBlockedContent(blocked)
            return false
        } else {
            notifyContentAllowed()
        }
        val selection = tunerController.selectAvStreams(service.serviceKey, service.pcrPid, service.streams, preferredAudioTrackId, selectedSubtitleTrackId)
        if (shouldRejectPlaybackSelectionWithoutVideoForTest(selection)) {
            currentPlaybackSignature = null
            pendingPlaybackSignature = null
            playbackStartGate.reset()
            tunerController.stopPlayback()
            notifyVideoUnavailable(mapUnavailableReason(PlaybackPipeline.PlaybackUnavailable(
                PlaybackPipeline.PlaybackUnavailableReason.UNSUPPORTED_VIDEO_STREAM,
                "r51対象の映像ESがありません service=${service.serviceKey}",
            )))
            return false
        }
        val signature = playbackSignatureFor(service, selection) ?: return false
        if (!playbackStartGate.shouldAttempt(signature)) {
            return currentPlaybackSignature == signature && playbackStartGate.isStartedSignature(signature)
        }
        val previousSignature = currentPlaybackSignature
        val shouldStopBeforeRestart = previousSignature != null && previousSignature != signature && playbackStartGate.isStartedSignature(previousSignature)
        playbackStartGate.recordAttempt(signature)
        val result = tunerController.startPlayback(selection)
        if (result == null) {
            pendingPlaybackSignature = null
            currentPlaybackSignature = null
            playbackStartGate.recordResult(signature, startedVideo = false)
            notifyVideoUnavailable(TvInputManager.VIDEO_UNAVAILABLE_REASON_UNKNOWN)
            return false
        }
        if (result.firstFramePending == true) {
            pendingPlaybackSignature = signature
            currentPlaybackSignature = null
            return false
        }
        val started = result.startedVideo
        playbackStartGate.recordResult(signature, started)
        if (started) {
            currentPlaybackSignature = signature
            pendingPlaybackSignature = null
            return true
        }
        if (shouldStopBeforeRestart) currentPlaybackSignature = null
        pendingPlaybackSignature = null
        return false
    }

    private fun playbackSignatureFor(
        service: AribService,
        selection: TunerController.AvStreamSelection,
    ): AvPlaybackSignature? {
        val video = selection.video ?: return null
        val audio = selection.audio
        return AvPlaybackSignature(
            serviceKey = service.serviceKey,
            pcrPid = selection.pcrPid,
            videoPid = video.elementaryPid,
            videoStreamType = video.streamType,
            audioPid = audio?.elementaryPid,
            audioStreamType = audio?.streamType,
            subtitlePid = selection.subtitle?.elementaryPid,
            subtitleDataComponentId = selection.subtitle?.dataComponentId,
            clear = true,
            keyTokenAvailable = false,
        )
    }

    override fun onSelectTrack(type: Int, trackId: String?): Boolean = runOnSessionExecutorBlocking {
        onSelectTrackOnSessionExecutor(type, trackId)
    }

    private fun onSelectTrackOnSessionExecutor(type: Int, trackId: String?): Boolean {
        val service = latestService ?: return false
        val tracks = tunerController.tracksFor(service.streams)
        return when (type) {
            TvTrackInfo.TYPE_AUDIO -> {
                if (trackId == null || tracks.none { it.type == TvTrackInfo.TYPE_AUDIO && it.id == trackId }) return false
                val previousAudioTrackId = preferredAudioTrackId
                val previousSignature = currentPlaybackSignature ?: return false
                preferredAudioTrackId = trackId
                val selection = tunerController.selectAvStreams(service.serviceKey, service.pcrPid, service.streams, preferredAudioTrackId, selectedSubtitleTrackId)
                val signature = playbackSignatureFor(service, selection) ?: run {
                    preferredAudioTrackId = previousAudioTrackId
                    currentPlaybackSignature = previousSignature
                    return false
                }
                val switched = tunerController.switchAudioTrack(selection)?.switchedAudio == true
                if (switched) {
                    currentPlaybackSignature = signature
                    pendingPlaybackSignature = null
                    playbackStartGate.recordAttempt(signature)
                    playbackStartGate.recordResult(signature, startedVideo = true)
                    notifyTrackSelected(TvTrackInfo.TYPE_AUDIO, trackId)
                    true
                } else {
                    preferredAudioTrackId = previousAudioTrackId
                    currentPlaybackSignature = previousSignature
                    false
                }
            }
            TvTrackInfo.TYPE_VIDEO -> {
                if (trackId == null) return false
                val currentVideo = tracks.firstOrNull { it.type == TvTrackInfo.TYPE_VIDEO }?.id
                if (trackId == currentVideo) {
                    notifyTrackSelected(TvTrackInfo.TYPE_VIDEO, trackId)
                    true
                } else {
                    false
                }
            }
            TvTrackInfo.TYPE_SUBTITLE -> {
                if (trackId == null) {
                    selectedSubtitleTrackId = null
                    captionController.selectTrack(null)
                    notifyTrackSelected(TvTrackInfo.TYPE_SUBTITLE, null)
                    playbackStartGate.allowRetry()
                    maybeStartPlayback(service)
                    true
                } else {
                    val subtitle = tracks.firstOrNull { it.type == TvTrackInfo.TYPE_SUBTITLE && it.id == trackId } ?: return false
                    selectedSubtitleTrackId = subtitle.id
                    captionController.selectTrack(subtitle)
                    if (captionEnabled) notifyTrackSelected(TvTrackInfo.TYPE_SUBTITLE, subtitle.id)
                    playbackStartGate.allowRetry()
                    maybeStartPlayback(service)
                    true
                }
            }
            else -> false
        }
    }

    private fun updateTracks(service: AribService) {
        val tracks = tunerController.tracksFor(service.streams)
        val signature = tracks.map { track ->
            val audioComponentType = if (track.type == TvTrackInfo.TYPE_AUDIO) track.componentType ?: -1 else -1
            val videoComponentType = if (track.type == TvTrackInfo.TYPE_VIDEO) track.componentType ?: -1 else -1
            val subtitleDataComponentId = if (track.type == TvTrackInfo.TYPE_SUBTITLE) track.dataComponentId ?: -1 else -1
            listOf(
                track.id,
                track.type.toString(),
                track.pid.toString(),
                track.streamType.toString(),
                track.componentTag?.toString() ?: "-1",
                track.language.orEmpty(),
                audioComponentType.toString(),
                videoComponentType.toString(),
                subtitleDataComponentId.toString(),
            ).joinToString("|")
        }.toSet()
        if (signature != currentTrackSignature) {
            currentTrackSignature = signature
            notifyTracksChanged(tracks.map { track ->
                val builder = TvTrackInfo.Builder(track.type, track.id)
                LanguageCodeNormalizer.normalizeForTvTrackLanguage(track.language)?.let { language ->
                    builder.setLanguage(language)
                }
                builder.build()
            })
        }
        tracks.firstOrNull { it.type == TvTrackInfo.TYPE_VIDEO }?.let { notifyTrackSelected(TvTrackInfo.TYPE_VIDEO, it.id) }
        val selectedAudio = preferredAudioTrackId?.let { wanted -> tracks.firstOrNull { it.id == wanted && it.type == TvTrackInfo.TYPE_AUDIO } }
            ?: tracks.firstOrNull { it.type == TvTrackInfo.TYPE_AUDIO }
        selectedAudio?.let {
            preferredAudioTrackId = it.id
            notifyTrackSelected(TvTrackInfo.TYPE_AUDIO, it.id)
        }
        updateSubtitleSelection(tracks)
    }

    private fun updateSubtitleSelection(tracks: List<TunerController.TisTrack>) {
        val selected = selectedSubtitleTrackId?.let { wanted -> tracks.firstOrNull { it.type == TvTrackInfo.TYPE_SUBTITLE && it.id == wanted } }
            ?: tracks.firstOrNull { it.type == TvTrackInfo.TYPE_SUBTITLE }
        selectedSubtitleTrackId = selected?.id
        captionController.selectTrack(selected)
        notifyTrackSelected(TvTrackInfo.TYPE_SUBTITLE, if (captionEnabled) selected?.id else null)
    }

    private fun handleFirstFrameAvailable() {
        val signature = pendingPlaybackSignature ?: currentPlaybackSignature
        if (signature != null) {
            playbackStartGate.recordResult(signature, startedVideo = true)
            currentPlaybackSignature = signature
            pendingPlaybackSignature = null
        }
        val blocked = blockedContentRating()
        if (blocked != null) {
            stopPlaybackForBlockedContent(blocked)
            return
        }
        notifyContentAllowed()
        notifyVideoAvailable()
    }

    private fun handlePlaybackUnavailable(reason: PlaybackPipeline.PlaybackUnavailable) {
        if (reason.reason == PlaybackPipeline.PlaybackUnavailableReason.AUDIO_UNAVAILABLE ||
            reason.reason == PlaybackPipeline.PlaybackUnavailableReason.AUDIO_FILTER_NOT_STARTED ||
            reason.reason == PlaybackPipeline.PlaybackUnavailableReason.UNSUPPORTED_AUDIO_STREAM) {
            android.util.Log.w(com.maleicacid.tvinput.common.LogTags.TIS, "audio unavailable は video unavailable として通知しません reason=${reason.reason} detail=${reason.detail}")
            return
        }
        if (reason.reason == PlaybackPipeline.PlaybackUnavailableReason.FIRST_FRAME_TIMEOUT ||
            reason.reason == PlaybackPipeline.PlaybackUnavailableReason.VIDEO_CODEC_ERROR ||
            reason.reason == PlaybackPipeline.PlaybackUnavailableReason.CODEC_CONFIG_TIMEOUT ||
            reason.reason == PlaybackPipeline.PlaybackUnavailableReason.VIDEO_FILTER_NOT_STARTED) {
            pendingPlaybackSignature = null
            currentPlaybackSignature = null
            playbackStartGate.allowRetry()
        }
        notifyVideoUnavailable(mapUnavailableReason(reason))
    }

    private fun stopPlaybackForBlockedContent(blocked: BlockedContent) {
        notifyContentBlocked(blocked.rating)
        currentPlaybackSignature = null
        pendingPlaybackSignature = null
        playbackStartGate.reset()
        tunerController.stopPlayback()
    }

    private fun blockedContentRating(): BlockedContent? {
        val manager = tvInputManager ?: return null
        if (!manager.isParentalControlsEnabled) return null
        val ratingSet = currentProgramRatingResolver.resolve(
            channelUri = currentChannelUri,
            serviceKey = currentService,
            latestEvents = aribSiEngine.takeProgramPublishSnapshot(takeUpdateWindows = false).events,
        )
        clearUnblocksIfCurrentProgramChanged(ratingSet)
        return ratingSet.ratingsForBlocking().firstNotNullOfOrNull { rating ->
            val unblockKey = ratingSet.unblockKeyFor(rating)
            if (unblockKey !in unblockedContentKeys && manager.isRatingBlocked(rating)) BlockedContent(rating, unblockKey) else null
        }
    }

    private fun clearUnblocksIfCurrentProgramChanged(ratingSet: CurrentProgramRatingResolver.CurrentProgramRatingSet) {
        currentUnblockProgramIdentityKey = updateUnblockStateForProgramChangeForTest(
            previousIdentityKey = currentUnblockProgramIdentityKey,
            nextIdentityKey = ratingSet.programIdentityKey(),
            unblockedContentKeys = unblockedContentKeys,
        )
    }

    private fun reevaluateParentalControls() {
        val blocked = blockedContentRating()
        if (blocked != null) {
            stopPlaybackForBlockedContent(blocked)
        } else {
            notifyContentAllowed()
            latestService?.let { service ->
                playbackStartGate.allowRetry()
                maybeStartPlayback(service)
            }
        }
    }

    private fun updateCurrentProgramVideoMetadata(info: PlaybackPipeline.VideoFormatInfo) {
        val key = currentService ?: return
        val now = System.currentTimeMillis()
        val channelFallbacks = tvProviderWriter.existingChannelsResult().getOrElse { error ->
            android.util.Log.w(com.maleicacid.tvinput.common.LogTags.TIS, "既存 channel 復元失敗のため video metadata provider-data 更新を中止します", error)
            return
        }
        val transaction = aribSiEngine.takeProgramPublishSnapshot(takeUpdateWindows = false)
        val records = eventModelMapper.toProgramRecords(
            events = transaction.events.filter { event ->
                event.serviceKey == key && now >= event.startTimeMillis && now < event.startTimeMillis + event.durationMillis
            },
            publishabilityByServiceKey = transaction.publishabilityDiagnostics.associateBy { it.serviceKey },
            channelFallbackByServiceKey = channelFallbacks.associateBy { it.serviceKey },
        )
        if (records.isEmpty()) return
        rememberVideoMetadata(records, info)
        publishLivePrograms(applyLatestVideoMetadata(records))
    }

    private fun refreshCurrentProgramRatingState() {
        val ratingSet = currentProgramRatingResolver.resolve(
            channelUri = currentChannelUri,
            serviceKey = currentService,
            latestEvents = aribSiEngine.takeProgramPublishSnapshot(takeUpdateWindows = false).events,
        )
        clearUnblocksIfCurrentProgramChanged(ratingSet)
    }

    private fun publishLiveProgramsForCurrentService() {
        val key = currentService ?: return
        val channelFallbacks = tvProviderWriter.existingChannelsResult().getOrElse { error ->
            android.util.Log.w(com.maleicacid.tvinput.common.LogTags.TIS, "既存 channel 復元失敗のため live Programs publish を中止します", error)
            return
        }
        val transaction = aribSiEngine.takeProgramPublishSnapshot(takeUpdateWindows = false)
        val records = eventModelMapper.toProgramRecords(
            events = transaction.events.filter { it.serviceKey == key },
            publishabilityByServiceKey = transaction.publishabilityDiagnostics.associateBy { it.serviceKey },
            channelFallbackByServiceKey = channelFallbacks.associateBy { it.serviceKey },
        )
        publishLivePrograms(applyLatestVideoMetadata(records))
    }

    private fun rememberVideoMetadata(records: List<ProgramRecord>, info: PlaybackPipeline.VideoFormatInfo) {
        records.forEach { record ->
            latestVideoMetadataByProgramKey[programVideoMetadataKey(record)] = info
        }
    }

    private fun applyLatestVideoMetadata(records: List<ProgramRecord>): List<ProgramRecord> = mergeVideoMetadataForTest(records, latestVideoMetadataByProgramKey)

    private fun publishLivePrograms(records: List<ProgramRecord>) {
        if (records.isEmpty()) return
        val result = programPublishCoordinator.publish(
            mode = ChannelScanController.PublishMode.LIVE_TUNE_REFRESH,
            allPrograms = records,
            allowedServiceKeys = null,
        )
        if (result.failures.isNotEmpty()) {
            android.util.Log.w(com.maleicacid.tvinput.common.LogTags.TIS, "live Programs 更新失敗=${result.failures}")
        }
    }

    private fun registerParentalControlReceiver() {
        val filter = IntentFilter().apply {
            addAction(TvInputManager.ACTION_BLOCKED_RATINGS_CHANGED)
            addAction(TvInputManager.ACTION_PARENTAL_CONTROLS_ENABLED_CHANGED)
        }
        runCatching {
            if (Build.VERSION.SDK_INT >= 33) {
                appContext.registerReceiver(parentalControlReceiver, filter, Context.RECEIVER_NOT_EXPORTED)
            } else {
                @Suppress("DEPRECATION")
                appContext.registerReceiver(parentalControlReceiver, filter)
            }
        }
    }

    private fun unregisterParentalControlReceiver() {
        runCatching { appContext.unregisterReceiver(parentalControlReceiver) }
    }


    override fun onUnblockContent(unblockedRating: TvContentRating?) {
        enqueueSessionAction { onUnblockContentOnSessionExecutor(unblockedRating) }
    }

    private fun onUnblockContentOnSessionExecutor(unblockedRating: TvContentRating?) {
        val rating = unblockedRating ?: return
        val ratingSet = currentProgramRatingResolver.resolve(
            channelUri = currentChannelUri,
            serviceKey = currentService,
            latestEvents = aribSiEngine.takeProgramPublishSnapshot(takeUpdateWindows = false).events,
        )
        clearUnblocksIfCurrentProgramChanged(ratingSet)
        val unblockKey = ratingSet.exactUnblockKeyFor(rating) ?: return
        unblockedContentKeys += unblockKey
        notifyContentAllowed()
        latestService?.let { service ->
            playbackStartGate.allowRetry()
            maybeStartPlayback(service)
        }
    }

    override fun onRelease() {
        if (!releaseOnce.compareAndSet(false, true)) return
        try {
            runOnSessionExecutorBlocking { releaseOnSessionExecutor() }
        } finally {
            sessionExecutor.shutdown()
        }
    }

    private fun releaseOnSessionExecutor() {
        try {
            surface = null
            currentChannelUri = null
            captionEnabled = false
            selectedSubtitleTrackId = null
            captionController.setEnabled(false)
            captionController.selectTrack(null)
            captionController.close()
            unblockedContentKeys.clear()
            currentUnblockProgramIdentityKey = null
            currentPlaybackSignature = null
            pendingPlaybackSignature = null
            playbackStartGate.reset()
            unregisterParentalControlReceiver()
            casController.close()
            tunerController.release()
            aribSiEngine.close()
        } finally {
            ChannelScanManager.unregisterLiveSession()
        }
    }

    companion object {
        private const val ENABLE_CAS_ORCHESTRATION = true

        fun updateUnblockStateForProgramChangeForTest(
            previousIdentityKey: String?,
            nextIdentityKey: String?,
            unblockedContentKeys: MutableSet<String>,
        ): String? {
            if (previousIdentityKey != nextIdentityKey) {
                unblockedContentKeys.clear()
            }
            return nextIdentityKey
        }

        fun shouldRejectPlaybackSelectionWithoutVideoForTest(selection: TunerController.AvStreamSelection): Boolean = selection.video == null

        fun unsupportedLivePlaybackReasonForTest(): Int = TvInputManager.VIDEO_UNAVAILABLE_REASON_UNKNOWN

        fun subtitleSelectionAcceptedForTest(
            trackId: String?,
            tracks: List<TunerController.TisTrack>,
            captionEnabled: Boolean,
        ): Boolean = if (trackId == null) {
            true
        } else {
            captionEnabled && tracks.any { it.type == TvTrackInfo.TYPE_SUBTITLE && it.id == trackId }
        }

        fun audioTrackSelectionAcceptedForTest(
            trackId: String?,
            tracks: List<TunerController.TisTrack>,
            audioSwitchSucceeded: Boolean,
        ): Boolean = trackId != null &&
            tracks.any { it.type == TvTrackInfo.TYPE_AUDIO && it.id == trackId } &&
            audioSwitchSucceeded

        fun preservesExistingPlaybackWhenAudioSwitchFailsForTest(
            previousSignature: AvPlaybackSignature?,
            restoredSignature: AvPlaybackSignature?,
            switchSucceeded: Boolean,
        ): Boolean = !switchSucceeded && previousSignature == restoredSignature

        fun shouldStopPlaybackWhenParentalControlBecomesBlockedForTest(blocked: Boolean): Boolean = blocked

        fun shouldRestartPlaybackAfterParentalControlAllowedForTest(latestServicePresent: Boolean): Boolean = latestServicePresent

        fun parentalBlockUsesNotifyVideoUnavailableForTest(): Boolean = false

        fun casPlaceholderUnavailableReasonForTest(): Int = TvInputManager.VIDEO_UNAVAILABLE_REASON_CAS_UNKNOWN

        fun videoMetadataProgramsForTest(
            events: List<com.maleicacid.tvinput.aribsi.AribEvent>,
            serviceKey: ServiceKey,
            nowMillis: Long,
            info: PlaybackPipeline.VideoFormatInfo,
        ): List<ProgramRecord> {
            val records = com.maleicacid.tvinput.aribsi.EventModelMapper().toProgramRecords(
                events.filter { event ->
                    event.serviceKey == serviceKey && nowMillis >= event.startTimeMillis && nowMillis < event.startTimeMillis + event.durationMillis
                },
            )
            val metadata = records.associate { programVideoMetadataKey(it) to info }
            return mergeVideoMetadataForTest(records, metadata)
        }

        fun programVideoMetadataKeyForTest(record: ProgramRecord): String = programVideoMetadataKey(record)

        fun mergeVideoMetadataForTest(
            records: List<ProgramRecord>,
            latestVideoMetadataByProgramKey: Map<String, PlaybackPipeline.VideoFormatInfo>,
        ): List<ProgramRecord> = records.map { record ->
            val info = latestVideoMetadataByProgramKey[programVideoMetadataKey(record)]
            if (info == null || record.videoWidth != null || record.videoHeight != null || record.videoFormat != null) {
                record
            } else {
                record.copy(videoWidth = info.width, videoHeight = info.height, videoFormat = info.mime)
            }
        }

        private fun programVideoMetadataKey(record: ProgramRecord): String {
            val key = record.serviceKey
            return listOf(
                key.originalNetworkId,
                key.transportStreamId,
                key.serviceId,
                record.eventId,
                record.stableIdentity,
            ).joinToString(":")
        }
    }
}

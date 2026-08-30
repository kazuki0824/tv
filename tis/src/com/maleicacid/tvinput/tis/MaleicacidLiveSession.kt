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
import com.maleicacid.tvinput.aribsi.AribRatingMapper
import com.maleicacid.tvinput.aribsi.PmtCatCaMetadataMapper
import com.maleicacid.tvinput.aribsi.SectionIngestController
import com.maleicacid.tvinput.aribsi.SiDiscoveryProfile
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.db.ChannelRecord
import com.maleicacid.tvinput.db.ProgramRecord

class MaleicacidLiveSession(
    serviceContext: Context,
    private val sessionContext: Context,
    private val inputId: String,
    private val sessionId: String? = null,
) : TvInputService.Session(sessionContext) {
    private val appContext = serviceContext.applicationContext
    private val tvInputManager: TvInputManager? = appContext.getSystemService(TvInputManager::class.java)
    private val aribSiEngine = AribSiEngine(serviceContext)
    private val sectionIngestController = SectionIngestController(aribSiEngine)
    private val tunerController = TunerController(serviceContext, inputId, sessionId = sessionId, sessionContext = sessionContext)
    private val casController = CasController()
    private val caMapper = PmtCatCaMetadataMapper()
    private val eventModelMapper = com.maleicacid.tvinput.aribsi.EventModelMapper()
    private val tvProviderWriter = TvProviderWriter(serviceContext, inputId)
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
    private var currentRatingProfile: AribRatingMapper.BroadcastProfile = AribRatingMapper.BroadcastProfile.UNRESOLVED
    private var currentGeneration: Long = 0L
    private var captionEnabled: Boolean = false
    private var streamVolume: Float = 1.0f
    private var playbackState: PlaybackStartState = PlaybackStartState.Idle
    private var latestService: AribService? = null
    private val latestVideoMetadataByProgramKey = linkedMapOf<String, PlaybackPipeline.VideoFormatInfo>()
    private var preferredAudioTrackId: String? = null
    private var selectedSubtitleTrackId: String? = null
    private var currentTrackSignature: Set<String> = emptySet()
    private val captionOverlayView = CaptionOverlayView(appContext)
    private val captionController = AribCaptionController(captionOverlayView) {
        tunerController.currentMediaClockSnapshot()
    }
    private val unblockedContentKeys = linkedSetOf<String>()
    private var currentUnblockProgramIdentityKey: String? = null
    private var lastParentalAccessState: ParentalAccessState = ParentalAccessState.UNKNOWN
    private var lastBlockedContent: BlockedContent? = null
    private data class BlockedContent(val rating: TvContentRating, val unblockKey: String)
    private enum class ParentalAccessState { UNKNOWN, ALLOWED, BLOCKED }
    private sealed class ContentAccessDecision {
        data class Block(val blocked: BlockedContent) : ContentAccessDecision()
        object Allow : ContentAccessDecision()
        data class HoldPrevious(val reason: String) : ContentAccessDecision()
    }
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
        tunerController.setOnSubtitlePesCallback { trackId, pesData, timestamp ->
            captionController.onPesData(trackId, pesData, timestamp)
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
            playbackState = PlaybackStartState.Stopped
            tunerController.stopPlayback()
            captionController.beginPlaybackGeneration(-1L, false)
            notifyVideoUnavailable(TvInputManager.VIDEO_UNAVAILABLE_REASON_UNKNOWN)
        } else {
            playbackState = PlaybackStartState.Idle
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
        playbackState = PlaybackStartState.Idle
        currentService = null
        currentRatingProfile = AribRatingMapper.BroadcastProfile.UNRESOLVED
        currentGeneration = 0L
        captionController.beginPlaybackGeneration(-1L, false)
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
        val outcome = tunerController.tuneForLive(channelUri)
        if (!outcome.success || outcome.channel == null) {
            notifyVideoUnavailable(TvInputManager.VIDEO_UNAVAILABLE_REASON_UNKNOWN)
            return false
        }
        currentService = outcome.channel.serviceKey
        aribSiEngine.setDiscoveryProfile(
            when {
                outcome.channel.deliverySystem == ChannelRecord.DELIVERY_SYSTEM_ISDB_T -> SiDiscoveryProfile.ISDB_T
                outcome.channel.satelliteBand == "110CS" -> SiDiscoveryProfile.CS110
                else -> SiDiscoveryProfile.BS
            },
        )
        currentRatingProfile = AribRatingMapper.profileForDeliverySystem(outcome.channel.deliverySystem)
        currentGeneration = outcome.generation
        if (PlaybackPolicy.isAudioOnlyService(outcome.channel.serviceType)) {
            notifyVideoUnavailable(TvInputManager.VIDEO_UNAVAILABLE_REASON_AUDIO_ONLY)
        }
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
        val transaction = aribSiEngine.livePlaybackSnapshot()
        val service = transaction.services.firstOrNull { it.serviceKey == serviceKey }
        val pmtPids = transaction.pmtPids.values.toSet()
        val allCaMetadata = if (ENABLE_CAS_ORCHESTRATION) transaction.caMetadata else emptyList()
        val serviceScopedCa = allCaMetadata.filter {
            it.serviceKey == serviceKey && it.source != com.maleicacid.tvinput.aribsi.CaMetadataSource.CAT
        }
        val catCa = allCaMetadata.filter { it.source == com.maleicacid.tvinput.aribsi.CaMetadataSource.CAT }
        val expanded = caMapper.expandProgramLevelToElementaryStreams(
            serviceScopedCa + catCa,
            transaction.services,
        )
        val serviceCaMetadata = expanded.filter { it.serviceKey == serviceKey }
        val caMetadata = expanded.filter { it.serviceKey == null || it.serviceKey == serviceKey }

        val casResult = if (caMetadata.isEmpty()) {
            casController.clearForClearService()
            CasController.UpdateResult(emptyList(), emptySet(), emptySet(), CasController.Readiness.CLEAR)
        } else {
            val prototype = if (serviceScopedCa.isEmpty()) null else tunerController.createDescramblerBridge()
            casController.updateFromCaMetadata(caMetadata, prototype)
        }
        // CasController owns B25/B1 execution policy. Only its filter plan reaches TunerController,
        // so B1 CAT metadata cannot accidentally reopen an EMM filter here.
        tunerController.updateDynamicSectionFiltersForService(
            serviceKey,
            pmtPids,
            casResult.ecmPids,
            casResult.emmPids,
            currentGeneration,
        )

        publishLiveProgramsForCurrentService()
        refreshCurrentProgramRatingState()

        if (serviceCaMetadata.isNotEmpty()) {
            when (casResult.readiness) {
                CasController.Readiness.READY -> Unit
                CasController.Readiness.ERROR,
                CasController.Readiness.CLOSED -> {
                    stopPlaybackForCasWait()
                    notifyVideoUnavailable(TvInputManager.VIDEO_UNAVAILABLE_REASON_CAS_UNKNOWN)
                    return
                }
                CasController.Readiness.WAITING_FOR_KEY,
                CasController.Readiness.CLEAR -> {
                    // Do not start encrypted AV until every required key context is linked.
                    stopPlaybackForCasWait()
                    notifyVideoUnavailable(TvInputManager.VIDEO_UNAVAILABLE_REASON_CAS_UNKNOWN)
                    return
                }
            }
        }

        if (service != null) {
            latestService = service
            updateTracks(service)
            maybeStartPlayback(service)
        }
    }

    private fun stopPlaybackForCasWait() {
        playbackState = PlaybackStartState.Stopped
        tunerController.stopPlayback()
        captionController.beginPlaybackGeneration(-1L, false)
    }

    private fun maybeStartPlayback(service: AribService): Boolean {
        when (val decision = contentAccessDecision()) {
            is ContentAccessDecision.Block -> {
                rememberBlockedContent(decision.blocked)
                stopPlaybackForBlockedContent(decision.blocked)
                return false
            }
            ContentAccessDecision.Allow -> {
                rememberAllowedContent()
                notifyContentAllowed()
            }
            is ContentAccessDecision.HoldPrevious -> {
                holdPreviousParentalAccessState(decision.reason)
                return false
            }
        }
        val selection = tunerController.selectAvStreams(service.serviceKey, service.pcrPid, service.streams, preferredAudioTrackId, selectedSubtitleTrackId)
        val audioOnly = PlaybackPolicy.isAudioOnlyService(service.serviceType)
        if (PlaybackPolicy.shouldRejectSelection(service.serviceType ?: -1, selection)) {
            playbackState = PlaybackStartState.Stopped
            tunerController.stopPlayback()
            captionController.beginPlaybackGeneration(-1L, false)
            val failure = if (audioOnly) {
                PlaybackPipeline.PlaybackUnavailable(
                    PlaybackPipeline.PlaybackUnavailableReason.UNSUPPORTED_AUDIO_STREAM,
                    "audio-only serviceに現行対応のaudio ESがありません service=${service.serviceKey}",
                )
            } else {
                PlaybackPipeline.PlaybackUnavailable(
                    PlaybackPipeline.PlaybackUnavailableReason.UNSUPPORTED_VIDEO_STREAM,
                    "audio-video serviceに現行対応のvideo ESがありません service=${service.serviceKey}",
                )
            }
            notifyVideoUnavailable(mapUnavailableReason(failure))
            return false
        }
        val signature = playbackSignatureFor(service, selection) ?: return false
        val stateBeforeAttempt = playbackState
        if (!PlaybackStartTransitions.shouldAttempt(stateBeforeAttempt, signature)) {
            return stateBeforeAttempt is PlaybackStartState.Started && stateBeforeAttempt.signature == signature
        }
        playbackState = PlaybackStartState.Starting(signature)
        val result = tunerController.startPlayback(selection)
        if (result == null) {
            playbackState = PlaybackStartState.Failed(signature, pipelineGeneration = null)
            notifyVideoUnavailable(TvInputManager.VIDEO_UNAVAILABLE_REASON_UNKNOWN)
            return false
        }
        captionController.beginPlaybackGeneration(result.generation, hasVideo = !audioOnly)
        captionController.onPlaybackClockChanged()
        if (result.firstFramePending == true) {
            playbackState = PlaybackStartState.WaitingFirstOutput(signature, result.generation)
            return false
        }
        val started = if (audioOnly) result.startedAudio else result.startedVideo
        if (started) {
            playbackState = PlaybackStartState.Started(signature, result.generation)
            if (audioOnly) notifyVideoUnavailable(TvInputManager.VIDEO_UNAVAILABLE_REASON_AUDIO_ONLY)
            return true
        }
        playbackState = PlaybackStartState.Failed(signature, result.generation)
        return false
    }

    private fun playbackSignatureFor(
        service: AribService,
        selection: TunerController.AvStreamSelection,
    ): AvPlaybackSignature? {
        val video = selection.video
        if (PlaybackPolicy.isAudioOnlyService(service.serviceType) && selection.audio == null) return null
        if (!PlaybackPolicy.isAudioOnlyService(service.serviceType) && video == null) return null
        val audio = selection.audio
        val casReadiness = casController.currentReadiness()
        return AvPlaybackSignature(
            serviceKey = service.serviceKey,
            pcrPid = selection.pcrPid,
            videoPid = video?.elementaryPid,
            videoStreamType = video?.streamType,
            audioPid = audio?.elementaryPid,
            audioStreamType = audio?.streamType,
            subtitlePid = selection.subtitle?.elementaryPid,
            subtitleDataComponentId = selection.subtitle?.dataComponentId,
            clear = casReadiness == CasController.Readiness.CLEAR,
            keyTokenAvailable = casReadiness == CasController.Readiness.READY,
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
                if (playbackState !is PlaybackStartState.Started) return false
                preferredAudioTrackId = trackId
                val selection = tunerController.selectAvStreams(service.serviceKey, service.pcrPid, service.streams, preferredAudioTrackId, selectedSubtitleTrackId)
                val signature = playbackSignatureFor(service, selection) ?: run {
                    preferredAudioTrackId = previousAudioTrackId
                    return false
                }
                val switched = tunerController.switchAudioTrack(selection)
                if (switched != null && switched.generation >= 0L) {
                    playbackState = PlaybackStartTransitions.afterRestartResult(
                        playbackState,
                        signature,
                        switched.generation,
                        switched.firstFramePending,
                        switched.switchedAudio,
                    )
                    captionController.beginPlaybackGeneration(
                        switched.generation,
                        hasVideo = !PlaybackPolicy.isAudioOnlyService(service.serviceType),
                    )
                    captionController.onPlaybackClockChanged()
                }
                if (switched?.switchedAudio == true) {
                    notifyTrackSelected(TvTrackInfo.TYPE_AUDIO, trackId)
                    true
                } else {
                    preferredAudioTrackId = previousAudioTrackId
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
                    maybeStartPlayback(service)
                    true
                } else {
                    val subtitle = tracks.firstOrNull { it.type == TvTrackInfo.TYPE_SUBTITLE && it.id == trackId } ?: return false
                    selectedSubtitleTrackId = subtitle.id
                    captionController.selectTrack(subtitle)
                    if (captionEnabled) notifyTrackSelected(TvTrackInfo.TYPE_SUBTITLE, subtitle.id)
                    maybeStartPlayback(service)
                    true
                }
            }
            else -> false
        }
    }

    private fun updateTracks(service: AribService) {
        val tracks = tunerController.tracksFor(service.streams).filterNot { track ->
            PlaybackPolicy.isAudioOnlyService(service.serviceType) && track.type == TvTrackInfo.TYPE_SUBTITLE
        }
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
        if (PlaybackPolicy.isAudioOnlyService(latestService?.serviceType)) {
            selectedSubtitleTrackId = null
            captionController.selectTrack(null)
            notifyTrackSelected(TvTrackInfo.TYPE_SUBTITLE, null)
            return
        }
        val selected = selectedSubtitleTrackId?.let { wanted -> tracks.firstOrNull { it.type == TvTrackInfo.TYPE_SUBTITLE && it.id == wanted } }
            ?: tracks.firstOrNull { it.type == TvTrackInfo.TYPE_SUBTITLE }
        selectedSubtitleTrackId = selected?.id
        captionController.selectTrack(selected)
        notifyTrackSelected(TvTrackInfo.TYPE_SUBTITLE, if (captionEnabled) selected?.id else null)
    }

    private fun handleFirstFrameAvailable() {
        captionController.onPlaybackClockChanged()
        when (val state = playbackState) {
            is PlaybackStartState.WaitingFirstOutput -> {
                playbackState = PlaybackStartState.Started(state.signature, state.pipelineGeneration)
            }
            else -> Unit
        }
        when (val decision = contentAccessDecision()) {
            is ContentAccessDecision.Block -> {
                rememberBlockedContent(decision.blocked)
                stopPlaybackForBlockedContent(decision.blocked)
                return
            }
            ContentAccessDecision.Allow -> {
                rememberAllowedContent()
                notifyContentAllowed()
                notifyVideoAvailable()
            }
            is ContentAccessDecision.HoldPrevious -> {
                holdPreviousParentalAccessState(decision.reason)
            }
        }
    }

    private fun handlePlaybackUnavailable(reason: PlaybackPipeline.PlaybackUnavailable) {
        val audioFailure = reason.reason == PlaybackPipeline.PlaybackUnavailableReason.AUDIO_UNAVAILABLE ||
            reason.reason == PlaybackPipeline.PlaybackUnavailableReason.AUDIO_FILTER_NOT_STARTED ||
            reason.reason == PlaybackPipeline.PlaybackUnavailableReason.UNSUPPORTED_AUDIO_STREAM
        if (audioFailure) {
            if (PlaybackPolicy.isAudioOnlyService(latestService?.serviceType)) {
                if (!PlaybackStartTransitions.acceptsGeneration(playbackState, reason.generation)) {
                    android.util.Log.w(
                        com.maleicacid.tvinput.common.LogTags.TIS,
                        "旧generationのaudio unavailableを破棄します reason=${reason.reason} generation=${reason.generation}",
                    )
                    return
                }
                playbackState = PlaybackStartTransitions.failCurrentGeneration(playbackState, reason.generation)
                captionController.beginPlaybackGeneration(-1L, false)
                notifyVideoUnavailable(mapUnavailableReason(reason))
                return
            }
            android.util.Log.w(com.maleicacid.tvinput.common.LogTags.TIS, "audio unavailable は video unavailable として通知しません reason=${reason.reason} detail=${reason.detail}")
            return
        }
        if (reason.reason == PlaybackPipeline.PlaybackUnavailableReason.FIRST_FRAME_TIMEOUT ||
            reason.reason == PlaybackPipeline.PlaybackUnavailableReason.VIDEO_CODEC_ERROR ||
            reason.reason == PlaybackPipeline.PlaybackUnavailableReason.CODEC_CONFIG_TIMEOUT ||
            reason.reason == PlaybackPipeline.PlaybackUnavailableReason.VIDEO_FILTER_NOT_STARTED) {
            val signature = PlaybackStartTransitions.signature(playbackState)
            if (signature != null) {
                playbackState = PlaybackStartState.Failed(
                    signature,
                    PlaybackStartTransitions.pipelineGeneration(playbackState),
                )
            }
        }
        notifyVideoUnavailable(mapUnavailableReason(reason))
    }

    private fun stopPlaybackForBlockedContent(blocked: BlockedContent) {
        notifyContentBlocked(blocked.rating)
        playbackState = PlaybackStartState.Stopped
        tunerController.stopPlayback()
        captionController.beginPlaybackGeneration(-1L, false)
    }

    private fun contentAccessDecision(): ContentAccessDecision {
        val manager = tvInputManager ?: return ContentAccessDecision.Allow
        if (!manager.isParentalControlsEnabled) return ContentAccessDecision.Allow
        return when (val result = currentProgramRatingResolver.resolveDetailed(
            channelUri = currentChannelUri,
            serviceKey = currentService,
            latestEvents = aribSiEngine.programStateSnapshot().events,
            ratingProfile = currentRatingProfile,
        )) {
            is CurrentProgramRatingResolver.ResolveResult.Ratings -> {
                val ratingSet = result.ratingSet
                clearUnblocksIfCurrentProgramChanged(ratingSet)
                ratingSet.ratingsForBlocking().firstNotNullOfOrNull { rating ->
                    val unblockKey = ratingSet.unblockKeyFor(rating)
                    if (unblockKey !in unblockedContentKeys && manager.isRatingBlocked(rating)) BlockedContent(rating, unblockKey) else null
                }?.let { ContentAccessDecision.Block(it) } ?: ContentAccessDecision.Allow
            }
            is CurrentProgramRatingResolver.ResolveResult.ProviderQueryFailed -> {
                android.util.Log.w(com.maleicacid.tvinput.common.LogTags.TIS, "TvProvider current program rating query failure; keeping previous parental access state reason=${result.reason}")
                ContentAccessDecision.HoldPrevious(result.reason)
            }
        }
    }

    private fun rememberAllowedContent() {
        lastParentalAccessState = ParentalAccessState.ALLOWED
        lastBlockedContent = null
    }

    private fun rememberBlockedContent(blocked: BlockedContent) {
        lastParentalAccessState = ParentalAccessState.BLOCKED
        lastBlockedContent = blocked
    }

    private fun holdPreviousParentalAccessState(reason: String) {
        when (lastParentalAccessState) {
            ParentalAccessState.ALLOWED -> {
                android.util.Log.w(com.maleicacid.tvinput.common.LogTags.TIS, "TvProvider rating query failure中のため、直前の許可状態を維持します reason=$reason")
                notifyVideoAvailable()
            }
            ParentalAccessState.BLOCKED -> {
                val blocked = lastBlockedContent
                android.util.Log.w(com.maleicacid.tvinput.common.LogTags.TIS, "TvProvider rating query failure中のため、直前の遮断状態を維持します reason=$reason")
                if (blocked != null) stopPlaybackForBlockedContent(blocked) else notifyVideoUnavailable(TvInputManager.VIDEO_UNAVAILABLE_REASON_UNKNOWN)
            }
            ParentalAccessState.UNKNOWN -> {
                android.util.Log.w(com.maleicacid.tvinput.common.LogTags.TIS, "TvProvider rating query failure中で直前状態が無いため、許可通知を出さず映像不可にします reason=$reason")
                notifyVideoUnavailable(TvInputManager.VIDEO_UNAVAILABLE_REASON_UNKNOWN)
            }
        }
    }

    private fun clearUnblocksIfCurrentProgramChanged(ratingSet: CurrentProgramRatingResolver.CurrentProgramRatingSet) {
        currentUnblockProgramIdentityKey = PlaybackPolicy.updateUnblockStateForProgramChange(
            previousIdentityKey = currentUnblockProgramIdentityKey,
            nextIdentityKey = ratingSet.programIdentityKey(),
            unblockedContentKeys = unblockedContentKeys,
        )
    }

    private fun reevaluateParentalControls() {
        when (val decision = contentAccessDecision()) {
            is ContentAccessDecision.Block -> {
                rememberBlockedContent(decision.blocked)
                stopPlaybackForBlockedContent(decision.blocked)
            }
            ContentAccessDecision.Allow -> {
                rememberAllowedContent()
                notifyContentAllowed()
                latestService?.let { service ->
                    playbackState = PlaybackStartTransitions.allowRetry(playbackState)
                    maybeStartPlayback(service)
                }
            }
            is ContentAccessDecision.HoldPrevious -> {
                holdPreviousParentalAccessState(decision.reason)
            }
        }
    }

    private fun updateCurrentProgramVideoMetadata(info: PlaybackPipeline.VideoFormatInfo) {
        captionController.updateVideoGeometry(
            PlaybackStartTransitions.pipelineGeneration(playbackState) ?: -1L,
            info.width,
            info.height,
        )
        val key = currentService ?: return
        val now = System.currentTimeMillis()
        val transaction = aribSiEngine.programStateSnapshot()
        val records = eventModelMapper.toProgramRecords(
            events = transaction.events.filter { event ->
                ProgramVideoMetadataPolicy.eventContainsTime(event, key, now)
            },
            semanticFactsByServiceKey = transaction.semanticFactsByServiceKey,
            malformedCaDescriptorCountByServiceId = transaction.malformedCaDescriptorCountByServiceId,
            ratingProfileByServiceKey = mapOf(key to currentRatingProfile),
        )
        if (records.isEmpty()) return
        rememberVideoMetadata(records, info)
        publishLivePrograms(applyLatestVideoMetadata(records))
    }

    private fun refreshCurrentProgramRatingState() {
        when (val result = currentProgramRatingResolver.resolveDetailed(
            channelUri = currentChannelUri,
            serviceKey = currentService,
            latestEvents = aribSiEngine.programStateSnapshot().events,
            ratingProfile = currentRatingProfile,
        )) {
            is CurrentProgramRatingResolver.ResolveResult.Ratings -> clearUnblocksIfCurrentProgramChanged(result.ratingSet)
            is CurrentProgramRatingResolver.ResolveResult.ProviderQueryFailed -> android.util.Log.w(com.maleicacid.tvinput.common.LogTags.TIS, "TvProvider rating query failure中のため unblock 状態を更新しません reason=${result.reason}")
        }
    }

    private fun publishLiveProgramsForCurrentService() {
        val key = currentService ?: return
        val transaction = aribSiEngine.programStateSnapshot()
        val records = eventModelMapper.toProgramRecords(
            events = transaction.events.filter { it.serviceKey == key },
            semanticFactsByServiceKey = transaction.semanticFactsByServiceKey,
            malformedCaDescriptorCountByServiceId = transaction.malformedCaDescriptorCountByServiceId,
            ratingProfileByServiceKey = mapOf(key to currentRatingProfile),
        )
        publishLivePrograms(applyLatestVideoMetadata(records))
    }

    private fun rememberVideoMetadata(records: List<ProgramRecord>, info: PlaybackPipeline.VideoFormatInfo) {
        records.forEach { record ->
            latestVideoMetadataByProgramKey[ProgramVideoMetadataPolicy.key(record)] = info
        }
    }

    private fun applyLatestVideoMetadata(records: List<ProgramRecord>): List<ProgramRecord> =
        ProgramVideoMetadataPolicy.merge(records, latestVideoMetadataByProgramKey)

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
        val ratingSet = when (val result = currentProgramRatingResolver.resolveDetailed(
            channelUri = currentChannelUri,
            serviceKey = currentService,
            latestEvents = aribSiEngine.programStateSnapshot().events,
            ratingProfile = currentRatingProfile,
        )) {
            is CurrentProgramRatingResolver.ResolveResult.Ratings -> {
                clearUnblocksIfCurrentProgramChanged(result.ratingSet)
                result.ratingSet
            }
            is CurrentProgramRatingResolver.ResolveResult.ProviderQueryFailed -> {
                android.util.Log.w(com.maleicacid.tvinput.common.LogTags.TIS, "TvProvider rating query failure中のため unblock 状態を更新しません reason=${result.reason}")
                return
            }
        }
        val unblockKey = ratingSet.exactUnblockKeyFor(rating) ?: return
        unblockedContentKeys += unblockKey
        notifyContentAllowed()
        latestService?.let { service ->
            playbackState = PlaybackStartTransitions.allowRetry(playbackState)
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
            playbackState = PlaybackStartState.Stopped
            unregisterParentalControlReceiver()
            casController.close()
            tunerController.release()
            aribSiEngine.close()
        } finally {
            ChannelScanManager.unregisterLiveSession(appContext)
        }
    }

    companion object {
        private const val ENABLE_CAS_ORCHESTRATION = true
    }
}

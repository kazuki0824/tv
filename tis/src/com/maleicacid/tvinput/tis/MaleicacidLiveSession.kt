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
import android.os.Bundle
import android.media.tv.tuner.frontend.OnTuneEventListener
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
    private val sessionId: String,
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
        Thread(runnable, "maleicacid-live-session-$sessionId").also { thread ->
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
    private var latestLiveSnapshot: com.maleicacid.tvinput.aribsi.LivePlaybackSnapshot? = null
    private val latestService: AribService?
        get() = latestLiveSnapshot?.services?.firstOrNull { it.serviceKey == currentService }
    private val latestVideoMetadataByProgramKey = linkedMapOf<String, PlaybackPipeline.VideoFormatInfo>()
    private var preferredAudioTrackId: String? = null
    private var audioFallbackDisabled: Boolean = false
    private var dualMonoPresentation: PlaybackPipeline.DualMonoPresentation = PlaybackPipeline.DualMonoPresentation.MAIN
    private var frontendSignalUnavailable: Boolean = false
    private var selectedSubtitleTrackId: String? = null
    private var subtitleExplicitlyDisabled: Boolean = false
    private var currentTrackSignature: Set<String> = emptySet()
    private val captionOverlayView = CaptionOverlayView(appContext)
    private val captionController = AribCaptionController(
        captionOverlayView,
        mediaClock = { tunerController.currentMediaClockSnapshot() },
    )
    private val superimposeController = AribCaptionController(
        captionOverlayView,
        { tunerController.currentMediaClockSnapshot() },
        overlayLayerId = "superimpose",
        allowNoPts = true,
        broadcastDeadline = { statementTime, generation -> tunerController.broadcastDeadlineUntil(statementTime, generation) },
    )
    private val temporaryUnblocks = TemporaryContentUnblocks()
    private val unblockTimerHandler = android.os.Handler(android.os.Looper.getMainLooper())
    private var unblockExpiryTask: Runnable? = null
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
            onVideoAvailable = { generation -> enqueueSessionAction { handleFirstFrameAvailable(generation) } },
            onVideoUnavailable = { reason -> enqueueSessionAction { handlePlaybackUnavailable(reason) } },
        )
        tunerController.setOnVideoFormatDiscoveredCallback { generation, info ->
            enqueueSessionAction { updateCurrentProgramVideoMetadata(generation, info) }
        }
        tunerController.setOnSubtitleContinuityLostCallback { generation, trackId ->
            enqueueSessionAction {
                if (PlaybackStartTransitions.acceptsGeneration(playbackState, generation)) {
                    if (trackId.startsWith("superimpose:")) {
                        superimposeController.flushForSubtitleContinuityLoss()
                    } else {
                        captionController.flushForSubtitleContinuityLoss()
                    }
                }
            }
        }
        tunerController.setOnSubtitlePesCallback { generation, trackId, pesData, timestamp, broadcastStatementTime ->
            enqueueSessionAction {
                if (PlaybackStartTransitions.acceptsGeneration(playbackState, generation)) {
                    // Rust caption JNI parses management/STM facts before this callback.
                    // Rebuild TIF tracks so newly discovered language_tag values become selectable.
                    latestService?.let(::updateTracks)
                    if (trackId.startsWith("superimpose:")) {
                        if (broadcastStatementTime != null) {
                            superimposeController.onBroadcastTimedPesData(trackId, pesData, broadcastStatementTime)
                        } else {
                            superimposeController.onPesData(trackId, pesData, timestamp)
                        }
                    } else {
                        captionController.onPesData(trackId, pesData, timestamp)
                    }
                }
            }
        }
        tunerController.setOnBroadcastClockUpdatedCallback {
            superimposeController.onBroadcastClockChanged()
        }
        tunerController.setOnPlaybackGenerationRestartedCallback { restart ->
            enqueueSessionAction { handlePlaybackGenerationRestart(restart) }
        }
        tunerController.setOnTunerResourceLostCallback { tuneGeneration ->
            enqueueSessionAction { handleTunerResourceLost(tuneGeneration) }
        }
        tunerController.setOnTuneEventCallback { tuneGeneration, event ->
            enqueueSessionAction { handleFrontendTuneEvent(tuneGeneration, event) }
        }
        superimposeController.setEnabled(true)
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
            beginCaptionPresentationGeneration(-1L, false)
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
            latestService?.let { updateSubtitleSelection(tunerController.tracksFor(it.streams, currentDefaultComponentGroupTags(it.serviceKey))) }
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
        beginCaptionPresentationGeneration(-1L, false)
        currentChannelUri = channelUri
        latestLiveSnapshot = null
        latestVideoMetadataByProgramKey.clear()
        clearTemporaryUnblocks()
        lastParentalAccessState = ParentalAccessState.UNKNOWN
        lastBlockedContent = null
        programPublishCoordinator.reset()
        preferredAudioTrackId = null
        audioFallbackDisabled = false
        dualMonoPresentation = PlaybackPipeline.DualMonoPresentation.MAIN
        frontendSignalUnavailable = false
        selectedSubtitleTrackId = null
        subtitleExplicitlyDisabled = false
        captionController.setEnabled(captionEnabled)
        captionController.selectTrack(null)
        superimposeController.setEnabled(true)
        superimposeController.selectTrack(null)
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
        latestLiveSnapshot = transaction
        val service = transaction.services.firstOrNull { it.serviceKey == serviceKey }
        val pmtPids = transaction.pmtPidsFor(serviceKey)
        val decision = currentServicePolicy()
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
        val casPids = SectionFilterPolicy.casPidsFor(decision, caMetadata)
        // policy不成立時はPMTを維持し、旧ECM/EMM集合を空へ置換して配送を止める。
        tunerController.updateDynamicSectionFiltersForService(serviceKey, pmtPids, casPids.ecm, casPids.emm, currentGeneration)

        if (!decision.casDecisionReady) {
            casController.clearForClearService()
            playbackState = PlaybackStartState.Stopped
            tunerController.stopPlayback()
            beginCaptionPresentationGeneration(-1L, false)
            notifyVideoUnavailable(if (decision.registrationReady) TvInputManager.VIDEO_UNAVAILABLE_REASON_CAS_UNKNOWN else TvInputManager.VIDEO_UNAVAILABLE_REASON_UNKNOWN)
        }
        if (decision.registrationReady) {
            publishLiveProgramsForCurrentService()
            refreshCurrentProgramRatingState()
        }
        if (!decision.casDecisionReady) return
        if (caMetadata.isEmpty()) {
            casController.clearForClearService()
        } else {
            val bridge = if (serviceScopedCa.isEmpty()) null else tunerController.createDescramblerBridge()
            val casResult = casController.updateFromCaMetadata(caMetadata, bridge)
            val blockingCasError = serviceCaMetadata.isNotEmpty() && casResult.diagnostics.any { it.state == CasController.State.ERROR }
            if (blockingCasError) {
                playbackState = PlaybackStartState.Stopped
                tunerController.stopPlayback()
                beginCaptionPresentationGeneration(-1L, false)
                notifyVideoUnavailable(TvInputManager.VIDEO_UNAVAILABLE_REASON_CAS_UNKNOWN)
                return
            }
            if (serviceCaMetadata.isNotEmpty()) {
                playbackState = PlaybackStartState.Stopped
                tunerController.stopPlayback()
                beginCaptionPresentationGeneration(-1L, false)
                notifyVideoUnavailable(mapUnavailableReason(PlaybackPipeline.PlaybackUnavailable(PlaybackPipeline.PlaybackUnavailableReason.CAS_NO_KEY, "r51 CAS placeholder cannot provide real key token")))
                return
            }
        }
        if (service != null) {
            updateTracks(service)
            maybeStartPlayback(service)
        }
    }

    private fun currentServicePolicy() =
        com.maleicacid.tvinput.aribsi.ServicePolicyEvaluator.evaluateLive(latestLiveSnapshot, currentService)

    private fun maybeStartPlayback(service: AribService): Boolean {
        if (!currentServicePolicy().clearLivePlaybackStaticallyEligible) return false
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
        val initialSelection = tunerController.selectAvStreams(
            service.serviceKey,
            service.pcrPid,
            service.streams,
            preferredAudioTrackId,
            selectedSubtitleTrackId,
            audioExplicitlyDisabled = audioFallbackDisabled,
            subtitleExplicitlyDisabled = subtitleExplicitlyDisabled,
            defaultComponentGroupTags = currentDefaultComponentGroupTags(service.serviceKey),
            dualMonoPresentation = dualMonoPresentation,
        )
        val selection = initialSelection.copy(
            audioComponentType = currentAudioComponent(service.serviceKey, initialSelection.audio?.componentTag)?.componentType
                ?: initialSelection.audio?.componentType,
        )
        val audioOnly = PlaybackPolicy.isAudioOnlyService(service.serviceType)
        if (PlaybackPolicy.shouldRejectSelection(service.serviceType ?: -1, selection)) {
            playbackState = PlaybackStartState.Stopped
            tunerController.stopPlayback()
            beginCaptionPresentationGeneration(-1L, false)
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
        beginCaptionPresentationGeneration(result.generation, hasVideo = !audioOnly)
        onCaptionPlaybackClockChanged()
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
        return AvPlaybackSignature(
            serviceKey = service.serviceKey,
            pcrPid = selection.pcrPid,
            videoPid = video?.elementaryPid,
            videoStreamType = video?.streamType,
            audioPid = audio?.elementaryPid,
            audioStreamType = audio?.streamType,
            videoConfiguration = video?.let { DecoderConfigurationIdentity.from(it) },
            audioConfiguration = audio?.let { DecoderConfigurationIdentity.from(it, selection.audioComponentType ?: it.componentType) },
            subtitlePid = selection.subtitle?.elementaryPid,
            subtitleDataComponentId = selection.subtitle?.dataComponentId,
            subtitleLanguageId = selection.subtitleLanguageId,
            superimposePid = selection.superimpose?.elementaryPid,
            superimposeDataComponentId = selection.superimpose?.dataComponentId,
            clear = true,
            keyTokenAvailable = false,
        )
    }

    override fun onAppPrivateCommand(action: String, data: Bundle?) {
        enqueueSessionAction {
            if (action != ACTION_SET_DUAL_MONO_PRESENTATION) return@enqueueSessionAction
            val presentation = when (data?.getString(EXTRA_DUAL_MONO_PRESENTATION)) {
                DUAL_MONO_MAIN -> PlaybackPipeline.DualMonoPresentation.MAIN
                DUAL_MONO_SUB -> PlaybackPipeline.DualMonoPresentation.SUB
                DUAL_MONO_MAIN_SUB -> PlaybackPipeline.DualMonoPresentation.MAIN_SUB
                else -> return@enqueueSessionAction
            }
            if (tunerController.setDualMonoPresentation(presentation)) {
                dualMonoPresentation = presentation
            }
        }
    }

    override fun onSelectTrack(type: Int, trackId: String?): Boolean = runOnSessionExecutorBlocking {
        onSelectTrackOnSessionExecutor(type, trackId)
    }

    private fun onSelectTrackOnSessionExecutor(type: Int, trackId: String?): Boolean {
        val service = latestService ?: return false
        val defaultComponentGroupTags = currentDefaultComponentGroupTags(service.serviceKey)
        val tracks = tunerController.tracksFor(service.streams, defaultComponentGroupTags)
        return when (type) {
            TvTrackInfo.TYPE_AUDIO -> {
                if (trackId == null || tracks.none { it.type == TvTrackInfo.TYPE_AUDIO && it.id == trackId }) return false
                val previousAudioTrackId = preferredAudioTrackId
                val previousAudioFallbackDisabled = audioFallbackDisabled
                val previousDualMonoPresentation = dualMonoPresentation
                if (playbackState !is PlaybackStartState.Started) return false
                preferredAudioTrackId = trackId
                audioFallbackDisabled = false
                if (trackId != previousAudioTrackId) dualMonoPresentation = PlaybackPipeline.DualMonoPresentation.MAIN
                val initialSelection = tunerController.selectAvStreams(
                    service.serviceKey,
                    service.pcrPid,
                    service.streams,
                    preferredAudioTrackId,
                    selectedSubtitleTrackId,
                    audioExplicitlyDisabled = audioFallbackDisabled,
                    subtitleExplicitlyDisabled = subtitleExplicitlyDisabled,
                    defaultComponentGroupTags = defaultComponentGroupTags,
                    dualMonoPresentation = dualMonoPresentation,
                )
                val selection = initialSelection.copy(
                    audioComponentType = currentAudioComponent(service.serviceKey, initialSelection.audio?.componentTag)?.componentType
                        ?: initialSelection.audio?.componentType,
                )
                val signature = playbackSignatureFor(service, selection) ?: run {
                    preferredAudioTrackId = previousAudioTrackId
                    audioFallbackDisabled = previousAudioFallbackDisabled
                    dualMonoPresentation = previousDualMonoPresentation
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
                    beginCaptionPresentationGeneration(
                        switched.generation,
                        hasVideo = !PlaybackPolicy.isAudioOnlyService(service.serviceType),
                    )
                    onCaptionPlaybackClockChanged()
                }
                if (switched?.switchedAudio == true) {
                    notifyTrackSelected(TvTrackInfo.TYPE_AUDIO, trackId)
                    true
                } else {
                    preferredAudioTrackId = previousAudioTrackId
                    audioFallbackDisabled = previousAudioFallbackDisabled
                    dualMonoPresentation = previousDualMonoPresentation
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
                    subtitleExplicitlyDisabled = true
                    captionController.selectTrack(null)
                    notifyTrackSelected(TvTrackInfo.TYPE_SUBTITLE, null)
                    maybeStartPlayback(service)
                    true
                } else {
                    val subtitle = tracks.firstOrNull { it.type == TvTrackInfo.TYPE_SUBTITLE && it.id == trackId } ?: return false
                    selectedSubtitleTrackId = subtitle.id
                    subtitleExplicitlyDisabled = false
                    captionController.selectTrack(subtitle)
                    if (captionEnabled) notifyTrackSelected(TvTrackInfo.TYPE_SUBTITLE, subtitle.id)
                    maybeStartPlayback(service)
                    true
                }
            }
            else -> false
        }
    }

    private fun currentProgramEvent(
        serviceKey: ServiceKey,
        nowMillis: Long = System.currentTimeMillis(),
    ) = latestLiveSnapshot?.programs?.events.orEmpty()
        .asSequence()
        .filter { event -> event.serviceKey == serviceKey && event.durationMillis > 0L }
        .filter { event -> nowMillis >= event.startTimeMillis && nowMillis < event.startTimeMillis + event.durationMillis }
        .minByOrNull { it.startTimeMillis }

    private fun currentAudioComponent(
        serviceKey: ServiceKey,
        componentTag: Int?,
        nowMillis: Long = System.currentTimeMillis(),
    ): com.maleicacid.tvinput.aribsi.AribComponentEntry? {
        componentTag ?: return null
        val currentEvent = currentProgramEvent(serviceKey, nowMillis) ?: return null
        return currentEvent.descriptors.components.audio
            .firstOrNull { component -> component.parseStatus.equals("OK", ignoreCase = true) && component.componentTag == componentTag }
    }

    private fun currentVideoComponent(
        serviceKey: ServiceKey,
        componentTag: Int?,
        nowMillis: Long = System.currentTimeMillis(),
    ): com.maleicacid.tvinput.aribsi.AribComponentEntry? {
        componentTag ?: return null
        val currentEvent = currentProgramEvent(serviceKey, nowMillis) ?: return null
        return currentEvent.descriptors.components.video
            .firstOrNull { component -> component.parseStatus.equals("OK", ignoreCase = true) && component.componentTag == componentTag }
    }

    private fun currentDefaultComponentGroupTags(serviceKey: ServiceKey, nowMillis: Long = System.currentTimeMillis()): Set<Int>? {
        val currentEvent = currentProgramEvent(serviceKey, nowMillis) ?: return null
        return currentEvent.descriptors.componentGroups
            .asSequence()
            .filter { it.componentGroupType == 0 }
            .flatMap { it.groups.asSequence() }
            .firstOrNull { it.componentGroupId == 0 }
            ?.componentTags
            ?.toSet()
            ?.takeIf { it.isNotEmpty() }
    }

    private fun updateTracks(service: AribService) {
        val defaultComponentGroupTags = currentDefaultComponentGroupTags(service.serviceKey)
        val tracks = tunerController.tracksFor(service.streams, defaultComponentGroupTags).filterNot { track ->
            PlaybackPolicy.isAudioOnlyService(service.serviceType) && track.type == TvTrackInfo.TYPE_SUBTITLE
        }
        val audioMetadataByTrackId = tracks
            .filter { it.type == TvTrackInfo.TYPE_AUDIO }
            .associate { track ->
                val component = currentAudioComponent(service.serviceKey, track.componentTag)
                track.id to AudioTrackMetadataPolicy.project(track.streamType, track.language, component)
            }
        val videoMetadataByTrackId = tracks
            .filter { it.type == TvTrackInfo.TYPE_VIDEO }
            .associate { track ->
                val component = currentVideoComponent(service.serviceKey, track.componentTag)
                track.id to VideoTrackMetadataPolicy.project(component)
            }
        val signature = tracks.map { track ->
            val audioMetadata = audioMetadataByTrackId[track.id]
            val videoMetadata = videoMetadataByTrackId[track.id]
            val subtitleDataComponentId = if (track.type == TvTrackInfo.TYPE_SUBTITLE) track.dataComponentId ?: -1 else -1
            listOf(
                track.id,
                track.type.toString(),
                track.pid.toString(),
                track.streamType.toString(),
                track.componentTag?.toString() ?: "-1",
                audioMetadata?.language ?: track.language.orEmpty(),
                audioMetadata?.encoding.orEmpty(),
                audioMetadata?.channelCount?.toString() ?: "-1",
                audioMetadata?.sampleRateHz?.toString() ?: "-1",
                audioMetadata?.description.orEmpty(),
                (audioMetadata?.audioDescription == true).toString(),
                (audioMetadata?.hardOfHearing == true).toString(),
                videoMetadata?.description.orEmpty(),
                videoMetadata?.width?.toString() ?: "-1",
                videoMetadata?.height?.toString() ?: "-1",
                subtitleDataComponentId.toString(),
            ).joinToString("|")
        }.toSet()
        if (signature != currentTrackSignature) {
            currentTrackSignature = signature
            notifyTracksChanged(tracks.map { track ->
                val builder = TvTrackInfo.Builder(track.type, track.id)
                val audioMetadata = audioMetadataByTrackId[track.id]
                val videoMetadata = videoMetadataByTrackId[track.id]
                val language = audioMetadata?.language ?: track.language
                LanguageCodeNormalizer.normalizeForTvTrackLanguage(language)?.let(builder::setLanguage)
                if (track.type == TvTrackInfo.TYPE_AUDIO && audioMetadata != null) {
                    audioMetadata.encoding?.let(builder::setEncoding)
                    audioMetadata.channelCount?.let(builder::setAudioChannelCount)
                    audioMetadata.sampleRateHz?.let(builder::setAudioSampleRate)
                    audioMetadata.description?.let(builder::setDescription)
                    if (audioMetadata.audioDescription) builder.setAudioDescription(true)
                    if (audioMetadata.hardOfHearing) builder.setHardOfHearing(true)
                }
                if (track.type == TvTrackInfo.TYPE_VIDEO && videoMetadata != null) {
                    videoMetadata.description?.let(builder::setDescription)
                    videoMetadata.width?.let(builder::setVideoWidth)
                    videoMetadata.height?.let(builder::setVideoHeight)
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
        updateSuperimposeSelection(service)
    }

    private fun updateSubtitleSelection(tracks: List<TunerController.TisTrack>) {
        if (PlaybackPolicy.isAudioOnlyService(latestService?.serviceType)) {
            selectedSubtitleTrackId = null
            captionController.selectTrack(null)
            notifyTrackSelected(TvTrackInfo.TYPE_SUBTITLE, null)
            return
        }
        if (subtitleExplicitlyDisabled) {
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

    private fun updateSuperimposeSelection(service: AribService) {
        if (PlaybackPolicy.isAudioOnlyService(service.serviceType)) {
            superimposeController.selectTrack(null)
            return
        }
        val track = tunerController.superimposeTrackFor(service.streams, currentDefaultComponentGroupTags(service.serviceKey))
        superimposeController.selectTrack(track?.takeIf { it.automaticPresentationOnReception == true })
    }

    private fun beginCaptionPresentationGeneration(generation: Long, hasVideo: Boolean) {
        captionController.beginPlaybackGeneration(generation, hasVideo)
        superimposeController.beginPlaybackGeneration(generation, hasVideo)
    }

    private fun onCaptionPlaybackClockChanged() {
        captionController.onPlaybackClockChanged()
        superimposeController.onPlaybackClockChanged()
    }

    private fun handleFirstFrameAvailable(generation: Long) {
        val state = playbackState as? PlaybackStartState.WaitingFirstOutput ?: return
        if (state.pipelineGeneration != generation) return
        playbackState = PlaybackStartState.Started(state.signature, state.pipelineGeneration)
        onCaptionPlaybackClockChanged()
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

    private fun handlePlaybackGenerationRestart(restart: PlaybackPipeline.PlaybackGenerationRestart) {
        if (!PlaybackStartTransitions.acceptsGeneration(playbackState, restart.originGeneration)) return
        val previousSignature = PlaybackStartTransitions.signature(playbackState) ?: return
        val restartedSignature = if (restart.videoOnly) {
            audioFallbackDisabled = true
            previousSignature.copy(audioPid = null, audioStreamType = null, audioConfiguration = null)
        } else previousSignature
        playbackState = PlaybackStartTransitions.afterRestartResult(
            playbackState,
            restartedSignature,
            restart.result.generation,
            restart.result.firstFramePending,
            restart.result.startedVideo || restart.result.startedAudio || restart.result.firstFramePending,
        )
        beginCaptionPresentationGeneration(restart.result.generation, hasVideo = restartedSignature.videoPid != null)
        onCaptionPlaybackClockChanged()
    }

    private fun handleTunerResourceLost(lostTuneGeneration: Long) {
        if (lostTuneGeneration != currentGeneration) return
        frontendSignalUnavailable = false
        playbackState = PlaybackStartState.Stopped
        beginCaptionPresentationGeneration(-1L, false)
        notifyVideoUnavailable(TvInputManager.VIDEO_UNAVAILABLE_REASON_UNKNOWN)
    }

    private fun handleFrontendTuneEvent(tuneGeneration: Long, event: Int) {
        if (tuneGeneration != currentGeneration) return
        when (event) {
            OnTuneEventListener.SIGNAL_NO_SIGNAL, OnTuneEventListener.SIGNAL_LOST_LOCK -> {
                frontendSignalUnavailable = true
                playbackState = PlaybackStartState.Stopped
                tunerController.stopPlayback()
                beginCaptionPresentationGeneration(-1L, false)
                notifyVideoUnavailable(TvInputManager.VIDEO_UNAVAILABLE_REASON_WEAK_SIGNAL)
            }
            OnTuneEventListener.SIGNAL_LOCKED -> {
                if (!frontendSignalUnavailable) return
                frontendSignalUnavailable = false
                playbackState = PlaybackStartState.Idle
                refreshDynamicSiAndCasFilters()
            }
        }
    }

    private fun handlePlaybackUnavailable(reason: PlaybackPipeline.PlaybackUnavailable) {
        if (reason.generation > 0L && !PlaybackStartTransitions.acceptsGeneration(playbackState, reason.generation)) {
            android.util.Log.w(com.maleicacid.tvinput.common.LogTags.TIS, "旧generationのplayback unavailableを破棄します reason=${reason.reason} generation=${reason.generation}")
            return
        }
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
                beginCaptionPresentationGeneration(-1L, false)
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
        beginCaptionPresentationGeneration(-1L, false)
    }

    private fun contentAccessDecision(): ContentAccessDecision {
        val manager = tvInputManager ?: return ContentAccessDecision.Allow
        if (!manager.isParentalControlsEnabled) return ContentAccessDecision.Allow
        return when (val result = currentProgramRatingResolver.resolveDetailed(
            channelUri = currentChannelUri,
            serviceKey = currentService,
            latestEvents = latestLiveSnapshot?.programs?.events.orEmpty(),
            eitAuthority = currentProgramRatingResolver.eitAuthority(latestLiveSnapshot?.programs, currentService),
            ratingProfile = currentRatingProfile,
        )) {
            is CurrentProgramRatingResolver.ResolveResult.Ratings -> {
                val ratingSet = result.ratingSet
                clearUnblocksIfCurrentProgramChanged(ratingSet)
                ratingSet.ratingsForBlocking().firstNotNullOfOrNull { rating ->
                    val unblockKey = ratingSet.unblockKeyFor(rating)
                    if (!temporaryUnblocks.contains(unblockKey, System.currentTimeMillis(), android.os.SystemClock.elapsedRealtime()) && manager.isRatingBlocked(rating)) BlockedContent(rating, unblockKey) else null
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
        temporaryUnblocks.updateProgram(ratingSet.programIdentityKey().takeIf { ratingSet.currentRowSelectionKey() != null })
        ratingSet.endTimeMillis?.let {
            temporaryUnblocks.restrictEnd(it, System.currentTimeMillis(), android.os.SystemClock.elapsedRealtime())
        }
        armUnblockExpiration()
    }

    private fun clearTemporaryUnblocks() {
        temporaryUnblocks.clear()
        unblockExpiryTask?.let(unblockTimerHandler::removeCallbacks)
        unblockExpiryTask = null
    }

    private fun armUnblockExpiration(): Boolean {
        unblockExpiryTask?.let(unblockTimerHandler::removeCallbacks)
        unblockExpiryTask = null
        val delay = temporaryUnblocks.nextDelayMillis(System.currentTimeMillis(), android.os.SystemClock.elapsedRealtime())
            ?: return true
        val task = object : Runnable {
            override fun run() {
                enqueueSessionAction {
                    if (unblockExpiryTask !== this) return@enqueueSessionAction
                    unblockExpiryTask = null
                    temporaryUnblocks.expire(System.currentTimeMillis(), android.os.SystemClock.elapsedRealtime())
                    reevaluateParentalControls()
                    armUnblockExpiration()
                }
            }
        }
        unblockExpiryTask = task
        if (unblockTimerHandler.postDelayed(task, delay)) return true
        clearTemporaryUnblocks()
        android.util.Log.w(com.maleicacid.tvinput.common.LogTags.TIS, "一時解除の失効通知を予約できないため解除を取り消します")
        return false
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

    private fun updateCurrentProgramVideoMetadata(generation: Long, info: PlaybackPipeline.VideoFormatInfo) {
        if (!PlaybackStartTransitions.acceptsGeneration(playbackState, generation)) return
        captionController.updateVideoGeometry(
            generation,
            info.width,
            info.height,
            info.displayAspectRatio,
        )
        superimposeController.updateVideoGeometry(
            generation,
            info.width,
            info.height,
            info.displayAspectRatio,
        )
        val key = currentService ?: return
        val now = System.currentTimeMillis()
        val transaction = latestLiveSnapshot?.programs ?: return
        val records = eventModelMapper.toProgramRecords(
            profile = transaction.discoveryProfile,
            events = transaction.events.filter { event ->
                ProgramVideoMetadataPolicy.eventContainsTime(event, key, now)
            },
            semanticFactsByServiceKey = transaction.semanticFactsByServiceKey,
            malformedCaDescriptorCountByServiceId = transaction.malformedCaDescriptorCountByServiceId,
            ratingProfileByServiceKey = mapOf(key to currentRatingProfile),
        )
        if (records.isEmpty()) return
        rememberVideoMetadata(records, info)
        publishLivePrograms(applyLatestVideoMetadata(records), transaction)
    }

    private fun refreshCurrentProgramRatingState() {
        when (val result = currentProgramRatingResolver.resolveDetailed(
            channelUri = currentChannelUri,
            serviceKey = currentService,
            latestEvents = latestLiveSnapshot?.programs?.events.orEmpty(),
            eitAuthority = currentProgramRatingResolver.eitAuthority(latestLiveSnapshot?.programs, currentService),
            ratingProfile = currentRatingProfile,
        )) {
            is CurrentProgramRatingResolver.ResolveResult.Ratings -> clearUnblocksIfCurrentProgramChanged(result.ratingSet)
            is CurrentProgramRatingResolver.ResolveResult.ProviderQueryFailed -> android.util.Log.w(com.maleicacid.tvinput.common.LogTags.TIS, "TvProvider rating query failure中のため unblock 状態を更新しません reason=${result.reason}")
        }
    }

    private fun publishLiveProgramsForCurrentService() {
        val key = currentService ?: return
        val transaction = latestLiveSnapshot?.programs ?: return
        val records = eventModelMapper.toProgramRecords(
            profile = transaction.discoveryProfile,
            events = transaction.events.filter { it.serviceKey == key },
            semanticFactsByServiceKey = transaction.semanticFactsByServiceKey,
            malformedCaDescriptorCountByServiceId = transaction.malformedCaDescriptorCountByServiceId,
            ratingProfileByServiceKey = mapOf(key to currentRatingProfile),
        )
        publishLivePrograms(applyLatestVideoMetadata(records), transaction)
    }

    private fun rememberVideoMetadata(records: List<ProgramRecord>, info: PlaybackPipeline.VideoFormatInfo) {
        records.forEach { record ->
            latestVideoMetadataByProgramKey[ProgramVideoMetadataPolicy.key(record)] = info
        }
    }

    private fun applyLatestVideoMetadata(records: List<ProgramRecord>): List<ProgramRecord> =
        ProgramVideoMetadataPolicy.merge(records, latestVideoMetadataByProgramKey)

    private fun publishLivePrograms(
        records: List<ProgramRecord>,
        snapshot: com.maleicacid.tvinput.aribsi.ProgramPublishSnapshot,
    ) {
        val key = currentService ?: return
        if (!currentServicePolicy().registrationReady) return
        val result = programPublishCoordinator.publishWithUpdates(
            mode = ChannelScanController.PublishMode.LIVE_TUNE_REFRESH,
            allPrograms = records,
            updateWindows = snapshot.updateWindows.filter { it.serviceKey == key }
                .map(ProgramPublishCoordinator::EpgUpdateWindow),
            allowedServiceKeys = setOf(key),
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
            latestEvents = latestLiveSnapshot?.programs?.events.orEmpty(),
            eitAuthority = currentProgramRatingResolver.eitAuthority(latestLiveSnapshot?.programs, currentService),
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
        val endTimeMillis = ratingSet.endTimeMillis ?: return
        if (!temporaryUnblocks.grant(unblockKey, endTimeMillis, System.currentTimeMillis(), android.os.SystemClock.elapsedRealtime())) return
        if (!armUnblockExpiration()) {
            reevaluateParentalControls()
            return
        }
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
        surface = null
        currentChannelUri = null
        captionEnabled = false
        selectedSubtitleTrackId = null
        clearTemporaryUnblocks()
        playbackState = PlaybackStartState.Stopped
        var failure: Throwable? = null
        fun release(action: () -> Unit) {
            try {
                action()
            } catch (error: Throwable) {
                val primary = failure
                if (primary == null) failure = error else if (primary !== error) primary.addSuppressed(error)
            }
        }
        release { captionController.close() }
        release { superimposeController.close() }
        release { unregisterParentalControlReceiver() }
        release { casController.close() }
        release { tunerController.release() }
        release { aribSiEngine.close() }
        // 解放未確認のsessionは使用中のまま保持し、EPG scanの受付を開かない。
        failure?.let { throw it }
        ChannelScanManager.unregisterLiveSession(appContext)
    }

    companion object {
        private const val ENABLE_CAS_ORCHESTRATION = true
        const val ACTION_SET_DUAL_MONO_PRESENTATION = "com.maleicacid.tvinput.tis.action.SET_DUAL_MONO_PRESENTATION"
        const val EXTRA_DUAL_MONO_PRESENTATION = "presentation"
        const val DUAL_MONO_MAIN = "main"
        const val DUAL_MONO_SUB = "sub"
        const val DUAL_MONO_MAIN_SUB = "main_sub"
    }
}

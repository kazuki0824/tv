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
    private var currentTrackSignature: Set<String> = emptySet()
    private val unblockedContentKeys = linkedSetOf<String>()
    private var currentUnblockProgramIdentityKey: String? = null
    private data class BlockedContent(val rating: TvContentRating, val unblockKey: String)
    private val parentalControlReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context?, intent: Intent?) {
            reevaluateParentalControls()
        }
    }

    init {
        tunerController.setSectionIngestController(sectionIngestController)
        tunerController.setCasController(casController)
        tunerController.setOnSectionIngestedCallback { refreshDynamicSiAndCasFilters() }
        tunerController.setPlaybackCallbacks(
            onVideoAvailable = { handleFirstFrameAvailable() },
            onVideoUnavailable = { reason -> handlePlaybackUnavailable(reason) },
        )
        tunerController.setOnVideoFormatDiscoveredCallback { info -> updateCurrentProgramVideoMetadata(info) }
        ChannelScanManager.registerLiveSession()
        registerParentalControlReceiver()
    }

    override fun onSetSurface(surface: Surface?): Boolean {
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

    override fun onSetStreamVolume(volume: Float) {
        streamVolume = volume.coerceIn(0.0f, 1.0f)
        tunerController.setStreamVolume(streamVolume)
    }

    override fun onSetCaptionEnabled(enabled: Boolean) {
        captionEnabled = enabled
    }

    override fun onTune(channelUri: Uri?): Boolean {
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
        val service = aribSiEngine.snapshotServices().firstOrNull { it.serviceKey == serviceKey }
        val pmtPids = aribSiEngine.snapshotPmtPidsForSectionFilters().filter { it in 0..0x1fff }.toSet()
        val allCaMetadata = if (ENABLE_CAS_ORCHESTRATION) aribSiEngine.snapshotCaMetadataForCasDiscovery() else emptyList()
        val serviceScopedCa = allCaMetadata.filter {
            it.serviceKey == serviceKey && it.source != com.maleicacid.tvinput.aribsi.CaMetadataSource.CAT
        }
        val catCa = allCaMetadata.filter { it.source == com.maleicacid.tvinput.aribsi.CaMetadataSource.CAT }
        val expanded = caMapper.expandProgramLevelToElementaryStreams(
            serviceScopedCa + catCa,
            aribSiEngine.snapshotServicesForCasDiscovery(),
        )
        val serviceCaMetadata = expanded.filter { it.serviceKey == serviceKey }
        val caMetadata = expanded.filter { it.serviceKey == null || it.serviceKey == serviceKey }
        val ecmPids = caMetadata.mapNotNull { it.ecmPid }.filter { it in 0..0x1fff }.toSet()
        val emmPids = caMetadata.mapNotNull { it.emmPid }.filter { it in 0..0x1fff }.toSet()
        tunerController.updateDynamicSectionFiltersForService(serviceKey, pmtPids, ecmPids, emmPids, currentGeneration)

        publishLiveProgramsForCurrentService()
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
        val selection = tunerController.selectAvStreams(service.serviceKey, service.pcrPid, service.streams, preferredAudioTrackId)
        val signature = playbackSignatureFor(service, selection) ?: return false
        if (!playbackStartGate.shouldAttempt(signature)) {
            return currentPlaybackSignature == signature && playbackStartGate.isStartedSignature(signature)
        }
        val previousSignature = currentPlaybackSignature
        val shouldStopBeforeRestart = previousSignature != null && previousSignature != signature && playbackStartGate.isStartedSignature(previousSignature)
        playbackStartGate.recordAttempt(signature)
        val result = tunerController.startPlayback(selection)
        if (result?.firstFramePending == true) {
            pendingPlaybackSignature = signature
            currentPlaybackSignature = null
            return false
        }
        val started = result?.startedVideo == true
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
            clear = true,
            keyTokenAvailable = false,
        )
    }

    override fun onSelectTrack(type: Int, trackId: String?): Boolean {
        val service = latestService ?: return false
        if (trackId == null) return false
        val tracks = tunerController.tracksFor(service.streams)
        return when (type) {
            TvTrackInfo.TYPE_AUDIO -> {
                if (tracks.none { it.type == TvTrackInfo.TYPE_AUDIO && it.id == trackId }) return false
                val previousAudioTrackId = preferredAudioTrackId
                val previousSignature = currentPlaybackSignature ?: return false
                preferredAudioTrackId = trackId
                val selection = tunerController.selectAvStreams(service.serviceKey, service.pcrPid, service.streams, preferredAudioTrackId)
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
                val currentVideo = tracks.firstOrNull { it.type == TvTrackInfo.TYPE_VIDEO }?.id
                if (trackId == currentVideo) {
                    notifyTrackSelected(TvTrackInfo.TYPE_VIDEO, trackId)
                    true
                } else {
                    false
                }
            }
            else -> false
        }
    }

    private fun updateTracks(service: AribService) {
        val tracks = tunerController.tracksFor(service.streams)
        val signature = tracks.map { track ->
            listOf(
                track.id,
                track.type.toString(),
                track.pid.toString(),
                track.streamType.toString(),
                track.componentTag?.toString() ?: "-1",
                track.language.orEmpty(),
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
            latestEvents = aribSiEngine.snapshotEvents(),
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
        val records = eventModelMapper.toProgramRecords(
            events = aribSiEngine.snapshotEvents().filter { event ->
                event.serviceKey == key && now >= event.startTimeMillis && now < event.startTimeMillis + event.durationMillis
            },
            publishabilityByServiceKey = aribSiEngine.snapshotPublishabilityDiagnostics().associateBy { it.serviceKey },
            channelFallbackByServiceKey = tvProviderWriter.existingChannels().associateBy { it.serviceKey },
        )
        if (records.isEmpty()) return
        rememberVideoMetadata(records, info)
        publishLivePrograms(applyLatestVideoMetadata(records))
    }

    private fun publishLiveProgramsForCurrentService() {
        val key = currentService ?: return
        val records = eventModelMapper.toProgramRecords(
            events = aribSiEngine.snapshotEvents().filter { it.serviceKey == key },
            publishabilityByServiceKey = aribSiEngine.snapshotPublishabilityDiagnostics().associateBy { it.serviceKey },
            channelFallbackByServiceKey = tvProviderWriter.existingChannels().associateBy { it.serviceKey },
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
        val rating = unblockedRating ?: return
        val ratingSet = currentProgramRatingResolver.resolve(
            channelUri = currentChannelUri,
            serviceKey = currentService,
            latestEvents = aribSiEngine.snapshotEvents(),
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
            surface = null
            currentChannelUri = null
            captionEnabled = false
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

package com.maleicacid.tvinput.tis

import android.content.Context
import android.media.tv.TvInputService
import android.media.tv.TvInputManager
import android.net.Uri
import android.view.Surface
import com.maleicacid.tvinput.aribsi.AribElementaryStream
import com.maleicacid.tvinput.aribsi.AribSiEngine
import com.maleicacid.tvinput.aribsi.PmtCatCaMetadataMapper
import com.maleicacid.tvinput.aribsi.SectionIngestController
import com.maleicacid.tvinput.common.ServiceKey

class MaleicacidLiveSession(
    context: Context,
    private val inputId: String,
    private val sessionId: String? = null,
    private val attributionSource: android.content.AttributionSource? = null,
) : TvInputService.Session(context) {
    private val aribSiEngine = AribSiEngine(context)
    private val sectionIngestController = SectionIngestController(aribSiEngine)
    private val tunerController = TunerController(context, inputId, attributionSource = attributionSource, sessionId = sessionId)
    private val casController = CasController()
    private val caMapper = PmtCatCaMetadataMapper()
    private var surface: Surface? = null
    private var currentService: ServiceKey? = null
    private var currentGeneration: Long = 0L
    private var captionEnabled: Boolean = false
    private var streamVolume: Float = 1.0f

    init {
        tunerController.setSectionIngestController(sectionIngestController)
        tunerController.setCasController(casController)
        tunerController.setOnSectionIngestedCallback { refreshDynamicSiAndCasFilters() }
        tunerController.setPlaybackCallbacks(
            onVideoAvailable = { notifyVideoAvailable() },
            onVideoUnavailable = { reason -> notifyVideoUnavailable(mapUnavailableReason(reason)) },
        )
    }

    override fun onSetSurface(surface: Surface?): Boolean {
        this.surface = surface
        tunerController.setSurface(surface)
        if (surface == null) {
            notifyVideoUnavailable(TvInputManager.VIDEO_UNAVAILABLE_REASON_UNKNOWN)
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
        PlaybackPipeline.PlaybackUnavailableReason.SURFACE_DETACHED -> TvInputManager.VIDEO_UNAVAILABLE_REASON_BUFFERING
        PlaybackPipeline.PlaybackUnavailableReason.FIRST_FRAME_TIMEOUT,
        PlaybackPipeline.PlaybackUnavailableReason.VIDEO_FILTER_NOT_STARTED,
        PlaybackPipeline.PlaybackUnavailableReason.AUDIO_FILTER_NOT_STARTED,
        PlaybackPipeline.PlaybackUnavailableReason.UNSUPPORTED_VIDEO_STREAM,
        PlaybackPipeline.PlaybackUnavailableReason.UNSUPPORTED_AUDIO_STREAM,
        PlaybackPipeline.PlaybackUnavailableReason.CAS_NO_KEY -> TvInputManager.VIDEO_UNAVAILABLE_REASON_TUNING
        else -> TvInputManager.VIDEO_UNAVAILABLE_REASON_UNKNOWN
    }

    private fun refreshDynamicSiAndCasFilters() {
        val serviceKey = currentService ?: return
        val service = aribSiEngine.snapshotServices().firstOrNull { it.serviceKey == serviceKey }
        val pmtPid = service?.pmtPid
        val expanded = if (ENABLE_CAS_ORCHESTRATION) caMapper.expandProgramLevelToElementaryStreams(aribSiEngine.snapshotCaMetadata(), aribSiEngine.snapshotServices()) else emptyList()
        val caMetadata = expanded.filter { it.serviceKey == null || it.serviceKey == serviceKey }
        val ecmPids = caMetadata.mapNotNull { it.ecmPid }.filter { it in 0..0x1fff }.toSet()
        val emmPids = caMetadata.mapNotNull { it.emmPid }.filter { it in 0..0x1fff }.toSet()
        tunerController.updateDynamicSectionFiltersForService(serviceKey, pmtPid, ecmPids, emmPids, currentGeneration)
        if (caMetadata.isEmpty()) {
            casController.clearForClearService()
        } else {
            val casResult = casController.updateFromCaMetadata(caMetadata, tunerController.createDescramblerBridge())
            if (casResult.diagnostics.any { it.state == CasController.State.ERROR }) {
                notifyVideoUnavailable(TvInputManager.VIDEO_UNAVAILABLE_REASON_UNKNOWN)
                return
            }
            notifyVideoUnavailable(mapUnavailableReason(PlaybackPipeline.PlaybackUnavailable(PlaybackPipeline.PlaybackUnavailableReason.CAS_NO_KEY, "r51 CAS placeholder cannot provide real key token")))
            return
        }
        if (service != null) {
            val selection = tunerController.selectAvStreams(serviceKey, service.pcrPid, service.streams)
            tunerController.startPlayback(selection)
        }
    }

    override fun onRelease() {
        surface = null
        captionEnabled = false
        casController.close()
        tunerController.release()
        aribSiEngine.close()
    }

    companion object {
        private const val ENABLE_CAS_ORCHESTRATION = true
    }
}

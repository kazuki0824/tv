package com.maleicacid.tvinput.tis

import android.media.tv.TvTrackInfo
import com.maleicacid.tvinput.aribsi.AribElementaryStream
import com.maleicacid.tvinput.common.StreamSelector
import com.maleicacid.tvinput.common.StreamSelectorType

/** Pure stream and track selection policy. Android Tuner resources remain owned by [TunerController]. */
object TunerSelectionPolicy {
    private val videoStreamTypes = setOf(0x02, 0x1b)
    private val audioStreamTypes = setOf(0x03, 0x04, 0x0f)
    private val captionDataComponentIds = setOf(0x0008, 0x0012)

    fun isSupportedVideoStreamType(streamType: Int): Boolean = streamType in videoStreamTypes
    fun isSupportedAudioStreamType(streamType: Int): Boolean = streamType in audioStreamTypes
    fun selectVideo(streams: List<AribElementaryStream>): AribElementaryStream? =
        streams.firstOrNull { isSupportedVideoStreamType(it.streamType) }

    fun hasSupportedVideo(streams: List<AribElementaryStream>): Boolean =
        streams.any { isSupportedVideoStreamType(it.streamType) }

    fun trackIdForVideo(stream: AribElementaryStream): String = "video:${stream.elementaryPid}"
    fun trackIdForAudio(stream: AribElementaryStream): String =
        stream.componentTag?.let { "audio:${stream.elementaryPid}:$it" } ?: "audio:${stream.elementaryPid}"

    fun trackIdForSubtitle(stream: AribElementaryStream): String =
        stream.componentTag?.let { "subtitle:${stream.elementaryPid}:$it" } ?: "subtitle:${stream.elementaryPid}"

    fun isCaptionStream(stream: AribElementaryStream): Boolean =
        stream.isCaption || stream.dataComponentId in captionDataComponentIds

    fun captionKind(stream: AribElementaryStream): String = when {
        stream.isSuperimpose -> "superimpose"
        stream.dataComponentId == 0x0012 -> "one-seg-caption"
        else -> "caption"
    }

    fun isCs110SelectorAllowed(satelliteBand: String?, selector: StreamSelector): Boolean =
        satelliteBand != "110CS" || selector.type == StreamSelectorType.NONE

    fun isSelectableTrack(type: Int, trackId: String?, tracks: List<TunerController.TisTrack>): Boolean = when (type) {
        TvTrackInfo.TYPE_AUDIO -> trackId != null && tracks.any { it.type == type && it.id == trackId }
        TvTrackInfo.TYPE_VIDEO -> trackId != null && tracks.firstOrNull { it.type == type }?.id == trackId
        TvTrackInfo.TYPE_SUBTITLE -> trackId != null && tracks.any { it.type == type && it.id == trackId }
        else -> false
    }
}

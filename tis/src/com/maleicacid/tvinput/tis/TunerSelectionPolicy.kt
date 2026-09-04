package com.maleicacid.tvinput.tis

import android.media.MediaFormat
import android.media.tv.TvTrackInfo
import com.maleicacid.tvinput.aribsi.AribComponentEntry
import com.maleicacid.tvinput.aribsi.AribElementaryStream
import com.maleicacid.tvinput.common.StreamSelector
import com.maleicacid.tvinput.common.StreamSelectorType

/** Pure stream and track selection policy. Android Tuner resources remain owned by [TunerController]. */
object TunerSelectionPolicy {
    private val videoStreamTypes = setOf(0x02, 0x1b)
    private val audioStreamTypes = setOf(0x03, 0x04, 0x0f)

    fun isSupportedVideoStreamType(streamType: Int): Boolean = streamType in videoStreamTypes
    fun isSupportedAudioStreamType(streamType: Int): Boolean = streamType in audioStreamTypes
    fun selectVideo(streams: List<AribElementaryStream>, componentGroupTags: Set<Int>? = null): AribElementaryStream? =
        selectDefault(streams.filter { isSupportedVideoStreamType(it.streamType) }, DEFAULT_VIDEO_COMPONENT_TAG, componentGroupTags)

    fun selectAudio(streams: List<AribElementaryStream>, componentGroupTags: Set<Int>? = null): AribElementaryStream? =
        selectDefault(streams.filter { isSupportedAudioStreamType(it.streamType) }, DEFAULT_AUDIO_COMPONENT_TAG, componentGroupTags)

    fun selectCaption(streams: List<AribElementaryStream>, componentGroupTags: Set<Int>? = null): AribElementaryStream? =
        selectDefault(streams.filter(::isCaptionStream), DEFAULT_CAPTION_COMPONENT_TAG, componentGroupTags)

    fun selectSuperimpose(streams: List<AribElementaryStream>, componentGroupTags: Set<Int>? = null): AribElementaryStream? =
        selectDefault(streams.filter(::isSuperimposeStream), DEFAULT_SUPERIMPOSE_COMPONENT_TAG, componentGroupTags)

    fun hasSupportedVideo(streams: List<AribElementaryStream>): Boolean =
        streams.any { isSupportedVideoStreamType(it.streamType) }

    fun trackIdForVideo(stream: AribElementaryStream): String = "video:${stream.elementaryPid}"
    fun trackIdForAudio(stream: AribElementaryStream): String =
        stream.componentTag?.let { "audio:${stream.elementaryPid}:$it" } ?: "audio:${stream.elementaryPid}"

    fun trackIdForSubtitle(stream: AribElementaryStream, languageId: Int = 1): String {
        val base = stream.componentTag?.let { "subtitle:${stream.elementaryPid}:$it" } ?: "subtitle:${stream.elementaryPid}"
        return "$base:lang$languageId"
    }

    fun trackIdForSuperimpose(stream: AribElementaryStream): String =
        stream.componentTag?.let { "superimpose:${stream.elementaryPid}:$it" } ?: "superimpose:${stream.elementaryPid}"

    fun isSuperimposeStream(stream: AribElementaryStream): Boolean = stream.isSuperimpose

    fun isCaptionStream(stream: AribElementaryStream): Boolean =
        !stream.isSuperimpose && (stream.isCaption || stream.dataComponentId == 0x0012)

    fun captionKind(stream: AribElementaryStream): String = when {
        stream.isSuperimpose -> "superimpose"
        stream.dataComponentId == 0x0012 -> "one-seg-caption"
        else -> "caption"
    }

    fun isCs110SelectorAllowed(satelliteBand: String?, selector: StreamSelector): Boolean =
        satelliteBand != "110CS" || selector.type == StreamSelectorType.NONE

    private fun selectDefault(
        candidates: List<AribElementaryStream>,
        defaultComponentTag: Int,
        componentGroupTags: Set<Int>?,
    ): AribElementaryStream? {
        val grouped = componentGroupTags
            ?.takeIf { it.isNotEmpty() }
            ?.let { tags -> candidates.filter { stream -> stream.componentTag?.let(tags::contains) == true } }
            ?.takeIf { it.isNotEmpty() }
        if (grouped != null) {
            return grouped.minWithOrNull(componentTagOrder)
        }
        return candidates.firstOrNull { it.componentTag == defaultComponentTag }
            ?: candidates.minWithOrNull(componentTagOrder)
    }

    fun orderedAudioStreams(streams: List<AribElementaryStream>, componentGroupTags: Set<Int>? = null): List<AribElementaryStream> =
        orderedWithDefault(streams.filter { isSupportedAudioStreamType(it.streamType) }, selectAudio(streams, componentGroupTags))

    fun orderedCaptionStreams(streams: List<AribElementaryStream>, componentGroupTags: Set<Int>? = null): List<AribElementaryStream> =
        orderedWithDefault(streams.filter(::isCaptionStream), selectCaption(streams, componentGroupTags))

    private fun orderedWithDefault(candidates: List<AribElementaryStream>, selected: AribElementaryStream?): List<AribElementaryStream> =
        buildList {
            selected?.let(::add)
            candidates.filterNot { it == selected }.sortedWith(componentTagOrder).forEach(::add)
        }

    private val componentTagOrder = compareBy<AribElementaryStream> { it.componentTag ?: Int.MAX_VALUE }
        .thenBy { it.elementaryPid.value }

    fun isSelectableTrack(type: Int, trackId: String?, tracks: List<TunerController.TisTrack>): Boolean = when (type) {
        TvTrackInfo.TYPE_AUDIO -> trackId != null && tracks.any { it.type == type && it.id == trackId }
        TvTrackInfo.TYPE_VIDEO -> trackId != null && tracks.firstOrNull { it.type == type }?.id == trackId
        TvTrackInfo.TYPE_SUBTITLE -> trackId != null && tracks.any { it.type == type && it.id == trackId }
        else -> false
    }

    private const val DEFAULT_VIDEO_COMPONENT_TAG = 0x00
    private const val DEFAULT_AUDIO_COMPONENT_TAG = 0x10
    private const val DEFAULT_CAPTION_COMPONENT_TAG = 0x30
    private const val DEFAULT_SUPERIMPOSE_COMPONENT_TAG = 0x38
}

/**
 * ARIB audio_component_descriptor facts to Android live-track metadata.
 * PMT stream_type remains the codec authority. Descriptor facts are used only when valid and
 * directly representable by TvTrackInfo; reserved/ambiguous values are left unset.
 */
object AudioTrackMetadataPolicy {
    data class Projection(
        val language: String?,
        val encoding: String?,
        val channelCount: Int?,
        val sampleRateHz: Int?,
        val description: String?,
        val audioDescription: Boolean,
        val hardOfHearing: Boolean,
    )

    fun project(
        pmtStreamType: Int,
        fallbackLanguage: String?,
        component: AribComponentEntry?,
    ): Projection {
        val valid = component?.takeIf { it.parseStatus.equals("OK", ignoreCase = true) }
        val componentType = valid?.componentType
        return Projection(
            language = valid?.language?.takeIf { it.isNotBlank() } ?: fallbackLanguage,
            encoding = encodingForPmtStreamType(pmtStreamType),
            channelCount = componentType?.let(::channelCountForComponentType),
            sampleRateHz = valid?.samplingRate?.let(::sampleRateHz),
            description = valid?.text?.takeIf { it.isNotBlank() },
            audioDescription = componentType?.let(::isAudioDescription) == true,
            hardOfHearing = componentType?.let(::isHardOfHearing) == true,
        )
    }

    fun encodingForPmtStreamType(streamType: Int): String? = when (streamType) {
        0x03, 0x04 -> MediaFormat.MIMETYPE_AUDIO_MPEG
        0x0f -> MediaFormat.MIMETYPE_AUDIO_AAC
        else -> null
    }

    fun channelCountForComponentType(componentType: Int): Int? = when (componentType and 0x1f) {
        0x01 -> 1
        0x02 -> 2
        0x03 -> 2
        0x04, 0x05 -> 3
        0x06, 0x07 -> 4
        0x08 -> 5
        0x09 -> 6
        else -> null
    }

    fun sampleRateHz(rawSamplingRate: Int): Int? = when (rawSamplingRate) {
        0x01 -> 16_000
        0x02 -> 22_050
        0x03 -> 24_000
        0x05 -> 32_000
        0x06 -> 44_100
        0x07 -> 48_000
        else -> null
    }

    fun isAudioDescription(componentType: Int): Boolean = ((componentType ushr 5) and 0x03) == 0x01
    fun isHardOfHearing(componentType: Int): Boolean = ((componentType ushr 5) and 0x03) == 0x02
}

/** EIT component_descriptor facts that have a direct TvTrackInfo video representation. */
object VideoTrackMetadataPolicy {
    data class Projection(
        val description: String?,
        val width: Int?,
        val height: Int?,
    )

    fun project(component: AribComponentEntry?): Projection {
        val valid = component?.takeIf { it.parseStatus.equals("OK", ignoreCase = true) }
            ?: return Projection(null, null, null)
        return Projection(
            description = valid.text?.takeIf { it.isNotBlank() },
            width = null,
            height = null,
        )
    }
}

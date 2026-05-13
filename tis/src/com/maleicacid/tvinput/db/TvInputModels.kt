package com.maleicacid.tvinput.db

import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.StreamSelector

data class ChannelRecord(
    val serviceKey: ServiceKey,
    val displayNumber: String,
    val displayName: String,
    val frequencyHz: Long,
    val tvProviderChannelId: Long? = null,
    val deliverySystem: String = DELIVERY_SYSTEM_ISDB_T,
    val streamSelector: StreamSelector = StreamSelector.NONE,
    val physicalChannel: Int? = null,
    val backendHint: String? = null,
    val satelliteBand: String? = null,
    val remoteControlKeyId: Int? = null,
    val requiresCas: Boolean = false,
    val unsupportedCas: Boolean = false,
    val clearLivePlaybackSupported: Boolean = false,
    val channelRegistrationReady: Boolean = false,
    val epgPublishable: Boolean = false,
) {
    companion object {
        const val DELIVERY_SYSTEM_ISDB_T = "ISDB_T"
        const val DELIVERY_SYSTEM_ISDB_S = "ISDB_S"
    }
}

data class ProgramRecord(
    val serviceKey: ServiceKey,
    val eventId: Int,
    val stableIdentity: String,
    val startTimeMillis: Long,
    val durationMillis: Long,
    val title: String,
    val description: String,
    val shortDescription: String = description.lineSequence().firstOrNull()?.take(256).orEmpty(),
    val extendedItemsJson: String = "[]",
    val componentText: String? = null,
    val audioComponentText: String? = null,
    val audioLanguage: String? = null,
    val canonicalGenre: String? = null, // 非推奨。r51 では COLUMN_CANONICAL_GENRE を書かない
    val broadcastGenre: String? = null,
    val genreSupplementText: String? = null,
    val eventGroupText: String? = null,
    val freeCaText: String? = null,
    val seriesName: String? = null,
    val requiresCas: Boolean = false,
    val unsupportedCas: Boolean = false,
    val clearLivePlaybackSupported: Boolean = false,
    val channelRegistrationReady: Boolean = false,
    val epgPublishable: Boolean = false,
    val publishStateSource: String = "NONE",
    val diagnosticText: String = "",
    val diagnosticDescriptorJson: String = "{}",
    val contentRatings: List<String> = emptyList(),
    val parentalRatingDiagnosticsJson: String = "{\"parentalRatings\":[]}",
    val videoWidth: Int? = null,
    val videoHeight: Int? = null,
    val videoFormat: String? = null,
    val unsupportedDescriptorJson: String = "{}",
    val malformedCaDescriptorCount: Int = 0,
    // Program provider-data 診断。ProgramPublishCoordinator が所有する
    // process内だけの再試行状態であり、process再起動時にresetされる。
    val droppedRetryWindowCount: Int = 0,
    val tvProviderProgramId: Long? = null,
)

data class CaMetadataRecord(
    val serviceKey: ServiceKey,
    val caSystemId: Int,
    val ecmPid: Int?,
    val emmPid: Int?,
    val elementaryPid: Int?,
)

package com.maleicacid.tvinput.db

import com.maleicacid.tvinput.aribsi.AribComponents
import com.maleicacid.tvinput.aribsi.AribContentGenre
import com.maleicacid.tvinput.aribsi.AribFreeCaMode
import com.maleicacid.tvinput.aribsi.AribLinkage
import com.maleicacid.tvinput.aribsi.AribParentalRating
import com.maleicacid.tvinput.aribsi.AribProgramSource
import com.maleicacid.tvinput.aribsi.AribEventGroup
import com.maleicacid.tvinput.aribsi.AribSeries
import com.maleicacid.tvinput.common.FrequencyHz
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.StreamSelector

data class ChannelRecord(
    val serviceKey: ServiceKey,
    val serviceType: Int,
    val displayNumber: String,
    val displayName: String,
    val frequencyHz: FrequencyHz,
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
    val inputId: String? = null,
) {
    companion object {
        const val DELIVERY_SYSTEM_ISDB_T = "ISDB_T"
        const val DELIVERY_SYSTEM_ISDB_S = "ISDB_S"
    }
}

data class ProgramDescriptors(
    val extendedItems: List<com.maleicacid.tvinput.aribsi.AribExtendedItem> = emptyList(),
    val componentText: String? = null,
    val audioComponentText: String? = null,
    val audioLanguage: String? = null,
    val contentGenres: List<AribContentGenre> = emptyList(),
    val broadcastGenre: String? = null,
    val genreSupplementText: String? = null,
    val eventGroups: List<AribEventGroup> = emptyList(),
    val linkage: List<AribLinkage> = emptyList(),
    val scrambled: Boolean? = null,
    val freeCaMode: AribFreeCaMode? = null,
    val seriesId: Int? = null,
    val episodeNumber: Int? = null,
    val lastEpisodeNumber: Int? = null,
    val series: AribSeries? = null,
    val descriptorDiagnosticsCanonicalJson: String = "[]",
    val parentalRatings: List<AribParentalRating> = emptyList(),
    val components: AribComponents = AribComponents(),
)

data class ProgramRecord(
    val serviceKey: ServiceKey,
    val eventId: Int,
    val stableIdentity: String,
    val startTimeMillis: Long,
    val durationMillis: Long,
    val title: String,
    val description: String,
    val shortDescription: String = description.lineSequence().firstOrNull()?.take(256).orEmpty(),
    val canonicalGenres: List<String> = emptyList(),
    val descriptors: ProgramDescriptors = ProgramDescriptors(),
    val source: AribProgramSource = AribProgramSource(),
    val requiresCas: Boolean = false,
    val unsupportedCas: Boolean = false,
    val clearLivePlaybackSupported: Boolean = false,
    val channelRegistrationReady: Boolean = false,
    val epgPublishable: Boolean = false,
    val publishStateSource: String = "NONE",
    val diagnosticText: String = "",
    val contentRatings: List<String> = emptyList(),
    val videoWidth: Int? = null,
    val videoHeight: Int? = null,
    val videoFormat: String? = null,
    val malformedCaDescriptorCount: Int = 0,
    val tvProviderProgramId: Long? = null,
)

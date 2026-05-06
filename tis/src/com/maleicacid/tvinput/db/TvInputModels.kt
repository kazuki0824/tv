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
    val canonicalGenre: String? = null,
    val genreSupplementText: String? = null,
    val eventGroupText: String? = null,
    val freeCaText: String? = null,
    val seriesName: String? = null,
    val diagnosticText: String = "",
    val diagnosticDescriptorJson: String = "{}",
    val tvProviderProgramId: Long? = null,
)

data class CaMetadataRecord(
    val serviceKey: ServiceKey,
    val caSystemId: Int,
    val ecmPid: Int?,
    val emmPid: Int?,
    val elementaryPid: Int?,
)

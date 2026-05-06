package com.maleicacid.tvinput.aribsi

import com.maleicacid.tvinput.common.ServiceKey

object SiStatus {
    const val OK = 0
    const val IGNORED_UNSUPPORTED_PID_OR_TABLE = 1
    const val INVALID_HANDLE = -1
    const val INVALID_PID = -2
    const val INVALID_SECTION = -3
    const val MALFORMED_DESCRIPTOR = -4
    const val INDEX_OUT_OF_RANGE = -5
    const val JNI_ERROR = -6
    const val INTERNAL_ERROR = -7
}

object SiDiscoveryStage {
    const val INCOMPLETE = 0
    const val PARTIAL = 1
    const val COMPLETE = 2
}

data class SiIngestResult(
    val pid: Int,
    val status: Int,
)

data class PmtPidMapping(
    val serviceKey: ServiceKey,
    val pmtPid: Int,
)

data class AribElementaryStream(
    val elementaryPid: Int,
    val streamType: Int,
    val componentTag: Int?,
    val componentType: Int?,
    val streamContent: Int?,
    val languageCodes: List<String> = emptyList(),
)

data class AribService(
    val serviceKey: ServiceKey,
    val name: String,
    val providerName: String = "",
    val serviceType: Int? = null,
    val pmtPid: Int? = null,
    val pcrPid: Int? = null,
    val freeCaMode: Boolean? = null,
    val streams: List<AribElementaryStream> = emptyList(),
)

data class AribTransport(
    val originalNetworkId: Int,
    val transportStreamId: Int,
    val networkName: String = "",
    val transportStreamName: String = "",
    val remoteControlKeyId: Int? = null,
)

data class AribExtendedItem(
    val itemDescription: String,
    val itemText: String,
)

data class AribEvent(
    val serviceKey: ServiceKey,
    val stableIdentity: String,
    val eventId: Int,
    val startTimeMillis: Long,
    val durationMillis: Long,
    val title: String,
    val description: String,
    val extendedDescription: String = "",
    val eventScope: String = "present_following",
    val extendedItems: List<AribExtendedItem> = emptyList(),
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
    val textDiagnostics: List<String> = emptyList(),
)

data class AribEventDiagnostic(
    val serviceKey: ServiceKey,
    val stableIdentity: String,
    val eventId: Int,
    val diagnosticText: String,
    val diagnosticDescriptorJson: String,
)

data class ServicePublishabilityDiagnostic(
    val serviceKey: ServiceKey,
    val publishable: Boolean,
    val missingComponents: List<String>,
)

enum class CaMetadataSource { PROGRAM, ELEMENTARY_STREAM, CAT }

data class CaMetadata(
    val serviceKey: ServiceKey?,
    val caSystemId: Int,
    val ecmPid: Int?,
    val emmPid: Int?,
    val elementaryPid: Int?,
    val privateData: ByteArray = ByteArray(0),
    val source: CaMetadataSource = when {
        ecmPid != null && elementaryPid != null -> CaMetadataSource.ELEMENTARY_STREAM
        ecmPid != null -> CaMetadataSource.PROGRAM
        else -> CaMetadataSource.CAT
    },
) {
    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other !is CaMetadata) return false
        return serviceKey == other.serviceKey &&
            caSystemId == other.caSystemId &&
            ecmPid == other.ecmPid &&
            emmPid == other.emmPid &&
            elementaryPid == other.elementaryPid &&
            privateData.contentEquals(other.privateData) &&
            source == other.source
    }

    override fun hashCode(): Int {
        var result = serviceKey?.hashCode() ?: 0
        result = 31 * result + caSystemId
        result = 31 * result + (ecmPid ?: 0)
        result = 31 * result + (emmPid ?: 0)
        result = 31 * result + (elementaryPid ?: 0)
        result = 31 * result + privateData.contentHashCode()
        result = 31 * result + source.hashCode()
        return result
    }
}


data class PrivateSection(
    val pid: Int,
    val tableId: Int,
    val bytes: ByteArray,
) {
    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other !is PrivateSection) return false
        return pid == other.pid && tableId == other.tableId && bytes.contentEquals(other.bytes)
    }

    override fun hashCode(): Int {
        var result = pid
        result = 31 * result + tableId
        result = 31 * result + bytes.contentHashCode()
        return result
    }
}

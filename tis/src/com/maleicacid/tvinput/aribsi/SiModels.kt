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

enum class CaDescriptorScope { PROGRAM, ES }

data class CaDescriptor(
    val caSystemId: Int,
    val caPid: Int?,
    val scope: CaDescriptorScope,
    val esPid: Int?,
    val rawDescriptor: ByteArray,
) {
    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other !is CaDescriptor) return false
        return caSystemId == other.caSystemId && caPid == other.caPid && scope == other.scope && esPid == other.esPid && rawDescriptor.contentEquals(other.rawDescriptor)
    }

    override fun hashCode(): Int {
        var result = caSystemId
        result = 31 * result + (caPid ?: 0)
        result = 31 * result + scope.hashCode()
        result = 31 * result + (esPid ?: 0)
        result = 31 * result + rawDescriptor.contentHashCode()
        return result
    }
}

data class AribElementaryStream(
    val elementaryPid: Int,
    val streamType: Int,
    val componentTag: Int?,
    val componentType: Int?,
    val streamContent: Int?,
    val languageCodes: List<String> = emptyList(),
    val dataComponentId: Int? = null,
    val isCaption: Boolean = false,
    val isSuperimpose: Boolean = false,
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
    val hasProgramCaDescriptor: Boolean = false,
    val hasEsCaDescriptor: Boolean = false,
    val serviceScopedCaDescriptors: List<CaDescriptor> = emptyList(),
    val components: AribComponents = AribComponents(),
) {
    val requiresCas: Boolean get() = hasProgramCaDescriptor || hasEsCaDescriptor
}

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

data class AribParentalRating(
    val countryCode: String,
    val ratingValue: Int,
    val rawRatingByte: Int,
    val supported: Boolean,
    val parseStatus: String = "OK",
)

data class AribContentGenre(
    val level1: Int,
    val level2: Int,
    val userNibble: Int = 0,
    val aribName: String = "",
    val parseStatus: String = "OK",
)


data class AribRelatedItem(
    val kind: String,
    val groupType: Int,
    val originalNetworkId: Int?,
    val transportStreamId: Int?,
    val serviceId: Int,
    val eventId: Int,
    val parseStatus: String = "OK",
)

data class AribLinkage(
    val linkageType: Int,
    val originalNetworkId: Int,
    val transportStreamId: Int,
    val serviceId: Int,
    val privateDataHex: String = "",
    val parseStatus: String = "OK",
)

data class AribFreeCaMode(
    val raw: Int?,
    val scrambled: Boolean?,
    val text: String?,
    val parseStatus: String = "OK",
)

data class AribSeries(
    val seriesId: Int?,
    val repeatLabel: Int = 0,
    val programPattern: Int = 0,
    val expireDateValid: Boolean = false,
    val expireDate: Int? = null,
    val episodeNumber: Int?,
    val lastEpisodeNumber: Int?,
    val name: String?,
    val parseStatus: String = "OK",
)

data class AribComponentEntry(
    val esPid: Int,
    val streamType: Int? = null,
    val componentTag: Int? = null,
    val componentType: Int? = null,
    val codec: String? = null,
    val language: String? = null,
    val secondLanguage: String? = null,
    val channelConfiguration: String? = null,
    val samplingInfo: String? = null,
    val sourceDescriptor: String? = null,
    val resolution: String? = null,
    val scan: String? = null,
    val aspect: String? = null,
    val profileLevel: String? = null,
    val dataComponentId: Int? = null,
    val trackId: String? = null,
    val captionServiceKind: String? = null,
    val r51PlaybackSupported: Boolean? = null,
    val liveViewableClaim: Boolean? = null,
    val diagnosticCode: String? = null,
    val main: Boolean? = null,
    val multiLingual: Boolean? = null,
    val qualityIndicator: Int? = null,
    val parseStatus: String = "OK",
)

data class AribComponents(
    val video: List<AribComponentEntry> = emptyList(),
    val audio: List<AribComponentEntry> = emptyList(),
    val subtitle: List<AribComponentEntry> = emptyList(),
    val data: List<AribComponentEntry> = emptyList(),
)

data class AribEventDiagnostics(
    val summary: String = "",
    val descriptorDiagnosticsCanonicalJson: String = "[]",
    val textDiagnostics: List<String> = emptyList(),
)

data class AribProgramSource(
    val pid: Int = 18,
    val tableId: Int = 0x4e,
    val version: Int = 0,
    val sectionNumber: Int = 0,
    val lastSectionNumber: Int = 0,
)

data class AribEventDescriptors(
    val extendedItems: List<AribExtendedItem> = emptyList(),
    val componentText: String? = null,
    val audioComponentText: String? = null,
    val audioLanguage: String? = null,
    val contentGenres: List<AribContentGenre> = emptyList(),
    val broadcastGenre: String? = null,
    val genreSupplementText: String? = null,
    val relatedItems: List<AribRelatedItem> = emptyList(),
    val linkage: List<AribLinkage> = emptyList(),
    val scrambled: Boolean? = null,
    val freeCaMode: AribFreeCaMode? = null,
    val seriesId: Int? = null,
    val episodeNumber: Int? = null,
    val lastEpisodeNumber: Int? = null,
    val series: AribSeries? = null,
    val parentalRatings: List<AribParentalRating> = emptyList(),
    val components: AribComponents = AribComponents(),
    val diagnostics: AribEventDiagnostics = AribEventDiagnostics(),
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
    val source: AribProgramSource = AribProgramSource(),
    val descriptors: AribEventDescriptors = AribEventDescriptors(),
)

data class AribEventDiagnostic(
    val serviceKey: ServiceKey,
    val stableIdentity: String,
    val eventId: Int,
    val diagnosticText: String,
)

data class DescriptorDiagnosticScope(
    val pid: Int?,
    val tableId: Int?,
    val tableIdExtension: Int?,
    val version: Int?,
    val sectionNumber: Int?,
    val originalNetworkId: Int?,
    val transportStreamId: Int?,
    val serviceId: Int?,
    val eventId: Int?,
)

data class DescriptorDiagnosticDescriptor(
    val tag: Int,
    val name: String?,
    val offset: Int,
    val declaredLength: Int,
    val actualRemainingLength: Int,
    val parseStatus: String,
    val rawPrefixHex: String,
)

data class DescriptorDiagnostic(
    val schema: String,
    val schemaVersion: Int,
    val severity: String,
    val code: String,
    val scope: DescriptorDiagnosticScope,
    val descriptor: DescriptorDiagnosticDescriptor,
    val message: String,
    val rawJson: String,
)

data class AribEpgUpdateWindow(
    val serviceKey: ServiceKey,
    val windowStartMillis: Long,
    val windowEndMillis: Long,
    val validProgramStableIdentities: List<String>,
    val deletionAuthoritative: Boolean = false,
)

typealias EpgUpdateWindow = AribEpgUpdateWindow
typealias ProgramPublishability = ServicePublishabilityDiagnostic

data class ParserDiagnostic(
    val code: String,
    val message: String,
    val severity: String? = null,
)

data class MalformedCaDescriptorDiagnostic(
    val pid: Int,
    val tableId: Int,
    val tableIdExtension: Int?,
    val serviceId: Int?,
    val elementaryPid: Int?,
    val scope: String,
    val offset: Int,
    val declaredLength: Int,
    val actualRemainingLength: Int,
    val reason: String,
    val rawPrefixHex: String,
)

data class TransportKey(
    val originalNetworkId: Int,
    val transportStreamId: Int,
)

data class ProgramPublishSnapshot(
    val snapshotGeneration: Long,
    val ingestSequence: Long,
    val events: List<AribEvent>,
    val updateWindows: List<EpgUpdateWindow>,
    val publishabilityByServiceKey: Map<ServiceKey, ProgramPublishability>,
    val descriptorDiagnostics: List<DescriptorDiagnostic>,
    val parserDiagnostics: List<ParserDiagnostic>,
    val malformedCaDescriptorCountByServiceId: Map<Int, Int> = emptyMap(),
)

data class ServiceRegistrationSnapshot(
    val snapshotGeneration: Long,
    val services: List<AribService>,
    val actualTransports: Set<TransportKey>,
    val publishabilityByServiceKey: Map<ServiceKey, ProgramPublishability>,
    val diagnostics: List<ParserDiagnostic>,
)

data class CasDiscoverySnapshot(
    val snapshotGeneration: Long,
    val services: List<AribService>,
    val caMetadata: List<CaMetadata>,
    val pmtPids: Map<ServiceKey, Int>,
    val catEmmPids: List<Int>,
    val diagnostics: List<DescriptorDiagnostic>,
    val malformedCaDescriptorDiagnostics: List<MalformedCaDescriptorDiagnostic> = emptyList(),
)

data class ServicePublishabilityDiagnostic(
    val serviceKey: ServiceKey,
    val publishable: Boolean,
    val channelRegistrationReady: Boolean,
    val epgPublishable: Boolean,
    val clearLivePlaybackSupported: Boolean,
    val requiresCas: Boolean,
    val unsupportedCas: Boolean,
    val pmtPidResolved: Boolean = false,
    val pmtParsed: Boolean = false,
    val caStateResolved: Boolean = false,
    val freeCaModeResolved: Boolean = false,
    val missingComponents: List<String>,
    val reasons: List<String>,
    val registrationReasons: List<String>,
    val epgReasons: List<String>,
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

package com.maleicacid.tvinput.aribsi

import com.maleicacid.tvinput.common.NetworkId16
import com.maleicacid.tvinput.common.ServiceId16
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.TransportStreamId16
import com.maleicacid.tvinput.common.TsPid

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
    const val INVALID_DISCOVERY_PROFILE = -8
}

object SiDiscoveryStage {
    const val INCOMPLETE = 0
    const val PARTIAL = 1
    const val COMPLETE = 2
}

object SiDiscoveryProfile {
    const val ISDB_T: Int = 0
    const val BS: Int = 1
    const val CS110: Int = 2
}

data class SiIngestResult(
    val pid: TsPid,
    val status: Int,
)

data class PmtPidMapping(
    val serviceKey: ServiceKey,
    val pmtPid: TsPid,
)

enum class CaDescriptorScope { PROGRAM, ES }

data class CaDescriptor(
    val caSystemId: Int,
    val caPid: TsPid?,
    val scope: CaDescriptorScope,
    val esPid: TsPid?,
    val rawDescriptor: ByteArray,
) {
    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other !is CaDescriptor) return false
        return caSystemId == other.caSystemId && caPid == other.caPid && scope == other.scope && esPid == other.esPid && rawDescriptor.contentEquals(other.rawDescriptor)
    }

    override fun hashCode(): Int {
        var result = caSystemId
        result = 31 * result + (caPid?.value ?: 0)
        result = 31 * result + scope.hashCode()
        result = 31 * result + (esPid?.value ?: 0)
        result = 31 * result + rawDescriptor.contentHashCode()
        return result
    }
}

data class AribElementaryStream(
    val elementaryPid: TsPid,
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
    val pmtPid: TsPid? = null,
    val pcrPid: TsPid? = null,
    val freeCaMode: Boolean? = null,
    val streams: List<AribElementaryStream> = emptyList(),
    val serviceScopedCaDescriptors: List<CaDescriptor> = emptyList(),
) {
    val hasProgramCaDescriptor: Boolean
        get() = serviceScopedCaDescriptors.any { it.scope == CaDescriptorScope.PROGRAM }
    val hasEsCaDescriptor: Boolean
        get() = serviceScopedCaDescriptors.any { it.scope == CaDescriptorScope.ES }
    val requiresCas: Boolean get() = serviceScopedCaDescriptors.isNotEmpty() || freeCaMode == true
}

data class AribTransport(
    val originalNetwork: NetworkId16,
    val transportStream: TransportStreamId16,
    val networkName: String = "",
    val transportStreamName: String = "",
    val remoteControlKeyId: Int? = null,
) {
    val originalNetworkId: Int get() = originalNetwork.value
    val transportStreamId: Int get() = transportStream.value
}

data class AribExtendedItem(
    val languageCode: String,
    val itemDescription: String,
    val itemText: String,
)

data class AribParentalRating(
    val countryCode: String,
    val rawRatingByte: Int,
    val parseStatus: String = "OK",
)

data class AribContentGenre(
    val level1: Int,
    val level2: Int,
    val userNibble: Int = 0,
    val aribName: String = "",
    val parseStatus: String = "OK",
)


data class AribEventGroupReference(
    val service: ServiceId16,
    val eventId: Int,
) {
    val serviceId: Int get() = service.value
}

data class AribOtherNetworkEventGroupReference(
    val originalNetwork: NetworkId16,
    val transportStream: TransportStreamId16,
    val service: ServiceId16,
    val eventId: Int,
) {
    val originalNetworkId: Int get() = originalNetwork.value
    val transportStreamId: Int get() = transportStream.value
    val serviceId: Int get() = service.value
}

data class AribEventGroup(
    val groupType: Int,
    val events: List<AribEventGroupReference> = emptyList(),
    val otherNetworkEvents: List<AribOtherNetworkEventGroupReference> = emptyList(),
    val privateDataHex: String = "",
    val parseStatus: String = "OK",
)

data class AribLinkage(
    val linkageType: Int,
    val serviceKey: ServiceKey,
    val privateDataHex: String = "",
    val parseStatus: String = "OK",
) {
    val originalNetworkId: Int get() = serviceKey.originalNetworkId
    val transportStreamId: Int get() = serviceKey.transportStreamId
    val serviceId: Int get() = serviceKey.serviceId
}

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
    val esPid: TsPid,
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
    val captionServiceKind: String? = null,
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
    val pid: TsPid = TsPid.EIT,
    val tableId: Int = 0x4e,
    val version: Int = 0,
    val sectionNumber: Int = 0,
    val lastSectionNumber: Int = 0,
)

data class AribEventDescriptors(
    val extendedItems: List<AribExtendedItem> = emptyList(),
    val componentText: String? = null,
    val audioComponentText: String? = null,
    val contentGenres: List<AribContentGenre> = emptyList(),
    val broadcastGenre: String? = null,
    val genreSupplementText: String? = null,
    val eventGroups: List<AribEventGroup> = emptyList(),
    val linkage: List<AribLinkage> = emptyList(),
    val scrambled: Boolean? = null,
    val freeCaMode: AribFreeCaMode? = null,
    val series: AribSeries? = null,
    val parentalRatings: List<AribParentalRating> = emptyList(),
    val components: AribComponents = AribComponents(),
    val diagnostics: AribEventDiagnostics = AribEventDiagnostics(),
)

data class AribEvent(
    val serviceKey: ServiceKey,
    val stableIdentity: String,
    val eventId: Int,
    val timingState: String = "DEFINED",
    val rawStartTimeHex: String = "",
    val rawDurationHex: String = "",
    val startTimeMillis: Long,
    val durationMillis: Long,
    val title: String,
    val description: String,
    val extendedDescription: String = "",
    val providerDataCanonicalJson: String = "",
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
    val pid: TsPid?,
    val tableId: Int?,
    val tableIdExtension: Int?,
    val version: Int?,
    val sectionNumber: Int?,
    val originalNetwork: NetworkId16?,
    val transportStream: TransportStreamId16?,
    val service: ServiceId16?,
    val eventId: Int?,
) {
    val originalNetworkId: Int? get() = originalNetwork?.value
    val transportStreamId: Int? get() = transportStream?.value
    val serviceId: Int? get() = service?.value
}

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

data class SmdSemanticFacts(
    val descriptorPresent: Boolean,
    val syntaxValid: Boolean,
    val systemManagementId: Int?,
    val broadcastingFlag: Int?,
    val broadcastingIdentifier: Int?,
    val additionalBroadcastingIdentification: Int?,
    val additionalIdentificationInfoHex: String,
    val semanticState: String,
    val diagnostic: String?,
)

data class ServiceSemanticFacts(
    val serviceKey: ServiceKey,
    val serviceType: Int?,
    val pmtPidResolved: Boolean,
    val pmtParsed: Boolean,
    val pcrPidResolved: Boolean,
    val elementaryStreams: List<AribElementaryStream>,
    val requiresCas: Boolean,
    val caDescriptorsResolved: Boolean,
    val freeCaMode: Boolean?,
    val smd: SmdSemanticFacts,
    val missingComponents: List<String>,
    val semanticDiagnostics: List<String>,
)

data class MalformedCaDescriptorDiagnostic(
    val pid: TsPid,
    val tableId: Int,
    val tableIdExtension: Int?,
    val service: ServiceId16?,
    val elementaryPid: TsPid?,
    val scope: String,
    val offset: Int,
    val declaredLength: Int,
    val actualRemainingLength: Int,
    val reason: String,
    val rawPrefixHex: String,
) {
    val serviceId: Int? get() = service?.value
}

data class TransportKey(
    val originalNetwork: NetworkId16,
    val transportStream: TransportStreamId16,
) {
    val originalNetworkId: Int get() = originalNetwork.value
    val transportStreamId: Int get() = transportStream.value
}

data class ProgramPublishSnapshot(
    val ingestSequence: Long,
    val events: List<AribEvent>,
    val updateWindows: List<EpgUpdateWindow>,
    val semanticFactsByServiceKey: Map<ServiceKey, ServiceSemanticFacts>,
    val descriptorDiagnostics: List<DescriptorDiagnostic>,
    val parserDiagnostics: List<ParserDiagnostic>,
    val malformedCaDescriptorCountByServiceId: Map<ServiceId16, Int> = emptyMap(),
)

data class TableRequirementStatus(
    val component: String,
    val originalNetworkId: Int?,
    val transportStreamId: Int?,
    val serviceId: Int?,
    val required: Boolean,
    val complete: Boolean,
)

data class ServiceRegistrationSnapshot(
    val discoveryStage: Int,
    val tableRequirements: List<TableRequirementStatus>,
    val services: List<AribService>,
    val actualTransports: Set<TransportKey>,
    val actualTransportMetadata: List<AribTransport>,
    val semanticFactsByServiceKey: Map<ServiceKey, ServiceSemanticFacts>,
    val diagnostics: List<ParserDiagnostic>,
)

data class CasDiscoverySnapshot(
    val services: List<AribService>,
    val caMetadata: List<CaMetadata>,
    val pmtPids: Map<ServiceKey, TsPid>,
    val catEmmPids: List<TsPid>,
    val diagnostics: List<DescriptorDiagnostic>,
    val malformedCaDescriptorDiagnostics: List<MalformedCaDescriptorDiagnostic> = emptyList(),
)

data class LivePlaybackSnapshot(
    val ingestSequence: Long,
    val services: List<AribService>,
    val caMetadata: List<CaMetadata>,
    val pmtPids: Map<ServiceKey, TsPid>,
    val catEmmPids: List<TsPid>,
    val semanticFactsByServiceKey: Map<ServiceKey, ServiceSemanticFacts>,
    val descriptorDiagnostics: List<DescriptorDiagnostic>,
    val parserDiagnostics: List<ParserDiagnostic>,
    val malformedCaDescriptorDiagnostics: List<MalformedCaDescriptorDiagnostic> = emptyList(),
)

data class ServicePolicyDecision(
    val serviceKey: ServiceKey,
    val registrationReady: Boolean,
    val requiresCas: Boolean,
    val reasons: List<String>,
) {
    val clearLivePlaybackSupported: Boolean get() = registrationReady && !requiresCas
}

typealias ServicePublishabilityDiagnostic = ServicePolicyDecision

enum class CaMetadataSource { PROGRAM, ELEMENTARY_STREAM, CAT }

data class CaMetadata(
    val serviceKey: ServiceKey?,
    val caSystemId: Int,
    val ecmPid: TsPid?,
    val emmPid: TsPid?,
    val elementaryPid: TsPid?,
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
        result = 31 * result + (ecmPid?.value ?: 0)
        result = 31 * result + (emmPid?.value ?: 0)
        result = 31 * result + (elementaryPid?.value ?: 0)
        result = 31 * result + privateData.contentHashCode()
        result = 31 * result + source.hashCode()
        return result
    }
}

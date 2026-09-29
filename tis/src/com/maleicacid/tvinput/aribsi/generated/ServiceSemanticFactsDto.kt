package com.maleicacid.tvinput.aribsi.generated


data class ServiceSemanticFactsDto(
    val originalNetworkId: Int,
    val transportStreamId: Int,
    val serviceId: Int,
    val serviceType: Int?,
    val pmtPidResolved: Boolean,
    val pmtParsed: Boolean,
    val pcrPidResolved: Boolean,
    val elementaryStreams: List<ElementaryStreamDto>,
    val requiresCas: Boolean,
    val casFactsCanonicalJson: String?,
    val caDescriptorsResolved: Boolean,
    val freeCaMode: Boolean?,
    val smd: SmdSemanticFactsDto,
    val missingComponents: List<String>,
    val semanticDiagnostics: List<String>,
    val name: String?,
    val providerName: String?,
    val pmtPid: Int?,
    val pcrPid: Int?,
    val serviceScopedCaDescriptors: List<ServiceCaDescriptorDto>
)


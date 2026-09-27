package com.maleicacid.tvinput.aribsi.generated


data class ServiceSemanticFactsDto(
    val originalNetworkId: Int,
    val transportStreamId: Int,
    val serviceId: Int,
    val serviceType: Int?,
    val pmtPidResolved: Boolean,
    val pmtParsed: Boolean,
    val pcrPidResolved: Boolean,
    val elementaryStreams: kotlin.collections.List<com.maleicacid.tvinput.aribsi.generated.ElementaryStreamDto>,
    val requiresCas: Boolean,
    val casFactsCanonicalJson: String?,
    val caDescriptorsResolved: Boolean,
    val freeCaMode: Boolean?,
    val smd: com.maleicacid.tvinput.aribsi.generated.SmdSemanticFactsDto,
    val missingComponents: kotlin.collections.List<String>,
    val semanticDiagnostics: kotlin.collections.List<String>,
    val name: String?,
    val providerName: String?,
    val pmtPid: Int?,
    val pcrPid: Int?,
    val serviceScopedCaDescriptors: kotlin.collections.List<com.maleicacid.tvinput.aribsi.generated.ServiceCaDescriptorDto>
) {
}


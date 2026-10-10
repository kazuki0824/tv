package com.maleicacid.tvinput.aribsi

import com.maleicacid.tvinput.common.ServiceId16
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.TsPid

data class NativeSiSnapshot(
    val collectionGeneration: Long,
    val ingestSequence: Long,
    val discoveryStage: Int,
    val broadcastClock: AribBroadcastClockFact?,
    val tableRequirements: List<TableRequirementStatus>,
    val catCaMetadata: List<CaMetadata>,
    val malformedCaDescriptorDiagnostics: List<MalformedCaDescriptorDiagnostic>,
    val malformedCaDescriptorCountByServiceId: Map<ServiceId16, Int>,
    val transportSemanticFacts: List<AribTransport>,
    val events: List<AribEvent>,
    val eitInstances: List<EitInstanceState>,
    val serviceSemanticFacts: List<ServiceSemanticFacts>,
    val parserDiagnostics: List<ParserDiagnostic>,
) {
    val services: List<AribService> get() =
        serviceSemanticFacts.map { facts ->
            AribService(
                serviceKey = facts.serviceKey,
                name = facts.name,
                providerName = facts.providerName,
                serviceType = facts.serviceType,
                pmtPid = facts.pmtPid,
                pcrPid = facts.pcrPid,
                freeCaMode = facts.freeCaMode,
                streams = facts.elementaryStreams,
                serviceScopedCaDescriptors = facts.serviceScopedCaDescriptors,
            )
        }

    val caMetadata: List<CaMetadata> get() =
        serviceSemanticFacts.flatMap { facts ->
            facts.serviceScopedCaDescriptors.map { descriptor ->
                CaMetadata(
                    serviceKey = facts.serviceKey,
                    caSystemId = descriptor.caSystemId,
                    ecmPid = descriptor.caPid,
                    emmPid = null,
                    elementaryPid = descriptor.esPid,
                    privateData = descriptor.privateData,
                    source =
                        if (descriptor.scope == CaDescriptorScope.ES) {
                            CaMetadataSource.ELEMENTARY_STREAM
                        } else {
                            CaMetadataSource.PROGRAM
                        },
                )
            }
        } + catCaMetadata

    val pmtPids: Map<ServiceKey, TsPid> get() =
        serviceSemanticFacts
            .mapNotNull { facts ->
                facts.pmtPid?.let { facts.serviceKey to it }
            }.toMap()
}

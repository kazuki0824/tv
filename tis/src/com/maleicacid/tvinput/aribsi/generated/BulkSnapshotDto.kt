package com.maleicacid.tvinput.aribsi.generated


data class BulkSnapshotDto(
    val collectionGeneration: Long,
    val ingestSequence: Long,
    val discoveryStage: Int,
    val broadcastClock: BroadcastClockDto?,
    val tableRequirements: List<TableRequirementDto>,
    val catCaMetadata: List<CaMetadataDto>,
    val malformedCaDescriptorDiagnostics: List<MalformedCaDescriptorDiagnosticDto>,
    val malformedCaDescriptorCounts: List<MalformedCaDescriptorCountDto>,
    val transportSemanticFacts: List<TransportSemanticFactsDto>,
    val events: List<EventDto>,
    val eitInstances: List<EitInstanceDto>,
    val serviceSemanticFacts: List<ServiceSemanticFactsDto>,
    val parserDiagnostics: List<ParserDiagnosticDto>
)

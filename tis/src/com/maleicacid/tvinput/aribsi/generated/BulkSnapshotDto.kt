package com.maleicacid.tvinput.aribsi.generated

data class BulkSnapshotDto(
    val collectionGeneration: Long,
    val ingestSequence: Long,
    val discoveryStage: Int,
    val broadcastClock: com.maleicacid.tvinput.aribsi.generated.BroadcastClockDto?,
    val tableRequirements: kotlin.collections.List<com.maleicacid.tvinput.aribsi.generated.TableRequirementDto>,
    val catCaMetadata: kotlin.collections.List<com.maleicacid.tvinput.aribsi.generated.CaMetadataDto>,
    val malformedCaDescriptorDiagnostics: kotlin.collections.List<com.maleicacid.tvinput.aribsi.generated.MalformedCaDescriptorDiagnosticDto>,
    val malformedCaDescriptorCounts: kotlin.collections.List<com.maleicacid.tvinput.aribsi.generated.MalformedCaDescriptorCountDto>,
    val transportSemanticFacts: kotlin.collections.List<com.maleicacid.tvinput.aribsi.generated.TransportSemanticFactsDto>,
    val events: kotlin.collections.List<com.maleicacid.tvinput.aribsi.generated.EventDto>,
    val eitInstances: kotlin.collections.List<com.maleicacid.tvinput.aribsi.generated.EitInstanceDto>,
    val serviceSemanticFacts: kotlin.collections.List<com.maleicacid.tvinput.aribsi.generated.ServiceSemanticFactsDto>,
    val parserDiagnostics: kotlin.collections.List<com.maleicacid.tvinput.aribsi.generated.ParserDiagnosticDto>
) {
}

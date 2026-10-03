package com.maleicacid.tvinput.aribsi.generated


data class ServiceRegistrationSnapshotDto(
    val discoveryStage: Int,
    val tableRequirements: List<TableRequirementDto>,
    val transportSemanticFacts: List<TransportSemanticFactsDto>,
    val eitInstances: List<EitInstanceDto>,
    val serviceSemanticFacts: List<ServiceSemanticFactsDto>,
    val parserDiagnostics: List<ParserDiagnosticDto>
)


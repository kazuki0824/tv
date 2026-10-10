package com.maleicacid.tvinput.aribsi.generated


data class SmdSemanticFactsDto(
    val descriptorPresent: Boolean,
    val syntaxValid: Boolean,
    val systemManagementId: Int?,
    val broadcastingFlag: Int?,
    val broadcastingIdentifier: Int?,
    val broadcastSystem: BroadcastSystemDto?,
    val additionalBroadcastingIdentification: Int?,
    val additionalIdentificationInfoHex: String,
    val semanticState: SmdSemanticStateDto,
    val diagnostic: String?
)

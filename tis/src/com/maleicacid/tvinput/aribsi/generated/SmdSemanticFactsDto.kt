package com.maleicacid.tvinput.aribsi.generated


data class SmdSemanticFactsDto(
    val descriptorPresent: Boolean,
    val syntaxValid: Boolean,
    val systemManagementId: Int?,
    val broadcastingFlag: Int?,
    val broadcastingIdentifier: Int?,
    val broadcastSystem: com.maleicacid.tvinput.aribsi.generated.BroadcastSystemDto?,
    val additionalBroadcastingIdentification: Int?,
    val additionalIdentificationInfoHex: String,
    val semanticState: com.maleicacid.tvinput.aribsi.generated.SmdSemanticStateDto,
    val diagnostic: String?
) {
}


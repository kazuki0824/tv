package com.maleicacid.tvinput.aribsi.generated


sealed class SmdSemanticStateDto {

    object SupportedBroadcast : SmdSemanticStateDto()

    object NonBroadcast : SmdSemanticStateDto()

    object UndefinedBroadcastClass : SmdSemanticStateDto()

    object UnsupportedBroadcastSystem : SmdSemanticStateDto()

    object UndeterminedSmd : SmdSemanticStateDto()
}


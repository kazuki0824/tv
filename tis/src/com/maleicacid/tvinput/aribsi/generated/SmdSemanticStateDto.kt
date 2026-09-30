// serde の wire enum 名を維持する生成ファイル。
@file:Suppress("ClassName")

package com.maleicacid.tvinput.aribsi.generated


sealed class SmdSemanticStateDto {

    object SUPPORTED_BROADCAST : SmdSemanticStateDto()

    object NON_BROADCAST : SmdSemanticStateDto()

    object UNDEFINED_BROADCAST_CLASS : SmdSemanticStateDto()

    object UNSUPPORTED_BROADCAST_SYSTEM : SmdSemanticStateDto()

    object UNDETERMINED_SMD : SmdSemanticStateDto()
}


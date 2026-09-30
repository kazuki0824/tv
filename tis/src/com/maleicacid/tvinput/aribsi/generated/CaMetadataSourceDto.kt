// serde の wire enum 名を維持する生成ファイル。
@file:Suppress("ClassName")

package com.maleicacid.tvinput.aribsi.generated


sealed class CaMetadataSourceDto {

    object PROGRAM : CaMetadataSourceDto()

    object ELEMENTARY_STREAM : CaMetadataSourceDto()

    object CAT : CaMetadataSourceDto()
}


package com.maleicacid.tvinput.aribsi.generated


data class CaMetadataDto(
    val serviceKey: ServiceKeyDto?,
    val caSystemId: Int,
    val ecmPid: Int?,
    val emmPid: Int?,
    val elementaryPid: Int?,
    val privateDataHex: String,
    val source: CaMetadataSourceDto
)


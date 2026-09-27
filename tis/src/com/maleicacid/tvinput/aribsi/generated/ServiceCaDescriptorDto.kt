package com.maleicacid.tvinput.aribsi.generated


data class ServiceCaDescriptorDto(
    val caSystemId: Int,
    val caPid: Int,
    val scope: com.maleicacid.tvinput.aribsi.generated.CaDescriptorScopeDto,
    val esPid: Int?,
    val rawDescriptorHex: String,
    val privateDataHex: String
) {
}


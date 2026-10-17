package com.maleicacid.tvinput.aribsi.generated


data class ServiceCaDescriptorDto(
    val caSystemId: Int,
    val caPid: Int,
    val scope: CaDescriptorScopeDto,
    val esPid: Int?,
    val rawDescriptorHex: String,
    val privateDataHex: String
)

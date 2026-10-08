package com.maleicacid.tvinput.aribsi.generated


data class DescriptorDiagnosticDto(
    val schema: String,
    val schemaVersion: Int,
    val severity: String,
    val code: String,
    val scope: DescriptorDiagnosticScopeDto,
    val descriptor: DescriptorDiagnosticDescriptorDto,
    val message: String
)

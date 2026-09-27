package com.maleicacid.tvinput.aribsi.generated


data class DescriptorDiagnosticDto(
    val schema: String,
    val schemaVersion: Int,
    val severity: String,
    val code: String,
    val scope: com.maleicacid.tvinput.aribsi.generated.DescriptorDiagnosticScopeDto,
    val descriptor: com.maleicacid.tvinput.aribsi.generated.DescriptorDiagnosticDescriptorDto,
    val message: String
) {
}


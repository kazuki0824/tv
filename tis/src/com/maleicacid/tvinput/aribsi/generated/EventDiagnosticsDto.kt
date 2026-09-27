package com.maleicacid.tvinput.aribsi.generated

data class EventDiagnosticsDto(
    val summary: String,
    val descriptorDiagnostics: kotlin.collections.List<com.maleicacid.tvinput.aribsi.generated.DescriptorDiagnosticDto>,
    val descriptorDiagnosticsCanonicalJson: String,
    val descriptorFactsCanonicalJson: String?,
    val textDiagnostics: kotlin.collections.List<String>,
    val truncatedDescriptorLoop: com.maleicacid.tvinput.aribsi.generated.TruncatedDescriptorLoopDto?
) {
}

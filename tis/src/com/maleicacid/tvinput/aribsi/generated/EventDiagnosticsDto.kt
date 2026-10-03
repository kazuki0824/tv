package com.maleicacid.tvinput.aribsi.generated


data class EventDiagnosticsDto(
    val summary: String,
    val descriptorDiagnostics: List<DescriptorDiagnosticDto>,
    val descriptorDiagnosticsCanonicalJson: String,
    val descriptorFactsCanonicalJson: String?,
    val textDiagnostics: List<String>,
    val truncatedDescriptorLoop: TruncatedDescriptorLoopDto?
)


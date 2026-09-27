package com.maleicacid.tvinput.aribsi.generated

data class MalformedCaDescriptorDiagnosticDto(
    val pid: Int,
    val tableId: Int,
    val tableIdExtension: Int?,
    val serviceId: Int?,
    val elementaryPid: Int?,
    val scope: String,
    val offset: Int,
    val declaredLength: Int,
    val actualRemainingLength: Int,
    val reason: String,
    val rawPrefixHex: String
) {
}

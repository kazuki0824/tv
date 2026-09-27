package com.maleicacid.tvinput.aribsi.generated


data class EitInstanceDto(
    val originalNetworkId: Int,
    val transportStreamId: Int,
    val serviceId: Int,
    val tableId: Int,
    val version: Int,
    val currentNextIndicator: Boolean,
    val lastSectionNumber: Int,
    val receivedSections: kotlin.collections.List<Int>,
    val missingSections: kotlin.collections.List<Int>,
    val safeSections: kotlin.collections.List<Int>,
    val complete: Boolean,
    val inconsistent: Boolean
) {
}


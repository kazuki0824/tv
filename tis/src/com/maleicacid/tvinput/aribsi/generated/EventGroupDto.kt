package com.maleicacid.tvinput.aribsi.generated


data class EventGroupDto(
    val groupType: Int,
    val events: kotlin.collections.List<com.maleicacid.tvinput.aribsi.generated.EventGroupReferenceDto>,
    val otherNetworkEvents: kotlin.collections.List<com.maleicacid.tvinput.aribsi.generated.OtherNetworkEventGroupReferenceDto>,
    val privateDataHex: String,
    val parseStatus: com.maleicacid.tvinput.aribsi.generated.SiParseStatusDto
) {
}


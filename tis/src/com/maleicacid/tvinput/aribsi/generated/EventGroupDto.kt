package com.maleicacid.tvinput.aribsi.generated


data class EventGroupDto(
    val groupType: Int,
    val events: List<EventGroupReferenceDto>,
    val otherNetworkEvents: List<OtherNetworkEventGroupReferenceDto>,
    val privateDataHex: String,
    val parseStatus: SiParseStatusDto
)


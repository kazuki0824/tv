package com.maleicacid.tvinput.aribsi.generated


data class EventDto(
    val serviceKey: com.maleicacid.tvinput.aribsi.generated.ServiceKeyDto,
    val stableIdentity: String?,
    val eventId: Int,
    val timingState: com.maleicacid.tvinput.aribsi.generated.EitTimingStateDto,
    val rawStartTimeHex: String,
    val rawDurationHex: String,
    val startTimeMillis: Long,
    val durationMillis: Long,
    val title: String,
    val description: String,
    val extendedDescription: String,
    val eventScope: String,
    val source: com.maleicacid.tvinput.aribsi.generated.ProgramSourceDto,
    val descriptors: com.maleicacid.tvinput.aribsi.generated.EventDescriptorsDto
) {
}


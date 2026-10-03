package com.maleicacid.tvinput.aribsi.generated


data class EventDto(
    val serviceKey: ServiceKeyDto,
    val stableIdentity: String?,
    val eventId: Int,
    val timingState: EitTimingStateDto,
    val rawStartTimeHex: String,
    val rawDurationHex: String,
    val startTimeMillis: Long,
    val durationMillis: Long,
    val title: String,
    val description: String,
    val extendedDescription: String,
    val eventScope: String,
    val source: ProgramSourceDto,
    val descriptors: EventDescriptorsDto
)


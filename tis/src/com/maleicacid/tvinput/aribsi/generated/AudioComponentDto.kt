package com.maleicacid.tvinput.aribsi.generated


data class AudioComponentDto(
    val streamType: Int?,
    val streamContent: Int?,
    val componentTag: Int?,
    val componentType: Int?,
    val language: String?,
    val secondLanguage: String?,
    val channelConfiguration: String?,
    val simulcastGroupTag: Int?,
    val samplingRate: Int?,
    val samplingInfo: String?,
    val text: String?,
    val sourceDescriptor: String?,
    val main: Boolean?,
    val multiLingual: Boolean?,
    val qualityIndicator: Int?,
    val parseStatus: SiParseStatusDto,
    val channelCount: Int?,
    val sampleRateHz: Int?,
    val audioDescription: Boolean?,
    val hardOfHearing: Boolean?,
    val dualMono: Boolean?
)


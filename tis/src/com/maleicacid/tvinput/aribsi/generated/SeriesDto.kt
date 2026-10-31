package com.maleicacid.tvinput.aribsi.generated


data class SeriesDto(
    val seriesId: Int?,
    val repeatLabel: Int,
    val programPattern: Int,
    val expireDateValid: Boolean,
    val expireDate: Int?,
    val episodeNumber: Int?,
    val lastEpisodeNumber: Int?,
    val name: String?,
    val parseStatus: SiParseStatusDto
)

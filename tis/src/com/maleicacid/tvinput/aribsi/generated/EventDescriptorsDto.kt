package com.maleicacid.tvinput.aribsi.generated


data class EventDescriptorsDto(
    val shortEvents: kotlin.collections.List<com.maleicacid.tvinput.aribsi.generated.ShortEventDto>,
    val extendedTexts: kotlin.collections.List<com.maleicacid.tvinput.aribsi.generated.ExtendedTextDto>,
    val extendedItems: kotlin.collections.List<com.maleicacid.tvinput.aribsi.generated.ExtendedItemDto>,
    val componentText: String?,
    val audioComponentText: String?,
    val contentGenres: kotlin.collections.List<com.maleicacid.tvinput.aribsi.generated.ContentGenreDto>,
    val genreSupplementText: String?,
    val eventGroups: kotlin.collections.List<com.maleicacid.tvinput.aribsi.generated.EventGroupDto>,
    val componentGroups: kotlin.collections.List<com.maleicacid.tvinput.aribsi.generated.ComponentGroupDescriptorDto>,
    val linkage: kotlin.collections.List<com.maleicacid.tvinput.aribsi.generated.LinkageDto>,
    val freeCaMode: com.maleicacid.tvinput.aribsi.generated.FreeCaModeDto?,
    val series: com.maleicacid.tvinput.aribsi.generated.SeriesDto?,
    val seriesCandidates: kotlin.collections.List<com.maleicacid.tvinput.aribsi.generated.SeriesDto>,
    val seriesCandidatesCanonicalJson: String?,
    val parentalRatings: kotlin.collections.List<com.maleicacid.tvinput.aribsi.generated.ParentalRatingDto>,
    val components: com.maleicacid.tvinput.aribsi.generated.ComponentsDto,
    val diagnostics: com.maleicacid.tvinput.aribsi.generated.EventDiagnosticsDto
) {
}


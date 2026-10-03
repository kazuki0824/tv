package com.maleicacid.tvinput.aribsi.generated


data class EventDescriptorsDto(
    val shortEvents: List<ShortEventDto>,
    val extendedTexts: List<ExtendedTextDto>,
    val extendedItems: List<ExtendedItemDto>,
    val componentText: String?,
    val audioComponentText: String?,
    val contentGenres: List<ContentGenreDto>,
    val genreSupplementText: String?,
    val eventGroups: List<EventGroupDto>,
    val componentGroups: List<ComponentGroupDescriptorDto>,
    val linkage: List<LinkageDto>,
    val freeCaMode: FreeCaModeDto?,
    val series: SeriesDto?,
    val seriesCandidates: List<SeriesDto>,
    val seriesCandidatesCanonicalJson: String?,
    val parentalRatings: List<ParentalRatingDto>,
    val components: ComponentsDto,
    val diagnostics: EventDiagnosticsDto
)


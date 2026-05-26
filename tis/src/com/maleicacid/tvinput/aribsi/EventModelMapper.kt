package com.maleicacid.tvinput.aribsi

import com.maleicacid.tvinput.common.ServiceId16
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.db.ChannelRecord
import com.maleicacid.tvinput.db.ProgramDescriptors
import com.maleicacid.tvinput.db.ProgramRecord

enum class ProgramPublishStateSource { CURRENT_DIAGNOSTIC, CHANNEL_FALLBACK, MERGED_CHANNEL_CAS_STATE, NONE }

data class ProgramPublishState(
    val requiresCas: Boolean,
    val unsupportedCas: Boolean,
    val clearLivePlaybackSupported: Boolean,
    val channelRegistrationReady: Boolean,
    val epgPublishable: Boolean,
    val source: ProgramPublishStateSource,
) {
    companion object {
        fun from(diagnostic: ServicePublishabilityDiagnostic?, fallback: ChannelRecord?): ProgramPublishState {
            val diagnosticComplete = diagnostic?.isCurrentDiagnosticComplete() == true
            val diagnosticCasResolved = diagnostic?.caStateResolved == true
            return when {
                diagnosticCasResolved -> {
                    val requiresCas = diagnostic!!.requiresCas
                    val unsupportedCas = diagnostic.unsupportedCas
                    ProgramPublishState(
                        requiresCas = requiresCas,
                        unsupportedCas = unsupportedCas,
                        clearLivePlaybackSupported = if (requiresCas || unsupportedCas) false else diagnostic.clearLivePlaybackSupported,
                        channelRegistrationReady = if (diagnosticComplete) diagnostic.channelRegistrationReady else fallback?.channelRegistrationReady ?: diagnostic.channelRegistrationReady,
                        epgPublishable = if (diagnosticComplete) diagnostic.epgPublishable else fallback?.epgPublishable ?: diagnostic.epgPublishable,
                        source = ProgramPublishStateSource.CURRENT_DIAGNOSTIC,
                    )
                }
                diagnostic != null && fallback != null -> {
                    val requiresCas = fallback.requiresCas
                    val unsupportedCas = fallback.unsupportedCas
                    ProgramPublishState(
                        requiresCas = requiresCas,
                        unsupportedCas = unsupportedCas,
                        clearLivePlaybackSupported = if (requiresCas || unsupportedCas) false else fallback.clearLivePlaybackSupported,
                        channelRegistrationReady = fallback.channelRegistrationReady || diagnostic.channelRegistrationReady,
                        epgPublishable = fallback.epgPublishable || diagnostic.epgPublishable,
                        source = ProgramPublishStateSource.CHANNEL_FALLBACK,
                    )
                }
                fallback != null -> ProgramPublishState(
                    requiresCas = fallback.requiresCas,
                    unsupportedCas = fallback.unsupportedCas,
                    clearLivePlaybackSupported = fallback.clearLivePlaybackSupported,
                    channelRegistrationReady = fallback.channelRegistrationReady,
                    epgPublishable = fallback.epgPublishable,
                    source = ProgramPublishStateSource.CHANNEL_FALLBACK,
                )
                else -> ProgramPublishState(false, false, false, false, false, ProgramPublishStateSource.NONE)
            }
        }

        fun resolveByServiceKey(
            diagnostics: Map<ServiceKey, ServicePublishabilityDiagnostic>,
            channelFallbacks: Map<ServiceKey, ChannelRecord>,
            serviceKeys: Set<ServiceKey>,
        ): Map<ServiceKey, ProgramPublishState> = serviceKeys.associateWith { key -> from(diagnostics[key], channelFallbacks[key]) }
    }
}

fun ServicePublishabilityDiagnostic.isCurrentDiagnosticComplete(): Boolean {
    if (!publishable) return false
    if (!channelRegistrationReady && !epgPublishable) return false
    if (missingComponents.isNotEmpty()) return false
    if (!pmtPidResolved || !pmtParsed) return false
    if (!caStateResolved) return false
    if (unsupportedCas && !requiresCas) return false
    val unresolvedMarkers = listOf("UNRESOLVED", "NO_RUST_PUBLISHABILITY_DIAGNOSTIC", "NO_PMT_PID", "NO_PMT", "NO_PCR_PID", "NO_SUPPORTED_VIDEO_ES", "MISSING")
    val allReasons = missingComponents + reasons + registrationReasons + epgReasons
    return allReasons.none { reason -> unresolvedMarkers.any { marker -> reason.contains(marker, ignoreCase = true) } }
}

class EventModelMapper {
    fun toProgramRecords(
        events: List<AribEvent>,
        publishabilityByServiceKey: Map<ServiceKey, ServicePublishabilityDiagnostic> = emptyMap(),
        channelFallbackByServiceKey: Map<ServiceKey, ChannelRecord> = emptyMap(),
        publishStateByServiceKey: Map<ServiceKey, ProgramPublishState> = emptyMap(),
        malformedCaDescriptorCountByServiceId: Map<ServiceId16, Int> = emptyMap(),
    ): List<ProgramRecord> {
        val serviceKeys = events.map { it.serviceKey }.toSet()
        val effectiveStates = publishStateByServiceKey.ifEmpty {
            ProgramPublishState.resolveByServiceKey(publishabilityByServiceKey, channelFallbackByServiceKey, serviceKeys)
        }
        return events.mapNotNull { event ->
            val state = effectiveStates[event.serviceKey]
            val end = event.startTimeMillis + event.durationMillis
            if (event.startTimeMillis <= 0L || end <= event.startTimeMillis) null else ProgramRecord(
                serviceKey = event.serviceKey,
                eventId = event.eventId,
                stableIdentity = event.stableIdentity,
                startTimeMillis = event.startTimeMillis,
                durationMillis = event.durationMillis,
                title = event.title.ifBlank { "event-${event.eventId}" },
                description = providerDescription(event),
                shortDescription = event.description.take(256),
                canonicalGenres = canonicalGenresFromContentGenres(event.descriptors.contentGenres),
                descriptors = ProgramDescriptors(
                    extendedItems = event.descriptors.extendedItems,
                    componentText = event.descriptors.componentText,
                    audioComponentText = event.descriptors.audioComponentText,
                    audioLanguage = event.descriptors.components.audio.firstOrNull { !it.language.isNullOrBlank() }?.language,
                    contentGenres = event.descriptors.contentGenres,
                    broadcastGenre = broadcastGenreText(event.descriptors.contentGenres),
                    genreSupplementText = genreSupplementText(event.descriptors.contentGenres, event.descriptors.genreSupplementText),
                    relatedItems = event.descriptors.relatedItems,
                    linkage = event.descriptors.linkage,
                    scrambled = event.descriptors.scrambled,
                    freeCaMode = event.descriptors.freeCaMode,
                    seriesId = event.descriptors.series?.seriesId,
                    episodeNumber = event.descriptors.series?.episodeNumber,
                    lastEpisodeNumber = event.descriptors.series?.lastEpisodeNumber,
                    series = event.descriptors.series,
                    descriptorDiagnosticsCanonicalJson = event.descriptors.diagnostics.descriptorDiagnosticsCanonicalJson,
                    parentalRatings = event.descriptors.parentalRatings,
                    components = event.descriptors.components,
                ),
                source = event.source,
                requiresCas = state?.requiresCas ?: false,
                unsupportedCas = state?.unsupportedCas ?: false,
                clearLivePlaybackSupported = state?.clearLivePlaybackSupported ?: false,
                channelRegistrationReady = state?.channelRegistrationReady ?: false,
                epgPublishable = state?.epgPublishable ?: false,
                publishStateSource = publishStateSourceName(state?.source),
                diagnosticText = event.descriptors.diagnostics.summary,
                contentRatings = event.descriptors.parentalRatings.mapNotNull { AribRatingMapper.toTvContentRatingString(it) },
                malformedCaDescriptorCount = malformedCaDescriptorCountByServiceId[event.serviceKey.service] ?: 0,
            )
        }
    }

    private fun providerDescription(event: AribEvent): String {
        val d = event.descriptors
        val extended = d.extendedItems.joinToString("\n") { item ->
            if (item.itemDescription.isBlank()) item.itemText else "【${item.itemDescription}】${item.itemText}"
        }
        val uiSupplements = listOfNotNull(
            d.componentText?.takeIf { it.isNotBlank() }?.let { "映像: $it" },
            d.audioComponentText?.takeIf { it.isNotBlank() }?.let { "音声: $it" },
            d.genreSupplementText?.takeIf { it.isNotBlank() }?.let { "ジャンル: $it" },
            (d.freeCaMode?.text ?: when (d.scrambled) { true -> "有料放送"; false -> "無料放送"; null -> null })?.takeIf { it.isNotBlank() }?.let { "放送種別: $it" },
        )
        return listOf(event.description, event.extendedDescription, extended)
            .plus(uiSupplements)
            .filter { it.isNotBlank() }
            .joinToString("\n")
    }

    private fun canonicalGenresFromContentGenres(genres: List<AribContentGenre>): List<String> {
        val out = linkedSetOf<String>()
        genres.forEach { genre ->
            when (genre.level1) {
                0x0 -> out += "NEWS"
                0x1 -> out += "SPORTS"
                0x3 -> out += "DRAMA"
                0x4 -> out += "MUSIC"
                0x5 -> {
                    out += "ENTERTAINMENT"
                    when (genre.level2) {
                        0x3 -> out += "COMEDY"
                        0x4 -> out += "MUSIC"
                        0x5 -> out += "TRAVEL"
                        0x6 -> out += "LIFE_STYLE"
                    }
                }
                0x6 -> out += "MOVIES"
                0x7 -> out += "ENTERTAINMENT"
                0x8 -> when (genre.level2) {
                    0x2 -> out += "ANIMAL_WILDLIFE"
                    0x3 -> out += "TECH_SCIENCE"
                    0x4, 0x5 -> out += "ARTS"
                    0x6 -> out += "SPORTS"
                }
                0x9 -> {
                    out += "ARTS"
                    when (genre.level2) {
                        0x1 -> out += "MUSIC"
                        0x3 -> out += "COMEDY"
                    }
                }
                0xA -> when (genre.level2) {
                    0x1 -> out += "LIFE_STYLE"
                    0x6 -> out += "GAMING"
                    0x7 -> out += "EDUCATION"
                    0x8 -> { out += "EDUCATION"; out += "FAMILY_KIDS" }
                    0x9, 0xA, 0xB, 0xC -> out += "EDUCATION"
                }
            }
        }
        return out.toList()
    }

    private fun broadcastGenreText(genres: List<AribContentGenre>): String? = genres.takeIf { it.isNotEmpty() }?.joinToString("、") { genre ->
        val name = genre.aribName.takeIf { it.isNotBlank() } ?: ""
        "ARIB(0x${genre.level1.toString(16)}/0x${genre.level2.toString(16)}):$name"
    }

    private fun genreSupplementText(genres: List<AribContentGenre>, fallback: String?): String? =
        fallback?.takeIf { it.isNotBlank() } ?: genres.takeIf { it.isNotEmpty() }?.joinToString("、") { it.aribName.takeIf { name -> name.isNotBlank() } ?: "ARIB(0x${it.level1.toString(16)}/0x${it.level2.toString(16)})" }



    private fun publishStateSourceName(source: ProgramPublishStateSource?): String = (source ?: ProgramPublishStateSource.NONE).name
}

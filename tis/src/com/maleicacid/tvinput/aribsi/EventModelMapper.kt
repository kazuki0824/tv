package com.maleicacid.tvinput.aribsi

import com.maleicacid.tvinput.common.ServiceId16
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.db.ProgramDescriptors
import com.maleicacid.tvinput.db.ProgramRecord

class EventModelMapper {
    fun toProgramRecords(
        events: List<AribEvent>,
        semanticFactsByServiceKey: Map<ServiceKey, ServiceSemanticFacts> = emptyMap(),
        malformedCaDescriptorCountByServiceId: Map<ServiceId16, Int> = emptyMap(),
        ratingProfileByServiceKey: Map<ServiceKey, AribRatingMapper.BroadcastProfile> = emptyMap(),
    ): List<ProgramRecord> {
        return events.mapNotNull { event ->
            if (
                event.source.tableId != 0x4e ||
                event.timingState != "DEFINED" ||
                event.providerDataCanonicalJson.isBlank()
            ) return@mapNotNull null
            val semanticFacts = semanticFactsByServiceKey[event.serviceKey]
            val end = runCatching { Math.addExact(event.startTimeMillis, event.durationMillis) }
                .getOrElse { return@mapNotNull null }
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
                    contentGenres = event.descriptors.contentGenres,
                    broadcastGenre = broadcastGenreText(event.descriptors.contentGenres),
                    genreSupplementText = genreSupplementText(event.descriptors.contentGenres, event.descriptors.genreSupplementText),
                    eventGroups = event.descriptors.eventGroups,
                    linkage = event.descriptors.linkage,
                    scrambled = event.descriptors.scrambled,
                    freeCaMode = event.descriptors.freeCaMode,
                    series = event.descriptors.series,
                    descriptorDiagnosticsCanonicalJson = event.descriptors.diagnostics.descriptorDiagnosticsCanonicalJson,
                    parentalRatings = event.descriptors.parentalRatings,
                    components = event.descriptors.components,
                ),
                source = event.source,
                requiresCas = semanticFacts?.requiresCas ?: event.descriptors.scrambled == true,
                diagnosticText = event.descriptors.diagnostics.summary,
                contentRatings = event.descriptors.parentalRatings.mapNotNull {
                    AribRatingMapper.toTvContentRatingString(
                        it,
                        ratingProfileByServiceKey[event.serviceKey] ?: AribRatingMapper.BroadcastProfile.UNRESOLVED,
                    )
                },
                malformedCaDescriptorCount = malformedCaDescriptorCountByServiceId[event.serviceKey.service] ?: 0,
                providerDataCanonicalJson = event.providerDataCanonicalJson,
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
        return listOf(event.extendedDescription, extended)
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
}

package com.maleicacid.tvinput.aribsi

import com.maleicacid.tvinput.common.ServiceId16
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.db.ProgramDescriptors
import com.maleicacid.tvinput.db.ProgramRecord

class EventModelMapper {
    // 同じ入力と資源寿命を扱う手順を一続きに確認できる形に保つ。
    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("LongMethod", "MaxLineLength")
    fun toProgramRecords(
        events: List<AribEvent>,
        profile: Int,
        semanticFactsByServiceKey: Map<ServiceKey, ServiceSemanticFacts> = emptyMap(),
        malformedCaDescriptorCountByServiceId: Map<ServiceId16, Int> = emptyMap(),
        ratingProfileByServiceKey: Map<ServiceKey, AribRatingMapper.BroadcastProfile> = emptyMap(),
    ): List<ProgramRecord> {
        return events.mapNotNull { event ->
            if (!EpgPublicationPolicy.isProgramRow(profile, event)) return@mapNotNull null
            val stableIdentity = event.stableIdentity ?: return@mapNotNull null
            val semanticFacts = semanticFactsByServiceKey[event.serviceKey]
            if (semanticFactsByServiceKey.isNotEmpty() && semanticFacts == null) return@mapNotNull null
            val end =
                runCatching { Math.addExact(event.startTimeMillis, event.durationMillis) }
                    .getOrElse { return@mapNotNull null }
            if (event.startTimeMillis <= 0L || end <= event.startTimeMillis) {
                null
            } else {
                ProgramRecord(
                    serviceKey = event.serviceKey,
                    eventId = event.eventId,
                    stableIdentity = stableIdentity,
                    startTimeMillis = event.startTimeMillis,
                    durationMillis = event.durationMillis,
                    title = event.title,
                    description = providerDescription(event),
                    shortDescription = event.description,
                    canonicalGenres = canonicalGenresFromContentGenres(event.descriptors.contentGenres),
                    descriptors =
                        ProgramDescriptors(
                            shortEvents = event.descriptors.shortEvents,
                            extendedTexts = event.descriptors.extendedTexts,
                            extendedItems = event.descriptors.extendedItems,
                            componentText = event.descriptors.componentText,
                            audioComponentText = event.descriptors.audioComponentText,
                            contentGenres = event.descriptors.contentGenres,
                            broadcastGenre = broadcastGenreText(event.descriptors.contentGenres),
                            genreSupplementText =
                                genreSupplementText(
                                    event.descriptors.contentGenres,
                                    event.descriptors.genreSupplementText,
                                ),
                            eventGroups = event.descriptors.eventGroups,
                            linkage = event.descriptors.linkage,
                            scrambled = event.descriptors.scrambled,
                            freeCaMode = event.descriptors.freeCaMode,
                            series = event.descriptors.series,
                            seriesCandidatesCanonicalJson = event.descriptors.seriesCandidatesCanonicalJson,
                            descriptorDiagnosticsCanonicalJson = event.descriptors.diagnostics.descriptorDiagnosticsCanonicalJson,
                            descriptorFactsCanonicalJson = event.descriptors.diagnostics.descriptorFactsCanonicalJson,
                            parentalRatings = event.descriptors.parentalRatings,
                            components = event.descriptors.components,
                        ),
                    source = event.source,
                    requiresCas = semanticFacts?.requiresCas ?: false,
                    casFactsCanonicalJson = semanticFacts?.casFactsCanonicalJson,
                    diagnosticText = event.descriptors.diagnostics.summary,
                    contentRatings =
                        event.descriptors.parentalRatings.mapNotNull {
                            AribRatingMapper.toTvContentRatingString(
                                it,
                                ratingProfileByServiceKey[event.serviceKey] ?: AribRatingMapper.BroadcastProfile.UNRESOLVED,
                            )
                        },
                    malformedCaDescriptorCount = malformedCaDescriptorCountByServiceId[event.serviceKey.service] ?: 0,
                )
            }
        }
    }

    private fun providerDescription(event: AribEvent): String {
        val d = event.descriptors
        val selectedLanguage =
            d.shortEvents.firstOrNull()?.languageCode
                ?: d.extendedTexts.firstOrNull()?.languageCode
                ?: d.extendedItems.firstOrNull()?.languageCode
        val extended =
            d.extendedItems
                .asSequence()
                .filter { item -> selectedLanguage == null || item.languageCode == selectedLanguage }
                .joinToString("\n") { item ->
                    if (item.itemDescription.isBlank()) item.itemText else "【${item.itemDescription}】${item.itemText}"
                }
        val uiSupplements =
            listOfNotNull(
                d.componentText?.takeIf { it.isNotBlank() }?.let { "映像: $it" },
                d.audioComponentText?.takeIf { it.isNotBlank() }?.let { "音声: $it" },
                d.genreSupplementText?.takeIf { it.isNotBlank() }?.let { "ジャンル: $it" },
                when (d.scrambled) {
                    true -> "放送種別: 有料放送"
                    false -> "放送種別: 無料放送"
                    null -> null
                },
            )
        return listOf(event.extendedDescription, extended)
            .plus(uiSupplements)
            .filter { it.isNotBlank() }
            .joinToString("\n")
    }

    // 同じ入力に対する分岐・項目写像を保持し、処理分割による状態の受け渡しを増やさない。
    // 同じ入力と資源寿命を扱う手順を一続きに確認できる形に保つ。
    // この処理の規格値・ビット幅・単位換算・固定上限をリテラルのまま照合できる形に保つ。
    @Suppress("CyclomaticComplexMethod", "LongMethod", "MagicNumber")
    private fun canonicalGenresFromContentGenres(genres: List<AribContentGenre>): List<String> {
        val out = linkedSetOf<String>()
        genres.forEach { genre ->
            when (genre.level1) {
                0x0 -> {
                    out += "NEWS"
                }

                0x1 -> {
                    out += "SPORTS"
                }

                0x3 -> {
                    out += "DRAMA"
                }

                0x4 -> {
                    out += "MUSIC"
                }

                0x5 -> {
                    out += "ENTERTAINMENT"
                    when (genre.level2) {
                        0x3 -> out += "COMEDY"
                        0x4 -> out += "MUSIC"
                        0x5 -> out += "TRAVEL"
                        0x6 -> out += "LIFE_STYLE"
                    }
                }

                0x6 -> {
                    out += "MOVIES"
                }

                0x7 -> {
                    out += "ENTERTAINMENT"
                }

                0x8 -> {
                    when (genre.level2) {
                        0x2 -> out += "ANIMAL_WILDLIFE"
                        0x3 -> out += "TECH_SCIENCE"
                        0x4, 0x5 -> out += "ARTS"
                        0x6 -> out += "SPORTS"
                    }
                }

                0x9 -> {
                    out += "ARTS"
                    when (genre.level2) {
                        0x1 -> out += "MUSIC"
                        0x3 -> out += "COMEDY"
                    }
                }

                0xA -> {
                    when (genre.level2) {
                        0x1 -> {
                            out += "LIFE_STYLE"
                        }

                        0x6 -> {
                            out += "GAMING"
                        }

                        0x7 -> {
                            out += "EDUCATION"
                        }

                        0x8 -> {
                            out += "EDUCATION"
                            out += "FAMILY_KIDS"
                        }

                        0x9, 0xA, 0xB, 0xC -> {
                            out += "EDUCATION"
                        }
                    }
                }
            }
        }
        return out.toList()
    }

    // この処理の規格値・ビット幅・単位換算・固定上限をリテラルのまま照合できる形に保つ。
    @Suppress("MagicNumber")
    private fun broadcastGenreText(genres: List<AribContentGenre>): String? =
        genres.takeIf { it.isNotEmpty() }?.joinToString("、") { genre ->
            val name = genre.aribName.takeIf { it.isNotBlank() } ?: ""
            "ARIB(0x${genre.level1.toString(16)}/0x${genre.level2.toString(16)}):$name"
        }

    // この処理の規格値・ビット幅・単位換算・固定上限をリテラルのまま照合できる形に保つ。
    @Suppress("MagicNumber")
    private fun genreSupplementText(
        genres: List<AribContentGenre>,
        fallback: String?,
    ): String? =
        fallback?.takeIf { it.isNotBlank() }
            ?: genres.takeIf { it.isNotEmpty() }?.joinToString("、") {
                it.aribName.takeIf { name -> name.isNotBlank() }
                    ?: "ARIB(0x${it.level1.toString(16)}/0x${it.level2.toString(16)})"
            }
}

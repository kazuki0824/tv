package com.maleicacid.tvinput.aribsi

import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.db.ChannelRecord
import com.maleicacid.tvinput.db.ProgramDescriptors
import com.maleicacid.tvinput.db.ProgramRecord
import org.json.JSONArray
import org.json.JSONObject

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
                canonicalGenres = canonicalGenresFromBroadcastGenre(event.descriptors.broadcastGenre),
                descriptors = ProgramDescriptors(
                    extendedItemsJson = extendedItemsJson(event.descriptors.extendedItems),
                    componentText = event.descriptors.componentText,
                    audioComponentText = event.descriptors.audioComponentText,
                    audioLanguage = event.descriptors.audioLanguage,
                    broadcastGenre = event.descriptors.broadcastGenre,
                    genreSupplementText = event.descriptors.genreSupplementText,
                    relatedItemsJson = event.descriptors.relatedItemsJson,
                    linkageJson = event.descriptors.linkageJson,
                    scrambled = event.descriptors.scrambled,
                    freeCaModeJson = event.descriptors.freeCaModeJson,
                    seriesId = event.descriptors.seriesId,
                    episodeNumber = event.descriptors.episodeNumber,
                    lastEpisodeNumber = event.descriptors.lastEpisodeNumber,
                    seriesJson = event.descriptors.seriesJson,
                    descriptorDiagnosticsJson = event.descriptors.diagnostics.descriptorDiagnosticsJson,
                    parentalRatings = event.descriptors.parentalRatings,
                    componentsJson = event.descriptors.componentsJson,
                ),
                requiresCas = state?.requiresCas ?: false,
                unsupportedCas = state?.unsupportedCas ?: false,
                clearLivePlaybackSupported = state?.clearLivePlaybackSupported ?: false,
                channelRegistrationReady = state?.channelRegistrationReady ?: false,
                epgPublishable = state?.epgPublishable ?: false,
                publishStateSource = publishStateSourceName(state?.source),
                diagnosticText = event.descriptors.diagnostics.summary,
                contentRatings = event.descriptors.parentalRatings.mapNotNull { AribRatingMapper.toTvContentRatingString(it) },
                malformedCaDescriptorCount = descriptorDiagnosticCount(event.descriptors.diagnostics.descriptorDiagnosticsJson),
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
            freeCaLabelFromJson(d.freeCaModeJson, d.scrambled)?.takeIf { it.isNotBlank() }?.let { "放送種別: $it" },
        )
        return listOf(event.description, event.extendedDescription, extended)
            .plus(uiSupplements)
            .filter { it.isNotBlank() }
            .joinToString("\n")
    }


    private fun freeCaLabelFromJson(raw: String, scrambled: Boolean?): String? {
        val fromJson = runCatching { org.json.JSONObject(raw).optString("text") }.getOrNull()?.takeIf { it.isNotBlank() }
        return fromJson ?: when (scrambled) {
            true -> "有料放送"
            false -> "無料放送"
            null -> null
        }
    }

    private fun canonicalGenresFromBroadcastGenre(broadcastGenre: String?): List<String> {
        if (broadcastGenre.isNullOrBlank()) return emptyList()
        val out = linkedSetOf<String>()
        Regex("ARIB\(0x([0-9a-fA-F]+)/0x([0-9a-fA-F]+)\)").findAll(broadcastGenre).forEach { match ->
            val level1 = match.groupValues[1].toIntOrNull(16) ?: return@forEach
            val level2 = match.groupValues[2].toIntOrNull(16) ?: return@forEach
            when (level1) {
                0x0 -> out += "NEWS"
                0x1 -> out += "SPORTS"
                0x3 -> out += "DRAMA"
                0x4 -> out += "MUSIC"
                0x5 -> {
                    out += "ENTERTAINMENT"
                    when (level2) {
                        0x3 -> out += "COMEDY"
                        0x4 -> out += "MUSIC"
                        0x5 -> out += "TRAVEL"
                        0x6 -> out += "LIFE_STYLE"
                    }
                }
                0x6 -> out += "MOVIES"
                0x7 -> out += "ENTERTAINMENT"
                0x8 -> when (level2) {
                    0x2 -> out += "ANIMAL_WILDLIFE"
                    0x3 -> out += "TECH_SCIENCE"
                    0x4, 0x5 -> out += "ARTS"
                    0x6 -> out += "SPORTS"
                }
                0x9 -> {
                    out += "ARTS"
                    when (level2) {
                        0x1 -> out += "MUSIC"
                        0x3 -> out += "COMEDY"
                    }
                }
                0xA -> when (level2) {
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

    private fun descriptorDiagnosticCount(json: String): Int = runCatching {
        JSONObject(json).optJSONArray("diagnostics")?.length() ?: JSONArray(json).length()
    }.getOrDefault(0)

    private fun extendedItemsJson(items: List<AribExtendedItem>): String = JSONArray().apply {
        items.forEach { put(JSONObject().put("description", it.itemDescription).put("text", it.itemText)) }
    }.toString()

    private fun publishStateSourceName(source: ProgramPublishStateSource?): String = (source ?: ProgramPublishStateSource.NONE).name
}

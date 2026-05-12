package com.maleicacid.tvinput.aribsi

import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.db.ChannelRecord
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
        fun from(
            diagnostic: ServicePublishabilityDiagnostic?,
            fallback: ChannelRecord?,
        ): ProgramPublishState {
            val diagnosticComplete = diagnostic?.isCurrentDiagnosticComplete() == true
            return when {
                diagnosticComplete -> ProgramPublishState(
                    requiresCas = diagnostic!!.requiresCas,
                    unsupportedCas = diagnostic.unsupportedCas,
                    clearLivePlaybackSupported = diagnostic.clearLivePlaybackSupported,
                    channelRegistrationReady = diagnostic.channelRegistrationReady,
                    epgPublishable = diagnostic.epgPublishable,
                    source = ProgramPublishStateSource.CURRENT_DIAGNOSTIC,
                )
                diagnostic != null && fallback != null -> {
                    val requiresCas = fallback.requiresCas || diagnostic.requiresCas
                    val unsupportedCas = fallback.unsupportedCas || diagnostic.unsupportedCas || requiresCas
                    ProgramPublishState(
                        requiresCas = requiresCas,
                        unsupportedCas = unsupportedCas,
                        clearLivePlaybackSupported = if (requiresCas || unsupportedCas) false else diagnostic.clearLivePlaybackSupported,
                        channelRegistrationReady = fallback.channelRegistrationReady || diagnostic.channelRegistrationReady,
                        epgPublishable = fallback.epgPublishable || diagnostic.epgPublishable,
                        source = ProgramPublishStateSource.MERGED_CHANNEL_CAS_STATE,
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
                else -> ProgramPublishState(
                    requiresCas = false,
                    unsupportedCas = false,
                    clearLivePlaybackSupported = false,
                    channelRegistrationReady = false,
                    epgPublishable = false,
                    source = ProgramPublishStateSource.NONE,
                )
            }
        }

        fun resolveByServiceKey(
            diagnostics: Map<ServiceKey, ServicePublishabilityDiagnostic>,
            channelFallbacks: Map<ServiceKey, ChannelRecord>,
            serviceKeys: Set<ServiceKey>,
        ): Map<ServiceKey, ProgramPublishState> = serviceKeys.associateWith { key ->
            from(diagnostics[key], channelFallbacks[key])
        }
    }
}

fun ServicePublishabilityDiagnostic.isCurrentDiagnosticComplete(): Boolean {
    if (!publishable) return false
    if (!channelRegistrationReady && !epgPublishable) return false
    if (missingComponents.isNotEmpty()) return false
    if (!pmtPidResolved || !pmtParsed) return false
    if (!caStateResolved) return false
    if (unsupportedCas && !requiresCas) return false
    val unresolvedMarkers = listOf(
        "UNRESOLVED",
        "NO_RUST_PUBLISHABILITY_DIAGNOSTIC",
        "NO_PMT_PID",
        "NO_PMT",
        "NO_PCR_PID",
        "NO_SUPPORTED_VIDEO_ES",
        "MISSING",
    )
    val allReasons = missingComponents + reasons + registrationReasons + epgReasons
    return allReasons.none { reason ->
        unresolvedMarkers.any { marker -> reason.contains(marker, ignoreCase = true) }
    }
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
            ProgramPublishState.resolveByServiceKey(
                diagnostics = publishabilityByServiceKey,
                channelFallbacks = channelFallbackByServiceKey,
                serviceKeys = serviceKeys,
            )
        }
        return events.mapNotNull { event ->
            val state = effectiveStates[event.serviceKey]
            val end = event.startTimeMillis + event.durationMillis
            if (event.startTimeMillis <= 0L || end <= event.startTimeMillis) {
                null
            } else {
                ProgramRecord(
                    serviceKey = event.serviceKey,
                    eventId = event.eventId,
                    stableIdentity = event.stableIdentity,
                    startTimeMillis = event.startTimeMillis,
                    durationMillis = event.durationMillis,
                    title = event.title.ifBlank { "event-${event.eventId}" },
                    description = providerDescription(event),
                    shortDescription = shortDescription(event),
                    extendedItemsJson = extendedItemsJson(event.extendedItems),
                    componentText = event.componentText,
                    audioComponentText = event.audioComponentText,
                    audioLanguage = event.audioLanguage,
                    canonicalGenre = null,
                    broadcastGenre = event.broadcastGenre,
                    genreSupplementText = event.genreSupplementText,
                    eventGroupText = event.eventGroupText,
                    freeCaText = event.freeCaText,
                    seriesName = event.seriesName,
                    requiresCas = state?.requiresCas ?: false,
                    unsupportedCas = state?.unsupportedCas ?: false,
                    clearLivePlaybackSupported = state?.clearLivePlaybackSupported ?: false,
                    channelRegistrationReady = state?.channelRegistrationReady ?: false,
                    epgPublishable = state?.epgPublishable ?: false,
                    publishStateSource = publishStateSourceName(state?.source),
                    diagnosticText = event.diagnosticText,
                    diagnosticDescriptorJson = event.diagnosticDescriptorJson,
                    contentRatings = event.parentalRatings.mapNotNull { AribRatingMapper.toTvContentRatingString(it) },
                    parentalRatingDiagnosticsJson = parentalRatingDiagnosticsJson(event),
                    unsupportedDescriptorJson = unsupportedDescriptorJson(event),
                )
            }
        }
    }

    private fun shortDescription(event: AribEvent): String = event.description.take(256)

    private fun providerDescription(event: AribEvent): String {
        val extended = event.extendedItems.joinToString("\n") { item ->
            if (item.itemDescription.isBlank()) item.itemText else "【${item.itemDescription}】${item.itemText}"
        }
        val uiSupplements = listOfNotNull(
            event.componentText?.takeIf { it.isNotBlank() }?.let { "映像: $it" },
            event.audioComponentText?.takeIf { it.isNotBlank() }?.let { "音声: $it" },
            event.genreSupplementText?.takeIf { it.isNotBlank() }?.let { "ジャンル: $it" },
            event.eventGroupText?.takeIf { it.isNotBlank() }?.let { "関連番組: $it" },
            event.freeCaText?.takeIf { it.isNotBlank() }?.let { "放送種別: $it" },
        )
        return listOf(event.description, event.extendedDescription, extended)
            .plus(uiSupplements)
            .filter { it.isNotBlank() }
            .joinToString("\n")
    }

    private fun unsupportedDescriptorJson(event: AribEvent): String {
        val unsupportedRatings = event.parentalRatings.filter { AribRatingMapper.toTvContentRatingString(it) == null }
        val arr = JSONArray()
        unsupportedRatings.forEach { rating ->
            arr.put(JSONObject()
                .put("tableId", -1)
                .put("pid", -1)
                .put("sectionNumber", -1)
                .put("descriptorOffset", -1)
                .put("diagnosticCode", "INVALID_PARENTAL_RATING")
                .put("country", rating.countryCode)
                .put("rating", rating.rating)
                .put("raw", rating.rawRating)
                .put("supported", rating.supported))
        }
        return JSONObject().put("diagnostics", arr).toString()
    }

    private fun parentalRatingDiagnosticsJson(event: AribEvent): String {
        val arr = JSONArray()
        event.parentalRatings.forEach { rating ->
            val mapped = AribRatingMapper.toTvContentRatingString(rating)
            val parseStatus = when {
                !rating.supported -> "unsupported"
                rating.countryCode != "JPN" -> "unsupported_country"
                rating.rating !in 4..20 -> "unsupported_rating"
                mapped == null -> "unmapped"
                else -> "mapped"
            }
            arr.put(JSONObject()
                .put("countryCode", rating.countryCode)
                .put("ratingValue", rating.rating)
                .put("rawRatingByte", rating.rawRating)
                .put("supported", rating.supported)
                .put("parseStatus", parseStatus)
                .put("mappedTvContentRating", mapped.orEmpty())
                .put("diagnosticCode", if (mapped == null) "INVALID_PARENTAL_RATING" else "MISSING_PARENTAL_RATING"))
        }
        return JSONObject().put("parentalRatings", arr).toString()
    }

    private fun extendedItemsJson(items: List<AribExtendedItem>): String {
        val arr = JSONArray()
        items.forEach { item ->
            arr.put(JSONObject().put("key", item.itemDescription).put("value", item.itemText))
        }
        return arr.toString()
    }

    private fun publishStateSourceName(source: ProgramPublishStateSource?): String = when (source) {
        ProgramPublishStateSource.CURRENT_DIAGNOSTIC -> "current"
        ProgramPublishStateSource.CHANNEL_FALLBACK,
        ProgramPublishStateSource.MERGED_CHANNEL_CAS_STATE -> "fallback"
        ProgramPublishStateSource.NONE,
        null -> "none"
    }
}

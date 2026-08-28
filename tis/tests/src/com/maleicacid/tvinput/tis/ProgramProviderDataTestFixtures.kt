package com.maleicacid.tvinput.tis

import com.maleicacid.tvinput.aribsi.AribEvent
import com.maleicacid.tvinput.aribsi.AribProgramSource
import com.maleicacid.tvinput.aribsi.ProviderDataBridge
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.db.ProgramDescriptors
import com.maleicacid.tvinput.db.ProgramRecord
import org.json.JSONArray
import org.json.JSONObject

internal fun AribEvent.withCanonicalProgramProviderDataForTest(
    requiresCas: Boolean = descriptors.scrambled == true,
): AribEvent = copy(
    providerDataCanonicalJson = canonicalProgramProviderDataForTest(
        serviceKey = serviceKey,
        eventId = eventId,
        startTimeMillis = startTimeMillis,
        durationMillis = durationMillis,
        source = source,
        descriptors = ProgramDescriptors(
            extendedItems = descriptors.extendedItems,
            contentGenres = descriptors.contentGenres,
            eventGroups = descriptors.eventGroups,
            linkage = descriptors.linkage,
            scrambled = descriptors.scrambled,
            freeCaMode = descriptors.freeCaMode,
            series = descriptors.series,
            descriptorDiagnosticsCanonicalJson = descriptors.diagnostics.descriptorDiagnosticsCanonicalJson,
            parentalRatings = descriptors.parentalRatings,
            components = descriptors.components,
        ),
        requiresCas = requiresCas,
    ),
)

internal fun ProgramRecord.withCanonicalProgramProviderDataForTest(): ProgramRecord = copy(
    providerDataCanonicalJson = canonicalProgramProviderDataForTest(
        serviceKey = serviceKey,
        eventId = eventId,
        startTimeMillis = startTimeMillis,
        durationMillis = durationMillis,
        source = source,
        descriptors = descriptors,
        requiresCas = requiresCas,
        malformedCaDescriptorCount = malformedCaDescriptorCount,
    ),
)

private fun canonicalProgramProviderDataForTest(
    serviceKey: ServiceKey,
    eventId: Int,
    startTimeMillis: Long,
    durationMillis: Long,
    source: AribProgramSource,
    descriptors: ProgramDescriptors,
    requiresCas: Boolean,
    malformedCaDescriptorCount: Int = 0,
): String = JSONObject()
    .put("schema", "maleicacid.tv.program")
    .put("schemaVersion", 1)
    .put(
        "programKey",
        JSONObject()
            .put("kind", "arib-event-v1")
            .put("originalNetworkId", serviceKey.originalNetworkId)
            .put("transportStreamId", serviceKey.transportStreamId)
            .put("serviceId", serviceKey.serviceId)
            .put("eventId", eventId),
    )
    .put(
        "timing",
        JSONObject()
            .put("startUtcMillis", startTimeMillis)
            .put("durationMillis", durationMillis),
    )
    .put(
        "source",
        JSONObject()
            .put("pid", source.pid.value)
            .put("tableId", source.tableId)
            .put("version", source.version)
            .put("sectionNumber", source.sectionNumber)
            .put("lastSectionNumber", source.lastSectionNumber),
    )
    .put(
        "cas",
        JSONObject()
            .put("requiresCas", requiresCas)
            .put("source", "SI_SEMANTICS"),
    )
    .put("ratings", JSONArray().apply {
        descriptors.parentalRatings.forEach { rating ->
            put(
                JSONObject()
                    .put("countryCode", rating.countryCode)
                    .put("rawRatingByte", rating.rawRatingByte)
                    .put("parseStatus", rating.parseStatus),
            )
        }
    })
    .put("genres", JSONArray().apply {
        descriptors.contentGenres.forEach { genre ->
            put(
                JSONObject()
                    .put("level1", genre.level1)
                    .put("level2", genre.level2)
                    .put("userNibble", genre.userNibble)
                    .put("aribName", genre.aribName)
                    .put("parseStatus", genre.parseStatus),
            )
        }
    })
    .put("series", descriptors.series?.let { series ->
        JSONObject()
            .put("seriesId", requireNotNull(series.seriesId))
            .put("repeatLabel", series.repeatLabel)
            .put("programPattern", series.programPattern)
            .put("expireDateValid", series.expireDateValid)
            .put("expireDate", series.expireDate ?: JSONObject.NULL)
            .put("episodeNumber", requireNotNull(series.episodeNumber))
            .put("lastEpisodeNumber", requireNotNull(series.lastEpisodeNumber))
            .put("name", series.name ?: JSONObject.NULL)
            .put("parseStatus", series.parseStatus)
    } ?: JSONObject.NULL)
    .put("eventGroups", JSONArray().apply {
        descriptors.eventGroups.forEach { group ->
            put(
                JSONObject()
                    .put("groupType", group.groupType)
                    .put("events", JSONArray().apply {
                        group.events.forEach { reference ->
                            put(JSONObject().put("serviceId", reference.serviceId).put("eventId", reference.eventId))
                        }
                    })
                    .put("otherNetworkEvents", JSONArray().apply {
                        group.otherNetworkEvents.forEach { reference ->
                            put(
                                JSONObject()
                                    .put("originalNetworkId", reference.originalNetworkId)
                                    .put("transportStreamId", reference.transportStreamId)
                                    .put("serviceId", reference.serviceId)
                                    .put("eventId", reference.eventId),
                            )
                        }
                    })
                    .put("privateDataHex", group.privateDataHex)
                    .put("parseStatus", group.parseStatus),
            )
        }
    })
    .put("linkage", JSONArray().apply {
        descriptors.linkage.forEach { linkage ->
            put(
                JSONObject()
                    .put("transportStreamId", linkage.transportStreamId)
                    .put("originalNetworkId", linkage.originalNetworkId)
                    .put("serviceId", linkage.serviceId)
                    .put("linkageType", linkage.linkageType)
                    .put("privateDataPrefixHex", linkage.privateDataHex)
                    .put("parseStatus", linkage.parseStatus),
            )
        }
    })
    .put("freeCaMode", descriptors.freeCaMode?.takeIf { it.raw != null && it.scrambled != null }?.let { mode ->
        JSONObject()
            .put("raw", mode.raw)
            .put("scrambled", mode.scrambled)
            .put("text", mode.text ?: JSONObject.NULL)
            .put("parseStatus", mode.parseStatus)
    } ?: JSONObject.NULL)
    .put("extendedItems", JSONArray().apply {
        descriptors.extendedItems.forEach { item ->
            put(
                JSONObject()
                    .put("languageCode", item.languageCode)
                    .put("description", item.itemDescription)
                    .put("text", item.itemText)
                    .put("parseStatus", "OK"),
            )
        }
    })
    .put("components", ProviderDataBridge.toComponentsObject(descriptors.components))
    .put(
        "diagnostics",
        JSONObject()
            .put(
                "descriptorDiagnostics",
                JSONArray(descriptors.descriptorDiagnosticsCanonicalJson.ifBlank { "[]" }),
            )
            .put("publishDiagnostics", JSONArray())
            .put("parserDiagnostics", JSONArray())
            .apply {
                if (malformedCaDescriptorCount > 0) {
                    put("malformedCaDescriptorCount", malformedCaDescriptorCount)
                }
            },
    )
    .toString()

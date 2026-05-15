package com.maleicacid.tvinput.aribsi

import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.StreamSelector
import com.maleicacid.tvinput.db.ChannelRecord
import com.maleicacid.tvinput.db.ProgramRecord
import org.json.JSONArray
import org.json.JSONObject

object ProviderDataBridge {
    data class Result(val json: String, val signature: String, val extractedKey: String)
    data class ProgramKeyResult(
        val serviceKey: ServiceKey,
        val eventId: Int,
        val key: String,
    )
    data class ChannelTuneKey(
        val serviceKey: ServiceKey,
        val system: String,
        val frequencyHz: Long,
        val streamSelector: StreamSelector,
        val physicalChannel: Int?,
        val backendHint: String?,
        val satelliteBand: String?,
        val remoteControlKeyId: Int?,
        val requiresCas: Boolean,
        val unsupportedCas: Boolean,
        val clearLivePlaybackSupported: Boolean,
        val channelRegistrationReady: Boolean,
        val epgPublishable: Boolean,
    )

    private val native by lazy { NativeAribSiParser() }

    fun buildChannelProviderData(channel: ChannelRecord): Result {
        val selector = channel.streamSelector
        val request = JSONObject()
            .put("serviceKey", JSONObject()
                .put("originalNetworkId", channel.serviceKey.originalNetworkId)
                .put("transportStreamId", channel.serviceKey.transportStreamId)
                .put("serviceId", channel.serviceKey.serviceId))
            .put("tune", JSONObject()
                .put("inputId", channel.inputId ?: JSONObject.NULL)
                .put("displayName", channel.displayName.ifBlank { JSONObject.NULL })
                .put("system", channel.deliverySystem)
                .put("frequencyHz", channel.frequencyHz)
                .put("streamSelector", JSONObject()
                    .put("type", selector.type.name)
                    .put("value", selector.value?.toString().orEmpty()))
                .put("physicalChannel", channel.physicalChannel ?: JSONObject.NULL)
                .put("backendHint", channel.backendHint ?: JSONObject.NULL)
                .put("satelliteBand", channel.satelliteBand ?: JSONObject.NULL)
                .put("remoteControlKeyId", channel.remoteControlKeyId ?: JSONObject.NULL))
            .put("cas", JSONObject()
                .put("requiresCas", channel.requiresCas)
                .put("unsupportedCas", channel.unsupportedCas)
                .put("clearLivePlaybackSupported", channel.clearLivePlaybackSupported))
            .put("diagnostics", JSONObject()
                .put("channelRegistrationReady", channel.channelRegistrationReady)
                .put("epgPublishable", channel.epgPublishable)
                .put("publishStateSource", "current"))
        return parseResult(native.buildChannelProviderData(request.toString()))
    }

    fun buildProgramKey(program: ProgramRecord): String =
        "onid=${program.serviceKey.originalNetworkId};tsid=${program.serviceKey.transportStreamId};sid=${program.serviceKey.serviceId};event=${program.eventId.takeIf { it >= 0 } ?: -1}"

    fun buildProgramProviderData(program: ProgramRecord): Result {
        val descriptors = program.descriptors
        val request = JSONObject()
            .put("programKey", JSONObject()
                .put("kind", "arib-event-v1")
                .put("originalNetworkId", program.serviceKey.originalNetworkId)
                .put("transportStreamId", program.serviceKey.transportStreamId)
                .put("serviceId", program.serviceKey.serviceId)
                .put("eventId", program.eventId.takeIf { it >= 0 } ?: -1))
            .put("serviceKey", JSONObject()
                .put("originalNetworkId", program.serviceKey.originalNetworkId)
                .put("transportStreamId", program.serviceKey.transportStreamId)
                .put("serviceId", program.serviceKey.serviceId))
            .put("timing", JSONObject()
                .put("startUtcMillis", program.startTimeMillis)
                .put("endUtcMillis", program.startTimeMillis + program.durationMillis)
                .put("durationMillis", program.durationMillis))
            .put("cas", JSONObject()
                .put("requiresCas", program.requiresCas)
                .put("unsupportedCas", program.unsupportedCas)
                .put("clearLivePlaybackSupported", program.clearLivePlaybackSupported)
                .put("source", program.publishStateSource.lowercase()))
            .put("extendedItems", toExtendedItemsArray(descriptors.extendedItems))
            .put("genres", genresJson(program))
            .put("relatedItems", toRelatedItemsArray(descriptors.relatedItems))
            .put("linkage", toLinkageArray(descriptors.linkage))
            .put("freeCaMode", toFreeCaModeObject(descriptors))
            .put("series", toSeriesObject(descriptors))
            .put("diagnostics", JSONObject()
                .put("descriptorDiagnostics", toDescriptorDiagnosticsArray(descriptors.descriptorDiagnostics))
                .put("publishDiagnostics", publishDiagnosticsJson(program))
                .put("parserDiagnostics", JSONArray()))
            .put("ratings", ratingsJson(program))
            .put("audioLanguages", audioLanguagesJson(descriptors))
            .put("audio", audioMetadataJson(descriptors))
            .put("video", videoMetadataJson(program))
            .put("components", toComponentsObject(descriptors.components))
            .put("source", JSONObject()
                .put("pid", 18)
                .put("tableId", 0x4e)
                .put("version", 0)
                .put("sectionNumber", 0)
                .put("lastSectionNumber", 0))
            .put("malformedCaDescriptorCount", program.malformedCaDescriptorCount.coerceAtLeast(0))
            .put("droppedRetryWindowCount", program.droppedRetryWindowCount.coerceAtLeast(0))
        return parseResult(native.buildProgramProviderData(request.toString()))
    }

    fun normalizeProgramProviderData(providerData: String?): Result =
        parseResult(native.normalizeProgramProviderData(providerData.orEmpty()))

    fun programProviderDataSignature(providerData: String?): String =
        native.programProviderDataSignature(providerData.orEmpty())

    fun extractProgramKeyResult(providerData: String?): ProgramKeyResult? {
        val raw = native.extractProgramKeyResult(providerData.orEmpty()).takeIf { it.isNotBlank() } ?: return null
        val obj = JSONObject(raw)
        val onid = obj.optInt("originalNetworkId", -1)
        val tsid = obj.optInt("transportStreamId", -1)
        val sid = obj.optInt("serviceId", -1)
        val eventId = obj.optInt("eventId", -1)
        val key = obj.optString("key")
        if (onid < 0 || tsid < 0 || sid < 0 || eventId < 0 || key.isBlank()) return null
        return ProgramKeyResult(ServiceKey(onid, tsid, sid), eventId, key)
    }

    fun extractProgramKey(providerData: String?): String? =
        extractProgramKeyResult(providerData)?.key

    fun appendCurrentProgramDiagnostics(providerData: String?, overlapCount: Int, selectedProgramId: Long, selectionRule: String): Result =
        parseResult(native.appendCurrentProgramDiagnostics(providerData.orEmpty(), overlapCount.toLong(), selectedProgramId, selectionRule))

    fun extractChannelTuneKey(providerData: String?): ChannelTuneKey? {
        val text = native.extractChannelTuneKey(providerData.orEmpty()).takeIf { it.isNotBlank() } ?: return null
        val obj = runCatching { JSONObject(text) }.getOrNull() ?: return null
        if (obj.optString("schema") != "maleicacid.tv.channel") return null
        val serviceKeyObj = obj.optJSONObject("serviceKey") ?: return null
        val tuneObj = obj.optJSONObject("tune") ?: return null
        val selectorObj = tuneObj.optJSONObject("streamSelector") ?: JSONObject()
        val casObj = obj.optJSONObject("cas") ?: JSONObject()
        val diagnosticsObj = obj.optJSONObject("diagnostics") ?: JSONObject()
        val onid = serviceKeyObj.optInt("originalNetworkId", -1)
        val tsid = serviceKeyObj.optInt("transportStreamId", -1)
        val sid = serviceKeyObj.optInt("serviceId", -1)
        val system = tuneObj.optString("system").ifBlank { return null }
        val frequencyHz = tuneObj.optLong("frequencyHz", -1L)
        if (onid < 0 || tsid < 0 || sid < 0 || frequencyHz <= 0L) return null
        return ChannelTuneKey(
            serviceKey = ServiceKey(onid, tsid, sid),
            system = system,
            frequencyHz = frequencyHz,
            streamSelector = runCatching { StreamSelector.fromStored(selectorObj.optString("type"), selectorObj.optString("value").takeIf { it.isNotBlank() }) }.getOrDefault(StreamSelector.NONE),
            physicalChannel = optIntOrNull(tuneObj, "physicalChannel"),
            backendHint = optStringOrNull(tuneObj, "backendHint"),
            satelliteBand = optStringOrNull(tuneObj, "satelliteBand"),
            remoteControlKeyId = optIntOrNull(tuneObj, "remoteControlKeyId"),
            requiresCas = casObj.optBoolean("requiresCas", false),
            unsupportedCas = casObj.optBoolean("unsupportedCas", false),
            clearLivePlaybackSupported = casObj.optBoolean("clearLivePlaybackSupported", false),
            channelRegistrationReady = diagnosticsObj.optBoolean("channelRegistrationReady", false),
            epgPublishable = diagnosticsObj.optBoolean("epgPublishable", false),
        )
    }

    private fun ratingsJson(program: ProgramRecord): JSONArray {
        val arr = JSONArray()
        program.descriptors.parentalRatings.forEach { rating ->
            arr.put(JSONObject()
                .put("countryCode", rating.countryCode)
                .put("ratingValue", rating.rating)
                .put("rawRatingByte", rating.rawRating)
                .put("supported", rating.supported)
                .put("mappedTvContentRating", AribRatingMapper.toTvContentRatingString(rating) ?: JSONObject.NULL)
                .put("parseStatus", if (rating.supported) "OK" else "UNSUPPORTED"))
        }
        if (arr.length() == 0) {
            program.contentRatings.distinct().sorted().forEach { flattened ->
                val rating = Regex("ISDB_(\d{1,2})").find(flattened)?.groupValues?.getOrNull(1)?.toIntOrNull() ?: return@forEach
                arr.put(JSONObject()
                    .put("countryCode", "JPN")
                    .put("ratingValue", rating)
                    .put("rawRatingByte", rating)
                    .put("supported", true)
                    .put("mappedTvContentRating", flattened)
                    .put("parseStatus", "OK"))
            }
        }
        return arr
    }

    private fun genresJson(program: ProgramRecord): JSONArray {
        val arr = JSONArray()
        val canonical = JSONArray(program.canonicalGenres.distinct().sorted())
        val genre = program.descriptors.broadcastGenre?.takeIf { it.isNotBlank() }
        if (genre == null) {
            if (canonical.length() > 0) {
                arr.put(JSONObject()
                    .put("level1", 0)
                    .put("level2", 0)
                    .put("userNibble", 0)
                    .put("aribName", "")
                    .put("unmappedReason", JSONObject.NULL)
                    .put("canonicalGenres", canonical)
                    .put("parseStatus", "TIS_CANONICAL_ONLY"))
            }
            return arr
        }
        val regex = Regex("ARIB\\(0x([0-9a-fA-F]+)/0x([0-9a-fA-F]+)\\):?([^、]*)")
        var matched = false
        regex.findAll(genre).forEach { match ->
            val level1 = match.groupValues.getOrNull(1)?.toIntOrNull(16) ?: return@forEach
            val level2 = match.groupValues.getOrNull(2)?.toIntOrNull(16) ?: return@forEach
            val aribName = match.groupValues.getOrNull(3)?.takeIf { it.isNotBlank() } ?: genre
            matched = true
            arr.put(JSONObject()
                .put("level1", level1)
                .put("level2", level2)
                .put("userNibble", 0)
                .put("aribName", aribName)
                .put("unmappedReason", if (canonical.length() == 0) "TIS_DECIDES_CANONICAL_GENRE" else JSONObject.NULL)
                .put("canonicalGenres", canonical)
                .put("parseStatus", "OK"))
        }
        if (!matched && canonical.length() > 0) {
            arr.put(JSONObject()
                .put("level1", 0)
                .put("level2", 0)
                .put("userNibble", 0)
                .put("aribName", genre)
                .put("unmappedReason", JSONObject.NULL)
                .put("canonicalGenres", canonical)
                .put("parseStatus", "TIS_CANONICAL_WITH_UNPARSED_ARIB_GENRE"))
        }
        return arr
    }

    private fun toFreeCaModeObject(descriptors: com.maleicacid.tvinput.db.ProgramDescriptors): Any {
        descriptors.freeCaMode?.let { mode ->
            return JSONObject()
                .put("raw", mode.raw ?: JSONObject.NULL)
                .put("scrambled", mode.scrambled ?: JSONObject.NULL)
                .put("text", mode.text ?: JSONObject.NULL)
                .put("parseStatus", mode.parseStatus)
        }
        return when (val scrambled = descriptors.scrambled) {
            null -> JSONObject.NULL
            else -> JSONObject()
                .put("raw", if (scrambled) 1 else 0)
                .put("scrambled", scrambled)
                .put("text", if (scrambled) "有料放送" else "無料放送")
                .put("parseStatus", "OK")
        }
    }

    private fun toSeriesObject(descriptors: com.maleicacid.tvinput.db.ProgramDescriptors): Any {
        descriptors.series?.let { series ->
            return JSONObject()
                .put("seriesId", series.seriesId ?: JSONObject.NULL)
                .put("repeatLabel", series.repeatLabel)
                .put("programPattern", series.programPattern)
                .put("expireDateValid", series.expireDateValid)
                .put("expireDate", series.expireDate ?: JSONObject.NULL)
                .put("episodeNumber", series.episodeNumber ?: JSONObject.NULL)
                .put("lastEpisodeNumber", series.lastEpisodeNumber ?: JSONObject.NULL)
                .put("name", series.name ?: JSONObject.NULL)
                .put("parseStatus", series.parseStatus)
        }
        val seriesId = descriptors.seriesId ?: return JSONObject.NULL
        return JSONObject()
            .put("seriesId", seriesId)
            .put("repeatLabel", 0)
            .put("programPattern", 0)
            .put("expireDateValid", false)
            .put("expireDate", JSONObject.NULL)
            .put("episodeNumber", descriptors.episodeNumber ?: 0)
            .put("lastEpisodeNumber", descriptors.lastEpisodeNumber ?: 0)
            .put("name", JSONObject.NULL)
            .put("parseStatus", "OK")
    }

    private fun publishDiagnosticsJson(program: ProgramRecord): JSONArray {
        val arr = JSONArray()
        program.descriptors.parentalRatings.filter { AribRatingMapper.toTvContentRatingString(it) == null }.forEach { rating ->
            arr.put(JSONObject()
                .put("code", "UNSUPPORTED_PARENTAL_RATING")
                .put("message", "country=${rating.countryCode} rating=${rating.rating} supported=${rating.supported}")
                .put("severity", "warning"))
        }
        return arr
    }

    private fun audioLanguagesJson(descriptors: com.maleicacid.tvinput.db.ProgramDescriptors): JSONArray = JSONArray().apply {
        descriptors.audioLanguage?.takeIf { it.isNotBlank() }?.let { put(JSONObject().put("language", it).put("source", "AUDIO_COMPONENT").put("parseStatus", "OK")) }
    }

    private fun audioMetadataJson(descriptors: com.maleicacid.tvinput.db.ProgramDescriptors): Any =
        descriptors.audioComponentText?.takeIf { it.isNotBlank() }?.let {
            JSONObject().put("codec", "UNKNOWN_AUDIO").put("language", descriptors.audioLanguage ?: JSONObject.NULL).put("text", it).put("parseStatus", "OK")
        } ?: JSONObject.NULL

    private fun videoMetadataJson(program: ProgramRecord): Any =
        if (program.videoFormat.isNullOrBlank() && program.videoWidth == null && program.videoHeight == null) JSONObject.NULL else JSONObject()
            .put("codec", program.videoFormat ?: "UNKNOWN_VIDEO")
            .put("format", program.videoFormat ?: JSONObject.NULL)
            .put("width", program.videoWidth ?: JSONObject.NULL)
            .put("height", program.videoHeight ?: JSONObject.NULL)
            .put("parseStatus", "OK")

    private fun optStringOrNull(obj: JSONObject, key: String): String? =
        if (obj.has(key) && !obj.isNull(key)) obj.optString(key).takeIf { it.isNotBlank() } else null

    private fun optIntOrNull(obj: JSONObject, key: String): Int? =
        if (obj.has(key) && !obj.isNull(key)) obj.optInt(key) else null

    private fun parseResult(raw: String): Result {
        val obj = JSONObject(raw.ifBlank { "{}" })
        return Result(
            json = obj.optString("json", "{}"),
            signature = obj.optString("signature", ""),
            extractedKey = obj.optString("extractedKey", ""),
        )
    }

    private fun toExtendedItemsArray(items: List<AribExtendedItem>): JSONArray = JSONArray().apply {
        items.forEach { item -> put(JSONObject().put("description", item.itemDescription).put("text", item.itemText)) }
    }

    private fun toRelatedItemsArray(items: List<AribRelatedItem>): JSONArray = JSONArray().apply {
        items.forEach { item ->
            put(JSONObject()
                .put("kind", item.kind)
                .put("groupType", item.groupType)
                .put("originalNetworkId", item.originalNetworkId ?: JSONObject.NULL)
                .put("transportStreamId", item.transportStreamId ?: JSONObject.NULL)
                .put("serviceId", item.serviceId)
                .put("eventId", item.eventId)
                .put("parseStatus", item.parseStatus))
        }
    }

    private fun toLinkageArray(items: List<AribLinkage>): JSONArray = JSONArray().apply {
        items.forEach { item ->
            put(JSONObject()
                .put("linkageType", item.linkageType)
                .put("originalNetworkId", item.originalNetworkId)
                .put("transportStreamId", item.transportStreamId)
                .put("serviceId", item.serviceId)
                .put("privateDataHex", item.privateDataHex)
                .put("parseStatus", item.parseStatus))
        }
    }

    fun toComponentsObject(components: AribComponents): JSONObject = JSONObject()
        .put("video", componentEntriesJson(components.video))
        .put("audio", componentEntriesJson(components.audio))
        .put("subtitle", componentEntriesJson(components.subtitle))
        .put("data", componentEntriesJson(components.data))

    private fun componentEntriesJson(entries: List<AribComponentEntry>): JSONArray = JSONArray().apply {
        entries.forEach { entry ->
            val obj = JSONObject().put("esPid", entry.esPid).put("parseStatus", entry.parseStatus)
            entry.streamType?.let { obj.put("streamType", it) }
            entry.componentTag?.let { obj.put("componentTag", it) }
            entry.componentType?.let { obj.put("componentType", it) }
            entry.codec?.let { obj.put("codec", it) }
            entry.language?.let { obj.put("language", it) }
            entry.secondLanguage?.let { obj.put("secondLanguage", it) }
            entry.channelConfiguration?.let { obj.put("channelConfiguration", it) }
            entry.samplingInfo?.let { obj.put("samplingInfo", it) }
            entry.sourceDescriptor?.let { obj.put("sourceDescriptor", it) }
            entry.resolution?.let { obj.put("resolution", it) }
            entry.scan?.let { obj.put("scan", it) }
            entry.aspect?.let { obj.put("aspect", it) }
            entry.profileLevel?.let { obj.put("profileLevel", it) }
            entry.dataComponentId?.let { obj.put("dataComponentId", it) }
            entry.trackId?.let { obj.put("trackId", it) }
            entry.captionServiceKind?.let { obj.put("captionServiceKind", it) }
            entry.r51PlaybackSupported?.let { obj.put("r51PlaybackSupported", it) }
            entry.liveViewableClaim?.let { obj.put("liveViewableClaim", it) }
            entry.diagnosticCode?.let { obj.put("diagnosticCode", it) }
            entry.main?.let { obj.put("main", it) }
            entry.multiLingual?.let { obj.put("multiLingual", it) }
            entry.qualityIndicator?.let { obj.put("qualityIndicator", it) }
            put(obj)
        }
    }

    fun toDescriptorDiagnosticsArray(items: List<AribDescriptorDiagnosticV1>): JSONArray = JSONArray().apply {
        items.forEach { item ->
            put(JSONObject()
                .put("schema", item.schema)
                .put("schemaVersion", item.schemaVersion)
                .put("severity", item.severity)
                .put("code", item.code)
                .put("scope", JSONObject()
                    .put("pid", item.scope.pid)
                    .put("tableId", item.scope.tableId)
                    .put("tableIdExtension", item.scope.tableIdExtension)
                    .put("version", item.scope.version)
                    .put("sectionNumber", item.scope.sectionNumber)
                    .put("originalNetworkId", item.scope.originalNetworkId)
                    .put("transportStreamId", item.scope.transportStreamId)
                    .put("serviceId", item.scope.serviceId)
                    .put("eventId", item.scope.eventId))
                .put("descriptor", JSONObject()
                    .put("tag", item.descriptor.tag)
                    .put("name", item.descriptor.name)
                    .put("offset", item.descriptor.offset)
                    .put("declaredLength", item.descriptor.declaredLength)
                    .put("actualRemainingLength", item.descriptor.actualRemainingLength)
                    .put("parseStatus", item.descriptor.parseStatus)
                    .put("rawPrefixHex", item.descriptor.rawPrefixHex))
                .put("message", item.message))
        }
    }
}

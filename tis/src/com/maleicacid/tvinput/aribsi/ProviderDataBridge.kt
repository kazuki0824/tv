package com.maleicacid.tvinput.aribsi

import com.maleicacid.tvinput.common.FrequencyHz
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.StreamSelector
import com.maleicacid.tvinput.db.ChannelRecord
import com.maleicacid.tvinput.db.ProgramRecord
import org.json.JSONArray
import org.json.JSONObject

object ProviderDataBridge {
    data class Result(
        val bytes: ByteArray,
        val signature: String,
        val schemaVersion: Int,
        val truncated: Boolean,
        val diagnosticsDroppedCount: Int,
    ) {
        val json: String get() = bytes.toString(Charsets.UTF_8)
    }
    data class ProgramKeyResult(
        val serviceKey: ServiceKey,
        val eventId: Int,
        val key: String,
    )
    data class ChannelTuneKey(
        val serviceKey: ServiceKey,
        val system: String,
        val frequencyHz: FrequencyHz,
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
            .put("schema", "maleicacid.tv.channelRequest")
            .put("schemaVersion", 1)
            .put("serviceKey", JSONObject()
                .put("originalNetworkId", channel.serviceKey.originalNetworkId)
                .put("transportStreamId", channel.serviceKey.transportStreamId)
                .put("serviceId", channel.serviceKey.serviceId))
            .put("tune", JSONObject()
                .put("inputId", channel.inputId ?: JSONObject.NULL)
                .put("displayName", channel.displayName)
                .put("deliverySystem", channel.deliverySystem)
                .put("frequencyHz", channel.frequencyHz.value)
                .put("streamId", selector.value ?: JSONObject.NULL)
                .put("streamIdType", selector.type.name)
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

    fun buildProgramKey(program: ProgramRecord): String = buildProgramKey(program.serviceKey, program.eventId)

    fun buildProgramKey(serviceKey: ServiceKey, eventId: Int): String =
        native.buildProgramKey(
            serviceKey.originalNetworkId,
            serviceKey.transportStreamId,
            serviceKey.serviceId,
            eventId,
        )

    fun buildProgramProviderData(program: ProgramRecord): Result {
        val descriptors = program.descriptors
        val request = JSONObject()
            .put("schema", "maleicacid.tv.programRequest")
            .put("schemaVersion", 1)
            .put("programKey", JSONObject()
                .put("kind", "arib-event-v1")
                .put("originalNetworkId", program.serviceKey.originalNetworkId)
                .put("transportStreamId", program.serviceKey.transportStreamId)
                .put("serviceId", program.serviceKey.serviceId)
                .put("eventId", program.eventId))
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
                .put("descriptorDiagnosticsCanonicalJson", descriptors.descriptorDiagnosticsCanonicalJson)
                .put("publishDiagnostics", publishDiagnosticsJson(program))
                .put("parserDiagnostics", JSONArray()))
            .put("ratings", ratingsJson(program))
            .put("audioLanguages", audioLanguagesJson(descriptors))
            .put("audio", audioMetadataJson(descriptors))
            .put("video", videoMetadataJson(program))
            .put("components", toComponentsObject(descriptors.components))
            .put("source", JSONObject()
                .put("pid", program.source.pid.value)
                .put("tableId", program.source.tableId)
                .put("version", program.source.version)
                .put("sectionNumber", program.source.sectionNumber)
                .put("lastSectionNumber", program.source.lastSectionNumber))
            .put("malformedCaDescriptorCount", program.malformedCaDescriptorCount.coerceAtLeast(0))
            .put("droppedRetryWindowCount", program.droppedRetryWindowCount.coerceAtLeast(0))
        return parseResult(native.buildProgramProviderData(request.toString()))
    }

    fun normalizeProgramProviderData(providerData: ByteArray?): Result =
        parseResult(native.normalizeProgramProviderData(providerData ?: ByteArray(0)))

    fun programProviderDataSignature(providerData: ByteArray?): String =
        native.programProviderDataSignature(providerData ?: ByteArray(0))

    fun extractProgramKeyResult(providerData: ByteArray?): ProgramKeyResult? {
        val raw = native.extractProgramKeyResult(providerData ?: ByteArray(0)).takeIf { it.isNotBlank() } ?: return null
        val obj = JSONObject(raw)
        val onid = obj.optInt("originalNetworkId", -1)
        val tsid = obj.optInt("transportStreamId", -1)
        val sid = obj.optInt("serviceId", -1)
        val eventId = obj.optInt("eventId", -1)
        val key = obj.optString("key")
        if (onid < 0 || tsid < 0 || sid < 0 || eventId < 0 || key.isBlank()) return null
        return ProgramKeyResult(ServiceKey(onid, tsid, sid), eventId, key)
    }

    fun extractProgramKey(providerData: ByteArray?): String? =
        extractProgramKeyResult(providerData)?.key

    fun appendCurrentProgramDiagnostics(providerData: ByteArray?, overlapCount: Int, selectedProgramId: Long, selectionRule: String): Result =
        parseResult(native.appendCurrentProgramDiagnostics(providerData ?: ByteArray(0), overlapCount.toLong(), selectedProgramId, selectionRule))

    fun extractChannelTuneKey(providerData: String?): ChannelTuneKey? {
        val text = native.extractChannelTuneKey(providerData.orEmpty()).takeIf { it.isNotBlank() } ?: return null
        val obj = runCatching { JSONObject(text) }.getOrNull() ?: return null
        if (obj.optString("schema") != "maleicacid.tv.channel") return null
        val serviceKeyObj = obj.optJSONObject("serviceKey") ?: return null
        val tuneObj = obj.optJSONObject("tune") ?: return null
        val casObj = obj.optJSONObject("cas") ?: JSONObject()
        val diagnosticsObj = obj.optJSONObject("diagnostics") ?: JSONObject()
        val onid = serviceKeyObj.optInt("originalNetworkId", -1)
        val tsid = serviceKeyObj.optInt("transportStreamId", -1)
        val sid = serviceKeyObj.optInt("serviceId", -1)
        val system = tuneObj.optString("deliverySystem").ifBlank { return null }
        val frequencyHz = FrequencyHz.fromOrNull(tuneObj.optLong("frequencyHz", -1L))
        if (onid < 0 || tsid < 0 || sid < 0 || frequencyHz == null) return null
        return ChannelTuneKey(
            serviceKey = ServiceKey(onid, tsid, sid),
            system = system,
            frequencyHz = frequencyHz,
            streamSelector = runCatching { StreamSelector.fromStored(tuneObj.optString("streamIdType"), optIntOrNull(tuneObj, "streamId")?.toString()) }.getOrDefault(StreamSelector.NONE),
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
                .put("ratingValue", rating.ratingValue)
                .put("rawRatingByte", rating.rawRatingByte)
                .put("supported", rating.supported)
                .put("mappedTvContentRating", AribRatingMapper.toTvContentRatingString(rating) ?: JSONObject.NULL)
                .put("parseStatus", rating.parseStatus))
        }
        return arr
    }

    private fun genresJson(program: ProgramRecord): JSONArray = JSONArray().apply {
        program.descriptors.contentGenres.forEach { genre ->
            put(JSONObject()
                .put("level1", genre.level1)
                .put("level2", genre.level2)
                .put("userNibble", genre.userNibble)
                .put("aribName", genre.aribName)
                .put("unmappedReason", JSONObject.NULL)
                .put("parseStatus", genre.parseStatus))
        }
    }

    private fun toFreeCaModeObject(descriptors: com.maleicacid.tvinput.db.ProgramDescriptors): Any = descriptors.freeCaMode?.let { mode ->
        JSONObject()
            .put("raw", mode.raw ?: JSONObject.NULL)
            .put("scrambled", mode.scrambled ?: JSONObject.NULL)
            .put("text", mode.text ?: JSONObject.NULL)
            .put("parseStatus", mode.parseStatus)
    } ?: JSONObject.NULL

    private fun toSeriesObject(descriptors: com.maleicacid.tvinput.db.ProgramDescriptors): Any = descriptors.series?.let { series ->
        JSONObject()
            .put("seriesId", series.seriesId ?: JSONObject.NULL)
            .put("repeatLabel", series.repeatLabel)
            .put("programPattern", series.programPattern)
            .put("expireDateValid", series.expireDateValid)
            .put("expireDate", series.expireDate ?: JSONObject.NULL)
            .put("episodeNumber", series.episodeNumber ?: JSONObject.NULL)
            .put("lastEpisodeNumber", series.lastEpisodeNumber ?: JSONObject.NULL)
            .put("name", series.name ?: JSONObject.NULL)
            .put("parseStatus", series.parseStatus)
    } ?: JSONObject.NULL

    private fun publishDiagnosticsJson(program: ProgramRecord): JSONArray {
        val arr = JSONArray()
        program.descriptors.parentalRatings.filter { AribRatingMapper.toTvContentRatingString(it) == null }.forEach { rating ->
            arr.put(JSONObject()
                .put("code", "UNSUPPORTED_PARENTAL_RATING")
                .put("message", "country=${rating.countryCode} rating=${rating.ratingValue} supported=${rating.supported}")
                .put("severity", "warning"))
        }
        return arr
    }

    private fun audioLanguagesJson(descriptors: com.maleicacid.tvinput.db.ProgramDescriptors): JSONArray = JSONArray().apply {
        descriptors.components.audio.mapNotNull { entry -> entry.language?.takeIf { it.isNotBlank() }?.let { it to entry.parseStatus } }
            .distinctBy { it.first }
            .forEach { (language, parseStatus) -> put(JSONObject().put("language", language).put("source", "AUDIO_COMPONENT").put("parseStatus", parseStatus)) }
    }

    private fun audioMetadataJson(descriptors: com.maleicacid.tvinput.db.ProgramDescriptors): Any {
        val selected = descriptors.components.audio.firstOrNull { it.main == true } ?: descriptors.components.audio.firstOrNull() ?: return JSONObject.NULL
        val codec = selected.codec ?: return JSONObject.NULL
        return JSONObject()
            .put("esPid", selected.esPid.value)
            .put("componentTag", selected.componentTag ?: JSONObject.NULL)
            .put("codec", codec)
            .put("language", selected.language ?: JSONObject.NULL)
            .put("text", selected.sourceDescriptor ?: JSONObject.NULL)
            .put("parseStatus", selected.parseStatus)
    }

    private fun videoMetadataJson(program: ProgramRecord): Any {
        val selected = program.descriptors.components.video.firstOrNull() ?: return JSONObject.NULL
        val codec = selected.codec ?: return JSONObject.NULL
        return JSONObject()
            .put("esPid", selected.esPid.value)
            .put("componentTag", selected.componentTag ?: JSONObject.NULL)
            .put("codec", codec)
            .put("format", selected.sourceDescriptor ?: JSONObject.NULL)
            .put("width", selected.resolution ?: JSONObject.NULL)
            .put("height", JSONObject.NULL)
            .put("parseStatus", selected.parseStatus)
    }

    private fun optStringOrNull(obj: JSONObject, key: String): String? =
        if (obj.has(key) && !obj.isNull(key)) obj.optString(key).takeIf { it.isNotBlank() } else null

    private fun optIntOrNull(obj: JSONObject, key: String): Int? =
        if (obj.has(key) && !obj.isNull(key)) obj.optInt(key) else null

    private fun parseResult(raw: String): Result {
        val obj = runCatching { JSONObject(raw) }.getOrElse { error ->
            throw IllegalStateException("provider-data JNI result is not JSON", error)
        }
        if (!obj.optBoolean("success", false)) {
            val code = obj.optString("errorCode", "PROVIDER_DATA_FAILED")
            val message = obj.optString("errorMessage", "provider-data generation failed")
            throw IllegalStateException("$code: $message")
        }
        val json = obj.optString("bytes", "")
        require(json.isNotBlank() && json != "{}") { "provider-data JNI result did not contain valid JSON v1 bytes" }
        val signature = obj.optString("signature", "")
        require(signature.isNotBlank()) { "provider-data JNI result did not contain signature" }
        return Result(
            bytes = json.toByteArray(Charsets.UTF_8),
            signature = signature,
            schemaVersion = obj.optInt("schemaVersion", 1),
            truncated = obj.optBoolean("truncated", false),
            diagnosticsDroppedCount = obj.optInt("diagnosticsDroppedCount", 0),
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
            val obj = JSONObject().put("esPid", entry.esPid.value).put("parseStatus", entry.parseStatus)
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


}

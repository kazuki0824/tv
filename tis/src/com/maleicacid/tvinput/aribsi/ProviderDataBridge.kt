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
    data class ChannelTune(
        val deliverySystem: String,
        val frequencyHz: FrequencyHz,
        val streamSelector: StreamSelector,
        val physicalChannel: Int?,
        val satelliteBand: String?,
        val remoteControlKeyId: Int?,
    )
    data class ChannelProviderDataResult(
        val canonicalBytes: ByteArray,
        val schemaVersion: Int,
        val serviceKey: ServiceKey,
        val tune: ChannelTune,
        val requiresCas: Boolean,
    )

    private val native by lazy { NativeAribSiParser() }

    fun buildChannelProviderData(channel: ChannelRecord): Result {
        val selector = when (channel.streamSelector.type) {
            com.maleicacid.tvinput.common.StreamSelectorType.RELATIVE -> StreamSelector.tsid(channel.serviceKey.transportStreamId)
            else -> channel.streamSelector
        }
        val request = JSONObject()
            .put("schema", "maleicacid.tv.channelRequest")
            .put("schemaVersion", 1)
            .put("serviceKey", JSONObject()
                .put("originalNetworkId", channel.serviceKey.originalNetworkId)
                .put("transportStreamId", channel.serviceKey.transportStreamId)
                .put("serviceId", channel.serviceKey.serviceId))
            .put("tune", JSONObject()
                .put("deliverySystem", channel.deliverySystem)
                .put("frequencyHz", channel.frequencyHz.value)
                .put("streamId", selector.value ?: JSONObject.NULL)
                .put("streamIdType", selector.type.name)
                .put("physicalChannel", channel.physicalChannel ?: JSONObject.NULL)
                .put("satelliteBand", channel.satelliteBand ?: JSONObject.NULL)
                .put("remoteControlKeyId", channel.remoteControlKeyId ?: JSONObject.NULL))
            .put("cas", JSONObject()
                .put("requiresCas", channel.requiresCas))
            .put("diagnostics", JSONObject())
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
        runCatching { Math.addExact(program.startTimeMillis, program.durationMillis) }
            .getOrElse { error -> throw IllegalArgumentException("program timing overflow", error) }
        val request = JSONObject()
            .put("schema", "maleicacid.tv.programRequest")
            .put("schemaVersion", 1)
            .put("programKey", JSONObject()
                .put("kind", "arib-event-v1")
                .put("originalNetworkId", program.serviceKey.originalNetworkId)
                .put("transportStreamId", program.serviceKey.transportStreamId)
                .put("serviceId", program.serviceKey.serviceId)
                .put("eventId", program.eventId))
            .put("timing", JSONObject()
                .put("startUtcMillis", program.startTimeMillis)
                .put("durationMillis", program.durationMillis))
            .put("cas", JSONObject()
                .put("requiresCas", program.requiresCas)
                .put("source", "SI_SEMANTICS"))
            .put("extendedItems", toExtendedItemsArray(descriptors.extendedItems))
            .put("genres", genresJson(program))
            .put("eventGroups", toEventGroupsArray(descriptors.eventGroups))
            .put("linkage", toLinkageArray(descriptors.linkage))
            .put("freeCaMode", toFreeCaModeObject(descriptors))
            .put("series", toSeriesObject(descriptors))
            .put("diagnostics", JSONObject()
                .put("descriptorDiagnosticsCanonicalJson", descriptors.descriptorDiagnosticsCanonicalJson)
                .put("publishDiagnostics", JSONArray())
                .put("parserDiagnostics", JSONArray()))
            .put("ratings", ratingsJson(program))
            .put("components", toComponentsObject(descriptors.components))
            .put("source", JSONObject()
                .put("pid", program.source.pid.value)
                .put("tableId", program.source.tableId)
                .put("version", program.source.version)
                .put("sectionNumber", program.source.sectionNumber)
                .put("lastSectionNumber", program.source.lastSectionNumber))
            .put("malformedCaDescriptorCount", program.malformedCaDescriptorCount.coerceAtLeast(0))
        return parseResult(native.buildProgramProviderData(request.toString()))
    }

    fun normalizeProgramProviderData(providerData: ByteArray?): Result =
        parseResult(native.normalizeProgramProviderData(providerData ?: ByteArray(0)))

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

    fun decodeChannelProviderData(providerData: ByteArray?): ChannelProviderDataResult? {
        val root = runCatching {
            JSONObject(native.decodeChannelProviderData(providerData ?: ByteArray(0)))
        }.getOrNull() ?: return null
        val canonical = root.optString("canonical").takeIf { it.isNotBlank() } ?: return null
        val schemaVersion = root.optInt("schemaVersion", -1).takeIf { it == 1 } ?: return null
        val serviceKey = root.optJSONObject("serviceKey") ?: return null
        val tune = root.optJSONObject("tune") ?: return null
        val cas = root.optJSONObject("cas") ?: return null
        val onid = serviceKey.optInt("originalNetworkId", -1)
        val tsid = serviceKey.optInt("transportStreamId", -1)
        val sid = serviceKey.optInt("serviceId", -1)
        val deliverySystem = tune.optString("deliverySystem").takeIf { it.isNotBlank() } ?: return null
        val frequencyHz = FrequencyHz.fromOrNull(tune.optLong("frequencyHz", -1L))
        if (onid < 0 || tsid < 0 || sid < 0 || frequencyHz == null) return null
        return ChannelProviderDataResult(
            canonicalBytes = canonical.toByteArray(Charsets.UTF_8),
            schemaVersion = schemaVersion,
            serviceKey = ServiceKey(onid, tsid, sid),
            tune = ChannelTune(
                deliverySystem = deliverySystem,
                frequencyHz = frequencyHz,
                streamSelector = runCatching {
                    StreamSelector.fromStored(
                        tune.optString("streamIdType"),
                        if (tune.isNull("streamId")) null else tune.optInt("streamId").toString(),
                    )
                }.getOrElse { return null },
                physicalChannel = if (tune.isNull("physicalChannel")) null else tune.optInt("physicalChannel"),
                satelliteBand = if (tune.isNull("satelliteBand")) null else tune.optString("satelliteBand"),
                remoteControlKeyId = if (tune.isNull("remoteControlKeyId")) null else tune.optInt("remoteControlKeyId"),
            ),
            requiresCas = cas.optBoolean("requiresCas", false),
        )
    }

    private fun ratingsJson(program: ProgramRecord): JSONArray {
        val arr = JSONArray()
        program.descriptors.parentalRatings.forEach { rating ->
            arr.put(JSONObject()
                .put("countryCode", rating.countryCode)
                .put("rawRatingByte", rating.rawRatingByte)
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
        return Result(
            bytes = json.toByteArray(Charsets.UTF_8),
            schemaVersion = obj.optInt("schemaVersion", 1),
            truncated = obj.optBoolean("truncated", false),
            diagnosticsDroppedCount = obj.optInt("diagnosticsDroppedCount", 0),
        )
    }

    private fun toExtendedItemsArray(items: List<AribExtendedItem>): JSONArray = JSONArray().apply {
        items.forEach { item ->
            put(
                JSONObject()
                    .put("languageCode", item.languageCode)
                    .put("description", item.itemDescription)
                    .put("text", item.itemText)
                    .put("parseStatus", "OK"),
            )
        }
    }

    private fun toEventGroupsArray(groups: List<AribEventGroup>): JSONArray = JSONArray().apply {
    groups.forEach { group ->
        put(
  JSONObject()
      .put("groupType", group.groupType)
      .put("events", JSONArray().apply {
group.events.forEach { event ->
    put(JSONObject()
        .put("serviceId", event.serviceId)
        .put("eventId", event.eventId))
}
      })
      .put("otherNetworkEvents", JSONArray().apply {
group.otherNetworkEvents.forEach { event ->
    put(JSONObject()
        .put("originalNetworkId", event.originalNetworkId)
        .put("transportStreamId", event.transportStreamId)
        .put("serviceId", event.serviceId)
        .put("eventId", event.eventId))
}
      })
      .put("privateDataHex", group.privateDataHex)
      .put("parseStatus", group.parseStatus),
        )
    }
}

private fun toLinkageArray(items: List<AribLinkage>): JSONArray = JSONArray().apply {
        items.forEach { item ->
            put(JSONObject()
                .put("linkageType", item.linkageType)
                .put("originalNetworkId", item.originalNetworkId)
                .put("transportStreamId", item.transportStreamId)
                .put("serviceId", item.serviceId)
                .put("privateDataPrefixHex", item.privateDataHex)
                .put("parseStatus", item.parseStatus))
        }
    }


    fun toComponentsObject(components: AribComponents): JSONObject = JSONObject()
        .put("video", videoComponentsJson(components.video))
        .put("audio", audioComponentsJson(components.audio))
        .put("subtitle", subtitleComponentsJson(components.subtitle))
        .put("data", dataComponentsJson(components.data))

    private fun videoComponentsJson(entries: List<AribComponentEntry>): JSONArray = JSONArray().apply {
        entries.forEach { entry ->
            val obj = JSONObject()
                .put("esPid", entry.esPid.value)
                .put("streamType", requireNotNull(entry.streamType) { "video streamType is required" })
                .put("componentTag", entry.componentTag ?: JSONObject.NULL)
                .put("componentType", entry.componentType ?: JSONObject.NULL)
                .put("codec", requireNotNull(entry.codec?.takeIf { it.isNotBlank() }) { "video codec is required" })
                .put("parseStatus", entry.parseStatus)
            entry.resolution?.let { obj.put("resolution", it) }
            entry.scan?.let { obj.put("scan", it) }
            entry.aspect?.let { obj.put("aspect", it) }
            entry.profileLevel?.let { obj.put("profileLevel", it) }
            entry.sourceDescriptor?.let { obj.put("sourceDescriptor", it) }
            entry.diagnosticCode?.let { obj.put("diagnosticCode", it) }
            put(obj)
        }
    }

    private fun audioComponentsJson(entries: List<AribComponentEntry>): JSONArray = JSONArray().apply {
        entries.forEach { entry ->
            val obj = JSONObject()
                .put("esPid", entry.esPid.value)
                .put("streamType", requireNotNull(entry.streamType) { "audio streamType is required" })
                .put("componentTag", entry.componentTag ?: JSONObject.NULL)
                .put("componentType", entry.componentType ?: JSONObject.NULL)
                .put("codec", requireNotNull(entry.codec?.takeIf { it.isNotBlank() }) { "audio codec is required" })
                .put("language", entry.language?.takeIf { it.isNotBlank() } ?: JSONObject.NULL)
                .put("secondLanguage", entry.secondLanguage?.takeIf { it.isNotBlank() } ?: JSONObject.NULL)
                .put("parseStatus", entry.parseStatus)
            entry.channelConfiguration?.let { obj.put("channelConfiguration", it) }
            entry.samplingInfo?.let { obj.put("samplingInfo", it) }
            entry.sourceDescriptor?.let { obj.put("sourceDescriptor", it) }
            entry.diagnosticCode?.let { obj.put("diagnosticCode", it) }
            put(obj)
        }
    }

    private fun subtitleComponentsJson(entries: List<AribComponentEntry>): JSONArray = JSONArray().apply {
        entries.forEach { entry ->
            put(
                JSONObject()
                    .put("esPid", entry.esPid.value)
                    .put("componentTag", entry.componentTag ?: JSONObject.NULL)
                    .put("dataComponentId", entry.dataComponentId ?: JSONObject.NULL)
                    .put("language", entry.language?.takeIf { it.isNotBlank() } ?: JSONObject.NULL)
                    .put(
                        "captionServiceKind",
                        requireNotNull(entry.captionServiceKind?.takeIf { it.isNotBlank() }) {
                            "subtitle captionServiceKind is required"
                        },
                    )
                    .put("parseStatus", entry.parseStatus),
            )
        }
    }

    private fun dataComponentsJson(entries: List<AribComponentEntry>): JSONArray = JSONArray().apply {
        entries.forEach { entry ->
            put(
                JSONObject()
                    .put("esPid", entry.esPid.value)
                    .put("componentTag", entry.componentTag ?: JSONObject.NULL)
                    .put("dataComponentId", entry.dataComponentId ?: JSONObject.NULL)
                    .put("componentType", entry.componentType ?: JSONObject.NULL)
                    .put("parseStatus", entry.parseStatus),
            )
        }
    }


}

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
            .put("programKey", buildProgramKey(program))
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
            .put("extendedItems", rawArray(descriptors.extendedItemsJson))
            .put("genres", genresJson(program))
            .put("relatedItems", rawArray(descriptors.relatedItemsJson))
            .put("linkage", rawArray(descriptors.linkageJson))
            .put("freeCaMode", freeCaModeJson(descriptors))
            .put("series", seriesJson(descriptors))
            .put("diagnostics", JSONObject()
                .put("descriptorDiagnostics", rawDiagnosticsArray(descriptors.descriptorDiagnosticsJson))
                .put("publishDiagnostics", publishDiagnosticsJson(program))
                .put("parserDiagnostics", JSONArray()))
            .put("ratings", ratingsJson(program))
            .put("audioLanguages", audioLanguagesJson(descriptors))
            .put("audio", audioMetadataJson(descriptors))
            .put("video", videoMetadataJson(program))
            .put("components", rawObject(descriptors.componentsJson))
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
        val map = text.split(';').mapNotNull { part ->
            val i = part.indexOf('=')
            if (i <= 0) null else part.substring(0, i) to part.substring(i + 1)
        }.toMap()
        val onid = map["originalNetworkId"]?.toIntOrNull() ?: return null
        val tsid = map["transportStreamId"]?.toIntOrNull() ?: return null
        val sid = map["serviceId"]?.toIntOrNull() ?: return null
        val system = map["system"].orEmpty().ifBlank { return null }
        val frequencyHz = map["frequencyHz"]?.toLongOrNull() ?: return null
        return ChannelTuneKey(
            serviceKey = ServiceKey(onid, tsid, sid),
            system = system,
            frequencyHz = frequencyHz,
            streamSelector = runCatching { StreamSelector.fromStored(map["streamSelectorType"], map["streamSelectorValue"]?.takeIf { it.isNotBlank() }) }.getOrDefault(StreamSelector.NONE),
            physicalChannel = map["physicalChannel"]?.toIntOrNull(),
            backendHint = map["backendHint"]?.takeIf { it.isNotBlank() },
            satelliteBand = map["satelliteBand"]?.takeIf { it.isNotBlank() },
            remoteControlKeyId = map["remoteControlKeyId"]?.toIntOrNull(),
            requiresCas = map["requiresCas"] == "true",
            unsupportedCas = map["unsupportedCas"] == "true",
            clearLivePlaybackSupported = map["clearLivePlaybackSupported"] == "true",
            channelRegistrationReady = map["channelRegistrationReady"] == "true",
            epgPublishable = map["epgPublishable"] == "true",
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
        val genre = program.descriptors.broadcastGenre?.takeIf { it.isNotBlank() } ?: return arr
        val regex = Regex("ARIB\(0x([0-9a-fA-F]+)/0x([0-9a-fA-F]+)\):?([^、]*)")
        regex.findAll(genre).forEach { match ->
            val level1 = match.groupValues.getOrNull(1)?.toIntOrNull(16) ?: return@forEach
            val level2 = match.groupValues.getOrNull(2)?.toIntOrNull(16) ?: return@forEach
            val aribName = match.groupValues.getOrNull(3)?.takeIf { it.isNotBlank() } ?: genre
            arr.put(JSONObject()
                .put("level1", level1)
                .put("level2", level2)
                .put("userNibble", 0)
                .put("aribName", aribName)
                .put("unmappedReason", "TIS_DECIDES_CANONICAL_GENRE")
                .put("parseStatus", "OK"))
        }
        return arr
    }

    private fun freeCaModeJson(descriptors: com.maleicacid.tvinput.db.ProgramDescriptors): Any =
        rawNullableObject(descriptors.freeCaModeJson) ?: when (val scrambled = descriptors.scrambled) {
            null -> JSONObject.NULL
            else -> JSONObject()
                .put("raw", if (scrambled) 1 else 0)
                .put("scrambled", scrambled)
                .put("text", if (scrambled) "有料放送" else "無料放送")
                .put("parseStatus", "OK")
        }

    private fun seriesJson(descriptors: com.maleicacid.tvinput.db.ProgramDescriptors): Any =
        rawNullableObject(descriptors.seriesJson) ?: run {
            val seriesId = descriptors.seriesId ?: return@run JSONObject.NULL
            JSONObject()
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

    private fun parseResult(raw: String): Result {
        val obj = JSONObject(raw.ifBlank { "{}" })
        return Result(
            json = obj.optString("json", "{}"),
            signature = obj.optString("signature", ""),
            extractedKey = obj.optString("extractedKey", ""),
        )
    }

    private fun rawArray(raw: String): JSONArray = runCatching { JSONArray(raw) }.getOrElse { JSONArray() }
    private fun rawObject(raw: String): JSONObject = runCatching { JSONObject(raw) }.getOrElse { JSONObject().put("video", JSONArray()).put("audio", JSONArray()).put("subtitle", JSONArray()).put("data", JSONArray()) }
    private fun rawNullableObject(raw: String): JSONObject? = runCatching { if (raw.isBlank() || raw.trim() == "null") null else JSONObject(raw) }.getOrNull()
    private fun rawDiagnosticsArray(raw: String): JSONArray = runCatching {
        when {
            raw.isBlank() -> JSONArray()
            raw.trimStart().startsWith("[") -> JSONArray(raw)
            else -> JSONObject(raw).optJSONArray("diagnostics") ?: JSONArray()
        }
    }.getOrElse { JSONArray() }
}

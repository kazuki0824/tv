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

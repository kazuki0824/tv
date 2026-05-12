package com.maleicacid.tvinput.aribsi

import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.StreamSelector
import com.maleicacid.tvinput.db.ChannelRecord
import com.maleicacid.tvinput.db.ProgramRecord
import org.json.JSONArray
import org.json.JSONObject

object ProviderDataBridge {
    data class Result(val json: String, val signature: String, val extractedKey: String)
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
            .put("originalNetworkId", channel.serviceKey.originalNetworkId)
            .put("transportStreamId", channel.serviceKey.transportStreamId)
            .put("serviceId", channel.serviceKey.serviceId)
            .put("system", channel.deliverySystem)
            .put("frequencyHz", channel.frequencyHz)
            .put("streamSelectorType", selector.type.name)
            .put("streamSelectorValue", selector.value?.toString().orEmpty())
            .put("physicalChannel", channel.physicalChannel ?: JSONObject.NULL)
            .put("backendHint", channel.backendHint.orEmpty())
            .put("satelliteBand", channel.satelliteBand.orEmpty())
            .put("remoteControlKeyId", channel.remoteControlKeyId ?: JSONObject.NULL)
            .put("requiresCas", channel.requiresCas)
            .put("unsupportedCas", channel.unsupportedCas)
            .put("clearLivePlaybackSupported", channel.clearLivePlaybackSupported)
            .put("channelRegistrationReady", channel.channelRegistrationReady)
            .put("epgPublishable", channel.epgPublishable)
        return parseResult(native.buildChannelProviderData(request.toString()))
    }

    fun buildProgramKey(program: ProgramRecord): String =
        "onid=${program.serviceKey.originalNetworkId};tsid=${program.serviceKey.transportStreamId};sid=${program.serviceKey.serviceId};event=${program.eventId.takeIf { it >= 0 } ?: -1}"

    fun buildProgramProviderData(program: ProgramRecord): Result {
        val request = JSONObject()
            .put("programKey", buildProgramKey(program))
            .put("originalNetworkId", program.serviceKey.originalNetworkId)
            .put("transportStreamId", program.serviceKey.transportStreamId)
            .put("serviceId", program.serviceKey.serviceId)
            .put("eventId", program.eventId)
            .put("startTimeMillis", program.startTimeMillis)
            .put("durationMillis", program.durationMillis)
            .put("requiresCas", program.requiresCas)
            .put("unsupportedCas", program.unsupportedCas)
            .put("clearLivePlaybackSupported", program.clearLivePlaybackSupported)
            .put("channelRegistrationReady", program.channelRegistrationReady)
            .put("epgPublishable", program.epgPublishable)
            .put("publishStateSource", program.publishStateSource.lowercase())
            .put("extendedItems", rawArray(program.extendedItemsJson))
            .put("componentText", program.componentText.orEmpty())
            .put("audioComponentText", program.audioComponentText.orEmpty())
            .put("audioLanguage", program.audioLanguage.orEmpty())
            .put("broadcastGenre", program.broadcastGenre.orEmpty())
            .put("genreSupplementText", program.genreSupplementText.orEmpty())
            .put("eventGroupText", program.eventGroupText.orEmpty())
            .put("freeCaText", program.freeCaText.orEmpty())
            .put("seriesName", program.seriesName.orEmpty())
            .put("diagnosticText", program.diagnosticText)
            .put("descriptorDiagnostics", rawObjectOrArray(program.diagnosticDescriptorJson))
            .put("contentRatings", JSONArray(program.contentRatings.distinct().sorted()))
            .put("parentalRatingDiagnostics", rawObjectOrArray(program.parentalRatingDiagnosticsJson))
            .put("unsupportedDescriptorDiagnostics", rawObjectOrArray(program.unsupportedDescriptorJson))
            .put("videoFormat", listOfNotNull(program.videoFormat, program.videoWidth?.toString(), program.videoHeight?.toString()).joinToString("/"))
            .put("malformedCaDescriptorCount", program.malformedCaDescriptorCount.coerceAtLeast(0))
            .put("droppedRetryWindowCount", program.droppedRetryWindowCount.coerceAtLeast(0))
        return parseResult(native.buildProgramProviderData(request.toString()))
    }

    fun normalizeProgramProviderData(providerData: String?): Result =
        parseResult(native.normalizeProgramProviderData(providerData.orEmpty()))

    fun extractProgramKey(providerData: String?): String? =
        native.extractProgramKey(providerData.orEmpty()).takeIf { it.isNotBlank() }

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

    private fun parseResult(raw: String): Result {
        val obj = JSONObject(raw.ifBlank { "{}" })
        return Result(
            json = obj.optString("json", "{}"),
            signature = obj.optString("signature", ""),
            extractedKey = obj.optString("extractedKey", ""),
        )
    }

    private fun rawArray(raw: String): JSONArray = runCatching { JSONArray(raw) }.getOrElse { JSONArray() }
    private fun rawObjectOrArray(raw: String): Any = runCatching { JSONObject(raw) as Any }.getOrElse {
        runCatching { JSONArray(raw) as Any }.getOrElse {
            JSONObject()
                .put("schemaVersion", 1)
                .put("diagnostics", JSONArray())
        }
    }
}

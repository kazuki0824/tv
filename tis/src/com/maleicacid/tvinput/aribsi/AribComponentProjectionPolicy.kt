package com.maleicacid.tvinput.aribsi

internal object AribComponentProjectionPolicy {
    private val r51VideoCodecs = mapOf(0x02 to "MPEG-2", 0x1b to "H.264")
    private val recognizedVideoCodecs = r51VideoCodecs + mapOf(0x24 to "HEVC")
    private val r51AudioCodecs = mapOf(0x03 to "MPEG-Audio", 0x04 to "MPEG-Audio", 0x0f to "AAC")
    private val recognizedAudioCodecs = r51AudioCodecs + mapOf(0x11 to "MPEG-4-AAC-LATM")

    fun componentsForService(service: AribService): AribComponents {
        val video = mutableListOf<AribComponentEntry>()
        val audio = mutableListOf<AribComponentEntry>()
        val subtitle = mutableListOf<AribComponentEntry>()
        val data = mutableListOf<AribComponentEntry>()
        service.streams.forEach { stream ->
            val videoCodec = recognizedVideoCodecs[stream.streamType]
            val audioCodec = recognizedAudioCodecs[stream.streamType]
            when {
                videoCodec != null -> video += codecComponent(stream, videoCodec)
                audioCodec != null -> audio += codecComponent(stream, audioCodec).copy(
                    language = stream.languageCodes.firstOrNull(),
                    secondLanguage = stream.languageCodes.drop(1).firstOrNull(),
                )
                stream.isCaption || stream.isSuperimpose || stream.dataComponentId == 0x0012 -> subtitle += AribComponentEntry(
                    esPid = stream.elementaryPid,
                    componentTag = stream.componentTag,
                    dataComponentId = stream.dataComponentId,
                    captionDmf = stream.captionDmf,
                    captionTiming = stream.captionTiming,
                    automaticPresentationOnReception = stream.automaticPresentationOnReception,
                    language = null,
                    captionServiceKind = when {
                        stream.isSuperimpose -> "superimpose"
                        stream.dataComponentId == 0x0012 -> "one-seg-caption"
                        else -> "caption"
                    },
                )
                stream.dataComponentId != null -> data += AribComponentEntry(
                    esPid = stream.elementaryPid,
                    componentTag = stream.componentTag,
                    dataComponentId = stream.dataComponentId,
                    componentType = stream.componentType,
                )
            }
        }
        return AribComponents(video = video, audio = audio, subtitle = subtitle, data = data)
    }

    fun mergeEventAndServiceComponents(
        eventComponents: AribComponents,
        serviceComponents: AribComponents,
    ): AribComponents = AribComponents(
        video = mergeComponentEntries(eventComponents.video, serviceComponents.video),
        audio = mergeComponentEntries(eventComponents.audio, serviceComponents.audio),
        subtitle = serviceComponents.subtitle + eventComponents.subtitle.filterNot { event ->
            serviceComponents.subtitle.any { sameComponentIdentity(it, event) }
        },
        data = serviceComponents.data + eventComponents.data.filterNot { event ->
            serviceComponents.data.any { sameComponentIdentity(it, event) }
        },
    )

    fun toComponentsObjectForService(service: AribService): String =
        ProviderDataBridge.toComponentsObject(componentsForService(service)).toString()

    fun isR51PlaybackSupportedVideoCodec(streamType: Int): Boolean = r51VideoCodecs.containsKey(streamType)

    fun isRecognizedVideoCodec(streamType: Int): Boolean = recognizedVideoCodecs.containsKey(streamType)

    fun isR51PlaybackSupportedAudioCodec(streamType: Int): Boolean = r51AudioCodecs.containsKey(streamType)

    fun isRecognizedAudioCodec(streamType: Int): Boolean = recognizedAudioCodecs.containsKey(streamType)

    private fun codecComponent(stream: AribElementaryStream, codec: String): AribComponentEntry = AribComponentEntry(
        esPid = stream.elementaryPid,
        streamType = stream.streamType,
        componentTag = stream.componentTag,
        componentType = stream.componentType,
        codec = codec,
        parseStatus = "OK",
    )

    private fun mergeComponentEntries(
        eventEntries: List<AribComponentEntry>,
        serviceEntries: List<AribComponentEntry>,
    ): List<AribComponentEntry> {
        if (eventEntries.isEmpty()) return serviceEntries
        val merged = serviceEntries.toMutableList()
        eventEntries.forEach { eventEntry ->
            val index = merged.indexOfFirst { sameComponentIdentity(it, eventEntry) }
            if (index >= 0) {
                merged[index] = mergeComponentEntry(eventEntry, merged[index])
            } else {
                merged += eventEntry
            }
        }
        return merged
    }

    private fun sameComponentIdentity(left: AribComponentEntry, right: AribComponentEntry): Boolean {
        val leftTag = left.componentTag
        val rightTag = right.componentTag
        return leftTag != null && rightTag != null && leftTag == rightTag
    }

    private fun mergeComponentEntry(
        eventEntry: AribComponentEntry,
        serviceEntry: AribComponentEntry,
    ): AribComponentEntry = serviceEntry.copy(
        componentType = eventEntry.componentType ?: serviceEntry.componentType,
        language = eventEntry.language ?: serviceEntry.language,
        secondLanguage = eventEntry.secondLanguage ?: serviceEntry.secondLanguage,
        channelConfiguration = eventEntry.channelConfiguration ?: serviceEntry.channelConfiguration,
        samplingInfo = eventEntry.samplingInfo ?: serviceEntry.samplingInfo,
        sourceDescriptor = eventEntry.sourceDescriptor ?: serviceEntry.sourceDescriptor,
        resolution = eventEntry.resolution ?: serviceEntry.resolution,
        scan = eventEntry.scan ?: serviceEntry.scan,
        aspect = eventEntry.aspect ?: serviceEntry.aspect,
        profileLevel = eventEntry.profileLevel ?: serviceEntry.profileLevel,
        main = eventEntry.main ?: serviceEntry.main,
        multiLingual = eventEntry.multiLingual ?: serviceEntry.multiLingual,
        qualityIndicator = eventEntry.qualityIndicator ?: serviceEntry.qualityIndicator,
        parseStatus = if (eventEntry.parseStatus != "OK") eventEntry.parseStatus else serviceEntry.parseStatus,
    )
}

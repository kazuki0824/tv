package com.maleicacid.tvinput.aribsi

class NativeAribSiParser : AutoCloseable {
    private var handle: Long = nativeCreate()

    fun ingestSection(pid: Int, section: ByteArray): Int {
        check(handle != 0L) { "ネイティブ解析器は終了済みです" }
        return nativeIngestSection(handle, pid, section)
    }

    fun lastStatus(): Int = nativeLastStatus(handle)

    fun discoveryStage(): Int = nativeGetDiscoveryStage(handle)

    fun isDiscoveryComplete(): Boolean = discoveryStage() == SiDiscoveryStage.COMPLETE

    fun snapshotPmtPids(): List<PmtPidMapping> {
        check(handle != 0L) { "ネイティブ解析器は終了済みです" }
        val count = nativeGetPmtPidMappingCount(handle).coerceAtLeast(0)
        return (0 until count).mapNotNull { index ->
            val onId = nativeGetPmtPidMappingOriginalNetworkId(handle, index)
            val tsId = nativeGetPmtPidMappingTransportStreamId(handle, index)
            val serviceId = nativeGetPmtPidMappingServiceId(handle, index)
            val pmtPid = nativeGetPmtPidMappingPmtPid(handle, index)
            if (onId < 0 || tsId < 0 || serviceId < 0 || pmtPid !in 0..0x1fff) null else PmtPidMapping(
                serviceKey = com.maleicacid.tvinput.common.ServiceKey(onId, tsId, serviceId),
                pmtPid = pmtPid,
            )
        }
    }

    fun snapshotPublishabilityDiagnostics(): List<ServicePublishabilityDiagnostic> {
        check(handle != 0L) { "ネイティブ解析器は終了済みです" }
        val count = nativeGetPublishabilityCount(handle).coerceAtLeast(0)
        return (0 until count).mapNotNull { index ->
            val onId = nativeGetPublishabilityOriginalNetworkId(handle, index)
            val tsId = nativeGetPublishabilityTransportStreamId(handle, index)
            val serviceId = nativeGetPublishabilityServiceId(handle, index)
            if (onId < 0 || tsId < 0 || serviceId < 0) null else ServicePublishabilityDiagnostic(
                serviceKey = com.maleicacid.tvinput.common.ServiceKey(onId, tsId, serviceId),
                publishable = nativeGetPublishabilityIsPublishable(handle, index) == 1,
                missingComponents = nativeGetPublishabilityMissingComponents(handle, index)
                    .split(',')
                    .filter { it.isNotBlank() },
            )
        }
    }

    fun snapshotServices(): List<AribService> {
        check(handle != 0L) { "ネイティブ解析器は終了済みです" }
        val count = nativeGetServiceCount(handle).coerceAtLeast(0)
        return (0 until count).mapNotNull { serviceIndex ->
            val serviceId = nativeGetServiceId(handle, serviceIndex)
            val tsId = nativeGetTransportStreamId(handle, serviceIndex)
            val onId = nativeGetOriginalNetworkId(handle, serviceIndex)
            if (serviceId < 0 || tsId < 0 || onId < 0) {
                null
            } else {
                AribService(
                    serviceKey = com.maleicacid.tvinput.common.ServiceKey(onId, tsId, serviceId),
                    name = nativeGetServiceName(handle, serviceIndex),
                    providerName = nativeGetProviderName(handle, serviceIndex),
                    serviceType = nativeGetServiceType(handle, serviceIndex).takeIf { it >= 0 },
                    pmtPid = nativeGetPmtPid(handle, serviceIndex).takeIf { it >= 0 },
                    pcrPid = nativeGetPcrPid(handle, serviceIndex).takeIf { it >= 0 },
                    freeCaMode = nativeGetFreeCaMode(handle, serviceIndex).takeIf { it >= 0 }?.let { it != 0 },
                    streams = snapshotElementaryStreams(serviceIndex),
                )
            }
        }
    }

    private fun snapshotElementaryStreams(serviceIndex: Int): List<AribElementaryStream> {
        val count = nativeGetServiceEsCount(handle, serviceIndex).coerceAtLeast(0)
        return (0 until count).mapNotNull { esIndex ->
            val pid = nativeGetServiceEsElementaryPid(handle, serviceIndex, esIndex)
            val streamType = nativeGetServiceEsStreamType(handle, serviceIndex, esIndex)
            if (pid < 0 || streamType < 0) {
                null
            } else {
                val languageCount = nativeGetServiceEsLanguageCount(handle, serviceIndex, esIndex).coerceAtLeast(0)
                AribElementaryStream(
                    elementaryPid = pid,
                    streamType = streamType,
                    componentTag = nativeGetServiceEsComponentTag(handle, serviceIndex, esIndex).takeIf { it >= 0 },
                    componentType = nativeGetServiceEsComponentType(handle, serviceIndex, esIndex).takeIf { it >= 0 },
                    streamContent = nativeGetServiceEsStreamContent(handle, serviceIndex, esIndex).takeIf { it >= 0 },
                    languageCodes = (0 until languageCount).map { langIndex ->
                        nativeGetServiceEsLanguage(handle, serviceIndex, esIndex, langIndex)
                    },
                )
            }
        }
    }

    fun snapshotTransports(): List<AribTransport> {
        check(handle != 0L) { "ネイティブ解析器は終了済みです" }
        val count = nativeGetTransportCount(handle).coerceAtLeast(0)
        return (0 until count).mapNotNull { index ->
            val tsId = nativeGetTransportStreamIdByIndex(handle, index)
            val onId = nativeGetTransportOriginalNetworkIdByIndex(handle, index)
            if (tsId < 0 || onId < 0) {
                null
            } else {
                AribTransport(
                    originalNetworkId = onId,
                    transportStreamId = tsId,
                    networkName = nativeGetNetworkName(handle, index),
                    transportStreamName = nativeGetTransportStreamName(handle, index),
                    remoteControlKeyId = nativeGetRemoteControlKeyId(handle, index).takeIf { it >= 0 },
                )
            }
        }
    }

    fun snapshotCaMetadata(): List<CaMetadata> {
        check(handle != 0L) { "ネイティブ解析器は終了済みです" }
        val services = snapshotServices()
        val programCa = services.flatMapIndexed { serviceIndex, service ->
            val count = nativeGetServiceProgramCaCount(handle, serviceIndex).coerceAtLeast(0)
            (0 until count).mapNotNull { caIndex ->
                val systemId = nativeGetServiceProgramCaSystemId(handle, serviceIndex, caIndex)
                val ecmPid = nativeGetServiceProgramCaPid(handle, serviceIndex, caIndex)
                if (systemId < 0 || ecmPid < 0) null else CaMetadata(
                    serviceKey = service.serviceKey,
                    caSystemId = systemId,
                    ecmPid = ecmPid,
                    emmPid = null,
                    elementaryPid = null,
                    privateData = nativeGetServiceProgramCaPrivateData(handle, serviceIndex, caIndex),
                    source = CaMetadataSource.PROGRAM,
                )
            }
        }
        val esCa = services.flatMapIndexed { serviceIndex, service ->
            val groupCount = nativeGetServiceEsCaCount(handle, serviceIndex).coerceAtLeast(0)
            (0 until groupCount).flatMap { esCaIndex ->
                val elementaryPid = nativeGetServiceEsCaElementaryPid(handle, serviceIndex, esCaIndex)
                val descriptorCount = nativeGetServiceEsCaDescriptorCount(handle, serviceIndex, esCaIndex).coerceAtLeast(0)
                (0 until descriptorCount).mapNotNull { caIndex ->
                    val systemId = nativeGetServiceEsCaSystemId(handle, serviceIndex, esCaIndex, caIndex)
                    val ecmPid = nativeGetServiceEsCaPid(handle, serviceIndex, esCaIndex, caIndex)
                    if (systemId < 0 || ecmPid < 0 || elementaryPid < 0) null else CaMetadata(
                        serviceKey = service.serviceKey,
                        caSystemId = systemId,
                        ecmPid = ecmPid,
                        emmPid = null,
                        elementaryPid = elementaryPid,
                        privateData = nativeGetServiceEsCaPrivateData(handle, serviceIndex, esCaIndex, caIndex),
                        source = CaMetadataSource.ELEMENTARY_STREAM,
                    )
                }
            }
        }
        val catCa = (0 until nativeGetCatCaCount(handle).coerceAtLeast(0)).mapNotNull { caIndex ->
            val systemId = nativeGetCatCaSystemId(handle, caIndex)
            val emmPid = nativeGetCatCaPid(handle, caIndex)
            if (systemId < 0 || emmPid < 0) null else CaMetadata(
                serviceKey = null,
                caSystemId = systemId,
                ecmPid = null,
                emmPid = emmPid,
                elementaryPid = null,
                privateData = nativeGetCatCaPrivateData(handle, caIndex),
                source = CaMetadataSource.CAT,
            )
        }
        return programCa + esCa + catCa
    }

    fun snapshotPrivateSections(): List<PrivateSection> {
        check(handle != 0L) { "ネイティブ解析器は終了済みです" }
        val count = nativeGetPrivateSectionCount(handle).coerceAtLeast(0)
        return (0 until count).mapNotNull { index ->
            val pid = nativeGetPrivateSectionPid(handle, index)
            val tableId = nativeGetPrivateSectionTableId(handle, index)
            if (pid < 0 || tableId < 0) null else PrivateSection(
                pid = pid,
                tableId = tableId,
                bytes = nativeGetPrivateSectionBytes(handle, index),
            )
        }
    }

    fun snapshotEvents(): List<AribEvent> {
        check(handle != 0L) { "ネイティブ解析器は終了済みです" }
        val count = nativeGetEventCount(handle).coerceAtLeast(0)
        return (0 until count).mapNotNull { index ->
            val serviceId = nativeGetEventServiceId(handle, index)
            val tsId = nativeGetEventTransportStreamId(handle, index)
            val onId = nativeGetEventOriginalNetworkId(handle, index)
            val eventId = nativeGetEventId(handle, index)
            val stableIdentity = nativeGetEventStableIdentity(handle, index)
            val start = nativeGetEventStartTimeMillis(handle, index)
            val duration = nativeGetEventDurationMillis(handle, index)
            if (serviceId < 0 || tsId < 0 || onId < 0 || eventId < 0 || start <= 0L || duration <= 0L) null else AribEvent(
                serviceKey = com.maleicacid.tvinput.common.ServiceKey(onId, tsId, serviceId),
                stableIdentity = stableIdentity,
                eventId = eventId,
                startTimeMillis = start,
                durationMillis = duration,
                title = nativeGetEventTitle(handle, index),
                description = nativeGetEventDescription(handle, index),
                extendedDescription = nativeGetEventExtendedDescription(handle, index),
                eventScope = nativeGetEventScope(handle, index),
                extendedItems = parseExtendedItemsJson(nativeGetEventExtendedItemsJson(handle, index)),
                componentText = nativeGetEventComponentText(handle, index).takeIf { it.isNotBlank() },
                audioComponentText = nativeGetEventAudioComponentText(handle, index).takeIf { it.isNotBlank() },
                audioLanguage = nativeGetEventAudioLanguage(handle, index).takeIf { it.isNotBlank() },
                canonicalGenre = nativeGetEventCanonicalGenre(handle, index).takeIf { it.isNotBlank() },
                genreSupplementText = nativeGetEventGenreSupplementText(handle, index).takeIf { it.isNotBlank() },
                eventGroupText = nativeGetEventGroupText(handle, index).takeIf { it.isNotBlank() },
                freeCaText = nativeGetEventFreeCaText(handle, index).takeIf { it.isNotBlank() },
                seriesName = nativeGetEventSeriesName(handle, index).takeIf { it.isNotBlank() },
                diagnosticText = nativeGetEventDiagnosticText(handle, index),
                diagnosticDescriptorJson = nativeGetEventDiagnosticDescriptorJson(handle, index),
                textDiagnostics = parseTextDiagnosticSummary(nativeGetEventDiagnosticText(handle, index)),
            )
        }
    }


    private fun parseExtendedItemsJson(raw: String): List<AribExtendedItem> {
        if (raw.isBlank() || raw == "[]") return emptyList()
        val pattern = Regex("""\{"description":"(.*?)","text":"(.*?)"\}""")
        return pattern.findAll(raw).map { match ->
            AribExtendedItem(
                itemDescription = unescapeJsonFragment(match.groupValues[1]),
                itemText = unescapeJsonFragment(match.groupValues[2]),
            )
        }.toList()
    }

    private fun parseTextDiagnosticSummary(raw: String): List<String> = raw
        .split(' ', '\n')
        .filter { it.contains("unknownCount=") || it.contains("json=") || it.contains("component=") || it.contains("audio=") }

    private fun unescapeJsonFragment(value: String): String = value
        .replace("\\n", "\n")
        .replace("\\\"", "\"")
        .replace("\\\\", "\\")

    fun decodeAribString(bytes: ByteArray): String = nativeDecodeAribString(bytes)

    fun decodeAribStringDiagnosticSummary(bytes: ByteArray): String = nativeDecodeAribStringDiagnosticSummary(bytes)

    override fun close() {
        val current = handle
        if (current != 0L) {
            nativeDestroy(current)
            handle = 0L
        }
    }

    private external fun nativeCreate(): Long
    private external fun nativeDestroy(handle: Long): Int
    private external fun nativeIngestSection(handle: Long, pid: Int, section: ByteArray): Int
    private external fun nativeLastStatus(handle: Long): Int
    private external fun nativeGetDiscoveryStage(handle: Long): Int

    private external fun nativeGetPublishabilityCount(handle: Long): Int
    private external fun nativeGetPublishabilityOriginalNetworkId(handle: Long, index: Int): Int
    private external fun nativeGetPublishabilityTransportStreamId(handle: Long, index: Int): Int
    private external fun nativeGetPublishabilityServiceId(handle: Long, index: Int): Int
    private external fun nativeGetPublishabilityIsPublishable(handle: Long, index: Int): Int
    private external fun nativeGetPublishabilityMissingComponents(handle: Long, index: Int): String

    private external fun nativeGetPmtPidMappingCount(handle: Long): Int
    private external fun nativeGetPmtPidMappingOriginalNetworkId(handle: Long, index: Int): Int
    private external fun nativeGetPmtPidMappingTransportStreamId(handle: Long, index: Int): Int
    private external fun nativeGetPmtPidMappingServiceId(handle: Long, index: Int): Int
    private external fun nativeGetPmtPidMappingPmtPid(handle: Long, index: Int): Int


    private external fun nativeGetServiceCount(handle: Long): Int
    private external fun nativeGetServiceId(handle: Long, index: Int): Int
    private external fun nativeGetTransportStreamId(handle: Long, index: Int): Int
    private external fun nativeGetOriginalNetworkId(handle: Long, index: Int): Int
    private external fun nativeGetServiceName(handle: Long, index: Int): String
    private external fun nativeGetProviderName(handle: Long, index: Int): String
    private external fun nativeGetServiceType(handle: Long, index: Int): Int
    private external fun nativeGetPmtPid(handle: Long, index: Int): Int
    private external fun nativeGetPcrPid(handle: Long, index: Int): Int
    private external fun nativeGetFreeCaMode(handle: Long, index: Int): Int

    private external fun nativeGetServiceEsCount(handle: Long, serviceIndex: Int): Int
    private external fun nativeGetServiceEsElementaryPid(handle: Long, serviceIndex: Int, esIndex: Int): Int
    private external fun nativeGetServiceEsStreamType(handle: Long, serviceIndex: Int, esIndex: Int): Int
    private external fun nativeGetServiceEsComponentTag(handle: Long, serviceIndex: Int, esIndex: Int): Int
    private external fun nativeGetServiceEsComponentType(handle: Long, serviceIndex: Int, esIndex: Int): Int
    private external fun nativeGetServiceEsStreamContent(handle: Long, serviceIndex: Int, esIndex: Int): Int
    private external fun nativeGetServiceEsLanguageCount(handle: Long, serviceIndex: Int, esIndex: Int): Int
    private external fun nativeGetServiceEsLanguage(handle: Long, serviceIndex: Int, esIndex: Int, langIndex: Int): String

    private external fun nativeGetServiceProgramCaCount(handle: Long, serviceIndex: Int): Int
    private external fun nativeGetServiceProgramCaSystemId(handle: Long, serviceIndex: Int, caIndex: Int): Int
    private external fun nativeGetServiceProgramCaPid(handle: Long, serviceIndex: Int, caIndex: Int): Int
    private external fun nativeGetServiceProgramCaPrivateData(handle: Long, serviceIndex: Int, caIndex: Int): ByteArray

    private external fun nativeGetServiceEsCaCount(handle: Long, serviceIndex: Int): Int
    private external fun nativeGetServiceEsCaElementaryPid(handle: Long, serviceIndex: Int, esCaIndex: Int): Int
    private external fun nativeGetServiceEsCaDescriptorCount(handle: Long, serviceIndex: Int, esCaIndex: Int): Int
    private external fun nativeGetServiceEsCaSystemId(handle: Long, serviceIndex: Int, esCaIndex: Int, caIndex: Int): Int
    private external fun nativeGetServiceEsCaPid(handle: Long, serviceIndex: Int, esCaIndex: Int, caIndex: Int): Int
    private external fun nativeGetServiceEsCaPrivateData(handle: Long, serviceIndex: Int, esCaIndex: Int, caIndex: Int): ByteArray

    private external fun nativeGetCatCaCount(handle: Long): Int
    private external fun nativeGetCatCaSystemId(handle: Long, caIndex: Int): Int
    private external fun nativeGetCatCaPid(handle: Long, caIndex: Int): Int
    private external fun nativeGetCatCaPrivateData(handle: Long, caIndex: Int): ByteArray

    private external fun nativeGetPrivateSectionCount(handle: Long): Int
    private external fun nativeGetPrivateSectionPid(handle: Long, index: Int): Int
    private external fun nativeGetPrivateSectionTableId(handle: Long, index: Int): Int
    private external fun nativeGetPrivateSectionBytes(handle: Long, index: Int): ByteArray

    private external fun nativeGetTransportCount(handle: Long): Int
    private external fun nativeGetTransportStreamIdByIndex(handle: Long, index: Int): Int
    private external fun nativeGetTransportOriginalNetworkIdByIndex(handle: Long, index: Int): Int
    private external fun nativeGetNetworkName(handle: Long, index: Int): String
    private external fun nativeGetTransportStreamName(handle: Long, index: Int): String
    private external fun nativeGetRemoteControlKeyId(handle: Long, index: Int): Int

    private external fun nativeDecodeAribString(bytes: ByteArray): String
    private external fun nativeDecodeAribStringDiagnosticSummary(bytes: ByteArray): String
    private external fun nativeGetEventCount(handle: Long): Int
    private external fun nativeGetEventServiceId(handle: Long, index: Int): Int
    private external fun nativeGetEventTransportStreamId(handle: Long, index: Int): Int
    private external fun nativeGetEventOriginalNetworkId(handle: Long, index: Int): Int
    private external fun nativeGetEventId(handle: Long, index: Int): Int
    private external fun nativeGetEventStableIdentity(handle: Long, index: Int): String
    private external fun nativeGetEventStartTimeMillis(handle: Long, index: Int): Long
    private external fun nativeGetEventDurationMillis(handle: Long, index: Int): Long
    private external fun nativeGetEventTitle(handle: Long, index: Int): String
    private external fun nativeGetEventDescription(handle: Long, index: Int): String
    private external fun nativeGetEventExtendedDescription(handle: Long, index: Int): String
    private external fun nativeGetEventExtendedItemsJson(handle: Long, index: Int): String
    private external fun nativeGetEventComponentText(handle: Long, index: Int): String
    private external fun nativeGetEventAudioComponentText(handle: Long, index: Int): String
    private external fun nativeGetEventAudioLanguage(handle: Long, index: Int): String
    private external fun nativeGetEventCanonicalGenre(handle: Long, index: Int): String
    private external fun nativeGetEventGenreSupplementText(handle: Long, index: Int): String
    private external fun nativeGetEventGroupText(handle: Long, index: Int): String
    private external fun nativeGetEventFreeCaText(handle: Long, index: Int): String
    private external fun nativeGetEventSeriesName(handle: Long, index: Int): String
    private external fun nativeGetEventDiagnosticDescriptorJson(handle: Long, index: Int): String
    private external fun nativeGetEventScope(handle: Long, index: Int): String
    private external fun nativeGetEventDiagnosticText(handle: Long, index: Int): String

    companion object {
        init {
            System.loadLibrary("maleicacid_arib_si_engine_jni")
        }
    }
}

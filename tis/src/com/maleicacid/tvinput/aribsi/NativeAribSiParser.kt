package com.maleicacid.tvinput.aribsi

import android.util.Log
import com.maleicacid.tvinput.common.LogTags
import org.json.JSONArray
import org.json.JSONObject

class NativeAribSiParser : AutoCloseable {
    data class BulkSnapshot(
        val services: List<AribService>,
        val servicesForCasDiscovery: List<AribService>,
        val caMetadata: List<CaMetadata>,
        val caMetadataForCasDiscovery: List<CaMetadata>,
        val pmtPidMappings: List<PmtPidMapping>,
        val pmtPidsForSectionFilters: List<Int>,
        val transports: List<AribTransport>,
        val sdtActualTransports: List<AribTransport>,
        val privateSections: List<PrivateSection>,
        val events: List<AribEvent>,
        val epgUpdateWindows: List<AribEpgUpdateWindow>,
        val publishabilityDiagnostics: List<ServicePublishabilityDiagnostic>,
    )

    private var handle: Long = nativeCreate()


    fun buildChannelProviderData(requestJson: String): String = nativeBuildChannelProviderData(requestJson)
    fun buildProgramProviderData(requestJson: String): String = nativeBuildProgramProviderData(requestJson)
    fun normalizeProgramProviderData(providerData: String): String = nativeNormalizeProgramProviderData(providerData)
    fun extractProgramKey(providerData: String): String = nativeExtractProgramKey(providerData)
    fun extractChannelTuneKey(providerData: String): String = nativeExtractChannelTuneKey(providerData)
    fun appendCurrentProgramDiagnostics(providerData: String, overlapCount: Long, selectedProgramId: Long, selectionRule: String): String =
        nativeAppendCurrentProgramDiagnostics(providerData, overlapCount, selectedProgramId, selectionRule)

    fun ingestSection(pid: Int, section: ByteArray): Int {
        check(handle != 0L) { "ネイティブ解析器は終了済みです" }
        return nativeIngestSection(handle, pid, section)
    }

    fun lastStatus(): Int = nativeLastStatus(handle)

    fun discoveryStage(): Int = nativeGetDiscoveryStage(handle)

    fun isDiscoveryComplete(): Boolean = discoveryStage() == SiDiscoveryStage.COMPLETE


    @Synchronized
    fun snapshotBulk(): BulkSnapshot = snapshotBulk(takeUpdateWindows = false)

    @Synchronized
    fun snapshotBulk(takeUpdateWindows: Boolean): BulkSnapshot {
        check(handle != 0L) { "ネイティブ解析器は終了済みです" }
        // B-10/N-10: production snapshot は nativeSnapshotBulkJson() 1回に限定する。
        // update windows を同一 transaction で drain する call-site では
        // nativeSnapshotBulkJson(handle, 1) をこの wrapper から使う。
        return parseBulkSnapshotJson(nativeSnapshotBulkJson(handle, if (takeUpdateWindows) 1 else 0))
    }

    private fun parseBulkSnapshotJson(raw: String): BulkSnapshot {
        val root = JSONObject(raw.ifBlank { "{}" })
        return BulkSnapshot(
            services = parseServices(root.optJSONArray("services")),
            servicesForCasDiscovery = parseServices(root.optJSONArray("servicesForCasDiscovery")),
            caMetadata = parseCaMetadataList(root.optJSONArray("caMetadata")),
            caMetadataForCasDiscovery = parseCaMetadataList(root.optJSONArray("caMetadataForCasDiscovery")),
            pmtPidMappings = parsePmtPidMappings(root.optJSONArray("pmtPidMappings")),
            pmtPidsForSectionFilters = parseIntArray(root.optJSONArray("pmtPidsForSectionFilters")).filter { it in 0..0x1fff },
            transports = parseTransports(root.optJSONArray("transports")),
            sdtActualTransports = parseTransports(root.optJSONArray("sdtActualTransports")),
            privateSections = parsePrivateSections(root.optJSONArray("privateSections")),
            events = parseEvents(root.optJSONArray("events")),
            epgUpdateWindows = parseEpgUpdateWindows(root.optJSONArray("epgUpdateWindows")),
            publishabilityDiagnostics = parsePublishabilityDiagnostics(root.optJSONArray("publishabilityDiagnostics")),
        )
    }

    private fun parseIntArray(array: JSONArray?): List<Int> = (0 until (array?.length() ?: 0)).map { index -> array!!.optInt(index) }
    private fun parseStringArray(array: JSONArray?): List<String> = (0 until (array?.length() ?: 0)).mapNotNull { index -> array!!.optString(index).takeIf { it.isNotBlank() } }
    private fun optIntOrNull(obj: JSONObject, key: String): Int? = if (obj.isNull(key)) null else obj.optInt(key)
    private fun optStringOrNull(obj: JSONObject, key: String): String? = obj.optString(key).takeIf { it.isNotBlank() }
    private fun hexToBytes(hex: String): ByteArray {
        if (hex.length % 2 != 0) return ByteArray(0)
        return ByteArray(hex.length / 2) { i -> hex.substring(i * 2, i * 2 + 2).toIntOrNull(16)?.toByte() ?: 0 }
    }

    private fun serviceKeyFrom(obj: JSONObject): com.maleicacid.tvinput.common.ServiceKey = com.maleicacid.tvinput.common.ServiceKey(
        originalNetworkId = obj.optInt("originalNetworkId", -1),
        transportStreamId = obj.optInt("transportStreamId", -1),
        serviceId = obj.optInt("serviceId", -1),
    )

    private fun parseServices(array: JSONArray?): List<AribService> = (0 until (array?.length() ?: 0)).mapNotNull { index ->
        val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
        val key = serviceKeyFrom(obj)
        if (key.originalNetworkId < 0 || key.transportStreamId < 0 || key.serviceId < 0) return@mapNotNull null
        AribService(
            serviceKey = key,
            name = obj.optString("name"),
            providerName = obj.optString("providerName"),
            serviceType = optIntOrNull(obj, "serviceType"),
            pmtPid = optIntOrNull(obj, "pmtPid"),
            pcrPid = optIntOrNull(obj, "pcrPid"),
            freeCaMode = if (obj.isNull("freeCaMode")) null else obj.optBoolean("freeCaMode"),
            streams = parseStreams(obj.optJSONArray("streams")),
            hasProgramCaDescriptor = obj.optBoolean("hasProgramCaDescriptor"),
            hasEsCaDescriptor = obj.optBoolean("hasEsCaDescriptor"),
            serviceScopedCaDescriptors = parseCaDescriptors(obj.optJSONArray("serviceScopedCaDescriptors")),
        )
    }

    private fun parseStreams(array: JSONArray?): List<AribElementaryStream> = (0 until (array?.length() ?: 0)).mapNotNull { index ->
        val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
        val pid = obj.optInt("elementaryPid", -1)
        val streamType = obj.optInt("streamType", -1)
        if (pid < 0 || streamType < 0) null else AribElementaryStream(
            elementaryPid = pid,
            streamType = streamType,
            componentTag = optIntOrNull(obj, "componentTag"),
            componentType = optIntOrNull(obj, "componentType"),
            streamContent = optIntOrNull(obj, "streamContent"),
            languageCodes = parseStringArray(obj.optJSONArray("languageCodes")),
        )
    }

    private fun parseCaDescriptors(array: JSONArray?): List<CaDescriptor> = (0 until (array?.length() ?: 0)).mapNotNull { index ->
        val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
        val systemId = obj.optInt("caSystemId", -1)
        if (systemId < 0) null else CaDescriptor(
            caSystemId = systemId,
            caPid = optIntOrNull(obj, "caPid"),
            scope = runCatching { CaDescriptorScope.valueOf(obj.optString("scope", "PROGRAM")) }.getOrDefault(CaDescriptorScope.PROGRAM),
            esPid = optIntOrNull(obj, "esPid"),
            rawDescriptor = hexToBytes(obj.optString("rawDescriptorHex")),
        )
    }

    private fun parseTransports(array: JSONArray?): List<AribTransport> = (0 until (array?.length() ?: 0)).mapNotNull { index ->
        val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
        val onid = obj.optInt("originalNetworkId", -1)
        val tsid = obj.optInt("transportStreamId", -1)
        if (onid < 0 || tsid < 0) null else AribTransport(
            originalNetworkId = onid,
            transportStreamId = tsid,
            networkName = obj.optString("networkName"),
            transportStreamName = obj.optString("transportStreamName"),
            remoteControlKeyId = optIntOrNull(obj, "remoteControlKeyId"),
        )
    }

    private fun parsePmtPidMappings(array: JSONArray?): List<PmtPidMapping> = (0 until (array?.length() ?: 0)).mapNotNull { index ->
        val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
        val key = serviceKeyFrom(obj)
        val pid = obj.optInt("pmtPid", -1)
        if (key.originalNetworkId < 0 || key.transportStreamId < 0 || key.serviceId < 0 || pid !in 0..0x1fff) null else PmtPidMapping(key, pid)
    }

    private fun parseCaMetadataList(array: JSONArray?): List<CaMetadata> = (0 until (array?.length() ?: 0)).mapNotNull { index ->
        val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
        val keyObj = obj.optJSONObject("serviceKey")
        val serviceKey = keyObj?.let { serviceKeyFrom(it) }?.takeIf { it.originalNetworkId >= 0 && it.transportStreamId >= 0 && it.serviceId >= 0 }
        val systemId = obj.optInt("caSystemId", -1)
        if (systemId < 0) null else CaMetadata(
            serviceKey = serviceKey,
            caSystemId = systemId,
            ecmPid = optIntOrNull(obj, "ecmPid"),
            emmPid = optIntOrNull(obj, "emmPid"),
            elementaryPid = optIntOrNull(obj, "elementaryPid"),
            privateData = hexToBytes(obj.optString("privateDataHex")),
            source = runCatching { CaMetadataSource.valueOf(obj.optString("source")) }.getOrDefault(CaMetadataSource.PROGRAM),
        )
    }

    private fun parsePrivateSections(array: JSONArray?): List<PrivateSection> = (0 until (array?.length() ?: 0)).mapNotNull { index ->
        val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
        val pid = obj.optInt("pid", -1)
        val tableId = obj.optInt("tableId", -1)
        if (pid < 0 || tableId < 0) null else PrivateSection(pid, tableId, hexToBytes(obj.optString("bytesHex")))
    }

    private fun parseEvents(array: JSONArray?): List<AribEvent> = (0 until (array?.length() ?: 0)).mapNotNull { index ->
        val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
        val key = serviceKeyFrom(obj)
        val eventId = obj.optInt("eventId", -1)
        val start = obj.optLong("startTimeMillis", -1L)
        val duration = obj.optLong("durationMillis", -1L)
        if (key.originalNetworkId < 0 || key.transportStreamId < 0 || key.serviceId < 0 || eventId < 0 || start <= 0L || duration <= 0L) null else AribEvent(
            serviceKey = key,
            stableIdentity = obj.optString("stableIdentity"),
            eventId = eventId,
            startTimeMillis = start,
            durationMillis = duration,
            title = obj.optString("title"),
            description = obj.optString("description"),
            extendedDescription = obj.optString("extendedDescription"),
            eventScope = obj.optString("eventScope", "present_following"),
            extendedItems = parseExtendedItems(obj.optJSONArray("extendedItems")),
            componentText = optStringOrNull(obj, "componentText"),
            audioComponentText = optStringOrNull(obj, "audioComponentText"),
            audioLanguage = optStringOrNull(obj, "audioLanguage"),
            canonicalGenre = optStringOrNull(obj, "canonicalGenre"),
            broadcastGenre = optStringOrNull(obj, "broadcastGenre"),
            genreSupplementText = optStringOrNull(obj, "genreSupplementText"),
            eventGroupText = optStringOrNull(obj, "eventGroupText"),
            freeCaText = optStringOrNull(obj, "freeCaText"),
            seriesName = optStringOrNull(obj, "seriesName"),
            diagnosticText = obj.optString("diagnosticText"),
            diagnosticDescriptorJson = obj.optString("diagnosticDescriptorJson", "{}"),
            textDiagnostics = parseTextDiagnosticSummary(obj.optString("diagnosticText")),
            parentalRatings = parseParentalRatings(obj.optJSONArray("parentalRatings")),
        )
    }

    private fun parseExtendedItems(array: JSONArray?): List<AribExtendedItem> = (0 until (array?.length() ?: 0)).mapNotNull { index ->
        val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
        AribExtendedItem(obj.optString("description"), obj.optString("text"))
    }

    private fun parseParentalRatings(array: JSONArray?): List<AribParentalRating> = (0 until (array?.length() ?: 0)).mapNotNull { index ->
        val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
        val country = obj.optString("countryCode")
        val rating = obj.optInt("rating", -1)
        val raw = obj.optInt("rawRating", -1)
        if (country.isBlank() || rating < 0 || raw < 0) null else AribParentalRating(country, rating, raw, obj.optBoolean("supported"))
    }

    private fun parseEpgUpdateWindows(array: JSONArray?): List<AribEpgUpdateWindow> = (0 until (array?.length() ?: 0)).mapNotNull { index ->
        val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
        val key = serviceKeyFrom(obj)
        val start = obj.optLong("windowStartMillis", -1L)
        val end = obj.optLong("windowEndMillis", -1L)
        if (key.originalNetworkId < 0 || key.transportStreamId < 0 || key.serviceId < 0 || start < 0L || end <= start) null else AribEpgUpdateWindow(
            serviceKey = key,
            windowStartMillis = start,
            windowEndMillis = end,
            validProgramStableIdentities = parseStringArray(obj.optJSONArray("validProgramStableIdentities")),
            deletionAuthoritative = obj.optBoolean("deletionAuthoritative", false),
        )
    }

    private fun parsePublishabilityDiagnostics(array: JSONArray?): List<ServicePublishabilityDiagnostic> = (0 until (array?.length() ?: 0)).mapNotNull { index ->
        val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
        val key = serviceKeyFrom(obj)
        if (key.originalNetworkId < 0 || key.transportStreamId < 0 || key.serviceId < 0) null else ServicePublishabilityDiagnostic(
            serviceKey = key,
            publishable = obj.optBoolean("publishable"),
            channelRegistrationReady = obj.optBoolean("channelRegistrationReady"),
            epgPublishable = obj.optBoolean("epgPublishable"),
            clearLivePlaybackSupported = obj.optBoolean("clearLivePlaybackSupported"),
            requiresCas = obj.optBoolean("requiresCas"),
            unsupportedCas = obj.optBoolean("unsupportedCas"),
            pmtPidResolved = obj.optBoolean("pmtPidResolved"),
            pmtParsed = obj.optBoolean("pmtParsed"),
            caStateResolved = obj.optBoolean("caStateResolved"),
            freeCaModeResolved = obj.optBoolean("freeCaModeResolved"),
            missingComponents = parseStringArray(obj.optJSONArray("missingComponents")),
            reasons = parseStringArray(obj.optJSONArray("reasons")),
            registrationReasons = parseStringArray(obj.optJSONArray("registrationReasons")),
            epgReasons = parseStringArray(obj.optJSONArray("epgReasons")),
        )
    }

    fun snapshotServicesBulk(): List<AribService> = snapshotBulk().services
    fun snapshotServicesForCasDiscoveryBulk(): List<AribService> = snapshotBulk().servicesForCasDiscovery
    fun snapshotCaMetadataBulk(): List<CaMetadata> = snapshotBulk().caMetadata
    fun snapshotCaMetadataForCasDiscoveryBulk(): List<CaMetadata> = snapshotBulk().caMetadataForCasDiscovery
    fun snapshotPmtPidsBulk(): List<PmtPidMapping> = snapshotBulk().pmtPidMappings
    fun snapshotPmtPidsForSectionFiltersBulk(): List<Int> = snapshotBulk().pmtPidsForSectionFilters
    fun snapshotTransportsBulk(): List<AribTransport> = snapshotBulk().transports
    fun snapshotSdtActualTransportsBulk(): List<AribTransport> = snapshotBulk().sdtActualTransports
    fun snapshotPrivateSectionsBulk(): List<PrivateSection> = snapshotBulk().privateSections
    fun eventsForTestOnly(): List<AribEvent> = snapshotBulk().events
    fun publishabilityDiagnosticsForTestOnly(): List<ServicePublishabilityDiagnostic> = snapshotBulk().publishabilityDiagnostics

    private fun snapshotPmtPidsIndexed(): List<PmtPidMapping> {
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

    /**
     * PMT section filter 起動専用の discovery API。
     * 通常 service snapshot / clear live playback 判定には依存しない。
     */
    private fun snapshotPmtPidsForSectionFiltersIndexed(): List<Int> {
        check(handle != 0L) { "ネイティブ解析器は終了済みです" }
        val count = nativeGetSectionFilterPmtPidCount(handle).coerceAtLeast(0)
        return (0 until count).mapNotNull { index ->
            nativeGetSectionFilterPmtPid(handle, index).takeIf { it in 0..0x1fff }
        }
    }

    private fun publishabilityDiagnosticsIndexed(): List<ServicePublishabilityDiagnostic> {
        check(handle != 0L) { "ネイティブ解析器は終了済みです" }
        val count = nativeGetPublishabilityCount(handle).coerceAtLeast(0)
        return (0 until count).mapNotNull { index ->
            val onId = nativeGetPublishabilityOriginalNetworkId(handle, index)
            val tsId = nativeGetPublishabilityTransportStreamId(handle, index)
            val serviceId = nativeGetPublishabilityServiceId(handle, index)
            if (onId < 0 || tsId < 0 || serviceId < 0) null else ServicePublishabilityDiagnostic(
                serviceKey = com.maleicacid.tvinput.common.ServiceKey(onId, tsId, serviceId),
                publishable = nativeGetPublishabilityIsPublishable(handle, index) == 1,
                channelRegistrationReady = nativeGetPublishabilityIsChannelRegistrationReady(handle, index) == 1,
                epgPublishable = nativeGetPublishabilityIsEpgPublishable(handle, index) == 1,
                clearLivePlaybackSupported = nativeGetPublishabilityIsClearLivePlaybackSupported(handle, index) == 1,
                requiresCas = nativeGetPublishabilityRequiresCas(handle, index) == 1,
                unsupportedCas = nativeGetPublishabilityUnsupportedCas(handle, index) == 1,
                pmtPidResolved = nativeGetPublishabilityPmtPidResolved(handle, index) == 1,
                pmtParsed = nativeGetPublishabilityPmtParsed(handle, index) == 1,
                caStateResolved = nativeGetPublishabilityCaStateResolved(handle, index) == 1,
                freeCaModeResolved = nativeGetPublishabilityFreeCaModeResolved(handle, index) == 1,
                missingComponents = nativeGetPublishabilityMissingComponents(handle, index)
                    .split(',')
                    .filter { it.isNotBlank() },
                reasons = nativeGetPublishabilityReasons(handle, index)
                    .split(',')
                    .filter { it.isNotBlank() },
                registrationReasons = nativeGetPublishabilityRegistrationReasons(handle, index)
                    .split(',')
                    .filter { it.isNotBlank() },
                epgReasons = nativeGetPublishabilityEpgReasons(handle, index)
                    .split(',')
                    .filter { it.isNotBlank() },
            )
        }
    }

    private fun snapshotServicesIndexed(): List<AribService> {
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
                    serviceScopedCaDescriptors = snapshotServiceScopedCaDescriptors(serviceIndex, casDiscovery = false),
                    hasProgramCaDescriptor = nativeGetServiceProgramCaCount(handle, serviceIndex) > 0,
                    hasEsCaDescriptor = nativeGetServiceEsCaCount(handle, serviceIndex) > 0,
                )
            }
        }
    }

    /**
     * CAS / diagnostics 用 discovery service snapshot。TvProvider 登録や live claim には使わない。
     */
    private fun snapshotServicesForCasDiscoveryIndexed(): List<AribService> {
        check(handle != 0L) { "ネイティブ解析器は終了済みです" }
        val count = nativeGetCasDiscoveryServiceCount(handle).coerceAtLeast(0)
        return (0 until count).mapNotNull { serviceIndex ->
            val serviceId = nativeGetCasDiscoveryServiceId(handle, serviceIndex)
            val tsId = nativeGetCasDiscoveryTransportStreamId(handle, serviceIndex)
            val onId = nativeGetCasDiscoveryOriginalNetworkId(handle, serviceIndex)
            if (serviceId < 0 || tsId < 0 || onId < 0) {
                null
            } else {
                AribService(
                    serviceKey = com.maleicacid.tvinput.common.ServiceKey(onId, tsId, serviceId),
                    name = nativeGetCasDiscoveryServiceName(handle, serviceIndex),
                    providerName = nativeGetCasDiscoveryProviderName(handle, serviceIndex),
                    serviceType = nativeGetCasDiscoveryServiceType(handle, serviceIndex).takeIf { it >= 0 },
                    pmtPid = nativeGetCasDiscoveryPmtPid(handle, serviceIndex).takeIf { it >= 0 },
                    pcrPid = nativeGetCasDiscoveryPcrPid(handle, serviceIndex).takeIf { it >= 0 },
                    freeCaMode = nativeGetCasDiscoveryFreeCaMode(handle, serviceIndex).takeIf { it >= 0 }?.let { it != 0 },
                    streams = snapshotCasDiscoveryElementaryStreams(serviceIndex),
                    serviceScopedCaDescriptors = snapshotServiceScopedCaDescriptors(serviceIndex, casDiscovery = true),
                    hasProgramCaDescriptor = nativeGetCasDiscoveryServiceProgramCaCount(handle, serviceIndex) > 0,
                    hasEsCaDescriptor = nativeGetCasDiscoveryServiceEsCaCount(handle, serviceIndex) > 0,
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

    private fun snapshotCasDiscoveryElementaryStreams(serviceIndex: Int): List<AribElementaryStream> {
        val count = nativeGetCasDiscoveryServiceEsCount(handle, serviceIndex).coerceAtLeast(0)
        return (0 until count).mapNotNull { esIndex ->
            val pid = nativeGetCasDiscoveryServiceEsElementaryPid(handle, serviceIndex, esIndex)
            val streamType = nativeGetCasDiscoveryServiceEsStreamType(handle, serviceIndex, esIndex)
            if (pid < 0 || streamType < 0) {
                null
            } else {
                val languageCount = nativeGetCasDiscoveryServiceEsLanguageCount(handle, serviceIndex, esIndex).coerceAtLeast(0)
                AribElementaryStream(
                    elementaryPid = pid,
                    streamType = streamType,
                    componentTag = nativeGetCasDiscoveryServiceEsComponentTag(handle, serviceIndex, esIndex).takeIf { it >= 0 },
                    componentType = nativeGetCasDiscoveryServiceEsComponentType(handle, serviceIndex, esIndex).takeIf { it >= 0 },
                    streamContent = nativeGetCasDiscoveryServiceEsStreamContent(handle, serviceIndex, esIndex).takeIf { it >= 0 },
                    languageCodes = (0 until languageCount).map { langIndex ->
                        nativeGetCasDiscoveryServiceEsLanguage(handle, serviceIndex, esIndex, langIndex)
                    },
                )
            }
        }
    }


    private fun snapshotServiceScopedCaDescriptors(serviceIndex: Int, casDiscovery: Boolean): List<CaDescriptor> {
        val programCount = if (casDiscovery) nativeGetCasDiscoveryServiceProgramCaCount(handle, serviceIndex) else nativeGetServiceProgramCaCount(handle, serviceIndex)
        val program = (0 until programCount.coerceAtLeast(0)).mapNotNull { caIndex ->
            val systemId = if (casDiscovery) nativeGetCasDiscoveryServiceProgramCaSystemId(handle, serviceIndex, caIndex) else nativeGetServiceProgramCaSystemId(handle, serviceIndex, caIndex)
            val caPid = if (casDiscovery) nativeGetCasDiscoveryServiceProgramCaPid(handle, serviceIndex, caIndex) else nativeGetServiceProgramCaPid(handle, serviceIndex, caIndex)
            val rawDescriptor = if (casDiscovery) nativeGetCasDiscoveryServiceProgramCaRawDescriptor(handle, serviceIndex, caIndex) else nativeGetServiceProgramCaRawDescriptor(handle, serviceIndex, caIndex)
            if (systemId < 0 || caPid < 0 || rawDescriptor.isEmpty()) null else CaDescriptor(systemId, caPid, CaDescriptorScope.PROGRAM, null, rawDescriptor)
        }
        val esGroupCount = if (casDiscovery) nativeGetCasDiscoveryServiceEsCaCount(handle, serviceIndex) else nativeGetServiceEsCaCount(handle, serviceIndex)
        val es = (0 until esGroupCount.coerceAtLeast(0)).flatMap { esCaIndex ->
            val esPid = if (casDiscovery) nativeGetCasDiscoveryServiceEsCaElementaryPid(handle, serviceIndex, esCaIndex) else nativeGetServiceEsCaElementaryPid(handle, serviceIndex, esCaIndex)
            val descriptorCount = if (casDiscovery) nativeGetCasDiscoveryServiceEsCaDescriptorCount(handle, serviceIndex, esCaIndex) else nativeGetServiceEsCaDescriptorCount(handle, serviceIndex, esCaIndex)
            (0 until descriptorCount.coerceAtLeast(0)).mapNotNull { caIndex ->
                val systemId = if (casDiscovery) nativeGetCasDiscoveryServiceEsCaSystemId(handle, serviceIndex, esCaIndex, caIndex) else nativeGetServiceEsCaSystemId(handle, serviceIndex, esCaIndex, caIndex)
                val caPid = if (casDiscovery) nativeGetCasDiscoveryServiceEsCaPid(handle, serviceIndex, esCaIndex, caIndex) else nativeGetServiceEsCaPid(handle, serviceIndex, esCaIndex, caIndex)
                val rawDescriptor = if (casDiscovery) nativeGetCasDiscoveryServiceEsCaRawDescriptor(handle, serviceIndex, esCaIndex, caIndex) else nativeGetServiceEsCaRawDescriptor(handle, serviceIndex, esCaIndex, caIndex)
                if (systemId < 0 || caPid < 0 || esPid < 0 || rawDescriptor.isEmpty()) null else CaDescriptor(systemId, caPid, CaDescriptorScope.ES, esPid, rawDescriptor)
            }
        }
        return program + es
    }


    private fun snapshotTransportsIndexed(): List<AribTransport> {
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

    private fun snapshotCaMetadataIndexed(): List<CaMetadata> {
        check(handle != 0L) { "ネイティブ解析器は終了済みです" }
        val services = snapshotServicesIndexed()
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

    /**
     * CAS / ECM / EMM / diagnostics 用 metadata。clear live playback service snapshot には依存しない。
     */
    private fun snapshotCaMetadataForCasDiscoveryIndexed(): List<CaMetadata> {
        check(handle != 0L) { "ネイティブ解析器は終了済みです" }
        val services = snapshotServicesForCasDiscoveryIndexed()
        val programCa = services.flatMapIndexed { serviceIndex, service ->
            val count = nativeGetCasDiscoveryServiceProgramCaCount(handle, serviceIndex).coerceAtLeast(0)
            (0 until count).mapNotNull { caIndex ->
                val systemId = nativeGetCasDiscoveryServiceProgramCaSystemId(handle, serviceIndex, caIndex)
                val ecmPid = nativeGetCasDiscoveryServiceProgramCaPid(handle, serviceIndex, caIndex)
                if (systemId < 0 || ecmPid < 0) null else CaMetadata(
                    serviceKey = service.serviceKey,
                    caSystemId = systemId,
                    ecmPid = ecmPid,
                    emmPid = null,
                    elementaryPid = null,
                    privateData = nativeGetCasDiscoveryServiceProgramCaPrivateData(handle, serviceIndex, caIndex),
                    source = CaMetadataSource.PROGRAM,
                )
            }
        }
        val esCa = services.flatMapIndexed { serviceIndex, service ->
            val groupCount = nativeGetCasDiscoveryServiceEsCaCount(handle, serviceIndex).coerceAtLeast(0)
            (0 until groupCount).flatMap { esCaIndex ->
                val elementaryPid = nativeGetCasDiscoveryServiceEsCaElementaryPid(handle, serviceIndex, esCaIndex)
                val descriptorCount = nativeGetCasDiscoveryServiceEsCaDescriptorCount(handle, serviceIndex, esCaIndex).coerceAtLeast(0)
                (0 until descriptorCount).mapNotNull { caIndex ->
                    val systemId = nativeGetCasDiscoveryServiceEsCaSystemId(handle, serviceIndex, esCaIndex, caIndex)
                    val ecmPid = nativeGetCasDiscoveryServiceEsCaPid(handle, serviceIndex, esCaIndex, caIndex)
                    if (systemId < 0 || ecmPid < 0 || elementaryPid < 0) null else CaMetadata(
                        serviceKey = service.serviceKey,
                        caSystemId = systemId,
                        ecmPid = ecmPid,
                        emmPid = null,
                        elementaryPid = elementaryPid,
                        privateData = nativeGetCasDiscoveryServiceEsCaPrivateData(handle, serviceIndex, esCaIndex, caIndex),
                        source = CaMetadataSource.ELEMENTARY_STREAM,
                    )
                }
            }
        }
        val catCa = (0 until nativeGetCasDiscoveryCatCaCount(handle).coerceAtLeast(0)).mapNotNull { caIndex ->
            val systemId = nativeGetCasDiscoveryCatCaSystemId(handle, caIndex)
            val emmPid = nativeGetCasDiscoveryCatCaPid(handle, caIndex)
            if (systemId < 0 || emmPid < 0) null else CaMetadata(
                serviceKey = null,
                caSystemId = systemId,
                ecmPid = null,
                emmPid = emmPid,
                elementaryPid = null,
                privateData = nativeGetCasDiscoveryCatCaPrivateData(handle, caIndex),
                source = CaMetadataSource.CAT,
            )
        }
        return programCa + esCa + catCa
    }

    private fun snapshotPrivateSectionsIndexed(): List<PrivateSection> {
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

    @Synchronized
    fun drainEpgWindowsForTestOnly(): List<AribEpgUpdateWindow> = snapshotBulk(takeUpdateWindows = true).epgUpdateWindows

    private fun eventsIndexed(): List<AribEvent> {
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
                canonicalGenre = null,
                broadcastGenre = nativeGetEventBroadcastGenre(handle, index).takeIf { it.isNotBlank() },
                genreSupplementText = nativeGetEventGenreSupplementText(handle, index).takeIf { it.isNotBlank() },
                eventGroupText = nativeGetEventGroupText(handle, index).takeIf { it.isNotBlank() },
                freeCaText = nativeGetEventFreeCaText(handle, index).takeIf { it.isNotBlank() },
                seriesName = nativeGetEventSeriesName(handle, index).takeIf { it.isNotBlank() },
                diagnosticText = nativeGetEventDiagnosticText(handle, index),
                diagnosticDescriptorJson = nativeGetEventDiagnosticDescriptorJson(handle, index),
                textDiagnostics = parseTextDiagnosticSummary(nativeGetEventDiagnosticText(handle, index)),
                parentalRatings = snapshotEventParentalRatings(index),
            )
        }
    }


    private fun snapshotEventParentalRatings(eventIndex: Int): List<AribParentalRating> {
        val count = nativeGetEventParentalRatingCount(handle, eventIndex).coerceAtLeast(0)
        return (0 until count).mapNotNull { ratingIndex ->
            val country = nativeGetEventParentalRatingCountryCode(handle, eventIndex, ratingIndex)
            val rating = nativeGetEventParentalRatingValue(handle, eventIndex, ratingIndex)
            val raw = nativeGetEventParentalRatingRawValue(handle, eventIndex, ratingIndex)
            if (country.isBlank() || rating < 0 || raw < 0) null else AribParentalRating(
                countryCode = country,
                rating = rating,
                rawRating = raw,
                supported = nativeGetEventParentalRatingSupported(handle, eventIndex, ratingIndex) == 1,
            )
        }
    }

    private fun parseExtendedItemsJson(raw: String): List<AribExtendedItem> = parseExtendedItemsJsonForTest(raw)

    private fun parseTextDiagnosticSummary(raw: String): List<String> = raw
        .split(' ', '\n')
        .filter { it.contains("unknownCount=") || it.contains("json=") || it.contains("component=") || it.contains("audio=") }

    fun decodeAribString(bytes: ByteArray): String = nativeDecodeAribString(bytes)

    fun decodeAribStringDiagnosticSummary(bytes: ByteArray): String = nativeDecodeAribStringDiagnosticSummary(bytes)

    override fun close() {
        val current = handle
        if (current != 0L) {
            nativeDestroy(current)
            handle = 0L
        }
    }

    companion object {
        fun parseExtendedItemsJsonForTest(raw: String): List<AribExtendedItem> {
            if (raw.isBlank() || raw == "[]") return emptyList()
            return runCatching {
                val array = JSONArray(raw)
                (0 until array.length()).map { index ->
                    val obj = array.getJSONObject(index)
                    AribExtendedItem(
                        itemDescription = obj.optString("description"),
                        itemText = obj.optString("text"),
                    )
                }
            }.getOrElse { error ->
                NativeAribSiParserDiagnostics.extendedItemJsonParseErrors.incrementAndGet()
                Log.w(LogTags.ARIBSI, "拡張形式イベント記述子 item JSON の解析に失敗しました", error)
                emptyList()
            }
        }
    }


    private external fun nativeBuildChannelProviderData(requestJson: String): String
    private external fun nativeBuildProgramProviderData(requestJson: String): String
    private external fun nativeNormalizeProgramProviderData(providerData: String): String
    private external fun nativeExtractProgramKey(providerData: String): String
    private external fun nativeExtractChannelTuneKey(providerData: String): String
    private external fun nativeAppendCurrentProgramDiagnostics(providerData: String, overlapCount: Long, selectedProgramId: Long, selectionRule: String): String

    private external fun nativeCreate(): Long
    private external fun nativeDestroy(handle: Long): Int
    private external fun nativeIngestSection(handle: Long, pid: Int, section: ByteArray): Int
    private external fun nativeLastStatus(handle: Long): Int
    private external fun nativeGetDiscoveryStage(handle: Long): Int
    private external fun nativeSnapshotBulkJson(handle: Long, takeUpdateWindows: Int): String

    private external fun nativeGetPublishabilityCount(handle: Long): Int
    private external fun nativeGetPublishabilityOriginalNetworkId(handle: Long, index: Int): Int
    private external fun nativeGetPublishabilityTransportStreamId(handle: Long, index: Int): Int
    private external fun nativeGetPublishabilityServiceId(handle: Long, index: Int): Int
    private external fun nativeGetPublishabilityIsPublishable(handle: Long, index: Int): Int
    private external fun nativeGetPublishabilityIsChannelRegistrationReady(handle: Long, index: Int): Int
    private external fun nativeGetPublishabilityIsClearLivePlaybackSupported(handle: Long, index: Int): Int
    private external fun nativeGetPublishabilityIsEpgPublishable(handle: Long, index: Int): Int
    private external fun nativeGetPublishabilityRequiresCas(handle: Long, index: Int): Int
    private external fun nativeGetPublishabilityUnsupportedCas(handle: Long, index: Int): Int
    private external fun nativeGetPublishabilityPmtPidResolved(handle: Long, index: Int): Int
    private external fun nativeGetPublishabilityPmtParsed(handle: Long, index: Int): Int
    private external fun nativeGetPublishabilityCaStateResolved(handle: Long, index: Int): Int
    private external fun nativeGetPublishabilityFreeCaModeResolved(handle: Long, index: Int): Int
    private external fun nativeGetPublishabilityMissingComponents(handle: Long, index: Int): String
    private external fun nativeGetPublishabilityReasons(handle: Long, index: Int): String
    private external fun nativeGetPublishabilityRegistrationReasons(handle: Long, index: Int): String
    private external fun nativeGetPublishabilityEpgReasons(handle: Long, index: Int): String

    private external fun nativeGetPmtPidMappingCount(handle: Long): Int
    private external fun nativeGetPmtPidMappingOriginalNetworkId(handle: Long, index: Int): Int
    private external fun nativeGetPmtPidMappingTransportStreamId(handle: Long, index: Int): Int
    private external fun nativeGetPmtPidMappingServiceId(handle: Long, index: Int): Int
    private external fun nativeGetPmtPidMappingPmtPid(handle: Long, index: Int): Int
    private external fun nativeGetSectionFilterPmtPidCount(handle: Long): Int
    private external fun nativeGetSectionFilterPmtPid(handle: Long, index: Int): Int


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
    private external fun nativeGetServiceProgramCaRawDescriptor(handle: Long, serviceIndex: Int, caIndex: Int): ByteArray

    private external fun nativeGetServiceEsCaCount(handle: Long, serviceIndex: Int): Int
    private external fun nativeGetServiceEsCaElementaryPid(handle: Long, serviceIndex: Int, esCaIndex: Int): Int
    private external fun nativeGetServiceEsCaDescriptorCount(handle: Long, serviceIndex: Int, esCaIndex: Int): Int
    private external fun nativeGetServiceEsCaSystemId(handle: Long, serviceIndex: Int, esCaIndex: Int, caIndex: Int): Int
    private external fun nativeGetServiceEsCaPid(handle: Long, serviceIndex: Int, esCaIndex: Int, caIndex: Int): Int
    private external fun nativeGetServiceEsCaPrivateData(handle: Long, serviceIndex: Int, esCaIndex: Int, caIndex: Int): ByteArray
    private external fun nativeGetServiceEsCaRawDescriptor(handle: Long, serviceIndex: Int, esCaIndex: Int, caIndex: Int): ByteArray

    private external fun nativeGetCatCaCount(handle: Long): Int
    private external fun nativeGetCatCaSystemId(handle: Long, caIndex: Int): Int
    private external fun nativeGetCatCaPid(handle: Long, caIndex: Int): Int
    private external fun nativeGetCatCaPrivateData(handle: Long, caIndex: Int): ByteArray
    private external fun nativeGetCatCaRawDescriptor(handle: Long, caIndex: Int): ByteArray

    private external fun nativeGetCasDiscoveryServiceCount(handle: Long): Int
    private external fun nativeGetCasDiscoveryServiceId(handle: Long, index: Int): Int
    private external fun nativeGetCasDiscoveryTransportStreamId(handle: Long, index: Int): Int
    private external fun nativeGetCasDiscoveryOriginalNetworkId(handle: Long, index: Int): Int
    private external fun nativeGetCasDiscoveryServiceName(handle: Long, index: Int): String
    private external fun nativeGetCasDiscoveryProviderName(handle: Long, index: Int): String
    private external fun nativeGetCasDiscoveryServiceType(handle: Long, index: Int): Int
    private external fun nativeGetCasDiscoveryPmtPid(handle: Long, index: Int): Int
    private external fun nativeGetCasDiscoveryPcrPid(handle: Long, index: Int): Int
    private external fun nativeGetCasDiscoveryFreeCaMode(handle: Long, index: Int): Int

    private external fun nativeGetCasDiscoveryServiceEsCount(handle: Long, serviceIndex: Int): Int
    private external fun nativeGetCasDiscoveryServiceEsElementaryPid(handle: Long, serviceIndex: Int, esIndex: Int): Int
    private external fun nativeGetCasDiscoveryServiceEsStreamType(handle: Long, serviceIndex: Int, esIndex: Int): Int
    private external fun nativeGetCasDiscoveryServiceEsComponentTag(handle: Long, serviceIndex: Int, esIndex: Int): Int
    private external fun nativeGetCasDiscoveryServiceEsComponentType(handle: Long, serviceIndex: Int, esIndex: Int): Int
    private external fun nativeGetCasDiscoveryServiceEsStreamContent(handle: Long, serviceIndex: Int, esIndex: Int): Int
    private external fun nativeGetCasDiscoveryServiceEsLanguageCount(handle: Long, serviceIndex: Int, esIndex: Int): Int
    private external fun nativeGetCasDiscoveryServiceEsLanguage(handle: Long, serviceIndex: Int, esIndex: Int, langIndex: Int): String

    private external fun nativeGetCasDiscoveryServiceProgramCaCount(handle: Long, serviceIndex: Int): Int
    private external fun nativeGetCasDiscoveryServiceProgramCaSystemId(handle: Long, serviceIndex: Int, caIndex: Int): Int
    private external fun nativeGetCasDiscoveryServiceProgramCaPid(handle: Long, serviceIndex: Int, caIndex: Int): Int
    private external fun nativeGetCasDiscoveryServiceProgramCaPrivateData(handle: Long, serviceIndex: Int, caIndex: Int): ByteArray
    private external fun nativeGetCasDiscoveryServiceProgramCaRawDescriptor(handle: Long, serviceIndex: Int, caIndex: Int): ByteArray

    private external fun nativeGetCasDiscoveryServiceEsCaCount(handle: Long, serviceIndex: Int): Int
    private external fun nativeGetCasDiscoveryServiceEsCaElementaryPid(handle: Long, serviceIndex: Int, esCaIndex: Int): Int
    private external fun nativeGetCasDiscoveryServiceEsCaDescriptorCount(handle: Long, serviceIndex: Int, esCaIndex: Int): Int
    private external fun nativeGetCasDiscoveryServiceEsCaSystemId(handle: Long, serviceIndex: Int, esCaIndex: Int, caIndex: Int): Int
    private external fun nativeGetCasDiscoveryServiceEsCaPid(handle: Long, serviceIndex: Int, esCaIndex: Int, caIndex: Int): Int
    private external fun nativeGetCasDiscoveryServiceEsCaPrivateData(handle: Long, serviceIndex: Int, esCaIndex: Int, caIndex: Int): ByteArray
    private external fun nativeGetCasDiscoveryServiceEsCaRawDescriptor(handle: Long, serviceIndex: Int, esCaIndex: Int, caIndex: Int): ByteArray

    private external fun nativeGetCasDiscoveryCatCaCount(handle: Long): Int
    private external fun nativeGetCasDiscoveryCatCaSystemId(handle: Long, caIndex: Int): Int
    private external fun nativeGetCasDiscoveryCatCaPid(handle: Long, caIndex: Int): Int
    private external fun nativeGetCasDiscoveryCatCaPrivateData(handle: Long, caIndex: Int): ByteArray
    private external fun nativeGetCasDiscoveryCatCaRawDescriptor(handle: Long, caIndex: Int): ByteArray

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
    private external fun nativeGetEventBroadcastGenre(handle: Long, index: Int): String
    private external fun nativeGetEventGenreSupplementText(handle: Long, index: Int): String
    private external fun nativeGetEventGroupText(handle: Long, index: Int): String
    private external fun nativeGetEventFreeCaText(handle: Long, index: Int): String
    private external fun nativeGetEventSeriesName(handle: Long, index: Int): String
    private external fun nativeGetEventDiagnosticDescriptorJson(handle: Long, index: Int): String
    private external fun nativeGetEventScope(handle: Long, index: Int): String
    private external fun nativeGetEventDiagnosticText(handle: Long, index: Int): String
    private external fun nativeGetEventParentalRatingCount(handle: Long, eventIndex: Int): Int
    private external fun nativeGetEventParentalRatingCountryCode(handle: Long, eventIndex: Int, ratingIndex: Int): String
    private external fun nativeGetEventParentalRatingValue(handle: Long, eventIndex: Int, ratingIndex: Int): Int
    private external fun nativeGetEventParentalRatingRawValue(handle: Long, eventIndex: Int, ratingIndex: Int): Int
    private external fun nativeGetEventParentalRatingSupported(handle: Long, eventIndex: Int, ratingIndex: Int): Int

    companion object {
        init {
            System.loadLibrary("maleicacid_arib_si_engine_jni")
        }
    }
}

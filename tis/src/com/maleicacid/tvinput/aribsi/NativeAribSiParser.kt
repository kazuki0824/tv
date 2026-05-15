package com.maleicacid.tvinput.aribsi

import com.maleicacid.tvinput.common.ServiceKey
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
    fun programProviderDataSignature(providerData: String): String = nativeProgramProviderDataSignature(providerData)
    fun extractProgramKeyResult(providerData: String): String = nativeExtractProgramKeyResult(providerData)
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
        return parseBulkSnapshotJson(nativeSnapshotBulkJson(handle, if (takeUpdateWindows) 1 else 0))
    }

    private fun parseBulkSnapshotJson(raw: String): BulkSnapshot {
        val root = JSONObject(raw.ifBlank { "{}" })
        val services = parseServices(root.optJSONArray("services"))
        val events = attachServiceComponentsToEvents(parseEvents(root.optJSONArray("events")), services)
        return BulkSnapshot(
            services = services,
            servicesForCasDiscovery = parseServices(root.optJSONArray("servicesForCasDiscovery")),
            caMetadata = parseCaMetadataList(root.optJSONArray("caMetadata")),
            caMetadataForCasDiscovery = parseCaMetadataList(root.optJSONArray("caMetadataForCasDiscovery")),
            pmtPidMappings = parsePmtPidMappings(root.optJSONArray("pmtPidMappings")),
            pmtPidsForSectionFilters = parseIntArray(root.optJSONArray("pmtPidsForSectionFilters")).filter { it in 0..0x1fff },
            transports = parseTransports(root.optJSONArray("transports")),
            sdtActualTransports = parseTransports(root.optJSONArray("sdtActualTransports")),
            privateSections = parsePrivateSections(root.optJSONArray("privateSections")),
            events = events,
            epgUpdateWindows = parseEpgUpdateWindows(root.optJSONArray("epgUpdateWindows")),
            publishabilityDiagnostics = parsePublishabilityDiagnostics(root.optJSONArray("publishabilityDiagnostics")),
        )
    }

    private fun parseIntArray(array: JSONArray?): List<Int> = (0 until (array?.length() ?: 0)).map { index -> array!!.optInt(index) }
    private fun parseStringArray(array: JSONArray?): List<String> = (0 until (array?.length() ?: 0)).mapNotNull { index -> array!!.optString(index).takeIf { it.isNotBlank() } }
    private fun optIntOrNull(obj: JSONObject, key: String): Int? = if (obj.isNull(key)) null else obj.optInt(key)
    private fun optStringOrNull(obj: JSONObject, key: String): String? = obj.optString(key).takeIf { it.isNotBlank() }
    private fun optBoolOrNull(obj: JSONObject, key: String): Boolean? = if (obj.isNull(key)) null else obj.optBoolean(key)

    private fun hexToBytes(hex: String): ByteArray {
        if (hex.length % 2 != 0) return ByteArray(0)
        return ByteArray(hex.length / 2) { index -> hex.substring(index * 2, index * 2 + 2).toIntOrNull(16)?.toByte() ?: 0 }
    }

    private fun serviceKeyFrom(obj: JSONObject): ServiceKey = ServiceKey(
        originalNetworkId = obj.optInt("originalNetworkId", -1),
        transportStreamId = obj.optInt("transportStreamId", -1),
        serviceId = obj.optInt("serviceId", -1),
    )

    private fun parseServices(array: JSONArray?): List<AribService> = (0 until (array?.length() ?: 0)).mapNotNull { index ->
        val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
        val key = serviceKeyFrom(obj)
        if (key.originalNetworkId < 0 || key.transportStreamId < 0 || key.serviceId < 0) return@mapNotNull null
        val streams = parseStreams(obj.optJSONArray("streams"))
        val caDescriptors = parseCaDescriptors(obj.optJSONArray("serviceScopedCaDescriptors"))
        val provisional = AribService(
            serviceKey = key,
            name = obj.optString("name"),
            providerName = obj.optString("providerName"),
            serviceType = optIntOrNull(obj, "serviceType"),
            pmtPid = optIntOrNull(obj, "pmtPid"),
            pcrPid = optIntOrNull(obj, "pcrPid"),
            freeCaMode = optBoolOrNull(obj, "freeCaMode"),
            streams = streams,
            hasProgramCaDescriptor = obj.optBoolean("hasProgramCaDescriptor"),
            hasEsCaDescriptor = obj.optBoolean("hasEsCaDescriptor"),
            serviceScopedCaDescriptors = caDescriptors,
        )
        provisional.copy(components = parseComponents(obj.optJSONObject("components")) ?: AribComponents())
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
            dataComponentId = optIntOrNull(obj, "dataComponentId"),
            isCaption = obj.optBoolean("isCaption"),
            isSuperimpose = obj.optBoolean("isSuperimpose"),
        )
    }

    private fun parseCaDescriptors(array: JSONArray?): List<CaDescriptor> = (0 until (array?.length() ?: 0)).mapNotNull { index ->
        val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
        val systemId = obj.optInt("caSystemId", -1)
        if (systemId < 0) null else CaDescriptor(
            caSystemId = systemId,
            caPid = optIntOrNull(obj, "caPid"),
            scope = if (obj.optString("scope") == "ES") CaDescriptorScope.ES else CaDescriptorScope.PROGRAM,
            esPid = optIntOrNull(obj, "esPid"),
            rawDescriptor = hexToBytes(obj.optString("rawDescriptorHex")),
        )
    }

    private fun parsePmtPidMappings(array: JSONArray?): List<PmtPidMapping> = (0 until (array?.length() ?: 0)).mapNotNull { index ->
        val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
        val key = serviceKeyFrom(obj)
        val pmtPid = obj.optInt("pmtPid", -1)
        if (key.originalNetworkId < 0 || key.transportStreamId < 0 || key.serviceId < 0 || pmtPid !in 0..0x1fff) null else PmtPidMapping(key, pmtPid)
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

    private fun attachServiceComponentsToEvents(events: List<AribEvent>, services: List<AribService>): List<AribEvent> {
        if (events.isEmpty() || services.isEmpty()) return events
        val componentsByService = services.associate { it.serviceKey to it.components }
        return events.map { event ->
            val serviceComponents = componentsByService[event.serviceKey]
            val components = if (serviceComponents == null) event.descriptors.components else mergeEventAndServiceComponents(event.descriptors.components, serviceComponents)
            event.copy(descriptors = event.descriptors.copy(components = components))
        }
    }

    private fun mergeEventAndServiceComponents(eventComponents: AribComponents, serviceComponents: AribComponents): AribComponents = AribComponents(
        video = mergeComponentEntries(eventComponents.video, serviceComponents.video),
        audio = mergeComponentEntries(eventComponents.audio, serviceComponents.audio),
        subtitle = serviceComponents.subtitle + eventComponents.subtitle.filterNot { e -> serviceComponents.subtitle.any { sameComponentIdentity(it, e) } },
        data = serviceComponents.data + eventComponents.data.filterNot { e -> serviceComponents.data.any { sameComponentIdentity(it, e) } },
    )

    private fun mergeComponentEntries(eventEntries: List<AribComponentEntry>, serviceEntries: List<AribComponentEntry>): List<AribComponentEntry> {
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

    private fun mergeComponentEntry(eventEntry: AribComponentEntry, serviceEntry: AribComponentEntry): AribComponentEntry = serviceEntry.copy(
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
        val descriptorsObj = obj.optJSONObject("descriptors") ?: JSONObject()
        val sourceObj = obj.optJSONObject("source") ?: JSONObject()
        val component = descriptorsObj.optJSONObject("component") ?: JSONObject()
        val audio = descriptorsObj.optJSONObject("audio") ?: JSONObject()
        val genres = descriptorsObj.optJSONObject("genres") ?: JSONObject()
        val freeCaMode = descriptorsObj.optJSONObject("freeCaMode") ?: JSONObject()
        val diagnostics = descriptorsObj.optJSONObject("diagnostics") ?: JSONObject()
        val series = descriptorsObj.optJSONObject("series")
        val descriptorDiagnostics = diagnostics.optJSONArray("descriptorDiagnostics")
        if (key.originalNetworkId < 0 || key.transportStreamId < 0 || key.serviceId < 0 || eventId < 0 || start <= 0L || duration <= 0L) return@mapNotNull null
        AribEvent(
            serviceKey = key,
            stableIdentity = obj.optString("stableIdentity"),
            eventId = eventId,
            startTimeMillis = start,
            durationMillis = duration,
            title = obj.optString("title"),
            description = obj.optString("description"),
            extendedDescription = obj.optString("extendedDescription"),
            eventScope = obj.optString("eventScope", "present_following"),
            descriptors = AribEventDescriptors(
                extendedItems = parseExtendedItems(descriptorsObj.optJSONArray("extendedItems")),
                componentText = optStringOrNull(component, "text"),
                audioComponentText = optStringOrNull(audio, "componentText"),
                audioLanguage = optStringOrNull(audio, "language"),
                broadcastGenre = optStringOrNull(genres, "broadcastGenre"),
                genreSupplementText = optStringOrNull(genres, "genreSupplementText"),
                relatedItems = parseRelatedItems(descriptorsObj.optJSONArray("relatedItems")),
                linkage = parseLinkage(descriptorsObj.optJSONArray("linkage")),
                scrambled = if (freeCaMode.isNull("scrambled")) null else freeCaMode.optBoolean("scrambled"),
                freeCaMode = parseFreeCaMode(freeCaMode),
                seriesId = if (series == null || series.isNull("seriesId")) null else series.optInt("seriesId"),
                episodeNumber = if (series == null || series.isNull("episodeNumber")) null else series.optInt("episodeNumber"),
                lastEpisodeNumber = if (series == null || series.isNull("lastEpisodeNumber")) null else series.optInt("lastEpisodeNumber"),
                series = parseSeries(series),
                parentalRatings = parseParentalRatings(descriptorsObj.optJSONArray("parentalRatings")),
                components = parseComponents(descriptorsObj.optJSONObject("components")) ?: AribComponents(),
                diagnostics = AribEventDiagnostics(
                    summary = diagnostics.optString("summary"),
                    descriptorDiagnostics = parseDescriptorDiagnostics(descriptorDiagnostics),
                    textDiagnostics = parseTextDiagnosticSummary(diagnostics.optString("summary")),
                ),
            ),
        )
    }

    private fun parseExtendedItems(array: JSONArray?): List<AribExtendedItem> = (0 until (array?.length() ?: 0)).mapNotNull { index ->
        val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
        AribExtendedItem(obj.optString("description"), obj.optString("text"))
    }

    private fun parseParentalRatings(array: JSONArray?): List<AribParentalRating> = (0 until (array?.length() ?: 0)).mapNotNull { index ->
        val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
        val country = obj.optString("countryCode")
        val rating = obj.optInt("ratingValue", obj.optInt("rating", -1))
        val raw = obj.optInt("rawRatingByte", obj.optInt("rawRating", -1))
        if (country.isBlank() || rating < 0 || raw < 0) null else AribParentalRating(country, rating, raw, obj.optBoolean("supported"))
    }


    private fun parseRelatedItems(array: JSONArray?): List<AribRelatedItem> = (0 until (array?.length() ?: 0)).mapNotNull { index ->
        val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
        AribRelatedItem(
            kind = obj.optString("kind", "unknown"),
            groupType = obj.optInt("groupType", 0),
            originalNetworkId = optIntOrNull(obj, "originalNetworkId"),
            transportStreamId = optIntOrNull(obj, "transportStreamId"),
            serviceId = obj.optInt("serviceId", -1),
            eventId = obj.optInt("eventId", -1),
            parseStatus = obj.optString("parseStatus", "OK"),
        ).takeIf { it.serviceId >= 0 && it.eventId >= 0 }
    }

    private fun parseLinkage(array: JSONArray?): List<AribLinkage> = (0 until (array?.length() ?: 0)).mapNotNull { index ->
        val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
        AribLinkage(
            linkageType = obj.optInt("linkageType", -1),
            originalNetworkId = obj.optInt("originalNetworkId", -1),
            transportStreamId = obj.optInt("transportStreamId", -1),
            serviceId = obj.optInt("serviceId", -1),
            privateDataHex = obj.optString("privateDataHex", ""),
            parseStatus = obj.optString("parseStatus", "OK"),
        ).takeIf { it.linkageType >= 0 && it.originalNetworkId >= 0 && it.transportStreamId >= 0 && it.serviceId >= 0 }
    }

    private fun parseFreeCaMode(obj: JSONObject): AribFreeCaMode? = if (obj.length() == 0) null else AribFreeCaMode(
        raw = optIntOrNull(obj, "raw"),
        scrambled = optBoolOrNull(obj, "scrambled"),
        text = optStringOrNull(obj, "text"),
        parseStatus = obj.optString("parseStatus", "OK"),
    )

    private fun parseSeries(obj: JSONObject?): AribSeries? = obj?.let {
        AribSeries(
            seriesId = optIntOrNull(it, "seriesId"),
            repeatLabel = it.optInt("repeatLabel", 0),
            programPattern = it.optInt("programPattern", 0),
            expireDateValid = it.optBoolean("expireDateValid"),
            expireDate = optIntOrNull(it, "expireDate"),
            episodeNumber = optIntOrNull(it, "episodeNumber"),
            lastEpisodeNumber = optIntOrNull(it, "lastEpisodeNumber"),
            name = optStringOrNull(it, "name"),
            parseStatus = it.optString("parseStatus", "OK"),
        )
    }

    private fun parseComponents(obj: JSONObject?): AribComponents? = obj?.let {
        AribComponents(
            video = parseComponentEntries(it.optJSONArray("video")),
            audio = parseComponentEntries(it.optJSONArray("audio")),
            subtitle = parseComponentEntries(it.optJSONArray("subtitle")),
            data = parseComponentEntries(it.optJSONArray("data")),
        )
    }

    private fun parseComponentEntries(array: JSONArray?): List<AribComponentEntry> = (0 until (array?.length() ?: 0)).mapNotNull { index ->
        val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
        val pid = obj.optInt("esPid", -1)
        if (pid < 0) null else AribComponentEntry(
            esPid = pid,
            streamType = optIntOrNull(obj, "streamType"),
            componentTag = optIntOrNull(obj, "componentTag"),
            componentType = optIntOrNull(obj, "componentType"),
            codec = optStringOrNull(obj, "codec"),
            language = optStringOrNull(obj, "language"),
            secondLanguage = optStringOrNull(obj, "secondLanguage"),
            channelConfiguration = optStringOrNull(obj, "channelConfiguration"),
            samplingInfo = optStringOrNull(obj, "samplingInfo"),
            sourceDescriptor = optStringOrNull(obj, "sourceDescriptor"),
            resolution = optStringOrNull(obj, "resolution"),
            scan = optStringOrNull(obj, "scan"),
            aspect = optStringOrNull(obj, "aspect"),
            profileLevel = optStringOrNull(obj, "profileLevel"),
            dataComponentId = optIntOrNull(obj, "dataComponentId"),
            trackId = optStringOrNull(obj, "trackId"),
            captionServiceKind = optStringOrNull(obj, "captionServiceKind"),
            r51PlaybackSupported = optBoolOrNull(obj, "r51PlaybackSupported"),
            liveViewableClaim = optBoolOrNull(obj, "liveViewableClaim"),
            diagnosticCode = optStringOrNull(obj, "diagnosticCode"),
            main = optBoolOrNull(obj, "main"),
            multiLingual = optBoolOrNull(obj, "multiLingual"),
            qualityIndicator = optIntOrNull(obj, "qualityIndicator"),
            parseStatus = obj.optString("parseStatus", "OK"),
        )
    }

    private fun parseDescriptorDiagnostics(array: JSONArray?): List<AribDescriptorDiagnosticV1> = (0 until (array?.length() ?: 0)).mapNotNull { index ->
        val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
        val scopeObj = obj.optJSONObject("scope") ?: JSONObject()
        val descriptorObj = obj.optJSONObject("descriptor") ?: return@mapNotNull null
        AribDescriptorDiagnosticV1(
            severity = obj.optString("severity", "info"),
            code = obj.optString("code", "UNKNOWN"),
            scope = AribDescriptorScope(
                pid = optIntOrNull(scopeObj, "pid"),
                tableId = optIntOrNull(scopeObj, "tableId"),
                tableIdExtension = optIntOrNull(scopeObj, "tableIdExtension"),
                version = optIntOrNull(scopeObj, "version"),
                sectionNumber = optIntOrNull(scopeObj, "sectionNumber"),
                originalNetworkId = optIntOrNull(scopeObj, "originalNetworkId"),
                transportStreamId = optIntOrNull(scopeObj, "transportStreamId"),
                serviceId = optIntOrNull(scopeObj, "serviceId"),
                eventId = optIntOrNull(scopeObj, "eventId"),
            ),
            descriptor = AribDescriptorInfo(
                tag = descriptorObj.optInt("tag", -1),
                name = optStringOrNull(descriptorObj, "name"),
                offset = descriptorObj.optInt("offset", 0),
                declaredLength = descriptorObj.optInt("declaredLength", 0),
                actualRemainingLength = descriptorObj.optInt("actualRemainingLength", 0),
                parseStatus = descriptorObj.optString("parseStatus", "UNKNOWN"),
                rawPrefixHex = descriptorObj.optString("rawPrefixHex", ""),
            ),
            message = obj.optString("message", ""),
        ).takeIf { it.descriptor.tag >= 0 }
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

    @Synchronized
    fun drainEpgWindowsForTestOnly(): List<AribEpgUpdateWindow> = snapshotBulk(takeUpdateWindows = true).epgUpdateWindows

    private fun parseTextDiagnosticSummary(raw: String): List<String> = raw
        .split(' ', '\n')
        .filter { it.contains("unknownCount=") || it.contains("component=") || it.contains("audio=") }

    fun decodeAribString(bytes: ByteArray): String = nativeDecodeAribString(bytes)
    fun decodeAribStringDiagnosticSummary(bytes: ByteArray): String = nativeDecodeAribStringDiagnosticSummary(bytes)

    override fun close() {
        val current = handle
        if (current != 0L) {
            nativeDestroy(current)
            handle = 0L
        }
    }

    private external fun nativeBuildChannelProviderData(requestJson: String): String
    private external fun nativeBuildProgramProviderData(requestJson: String): String
    private external fun nativeNormalizeProgramProviderData(providerData: String): String
    private external fun nativeProgramProviderDataSignature(providerData: String): String
    private external fun nativeExtractProgramKeyResult(providerData: String): String
    private external fun nativeExtractChannelTuneKey(providerData: String): String
    private external fun nativeAppendCurrentProgramDiagnostics(providerData: String, overlapCount: Long, selectedProgramId: Long, selectionRule: String): String
    private external fun nativeCreate(): Long
    private external fun nativeDestroy(handle: Long): Int
    private external fun nativeIngestSection(handle: Long, pid: Int, section: ByteArray): Int
    private external fun nativeLastStatus(handle: Long): Int
    private external fun nativeGetDiscoveryStage(handle: Long): Int
    private external fun nativeSnapshotBulkJson(handle: Long, takeUpdateWindows: Int): String
    private external fun nativeDecodeAribString(bytes: ByteArray): String
    private external fun nativeDecodeAribStringDiagnosticSummary(bytes: ByteArray): String

    companion object {
        private val R51_VIDEO_CODECS = mapOf(0x02 to "MPEG-2", 0x1b to "H.264")
        private val RECOGNIZED_VIDEO_CODECS = R51_VIDEO_CODECS + mapOf(0x24 to "HEVC")
        private val R51_AUDIO_CODECS = mapOf(0x03 to "MPEG-Audio", 0x04 to "MPEG-Audio", 0x0f to "AAC")
        private val RECOGNIZED_AUDIO_CODECS = R51_AUDIO_CODECS + mapOf(0x11 to "MPEG-4-AAC-LATM")
        private val CAPTION_DATA_COMPONENT_IDS = setOf(0x0008, 0x0012)

        init {
            System.loadLibrary("maleicacid_arib_si_engine_jni")
        }

        fun componentsForServiceForTest(service: AribService): AribComponents {
            val video = mutableListOf<AribComponentEntry>()
            val audio = mutableListOf<AribComponentEntry>()
            val subtitle = mutableListOf<AribComponentEntry>()
            val data = mutableListOf<AribComponentEntry>()
            service.streams.forEach { stream ->
                val videoCodec = RECOGNIZED_VIDEO_CODECS[stream.streamType]
                val audioCodec = RECOGNIZED_AUDIO_CODECS[stream.streamType]
                when {
                    videoCodec != null -> video += codecComponent(stream, videoCodec, r51Supported = R51_VIDEO_CODECS.containsKey(stream.streamType))
                    audioCodec != null -> audio += codecComponent(stream, audioCodec, r51Supported = R51_AUDIO_CODECS.containsKey(stream.streamType)).copy(language = stream.languageCodes.firstOrNull() ?: "jpn")
                    stream.isCaption || stream.dataComponentId in CAPTION_DATA_COMPONENT_IDS -> subtitle += AribComponentEntry(
                        esPid = stream.elementaryPid,
                        componentTag = stream.componentTag ?: 0,
                        dataComponentId = stream.dataComponentId ?: 0x0008,
                        language = stream.languageCodes.firstOrNull() ?: "jpn",
                        trackId = stream.componentTag?.let { "subtitle:${stream.elementaryPid}:$it" } ?: "subtitle:${stream.elementaryPid}",
                        captionServiceKind = when {
                            stream.isSuperimpose -> "superimpose"
                            stream.dataComponentId == 0x0012 -> "one-seg-caption"
                            else -> "caption"
                        },
                    )
                    stream.dataComponentId != null -> data += AribComponentEntry(
                        esPid = stream.elementaryPid,
                        componentTag = stream.componentTag ?: 0,
                        dataComponentId = stream.dataComponentId,
                        componentType = stream.componentType ?: 0,
                    )
                }
            }
            return AribComponents(video = video, audio = audio, subtitle = subtitle, data = data)
        }

        fun toComponentsObjectForServiceForTest(service: AribService): String = ProviderDataBridge.toComponentsObject(componentsForServiceForTest(service)).toString()

        private fun codecComponent(stream: AribElementaryStream, codec: String, r51Supported: Boolean): AribComponentEntry = AribComponentEntry(
            esPid = stream.elementaryPid,
            streamType = stream.streamType,
            componentTag = stream.componentTag ?: 0,
            componentType = stream.componentType ?: 0,
            codec = codec,
            r51PlaybackSupported = r51Supported,
            liveViewableClaim = r51Supported,
            diagnosticCode = if (r51Supported) "OK" else "UNSUPPORTED_R51_CODEC",
            parseStatus = if (r51Supported) "OK" else "UNSUPPORTED_R51",
        )

        fun isR51PlaybackSupportedVideoCodecForTest(streamType: Int): Boolean = R51_VIDEO_CODECS.containsKey(streamType)
        fun isRecognizedVideoCodecForTest(streamType: Int): Boolean = RECOGNIZED_VIDEO_CODECS.containsKey(streamType)
        fun isR51PlaybackSupportedAudioCodecForTest(streamType: Int): Boolean = R51_AUDIO_CODECS.containsKey(streamType)
        fun isRecognizedAudioCodecForTest(streamType: Int): Boolean = RECOGNIZED_AUDIO_CODECS.containsKey(streamType)
    }
}

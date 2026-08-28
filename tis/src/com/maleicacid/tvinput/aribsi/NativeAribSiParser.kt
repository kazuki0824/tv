package com.maleicacid.tvinput.aribsi

import com.maleicacid.tvinput.common.NetworkId16
import com.maleicacid.tvinput.common.ServiceId16
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.TransportStreamId16
import com.maleicacid.tvinput.common.TsPid
import org.json.JSONArray
import org.json.JSONObject

class NativeAribSiParser : AutoCloseable {
    private data class NativeTransaction(
        val ingestSequence: Long,
        val discoveryStage: Int,
        val tableRequirements: List<TableRequirementStatus>,
        val services: List<AribService>,
        val caMetadata: List<CaMetadata>,
        val malformedCaDescriptorDiagnostics: List<MalformedCaDescriptorDiagnostic>,
        val malformedCaDescriptorCountByServiceId: Map<ServiceId16, Int>,
        val pmtPidMappings: List<PmtPidMapping>,
        val sdtActualTransports: List<AribTransport>,
        val events: List<AribEvent>,
        val epgUpdateWindows: List<AribEpgUpdateWindow>,
        val serviceSemanticFacts: List<ServiceSemanticFacts>,
        val parserDiagnostics: List<ParserDiagnostic>,
    )

    private var handle: Long = nativeCreate()

    fun buildChannelProviderData(requestJson: String): String = nativeBuildChannelProviderData(requestJson)
    fun buildProgramProviderData(requestJson: String): String = nativeBuildProgramProviderData(requestJson)
    fun buildProgramKey(onid: Int, tsid: Int, sid: Int, eventId: Int): String = nativeBuildProgramKey(onid, tsid, sid, eventId)
    fun normalizeProgramProviderData(providerData: ByteArray): String = nativeNormalizeProgramProviderData(providerData)
    fun extractProgramKeyResult(providerData: ByteArray): String = nativeExtractProgramKeyResult(providerData)
    fun decodeChannelProviderData(providerData: ByteArray): String = nativeDecodeChannelProviderData(providerData)

    fun ingestSection(pid: TsPid, section: ByteArray): Int {
        check(handle != 0L) { "ネイティブ解析器は終了済みです" }
        return nativeIngestSection(handle, pid.value, section)
    }

    fun lastStatus(): Int = nativeLastStatus(handle)
    fun setDiscoveryProfile(profile: Int) {
        check(nativeSetDiscoveryProfile(handle, profile) == SiStatus.OK) {
            "SI discovery profileを設定できません profile=$profile"
        }
    }
    @Synchronized
    fun takeProgramPublishSnapshot(): ProgramPublishSnapshot = buildProgramPublishSnapshot(readNativeTransaction(takeUpdateWindows = true))

    @Synchronized
    fun programStateSnapshot(): ProgramPublishSnapshot = buildProgramPublishSnapshot(readNativeTransaction(takeUpdateWindows = false))

    @Synchronized
    fun serviceRegistrationSnapshot(): ServiceRegistrationSnapshot {
        val snapshot = readNativeTransaction(takeUpdateWindows = false)
        return ServiceRegistrationSnapshot(
            discoveryStage = snapshot.discoveryStage,
            tableRequirements = snapshot.tableRequirements,
            services = snapshot.services,
            actualTransports = snapshot.sdtActualTransports.map { TransportKey(it.originalNetwork, it.transportStream) }.toSet(),
            actualTransportMetadata = snapshot.sdtActualTransports,
            semanticFactsByServiceKey = snapshot.serviceSemanticFacts.associateBy { it.serviceKey },
            diagnostics = snapshot.parserDiagnostics,
        )
    }

    @Synchronized
    fun casDiscoverySnapshot(): CasDiscoverySnapshot {
        val snapshot = readNativeTransaction(takeUpdateWindows = false)
        return CasDiscoverySnapshot(
            services = snapshot.services,
            caMetadata = snapshot.caMetadata,
            pmtPids = snapshot.pmtPidMappings.associate { it.serviceKey to it.pmtPid },
            catEmmPids = snapshot.caMetadata.mapNotNull { it.emmPid }.distinct().sorted(),
            diagnostics = descriptorDiagnosticsFromEvents(snapshot.events),
            malformedCaDescriptorDiagnostics = snapshot.malformedCaDescriptorDiagnostics,
        )
    }

    @Synchronized
    fun livePlaybackSnapshot(): LivePlaybackSnapshot {
        val snapshot = readNativeTransaction(takeUpdateWindows = false)
        return LivePlaybackSnapshot(
            ingestSequence = snapshot.ingestSequence,
            services = snapshot.services,
            caMetadata = snapshot.caMetadata,
            pmtPids = snapshot.pmtPidMappings.associate { it.serviceKey to it.pmtPid },
            catEmmPids = snapshot.caMetadata.mapNotNull { it.emmPid }.distinct().sorted(),
            semanticFactsByServiceKey = snapshot.serviceSemanticFacts.associateBy { it.serviceKey },
            descriptorDiagnostics = descriptorDiagnosticsFromEvents(snapshot.events),
            parserDiagnostics = snapshot.parserDiagnostics,
            malformedCaDescriptorDiagnostics = snapshot.malformedCaDescriptorDiagnostics,
        )
    }

    private fun buildProgramPublishSnapshot(snapshot: NativeTransaction): ProgramPublishSnapshot = ProgramPublishSnapshot(
        ingestSequence = snapshot.ingestSequence,
        events = snapshot.events,
        updateWindows = snapshot.epgUpdateWindows,
        semanticFactsByServiceKey = snapshot.serviceSemanticFacts.associateBy { it.serviceKey },
        descriptorDiagnostics = descriptorDiagnosticsFromEvents(snapshot.events),
        parserDiagnostics = snapshot.parserDiagnostics,
        malformedCaDescriptorCountByServiceId = snapshot.malformedCaDescriptorCountByServiceId,
    )

    private fun descriptorDiagnosticsFromEvents(events: List<AribEvent>): List<DescriptorDiagnostic> = events.flatMap { event ->
        parseDescriptorDiagnostics(event.descriptors.diagnostics.descriptorDiagnosticsCanonicalJson)
    }

    private fun parseDescriptorDiagnostics(raw: String): List<DescriptorDiagnostic> {
        val array = runCatching { JSONArray(raw.ifBlank { "[]" }) }.getOrNull() ?: return emptyList()
        return (0 until array.length()).mapNotNull { index ->
            val obj = array.optJSONObject(index) ?: return@mapNotNull null
            val scope = obj.optJSONObject("scope") ?: JSONObject()
            val descriptor = obj.optJSONObject("descriptor") ?: JSONObject()
            DescriptorDiagnostic(
                schema = obj.optString("schema"),
                schemaVersion = obj.optInt("schemaVersion", 0),
                severity = obj.optString("severity"),
                code = obj.optString("code"),
                scope = DescriptorDiagnosticScope(
                    pid = TsPid.fromOrNull(optIntOrNull(scope, "pid")),
                    tableId = optIntOrNull(scope, "tableId"),
                    tableIdExtension = optIntOrNull(scope, "tableIdExtension"),
                    version = optIntOrNull(scope, "version"),
                    sectionNumber = optIntOrNull(scope, "sectionNumber"),
                    originalNetwork = NetworkId16.fromOrNull(optIntOrNull(scope, "originalNetworkId")),
                    transportStream = TransportStreamId16.fromOrNull(optIntOrNull(scope, "transportStreamId")),
                    service = ServiceId16.fromOrNull(optIntOrNull(scope, "serviceId")),
                    eventId = optIntOrNull(scope, "eventId"),
                ),
                descriptor = DescriptorDiagnosticDescriptor(
                    tag = descriptor.optInt("tag", -1),
                    name = optStringOrNull(descriptor, "name"),
                    offset = descriptor.optInt("offset", -1),
                    declaredLength = descriptor.optInt("declaredLength", -1),
                    actualRemainingLength = descriptor.optInt("actualRemainingLength", -1),
                    parseStatus = descriptor.optString("parseStatus"),
                    rawPrefixHex = descriptor.optString("rawPrefixHex"),
                ),
                message = obj.optString("message"),
                rawJson = obj.toString(),
            )
        }
    }

    private fun readNativeTransaction(takeUpdateWindows: Boolean): NativeTransaction {
        check(handle != 0L) { "ネイティブ解析器は終了済みです" }
        return parseNativeTransactionJson(nativeSnapshotBulkJson(handle, if (takeUpdateWindows) 1 else 0))
    }

    private fun parseNativeTransactionJson(raw: String): NativeTransaction {
        val root = JSONObject(raw.ifBlank { "{}" })
        val services = parseServices(root.optJSONArray("services"))
        val events = attachServiceComponentsToEvents(parseEvents(root.optJSONArray("events")), services)
        return NativeTransaction(
            ingestSequence = root.optLong("ingestSequence", 0L),
            discoveryStage = root.optInt("discoveryStage", SiDiscoveryStage.INCOMPLETE),
            tableRequirements = parseTableRequirements(root.optJSONArray("tableRequirements")),
            services = services,
            caMetadata = parseCaMetadataList(root.optJSONArray("caMetadata")),
            malformedCaDescriptorDiagnostics = parseMalformedCaDescriptorDiagnostics(root.optJSONArray("malformedCaDescriptorDiagnostics")),
            malformedCaDescriptorCountByServiceId = parseMalformedCaDescriptorCounts(root.optJSONArray("malformedCaDescriptorCounts")),
            pmtPidMappings = parsePmtPidMappings(root.optJSONArray("pmtPidMappings")),
            sdtActualTransports = parseTransports(root.optJSONArray("sdtActualTransports")),
            events = events,
            epgUpdateWindows = parseEpgUpdateWindows(root.optJSONArray("epgUpdateWindows")),
            serviceSemanticFacts = parseServiceSemanticFacts(root.optJSONArray("serviceSemanticFacts")),
            parserDiagnostics = parseParserDiagnostics(root.optJSONArray("parserDiagnostics")),
        )
    }

    private fun parseStringArray(array: JSONArray?): List<String> = (0 until (array?.length() ?: 0)).mapNotNull { index -> array!!.optString(index).takeIf { it.isNotBlank() } }

    private fun parseTableRequirements(array: JSONArray?): List<TableRequirementStatus> =
        (0 until (array?.length() ?: 0)).mapNotNull { index ->
            val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
            val component = obj.optString("component").takeIf { it.isNotBlank() }
                ?: return@mapNotNull null
            TableRequirementStatus(
                component = component,
                originalNetworkId = optIntOrNull(obj, "originalNetworkId"),
                transportStreamId = optIntOrNull(obj, "transportStreamId"),
                serviceId = optIntOrNull(obj, "serviceId"),
                required = obj.optBoolean("required"),
                complete = obj.optBoolean("complete"),
            )
        }
    private fun optIntOrNull(obj: JSONObject, key: String): Int? = if (obj.isNull(key)) null else obj.optInt(key)
    private fun optStringOrNull(obj: JSONObject, key: String): String? = obj.optString(key).takeIf { it.isNotBlank() }
    private fun optBoolOrNull(obj: JSONObject, key: String): Boolean? = if (obj.isNull(key)) null else obj.optBoolean(key)

    private fun hexToBytes(hex: String): ByteArray {
        if (hex.length % 2 != 0) return ByteArray(0)
        return ByteArray(hex.length / 2) { index -> hex.substring(index * 2, index * 2 + 2).toIntOrNull(16)?.toByte() ?: 0 }
    }

    private fun serviceKeyFrom(obj: JSONObject): ServiceKey? = ServiceKey.fromOrNull(
        originalNetworkId = obj.optInt("originalNetworkId", -1),
        transportStreamId = obj.optInt("transportStreamId", -1),
        serviceId = obj.optInt("serviceId", -1),
    )

    private fun parseServices(array: JSONArray?): List<AribService> = (0 until (array?.length() ?: 0)).mapNotNull { index ->
        val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
        val key = serviceKeyFrom(obj) ?: return@mapNotNull null
        val streams = parseStreams(obj.optJSONArray("streams"))
        val caDescriptors = parseCaDescriptors(obj.optJSONArray("serviceScopedCaDescriptors"))
        return@mapNotNull AribService(
            serviceKey = key,
            name = obj.optString("name"),
            providerName = obj.optString("providerName"),
            serviceType = optIntOrNull(obj, "serviceType"),
            pmtPid = TsPid.fromOrNull(optIntOrNull(obj, "pmtPid")),
            pcrPid = TsPid.fromOrNull(optIntOrNull(obj, "pcrPid")),
            freeCaMode = optBoolOrNull(obj, "freeCaMode"),
            streams = streams,
            serviceScopedCaDescriptors = caDescriptors,
        )
    }

    private fun parseStreams(array: JSONArray?): List<AribElementaryStream> = (0 until (array?.length() ?: 0)).mapNotNull { index ->
        val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
        val pid = TsPid.fromOrNull(obj.optInt("elementaryPid", -1))
        val streamType = obj.optInt("streamType", -1)
        if (pid == null || streamType < 0) null else AribElementaryStream(
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
            caPid = TsPid.fromOrNull(optIntOrNull(obj, "caPid")),
            scope = if (obj.optString("scope") == "ES") CaDescriptorScope.ES else CaDescriptorScope.PROGRAM,
            esPid = TsPid.fromOrNull(optIntOrNull(obj, "esPid")),
            rawDescriptor = hexToBytes(obj.optString("rawDescriptorHex")),
        )
    }

    private fun parsePmtPidMappings(array: JSONArray?): List<PmtPidMapping> = (0 until (array?.length() ?: 0)).mapNotNull { index ->
        val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
        val key = serviceKeyFrom(obj) ?: return@mapNotNull null
        val pmtPid = TsPid.fromOrNull(obj.optInt("pmtPid", -1))
        if (pmtPid == null) null else PmtPidMapping(key, pmtPid)
    }

    private fun parseTransports(array: JSONArray?): List<AribTransport> = (0 until (array?.length() ?: 0)).mapNotNull { index ->
        val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
        val onid = NetworkId16.fromOrNull(obj.optInt("originalNetworkId", -1))
        val tsid = TransportStreamId16.fromOrNull(obj.optInt("transportStreamId", -1))
        if (onid == null || tsid == null) null else AribTransport(
            originalNetwork = onid,
            transportStream = tsid,
            networkName = obj.optString("networkName"),
            transportStreamName = obj.optString("transportStreamName"),
            remoteControlKeyId = optIntOrNull(obj, "remoteControlKeyId"),
        )
    }

    private fun attachServiceComponentsToEvents(events: List<AribEvent>, services: List<AribService>): List<AribEvent> {
        if (events.isEmpty() || services.isEmpty()) return events
        val componentsByService = services.associate { it.serviceKey to componentsForService(it) }
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
        val serviceKey = keyObj?.let { serviceKeyFrom(it) }
        val systemId = obj.optInt("caSystemId", -1)
        if (systemId < 0) null else CaMetadata(
            serviceKey = serviceKey,
            caSystemId = systemId,
            ecmPid = TsPid.fromOrNull(optIntOrNull(obj, "ecmPid")),
            emmPid = TsPid.fromOrNull(optIntOrNull(obj, "emmPid")),
            elementaryPid = TsPid.fromOrNull(optIntOrNull(obj, "elementaryPid")),
            privateData = hexToBytes(obj.optString("privateDataHex")),
            source = runCatching { CaMetadataSource.valueOf(obj.optString("source")) }.getOrDefault(CaMetadataSource.PROGRAM),
        )
    }

    private fun parseMalformedCaDescriptorDiagnostics(array: JSONArray?): List<MalformedCaDescriptorDiagnostic> = (0 until (array?.length() ?: 0)).mapNotNull { index ->
        val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
        MalformedCaDescriptorDiagnostic(
            pid = TsPid.fromOrNull(obj.optInt("pid", -1)) ?: return@mapNotNull null,
            tableId = obj.optInt("tableId", -1),
            tableIdExtension = optIntOrNull(obj, "tableIdExtension"),
            service = ServiceId16.fromOrNull(optIntOrNull(obj, "serviceId")),
            elementaryPid = TsPid.fromOrNull(optIntOrNull(obj, "elementaryPid")),
            scope = obj.optString("scope"),
            offset = obj.optInt("offset", -1),
            declaredLength = obj.optInt("declaredLength", -1),
            actualRemainingLength = obj.optInt("actualRemainingLength", -1),
            reason = obj.optString("reason"),
            rawPrefixHex = obj.optString("rawPrefixHex"),
        ).takeIf { it.tableId >= 0 && it.offset >= 0 && it.reason.isNotBlank() }
    }

    private fun parseMalformedCaDescriptorCounts(array: JSONArray?): Map<ServiceId16, Int> = (0 until (array?.length() ?: 0)).mapNotNull { index ->
        val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
        val serviceId = ServiceId16.fromOrNull(obj.optInt("serviceId", -1))
        val count = obj.optInt("count", 0)
        if (serviceId == null || count <= 0) null else serviceId to count
    }.toMap()

    private fun parseEvents(array: JSONArray?): List<AribEvent> = (0 until (array?.length() ?: 0)).mapNotNull { index ->
        val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
        val serviceKeyObj = obj.optJSONObject("serviceKey") ?: return@mapNotNull null
        val timingObj = obj.optJSONObject("timing") ?: return@mapNotNull null
        val key = serviceKeyFrom(serviceKeyObj) ?: return@mapNotNull null
        val eventId = obj.optInt("eventId", obj.optJSONObject("programKey")?.optInt("eventId", -1) ?: -1)
        val start = timingObj.optLong("startUtcMillis", 0L)
        val duration = timingObj.optLong("durationMillis", 0L)
        val descriptorsObj = obj.optJSONObject("descriptors") ?: JSONObject()
        val sourceObj = obj.optJSONObject("source") ?: JSONObject()
        val component = descriptorsObj.optJSONObject("component") ?: JSONObject()
        val audio = descriptorsObj.optJSONObject("audio") ?: JSONObject()
        val genres = descriptorsObj.optJSONObject("genres") ?: JSONObject()
        val freeCaMode = descriptorsObj.optJSONObject("freeCaMode") ?: JSONObject()
        val diagnostics = descriptorsObj.optJSONObject("diagnostics") ?: JSONObject()
        val series = descriptorsObj.optJSONObject("series")
        val descriptorDiagnosticsCanonicalJson = diagnostics.optString("descriptorDiagnosticsCanonicalJson", "[]")
        if (eventId < 0) return@mapNotNull null
        AribEvent(
            serviceKey = key,
            stableIdentity = obj.optString("stableIdentity"),
            eventId = eventId,
            timingState = timingObj.optString("state", "MALFORMED_TIMING"),
            rawStartTimeHex = timingObj.optString("rawStartTimeHex"),
            rawDurationHex = timingObj.optString("rawDurationHex"),
            startTimeMillis = start,
            durationMillis = duration,
            title = obj.optString("title"),
            description = obj.optString("description"),
            extendedDescription = obj.optString("extendedDescription"),
            eventScope = obj.optString("eventScope", "present_following"),
            source = AribProgramSource(
                pid = TsPid.fromOrNull(sourceObj.optInt("pid", 18)) ?: TsPid.EIT,
                tableId = sourceObj.optInt("tableId", 0x4e),
                version = sourceObj.optInt("version", 0),
                sectionNumber = sourceObj.optInt("sectionNumber", 0),
                lastSectionNumber = sourceObj.optInt("lastSectionNumber", 0),
            ),
            descriptors = AribEventDescriptors(
                extendedItems = parseExtendedItems(descriptorsObj.optJSONArray("extendedItems")),
                componentText = optStringOrNull(component, "text"),
                audioComponentText = optStringOrNull(audio, "componentText"),
                contentGenres = parseContentGenres(genres.optJSONArray("content")),
                genreSupplementText = optStringOrNull(genres, "genreSupplementText"),
                eventGroups = parseEventGroups(descriptorsObj.optJSONArray("eventGroups")),
                linkage = parseLinkage(descriptorsObj.optJSONArray("linkage")),
                scrambled = if (freeCaMode.isNull("scrambled")) null else freeCaMode.optBoolean("scrambled"),
                freeCaMode = parseFreeCaMode(freeCaMode),
                series = parseSeries(series),
                parentalRatings = parseParentalRatings(descriptorsObj.optJSONArray("parentalRatings")),
                components = parseComponents(descriptorsObj.optJSONObject("components")) ?: AribComponents(),
                diagnostics = AribEventDiagnostics(
                    summary = diagnostics.optString("summary"),
                    descriptorDiagnosticsCanonicalJson = descriptorDiagnosticsCanonicalJson,
                    textDiagnostics = parseTextDiagnosticSummary(diagnostics.optString("summary")),
                ),
            ),
        )
    }

    private fun parseExtendedItems(array: JSONArray?): List<AribExtendedItem> = (0 until (array?.length() ?: 0)).mapNotNull { index ->
        val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
        val languageCode = obj.optString("languageCode")
        if (languageCode.length != 3) null else AribExtendedItem(
            languageCode = languageCode,
            itemDescription = obj.optString("description"),
            itemText = obj.optString("text"),
        )
    }

    private fun parseParentalRatings(array: JSONArray?): List<AribParentalRating> = (0 until (array?.length() ?: 0)).mapNotNull { index ->
        val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
        val country = obj.optString("countryCode")
        val raw = obj.optInt("rawRatingByte", -1)
        if (country.isBlank() || raw < 0) null else AribParentalRating(
            countryCode = country,
            rawRatingByte = raw,
            parseStatus = obj.optString("parseStatus", "OK"),
        )
    }

    private fun parseContentGenres(array: JSONArray?): List<AribContentGenre> = (0 until (array?.length() ?: 0)).mapNotNull { index ->
        val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
        val level1 = obj.optInt("level1", -1)
        val level2 = obj.optInt("level2", -1)
        if (level1 < 0 || level2 < 0) null else AribContentGenre(
            level1 = level1,
            level2 = level2,
            userNibble = obj.optInt("userNibble", 0),
            aribName = obj.optString("aribName"),
            parseStatus = obj.optString("parseStatus", "OK"),
        )
    }


    private fun parseEventGroups(array: JSONArray?): List<AribEventGroup> =
    (0 until (array?.length() ?: 0)).mapNotNull { index ->
        val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
        val groupType = obj.optInt("groupType", -1)
        if (groupType !in 0..15) return@mapNotNull null
        val events = parseEventGroupReferences(obj.optJSONArray("events"))
        val otherNetworkEvents = parseOtherNetworkEventGroupReferences(obj.optJSONArray("otherNetworkEvents"))
        val privateDataHex = obj.optString("privateDataHex", "")
        if (!isEvenHex(privateDataHex)) return@mapNotNull null
        if (groupType == 4 || groupType == 5) {
  if (privateDataHex.isNotEmpty()) return@mapNotNull null
        } else if (otherNetworkEvents.isNotEmpty()) {
  return@mapNotNull null
        }
        AribEventGroup(
  groupType = groupType,
  events = events,
  otherNetworkEvents = otherNetworkEvents,
  privateDataHex = privateDataHex,
  parseStatus = obj.optString("parseStatus", "OK"),
        )
    }

private fun parseEventGroupReferences(array: JSONArray?): List<AribEventGroupReference> =
    (0 until (array?.length() ?: 0)).mapNotNull { index ->
        val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
        val service = ServiceId16.fromOrNull(obj.optInt("serviceId", -1)) ?: return@mapNotNull null
        val eventId = obj.optInt("eventId", -1).takeIf { it in 0..0xffff } ?: return@mapNotNull null
        AribEventGroupReference(service = service, eventId = eventId)
    }

private fun parseOtherNetworkEventGroupReferences(array: JSONArray?): List<AribOtherNetworkEventGroupReference> =
    (0 until (array?.length() ?: 0)).mapNotNull { index ->
        val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
        val originalNetwork = NetworkId16.fromOrNull(obj.optInt("originalNetworkId", -1)) ?: return@mapNotNull null
        val transportStream = TransportStreamId16.fromOrNull(obj.optInt("transportStreamId", -1)) ?: return@mapNotNull null
        val service = ServiceId16.fromOrNull(obj.optInt("serviceId", -1)) ?: return@mapNotNull null
        val eventId = obj.optInt("eventId", -1).takeIf { it in 0..0xffff } ?: return@mapNotNull null
        AribOtherNetworkEventGroupReference(
  originalNetwork = originalNetwork,
  transportStream = transportStream,
  service = service,
  eventId = eventId,
        )
    }

private fun isEvenHex(value: String): Boolean =
    value.length % 2 == 0 && value.all { it.isDigit() || it.lowercaseChar() in 'a'..'f' }

private fun parseLinkage(array: JSONArray?): List<AribLinkage> = (0 until (array?.length() ?: 0)).mapNotNull { index ->
        val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
        val key = ServiceKey.fromOrNull(
            originalNetworkId = obj.optInt("originalNetworkId", -1),
            transportStreamId = obj.optInt("transportStreamId", -1),
            serviceId = obj.optInt("serviceId", -1),
        ) ?: return@mapNotNull null
        AribLinkage(
            linkageType = obj.optInt("linkageType", -1),
            serviceKey = key,
            privateDataHex = obj.optString("privateDataHex", ""),
            parseStatus = obj.optString("parseStatus", "OK"),
        ).takeIf { it.linkageType >= 0 }
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
        val pid = TsPid.fromOrNull(obj.optInt("esPid", -1))
        if (pid == null && optIntOrNull(obj, "componentTag") == null) null else AribComponentEntry(
            esPid = pid ?: TsPid.PAT,
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
            captionServiceKind = optStringOrNull(obj, "captionServiceKind"),
            diagnosticCode = optStringOrNull(obj, "diagnosticCode"),
            main = optBoolOrNull(obj, "main"),
            multiLingual = optBoolOrNull(obj, "multiLingual"),
            qualityIndicator = optIntOrNull(obj, "qualityIndicator"),
            parseStatus = obj.optString("parseStatus", "OK"),
        )
    }

    private fun parseEpgUpdateWindows(array: JSONArray?): List<AribEpgUpdateWindow> = (0 until (array?.length() ?: 0)).mapNotNull { index ->
        val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
        val key = serviceKeyFrom(obj) ?: return@mapNotNull null
        val start = obj.optLong("windowStartMillis", -1L)
        val end = obj.optLong("windowEndMillis", -1L)
        if (start < 0L || end <= start) null else AribEpgUpdateWindow(
            serviceKey = key,
            windowStartMillis = start,
            windowEndMillis = end,
            validProgramStableIdentities = parseStringArray(obj.optJSONArray("validProgramStableIdentities")),
            deletionAuthoritative = obj.optBoolean("deletionAuthoritative", false),
        )
    }

    private fun parseServiceSemanticFacts(array: JSONArray?): List<ServiceSemanticFacts> = (0 until (array?.length() ?: 0)).mapNotNull { index ->
        val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
        val key = serviceKeyFrom(obj) ?: return@mapNotNull null
        val smd = obj.optJSONObject("smd") ?: JSONObject()
        ServiceSemanticFacts(
            serviceKey = key,
            serviceType = optIntOrNull(obj, "serviceType"),
            pmtPidResolved = obj.optBoolean("pmtPidResolved"),
            pmtParsed = obj.optBoolean("pmtParsed"),
            pcrPidResolved = obj.optBoolean("pcrPidResolved"),
            elementaryStreams = parseStreams(obj.optJSONArray("elementaryStreams")),
            requiresCas = obj.optBoolean("requiresCas"),
            caDescriptorsResolved = obj.optBoolean("caDescriptorsResolved"),
            freeCaMode = optBoolOrNull(obj, "freeCaMode"),
            smd = SmdSemanticFacts(
                descriptorPresent = smd.optBoolean("descriptorPresent"),
                syntaxValid = smd.optBoolean("syntaxValid"),
                systemManagementId = optIntOrNull(smd, "systemManagementId"),
                broadcastingFlag = optIntOrNull(smd, "broadcastingFlag"),
                broadcastingIdentifier = optIntOrNull(smd, "broadcastingIdentifier"),
                additionalBroadcastingIdentification = optIntOrNull(smd, "additionalBroadcastingIdentification"),
                additionalIdentificationInfoHex = smd.optString("additionalIdentificationInfoHex"),
                semanticState = smd.optString("semanticState", "UNDETERMINED_SMD"),
                diagnostic = optStringOrNull(smd, "diagnostic"),
            ),
            missingComponents = parseStringArray(obj.optJSONArray("missingComponents")),
            semanticDiagnostics = parseStringArray(obj.optJSONArray("semanticDiagnostics")),
        )
    }


    private fun parseParserDiagnostics(array: JSONArray?): List<ParserDiagnostic> = (0 until (array?.length() ?: 0)).mapNotNull { index ->
        val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
        val code = obj.optString("code").takeIf { it.isNotBlank() } ?: return@mapNotNull null
        ParserDiagnostic(
            code = code,
            message = obj.optString("message"),
            severity = optStringOrNull(obj, "severity"),
        )
    }

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
    private external fun nativeBuildProgramKey(onid: Int, tsid: Int, sid: Int, eventId: Int): String
    private external fun nativeNormalizeProgramProviderData(providerData: ByteArray): String
    private external fun nativeExtractProgramKeyResult(providerData: ByteArray): String
    private external fun nativeDecodeChannelProviderData(providerData: ByteArray): String
    private external fun nativeCreate(): Long
    private external fun nativeDestroy(handle: Long): Int
    private external fun nativeIngestSection(handle: Long, pid: Int, section: ByteArray): Int
    private external fun nativeLastStatus(handle: Long): Int
    private external fun nativeSetDiscoveryProfile(handle: Long, profile: Int): Int
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

        private fun componentsForService(service: AribService): AribComponents {
            val video = mutableListOf<AribComponentEntry>()
            val audio = mutableListOf<AribComponentEntry>()
            val subtitle = mutableListOf<AribComponentEntry>()
            val data = mutableListOf<AribComponentEntry>()
            service.streams.forEach { stream ->
                val videoCodec = RECOGNIZED_VIDEO_CODECS[stream.streamType]
                val audioCodec = RECOGNIZED_AUDIO_CODECS[stream.streamType]
                when {
                    videoCodec != null -> video += codecComponent(stream, videoCodec, r51Supported = R51_VIDEO_CODECS.containsKey(stream.streamType))
                    audioCodec != null -> audio += codecComponent(stream, audioCodec, r51Supported = R51_AUDIO_CODECS.containsKey(stream.streamType)).copy(
                        language = stream.languageCodes.firstOrNull(),
                        secondLanguage = stream.languageCodes.drop(1).firstOrNull(),
                    )
                    stream.isCaption || stream.dataComponentId in CAPTION_DATA_COMPONENT_IDS -> subtitle += AribComponentEntry(
                        esPid = stream.elementaryPid,
                        componentTag = stream.componentTag,
                        dataComponentId = stream.dataComponentId,
                        language = stream.languageCodes.firstOrNull(),
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

        fun componentsForServiceForTest(service: AribService): AribComponents = componentsForService(service)

        fun toComponentsObjectForServiceForTest(service: AribService): String = ProviderDataBridge.toComponentsObject(componentsForServiceForTest(service)).toString()

        private fun codecComponent(stream: AribElementaryStream, codec: String, r51Supported: Boolean): AribComponentEntry = AribComponentEntry(
            esPid = stream.elementaryPid,
            streamType = stream.streamType,
            componentTag = stream.componentTag,
            componentType = stream.componentType,
            codec = codec,
            diagnosticCode = if (r51Supported) "CODEC_SIGNALING_OBSERVED" else "UNSUPPORTED_R51_CODEC_SIGNALING",
            parseStatus = "OK",
        )

        fun isR51PlaybackSupportedVideoCodecForTest(streamType: Int): Boolean = R51_VIDEO_CODECS.containsKey(streamType)
        fun isRecognizedVideoCodecForTest(streamType: Int): Boolean = RECOGNIZED_VIDEO_CODECS.containsKey(streamType)
        fun isR51PlaybackSupportedAudioCodecForTest(streamType: Int): Boolean = R51_AUDIO_CODECS.containsKey(streamType)
        fun isRecognizedAudioCodecForTest(streamType: Int): Boolean = RECOGNIZED_AUDIO_CODECS.containsKey(streamType)
    }
}

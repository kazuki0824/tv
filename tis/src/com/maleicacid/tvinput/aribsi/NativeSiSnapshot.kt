@file:Suppress("LargeClass", "LongParameterList", "MaxLineLength", "TooManyFunctions")

package com.maleicacid.tvinput.aribsi

import com.maleicacid.tvinput.common.NetworkId16
import com.maleicacid.tvinput.common.ServiceId16
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.TransportStreamId16
import com.maleicacid.tvinput.common.TsPid

data class NativeSiSnapshot(
    val collectionGeneration: Long,
    val ingestSequence: Long,
    val discoveryStage: Int,
    val broadcastClock: AribBroadcastClockFact?,
    val tableRequirements: List<TableRequirementStatus>,
    val catCaMetadata: List<CaMetadata>,
    val malformedCaDescriptorDiagnostics: List<MalformedCaDescriptorDiagnostic>,
    val malformedCaDescriptorCountByServiceId: Map<ServiceId16, Int>,
    val transportSemanticFacts: List<AribTransport>,
    val events: List<AribEvent>,
    val eitInstances: List<EitInstanceState>,
    val serviceSemanticFacts: List<ServiceSemanticFacts>,
    val parserDiagnostics: List<ParserDiagnostic>,
) {
    val services: List<AribService> get() =
        serviceSemanticFacts.map { facts ->
            AribService(
                serviceKey = facts.serviceKey,
                name = facts.name,
                providerName = facts.providerName,
                serviceType = facts.serviceType,
                pmtPid = facts.pmtPid,
                pcrPid = facts.pcrPid,
                freeCaMode = facts.freeCaMode,
                streams = facts.elementaryStreams,
                serviceScopedCaDescriptors = facts.serviceScopedCaDescriptors,
            )
        }

    val caMetadata: List<CaMetadata> get() =
        serviceSemanticFacts.flatMap { facts ->
            facts.serviceScopedCaDescriptors.map { descriptor ->
                CaMetadata(
                    serviceKey = facts.serviceKey,
                    caSystemId = descriptor.caSystemId,
                    ecmPid = descriptor.caPid,
                    emmPid = null,
                    elementaryPid = descriptor.esPid,
                    privateData = descriptor.privateData,
                    source =
                        if (descriptor.scope == CaDescriptorScope.ES) {
                            CaMetadataSource.ELEMENTARY_STREAM
                        } else {
                            CaMetadataSource.PROGRAM
                        },
                )
            }
        } + catCaMetadata

    val pmtPids: Map<ServiceKey, TsPid> get() =
        serviceSemanticFacts
            .mapNotNull { facts ->
                facts.pmtPid?.let { facts.serviceKey to it }
            }.toMap()

    val actualTransports: List<AribTransport> get() =
        transportSemanticFacts.filter { it.sdtActual }
}

/**
 * JNI専用の機械的construction boundary。
 * fieldの意味・値域・nullable条件・enum選択はRust側が所有し、ここでは再判定しない。
 */
object NativeSiJvmFactory {
    @JvmStatic
    fun broadcastClock(
        tableId: Int,
        mjd: Int,
        millisOfDay: Long,
    ) = AribBroadcastClockFact(tableId, mjd, millisOfDay)

    @JvmStatic
    fun tableRequirement(
        component: String,
        originalNetworkId: Int?,
        transportStreamId: Int?,
        serviceId: Int?,
        required: Boolean,
        complete: Boolean,
    ) = TableRequirementStatus(component, originalNetworkId, transportStreamId, serviceId, required, complete)

    @JvmStatic
    fun caMetadata(
        originalNetworkId: Int?,
        transportStreamId: Int?,
        serviceId: Int?,
        caSystemId: Int,
        ecmPid: Int?,
        emmPid: Int?,
        elementaryPid: Int?,
        privateData: ByteArray,
        source: CaMetadataSource,
    ) = CaMetadata(
        serviceKey =
            if (originalNetworkId == null || transportStreamId == null || serviceId == null) {
                null
            } else {
                ServiceKey(originalNetworkId, transportStreamId, serviceId)
            },
        caSystemId = caSystemId,
        ecmPid = TsPid.fromOrNull(ecmPid),
        emmPid = TsPid.fromOrNull(emmPid),
        elementaryPid = TsPid.fromOrNull(elementaryPid),
        privateData = privateData,
        source = source,
    )

    @JvmStatic
    fun malformedCaDescriptorDiagnostic(
        pid: Int,
        tableId: Int,
        tableIdExtension: Int?,
        serviceId: Int?,
        elementaryPid: Int?,
        scope: String,
        offset: Int,
        declaredLength: Int,
        actualRemainingLength: Int,
        reason: String,
        rawPrefixHex: String,
    ) = MalformedCaDescriptorDiagnostic(
        pid = TsPid(pid),
        tableId = tableId,
        tableIdExtension = tableIdExtension,
        service = ServiceId16.fromOrNull(serviceId),
        elementaryPid = TsPid.fromOrNull(elementaryPid),
        scope = scope,
        offset = offset,
        declaredLength = declaredLength,
        actualRemainingLength = actualRemainingLength,
        reason = reason,
        rawPrefixHex = rawPrefixHex,
    )

    @JvmStatic
    fun malformedCaDescriptorCountMap(
        serviceIds: List<Int>,
        counts: List<Int>,
    ): Map<ServiceId16, Int> =
        serviceIds.zip(counts).associate { (serviceId, count) ->
            ServiceId16(serviceId) to count
        }

    @JvmStatic
    fun transport(
        originalNetworkId: Int,
        transportStreamId: Int,
        networkName: String?,
        transportStreamName: String?,
        sdtActual: Boolean,
        remoteControlKeyId: Int?,
    ) = AribTransport(
        originalNetwork = NetworkId16(originalNetworkId),
        transportStream = TransportStreamId16(transportStreamId),
        networkName = networkName,
        transportStreamName = transportStreamName,
        sdtActual = sdtActual,
        remoteControlKeyId = remoteControlKeyId,
    )

    @JvmStatic
    fun parserDiagnostic(
        code: String,
        message: String,
        severity: String?,
    ) = ParserDiagnostic(code, message, severity)

    @JvmStatic
    fun eitInstance(
        originalNetworkId: Int,
        transportStreamId: Int,
        serviceId: Int,
        tableId: Int,
        version: Int,
        currentNextIndicator: Boolean,
        lastSectionNumber: Int,
        receivedSections: List<Int>,
        missingSections: List<Int>,
        safeSections: List<Int>,
        complete: Boolean,
        inconsistent: Boolean,
    ) = EitInstanceState(
        serviceKey = ServiceKey(originalNetworkId, transportStreamId, serviceId),
        tableId = tableId,
        version = version,
        currentNextIndicator = currentNextIndicator,
        lastSectionNumber = lastSectionNumber,
        receivedSections = receivedSections,
        missingSections = missingSections,
        safeSections = safeSections,
        complete = complete,
        inconsistent = inconsistent,
    )

    @JvmStatic
    fun avcSignaling(
        profileIdc: Int,
        constraintFlags: Int,
        levelIdc: Int,
    ) = AribAvcSignaling(profileIdc, constraintFlags, levelIdc)

    @JvmStatic
    fun audioConfigHeader(
        audioObjectType: Int,
        samplingFrequency: Int,
        channelConfiguration: Int,
        extensionSamplingFrequency: Int?,
        coreAudioObjectType: Int?,
        channelCount: Int?,
    ) = AribAudioConfigHeader(
        audioObjectType,
        samplingFrequency,
        channelConfiguration,
        extensionSamplingFrequency,
        coreAudioObjectType,
        channelCount,
    )

    @JvmStatic
    fun codecFacts(
        avc: AribAvcSignaling?,
        audioConfigHex: String?,
        audioConfigHeader: AribAudioConfigHeader?,
        rawDescriptorsHex: String?,
        profileLevel: String?,
        resolved: Boolean,
    ) = AribCodecFacts(avc, audioConfigHex, audioConfigHeader, rawDescriptorsHex, profileLevel, resolved)

    @JvmStatic
    fun elementaryStream(
        elementaryPid: Int,
        streamType: Int,
        componentTag: Int?,
        componentType: Int?,
        streamContent: Int?,
        languageCodes: List<String>,
        dataComponentId: Int?,
        captionDmf: Int?,
        captionTiming: Int?,
        automaticPresentationOnReception: Boolean?,
        isCaption: Boolean,
        isSuperimpose: Boolean,
        codec: String?,
        codecKind: ElementaryStreamKind?,
        codecFacts: AribCodecFacts,
    ) = AribElementaryStream(
        elementaryPid = TsPid(elementaryPid),
        streamType = streamType,
        componentTag = componentTag,
        componentType = componentType,
        streamContent = streamContent,
        languageCodes = languageCodes,
        dataComponentId = dataComponentId,
        captionDmf = captionDmf,
        captionTiming = captionTiming,
        automaticPresentationOnReception = automaticPresentationOnReception,
        isCaption = isCaption,
        isSuperimpose = isSuperimpose,
        codec = codec,
        codecKind = codecKind,
        codecFacts = codecFacts,
    )

    @JvmStatic
    fun caDescriptor(
        caSystemId: Int,
        caPid: Int?,
        scope: CaDescriptorScope,
        esPid: Int?,
        rawDescriptor: ByteArray,
        privateData: ByteArray,
    ) = CaDescriptor(
        caSystemId = caSystemId,
        caPid = TsPid.fromOrNull(caPid),
        scope = scope,
        esPid = TsPid.fromOrNull(esPid),
        rawDescriptor = rawDescriptor,
        privateData = privateData,
    )

    @JvmStatic
    fun smdSemanticFacts(
        descriptorPresent: Boolean,
        syntaxValid: Boolean,
        systemManagementId: Int?,
        broadcastingFlag: Int?,
        broadcastingIdentifier: Int?,
        broadcastSystem: BroadcastSystem?,
        additionalBroadcastingIdentification: Int?,
        additionalIdentificationInfoHex: String,
        semanticState: SmdSemanticState,
        diagnostic: String?,
    ) = SmdSemanticFacts(
        descriptorPresent,
        syntaxValid,
        systemManagementId,
        broadcastingFlag,
        broadcastingIdentifier,
        broadcastSystem,
        additionalBroadcastingIdentification,
        additionalIdentificationInfoHex,
        semanticState,
        diagnostic,
    )

    @JvmStatic
    fun serviceSemanticFacts(
        originalNetworkId: Int,
        transportStreamId: Int,
        serviceId: Int,
        serviceType: Int?,
        pmtPidResolved: Boolean,
        pmtParsed: Boolean,
        pcrPidResolved: Boolean,
        elementaryStreams: List<AribElementaryStream>,
        requiresCas: Boolean,
        caDescriptorsResolved: Boolean,
        freeCaMode: Boolean?,
        smd: SmdSemanticFacts,
        missingComponents: List<String>,
        semanticDiagnostics: List<String>,
        name: String?,
        providerName: String?,
        pmtPid: Int?,
        pcrPid: Int?,
        serviceScopedCaDescriptors: List<CaDescriptor>,
        casFactsCanonicalJson: String?,
    ) = ServiceSemanticFacts(
        serviceKey = ServiceKey(originalNetworkId, transportStreamId, serviceId),
        serviceType = serviceType,
        pmtPidResolved = pmtPidResolved,
        pmtParsed = pmtParsed,
        pcrPidResolved = pcrPidResolved,
        elementaryStreams = elementaryStreams,
        requiresCas = requiresCas,
        caDescriptorsResolved = caDescriptorsResolved,
        freeCaMode = freeCaMode,
        smd = smd,
        missingComponents = missingComponents,
        semanticDiagnostics = semanticDiagnostics,
        name = name,
        providerName = providerName,
        pmtPid = TsPid.fromOrNull(pmtPid),
        pcrPid = TsPid.fromOrNull(pcrPid),
        serviceScopedCaDescriptors = serviceScopedCaDescriptors,
        casFactsCanonicalJson = casFactsCanonicalJson,
    )

    @JvmStatic
    fun shortEvent(
        languageCode: String,
        title: String,
        text: String,
        parseStatus: SiParseStatus,
    ) = AribShortEventText(languageCode, title, text, parseStatus)

    @JvmStatic
    fun extendedText(
        languageCode: String,
        text: String,
        parseStatus: SiParseStatus,
    ) = AribExtendedEventText(languageCode, text, parseStatus)

    @JvmStatic
    fun extendedItem(
        languageCode: String,
        description: String,
        text: String,
    ) = AribExtendedItem(languageCode, description, text)

    @JvmStatic
    fun parentalRating(
        countryCode: String,
        rawRatingByte: Int,
        parseStatus: SiParseStatus,
    ) = AribParentalRating(countryCode, rawRatingByte, parseStatus)

    @JvmStatic
    fun contentGenre(
        level1: Int,
        level2: Int,
        userNibble: Int,
        aribName: String,
        parseStatus: SiParseStatus,
    ) = AribContentGenre(level1, level2, userNibble, aribName, parseStatus)

    @JvmStatic
    fun eventGroupReference(
        serviceId: Int,
        eventId: Int,
    ) = AribEventGroupReference(ServiceId16(serviceId), eventId)

    @JvmStatic
    fun otherNetworkEventGroupReference(
        originalNetworkId: Int,
        transportStreamId: Int,
        serviceId: Int,
        eventId: Int,
    ) = AribOtherNetworkEventGroupReference(
        NetworkId16(originalNetworkId),
        TransportStreamId16(transportStreamId),
        ServiceId16(serviceId),
        eventId,
    )

    @JvmStatic
    fun eventGroup(
        groupType: Int,
        events: List<AribEventGroupReference>,
        otherNetworkEvents: List<AribOtherNetworkEventGroupReference>,
        privateDataHex: String,
        parseStatus: SiParseStatus,
    ) = AribEventGroup(groupType, events, otherNetworkEvents, privateDataHex, parseStatus)

    @JvmStatic
    fun componentGroup(
        componentGroupId: Int,
        componentTags: List<Int>,
    ) = AribComponentGroup(componentGroupId, componentTags)

    @JvmStatic
    fun componentGroupDescriptor(
        componentGroupType: Int,
        groups: List<AribComponentGroup>,
        parseStatus: SiParseStatus,
    ) = AribComponentGroupDescriptor(componentGroupType, groups, parseStatus)

    @JvmStatic
    fun linkage(
        linkageType: Int,
        originalNetworkId: Int,
        transportStreamId: Int,
        serviceId: Int,
        privateDataPrefixHex: String,
        parseStatus: SiParseStatus,
    ) = AribLinkage(
        linkageType,
        ServiceKey(originalNetworkId, transportStreamId, serviceId),
        privateDataPrefixHex,
        parseStatus,
    )

    @JvmStatic
    fun freeCaMode(
        raw: Int?,
        scrambled: Boolean?,
        parseStatus: SiParseStatus,
    ) = AribFreeCaMode(raw, scrambled, parseStatus)

    @JvmStatic
    fun series(
        seriesId: Int?,
        repeatLabel: Int,
        programPattern: Int,
        expireDateValid: Boolean,
        expireDate: Int?,
        episodeNumber: Int?,
        lastEpisodeNumber: Int?,
        name: String?,
        parseStatus: SiParseStatus,
    ) = AribSeries(
        seriesId,
        repeatLabel,
        programPattern,
        expireDateValid,
        expireDate,
        episodeNumber,
        lastEpisodeNumber,
        name,
        parseStatus,
    )

    @JvmStatic
    fun videoComponentEntry(
        streamContent: Int?,
        componentTag: Int?,
        componentType: Int?,
        language: String?,
        text: String?,
        sourceDescriptor: String?,
        resolution: String?,
        scan: String?,
        aspect: String?,
        profileLevel: String?,
        parseStatus: SiParseStatus,
    ) = AribComponentEntry(
        esPid = null,
        streamContent = streamContent,
        componentTag = componentTag,
        componentType = componentType,
        language = language,
        text = text,
        sourceDescriptor = sourceDescriptor,
        resolution = resolution,
        scan = scan,
        aspect = aspect,
        profileLevel = profileLevel,
        parseStatus = parseStatus,
    )

    @JvmStatic
    fun audioComponentEntry(
        streamType: Int?,
        streamContent: Int?,
        componentTag: Int?,
        componentType: Int?,
        language: String?,
        secondLanguage: String?,
        channelConfiguration: String?,
        simulcastGroupTag: Int?,
        samplingRate: Int?,
        samplingInfo: String?,
        text: String?,
        sourceDescriptor: String?,
        main: Boolean?,
        multiLingual: Boolean?,
        qualityIndicator: Int?,
        parseStatus: SiParseStatus,
        channelCount: Int?,
        sampleRateHz: Int?,
        audioDescription: Boolean?,
        hardOfHearing: Boolean?,
        dualMono: Boolean?,
    ) = AribComponentEntry(
        esPid = null,
        streamType = streamType,
        streamContent = streamContent,
        componentTag = componentTag,
        componentType = componentType,
        language = language,
        secondLanguage = secondLanguage,
        channelConfiguration = channelConfiguration,
        simulcastGroupTag = simulcastGroupTag,
        samplingRate = samplingRate,
        samplingInfo = samplingInfo,
        text = text,
        sourceDescriptor = sourceDescriptor,
        main = main,
        multiLingual = multiLingual,
        qualityIndicator = qualityIndicator,
        parseStatus = parseStatus,
        channelCount = channelCount,
        sampleRateHz = sampleRateHz,
        audioDescription = audioDescription,
        hardOfHearing = hardOfHearing,
        dualMono = dualMono,
    )

    @JvmStatic
    fun components(
        video: List<AribComponentEntry>,
        audio: List<AribComponentEntry>,
    ) = AribComponents(video = video, audio = audio)

    @JvmStatic
    fun truncatedDescriptorLoop(
        declaredLength: Int,
        rawBytesHex: String,
        parseStatus: SiParseStatus,
    ) = AribTruncatedDescriptorLoop(declaredLength, rawBytesHex, parseStatus)

    @JvmStatic
    fun descriptorDiagnostic(
        schema: String,
        schemaVersion: Int,
        severity: String,
        code: String,
        pid: Int?,
        tableId: Int?,
        tableIdExtension: Int?,
        version: Int?,
        sectionNumber: Int?,
        originalNetworkId: Int?,
        transportStreamId: Int?,
        serviceId: Int?,
        eventId: Int?,
        tag: Int,
        name: String?,
        offset: Int,
        declaredLength: Int,
        actualRemainingLength: Int,
        parseStatus: String,
        rawPrefixHex: String,
        message: String,
    ) = DescriptorDiagnostic(
        schema,
        schemaVersion,
        severity,
        code,
        DescriptorDiagnosticScope(
            pid = TsPid.fromOrNull(pid),
            tableId = tableId,
            tableIdExtension = tableIdExtension,
            version = version,
            sectionNumber = sectionNumber,
            originalNetwork = NetworkId16.fromOrNull(originalNetworkId),
            transportStream = TransportStreamId16.fromOrNull(transportStreamId),
            service = ServiceId16.fromOrNull(serviceId),
            eventId = eventId,
        ),
        DescriptorDiagnosticDescriptor(
            tag,
            name,
            offset,
            declaredLength,
            actualRemainingLength,
            parseStatus,
            rawPrefixHex,
        ),
        message,
    )

    @JvmStatic
    fun eventDiagnostics(
        summary: String,
        descriptorDiagnostics: List<DescriptorDiagnostic>,
        descriptorDiagnosticsCanonicalJson: String,
        descriptorFactsCanonicalJson: String?,
        textDiagnostics: List<String>,
        truncatedDescriptorLoop: AribTruncatedDescriptorLoop?,
    ) = AribEventDiagnostics(
        summary = summary,
        descriptorDiagnostics = descriptorDiagnostics,
        descriptorDiagnosticsCanonicalJson = descriptorDiagnosticsCanonicalJson,
        descriptorFactsCanonicalJson = descriptorFactsCanonicalJson,
        textDiagnostics = textDiagnostics,
        truncatedDescriptorLoop = truncatedDescriptorLoop,
    )

    @JvmStatic
    fun eventDescriptors(
        shortEvents: List<AribShortEventText>,
        extendedTexts: List<AribExtendedEventText>,
        extendedItems: List<AribExtendedItem>,
        componentText: String?,
        audioComponentText: String?,
        contentGenres: List<AribContentGenre>,
        genreSupplementText: String?,
        eventGroups: List<AribEventGroup>,
        componentGroups: List<AribComponentGroupDescriptor>,
        linkage: List<AribLinkage>,
        freeCaMode: AribFreeCaMode?,
        series: AribSeries?,
        seriesCandidates: List<AribSeries>,
        seriesCandidatesCanonicalJson: String?,
        parentalRatings: List<AribParentalRating>,
        components: AribComponents,
        diagnostics: AribEventDiagnostics,
    ) = AribEventDescriptors(
        shortEvents = shortEvents,
        extendedTexts = extendedTexts,
        extendedItems = extendedItems,
        componentText = componentText,
        audioComponentText = audioComponentText,
        contentGenres = contentGenres,
        genreSupplementText = genreSupplementText,
        eventGroups = eventGroups,
        componentGroups = componentGroups,
        linkage = linkage,
        scrambled = freeCaMode?.scrambled,
        freeCaMode = freeCaMode,
        series = series,
        seriesCandidates = seriesCandidates,
        seriesCandidatesCanonicalJson = seriesCandidatesCanonicalJson,
        parentalRatings = parentalRatings,
        components = components,
        diagnostics = diagnostics,
    )

    @JvmStatic
    fun programSource(
        pid: Int,
        tableId: Int,
        version: Int,
        sectionNumber: Int,
        lastSectionNumber: Int,
    ) = AribProgramSource(TsPid(pid), tableId, version, sectionNumber, lastSectionNumber)

    @JvmStatic
    fun event(
        originalNetworkId: Int,
        transportStreamId: Int,
        serviceId: Int,
        stableIdentity: String?,
        eventId: Int,
        timingState: EitTimingState,
        rawStartTimeHex: String,
        rawDurationHex: String,
        startTimeMillis: Long,
        durationMillis: Long,
        title: String,
        description: String,
        extendedDescription: String,
        eventScope: String,
        source: AribProgramSource,
        descriptors: AribEventDescriptors,
    ) = AribEvent(
        serviceKey = ServiceKey(originalNetworkId, transportStreamId, serviceId),
        stableIdentity = stableIdentity,
        eventId = eventId,
        timingState = timingState,
        rawStartTimeHex = rawStartTimeHex,
        rawDurationHex = rawDurationHex,
        startTimeMillis = startTimeMillis,
        durationMillis = durationMillis,
        title = title,
        description = description,
        extendedDescription = extendedDescription,
        eventScope = eventScope,
        source = source,
        descriptors = descriptors,
    )

    @JvmStatic
    fun snapshot(
        collectionGeneration: Long,
        ingestSequence: Long,
        discoveryStage: Int,
        broadcastClock: AribBroadcastClockFact?,
        tableRequirements: List<TableRequirementStatus>,
        catCaMetadata: List<CaMetadata>,
        malformedCaDescriptorDiagnostics: List<MalformedCaDescriptorDiagnostic>,
        malformedCaDescriptorCountByServiceId: Map<ServiceId16, Int>,
        transportSemanticFacts: List<AribTransport>,
        events: List<AribEvent>,
        eitInstances: List<EitInstanceState>,
        serviceSemanticFacts: List<ServiceSemanticFacts>,
        parserDiagnostics: List<ParserDiagnostic>,
    ) = NativeSiSnapshot(
        collectionGeneration,
        ingestSequence,
        discoveryStage,
        broadcastClock,
        tableRequirements,
        catCaMetadata,
        malformedCaDescriptorDiagnostics,
        malformedCaDescriptorCountByServiceId,
        transportSemanticFacts,
        events,
        eitInstances,
        serviceSemanticFacts,
        parserDiagnostics,
    )
}

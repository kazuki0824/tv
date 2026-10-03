@file:Suppress("LongMethod", "MagicNumber", "MaxLineLength", "TooManyFunctions")

package com.maleicacid.tvinput.aribsi

import com.maleicacid.tvinput.aribsi.generated.AudioComponentDto
import com.maleicacid.tvinput.aribsi.generated.BroadcastSystemDto
import com.maleicacid.tvinput.aribsi.generated.BulkSnapshotDto
import com.maleicacid.tvinput.aribsi.generated.CaDescriptorScopeDto
import com.maleicacid.tvinput.aribsi.generated.CaMetadataSourceDto
import com.maleicacid.tvinput.aribsi.generated.CodecFactsDto
import com.maleicacid.tvinput.aribsi.generated.ComponentGroupDescriptorDto
import com.maleicacid.tvinput.aribsi.generated.ContentGenreDto
import com.maleicacid.tvinput.aribsi.generated.DescriptorDiagnosticDto
import com.maleicacid.tvinput.aribsi.generated.EitInstanceDto
import com.maleicacid.tvinput.aribsi.generated.EitTimingStateDto
import com.maleicacid.tvinput.aribsi.generated.ElementaryStreamDto
import com.maleicacid.tvinput.aribsi.generated.ElementaryStreamKindDto
import com.maleicacid.tvinput.aribsi.generated.EventDescriptorsDto
import com.maleicacid.tvinput.aribsi.generated.EventDto
import com.maleicacid.tvinput.aribsi.generated.EventGroupDto
import com.maleicacid.tvinput.aribsi.generated.FreeCaModeDto
import com.maleicacid.tvinput.aribsi.generated.LinkageDto
import com.maleicacid.tvinput.aribsi.generated.ServiceCaDescriptorDto
import com.maleicacid.tvinput.aribsi.generated.ServiceKeyDto
import com.maleicacid.tvinput.aribsi.generated.ServiceSemanticFactsDto
import com.maleicacid.tvinput.aribsi.generated.ServiceRegistrationSnapshotDto
import com.maleicacid.tvinput.aribsi.generated.SiParseStatusDto
import com.maleicacid.tvinput.aribsi.generated.SmdSemanticStateDto
import com.maleicacid.tvinput.aribsi.generated.VideoComponentDto
import com.maleicacid.tvinput.common.NetworkId16
import com.maleicacid.tvinput.common.ServiceId16
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.TransportStreamId16
import com.maleicacid.tvinput.common.TsPid

private fun String.hexBytes(): ByteArray = chunked(2).map { it.toInt(16).toByte() }.toByteArray()

private fun ServiceKeyDto.toDomain(): ServiceKey = ServiceKey(originalNetworkId, transportStreamId, serviceId)

private fun SiParseStatusDto.toDomain(): SiParseStatus =
    when (this) {
        SiParseStatusDto.Ok -> SiParseStatus.OK
        SiParseStatusDto.MalformedLength -> SiParseStatus.MALFORMED_LENGTH
        SiParseStatusDto.TruncatedDescriptor -> SiParseStatus.TRUNCATED_DESCRIPTOR
        SiParseStatusDto.UnsupportedValue -> SiParseStatus.UNSUPPORTED_VALUE
        SiParseStatusDto.InvalidSequence -> SiParseStatus.INVALID_SEQUENCE
        SiParseStatusDto.Unresolved -> SiParseStatus.UNRESOLVED
    }

private fun EitTimingStateDto.toDomain(): EitTimingState =
    when (this) {
        EitTimingStateDto.Defined -> EitTimingState.DEFINED
        EitTimingStateDto.UndefinedTime -> EitTimingState.UNDEFINED_TIME
        EitTimingStateDto.BothTimingUndefined -> EitTimingState.BOTH_TIMING_UNDEFINED
        EitTimingStateDto.MalformedTiming -> EitTimingState.MALFORMED_TIMING
    }

private fun ElementaryStreamKindDto.toDomain(): ElementaryStreamKind =
    when (this) {
        ElementaryStreamKindDto.Video -> ElementaryStreamKind.VIDEO
        ElementaryStreamKindDto.Audio -> ElementaryStreamKind.AUDIO
    }

private fun BroadcastSystemDto.toDomain(): BroadcastSystem =
    when (this) {
        BroadcastSystemDto.IsdbT -> BroadcastSystem.ISDB_T
        BroadcastSystemDto.IsdbSBs -> BroadcastSystem.ISDB_S_BS
        BroadcastSystemDto.IsdbS110Cs -> BroadcastSystem.ISDB_S_110CS
    }

private fun SmdSemanticStateDto.toDomain(): SmdSemanticState =
    when (this) {
        SmdSemanticStateDto.SupportedBroadcast -> SmdSemanticState.SUPPORTED_BROADCAST
        SmdSemanticStateDto.NonBroadcast -> SmdSemanticState.NON_BROADCAST
        SmdSemanticStateDto.UndefinedBroadcastClass -> SmdSemanticState.UNDEFINED_BROADCAST_CLASS
        SmdSemanticStateDto.UnsupportedBroadcastSystem -> SmdSemanticState.UNSUPPORTED_BROADCAST_SYSTEM
        SmdSemanticStateDto.UndeterminedSmd -> SmdSemanticState.UNDETERMINED_SMD
    }

private fun CaDescriptorScopeDto.toDomain(): CaDescriptorScope =
    when (this) {
        CaDescriptorScopeDto.Program -> CaDescriptorScope.PROGRAM
        CaDescriptorScopeDto.Es -> CaDescriptorScope.ES
    }

private fun CaMetadataSourceDto.toDomain(): CaMetadataSource =
    when (this) {
        CaMetadataSourceDto.Program -> CaMetadataSource.PROGRAM
        CaMetadataSourceDto.ElementaryStream -> CaMetadataSource.ELEMENTARY_STREAM
        CaMetadataSourceDto.Cat -> CaMetadataSource.CAT
    }

private fun CodecFactsDto.toDomain(): AribCodecFacts =
    AribCodecFacts(
        avc = avc?.let { AribAvcSignaling(it.profileIdc, it.constraintFlags, it.levelIdc) },
        audioConfigHex = audioConfigHex,
        audioConfigHeader =
            audioConfigHeader?.let {
                AribAudioConfigHeader(
                    audioObjectType = it.audioObjectType,
                    samplingFrequency = it.samplingFrequency,
                    channelConfiguration = it.channelConfiguration,
                    extensionSamplingFrequency = it.extensionSamplingFrequency,
                    coreAudioObjectType = it.coreAudioObjectType,
                    channelCount = it.channelCount,
                )
            },
        rawDescriptorsHex = rawDescriptorsHex,
        profileLevel = profileLevel,
        resolved = resolved,
    )

private fun ElementaryStreamDto.toDomain(): AribElementaryStream =
    AribElementaryStream(
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
        codecKind = codecKind?.toDomain(),
        codecFacts = codecFacts.toDomain(),
    )

private fun ServiceCaDescriptorDto.toDomain(): CaDescriptor =
    CaDescriptor(
        caSystemId = caSystemId,
        caPid = TsPid(caPid),
        scope = scope.toDomain(),
        esPid = esPid?.let(::TsPid),
        rawDescriptor = rawDescriptorHex.hexBytes(),
        privateData = privateDataHex.hexBytes(),
    )

private fun ServiceSemanticFactsDto.toDomain(): ServiceSemanticFacts =
    ServiceSemanticFacts(
        serviceKey = ServiceKey(originalNetworkId, transportStreamId, serviceId),
        serviceType = serviceType,
        pmtPidResolved = pmtPidResolved,
        pmtParsed = pmtParsed,
        pcrPidResolved = pcrPidResolved,
        elementaryStreams = elementaryStreams.map { it.toDomain() },
        requiresCas = requiresCas,
        caDescriptorsResolved = caDescriptorsResolved,
        freeCaMode = freeCaMode,
        smd =
            SmdSemanticFacts(
                descriptorPresent = smd.descriptorPresent,
                syntaxValid = smd.syntaxValid,
                systemManagementId = smd.systemManagementId,
                broadcastingFlag = smd.broadcastingFlag,
                broadcastingIdentifier = smd.broadcastingIdentifier,
                broadcastSystem = smd.broadcastSystem?.toDomain(),
                additionalBroadcastingIdentification = smd.additionalBroadcastingIdentification,
                additionalIdentificationInfoHex = smd.additionalIdentificationInfoHex,
                semanticState = smd.semanticState.toDomain(),
                diagnostic = smd.diagnostic,
            ),
        missingComponents = missingComponents,
        semanticDiagnostics = semanticDiagnostics,
        name = name,
        providerName = providerName,
        pmtPid = pmtPid?.let(::TsPid),
        pcrPid = pcrPid?.let(::TsPid),
        serviceScopedCaDescriptors = serviceScopedCaDescriptors.map { it.toDomain() },
        casFactsCanonicalJson = casFactsCanonicalJson,
    )

private fun EitInstanceDto.toDomain(): EitInstanceState =
    EitInstanceState(
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

private fun DescriptorDiagnosticDto.toDomain(): DescriptorDiagnostic =
    DescriptorDiagnostic(
        schema = schema,
        schemaVersion = schemaVersion,
        severity = severity,
        code = code,
        scope =
            DescriptorDiagnosticScope(
                pid = scope.pid?.let(::TsPid),
                tableId = scope.tableId,
                tableIdExtension = scope.tableIdExtension,
                version = scope.version,
                sectionNumber = scope.sectionNumber,
                originalNetwork = scope.originalNetworkId?.let(::NetworkId16),
                transportStream = scope.transportStreamId?.let(::TransportStreamId16),
                service = scope.serviceId?.let(::ServiceId16),
                eventId = scope.eventId,
            ),
        descriptor =
            DescriptorDiagnosticDescriptor(
                tag = descriptor.tag,
                name = descriptor.name,
                offset = descriptor.offset,
                declaredLength = descriptor.declaredLength,
                actualRemainingLength = descriptor.actualRemainingLength,
                parseStatus = descriptor.parseStatus,
                rawPrefixHex = descriptor.rawPrefixHex,
            ),
        message = message,
    )

private fun VideoComponentDto.toDomain(): AribComponentEntry =
    AribComponentEntry(
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
        parseStatus = parseStatus.toDomain(),
    )

private fun AudioComponentDto.toDomain(): AribComponentEntry =
    AribComponentEntry(
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
        parseStatus = parseStatus.toDomain(),
        channelCount = channelCount,
        sampleRateHz = sampleRateHz,
        audioDescription = audioDescription,
        hardOfHearing = hardOfHearing,
        dualMono = dualMono,
    )

private fun ContentGenreDto.toDomain(): AribContentGenre = AribContentGenre(level1, level2, userNibble, aribName, parseStatus.toDomain())

private fun EventGroupDto.toDomain(): AribEventGroup =
    AribEventGroup(
        groupType = groupType,
        events = events.map { AribEventGroupReference(ServiceId16(it.serviceId), it.eventId) },
        otherNetworkEvents =
            otherNetworkEvents.map {
                AribOtherNetworkEventGroupReference(
                    NetworkId16(it.originalNetworkId),
                    TransportStreamId16(it.transportStreamId),
                    ServiceId16(it.serviceId),
                    it.eventId,
                )
            },
        privateDataHex = privateDataHex,
        parseStatus = parseStatus.toDomain(),
    )

private fun ComponentGroupDescriptorDto.toDomain(): AribComponentGroupDescriptor =
    AribComponentGroupDescriptor(
        componentGroupType = componentGroupType,
        groups = groups.map { AribComponentGroup(it.componentGroupId, it.componentTags) },
        parseStatus = parseStatus.toDomain(),
    )

private fun LinkageDto.toDomain(): AribLinkage =
    AribLinkage(
        linkageType = linkageType,
        serviceKey = ServiceKey(originalNetworkId, transportStreamId, serviceId),
        privateDataPrefixHex = privateDataPrefixHex,
        parseStatus = parseStatus.toDomain(),
    )

private fun FreeCaModeDto.toDomain(): AribFreeCaMode = AribFreeCaMode(raw, scrambled, parseStatus.toDomain())

private fun com.maleicacid.tvinput.aribsi.generated.SeriesDto.toDomain(): AribSeries =
    AribSeries(
        seriesId,
        repeatLabel,
        programPattern,
        expireDateValid,
        expireDate,
        episodeNumber,
        lastEpisodeNumber,
        name,
        parseStatus.toDomain(),
    )

private fun EventDescriptorsDto.toDomain(): AribEventDescriptors =
    AribEventDescriptors(
        shortEvents =
            shortEvents.map {
                AribShortEventText(it.languageCode, it.title, it.text, it.parseStatus.toDomain())
            },
        extendedTexts =
            extendedTexts.map {
                AribExtendedEventText(it.languageCode, it.text, it.parseStatus.toDomain())
            },
        extendedItems =
            extendedItems.map {
                AribExtendedItem(it.languageCode, it.description, it.text)
            },
        componentText = componentText,
        audioComponentText = audioComponentText,
        contentGenres = contentGenres.map { it.toDomain() },
        genreSupplementText = genreSupplementText,
        eventGroups = eventGroups.map { it.toDomain() },
        componentGroups = componentGroups.map { it.toDomain() },
        linkage = linkage.map { it.toDomain() },
        scrambled = freeCaMode?.scrambled,
        freeCaMode = freeCaMode?.toDomain(),
        series = series?.toDomain(),
        seriesCandidates = seriesCandidates.map { it.toDomain() },
        seriesCandidatesCanonicalJson = seriesCandidatesCanonicalJson,
        parentalRatings =
            parentalRatings.map {
                AribParentalRating(it.countryCode, it.rawRatingByte, it.parseStatus.toDomain())
            },
        components =
            AribComponents(
                video = components.video.map { it.toDomain() },
                audio = components.audio.map { it.toDomain() },
            ),
        diagnostics =
            AribEventDiagnostics(
                summary = diagnostics.summary,
                descriptorDiagnostics = diagnostics.descriptorDiagnostics.map { it.toDomain() },
                descriptorDiagnosticsCanonicalJson = diagnostics.descriptorDiagnosticsCanonicalJson,
                descriptorFactsCanonicalJson = diagnostics.descriptorFactsCanonicalJson,
                textDiagnostics = diagnostics.textDiagnostics,
                truncatedDescriptorLoop =
                    diagnostics.truncatedDescriptorLoop?.let {
                        AribTruncatedDescriptorLoop(
                            it.declaredLength,
                            it.rawBytesHex,
                            it.parseStatus.toDomain(),
                        )
                    },
            ),
    )

private fun EventDto.toDomain(): AribEvent =
    AribEvent(
        serviceKey = serviceKey.toDomain(),
        stableIdentity = stableIdentity,
        eventId = eventId,
        timingState = timingState.toDomain(),
        rawStartTimeHex = rawStartTimeHex,
        rawDurationHex = rawDurationHex,
        startTimeMillis = startTimeMillis,
        durationMillis = durationMillis,
        title = title,
        description = description,
        extendedDescription = extendedDescription,
        eventScope = eventScope,
        source =
            AribProgramSource(
                pid = TsPid(source.pid),
                tableId = source.tableId,
                version = source.version,
                sectionNumber = source.sectionNumber,
                lastSectionNumber = source.lastSectionNumber,
            ),
        descriptors = descriptors.toDomain(),
    )

internal fun ServiceRegistrationSnapshotDto.toDomainServiceRegistrationSnapshot(): ServiceRegistrationSnapshot {
    val serviceFacts = serviceSemanticFacts.map { it.toDomain() }
    val transports =
        transportSemanticFacts.map {
            AribTransport(
                originalNetwork = NetworkId16(it.originalNetworkId),
                transportStream = TransportStreamId16(it.transportStreamId),
                networkName = it.networkName,
                transportStreamName = it.transportStreamName,
                sdtActual = it.sdtActual,
                remoteControlKeyId = it.remoteControlKeyId,
            )
        }
    val actualTransports = transports.filter { it.sdtActual }
    return ServiceRegistrationSnapshot(
        discoveryStage = discoveryStage,
        tableRequirements =
            tableRequirements.map {
                TableRequirementStatus(
                    component = it.component,
                    originalNetworkId = it.originalNetworkId,
                    transportStreamId = it.transportStreamId,
                    serviceId = it.serviceId,
                    required = it.required,
                    complete = it.complete,
                )
            },
        services =
            serviceFacts.map { facts ->
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
            },
        actualTransports =
            actualTransports
                .map { TransportKey(it.originalNetwork, it.transportStream) }
                .toSet(),
        actualTransportMetadata = actualTransports,
        semanticFactsByServiceKey = serviceFacts.associateBy { it.serviceKey },
        diagnostics =
            parserDiagnostics.map {
                ParserDiagnostic(it.code, it.message, it.severity)
            },
        eitInstances = eitInstances.map { it.toDomain() },
    )
}

internal fun BulkSnapshotDto.toDomainSnapshot(): NativeSiSnapshot =
    NativeSiSnapshot(
        collectionGeneration = collectionGeneration,
        ingestSequence = ingestSequence,
        discoveryStage = discoveryStage,
        broadcastClock =
            broadcastClock?.let {
                AribBroadcastClockFact(it.tableId, it.mjd, it.millisOfDay)
            },
        tableRequirements =
            tableRequirements.map {
                TableRequirementStatus(
                    component = it.component,
                    originalNetworkId = it.originalNetworkId,
                    transportStreamId = it.transportStreamId,
                    serviceId = it.serviceId,
                    required = it.required,
                    complete = it.complete,
                )
            },
        catCaMetadata =
            catCaMetadata.map {
                CaMetadata(
                    serviceKey = it.serviceKey?.toDomain(),
                    caSystemId = it.caSystemId,
                    ecmPid = it.ecmPid?.let(::TsPid),
                    emmPid = it.emmPid?.let(::TsPid),
                    elementaryPid = it.elementaryPid?.let(::TsPid),
                    privateData = it.privateDataHex.hexBytes(),
                    source = it.source.toDomain(),
                )
            },
        malformedCaDescriptorDiagnostics =
            malformedCaDescriptorDiagnostics.map {
                MalformedCaDescriptorDiagnostic(
                    pid = TsPid(it.pid),
                    tableId = it.tableId,
                    tableIdExtension = it.tableIdExtension,
                    service = it.serviceId?.let(::ServiceId16),
                    elementaryPid = it.elementaryPid?.let(::TsPid),
                    scope = it.scope,
                    offset = it.offset,
                    declaredLength = it.declaredLength,
                    actualRemainingLength = it.actualRemainingLength,
                    reason = it.reason,
                    rawPrefixHex = it.rawPrefixHex,
                )
            },
        malformedCaDescriptorCountByServiceId =
            malformedCaDescriptorCounts.associate {
                ServiceId16(it.serviceId) to it.count
            },
        transportSemanticFacts =
            transportSemanticFacts.map {
                AribTransport(
                    originalNetwork = NetworkId16(it.originalNetworkId),
                    transportStream = TransportStreamId16(it.transportStreamId),
                    networkName = it.networkName,
                    transportStreamName = it.transportStreamName,
                    sdtActual = it.sdtActual,
                    remoteControlKeyId = it.remoteControlKeyId,
                )
            },
        events = events.map { it.toDomain() },
        eitInstances = eitInstances.map { it.toDomain() },
        serviceSemanticFacts = serviceSemanticFacts.map { it.toDomain() },
        parserDiagnostics =
            parserDiagnostics.map {
                ParserDiagnostic(it.code, it.message, it.severity)
            },
    )

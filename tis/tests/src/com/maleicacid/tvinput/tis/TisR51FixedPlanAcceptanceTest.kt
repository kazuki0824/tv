package com.maleicacid.tvinput.tis

import android.content.ContentValues
import android.media.tv.TvContentRating
import android.media.tv.TvContract
import android.media.tv.TvInputService
import android.media.tv.TvTrackInfo
import android.media.tv.tuner.frontend.OnTuneEventListener
import com.maleicacid.tvinput.aribsi.AribElementaryStream
import com.maleicacid.tvinput.aribsi.AribEventDescriptors
import com.maleicacid.tvinput.aribsi.AribComponents
import com.maleicacid.tvinput.aribsi.AribComponentEntry
import com.maleicacid.tvinput.aribsi.AribEvent
import com.maleicacid.tvinput.aribsi.AribParentalRating
import com.maleicacid.tvinput.aribsi.AribService
import com.maleicacid.tvinput.aribsi.AribRatingMapper
import com.maleicacid.tvinput.aribsi.EventModelMapper
import com.maleicacid.tvinput.aribsi.NativeAribSiParser
import com.maleicacid.tvinput.aribsi.SectionIngestController
import com.maleicacid.tvinput.aribsi.ServiceListBuilder
import com.maleicacid.tvinput.aribsi.ServicePublishabilityDiagnostic
import com.maleicacid.tvinput.aribsi.ServiceSemanticFacts
import com.maleicacid.tvinput.aribsi.SiStatus
import com.maleicacid.tvinput.aribsi.SmdSemanticFacts
import com.maleicacid.tvinput.common.CaptionTimestamp
import com.maleicacid.tvinput.common.FrequencyHz
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.TsPid
import com.maleicacid.tvinput.common.TunerKeyToken
import com.maleicacid.tvinput.common.StreamSelector
import com.maleicacid.tvinput.db.ChannelRecord
import com.maleicacid.tvinput.db.ProgramRecord
import org.json.JSONObject
import org.junit.Test

class TisR51FixedPlanAcceptanceTest {
    private val key = ServiceKey(4, 0x4010, 101)
    private val otherKey = ServiceKey(4, 0x4010, 102)

    @Test fun api30SessionIdIsPropagatedWithoutFallback() {
        val sessionId = "framework-session-123"
        check(MaleicacidTvInputService.api30SessionIdForTest("input.test", sessionId) == sessionId)
        check(TunerController.normalizedTvInputSessionId(sessionId) == sessionId)
        check(TunerController.normalizedTvInputSessionId(null) == null)
        check(TunerController.normalizedTvInputSessionId("") == null)
        check(TunerController.isSignalUnavailableTuneEventForTest(OnTuneEventListener.SIGNAL_NO_SIGNAL))
        check(TunerController.isSignalUnavailableTuneEventForTest(OnTuneEventListener.SIGNAL_LOST_LOCK))
        check(!TunerController.isSignalUnavailableTuneEventForTest(OnTuneEventListener.SIGNAL_LOCKED))
    }

    @Test fun hevcOnlyServiceIsNotR51VideoCandidate() {
        check(!TunerSelectionPolicy.isSupportedVideoStreamType(0x24))
        check(TunerSelectionPolicy.selectVideo(listOf(es(TsPid(0x120), 0x24))) == null)
    }

    @Test fun h264AndMpeg2RemainR51VideoCandidates() {
        check(TunerSelectionPolicy.isSupportedVideoStreamType(0x02))
        check(TunerSelectionPolicy.isSupportedVideoStreamType(0x1b))
        check(TunerSelectionPolicy.selectVideo(listOf(es(TsPid(0x100), 0x1b)))?.streamType == 0x1b)
        check(TunerSelectionPolicy.selectVideo(listOf(es(TsPid(0x101), 0x02)))?.streamType == 0x02)
    }

    @Test fun unsupportedVideoCodecMetadataIsSeparatedFromR51PlaybackClaim() {
        check(NativeAribSiParser.isRecognizedVideoCodecForTest(0x24))
        check(!NativeAribSiParser.isR51PlaybackSupportedVideoCodecForTest(0x24))
        val service = AribService(
            serviceKey = key,
            name = "HEVC service",
            pcrPid = TsPid(0x100),
            freeCaMode = false,
            streams = listOf(es(TsPid(0x120), 0x24, componentTag = 1)),
        )
        val components = org.json.JSONObject(NativeAribSiParser.toComponentsObjectForServiceForTest(service))
        val video = components.getJSONArray("video").getJSONObject(0)
        check(video.getString("codec") == "HEVC")
        check(video.getString("parseStatus") == "OK")
        check(!video.has("diagnosticCode"))
        check(!video.has("r51PlaybackSupported"))
        check(!video.has("liveViewableClaim"))
        val providerData = org.json.JSONObject(TvProviderWriter.programProviderDataForTest(
            EventModelMapper().toProgramRecords(listOf(aribEvent().withComponents(componentsFromJson(components)))).single(),
        ))
        val providerVideo = providerData.getJSONObject("components").getJSONArray("video").getJSONObject(0)
        check(providerVideo.getString("codec") == "HEVC")
        check(!providerVideo.has("r51PlaybackSupported"))
        check(!providerVideo.has("liveViewableClaim"))
        check(TunerSelectionPolicy.selectVideo(service.streams) == null)
        val bsSeed = JapanIsdbScanPlan.isdbsBsBands().first()
        check(bsSeed.streamSelector == com.maleicacid.tvinput.common.StreamSelector.NONE)
        val discoveredBs = JapanIsdbScanPlan.explicitBsCandidatesFromScan(bsSeed, listOf(18803, 18803, 0xffff, -1))
        check(discoveredBs.size == 1)
        check(discoveredBs.single().streamSelector.value == 18803)
        check(JapanIsdbScanPlan.fallbackBsCandidates(bsSeed).all { it.streamSelector.type == com.maleicacid.tvinput.common.StreamSelectorType.TSID })
    }

    @Test fun unsupportedAudioCodecMetadataDoesNotMakePlaybackUnsupportedWhenVideoIsSupported() {
        check(NativeAribSiParser.isRecognizedAudioCodecForTest(0x11))
        check(!NativeAribSiParser.isR51PlaybackSupportedAudioCodecForTest(0x11))
        val service = AribService(
            serviceKey = key,
            name = "H264 with LATM audio",
            pcrPid = TsPid(0x100),
            freeCaMode = false,
            streams = listOf(es(TsPid(0x101), 0x1b), es(TsPid(0x111), 0x11, componentTag = 2, language = "jpn")),
        )
        val components = org.json.JSONObject(NativeAribSiParser.toComponentsObjectForServiceForTest(service))
        val audio = components.getJSONArray("audio").getJSONObject(0)
        check(audio.getString("codec") == "MPEG-4-AAC-LATM")
        check(audio.getString("parseStatus") == "OK")
        check(!audio.has("r51PlaybackSupported"))
        val providerData = org.json.JSONObject(TvProviderWriter.programProviderDataForTest(
            EventModelMapper().toProgramRecords(listOf(aribEvent().withComponents(componentsFromJson(components)))).single(),
        ))
        val providerAudio = providerData.getJSONObject("components").getJSONArray("audio").getJSONObject(0)
        check(providerAudio.getString("codec") == "MPEG-4-AAC-LATM")
        check(!providerAudio.has("r51PlaybackSupported"))
        check(AudioTrackMetadataPolicy.encodingForPmtStreamType(0x0f) == android.media.MediaFormat.MIMETYPE_AUDIO_AAC)
        check(AudioTrackMetadataPolicy.channelCountForComponentType(0x02) == 2)
        check(AudioTrackMetadataPolicy.channelCountForComponentType(0x29) == 6)
        check(AudioTrackMetadataPolicy.sampleRateHz(0x07) == 48_000)
        check(AudioTrackMetadataPolicy.sampleRateHz(0x04) == null)
        check(AudioTrackMetadataPolicy.isAudioDescription(0x20))
        check(AudioTrackMetadataPolicy.isHardOfHearing(0x40))
        check(TunerSelectionPolicy.selectVideo(service.streams)?.streamType == 0x1b)
        check(!PlaybackPipeline.isSupportedAudioStreamTypeForTest(0x11))
    }

    @Test fun mixedH264AndHevcSelectsH264CapablePath() {
        val selected = TunerSelectionPolicy.selectVideo(listOf(es(TsPid(0x200), 0x24), es(TsPid(0x201), 0x1b)))
        check(selected?.streamType == 0x1b)
    }

    @Test fun validEitComponentWithoutPmtComponentTagIsPreserved() {
        val videoComponent = AribComponentEntry(
            esPid = null,
            componentTag = 7,
            componentType = 0xb3,
            language = "jpn",
            sourceDescriptor = "component_descriptor",
            parseStatus = "OK",
        )
        val audioComponent = AribComponentEntry(
            esPid = null,
            componentTag = 8,
            componentType = 0x03,
            language = "jpn",
            sourceDescriptor = "audio_component_descriptor",
            parseStatus = "OK",
        )
        val merged = NativeAribSiParser.mergeEventAndServiceComponentsForTest(
            eventComponents = AribComponents(
                video = listOf(videoComponent),
                audio = listOf(audioComponent),
            ),
            serviceComponents = AribComponents(),
        )

        check(merged.video == listOf(videoComponent)) {
            "PMTにcomponent_tagが無くても有効なEIT component_descriptor事実を失ってはなりません"
        }
        check(merged.audio == listOf(audioComponent)) {
            "PMTにcomponent_tagが無くても有効なEIT audio_component_descriptor事実を失ってはなりません"
        }

        val program = EventModelMapper()
            .toProgramRecords(listOf(aribEvent().withComponents(merged)))
            .single()
        val providerData = JSONObject(TvProviderWriter.programProviderDataForTest(program))
        val providerVideo = providerData.getJSONObject("components").getJSONArray("video").getJSONObject(0)
        check(providerVideo.isNull("esPid"))
        check(providerVideo.isNull("streamType"))
        check(providerVideo.isNull("codec"))
        check(providerVideo.getInt("componentTag") == 7)
        check(providerVideo.getString("sourceDescriptor") == "component_descriptor")
    }

    @Test fun audioOnlyServiceTypeAcceptsSupportedAudioWithoutVideo() {
        val selection = TunerController.AvStreamSelection(
            serviceKey = key,
            pcrPid = TsPid(0x100),
            video = null,
            audio = es(TsPid(0x110), 0x0f),
        )
        check(!TunerSelectionPolicy.hasSupportedVideo(listOf(es(TsPid(0x110), 0x0f))))
        check(!PlaybackPolicy.shouldRejectSelection(0x02, selection))
        check(PlaybackPolicy.shouldRejectSelection(0x01, selection))
    }

    @Test fun hevcOnlyServiceIsRejectedBeforePlaybackStart() {
        val streams = listOf(es(TsPid(0x120), 0x24))
        val selection = TunerController.AvStreamSelection(
            serviceKey = key,
            pcrPid = TsPid(0x100),
            video = TunerSelectionPolicy.selectVideo(streams),
            audio = null,
        )
        check(!TunerSelectionPolicy.hasSupportedVideo(streams))
        check(PlaybackPolicy.shouldRejectSelection(0x01, selection))
    }

    @Test fun supportedAribRawRatingConvertsToTvContentRatingString() {
        val flattened = AribRatingMapper.toTvContentRatingString(
            AribParentalRating("JPN", 12),
            AribRatingMapper.BroadcastProfile.BS_CS,
        )
        check(flattened != null)
        check(flattened!!.contains(AribRatingMapper.DOMAIN))
        check(flattened.contains("ISDB"))
        check(flattened.contains("ISDB_15"))
    }

    @Test fun isdbRawRatingBoundaryValuesAreProjected() {
        check(requireNotNull(AribRatingMapper.toTvContentRatingString(AribParentalRating("JPN", 1), AribRatingMapper.BroadcastProfile.BS_CS)).contains("ISDB_4"))
        check(requireNotNull(AribRatingMapper.toTvContentRatingString(AribParentalRating("JPN", 17), AribRatingMapper.BroadcastProfile.BS_CS)).contains("ISDB_20"))
    }

    @Test fun explicitAribExceptionalRatingUsesProductDomainInsteadOfUnrated() {
        listOf(0x12, 0x15, 0x63, 0xff).forEach { raw ->
            val flattened = requireNotNull(AribRatingMapper.toTvContentRatingString(AribParentalRating("JPN", raw), AribRatingMapper.BroadcastProfile.BS_CS))
            check(flattened.contains(AribRatingMapper.EXCEPTIONAL_DOMAIN))
            check(flattened.contains(AribRatingMapper.EXCEPTIONAL_RATING_SYSTEM))
            check(flattened.contains(AribRatingMapper.EXCEPTIONAL_RATING))
            check(flattened != TvContentRating.UNRATED.flattenToString())
        }
        check(AribRatingMapper.toTvContentRatingString(AribParentalRating("USA", 0x12), AribRatingMapper.BroadcastProfile.BS_CS) == null)
        check(AribRatingMapper.toTvContentRatingString(AribParentalRating("JPN", 0), AribRatingMapper.BroadcastProfile.BS_CS) == null)
    }

    @Test fun explicitAribExceptionalRatingIsWrittenToProgramsContentRating() {
        val store = FakeStore()
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        writer.upsertChannels(listOf(ChannelRecord(key, 0x01, "101", "NHK", FrequencyHz(473_142_857L))))
        val record = EventModelMapper().toProgramRecords(
            events = listOf(aribEvent(parentalRatings = listOf(AribParentalRating("JPN", 0x12)))),
            ratingProfileByServiceKey = mapOf(key to AribRatingMapper.BroadcastProfile.BS_CS),
        ).single()
        writer.upsertPrograms(listOf(record))
        val contentRating = store.programs.values.single().getAsString(TvContract.Programs.COLUMN_CONTENT_RATING)
        check(contentRating != null)
        check(contentRating.contains(AribRatingMapper.EXCEPTIONAL_DOMAIN))
        check(contentRating != TvContentRating.UNRATED.flattenToString())
        val providerData = org.json.JSONObject(TvProviderWriter.programProviderDataForTest(record))
        val rating = providerData.getJSONArray("ratings").getJSONObject(0)
        check(rating.getInt("rawRatingByte") == 0x12)
        check(rating.getString("parseStatus") == "OK")
        check(!rating.has("supported"))
        check(!rating.has("mappedTvContentRating"))
    }

    @Test fun unsupportedRatingRemainsRawWithoutProductDiagnostics() {
        val event = aribEvent(parentalRatings = listOf(AribParentalRating("USA", 15)))
        val record = EventModelMapper().toProgramRecords(listOf(event)).single()
        check(record.contentRatings.isEmpty())
        val ratingEntry = record.descriptors.parentalRatings.single()
        check(ratingEntry.countryCode == "USA")
        check(ratingEntry.rawRatingByte == 15)
        val providerData = org.json.JSONObject(TvProviderWriter.programProviderDataForTest(record))
        val ratings = providerData.getJSONArray("ratings")
        check(ratings.length() == 1)
        check(ratings.getJSONObject(0).getString("countryCode") == "USA")
        val diagnostics = providerData.getJSONObject("diagnostics")
        check(diagnostics.getJSONArray("descriptorDiagnostics").length() == 0)
        check(diagnostics.getJSONArray("publishDiagnostics").length() == 0)
        check(!providerData.has("unsupportedDescriptorDiagnostics"))
        check(!providerData.has("parentalRatingDiagnostics"))
    }

    @Test fun malformedAndTruncatedParentalRatingAreNotProjectedToContentRating() {
        val malformed = aribEvent(
            parentalRatings = listOf(AribParentalRating("JPN", 12, parseStatus = "MalformedLength")),
            descriptorDiagnosticsCanonicalJson = descriptorDiagnosticsCanonicalJson("MalformedLength", 0x55),
        )
        val truncated = aribEvent(
            parentalRatings = listOf(AribParentalRating("JPN", 15, parseStatus = "TruncatedDescriptor")),
            descriptorDiagnosticsCanonicalJson = descriptorDiagnosticsCanonicalJson("TruncatedDescriptor", 0x55),
        )
        val records = EventModelMapper().toProgramRecords(
            events = listOf(malformed, truncated),
            ratingProfileByServiceKey = mapOf(key to AribRatingMapper.BroadcastProfile.BS_CS),
        )
        check(records.size == 2)
        records.forEach { record ->
            check(record.contentRatings.isEmpty())
            val providerData = TvProviderWriter.programProviderDataForTest(record)
            check(providerData.contains("descriptorDiagnostics"))
            check(!providerData.contains("unsupportedDescriptorDiagnostics"))
            check(!providerData.contains("parentalRatingDiagnostics"))
        }
    }

    @Test fun unsupportedParentalRatingsDoNotWriteProgramsContentRatingColumn() {
        val store = FakeStore()
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        writer.upsertChannels(listOf(ChannelRecord(key, 0x01, "101", "NHK", FrequencyHz(473_142_857L))))
        val unsupported = EventModelMapper().toProgramRecords(listOf(
            aribEvent(parentalRatings = listOf(
                AribParentalRating("USA", 12),
                AribParentalRating("JPN", 12),
            )),
        )).single()
        writer.upsertPrograms(listOf(unsupported))
        val values = store.programs.values.single()
        check(values.getAsString(TvContract.Programs.COLUMN_CONTENT_RATING) == null)
        val providerData = values.getAsByteArray(TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA)
        check(providerData.utf8Contains("descriptorDiagnostics"))
        check(!providerData.utf8Contains("unsupportedDescriptorDiagnostics"))
        check(!providerData.utf8Contains("parentalRatingDiagnostics"))
    }

    @Test fun productExceptionalAribRatingDomainIsGeneratedSeparatelyFromAospIsdbAgeDomain() {
        val age = requireNotNull(AribRatingMapper.toTvContentRatingString(AribParentalRating("JPN", 12), AribRatingMapper.BroadcastProfile.BS_CS))
        check(age.contains("com.android.tv"))
        check(age.contains("ISDB"))
        check(age.contains("ISDB_15"))
        val exceptional = requireNotNull(AribRatingMapper.toTvContentRatingString(AribParentalRating("JPN", 0x12), AribRatingMapper.BroadcastProfile.BS_CS))
        check(exceptional.contains(AribRatingMapper.EXCEPTIONAL_DOMAIN))
        check(exceptional.contains(AribRatingMapper.EXCEPTIONAL_RATING_SYSTEM))
        check(exceptional.contains(AribRatingMapper.EXCEPTIONAL_RATING))
    }

    @Test fun programProviderDataKeepsOnlyBroadcastCasSemanticFact() {
        val record = EventModelMapper().toProgramRecords(
            events = listOf(aribEvent()),
            semanticFactsByServiceKey = mapOf(key to semanticFacts(requiresCas = true)),
        ).single()
        val providerData = JSONObject(TvProviderWriter.programProviderDataForTest(record))
        val cas = providerData.getJSONObject("cas")
        check(cas.getBoolean("requiresCas"))
        check(cas.getString("source") == "SI_SEMANTICS")
        check(!cas.has("unsupportedCas"))
        check(!providerData.has("clearLivePlaybackSupported"))
        check(!providerData.has("channelRegistrationReady"))
        check(!providerData.has("epgPublishable"))
        check(!providerData.has("publishStateSource"))
    }

    @Test fun contentRatingWrittenToPrograms() {
        val store = FakeStore()
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        writer.upsertChannels(listOf(ChannelRecord(key, 0x01, "101", "NHK", FrequencyHz(473_142_857L))))
        val rating = requireNotNull(AribRatingMapper.toTvContentRatingString(AribParentalRating("JPN", 12), AribRatingMapper.BroadcastProfile.BS_CS))
        writer.upsertPrograms(listOf(program(key, contentRatings = listOf(rating))))
        val values = store.programs.values.single()
        check(values.getAsString(TvContract.Programs.COLUMN_CONTENT_RATING) == rating)
    }

    @Test fun longDescriptionWrittenToPrograms() {
        val writer = TvProviderWriter("input.test", FakeStore(), testOnly = true)
        val values = writer.programValuesForTest(1L, program(key, description = "long description"))
        check(values.getAsString(TvContract.Programs.COLUMN_LONG_DESCRIPTION) == "long description")
    }

    @Test fun firstFrameGenerationGuardRejectsStaleCallback() {
        check(!PlaybackPipeline.acceptsFirstFrameForTest(callbackGeneration = 1, currentGeneration = 2, surfaceValid = true, alreadyNotified = false))
        check(!PlaybackPipeline.acceptsFirstFrameForTest(callbackGeneration = 2, currentGeneration = 2, surfaceValid = false, alreadyNotified = false))
        check(!PlaybackPipeline.acceptsFirstFrameForTest(callbackGeneration = 2, currentGeneration = 2, surfaceValid = true, alreadyNotified = true))
        check(PlaybackPipeline.acceptsFirstFrameForTest(callbackGeneration = 2, currentGeneration = 2, surfaceValid = true, alreadyNotified = false))
    }

    @Test fun localPolicyIsDerivedFromRawServiceSemanticFacts() {
        val clearService = aribService(
            pmtPid = TsPid(0x100),
            pcrPid = TsPid(0x101),
            freeCaMode = false,
            streams = listOf(es(TsPid(0x101), 0x1b)),
        )
        val clearFacts = semanticFacts(elementaryStreams = clearService.streams)
        check(ServiceListBuilder.completenessForModel(clearService, clearFacts).clearLivePlaybackSupported)
        check(ServiceListBuilder.completenessForModel(clearService, clearFacts).registrationReady)

        check(!ServiceListBuilder.completenessForModel(clearService, clearFacts.copy(pcrPidResolved = false)).clearLivePlaybackSupported)
        check(!ServiceListBuilder.completenessForModel(clearService, clearFacts.copy(elementaryStreams = emptyList())).clearLivePlaybackSupported)
        check(!ServiceListBuilder.completenessForModel(clearService, clearFacts.copy(requiresCas = true)).clearLivePlaybackSupported)
        check(ServiceListBuilder.completenessForModel(clearService, clearFacts.copy(freeCaMode = true)).clearLivePlaybackSupported)
    }

    @Test fun scrambledCasPlaceholderServiceCanRegisterAndPublishEpgButNotClearLive() {
        val scrambledService = aribService(
            pmtPid = TsPid(0x100),
            pcrPid = TsPid(0x101),
            freeCaMode = true,
            streams = listOf(es(TsPid(0x101), 0x1b)),
        )
        val facts = semanticFacts(
            elementaryStreams = scrambledService.streams,
            requiresCas = true,
            freeCaMode = true,
        )
        val completeness = ServiceListBuilder.completenessForModel(scrambledService, facts)
        check(completeness.registrationReady)
        check(!completeness.clearLivePlaybackSupported)
        check(completeness.requiresCas)

        val event = aribEvent()
        val record = EventModelMapper().toProgramRecords(listOf(event), mapOf(event.serviceKey to facts)).single()
        check(record.requiresCas)
        val providerData = JSONObject(TvProviderWriter.programProviderDataForTest(record))
        check(providerData.getJSONObject("cas").getBoolean("requiresCas"))
        check(!providerData.getJSONObject("cas").has("unsupportedCas"))
        check(!providerData.has("clearLivePlaybackSupported"))
        check(!providerData.has("channelRegistrationReady"))
        check(!providerData.has("epgPublishable"))
        check(!providerData.has("publishStateSource"))
    }

    @Test fun liveRefreshSkipsUnknownChannelEvents() {
        val retained = ChannelScanController.filterProgramServiceKeysForPublishModeForTest(
            mode = ChannelScanController.PublishMode.LIVE_TUNE_REFRESH,
            allServiceKeys = listOf(key, otherKey),
            existingServiceKeys = setOf(key),
            allowedServiceKeys = null,
        )
        check(retained == setOf(key))
    }

    @Test fun scanStablePartialRequiresRegistrationReadySnapshotAndStableWait() {
        val policy = ChannelScanController.SiCollectionPolicy(minWaitMs = 100, maxWaitMs = 1_000, stableWaitMs = 200, pollIntervalMs = 10)
        check(ChannelScanController.siCollectionOutcomeForTest(false, false, 150, 50, 1, policy) == ChannelScanController.SiCollectionOutcome.TIMEOUT_PARTIAL)
        check(ChannelScanController.siCollectionOutcomeForTest(false, false, 300, 250, 1, policy) == ChannelScanController.SiCollectionOutcome.STABLE_PARTIAL)
        check(ChannelScanController.siCollectionOutcomeForTest(false, false, 1_000, 100, 1, policy) == ChannelScanController.SiCollectionOutcome.TIMEOUT_PARTIAL)
        check(ChannelScanController.siCollectionOutcomeForTest(false, false, 1_000, 250, 0, policy) == ChannelScanController.SiCollectionOutcome.INCOMPLETE_NO_REGISTRATION_READY_SERVICE)
        check(ChannelScanController.siCollectionOutcomeForTest(true, false, 100, 0, 0, policy) == ChannelScanController.SiCollectionOutcome.COMPLETE)
    }

    @Test fun partialSiDiscoveryPublishesOnlyRegistrationReadySnapshotServices() {
        check(ChannelScanController.SiCollectionResult(ChannelScanController.SiCollectionOutcome.STABLE_PARTIAL, null, clearLivePlaybackSupportedServices = 0, registrationReadyServices = 1).mayPublishChannels)
        check(ChannelScanController.SiCollectionResult(ChannelScanController.SiCollectionOutcome.TIMEOUT_PARTIAL, null, clearLivePlaybackSupportedServices = 0, registrationReadyServices = 1).mayPublishChannels)
        check(!ChannelScanController.SiCollectionResult(ChannelScanController.SiCollectionOutcome.TIMEOUT_PARTIAL, null, clearLivePlaybackSupportedServices = 0, registrationReadyServices = 0).mayPublishChannels)
        check(!ChannelScanController.SiCollectionResult(ChannelScanController.SiCollectionOutcome.CANCELLED, null, clearLivePlaybackSupportedServices = 0, registrationReadyServices = 1).mayPublishChannels)
    }

    @Test fun setupScanPublishesEventsForRegisteredServicesOnly() {
        val retained = ChannelScanController.filterProgramServiceKeysForPublishModeForTest(
            mode = ChannelScanController.PublishMode.SETUP_SCAN,
            allServiceKeys = listOf(key, otherKey),
            existingServiceKeys = emptySet(),
            allowedServiceKeys = setOf(otherKey),
        )
        check(retained == setOf(otherKey))
    }

    @Test fun h264Sps480iSetsFormatSize() {
        check(PlaybackPipeline.h264DimensionsForTest(makeSps(codedWidth = 720, codedHeight = 480, frameMbsOnly = false)) == 720 to 480)
    }

    @Test fun h264Sps720pSetsFormatSize() {
        check(PlaybackPipeline.h264DimensionsForTest(makeSps(codedWidth = 1280, codedHeight = 720, frameMbsOnly = true)) == 1280 to 720)
    }

    @Test fun h264Sps1080iSetsFormatSize() {
        check(PlaybackPipeline.h264DimensionsForTest(makeSps(codedWidth = 1920, codedHeight = 1088, frameMbsOnly = false, cropBottom = 2)) == 1920 to 1080)
    }

    @Test fun h264SpsWithCropSetsDisplaySize() {
        check(PlaybackPipeline.h264DimensionsForTest(makeSps(codedWidth = 1920, codedHeight = 1088, frameMbsOnly = true, cropRight = 8, cropBottom = 4)) == 1904 to 1080)
    }

    @Test fun malformedSpsDoesNotUseDefault1920x1080() {
        val malformed = byteArrayOf(0, 0, 0, 1, 0x67, 0x42, 0x00)
        check(PlaybackPipeline.h264DimensionsForTest(malformed) == null)
    }

    @Test fun pmtGeneratesVideoAudioSubtitleTrackIds() {
        val video = es(TsPid(0x101), 0x1b)
        val audio = es(TsPid(0x110), 0x0f, componentTag = 7, language = "jpn")
        val subtitle = es(TsPid(0x130), 0x06, componentTag = 8, dataComponentId = 0x0008, isCaption = true)
        check(TunerSelectionPolicy.trackIdForVideo(video) == "video:257")
        check(TunerSelectionPolicy.trackIdForAudio(audio) == "audio:272:7")
        check(TunerSelectionPolicy.trackIdForSubtitle(subtitle) == "subtitle:304:8:lang1")
        val tracks = listOf(
            TunerController.TisTrack("video:257", TvTrackInfo.TYPE_VIDEO, TsPid(0x101), 0x1b, null, -1, null),
            TunerController.TisTrack("audio:272:7", TvTrackInfo.TYPE_AUDIO, TsPid(0x110), 0x0f, 7, -1, "jpn"),
            TunerController.TisTrack("subtitle:304:8:lang1", TvTrackInfo.TYPE_SUBTITLE, TsPid(0x130), 0x06, 8, null, null, dataComponentId = 0x0008, captionLanguageId = 1),
        )
        check(TunerSelectionPolicy.isSelectableTrack(TvTrackInfo.TYPE_AUDIO, "audio:272:7", tracks))
        check(!TunerSelectionPolicy.isSelectableTrack(TvTrackInfo.TYPE_AUDIO, "audio:999", tracks))
        check(TunerSelectionPolicy.isSelectableTrack(TvTrackInfo.TYPE_SUBTITLE, "subtitle:304:8:lang1", tracks))
        check(!TunerSelectionPolicy.isSelectableTrack(TvTrackInfo.TYPE_SUBTITLE, "subtitle:1", tracks))

        val normalDefaultVideo = es(TsPid(0x201), 0x1b, componentTag = 0x00)
        val mvtvMainVideo = es(TsPid(0x202), 0x1b, componentTag = 0x05)
        val mvtvHigherVideo = es(TsPid(0x203), 0x1b, componentTag = 0x06)
        check(TunerSelectionPolicy.selectVideo(listOf(mvtvHigherVideo, normalDefaultVideo, mvtvMainVideo)) == normalDefaultVideo)
        check(TunerSelectionPolicy.selectVideo(
            listOf(normalDefaultVideo, mvtvHigherVideo, mvtvMainVideo),
            componentGroupTags = setOf(0x05, 0x06),
        ) == mvtvMainVideo)
    }

    @Test fun captionControllerDrawsOnlyEnabledSelectedTrack() {
        check(AribCaptionController.shouldDrawCaptionForTest(enabled = true, selectedTrackId = "subtitle:304:8", incomingTrackId = "subtitle:304:8"))
        check(!AribCaptionController.shouldDrawCaptionForTest(enabled = false, selectedTrackId = "subtitle:304:8", incomingTrackId = "subtitle:304:8"))
        check(!AribCaptionController.shouldDrawCaptionForTest(enabled = true, selectedTrackId = "subtitle:304:8", incomingTrackId = "subtitle:999"))
        check(!AribCaptionController.shouldDrawCaptionForTest(enabled = true, selectedTrackId = null, incomingTrackId = "subtitle:304:8"))
    }

    @Test fun unblockKeyIncludesCurrentProgramIdentityAndRating() {
        val rating = requireNotNull(AribRatingMapper.parseFlattened(requireNotNull(AribRatingMapper.toTvContentRatingString(AribParentalRating("JPN", 12), AribRatingMapper.BroadcastProfile.BS_CS))))
        val keyString = CurrentProgramRatingResolver.unblockKey(
            channelUriString = "content://android.media.tv/channel/1",
            serviceKey = key,
            eventId = 10,
            ratingString = rating.flattenToString(),
        )
        check(keyString.contains("\"originalNetworkId\":4"))
        check(keyString.contains("\"eventId\":10"))
        check(keyString.contains("ISDB_15"))
    }

    @Test fun onUnblockContentAcceptsOnlySameCurrentProgramRatingWithCompleteIdentity() {
        val rating15 = requireNotNull(AribRatingMapper.parseFlattened(requireNotNull(AribRatingMapper.toTvContentRatingString(AribParentalRating("JPN", 12), AribRatingMapper.BroadcastProfile.BS_CS))))
        val rating18 = requireNotNull(AribRatingMapper.parseFlattened(requireNotNull(AribRatingMapper.toTvContentRatingString(AribParentalRating("JPN", 15), AribRatingMapper.BroadcastProfile.BS_CS))))
        val current = CurrentProgramRatingResolver.CurrentProgramRatingSet(
            ratings = listOf(rating15),
            source = CurrentProgramRatingResolver.Source.LATEST_EIT_CACHE,
            channelUriString = "content://android.media.tv/channel/1",
            serviceKey = key,
            eventId = 10,
            startTimeMillis = 1_700_000_000_000L,
            endTimeMillis = 1_700_001_800_000L,
        )
        check(current.exactUnblockKeyFor(rating15) != null)
        check(current.exactUnblockKeyFor(rating18) == null)

        val unratedFallback = CurrentProgramRatingResolver.CurrentProgramRatingSet(
            ratings = listOf(TvContentRating.UNRATED),
            source = CurrentProgramRatingResolver.Source.UNRATED_FALLBACK,
            channelUriString = "content://android.media.tv/channel/1",
            serviceKey = key,
            eventId = null,
            startTimeMillis = null,
            endTimeMillis = null,
        )
        check(unratedFallback.exactUnblockKeyFor(TvContentRating.UNRATED) == null)
    }

    @Test fun videoHeaderMetadataIsProjectedIntoCurrentProgramRecord() {
        val info = PlaybackPipeline.VideoFormatInfo(0x1b, "video/avc", 1280, 720)
        val records = ProgramVideoMetadataPolicy.currentProgramsWithMetadata(
            events = listOf(aribEvent()),
            serviceKey = key,
            nowMillis = 1_700_000_000_100L,
            info = info,
        )
        val record = records.single()
        check(record.videoWidth == 1280)
        check(record.videoHeight == 720)
        check(record.videoFormat == "video/avc")
        check(!TvProviderWriter.programProviderDataForTest(record).contains("videoFormat"))
        check(!TvProviderWriter.programProviderDataForTest(record).contains("video/avc"))
    }

    @Test fun videoHeaderMetadataIgnoresNonCurrentEvent() {
        val info = PlaybackPipeline.VideoFormatInfo(0x1b, "video/avc", 1280, 720)
        val records = ProgramVideoMetadataPolicy.currentProgramsWithMetadata(
            events = listOf(aribEvent().copy(startTimeMillis = 1_600_000_000_000L)),
            serviceKey = key,
            nowMillis = 1_700_000_000_100L,
            info = info,
        )
        check(records.isEmpty())
    }

    @Test fun cs110SelectorNoneRejectsTsidAndRelative() {
        check(TunerSelectionPolicy.isCs110SelectorAllowed("110CS", StreamSelector.NONE))
        check(!TunerSelectionPolicy.isCs110SelectorAllowed("110CS", StreamSelector.tsid(0x4010)))
        check(!TunerSelectionPolicy.isCs110SelectorAllowed("110CS", StreamSelector.relative(2)))
        check(TunerSelectionPolicy.isCs110SelectorAllowed("BS", StreamSelector.tsid(0x4010)))
    }

    @Test fun bootAndBackgroundMaintenanceUpdateExistingChannelsOnly() {
        val bootRetained = ChannelScanController.filterProgramServiceKeysForPublishModeForTest(
            mode = ChannelScanController.PublishMode.BOOT_EPG_SYNC,
            allServiceKeys = listOf(key, otherKey),
            existingServiceKeys = setOf(key, otherKey),
            allowedServiceKeys = setOf(key),
        )
        val backgroundRetained = ChannelScanController.filterProgramServiceKeysForPublishModeForTest(
            mode = ChannelScanController.PublishMode.BACKGROUND_CHANNEL_MAINTENANCE,
            allServiceKeys = listOf(key, otherKey),
            existingServiceKeys = setOf(key),
            allowedServiceKeys = null,
        )
        check(bootRetained == setOf(key))
        check(backgroundRetained == setOf(key))
    }

    @Test fun authoritativeDeletionUsesRustValidIdentitySetIncludingUndefinedTimeEvents() {
        val undefinedTimeIdentity = TvProviderWriter.programKeyForTest(program(key).copy(eventId = 88))
        val update = com.maleicacid.tvinput.aribsi.AribEpgUpdateWindow(
            serviceKey = key,
            windowStartMillis = 1_700_000_000_000L,
            windowEndMillis = 1_700_001_800_000L,
            validProgramStableIdentities = listOf(undefinedTimeIdentity),
            deletionAuthoritative = true,
        )
        check(ChannelScanController.validProgramKeysForUpdateForTest(update) == setOf(undefinedTimeIdentity))
    }

    @Test fun bootEpgSyncStartRequiresIdleScanAndNoLiveSession() {
        val active = ChannelScanManager.bootEpgSyncStartDecisionForTest(activeLiveSessionCount = 1, scanRunning = false)
        check(!active.allowed)
        check(active.reason == "LIVE_SESSION_STARTING_OR_ACTIVE")

        val creating = ChannelScanManager.bootEpgSyncStartDecisionForTest(activeLiveSessionCount = 0, scanRunning = false, sessionCreationInProgress = true)
        check(!creating.allowed)
        check(creating.reason == "LIVE_SESSION_STARTING_OR_ACTIVE")

        val scanRunning = ChannelScanManager.bootEpgSyncStartDecisionForTest(activeLiveSessionCount = 0, scanRunning = true)
        check(!scanRunning.allowed)
        check(scanRunning.reason == "SCAN_RUNNING")

        val idle = ChannelScanManager.bootEpgSyncStartDecisionForTest(activeLiveSessionCount = 0, scanRunning = false)
        check(idle.allowed)
        check(idle.reason == null)
    }

    @Test fun backgroundMaintenanceStartRequiresIdleScanAndNoLiveSession() {
        val active = ChannelScanManager.backgroundMaintenanceStartDecisionForTest(activeLiveSessionCount = 1, scanRunning = false)
        check(!active.allowed)
        check(active.reason == "LIVE_SESSION_STARTING_OR_ACTIVE")

        val creating = ChannelScanManager.backgroundMaintenanceStartDecisionForTest(activeLiveSessionCount = 0, scanRunning = false, sessionCreationInProgress = true)
        check(!creating.allowed)
        check(creating.reason == "LIVE_SESSION_STARTING_OR_ACTIVE")

        val scanRunning = ChannelScanManager.backgroundMaintenanceStartDecisionForTest(activeLiveSessionCount = 0, scanRunning = true)
        check(!scanRunning.allowed)
        check(scanRunning.reason == "SCAN_RUNNING")

        val idle = ChannelScanManager.backgroundMaintenanceStartDecisionForTest(activeLiveSessionCount = 0, scanRunning = false)
        check(idle.allowed)
        check(idle.reason == null)
    }

    @Test fun liveSessionPreemptsBootAndBackgroundButNotSetupScan() {
        check(tunerPriorityHintUseCase(ScanPurpose.SETUP_SCAN) == TvInputService.PRIORITY_HINT_USE_CASE_TYPE_SCAN)
        check(tunerPriorityHintUseCase(ScanPurpose.BOOT_EPG_SYNC) == TvInputService.PRIORITY_HINT_USE_CASE_TYPE_BACKGROUND)
        check(tunerPriorityHintUseCase(ScanPurpose.BACKGROUND_MAINTENANCE) == TvInputService.PRIORITY_HINT_USE_CASE_TYPE_BACKGROUND)

        val idle = ChannelScanManager.liveSessionPreemptDecisionForTest(scanRunning = false, purpose = null)
        check(!idle.shouldCancel)
        check(!idle.deferBootEpgSync)

        val setup = ChannelScanManager.liveSessionPreemptDecisionForTest(scanRunning = true, purpose = ScanPurpose.SETUP_SCAN)
        check(!setup.shouldCancel)
        check(!setup.deferBootEpgSync)

        val boot = ChannelScanManager.liveSessionPreemptDecisionForTest(scanRunning = true, purpose = ScanPurpose.BOOT_EPG_SYNC)
        check(boot.shouldCancel)
        check(boot.deferBootEpgSync)
        check(boot.diagnosticReason == "LIVE_SESSION_PREEMPTED_RUNNING_BOOT_EPG_SYNC")

        val background = ChannelScanManager.liveSessionPreemptDecisionForTest(scanRunning = true, purpose = ScanPurpose.BACKGROUND_MAINTENANCE)
        check(background.shouldCancel)
        check(!background.deferBootEpgSync)
        check(background.diagnosticReason == "LIVE_SESSION_PREEMPTED_RUNNING_BACKGROUND_MAINTENANCE")
    }

    @Test fun sectionStatusDiagnosticsAreBucketedByStatus() {
        check(SectionIngestController.statusBucketForTest(SiStatus.OK) == "accepted")
        check(SectionIngestController.statusBucketForTest(SiStatus.INVALID_SECTION) == "crc")
        check(SectionIngestController.statusBucketForTest(SiStatus.MALFORMED_DESCRIPTOR) == "malformed")
        check(SectionIngestController.statusBucketForTest(SiStatus.INTERNAL_ERROR) == "malformed")
    }

    @Test fun sectionShortReadIsDiagnosticAndNotIngest() {
        check(SectionFilterPolicy.readDecision(expected = 64, actual = 64, sourceIsCurrent = true) == SectionFilterPolicy.ReadDecision.INGEST)
        check(SectionFilterPolicy.readDecision(expected = 64, actual = 12, sourceIsCurrent = true) == SectionFilterPolicy.ReadDecision.SHORT_READ)
        check(SectionFilterPolicy.readDecision(expected = 64, actual = 0, sourceIsCurrent = true) == SectionFilterPolicy.ReadDecision.READ_ERROR)
        check(SectionFilterPolicy.readDecision(expected = 64, actual = 64, sourceIsCurrent = false) == SectionFilterPolicy.ReadDecision.STALE_SOURCE)
    }

    @Test fun firstFrameTimeoutUsesGenerationAndNotificationState() {
        check(PlaybackPipeline.shouldTriggerFirstFrameTimeoutForTest(timeoutGeneration = 3, currentGeneration = 3, alreadyNotified = false))
        check(!PlaybackPipeline.shouldTriggerFirstFrameTimeoutForTest(timeoutGeneration = 2, currentGeneration = 3, alreadyNotified = false))
        check(!PlaybackPipeline.shouldTriggerFirstFrameTimeoutForTest(timeoutGeneration = 3, currentGeneration = 3, alreadyNotified = true))
    }

    @Test fun tunerKeyTokenRejectsZeroAndOverSixteenBytes() {
        check(TunerKeyToken.fromOrNull(ByteArray(0)) == null)
        check(TunerKeyToken.fromOrNull(ByteArray(17)) == null)
        check(TunerKeyToken.fromOrNull(ByteArray(16)) != null)
        check(runCatching { TunerKeyToken(ByteArray(0)) }.isFailure)
        check(runCatching { TunerKeyToken(ByteArray(17)) }.isFailure)
    }

    @Test fun subtitlePesTimestampUsesPtsWhenPresentAndNoPtsOnlyWhenMissing() {
        val pts = PlaybackPipeline.captionTimestampForTest(isPtsPresent = true, pts90k = 90_000L)
        check(pts is CaptionTimestamp.Pts)
        check((pts as CaptionTimestamp.Pts).ptsMillis.value == 1_000L)
        check(PlaybackPipeline.captionTimestampForTest(isPtsPresent = false, pts90k = 90_000L) == CaptionTimestamp.NoPts)
        check(PlaybackPipeline.captionTimestampForTest(isPtsPresent = true, pts90k = -1L) == CaptionTimestamp.NoPts)
    }

    @Test fun mediaEventCallbackFailureMapsToRecoverableUnavailableReason() {
        check(PlaybackPipeline.unavailableReasonForMediaEventCallbackFailureForTest(isAudio = true) == PlaybackPipeline.PlaybackUnavailableReason.AUDIO_UNAVAILABLE)
        check(PlaybackPipeline.unavailableReasonForMediaEventCallbackFailureForTest(isAudio = false) == PlaybackPipeline.PlaybackUnavailableReason.VIDEO_CODEC_ERROR)
    }

    @Test fun programPublishSignatureChangesOnlyWhenProjectedContentChanges() {
        val first = listOf(program(key, description = "desc"))
        val same = listOf(program(key, description = "desc"))
        val changedDescription = listOf(program(key, description = "updated"))
        val changedRating = listOf(program(key, description = "desc", contentRatings = listOf(requireNotNull(AribRatingMapper.toTvContentRatingString(AribParentalRating("JPN", 15), AribRatingMapper.BroadcastProfile.BS_CS)))))
        check(ProgramPublishCoordinator.programSignatureForTest(first) == ProgramPublishCoordinator.programSignatureForTest(same))
        check(ProgramPublishCoordinator.programSignatureForTest(first) != ProgramPublishCoordinator.programSignatureForTest(changedDescription))
        check(ProgramPublishCoordinator.programSignatureForTest(first) != ProgramPublishCoordinator.programSignatureForTest(changedRating))
    }

    @Test fun currentProgramChangeClearsTemporaryUnblockKeys() {
        val keys = linkedSetOf("old-unblock-key")
        var identity = PlaybackPolicy.updateUnblockStateForProgramChange(
            previousIdentityKey = "program-a",
            nextIdentityKey = "program-a",
            unblockedContentKeys = keys,
        )
        check(identity == "program-a")
        check("old-unblock-key" in keys)

        identity = PlaybackPolicy.updateUnblockStateForProgramChange(
            previousIdentityKey = identity,
            nextIdentityKey = "program-b",
            unblockedContentKeys = keys,
        )
        check(identity == "program-b")
        check(keys.isEmpty())

        keys += "program-b-unblock"
        identity = PlaybackPolicy.updateUnblockStateForProgramChange(
            previousIdentityKey = identity,
            nextIdentityKey = null,
            unblockedContentKeys = keys,
        )
        check(identity == null)
        check(keys.isEmpty())
    }

    private fun aribService(
        pmtPid: TsPid?,
        pcrPid: TsPid?,
        freeCaMode: Boolean?,
        streams: List<AribElementaryStream>,
    ) = com.maleicacid.tvinput.aribsi.AribService(
        serviceKey = key,
        name = "NHK",
        pmtPid = pmtPid,
        pcrPid = pcrPid,
        freeCaMode = freeCaMode,
        streams = streams,
    )

    private fun publishability(
        serviceKey: ServiceKey,
        clearLive: Boolean,
        reasons: List<String> = emptyList(),
    ) = ServicePublishabilityDiagnostic(
        serviceKey = serviceKey,
        registrationReady = clearLive,
        requiresCas = false,
        reasons = reasons,
    )

    private fun semanticFacts(
        serviceType: Int = 0x01,
        elementaryStreams: List<AribElementaryStream> = listOf(es(TsPid(0x101), 0x1b)),
        requiresCas: Boolean = false,
        freeCaMode: Boolean? = false,
    ) = ServiceSemanticFacts(
        serviceKey = key,
        serviceType = serviceType,
        pmtPidResolved = true,
        pmtParsed = true,
        pcrPidResolved = true,
        elementaryStreams = elementaryStreams,
        requiresCas = requiresCas,
        caDescriptorsResolved = true,
        freeCaMode = freeCaMode,
        smd = SmdSemanticFacts(
            descriptorPresent = true,
            syntaxValid = true,
            systemManagementId = 0,
            broadcastingFlag = 0,
            broadcastingIdentifier = 0,
            additionalBroadcastingIdentification = 0,
            additionalIdentificationInfoHex = "",
            semanticState = "SUPPORTED_BROADCAST",
            diagnostic = null,
        ),
        missingComponents = emptyList(),
        semanticDiagnostics = emptyList(),
    )

    private fun es(
        pid: TsPid,
        streamType: Int,
        componentTag: Int? = null,
        language: String? = null,
        dataComponentId: Int? = null,
        isCaption: Boolean = false,
        isSuperimpose: Boolean = false,
    ): AribElementaryStream =
        AribElementaryStream(
            elementaryPid = pid,
            streamType = streamType,
            componentTag = componentTag,
            componentType = null,
            streamContent = null,
            languageCodes = listOfNotNull(language),
            dataComponentId = dataComponentId,
            isCaption = isCaption,
            isSuperimpose = isSuperimpose,
        )

    private fun aribEvent(
        parentalRatings: List<AribParentalRating> = emptyList(),
        descriptorDiagnosticsCanonicalJson: String = "[]",
    ): AribEvent = AribEvent(
        serviceKey = key,
        stableIdentity = "{\"kind\":\"arib-event-v1\",\"originalNetworkId\":4,\"transportStreamId\":16400,\"serviceId\":101,\"eventId\":1}",
        eventId = 1,
        startTimeMillis = 1_700_000_000_000L,
        durationMillis = 1_800_000L,
        title = "title",
        description = "desc",
        descriptors = AribEventDescriptors(
            parentalRatings = parentalRatings,
            diagnostics = com.maleicacid.tvinput.aribsi.AribEventDiagnostics(descriptorDiagnosticsCanonicalJson = descriptorDiagnosticsCanonicalJson),
        ),
    )

    private fun AribEvent.withComponents(components: AribComponents): AribEvent =
        copy(descriptors = descriptors.copy(components = components))

    private fun componentsFromJson(obj: org.json.JSONObject): AribComponents = AribComponents(
        video = componentEntries(obj.optJSONArray("video")),
        audio = componentEntries(obj.optJSONArray("audio")),
        subtitle = componentEntries(obj.optJSONArray("subtitle")),
        data = componentEntries(obj.optJSONArray("data")),
    )

    private fun componentEntries(array: org.json.JSONArray?): List<AribComponentEntry> = (0 until (array?.length() ?: 0)).mapNotNull { index ->
        val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
        val pid = obj.optInt("esPid", -1)
        if (pid < 0) null else AribComponentEntry(
            esPid = TsPid(pid),
            streamType = obj.optInt("streamType").takeIf { obj.has("streamType") },
            componentTag = obj.optInt("componentTag").takeIf { obj.has("componentTag") },
            componentType = obj.optInt("componentType").takeIf { obj.has("componentType") },
            codec = obj.optString("codec").takeIf { !obj.isNull("codec") && it.isNotBlank() },
            language = obj.optString("language").takeIf { !obj.isNull("language") && it.isNotBlank() },
            dataComponentId = obj.optInt("dataComponentId").takeIf { obj.has("dataComponentId") && !obj.isNull("dataComponentId") },
            captionServiceKind = obj.optString("captionServiceKind").takeIf { !obj.isNull("captionServiceKind") && it.isNotBlank() },
            parseStatus = obj.optString("parseStatus", "OK"),
        )
    }

    private fun descriptorDiagnosticsCanonicalJson(status: String, tag: Int): String = org.json.JSONArray()
        .put(org.json.JSONObject()
            .put("schema", "maleicacid.tv.descriptorDiagnostic")
            .put("schemaVersion", 1)
            .put("severity", "warning")
            .put("code", status)
            .put("scope", org.json.JSONObject()
                .put("pid", 18)
                .put("tableId", 78)
                .put("tableIdExtension", 101)
                .put("version", org.json.JSONObject.NULL)
                .put("sectionNumber", org.json.JSONObject.NULL)
                .put("originalNetworkId", 4)
                .put("transportStreamId", 16400)
                .put("serviceId", 101)
                .put("eventId", 1))
            .put("descriptor", org.json.JSONObject()
                .put("tag", tag)
                .put("name", org.json.JSONObject.NULL)
                .put("offset", 0)
                .put("declaredLength", 0)
                .put("actualRemainingLength", 0)
                .put("parseStatus", status)
                .put("rawPrefixHex", ""))
            .put("message", status))
        .toString()

    private fun program(
        serviceKey: ServiceKey,
        description: String = "desc",
        contentRatings: List<String> = emptyList(),
    ): ProgramRecord = ProgramRecord(
        serviceKey = serviceKey,
        eventId = 1,
        stableIdentity = org.json.JSONObject()
            .put("kind", "arib-event-v1")
            .put("originalNetworkId", serviceKey.originalNetworkId)
            .put("transportStreamId", serviceKey.transportStreamId)
            .put("serviceId", serviceKey.serviceId)
            .put("eventId", 1)
            .toString(),
        startTimeMillis = 1_700_000_000_000L,
        durationMillis = 1_800_000L,
        title = "title",
        description = description,
        contentRatings = contentRatings,
    )

    private fun makeSps(
        codedWidth: Int,
        codedHeight: Int,
        frameMbsOnly: Boolean,
        cropRight: Int = 0,
        cropBottom: Int = 0,
    ): ByteArray {
        require(codedWidth % 16 == 0)
        val frameMbsFactor = if (frameMbsOnly) 1 else 2
        require(codedHeight % (16 * frameMbsFactor) == 0)
        val bits = BitWriter()
        bits.writeBits(66, 8) // プロファイル識別子 baseline
        bits.writeBits(0, 8) // 制約フラグ
        bits.writeBits(30, 8) // レベル識別子
        bits.writeUE(0) // SPS 識別子
        bits.writeUE(0) // frame_num 上限補正値
        bits.writeUE(0) // 表示順序型
        bits.writeUE(0) // 表示順序 LSB 上限補正値
        bits.writeUE(1) // 参照フレーム数上限
        bits.writeBit(0) // frame_num 欠落許可フラグ
        bits.writeUE(codedWidth / 16 - 1)
        bits.writeUE(codedHeight / (16 * frameMbsFactor) - 1)
        bits.writeBit(if (frameMbsOnly) 1 else 0)
        if (!frameMbsOnly) bits.writeBit(0) // MB 適応フレームフィールドフラグ
        bits.writeBit(1) // 8x8 直接推定フラグ
        val hasCrop = cropRight != 0 || cropBottom != 0
        bits.writeBit(if (hasCrop) 1 else 0)
        if (hasCrop) {
            bits.writeUE(0)
            bits.writeUE(cropRight)
            bits.writeUE(0)
            bits.writeUE(cropBottom)
        }
        return byteArrayOf(0, 0, 0, 1, 0x67) + bits.toRbsp()
    }

    private class BitWriter {
        private val bits = mutableListOf<Int>()
        fun writeBit(value: Int) { bits += value and 1 }
        fun writeBits(value: Int, count: Int) {
            for (i in count - 1 downTo 0) writeBit(value ushr i)
        }
        fun writeUE(value: Int) {
            val codeNum = value + 1
            val size = 32 - Integer.numberOfLeadingZeros(codeNum)
            repeat(size - 1) { writeBit(0) }
            writeBits(codeNum, size)
        }
        fun toRbsp(): ByteArray {
            writeBit(1)
            while (bits.size % 8 != 0) writeBit(0)
            return ByteArray(bits.size / 8) { index ->
                var b = 0
                repeat(8) { bit -> b = (b shl 1) or bits[index * 8 + bit] }
                b.toByte()
            }
        }
    }

    private class FakeStore : TvProviderWriter.ChannelStore {
        private var nextChannelId = 1L
        private var nextProgramId = 100L
        val channels = LinkedHashMap<Long, ContentValues>()
        val programs = LinkedHashMap<Long, ContentValues>()

        override fun findExistingChannelId(key: ServiceKey): Result<Long?> = Result.success(
            channels.entries.firstOrNull { (_, v) ->
                v.getAsInteger(TvContract.Channels.COLUMN_ORIGINAL_NETWORK_ID) == key.originalNetworkId &&
                    v.getAsInteger(TvContract.Channels.COLUMN_TRANSPORT_STREAM_ID) == key.transportStreamId &&
                    v.getAsInteger(TvContract.Channels.COLUMN_SERVICE_ID) == key.serviceId
            }?.key,
        )
        override fun insertChannel(values: ContentValues): Result<Long?> { val id = nextChannelId++; channels[id] = ContentValues(values); return Result.success(id) }
        override fun updateChannel(channelId: Long, values: ContentValues): Result<Int> { channels[channelId] = ContentValues(values); return Result.success(1) }
        override fun indexExistingProgramsForWindow(channelId: Long, windowStartMs: Long, windowEndMs: Long): Result<Map<String, Long>> = Result.success(
            programs.entries.mapNotNull { (id, v) ->
                if (v.getAsLong(TvContract.Programs.COLUMN_CHANNEL_ID) != channelId) return@mapNotNull null
                val end = v.getAsLong(TvContract.Programs.COLUMN_END_TIME_UTC_MILLIS)
                val start = v.getAsLong(TvContract.Programs.COLUMN_START_TIME_UTC_MILLIS)
                if (end <= windowStartMs || start >= windowEndMs) return@mapNotNull null
                val key = TvProviderWriter.parseProgramKey(v.getAsByteArray(TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA)) ?: return@mapNotNull null
                key to id
            }.toMap(),
        )
        override fun insertProgram(values: ContentValues): Result<Long?> { val id = nextProgramId++; programs[id] = ContentValues(values); return Result.success(id) }
        override fun updateProgram(programId: Long, values: ContentValues): Result<Int> { programs[programId] = ContentValues(values); return Result.success(1) }
    }
}

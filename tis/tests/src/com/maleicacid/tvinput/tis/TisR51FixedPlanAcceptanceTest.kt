package com.maleicacid.tvinput.tis

import android.content.ContentValues
import android.media.tv.TvContentRating
import android.media.tv.TvContract
import android.media.tv.TvTrackInfo
import com.maleicacid.tvinput.aribsi.AribElementaryStream
import com.maleicacid.tvinput.aribsi.AribEvent
import com.maleicacid.tvinput.aribsi.AribParentalRating
import com.maleicacid.tvinput.aribsi.AribRatingMapper
import com.maleicacid.tvinput.aribsi.EventModelMapper
import com.maleicacid.tvinput.aribsi.isCurrentDiagnosticComplete
import com.maleicacid.tvinput.aribsi.SectionIngestController
import com.maleicacid.tvinput.aribsi.ServiceListBuilder
import com.maleicacid.tvinput.aribsi.ServicePublishabilityDiagnostic
import com.maleicacid.tvinput.aribsi.SiStatus
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.StreamSelector
import com.maleicacid.tvinput.db.ChannelRecord
import com.maleicacid.tvinput.db.ProgramRecord
import org.junit.Test

class TisR51FixedPlanAcceptanceTest {
    private val key = ServiceKey(4, 0x4010, 101)
    private val otherKey = ServiceKey(4, 0x4010, 102)

    @Test fun api30SessionIdIsPropagatedWithoutFallback() {
        val sessionId = "framework-session-123"
        check(MaleicacidTvInputService.api30SessionIdForTest("input.test", sessionId) == sessionId)
        check(!MaleicacidTvInputService.legacyFallbackSessionIdForTest("input.test").contains(sessionId))
    }

    @Test fun hevcOnlyServiceIsNotR51VideoCandidate() {
        check(!TunerController.isR51SupportedVideoStreamTypeForTest(0x24))
        check(TunerController.selectVideoForTest(listOf(es(0x120, 0x24))) == null)
    }

    @Test fun h264AndMpeg2RemainR51VideoCandidates() {
        check(TunerController.isR51SupportedVideoStreamTypeForTest(0x02))
        check(TunerController.isR51SupportedVideoStreamTypeForTest(0x1b))
        check(TunerController.selectVideoForTest(listOf(es(0x100, 0x1b)))?.streamType == 0x1b)
        check(TunerController.selectVideoForTest(listOf(es(0x101, 0x02)))?.streamType == 0x02)
    }

    @Test fun mixedH264AndHevcSelectsH264CapablePath() {
        val selected = TunerController.selectVideoForTest(listOf(es(0x200, 0x24), es(0x201, 0x1b)))
        check(selected?.streamType == 0x1b)
    }

    @Test fun supportedAribRatingConvertsToTvContentRatingString() {
        val flattened = AribRatingMapper.toTvContentRatingString(AribParentalRating("JPN", 15, 15, supported = true))
        check(flattened != null)
        check(flattened!!.contains(AribRatingMapper.DOMAIN))
        check(flattened.contains("ISDB"))
        check(flattened.contains("ISDB_15"))
    }

    @Test fun isdbRatingBoundaryValuesAreProjected() {
        check(requireNotNull(AribRatingMapper.toTvContentRatingString(AribParentalRating("JPN", 4, 4, supported = true))).contains("ISDB_4"))
        check(requireNotNull(AribRatingMapper.toTvContentRatingString(AribParentalRating("JPN", 20, 20, supported = true))).contains("ISDB_20"))
    }

    @Test fun unsupportedRatingIsNotGuessed() {
        check(AribRatingMapper.toTvContentRatingString(AribParentalRating("USA", 15, 15, supported = false)) == null)
        check(AribRatingMapper.toTvContentRatingString(AribParentalRating("JPN", 3, 3, supported = true)) == null)
        check(AribRatingMapper.toTvContentRatingString(AribParentalRating("JPN", 21, 21, supported = true)) == null)
        check(AribRatingMapper.toTvContentRatingString(AribParentalRating("JPN", 99, 99, supported = false)) == null)
        check(AribRatingMapper.toTvContentRatingString(AribParentalRating("JPN", 12, 12, supported = false)) == null)
    }

    @Test fun unsupportedDescriptorGoesToInternalProviderData() {
        val event = aribEvent(parentalRatings = listOf(AribParentalRating("USA", 15, 15, supported = false)))
        val record = EventModelMapper().toProgramRecords(listOf(event)).single()
        check(record.contentRatings.isEmpty())
        val unsupported = org.json.JSONObject(record.unsupportedDescriptorJson)
        check(unsupported.getInt("schemaVersion") == 1)
        val unsupportedEntry = unsupported.getJSONArray("diagnostics").getJSONObject(0)
        check(unsupportedEntry.getString("parseStatus") == "UnsupportedValue")
        check(unsupportedEntry.getInt("tag") == 0x55)
        check(unsupportedEntry.getJSONObject("serviceKey").getInt("serviceId") == key.serviceId)
        check(unsupportedEntry.getInt("eventId") == event.eventId)
        check(unsupportedEntry.getString("message").contains("USA"))
        check(!record.unsupportedDescriptorJson.contains("diagnosticCode"))
        check(!record.unsupportedDescriptorJson.contains("descriptorOffset"))
        val providerData = org.json.JSONObject(TvProviderWriter.programProviderDataForTest(record))
        val normalizedUnsupported = providerData.getJSONObject("unsupportedDescriptorDiagnostics")
        check(normalizedUnsupported.getInt("schemaVersion") == 1)
        val normalizedEntry = normalizedUnsupported.getJSONArray("diagnostics").getJSONObject(0)
        check(normalizedEntry.getString("parseStatus") == "UnsupportedValue")
        check(normalizedEntry.getJSONObject("serviceKey").getInt("serviceId") == key.serviceId)
        check(providerData.has("parentalRatingDiagnostics"))
    }



    @Test fun malformedAndTruncatedParentalRatingAreNotProjectedToContentRating() {
        val malformed = aribEvent(
            parentalRatings = listOf(AribParentalRating("JPN", 12, 12, supported = false)),
        ).copy(
            diagnosticDescriptorJson = """{"parentalRatingDescriptors":[{"parseStatus":"MalformedLength"}]}""",
        )
        val truncated = aribEvent(
            parentalRatings = listOf(AribParentalRating("JPN", 15, 15, supported = false)),
        ).copy(
            diagnosticDescriptorJson = """{"diagnostics":[{"parseStatus":"TruncatedDescriptor","tag":85}]}""",
        )
        val records = EventModelMapper().toProgramRecords(listOf(malformed, truncated))
        check(records.size == 2)
        records.forEach { record ->
            check(record.contentRatings.isEmpty())
            val providerData = TvProviderWriter.programProviderDataForTest(record)
            check(providerData.contains("unsupportedDescriptorDiagnostics"))
            check(providerData.contains("parentalRatingDiagnostics"))
        }
    }

    @Test fun unsupportedParentalRatingsDoNotWriteProgramsContentRatingColumn() {
        val store = FakeStore()
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        writer.upsertChannels(listOf(ChannelRecord(key, "101", "NHK", 473_142_857L)))
        val unsupported = EventModelMapper().toProgramRecords(listOf(
            aribEvent(parentalRatings = listOf(
                AribParentalRating("USA", 12, 12, supported = false),
                AribParentalRating("JPN", 3, 3, supported = true),
                AribParentalRating("JPN", 21, 21, supported = true),
            )),
        )).single()
        writer.upsertPrograms(listOf(unsupported))
        val values = store.programs.values.single()
        check(values.getAsString(TvContract.Programs.COLUMN_CONTENT_RATING) == null)
        val providerData = values.getAsByteArray(TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA).toString(Charsets.UTF_8)
        check(providerData.contains("unsupportedDescriptorDiagnostics"))
        check(providerData.contains("parentalRatingDiagnostics"))
    }

    @Test fun oldCustomAribRatingDomainIsNotGeneratedOnProductPath() {
        val flattened = requireNotNull(AribRatingMapper.toTvContentRatingString(AribParentalRating("JPN", 12, 12, supported = true)))
        check(flattened.contains("com.android.tv"))
        check(flattened.contains("ISDB"))
        check(flattened.contains("ISDB_12"))
        check(!flattened.contains("com.maleicacid.tvinput.arib"))
        check(!flattened.contains("AR" + "IB_JP"))
        check(!flattened.contains("AG" + "E_12"))
    }

    @Test fun mergedCasStateIsPersistedInProgramProviderData() {
        val event = aribEvent()
        val fallbackChannel = ChannelRecord(
            key,
            "101",
            "NHK",
            473_142_857L,
            requiresCas = true,
            unsupportedCas = true,
            clearLivePlaybackSupported = false,
            channelRegistrationReady = true,
            epgPublishable = true,
        )
        val incompleteDiagnostic = ServicePublishabilityDiagnostic(
            serviceKey = key,
            publishable = false,
            channelRegistrationReady = false,
            epgPublishable = false,
            clearLivePlaybackSupported = false,
            requiresCas = false,
            unsupportedCas = false,
            pmtPidResolved = false,
            pmtParsed = false,
            caStateResolved = false,
            freeCaModeResolved = false,
            missingComponents = listOf("NO_PMT"),
            reasons = listOf("CA_STATE_UNRESOLVED"),
            registrationReasons = listOf("NO_PMT"),
            epgReasons = listOf("NO_PMT"),
        )
        val record = EventModelMapper().toProgramRecords(
            events = listOf(event),
            publishabilityByServiceKey = mapOf(key to incompleteDiagnostic),
            channelFallbackByServiceKey = mapOf(key to fallbackChannel),
        ).single()
        val providerData = TvProviderWriter.programProviderDataForTest(record)
        check(providerData.contains("\"requiresCas\":true"))
        check(providerData.contains("\"unsupportedCas\":true"))
        check(providerData.contains("\"clearLivePlaybackSupported\":false"))
        check(providerData.contains("\"publishStateSource\":\"fallback\""))
    }

    @Test fun contentRatingWrittenToPrograms() {
        val store = FakeStore()
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        writer.upsertChannels(listOf(ChannelRecord(key, "101", "NHK", 473_142_857L)))
        val rating = requireNotNull(AribRatingMapper.toTvContentRatingString(AribParentalRating("JPN", 15, 15, true)))
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


    @Test fun localClearLivePlaybackRequiresRustDiagnostic() {
        val clearService = aribService(
            pmtPid = 0x100,
            pcrPid = 0x101,
            freeCaMode = false,
            streams = listOf(es(0x101, 0x1b)),
        )
        val clearDiag = publishability(clearService.serviceKey, clearLive = true)
        check(ServiceListBuilder.completenessForModel(clearService, clearDiag).isClearLivePlaybackSupported)
        check(ServiceListBuilder.completenessForModel(clearService, clearDiag).isRegistrationReady)

        check(!ServiceListBuilder.completenessForModel(clearService, publishability(clearService.serviceKey, clearLive = false, reasons = listOf("NO_PCR_PID"))).isClearLivePlaybackSupported)
        check(!ServiceListBuilder.completenessForModel(clearService, publishability(clearService.serviceKey, clearLive = false, reasons = listOf("SCRAMBLED_OR_UNKNOWN_SDT_FREE_CA_MODE"))).isClearLivePlaybackSupported)
        check(!ServiceListBuilder.completenessForModel(clearService, publishability(clearService.serviceKey, clearLive = false, reasons = listOf("NO_SUPPORTED_VIDEO_ES"))).isClearLivePlaybackSupported)
        check(!ServiceListBuilder.completenessForModel(clearService, publishability(clearService.serviceKey, clearLive = false, reasons = listOf("PMT_PROGRAM_CA_DESCRIPTOR"))).isClearLivePlaybackSupported)
        check(!ServiceListBuilder.completenessForModel(clearService, publishability(clearService.serviceKey, clearLive = false, reasons = listOf("VIDEO_ES_CA_DESCRIPTOR"))).isClearLivePlaybackSupported)
    }

    @Test fun scrambledCasPlaceholderServiceCanRegisterAndPublishEpgButNotClearLive() {
        val scrambledService = aribService(
            pmtPid = 0x100,
            pcrPid = 0x101,
            freeCaMode = true,
            streams = listOf(es(0x101, 0x1b)),
        )
        val diagnostic = ServicePublishabilityDiagnostic(
            serviceKey = scrambledService.serviceKey,
            publishable = true,
            channelRegistrationReady = true,
            epgPublishable = true,
            clearLivePlaybackSupported = false,
            requiresCas = true,
            unsupportedCas = true,
            pmtPidResolved = true,
            pmtParsed = true,
            caStateResolved = true,
            freeCaModeResolved = true,
            missingComponents = emptyList(),
            reasons = listOf("SCRAMBLED_OR_UNKNOWN_SDT_FREE_CA_MODE"),
            registrationReasons = emptyList(),
            epgReasons = emptyList(),
        )
        val completeness = ServiceListBuilder.completenessForModel(scrambledService, diagnostic)
        check(completeness.isRegistrationReady)
        check(completeness.isEpgPublishable)
        check(!completeness.isClearLivePlaybackSupported)
        check(completeness.requiresCas)
        check(completeness.unsupportedCas)

        val event = aribEvent()
        val record = EventModelMapper().toProgramRecords(listOf(event), mapOf(event.serviceKey to diagnostic)).single()
        check(record.requiresCas)
        check(record.unsupportedCas)
        check(!record.clearLivePlaybackSupported)
        check(record.channelRegistrationReady)
        check(record.epgPublishable)
        val providerData = TvProviderWriter.programProviderDataForTest(record)
        check(providerData.contains("\"requiresCas\":true"))
        check(providerData.contains("\"unsupportedCas\":true"))
        check(providerData.contains("\"clearLivePlaybackSupported\":false"))
        check(providerData.contains("\"channelRegistrationReady\":true"))
        check(providerData.contains("\"epgPublishable\":true"))
        check(providerData.contains("\"publishStateSource\":\"current\""))
    }

    @Test fun programCasStateFallsBackToExistingScrambledChannelWhenDiagnosticMissing() {
        val event = aribEvent()
        val fallbackChannel = ChannelRecord(
            key,
            "101",
            "NHK",
            473_142_857L,
            requiresCas = true,
            unsupportedCas = true,
            clearLivePlaybackSupported = false,
            channelRegistrationReady = true,
            epgPublishable = true,
        )
        val record = EventModelMapper().toProgramRecords(
            events = listOf(event),
            channelFallbackByServiceKey = mapOf(key to fallbackChannel),
        ).single()
        check(record.requiresCas)
        check(record.unsupportedCas)
        check(!record.clearLivePlaybackSupported)
        check(record.channelRegistrationReady)
        check(record.epgPublishable)
        check(record.publishStateSource == "CHANNEL_FALLBACK")
    }

    @Test fun incompleteCurrentDiagnosticMergesExistingScrambledChannelState() {
        val event = aribEvent()
        val fallbackChannel = ChannelRecord(
            key,
            "101",
            "NHK",
            473_142_857L,
            requiresCas = true,
            unsupportedCas = true,
            clearLivePlaybackSupported = false,
            channelRegistrationReady = true,
            epgPublishable = true,
        )
        val incompleteDiagnostic = ServicePublishabilityDiagnostic(
            serviceKey = key,
            publishable = false,
            channelRegistrationReady = false,
            epgPublishable = false,
            clearLivePlaybackSupported = false,
            requiresCas = false,
            unsupportedCas = false,
            pmtPidResolved = false,
            pmtParsed = false,
            caStateResolved = false,
            freeCaModeResolved = false,
            missingComponents = listOf("NO_PMT"),
            reasons = listOf("CA_STATE_UNRESOLVED"),
            registrationReasons = listOf("NO_PMT"),
            epgReasons = listOf("NO_PMT"),
        )
        val record = EventModelMapper().toProgramRecords(
            events = listOf(event),
            publishabilityByServiceKey = mapOf(key to incompleteDiagnostic),
            channelFallbackByServiceKey = mapOf(key to fallbackChannel),
        ).single()
        check(record.requiresCas)
        check(record.unsupportedCas)
        check(!record.clearLivePlaybackSupported)
        check(record.channelRegistrationReady)
        check(record.epgPublishable)
        check(record.publishStateSource == "MERGED_CHANNEL_CAS_STATE")
    }

    @Test fun completeCurrentDiagnosticOverridesExistingScrambledFallback() {
        val event = aribEvent()
        val fallbackChannel = ChannelRecord(
            key,
            "101",
            "NHK",
            473_142_857L,
            requiresCas = true,
            unsupportedCas = true,
            clearLivePlaybackSupported = false,
            channelRegistrationReady = true,
            epgPublishable = true,
        )
        val clearDiagnostic = ServicePublishabilityDiagnostic(
            serviceKey = key,
            publishable = true,
            channelRegistrationReady = true,
            epgPublishable = true,
            clearLivePlaybackSupported = true,
            requiresCas = false,
            unsupportedCas = false,
            pmtPidResolved = true,
            pmtParsed = true,
            caStateResolved = true,
            freeCaModeResolved = true,
            missingComponents = emptyList(),
            reasons = emptyList(),
            registrationReasons = emptyList(),
            epgReasons = emptyList(),
        )
        val record = EventModelMapper().toProgramRecords(
            events = listOf(event),
            publishabilityByServiceKey = mapOf(key to clearDiagnostic),
            channelFallbackByServiceKey = mapOf(key to fallbackChannel),
        ).single()
        check(!record.requiresCas)
        check(!record.unsupportedCas)
        check(record.clearLivePlaybackSupported)
        check(record.publishStateSource == "CURRENT_DIAGNOSTIC")
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
        check(ChannelScanController.siCollectionOutcomeForTest(false, false, 300, 250, 1, policy, registrationReadySnapshotAvailable = true) == ChannelScanController.SiCollectionOutcome.STABLE_PARTIAL)
        check(ChannelScanController.siCollectionOutcomeForTest(false, false, 1_000, 100, 1, policy, registrationReadySnapshotAvailable = true) == ChannelScanController.SiCollectionOutcome.TIMEOUT_PARTIAL)
        check(ChannelScanController.siCollectionOutcomeForTest(false, false, 1_000, 250, 0, policy) == ChannelScanController.SiCollectionOutcome.INCOMPLETE_NO_REGISTRATION_READY_SERVICE)
        check(ChannelScanController.siCollectionOutcomeForTest(true, false, 100, 0, 0, policy) == ChannelScanController.SiCollectionOutcome.COMPLETE)
    }

    @Test fun partialSiDiscoveryPublishesOnlyRegistrationReadySnapshotServices() {
        check(ChannelScanController.SiCollectionResult(ChannelScanController.SiCollectionOutcome.STABLE_PARTIAL, null, countsSignature = "v=1", clearLivePlaybackSupportedServices = 0, registrationReadyServices = 1, registrationReadySnapshotAvailable = true).mayPublishChannels)
        check(ChannelScanController.SiCollectionResult(ChannelScanController.SiCollectionOutcome.TIMEOUT_PARTIAL, null, countsSignature = "v=1", clearLivePlaybackSupportedServices = 0, registrationReadyServices = 1, registrationReadySnapshotAvailable = true).mayPublishChannels)
        check(!ChannelScanController.SiCollectionResult(ChannelScanController.SiCollectionOutcome.TIMEOUT_PARTIAL, null, countsSignature = "v=1", clearLivePlaybackSupportedServices = 0, registrationReadyServices = 1, registrationReadySnapshotAvailable = false).mayPublishChannels)
        check(!ChannelScanController.SiCollectionResult(ChannelScanController.SiCollectionOutcome.TIMEOUT_PARTIAL, null, countsSignature = "v=0", clearLivePlaybackSupportedServices = 0, registrationReadyServices = 0, registrationReadySnapshotAvailable = false).mayPublishChannels)
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

    @Test fun pmtGeneratesVideoAudioTrackIds() {
        val video = es(0x101, 0x1b)
        val audio = es(0x110, 0x0f, componentTag = 7, language = "jpn")
        check(TunerController.trackIdForVideoStream(video) == "video:257")
        check(TunerController.trackIdForAudioStream(audio) == "audio:272:7")
        val tracks = listOf(
            TunerController.TisTrack("video:257", TvTrackInfo.TYPE_VIDEO, 0x101, 0x1b, null, -1, null),
            TunerController.TisTrack("audio:272:7", TvTrackInfo.TYPE_AUDIO, 0x110, 0x0f, 7, -1, "jpn"),
        )
        check(TunerController.isSelectableTrackForTest(TvTrackInfo.TYPE_AUDIO, "audio:272:7", tracks))
        check(!TunerController.isSelectableTrackForTest(TvTrackInfo.TYPE_AUDIO, "audio:999", tracks))
        check(!TunerController.isSelectableTrackForTest(TvTrackInfo.TYPE_SUBTITLE, "subtitle:1", tracks))
    }


    @Test fun audioSelectTrackCommitsOnlyWhenAudioSwitchSucceeds() {
        val tracks = listOf(
            TunerController.TisTrack("audio:272:7", TvTrackInfo.TYPE_AUDIO, 0x110, 0x0f, 7, -1, "jpn"),
        )
        check(MaleicacidLiveSession.audioTrackSelectionAcceptedForTest("audio:272:7", tracks, audioSwitchSucceeded = true))
        check(!MaleicacidLiveSession.audioTrackSelectionAcceptedForTest("audio:272:7", tracks, audioSwitchSucceeded = false))
        check(!MaleicacidLiveSession.audioTrackSelectionAcceptedForTest("audio:999", tracks, audioSwitchSucceeded = true))
        check(!MaleicacidLiveSession.audioTrackSelectionAcceptedForTest(null, tracks, audioSwitchSucceeded = true))
    }


    @Test fun audioSelectTrackFailurePreservesExistingPlaybackSignature() {
        val previous = AvPlaybackSignature(
            serviceKey = key,
            pcrPid = 0x100,
            videoPid = 0x101,
            videoStreamType = 0x1b,
            audioPid = 0x110,
            audioStreamType = 0x0f,
            clear = true,
            keyTokenAvailable = false,
        )
        check(MaleicacidLiveSession.preservesExistingPlaybackWhenAudioSwitchFailsForTest(previous, previous, switchSucceeded = false))
    }

    @Test fun parentalBlockedReevaluationStopsPlayback() {
        check(MaleicacidLiveSession.shouldStopPlaybackWhenParentalControlBecomesBlockedForTest(blocked = true))
        check(!MaleicacidLiveSession.shouldStopPlaybackWhenParentalControlBecomesBlockedForTest(blocked = false))
        check(!MaleicacidLiveSession.parentalBlockUsesNotifyVideoUnavailableForTest())
    }

    @Test fun casPlaceholderUsesCasUnknownUnavailableReason() {
        check(MaleicacidLiveSession.casPlaceholderUnavailableReasonForTest() == android.media.tv.TvInputManager.VIDEO_UNAVAILABLE_REASON_CAS_UNKNOWN)
    }

    @Test fun parentalAllowedReevaluationRestartsPlaybackWhenServiceIsAvailable() {
        check(MaleicacidLiveSession.shouldRestartPlaybackAfterParentalControlAllowedForTest(latestServicePresent = true))
        check(!MaleicacidLiveSession.shouldRestartPlaybackAfterParentalControlAllowedForTest(latestServicePresent = false))
    }

    @Test fun unblockKeyIncludesCurrentProgramIdentityAndRating() {
        val rating = requireNotNull(AribRatingMapper.parseFlattened(requireNotNull(AribRatingMapper.toTvContentRatingString(AribParentalRating("JPN", 12, 12, true)))))
        val keyString = CurrentProgramRatingResolver.unblockKey(
            serviceKey = key,
            eventId = 10,
            ratingString = rating.flattenToString(),
        )
        check(keyString.contains("onid=4;tsid=16400;sid=101;event=10"))
        check(keyString.contains("ISDB_12"))
    }

    @Test fun onUnblockContentAcceptsOnlySameCurrentProgramRatingWithCompleteIdentity() {
        val rating12 = requireNotNull(AribRatingMapper.parseFlattened(requireNotNull(AribRatingMapper.toTvContentRatingString(AribParentalRating("JPN", 12, 12, true)))))
        val rating15 = requireNotNull(AribRatingMapper.parseFlattened(requireNotNull(AribRatingMapper.toTvContentRatingString(AribParentalRating("JPN", 15, 15, true)))))
        val current = CurrentProgramRatingResolver.CurrentProgramRatingSet(
            ratings = listOf(rating12),
            source = CurrentProgramRatingResolver.Source.LATEST_EIT_CACHE,
            channelUriString = "content://android.media.tv/channel/1",
            serviceKey = key,
            eventId = 10,
            startTimeMillis = 1_700_000_000_000L,
            endTimeMillis = 1_700_001_800_000L,
        )
        check(current.exactUnblockKeyFor(rating12) != null)
        check(current.exactUnblockKeyFor(rating15) == null)

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
        val records = MaleicacidLiveSession.videoMetadataProgramsForTest(
            events = listOf(aribEvent()),
            serviceKey = key,
            nowMillis = 1_700_000_000_100L,
            info = info,
        )
        val record = records.single()
        check(record.videoWidth == 1280)
        check(record.videoHeight == 720)
        check(record.videoFormat == "video/avc")
        check(TvProviderWriter.programProviderDataForTest(record).contains("videoFormat"))
    }

    @Test fun videoHeaderMetadataIgnoresNonCurrentEvent() {
        val info = PlaybackPipeline.VideoFormatInfo(0x1b, "video/avc", 1280, 720)
        val records = MaleicacidLiveSession.videoMetadataProgramsForTest(
            events = listOf(aribEvent().copy(startTimeMillis = 1_600_000_000_000L)),
            serviceKey = key,
            nowMillis = 1_700_000_000_100L,
            info = info,
        )
        check(records.isEmpty())
    }

    @Test fun cs110SelectorNoneRejectsTsidAndRelative() {
        check(TunerController.isCs110SelectorAllowedForTest("110CS", StreamSelector.NONE))
        check(!TunerController.isCs110SelectorAllowedForTest("110CS", StreamSelector.tsid(0x4010)))
        check(!TunerController.isCs110SelectorAllowedForTest("110CS", StreamSelector.relative(2)))
        check(TunerController.isCs110SelectorAllowedForTest("BS", StreamSelector.tsid(0x4010)))
    }

    @Test fun bootAndBackgroundMaintenanceUpdateExistingChannelsOnly() {
        val bootRetained = ChannelScanController.filterProgramServiceKeysForPublishModeForTest(
            mode = ChannelScanController.PublishMode.BOOT_EPG_SYNC,
            allServiceKeys = listOf(key, otherKey),
            existingServiceKeys = setOf(key),
            allowedServiceKeys = null,
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


    @Test fun bootEpgSyncStartRequiresIdleScanAndNoLiveSession() {
        val active = ChannelScanManager.bootEpgSyncStartDecisionForTest(activeLiveSessionCount = 1, scanRunning = false)
        check(!active.allowed)
        check(active.reason == "ACTIVE_LIVE_SESSION")

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
        check(active.reason == "ACTIVE_LIVE_SESSION")

        val scanRunning = ChannelScanManager.backgroundMaintenanceStartDecisionForTest(activeLiveSessionCount = 0, scanRunning = true)
        check(!scanRunning.allowed)
        check(scanRunning.reason == "SCAN_RUNNING")

        val idle = ChannelScanManager.backgroundMaintenanceStartDecisionForTest(activeLiveSessionCount = 0, scanRunning = false)
        check(idle.allowed)
        check(idle.reason == null)
    }

    @Test fun sectionStatusDiagnosticsAreBucketedByStatus() {
        check(SectionIngestController.statusBucketForTest(SiStatus.OK) == "accepted")
        check(SectionIngestController.statusBucketForTest(SiStatus.INVALID_SECTION) == "crc")
        check(SectionIngestController.statusBucketForTest(SiStatus.MALFORMED_DESCRIPTOR) == "malformed")
        check(SectionIngestController.statusBucketForTest(SiStatus.INTERNAL_ERROR) == "malformed")
    }

    @Test fun sectionShortReadIsDiagnosticAndNotIngest() {
        check(TunerController.sectionReadDecisionForTest(expected = 64, actual = 64, generationMatches = true) == TunerController.SectionReadDecision.INGEST)
        check(TunerController.sectionReadDecisionForTest(expected = 64, actual = 12, generationMatches = true) == TunerController.SectionReadDecision.SHORT_READ)
        check(TunerController.sectionReadDecisionForTest(expected = 64, actual = 0, generationMatches = true) == TunerController.SectionReadDecision.READ_ERROR)
        check(TunerController.sectionReadDecisionForTest(expected = 64, actual = 64, generationMatches = false) == TunerController.SectionReadDecision.STALE_GENERATION)
    }

    @Test fun firstFrameTimeoutUsesGenerationAndNotificationState() {
        check(PlaybackPipeline.shouldTriggerFirstFrameTimeoutForTest(timeoutGeneration = 3, currentGeneration = 3, alreadyNotified = false))
        check(!PlaybackPipeline.shouldTriggerFirstFrameTimeoutForTest(timeoutGeneration = 2, currentGeneration = 3, alreadyNotified = false))
        check(!PlaybackPipeline.shouldTriggerFirstFrameTimeoutForTest(timeoutGeneration = 3, currentGeneration = 3, alreadyNotified = true))
    }

    @Test fun audioMasterHoldsVideoUntilAudioAnchorOrFallback() {
        check(PlaybackPipeline.syncModeForTest(audioExpected = true) == "AUDIO_MASTER")
        check(PlaybackPipeline.syncModeForTest(audioExpected = false) == "VIDEO_MASTER")
        check(PlaybackPipeline.shouldHoldVideoBeforeAudioAnchorForTest(audioExpected = true, audioAnchored = false, fallbackDeadlineReached = false))
        check(!PlaybackPipeline.shouldHoldVideoBeforeAudioAnchorForTest(audioExpected = true, audioAnchored = true, fallbackDeadlineReached = false))
        check(!PlaybackPipeline.shouldHoldVideoBeforeAudioAnchorForTest(audioExpected = true, audioAnchored = false, fallbackDeadlineReached = true))
        check(!PlaybackPipeline.shouldHoldVideoBeforeAudioAnchorForTest(audioExpected = false, audioAnchored = false, fallbackDeadlineReached = false))
    }

    @Test fun mediaEventCallbackFailureMapsToRecoverableUnavailableReason() {
        check(PlaybackPipeline.unavailableReasonForMediaEventCallbackFailureForTest(isAudio = true) == PlaybackPipeline.PlaybackUnavailableReason.AUDIO_UNAVAILABLE)
        check(PlaybackPipeline.unavailableReasonForMediaEventCallbackFailureForTest(isAudio = false) == PlaybackPipeline.PlaybackUnavailableReason.VIDEO_CODEC_ERROR)
    }

    @Test fun programPublishSignatureChangesOnlyWhenProjectedContentChanges() {
        val first = listOf(program(key, description = "desc"))
        val same = listOf(program(key, description = "desc"))
        val changedDescription = listOf(program(key, description = "updated"))
        val changedRating = listOf(program(key, description = "desc", contentRatings = listOf(requireNotNull(AribRatingMapper.toTvContentRatingString(AribParentalRating("JPN", 18, 18, true))))))
        val changedUnsupported = listOf(program(key, description = "desc", unsupportedDescriptorJson = "{\"tag\":255}"))
        check(ProgramPublishCoordinator.programSignatureForTest(first) == ProgramPublishCoordinator.programSignatureForTest(same))
        check(ProgramPublishCoordinator.programSignatureForTest(first) != ProgramPublishCoordinator.programSignatureForTest(changedDescription))
        check(ProgramPublishCoordinator.programSignatureForTest(first) != ProgramPublishCoordinator.programSignatureForTest(changedRating))
        check(ProgramPublishCoordinator.programSignatureForTest(first) != ProgramPublishCoordinator.programSignatureForTest(changedUnsupported))
    }

    @Test fun currentDiagnosticCompleteRequiresExplicitPmtAndCaStateFields() {
        val incompletePmt = publishability(key, clearLive = true).copy(
            pmtPidResolved = false,
            pmtParsed = false,
        )
        check(!incompletePmt.isCurrentDiagnosticComplete())

        val incompleteCaState = publishability(key, clearLive = true).copy(
            caStateResolved = false,
            freeCaModeResolved = false,
        )
        check(!incompleteCaState.isCurrentDiagnosticComplete())

        val complete = publishability(key, clearLive = true)
        check(complete.isCurrentDiagnosticComplete())
    }

    @Test fun currentProgramChangeClearsTemporaryUnblockKeys() {
        val keys = linkedSetOf("old-unblock-key")
        var identity = MaleicacidLiveSession.updateUnblockStateForProgramChangeForTest(
            previousIdentityKey = "program-a",
            nextIdentityKey = "program-a",
            unblockedContentKeys = keys,
        )
        check(identity == "program-a")
        check("old-unblock-key" in keys)

        identity = MaleicacidLiveSession.updateUnblockStateForProgramChangeForTest(
            previousIdentityKey = identity,
            nextIdentityKey = "program-b",
            unblockedContentKeys = keys,
        )
        check(identity == "program-b")
        check(keys.isEmpty())

        keys += "program-b-unblock"
        identity = MaleicacidLiveSession.updateUnblockStateForProgramChangeForTest(
            previousIdentityKey = identity,
            nextIdentityKey = null,
            unblockedContentKeys = keys,
        )
        check(identity == null)
        check(keys.isEmpty())
    }


    private fun aribService(
        pmtPid: Int?,
        pcrPid: Int?,
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
        publishable = clearLive,
        channelRegistrationReady = clearLive,
        epgPublishable = clearLive,
        clearLivePlaybackSupported = clearLive,
        requiresCas = false,
        unsupportedCas = false,
        pmtPidResolved = clearLive,
        pmtParsed = clearLive,
        caStateResolved = clearLive,
        freeCaModeResolved = clearLive,
        missingComponents = emptyList(),
        reasons = reasons,
        registrationReasons = if (clearLive) emptyList() else reasons,
        epgReasons = if (clearLive) emptyList() else reasons,
    )

    private fun es(pid: Int, streamType: Int, componentTag: Int? = null, language: String? = null): AribElementaryStream =
        AribElementaryStream(
            elementaryPid = pid,
            streamType = streamType,
            componentTag = componentTag,
            componentType = null,
            streamContent = null,
            languageCodes = listOfNotNull(language),
        )

    private fun aribEvent(parentalRatings: List<AribParentalRating> = emptyList()): AribEvent = AribEvent(
        serviceKey = key,
        stableIdentity = "onid=4;tsid=16400;sid=101;event=1",
        eventId = 1,
        startTimeMillis = 1_700_000_000_000L,
        durationMillis = 1_800_000L,
        title = "title",
        description = "desc",
        parentalRatings = parentalRatings,
    )

    private fun program(
        serviceKey: ServiceKey,
        description: String = "desc",
        contentRatings: List<String> = emptyList(),
        unsupportedDescriptorJson: String = "{}",
    ): ProgramRecord = ProgramRecord(
        serviceKey = serviceKey,
        eventId = 1,
        stableIdentity = "onid=${serviceKey.originalNetworkId};tsid=${serviceKey.transportStreamId};sid=${serviceKey.serviceId};event=1",
        startTimeMillis = 1_700_000_000_000L,
        durationMillis = 1_800_000L,
        title = "title",
        description = description,
        contentRatings = contentRatings,
        unsupportedDescriptorJson = unsupportedDescriptorJson,
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
        override fun findExistingProgramId(channelId: Long, programKey: String): Result<Long?> = Result.success(
            programs.entries.firstOrNull { (_, v) ->
                v.getAsLong(TvContract.Programs.COLUMN_CHANNEL_ID) == channelId && TvProviderWriter.parseProgramKey(v.getAsByteArray(TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA).toString(Charsets.UTF_8)) == programKey
            }?.key,
        )
        override fun indexExistingProgramsForWindow(channelId: Long, windowStartMs: Long, windowEndMs: Long): Result<Map<String, Long>> = Result.success(
            programs.entries.mapNotNull { (id, v) ->
                if (v.getAsLong(TvContract.Programs.COLUMN_CHANNEL_ID) != channelId) return@mapNotNull null
                val end = v.getAsLong(TvContract.Programs.COLUMN_END_TIME_UTC_MILLIS)
                val start = v.getAsLong(TvContract.Programs.COLUMN_START_TIME_UTC_MILLIS)
                if (end <= windowStartMs || start >= windowEndMs) return@mapNotNull null
                val key = TvProviderWriter.parseProgramKey(v.getAsByteArray(TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA).toString(Charsets.UTF_8)) ?: return@mapNotNull null
                key to id
            }.toMap(),
        )
        override fun insertProgram(values: ContentValues): Result<Long?> { val id = nextProgramId++; programs[id] = ContentValues(values); return Result.success(id) }
        override fun updateProgram(programId: Long, values: ContentValues): Result<Int> { programs[programId] = ContentValues(values); return Result.success(1) }
    }
}

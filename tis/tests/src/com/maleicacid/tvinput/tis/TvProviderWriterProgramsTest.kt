package com.maleicacid.tvinput.tis

import android.content.ContentValues
import android.media.tv.TvContract
import com.maleicacid.tvinput.aribsi.AribComponentEntry
import com.maleicacid.tvinput.aribsi.AribComponents
import com.maleicacid.tvinput.aribsi.AribContentGenre
import com.maleicacid.tvinput.aribsi.AribParentalRating
import com.maleicacid.tvinput.aribsi.AribRatingMapper
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.db.ChannelRecord
import com.maleicacid.tvinput.db.ProgramRecord
import com.maleicacid.tvinput.db.ProgramDescriptors
import com.maleicacid.tvinput.aribsi.AribSeries
import com.maleicacid.tvinput.aribsi.AribRelatedItem
import com.maleicacid.tvinput.aribsi.AribFreeCaMode
import com.maleicacid.tvinput.aribsi.AribExtendedItem
import org.junit.Test

class TvProviderWriterProgramsTest {
    private val key = ServiceKey(4, 16625, 101)

    @Test fun insertAndUpdateProgram() {
        val store = FakeStore()
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        writer.upsertChannels(listOf(ChannelRecord(key, "101", "NHK", 473_142_857L)))
        val p = ProgramRecord(key, 10, "{\"kind\":\"arib-event-v1\",\"originalNetworkId\":4,\"transportStreamId\":16625,\"serviceId\":101,\"eventId\":10}", 1_700_000_000_000L, 1_800_000L, "News", "desc")
        val first = writer.upsertPrograms(listOf(p))
        check(first.inserted == 1)
        val second = writer.upsertPrograms(listOf(p.copy(description = "updated", shortDescription = "updated")))
        check(second.updated == 1)
        check(store.programs.values.single().getAsString(TvContract.Programs.COLUMN_SHORT_DESCRIPTION) == "updated")
        check(store.programs.values.single().getAsString(TvContract.Programs.COLUMN_LONG_DESCRIPTION) == "updated")
        check(store.programs.values.single().getAsInteger(TvContract.Programs.COLUMN_EVENT_ID) == 10)
        val providerData = store.programs.values.single().getAsByteArray(TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA)
        check(TvProviderWriter.parseProgramKey(providerData) == TvProviderWriter.programKeyForTest(p))
        check(providerData.utf8Contains("\"schema\":\"maleicacid.tv.program\""))
        check(providerData.utf8Contains(TvProviderWriter.PROGRAM_KEY_FIELD))
        check(!providerData.utf8Contains("programKeyB64"))
    }


    @Test fun sameEventWithMovedTimeUpdatesExistingRowOutsideNewWindow() {
        val store = FakeStore()
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        writer.upsertChannels(listOf(ChannelRecord(key, "101", "NHK", 473_142_857L)))
        val original = ProgramRecord(key, 10, "{\"kind\":\"arib-event-v1\",\"originalNetworkId\":4,\"transportStreamId\":16625,\"serviceId\":101,\"eventId\":10}", 1_700_000_000_000L, 1_800_000L, "News", "desc")
        val first = writer.upsertPrograms(listOf(original))
        check(first.inserted == 1) { first.toString() }

        val moved = original.copy(
            startTimeMillis = 1_700_007_200_000L,
            durationMillis = 1_800_000L,
            shortDescription = "moved",
            description = "moved",
        )
        val second = writer.upsertPrograms(listOf(moved))
        check(second.inserted == 0) { second.toString() }
        check(second.updated == 1) { second.toString() }
        check(store.programs.size == 1)
        val row = store.programs.values.single()
        check(row.getAsLong(TvContract.Programs.COLUMN_START_TIME_UTC_MILLIS) == moved.startTimeMillis)
        check(row.getAsString(TvContract.Programs.COLUMN_LONG_DESCRIPTION) == "moved")
        val providerData = row.getAsByteArray(TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA)
        check(TvProviderWriter.parseProgramKey(providerData) == TvProviderWriter.programKeyForTest(moved))
    }

    @Test fun programProviderDataContainsCasAndReadinessState() {
        val store = FakeStore()
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        writer.upsertChannels(listOf(ChannelRecord(key, "101", "NHK", 473_142_857L)))
        val p = ProgramRecord(
            key,
            14,
            "{\"kind\":\"arib-event-v1\",\"originalNetworkId\":4,\"transportStreamId\":16625,\"serviceId\":101,\"eventId\":14}",
            1_700_000_000_000L,
            1_800_000L,
            "Scrambled EPG",
            "desc",
            requiresCas = true,
            unsupportedCas = true,
            clearLivePlaybackSupported = false,
            channelRegistrationReady = true,
            epgPublishable = true,
        )
        writer.upsertPrograms(listOf(p))
        val providerData = store.programs.values.single().getAsByteArray(TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA)
        check(providerData.utf8Contains("\"requiresCas\":true"))
        check(providerData.utf8Contains("\"unsupportedCas\":true"))
        check(providerData.utf8Contains("\"clearLivePlaybackSupported\":false"))
        check(providerData.utf8Contains("\"channelRegistrationReady\":true"))
        check(providerData.utf8Contains("\"epgPublishable\":true"))
    }

    @Test fun descriptorDetailsStayInInternalProviderData() {
        val store = FakeStore()
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        writer.upsertChannels(listOf(ChannelRecord(key, "101", "NHK", 473_142_857L)))
        val p = ProgramRecord(
            key, 11, "{\"kind\":\"arib-event-v1\",\"originalNetworkId\":4,\"transportStreamId\":16625,\"serviceId\":101,\"eventId\":11}", 1_700_000_000_000L, 1_800_000L,
            "News", "desc",
            canonicalGenres = listOf("NEWS"),
            descriptors = ProgramDescriptors(
                extendedItems = listOf(AribExtendedItem("出演", "A")),
                componentText = "映像",
                audioComponentText = "音声",
                contentGenres = listOf(AribContentGenre(0x0, 0x0, aribName = "ニュース/報道/定時・総合")),
                broadcastGenre = "ARIB(0x0/0x0):ニュース/報道/定時・総合",
                genreSupplementText = "ニュース/報道/定時・総合",
                relatedItems = listOf(AribRelatedItem("shared", 1, 4, 16625, 101, 202)),
                scrambled = false,
                freeCaMode = AribFreeCaMode(raw = 0, scrambled = false, text = "無料放送"),
                series = AribSeries(seriesId = 100, episodeNumber = 3, lastEpisodeNumber = 12, name = "シリーズ"),
                components = AribComponents(audio = listOf(AribComponentEntry(esPid = 256, streamType = 0x0f, componentTag = 1, componentType = 3, codec = "AAC", language = "jpn", parseStatus = "OK"))),
            ),
            diagnosticText = "unknownCount=0",
        )
        writer.upsertPrograms(listOf(p))
        val providerData = store.programs.values.single().getAsByteArray(TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA)
        check(providerData.utf8Contains("extendedItems"))
        check(providerData.utf8Contains("audioLanguages"))
        check(providerData.utf8Contains("genres"))
        check(providerData.utf8Contains("diagnostics"))
        check(!providerData.utf8Contains("componentText"))
        check(!providerData.utf8Contains("audioComponentText"))
        check(!providerData.utf8Contains("genreSupplementText"))
        check(!providerData.utf8Contains("eventGroupText"))
        check(providerData.utf8Contains("relatedItems"))
        check(providerData.utf8Contains("freeCaMode"))
        check(providerData.utf8Contains("seriesId"))
        check(store.programs.values.single().getAsString(TvContract.Programs.COLUMN_AUDIO_LANGUAGE) == "jpn")
        check(store.programs.values.single().getAsString(TvContract.Programs.COLUMN_CANONICAL_GENRE) == TvContract.Programs.Genres.encode("NEWS"))
        check(store.programs.values.single().getAsString(TvContract.Programs.COLUMN_BROADCAST_GENRE) == TvContract.Programs.Genres.encode("ARIB(0x0/0x0):ニュース/報道/定時・総合"))
        check(store.programs.values.single().getAsInteger(TvProviderWriter.COLUMN_SCRAMBLED) == 0)
        check(store.programs.values.single().getAsInteger(TvProviderWriter.COLUMN_SERIES_ID) == 100)
        check(store.programs.values.single().getAsString(TvProviderWriter.COLUMN_EPISODE_DISPLAY_NUMBER) == "3")
        check(store.programs.values.single().getAsInteger(TvProviderWriter.COLUMN_ITEM_COUNT) == 12)
        check(store.programs.values.single().getAsString(TvContract.Programs.COLUMN_SHORT_DESCRIPTION) == "desc")
    }


    @Test fun liveProgramCoordinatorPublishesOnlyExistingChannelAndProjectedChanges() {
        val store = FakeStore()
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        val coordinator = ProgramPublishCoordinator(writer)
        val p = ProgramRecord(key, 12, "{\"kind\":\"arib-event-v1\",\"originalNetworkId\":4,\"transportStreamId\":16625,\"serviceId\":101,\"eventId\":12}", 1_700_000_000_000L, 1_800_000L, "News", "desc")

        val missingChannel = coordinator.publish(ChannelScanController.PublishMode.LIVE_TUNE_REFRESH, listOf(p), allowedServiceKeys = null)
        check(missingChannel.inserted == 0 && missingChannel.updated == 0 && store.programs.isEmpty())

        writer.upsertChannels(listOf(ChannelRecord(key, "101", "NHK", 473_142_857L)))
        val first = coordinator.publish(ChannelScanController.PublishMode.LIVE_TUNE_REFRESH, listOf(p), allowedServiceKeys = null)
        check(first.inserted == 1)

        val same = coordinator.publish(ChannelScanController.PublishMode.LIVE_TUNE_REFRESH, listOf(p), allowedServiceKeys = null)
        check(same.inserted == 0 && same.updated == 0)

        val changedDescription = coordinator.publish(ChannelScanController.PublishMode.LIVE_TUNE_REFRESH, listOf(p.copy(description = "updated", shortDescription = "updated")), allowedServiceKeys = null)
        check(changedDescription.updated == 1)
        check(store.programs.values.single().getAsString(TvContract.Programs.COLUMN_LONG_DESCRIPTION) == "updated")

        val rating18 = requireNotNull(AribRatingMapper.toTvContentRatingString(AribParentalRating("JPN", 18, 18, true)))
        val changedProjectedDetails = coordinator.publish(
            ChannelScanController.PublishMode.LIVE_TUNE_REFRESH,
            listOf(p.copy(contentRatings = listOf(rating18), videoFormat = "video/avc", videoWidth = 1920, videoHeight = 1080)),
            allowedServiceKeys = null,
        )
        check(changedProjectedDetails.updated == 1)
        val providerData = store.programs.values.single().getAsByteArray(TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA)
        check(providerData.utf8Contains("descriptorDiagnostics"))
        check(providerData.utf8Contains("video/avc"))
        check(!providerData.utf8Contains("unsupportedDescriptorDiagnostics"))
        check(!providerData.utf8Contains("videoFormat"))
        check(store.programs.values.single().getAsString(TvContract.Programs.COLUMN_CONTENT_RATING) == rating18)
    }

    @Test fun liveVideoMetadataSurvivesLaterEitProgramPublish() {
        val store = FakeStore()
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        val coordinator = ProgramPublishCoordinator(writer)
        val p = ProgramRecord(key, 13, "{\"kind\":\"arib-event-v1\",\"originalNetworkId\":4,\"transportStreamId\":16625,\"serviceId\":101,\"eventId\":13}", 1_700_000_000_000L, 1_800_000L, "News", "desc")
        val info = PlaybackPipeline.VideoFormatInfo(0x1b, "video/avc", 1280, 720)
        val metadata = mapOf(MaleicacidLiveSession.programVideoMetadataKeyForTest(p) to info)

        writer.upsertChannels(listOf(ChannelRecord(key, "101", "NHK", 473_142_857L)))
        val withVideoMetadata = MaleicacidLiveSession.mergeVideoMetadataForTest(listOf(p), metadata)
        val first = coordinator.publish(ChannelScanController.PublishMode.LIVE_TUNE_REFRESH, withVideoMetadata, allowedServiceKeys = null)
        check(first.inserted == 1)
        var providerData = store.programs.values.single().getAsByteArray(TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA)
        check(providerData.utf8Contains("video/avc"))
        check(!providerData.utf8Contains("videoFormat"))

        val laterEitRecord = p.copy(durationMillis = 2_400_000L, description = "EIT更新", shortDescription = "EIT更新")
        val mergedLaterEit = MaleicacidLiveSession.mergeVideoMetadataForTest(listOf(laterEitRecord), metadata)
        val updated = coordinator.publish(ChannelScanController.PublishMode.LIVE_TUNE_REFRESH, mergedLaterEit, allowedServiceKeys = null)
        check(updated.updated == 1)
        providerData = store.programs.values.single().getAsByteArray(TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA)
        check(providerData.utf8Contains("video/avc"))
        check(!providerData.utf8Contains("videoFormat"))
        check(store.programs.values.single().getAsString(TvContract.Programs.COLUMN_LONG_DESCRIPTION) == "EIT更新")
    }

    @Test fun obsoleteProgramsInsideCurrentUpdateWindowAreDeletedOnlyWhenAuthoritative() {
        val store = FakeStore()
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        writer.upsertChannels(listOf(ChannelRecord(key, "101", "NHK", 473_142_857L)))
        val p1 = ProgramRecord(key, 21, "{\"kind\":\"arib-event-v1\",\"originalNetworkId\":4,\"transportStreamId\":16625,\"serviceId\":101,\"eventId\":21}", 1_700_000_000_000L, 1_800_000L, "P1", "desc")
        val p2 = ProgramRecord(key, 22, "{\"kind\":\"arib-event-v1\",\"originalNetworkId\":4,\"transportStreamId\":16625,\"serviceId\":101,\"eventId\":22}", 1_700_000_600_000L, 600_000L, "P2", "desc")
        val p3 = ProgramRecord(key, 23, "{\"kind\":\"arib-event-v1\",\"originalNetworkId\":4,\"transportStreamId\":16625,\"serviceId\":101,\"eventId\":23}", 1_700_001_200_000L, 600_000L, "P3", "desc")
        val first = writer.upsertPrograms(listOf(p1, p2, p3))
        check(first.inserted == 3)

        val nonAuthoritative = writer.upsertProgramsForWindows(
            programs = listOf(p1, p3),
            windows = listOf(
                ProgramPublishCoordinator.EpgUpdateWindow(
                    serviceKey = key,
                    windowStartMs = p1.startTimeMillis,
                    windowEndMs = p3.startTimeMillis + p3.durationMillis,
                    validProgramKeys = setOf(TvProviderWriter.programKeyForTest(p1), TvProviderWriter.programKeyForTest(p3)),
                    deletionAuthoritative = false,
                ),
            ),
        )
        check(nonAuthoritative.deleted == 0)
        check(store.programs.size == 3)

        val authoritative = writer.upsertProgramsForWindows(
            programs = listOf(p1, p3),
            windows = listOf(
                ProgramPublishCoordinator.EpgUpdateWindow(
                    serviceKey = key,
                    windowStartMs = p1.startTimeMillis,
                    windowEndMs = p3.startTimeMillis + p3.durationMillis,
                    validProgramKeys = setOf(TvProviderWriter.programKeyForTest(p1), TvProviderWriter.programKeyForTest(p3)),
                    deletionAuthoritative = true,
                ),
            ),
        )
        check(authoritative.deleted == 1)
        check(store.programs.size == 2)
        check(store.programs.values.none { value ->
            TvProviderWriter.parseProgramKey(value.getAsByteArray(TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA)) == TvProviderWriter.programKeyForTest(p2)
        })
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
                v.getAsLong(TvContract.Programs.COLUMN_CHANNEL_ID) == channelId && TvProviderWriter.parseProgramKey(v.getAsByteArray(TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA)) == programKey
            }?.key,
        )
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
        override fun deleteObsoletePrograms(channelId: Long, validProgramKeys: Set<String>, windowStartMs: Long, windowEndMs: Long): Result<Int> {
            val obsolete = programs.filter { (_, v) ->
                if (v.getAsLong(TvContract.Programs.COLUMN_CHANNEL_ID) != channelId) return@filter false
                val start = v.getAsLong(TvContract.Programs.COLUMN_START_TIME_UTC_MILLIS)
                val end = v.getAsLong(TvContract.Programs.COLUMN_END_TIME_UTC_MILLIS)
                if (end <= windowStartMs || start >= windowEndMs) return@filter false
                val key = TvProviderWriter.parseProgramKey(v.getAsByteArray(TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA))
                key == null || key !in validProgramKeys
            }.keys.toList()
            obsolete.forEach { programs.remove(it) }
            return Result.success(obsolete.size)
        }
    }
}

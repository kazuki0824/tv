package com.maleicacid.tvinput.tis

import android.content.ContentValues
import android.media.tv.TvContract
import com.maleicacid.tvinput.aribsi.AribComponentEntry
import com.maleicacid.tvinput.aribsi.AribComponents
import com.maleicacid.tvinput.aribsi.AribContentGenre
import com.maleicacid.tvinput.aribsi.AribParentalRating
import com.maleicacid.tvinput.aribsi.AribRatingMapper
import com.maleicacid.tvinput.common.FrequencyHz
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.TsPid
import com.maleicacid.tvinput.db.ChannelRecord
import com.maleicacid.tvinput.db.ProgramRecord
import com.maleicacid.tvinput.aribsi.AribFreeCaMode
import com.maleicacid.tvinput.aribsi.AribSeries
import com.maleicacid.tvinput.db.ProgramDescriptors
import org.junit.Test

class TvProviderWriterR51FixTest {
    private val key = ServiceKey(4, 16625, 101)

    @Test fun optionalProgramColumnsAreClearedByMergeUpdate() {
        val store = MergeStore()
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        writer.upsertChannels(listOf(ChannelRecord(key, 0x01, "101", "NHK", FrequencyHz(473_142_857L))))
        val rating15 = requireNotNull(
            AribRatingMapper.toTvContentRatingString(
                AribParentalRating("JPN", 15),
                AribRatingMapper.BroadcastProfile.BS_CS,
            ),
        )
        val p = ProgramRecord(
            key, 1, "p1", 1_700_000_000_000L, 1_800_000L, "title", "desc",
            canonicalGenres = listOf("NEWS"),
            descriptors = ProgramDescriptors(
                contentGenres = listOf(AribContentGenre(0x0, 0x0, aribName = "ニュース/報道/定時・総合")),
                broadcastGenre = "ARIB(0x0/0x0):ニュース/報道/定時・総合",
                scrambled = false,
                freeCaMode = AribFreeCaMode(raw = 0, scrambled = false),
                series = AribSeries(seriesId = 100, episodeNumber = 3, lastEpisodeNumber = 12, name = null),
                components = AribComponents(audio = listOf(AribComponentEntry(esPid = TsPid(256), streamType = 0x0f, componentTag = 1, componentType = 3, codec = "AAC", language = "jpn", parseStatus = "OK"))),
            ),
            contentRatings = listOf(rating15),
        )
        writer.upsertPrograms(listOf(p))
        writer.upsertPrograms(listOf(p.copy(canonicalGenres = emptyList(), descriptors = ProgramDescriptors(), contentRatings = emptyList())))
        val values = store.programs.values.single()
        check(values.get(TvContract.Programs.COLUMN_AUDIO_LANGUAGE) == null)
        check(values.get(TvContract.Programs.COLUMN_BROADCAST_GENRE) == null)
        check(values.get(TvContract.Programs.COLUMN_CANONICAL_GENRE) == null)
        check(values.get(TvContract.Programs.COLUMN_CONTENT_RATING) == null)
        check(values.get(TvProviderWriter.COLUMN_SCRAMBLED) == null)
        check(values.get(TvProviderWriter.COLUMN_SERIES_ID) == null)
        check(values.get(TvProviderWriter.COLUMN_EPISODE_DISPLAY_NUMBER) == null)
        check(values.get("item_count") == null)
    }

    @Test fun genreReadbackKeepsDirectProjectionSeparateFromProviderObservation() {
        val store = MergeStore()
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        writer.upsertChannels(listOf(ChannelRecord(key, 0x01, "101", "NHK", FrequencyHz(473_142_857L))))
        val program = ProgramRecord(key, 1, "p1", 1_700_000_000_000L, 1_800_000L, "番組", "本文", canonicalGenres = listOf("NEWS"))
        store.genreReadback = Result.success("MOVIES")
        val first = writer.upsertPrograms(listOf(program))
        check(first.inserted == 1 && first.failures.isEmpty())
        val diagnostic = first.genreDiagnostics.single()
        check(diagnostic.directlySetCanonicalGenre == "NEWS")
        check(diagnostic.readBackCanonicalGenre == "MOVIES")
        check(diagnostic.readFailure == null)
        store.genreReadback = Result.failure(IllegalStateException("読戻し失敗"))
        val second = writer.upsertPrograms(listOf(program))
        check(second.updated == 1 && second.failures.isEmpty())
        check(second.genreDiagnostics.single().readFailure == "読戻し失敗")
    }

    @Test fun publicationFingerprintUsesActualChannelAndWindowAndWritesPreparedBytes() {
        val store = MergeStore()
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        writer.upsertChannels(listOf(ChannelRecord(key, 0x01, "101", "NHK", FrequencyHz(473_142_857L))))
        val program = ProgramRecord(key, 1, "p1", 1_700_000_000_000L, 1_800_000L, "番組", "本文")
        val window = ProgramPublishCoordinator.EpgUpdateWindow(key, program.startTimeMillis, program.startTimeMillis + program.durationMillis, setOf(TvProviderWriter.programKeyForTest(program)))
        val plan = writer.prepareProgramPublication(listOf(program), listOf(window))
        check(requireNotNull(plan.fingerprint).matches(Regex("[0-9a-f]{64}")))
        val values = plan.services.single().programs.single().second
        check(values.getAsLong(TvContract.Programs.COLUMN_CHANNEL_ID) == 1L)
        writer.upsertPreparedPrograms(plan)
        check(store.programs.values.single().getAsByteArray(TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA).contentEquals(values.getAsByteArray(TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA)))
        check(writer.prepareProgramPublication(listOf(program), listOf(window.copy(windowEndMs = window.windowEndMs + 1))).fingerprint != plan.fingerprint)

        val coordinator = ProgramPublishCoordinator(writer)
        check(coordinator.publish(ChannelScanController.PublishMode.SETUP_SCAN, listOf(program), null).updated == 1)
        check(coordinator.publish(ChannelScanController.PublishMode.SETUP_SCAN, listOf(program), null).skippedUnchanged == 1)
        store.channels[17L] = requireNotNull(store.channels.remove(1L))
        check(writer.prepareProgramPublication(listOf(program), listOf(window)).fingerprint != plan.fingerprint)
        check(coordinator.publish(ChannelScanController.PublishMode.SETUP_SCAN, listOf(program), null).updated == 1)
        check(store.programs.values.single().getAsLong(TvContract.Programs.COLUMN_CHANNEL_ID) == 17L)
    }

    private class MergeStore : TvProviderWriter.ChannelStore {
        private var nextChannelId = 1L
        private var nextProgramId = 100L
        val channels = LinkedHashMap<Long, ContentValues>()
        val programs = LinkedHashMap<Long, ContentValues>()
        var genreReadback: Result<String?> = Result.success(null)
        override fun readCanonicalGenre(programId: Long): Result<String?> = genreReadback
        override fun findExistingChannelId(key: ServiceKey): Result<Long?> = Result.success(channels.keys.firstOrNull())
        override fun insertChannel(values: ContentValues): Result<Long?> { val id = nextChannelId++; channels[id] = ContentValues(values); return Result.success(id) }
        override fun updateChannel(channelId: Long, values: ContentValues): Result<Int> { channels[channelId]?.putAll(values); return Result.success(1) }
        override fun indexExistingProgramsForWindow(channelId: Long, windowStartMs: Long, windowEndMs: Long): Result<Map<String, Long>> = Result.success(
            programs.entries.mapNotNull { (id, v) ->
                val key = TvProviderWriter.parseProgramKey(v.getAsByteArray(TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA)) ?: return@mapNotNull null
                key to id
            }.toMap(),
        )
        override fun insertProgram(values: ContentValues): Result<Long?> { val id = nextProgramId++; programs[id] = ContentValues(values); return Result.success(id) }
        override fun updateProgram(programId: Long, values: ContentValues): Result<Int> { programs[programId]?.putAll(values); return Result.success(1) }
    }
}

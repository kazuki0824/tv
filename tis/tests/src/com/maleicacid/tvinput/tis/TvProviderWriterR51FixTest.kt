package com.maleicacid.tvinput.tis

import android.content.ContentValues
import android.media.tv.TvContract
import com.maleicacid.tvinput.aribsi.AribParentalRating
import com.maleicacid.tvinput.aribsi.AribRatingMapper
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.db.ChannelRecord
import com.maleicacid.tvinput.db.ProgramRecord
import org.junit.Test

class TvProviderWriterR51FixTest {
    private val key = ServiceKey(4, 16625, 101)

    @Test fun optionalProgramColumnsAreClearedByMergeUpdate() {
        val store = MergeStore()
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        writer.upsertChannels(listOf(ChannelRecord(key, "101", "NHK", 473_142_857L)))
        val rating15 = requireNotNull(AribRatingMapper.toTvContentRatingString(AribParentalRating("JPN", 15, 15, true)))
        val p = ProgramRecord(
            key, 1, "p1", 1_700_000_000_000L, 1_800_000L, "title", "desc",
            audioLanguage = "jpn", canonicalGenres = listOf("NEWS"), broadcastGenre = "ARIB(0x0/0x0):ニュース/報道/定時・総合", contentRatings = listOf(rating15), scrambled = false, seriesId = 100, episodeNumber = 3, lastEpisodeNumber = 12,
        )
        writer.upsertPrograms(listOf(p))
        writer.upsertPrograms(listOf(p.copy(audioLanguage = null, canonicalGenres = emptyList(), broadcastGenre = null, contentRatings = emptyList(), scrambled = null, seriesId = null, episodeNumber = null, lastEpisodeNumber = null)))
        val values = store.programs.values.single()
        check(values.get(TvContract.Programs.COLUMN_AUDIO_LANGUAGE) == null)
        check(values.get(TvContract.Programs.COLUMN_BROADCAST_GENRE) == null)
        check(values.get(TvContract.Programs.COLUMN_CANONICAL_GENRE) == null)
        check(values.get(TvContract.Programs.COLUMN_CONTENT_RATING) == null)
        check(values.get(TvProviderWriter.COLUMN_SCRAMBLED) == null)
        check(values.get(TvProviderWriter.COLUMN_SERIES_ID) == null)
        check(values.get(TvProviderWriter.COLUMN_EPISODE_DISPLAY_NUMBER) == null)
        check(values.get(TvProviderWriter.COLUMN_ITEM_COUNT) == null)
    }

    private class MergeStore : TvProviderWriter.ChannelStore {
        private var nextChannelId = 1L
        private var nextProgramId = 100L
        val channels = LinkedHashMap<Long, ContentValues>()
        val programs = LinkedHashMap<Long, ContentValues>()
        override fun findExistingChannelId(key: ServiceKey): Result<Long?> = Result.success(channels.keys.firstOrNull())
        override fun insertChannel(values: ContentValues): Result<Long?> { val id = nextChannelId++; channels[id] = ContentValues(values); return Result.success(id) }
        override fun updateChannel(channelId: Long, values: ContentValues): Result<Int> { channels[channelId]?.putAll(values); return Result.success(1) }
        override fun findExistingProgramId(channelId: Long, programKey: String): Result<Long?> = Result.success(programs.entries.firstOrNull { (_, v) -> TvProviderWriter.parseProgramKey(v.getAsByteArray(TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA).toString(Charsets.UTF_8)) == programKey }?.key)
        override fun indexExistingProgramsForWindow(channelId: Long, windowStartMs: Long, windowEndMs: Long): Result<Map<String, Long>> = Result.success(
            programs.entries.mapNotNull { (id, v) ->
                val key = TvProviderWriter.parseProgramKey(v.getAsByteArray(TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA).toString(Charsets.UTF_8)) ?: return@mapNotNull null
                key to id
            }.toMap(),
        )
        override fun insertProgram(values: ContentValues): Result<Long?> { val id = nextProgramId++; programs[id] = ContentValues(values); return Result.success(id) }
        override fun updateProgram(programId: Long, values: ContentValues): Result<Int> { programs[programId]?.putAll(values); return Result.success(1) }
    }
}

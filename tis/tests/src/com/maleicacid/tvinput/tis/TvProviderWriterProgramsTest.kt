package com.maleicacid.tvinput.tis

import android.content.ContentValues
import android.media.tv.TvContract
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.db.ChannelRecord
import com.maleicacid.tvinput.db.ProgramRecord
import org.junit.Test

class TvProviderWriterProgramsTest {
    private val key = ServiceKey(4, 16625, 101)

    @Test fun insertAndUpdateProgram() {
        val store = FakeStore()
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        writer.upsertChannels(listOf(ChannelRecord(key, "101", "NHK", 473_142_857L)))
        val p = ProgramRecord(key, 10, "onid=4;tsid=16625;sid=101;event=10", 1_700_000_000_000L, 1_800_000L, "News", "desc")
        val first = writer.upsertPrograms(listOf(p))
        check(first.inserted == 1)
        val second = writer.upsertPrograms(listOf(p.copy(description = "updated", shortDescription = "updated")))
        check(second.updated == 1)
        check(store.programs.values.single().getAsString(TvContract.Programs.COLUMN_SHORT_DESCRIPTION) == "updated")
        check(store.programs.values.single().getAsString(TvContract.Programs.COLUMN_LONG_DESCRIPTION) == "updated")
        check(store.programs.values.single().getAsInteger(TvContract.Programs.COLUMN_EVENT_ID) == 10)
        val providerData = store.programs.values.single().getAsByteArray(TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA).toString(Charsets.UTF_8)
        check(TvProviderWriter.parseProgramKey(providerData) == p.stableIdentity)
        check(providerData.contains(TvProviderWriter.PROGRAM_KEY_FIELD))
    }

    @Test fun descriptorDetailsStayInInternalProviderData() {
        val store = FakeStore()
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        writer.upsertChannels(listOf(ChannelRecord(key, "101", "NHK", 473_142_857L)))
        val p = ProgramRecord(
            key, 11, "onid=4;tsid=16625;sid=101;event=11", 1_700_000_000_000L, 1_800_000L,
            "News", "desc", extendedItemsJson = "[{\"description\":\"出演\",\"text\":\"A\"}]",
            componentText = "映像", audioComponentText = "音声", audioLanguage = "jpn", canonicalGenre = "NEWS", genreSupplementText = "ニュース/報道(0/0)", eventGroupText = "sid=101 event=202", freeCaText = "無料放送", seriesName = "シリーズ", diagnosticText = "unknownCount=0", diagnosticDescriptorJson = "{}",
        )
        writer.upsertPrograms(listOf(p))
        val providerData = store.programs.values.single().getAsByteArray(TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA).toString(Charsets.UTF_8)
        check(providerData.contains("extendedItemsB64"))
        check(providerData.contains("componentTextB64"))
        check(providerData.contains("audioComponentTextB64"))
        check(providerData.contains("audioLanguageB64"))
        check(providerData.contains("canonicalGenreB64"))
        check(providerData.contains("genreSupplementTextB64"))
        check(providerData.contains("eventGroupTextB64"))
        check(providerData.contains("freeCaTextB64"))
        check(providerData.contains("seriesNameB64"))
        check(store.programs.values.single().getAsString(TvContract.Programs.COLUMN_AUDIO_LANGUAGE) == "jpn")
        check(store.programs.values.single().getAsString(TvContract.Programs.COLUMN_CANONICAL_GENRE) == "NEWS")
        check(store.programs.values.single().getAsString(TvContract.Programs.COLUMN_SHORT_DESCRIPTION) == "desc")
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
        override fun insertProgram(values: ContentValues): Result<Long?> { val id = nextProgramId++; programs[id] = ContentValues(values); return Result.success(id) }
        override fun updateProgram(programId: Long, values: ContentValues): Result<Int> { programs[programId] = ContentValues(values); return Result.success(1) }
    }
}

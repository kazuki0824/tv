package com.maleicacid.tvinput.tis

import android.content.ContentValues
import android.media.tv.TvContract
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.db.ChannelRecord
import org.junit.Test

/** AndroidJUnitRunner から実行する TvProviderWriter チャンネル更新テスト。 */
class TvProviderWriterUpsertTest {
    private val key = ServiceKey(originalNetworkId = 4, transportStreamId = 16625, serviceId = 101)

    @Test fun insertNewChannel() {
        val store = FakeChannelStore()
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        val result = writer.upsertChannels(listOf(ChannelRecord(key, displayNumber = "101", displayName = "NHK", frequencyHz = 473_142_857L)))
        check(result.inserted == 1) { result.toString() }
        check(result.updated == 0)
        check(result.failures.isEmpty())
        check(store.rows.size == 1)
        check(store.rows.values.single().getAsInteger(TvContract.Channels.COLUMN_BROWSABLE) == 1)
        check(store.rows.values.single().getAsInteger(TvContract.Channels.COLUMN_SEARCHABLE) == 1)
    }

    @Test fun updateExistingChannel() {
        val store = FakeChannelStore()
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        writer.upsertChannels(listOf(ChannelRecord(key, displayNumber = "101", displayName = "NHK", frequencyHz = 473_142_857L)))
        val result = writer.upsertChannels(listOf(ChannelRecord(key, displayNumber = "101", displayName = "NHK G", frequencyHz = 473_142_857L)))
        check(result.inserted == 0) { result.toString() }
        check(result.updated == 1)
        check(store.rows.size == 1)
        check(store.rows.values.single().getAsString(TvContract.Channels.COLUMN_DISPLAY_NAME) == "NHK G")
    }

    @Test fun rejectInvalidMetadata() {
        val writer = TvProviderWriter("input.test", FakeChannelStore(), testOnly = true)
        val invalid = ChannelRecord(ServiceKey(originalNetworkId = -1, transportStreamId = 16625, serviceId = 101), displayNumber = "101", displayName = "bad", frequencyHz = 473_142_857L)
        val result = writer.upsertChannels(listOf(invalid))
        check(result.inserted == 0)
        check(result.updated == 0)
        check(result.failures.single().operation == "validate")
    }

    @Test fun providerFailureIsDiagnostic() {
        val store = FakeChannelStore(failInsert = true)
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        val result = writer.upsertChannels(listOf(ChannelRecord(key, displayNumber = "101", displayName = "NHK", frequencyHz = 473_142_857L)))
        check(result.inserted == 0)
        check(result.failures.single().operation == "insert")
    }

    private class FakeChannelStore(private val failInsert: Boolean = false) : TvProviderWriter.ChannelStore {
        private var nextId = 1L
        val rows = LinkedHashMap<Long, ContentValues>()

        override fun findExistingChannelId(key: ServiceKey): Result<Long?> = Result.success(
            rows.entries.firstOrNull { (_, values) ->
                values.getAsInteger(TvContract.Channels.COLUMN_ORIGINAL_NETWORK_ID) == key.originalNetworkId &&
                    values.getAsInteger(TvContract.Channels.COLUMN_TRANSPORT_STREAM_ID) == key.transportStreamId &&
                    values.getAsInteger(TvContract.Channels.COLUMN_SERVICE_ID) == key.serviceId
            }?.key,
        )

        override fun insertChannel(values: ContentValues): Result<Long?> {
            if (failInsert) return Result.failure(IllegalStateException("insert failed"))
            val id = nextId++
            rows[id] = ContentValues(values)
            return Result.success(id)
        }

        override fun updateChannel(channelId: Long, values: ContentValues): Result<Int> {
            if (!rows.containsKey(channelId)) return Result.success(0)
            rows[channelId] = ContentValues(values)
            return Result.success(1)
        }
    }
}

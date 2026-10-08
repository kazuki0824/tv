// テストの入力・期待値を本体の定数と独立した具体値で記述する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import android.content.ContentValues
import android.media.tv.TvContract
import com.maleicacid.tvinput.common.FrequencyHz
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.db.ChannelRecord
import org.junit.Test

/** AndroidJUnitRunner から実行する TvProviderWriter チャンネル更新テスト。 */
class TvProviderWriterUpsertTest {
    private val key = ServiceKey(originalNetworkId = 4, transportStreamId = 16625, serviceId = 101)

    @Test fun insertNewChannel() {
        val store = FakeChannelStore()
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        val result =
            writer.upsertChannels(
                listOf(
                    ChannelRecord(
                        key,
                        serviceType = 0x01,
                        displayNumber = "101",
                        displayName = "NHK",
                        frequencyHz = FrequencyHz(473_142_857L),
                        casFactsCanonicalJson =
                            com.maleicacid.tvinput.tis
                                .testCasFacts(false),
                    ),
                ),
            )
        check(result.inserted == 1) { result.toString() }
        check(result.updated == 0)
        check(result.failures.isEmpty())
        check(store.rows.size == 1)
        check(
            store.rows.values
                .single()
                .get(TvContract.Channels.COLUMN_BROWSABLE) == null,
        )
        check(
            store.rows.values
                .single()
                .getAsInteger(TvContract.Channels.COLUMN_SEARCHABLE) == 1,
        )
    }

    @Test fun updateExistingChannel() {
        val store = FakeChannelStore()
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        writer.upsertChannels(
            listOf(
                ChannelRecord(
                    key,
                    serviceType = 0x01,
                    displayNumber = "101",
                    displayName = "NHK",
                    frequencyHz = FrequencyHz(473_142_857L),
                    casFactsCanonicalJson =
                        com.maleicacid.tvinput.tis
                            .testCasFacts(false),
                ),
            ),
        )
        val result =
            writer.upsertChannels(
                listOf(
                    ChannelRecord(
                        key,
                        serviceType = 0x01,
                        displayNumber = "101",
                        displayName = "NHK G",
                        frequencyHz = FrequencyHz(473_142_857L),
                        casFactsCanonicalJson =
                            com.maleicacid.tvinput.tis
                                .testCasFacts(false),
                    ),
                ),
            )
        check(result.inserted == 0) { result.toString() }
        check(result.updated == 1)
        check(store.rows.size == 1)
        check(
            store.rows.values
                .single()
                .getAsString(TvContract.Channels.COLUMN_DISPLAY_NAME) == "NHK G",
        )
    }

    @Test fun setupCompletionMarksOnlyNewNonOneSegBrowsable() {
        val store = FakeChannelStore()
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        val oneSegKey = ServiceKey(originalNetworkId = 4, transportStreamId = 16625, serviceId = 102)
        val result =
            writer.upsertChannels(
                listOf(
                    ChannelRecord(
                        key,
                        serviceType = 0x01,
                        displayNumber = "101",
                        displayName = "NHK",
                        frequencyHz = FrequencyHz(473_142_857L),
                        casFactsCanonicalJson = testCasFacts(false),
                    ),
                    ChannelRecord(
                        oneSegKey,
                        serviceType = 0xc0,
                        displayNumber = "102",
                        displayName = "1seg",
                        frequencyHz = FrequencyHz(473_142_857L),
                        casFactsCanonicalJson = testCasFacts(false),
                        partialReception = true,
                    ),
                ),
            )
        val normalId = requireNotNull(result.insertedChannelIds[key])
        val oneSegId = requireNotNull(result.insertedChannelIds[oneSegKey])
        check(
            writer
                .finalizeSetupChannels(
                    completed = true,
                    insertedChannelIds = setOf(normalId, oneSegId),
                    initialBrowsablePendingChannelIds = result.initialBrowsablePendingChannelIds.values.toSet(),
                ).isSuccess,
        )
        check(store.rows.getValue(normalId).getAsInteger(TvContract.Channels.COLUMN_BROWSABLE) == 1)
        check(store.rows.getValue(oneSegId).get(TvContract.Channels.COLUMN_BROWSABLE) == null)
    }

    @Test fun rescanPreservesExistingHiddenChannel() {
        val store = FakeChannelStore()
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        val first =
            writer.upsertChannels(
                listOf(
                    ChannelRecord(
                        key,
                        serviceType = 0x01,
                        displayNumber = "101",
                        displayName = "NHK",
                        frequencyHz = FrequencyHz(473_142_857L),
                        casFactsCanonicalJson = testCasFacts(false),
                    ),
                ),
            )
        val channelId = requireNotNull(first.insertedChannelIds[key])
        check(
            writer
                .finalizeSetupChannels(
                    completed = true,
                    insertedChannelIds = setOf(channelId),
                    initialBrowsablePendingChannelIds = first.initialBrowsablePendingChannelIds.values.toSet(),
                ).isSuccess,
        )
        store.rows.getValue(channelId).put(TvContract.Channels.COLUMN_BROWSABLE, 0)

        val rescan =
            writer.upsertChannels(
                listOf(
                    ChannelRecord(
                        key,
                        serviceType = 0x01,
                        displayNumber = "101",
                        displayName = "NHK G",
                        frequencyHz = FrequencyHz(473_142_857L),
                        casFactsCanonicalJson = testCasFacts(false),
                    ),
                ),
            )
        check(rescan.insertedChannelIds.isEmpty())
        check(
            writer
                .finalizeSetupChannels(
                    completed = true,
                    insertedChannelIds = emptySet(),
                    initialBrowsablePendingChannelIds = rescan.initialBrowsablePendingChannelIds.values.toSet(),
                ).isSuccess,
        )
        check(store.rows.getValue(channelId).getAsInteger(TvContract.Channels.COLUMN_BROWSABLE) == 0)
        check(store.rows.getValue(channelId).getAsString(TvContract.Channels.COLUMN_DISPLAY_NAME) == "NHK G")
    }

    @Test fun browsableCommitFailureIsReportedWithoutExposingChannel() {
        val store = FakeChannelStore(failBrowsable = true)
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        val result =
            writer.upsertChannels(
                listOf(
                    ChannelRecord(
                        key,
                        serviceType = 0x01,
                        displayNumber = "101",
                        displayName = "NHK",
                        frequencyHz = FrequencyHz(473_142_857L),
                        casFactsCanonicalJson = testCasFacts(false),
                    ),
                ),
            )
        val insertedId = requireNotNull(result.insertedChannelIds[key])
        check(
            writer
                .finalizeSetupChannels(
                    completed = true,
                    insertedChannelIds = setOf(insertedId),
                    initialBrowsablePendingChannelIds = result.initialBrowsablePendingChannelIds.values.toSet(),
                ).isFailure,
        )
        check(!store.rows.containsKey(insertedId))
    }

    @Test fun unfinishedSetupPendingChannelIsRecoveredByNextSuccessfulScan() {
        val store = FakeChannelStore()
        val firstWriter = TvProviderWriter("input.test", store, testOnly = true)
        val first =
            firstWriter.upsertChannels(
                listOf(
                    ChannelRecord(
                        key,
                        serviceType = 0x01,
                        displayNumber = "101",
                        displayName = "NHK",
                        frequencyHz = FrequencyHz(473_142_857L),
                        casFactsCanonicalJson = testCasFacts(false),
                    ),
                ),
            )
        val channelId = requireNotNull(first.insertedChannelIds[key])
        check(first.initialBrowsablePendingChannelIds[key] == channelId)
        check(store.rows.getValue(channelId).get(TvContract.Channels.COLUMN_BROWSABLE) == null)

        val nextWriter = TvProviderWriter("input.test", store, testOnly = true)
        val next =
            nextWriter.upsertChannels(
                listOf(
                    ChannelRecord(
                        key,
                        serviceType = 0x01,
                        displayNumber = "101",
                        displayName = "NHK",
                        frequencyHz = FrequencyHz(473_142_857L),
                        casFactsCanonicalJson = testCasFacts(false),
                    ),
                ),
            )
        check(next.insertedChannelIds.isEmpty())
        check(next.initialBrowsablePendingChannelIds[key] == channelId)
        check(
            nextWriter
                .finalizeSetupChannels(
                    completed = true,
                    insertedChannelIds = emptySet(),
                    initialBrowsablePendingChannelIds = next.initialBrowsablePendingChannelIds.values.toSet(),
                ).isSuccess,
        )
        check(store.rows.getValue(channelId).getAsInteger(TvContract.Channels.COLUMN_BROWSABLE) == 1)
        check(store.rows.getValue(channelId).getAsLong(TvContract.Channels.COLUMN_INTERNAL_PROVIDER_FLAG1) == 0L)
    }

    @Test fun invalidServiceKeyIsRejectedBeforeChannelRecordConstruction() {
        check(ServiceKey.fromOrNull(originalNetworkId = -1, transportStreamId = 16625, serviceId = 101) == null)
    }

    @Test fun providerFailureIsDiagnostic() {
        val store = FakeChannelStore(failInsert = true)
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        val result =
            writer.upsertChannels(
                listOf(
                    ChannelRecord(
                        key,
                        serviceType = 0x01,
                        displayNumber = "101",
                        displayName = "NHK",
                        frequencyHz = FrequencyHz(473_142_857L),
                        casFactsCanonicalJson =
                            com.maleicacid.tvinput.tis
                                .testCasFacts(false),
                    ),
                ),
            )
        check(result.inserted == 0)
        check(result.failures.single().operation == "insert")
    }

    @Test
    fun upgradeNormalChannelToOneSegRecreatesImmutableTypeAndClearsPending() {
        val store = FakeChannelStore()
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        val normal =
            ChannelRecord(
                key,
                0x01,
                "101",
                "NHK",
                FrequencyHz(473_142_857L),
                casFactsCanonicalJson = testCasFacts(false),
            )
        val first = writer.upsertChannels(listOf(normal))
        val oldId = first.insertedChannelIds.getValue(key)
        store.rows.getValue(oldId).put(TvContract.Channels.COLUMN_BROWSABLE, 1)
        val upgraded = writer.upsertChannels(listOf(normal.copy(serviceType = 0xc0, partialReception = true)))
        check(upgraded.updated == 1 && upgraded.failures.isEmpty())
        check(upgraded.insertedChannelIds.isEmpty() && upgraded.initialBrowsablePendingChannelIds.isEmpty())
        check(oldId !in store.rows)
        val replacement = store.rows.values.single()
        check(replacement.getAsString(TvContract.Channels.COLUMN_TYPE) == TvContract.Channels.TYPE_1SEG)
        check(replacement.getAsLong(TvContract.Channels.COLUMN_INTERNAL_PROVIDER_FLAG1) == 0L)
        check(replacement.getAsInteger(TvContract.Channels.COLUMN_BROWSABLE) == 0)
    }

    @Test
    fun failedTypeRecreationPreservesOldChannel() {
        val store = FakeChannelStore(failRecreate = true)
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        val normal =
            ChannelRecord(
                key,
                0x01,
                "101",
                "NHK",
                FrequencyHz(473_142_857L),
                casFactsCanonicalJson = testCasFacts(false),
            )
        val first = writer.upsertChannels(listOf(normal))
        val oldId = first.insertedChannelIds.getValue(key)
        val before = ContentValues(store.rows.getValue(oldId))
        val result = writer.upsertChannels(listOf(normal.copy(serviceType = 0xc0, partialReception = true)))
        check(result.updated == 0 && result.failures.single().operation == "update")
        check(store.rows.getValue(oldId) == before)
        check(store.rows.size == 1)
    }

    private class FakeChannelStore(
        private val failInsert: Boolean = false,
        private val failBrowsable: Boolean = false,
        private val failRecreate: Boolean = false,
    ) : TvProviderWriter.ChannelStore {
        private var nextId = 1L
        val rows = LinkedHashMap<Long, ContentValues>()

        // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
        @Suppress("MaxLineLength")
        override fun indexExistingChannelIds(keys: Set<ServiceKey>): Result<Map<ServiceKey, Long>> =
            Result.success(
                buildMap {
                    rows.forEach { (id, values) ->
                        val key =
                            ServiceKey(
                                values.getAsInteger(TvContract.Channels.COLUMN_ORIGINAL_NETWORK_ID),
                                values.getAsInteger(TvContract.Channels.COLUMN_TRANSPORT_STREAM_ID),
                                values.getAsInteger(TvContract.Channels.COLUMN_SERVICE_ID),
                            )
                        if (key in keys && !containsKey(key)) put(key, id)
                    }
                },
            )

        override fun insertChannel(values: ContentValues): Result<Long?> {
            if (failInsert) return Result.failure(IllegalStateException("挿入失敗"))
            val id = nextId++
            rows[id] = ContentValues(values)
            return Result.success(id)
        }

        override fun updateChannel(
            channelId: Long,
            values: ContentValues,
        ): Result<Int> {
            val existing = rows[channelId] ?: return Result.success(0)
            existing.putAll(values)
            return Result.success(1)
        }

        override fun upsertExistingChannel(
            channelId: Long,
            values: ContentValues,
        ): Result<TvProviderWriter.ExistingChannelUpsertOutcome> {
            val old = rows.getValue(channelId)
            val oldType = old.getAsString(TvContract.Channels.COLUMN_TYPE)
            val nextType = values.getAsString(TvContract.Channels.COLUMN_TYPE)
            return if (oldType == nextType) {
                super<TvProviderWriter.ChannelStore>.upsertExistingChannel(channelId, values)
            } else if (failRecreate) {
                Result.failure(IllegalStateException("再作成失敗"))
            } else {
                val id = nextId++
                rows[id] = ContentValues(values).apply { put(TvContract.Channels.COLUMN_BROWSABLE, 0) }
                rows.remove(channelId)
                Result.success(TvProviderWriter.ExistingChannelUpsertOutcome(id, recreated = true))
            }
        }

        override fun indexInitialBrowsablePendingChannelIds(keys: Set<ServiceKey>): Result<Map<ServiceKey, Long>> =
            Result.success(
                rows.entries
                    .mapNotNull { (id, values) ->
                        val rowKey =
                            ServiceKey(
                                values.getAsInteger(TvContract.Channels.COLUMN_ORIGINAL_NETWORK_ID),
                                values.getAsInteger(TvContract.Channels.COLUMN_TRANSPORT_STREAM_ID),
                                values.getAsInteger(TvContract.Channels.COLUMN_SERVICE_ID),
                            )
                        if (
                            rowKey in keys &&
                            values.getAsLong(TvContract.Channels.COLUMN_INTERNAL_PROVIDER_FLAG1) == 1L
                        ) {
                            rowKey to id
                        } else {
                            null
                        }
                    }.toMap(),
            )

        override fun commitInitialBrowsable(channelIds: Set<Long>): Result<Int> {
            if (failBrowsable) return Result.failure(IllegalStateException("browsable更新失敗"))
            var updated = 0
            channelIds.forEach { channelId ->
                rows[channelId]?.let { values ->
                    values.put(TvContract.Channels.COLUMN_BROWSABLE, 1)
                    values.put(TvContract.Channels.COLUMN_INTERNAL_PROVIDER_FLAG1, 0L)
                    updated++
                }
            }
            return Result.success(updated)
        }

        override fun deleteChannels(channelIds: Set<Long>): Result<Int> {
            var deleted = 0
            channelIds.forEach { channelId ->
                if (rows.remove(channelId) != null) deleted++
            }
            return Result.success(deleted)
        }
    }
}

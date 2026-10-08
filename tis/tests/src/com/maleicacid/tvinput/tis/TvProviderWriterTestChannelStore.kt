package com.maleicacid.tvinput.tis

import android.content.ContentValues
import android.media.tv.TvContract

/** test storeのrow操作。型不一致は新row作成後の旧row削除で表し、mutable updateと区別する。 */
internal fun testUpsertExistingChannel(
    rows: MutableMap<Long, ContentValues>,
    channelId: Long,
    values: ContentValues,
    insert: (ContentValues) -> Result<Long?>,
): Result<TvProviderWriter.ExistingChannelUpsertOutcome> =
    runCatching {
        val current = rows.getValue(channelId)
        if (current.getAsString(TvContract.Channels.COLUMN_TYPE) == values.getAsString(TvContract.Channels.COLUMN_TYPE)) {
            val update = ContentValues(values)
            update.remove(TvContract.Channels.COLUMN_TYPE)
            update.remove(TvContract.Channels.COLUMN_INTERNAL_PROVIDER_FLAG1)
            current.putAll(update)
            TvProviderWriter.ExistingChannelUpsertOutcome(channelId, recreated = false)
        } else {
            val replacement = ContentValues(values).apply { put(TvContract.Channels.COLUMN_BROWSABLE, 0) }
            val id = checkNotNull(insert(replacement).getOrThrow()) { "test store再作成rowのIDがありません" }
            rows.remove(channelId)
            TvProviderWriter.ExistingChannelUpsertOutcome(id, recreated = true)
        }
    }

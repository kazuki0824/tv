// test fixtureのbatch境界を独立した具体値で照合する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import android.content.ContentValues

/** fixtureも64操作単位のbatchをstageし、当該batchの成功後にだけrowをcommitする。 */
internal fun testUpsertProgramsBatch(
    rows: MutableMap<Long, ContentValues>,
    requests: List<TvProviderWriter.ProgramUpsertRequest>,
    allocateId: () -> Long,
): Result<List<TvProviderWriter.ProgramUpsertOutcome>> =
    runCatching {
        val outcomes = mutableListOf<TvProviderWriter.ProgramUpsertOutcome>()
        for (batch in requests.chunked(64)) {
            val staged = rows.mapValues { ContentValues(it.value) }.toMutableMap()
            val batchOutcomes =
                batch.map { request ->
                    val existingId = request.existingProgramId
                    if (existingId == null) {
                        val id = allocateId()
                        staged[id] = ContentValues(request.values)
                        TvProviderWriter.ProgramUpsertOutcome(id, updated = false)
                    } else {
                        val current = staged[existingId]
                        current?.putAll(request.values)
                        val affected = current != null
                        TvProviderWriter.ProgramUpsertOutcome(existingId.takeIf { affected }, updated = affected)
                    }
                }
            rows.clear()
            rows.putAll(staged)
            outcomes += batchOutcomes
        }
        outcomes
    }

internal fun testDeleteObsoletePrograms(
    rows: MutableMap<Long, ContentValues>,
    channelId: Long,
    validKeys: Set<String>,
    startMs: Long,
    endMs: Long,
): Result<Int> =
    runCatching {
        val obsolete =
            rows
                .filterValues { values ->
                    val start = values.getAsLong(android.media.tv.TvContract.Programs.COLUMN_START_TIME_UTC_MILLIS)
                    val end = values.getAsLong(android.media.tv.TvContract.Programs.COLUMN_END_TIME_UTC_MILLIS)
                    val key =
                        TvProviderWriter.parseProgramKey(
                            values.getAsByteArray(android.media.tv.TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA),
                        )
                    values.getAsLong(android.media.tv.TvContract.Programs.COLUMN_CHANNEL_ID) == channelId &&
                        end > startMs && start < endMs && key !in validKeys
                }.keys
                .toList()
        obsolete.forEach(rows::remove)
        obsolete.size
    }

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
                        TvProviderWriter.ProgramUpsertOutcome(existingId.takeIf { current != null }, updated = current != null)
                    }
                }
            rows.clear()
            rows.putAll(staged)
            outcomes += batchOutcomes
        }
        outcomes
    }

// 試験入力・環境budgetは本体の定数と独立した値で記述する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import android.content.ContentProvider
import android.content.ContentProviderOperation
import android.content.ContentProviderResult
import android.content.ContentUris
import android.content.ContentValues
import android.content.pm.ProviderInfo
import android.database.Cursor
import android.media.tv.TvContract
import android.net.Uri
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.shadows.ShadowContentResolver

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class ProgramBatchIpcBudgetTest {
    @Test
    fun parcelBudgetsSplitDifferentlyAndKeepOrderAndCountLimit() {
        val attribution = RuntimeEnvironment.getApplication().attributionSource
        val operations = List(12) { index -> operation(index, 4_000) }
        val small = TvProviderWriter.programOperationBatches(operations, attribution, 16_384)
        val large = TvProviderWriter.programOperationBatches(operations, attribution, 32_768)
        check(small.size > large.size)
        check(small.flatten() == operations && large.flatten() == operations)
        val countLimited =
            TvProviderWriter.programOperationBatches(
                List(65) { operation(it, 1) },
                attribution,
                1_000_000,
            )
        check(countLimited.map { it.size } == listOf(64, 1))
    }

    @Test
    fun androidStoreSplitsAcceptedRowsAndRejectsOversizedSingleOperationBeforeWriting() {
        val provider = BatchProvider()
        val context = RuntimeEnvironment.getApplication()
        provider.attachInfo(context, ProviderInfo().apply { authority = TvContract.AUTHORITY })
        ShadowContentResolver.registerProviderInternal(TvContract.AUTHORITY, provider)
        val writer = TvProviderWriter(context, "input.test")
        val store =
            TvProviderWriter::class.java
                .getDeclaredField("channelStore")
                .apply { isAccessible = true }
                .get(writer) as TvProviderWriter.ChannelStore
        val requests = List(64) { index -> TvProviderWriter.ProgramUpsertRequest(null, values(index, 24_000)) }
        val outcomes = store.upsertProgramsBatch(requests).getOrThrow()
        check(outcomes.size == 64 && outcomes.map { it.programId } == (1L..64L).toList())
        check(provider.batches.size > 1 && provider.batches.sumOf { it.size } == 64)
        // Rustの正常上限のopaque bytesと標準列を同じ本番store入口へ渡す。
        val boundary = values(65, 32_768).apply {
            put(TvContract.Programs.COLUMN_START_TIME_UTC_MILLIS, 1_700_000_000_000L)
            put(TvContract.Programs.COLUMN_END_TIME_UTC_MILLIS, 1_700_000_060_000L)
            put(TvContract.Programs.COLUMN_SHORT_DESCRIPTION, "番組の説明")
            put(TvContract.Programs.COLUMN_LONG_DESCRIPTION, "追加の番組説明")
            put(TvContract.Programs.COLUMN_VIDEO_WIDTH, 1_920)
            put(TvContract.Programs.COLUMN_VIDEO_HEIGHT, 1_080)
            put(TvContract.Programs.COLUMN_AUDIO_LANGUAGE, "ja")
            put(TvContract.Programs.COLUMN_CANONICAL_GENRE, "NEWS")
        }
        val boundaryResult = store.upsertProgramsBatch(listOf(TvProviderWriter.ProgramUpsertRequest(null, boundary)))
        check(boundaryResult.getOrThrow().single().programId == 65L)
        check(provider.batches.last().size == 1)
        val before = provider.batches.size
        val rejected =
            store.upsertProgramsBatch(
                listOf(requests.first(), TvProviderWriter.ProgramUpsertRequest(null, values(65, 100_000))),
            )
        check(
            rejected.isFailure &&
                rejected
                    .exceptionOrNull()
                    ?.message
                    .orEmpty()
                    .contains("単一Program operation"),
        )
        check(provider.batches.size == before)
    }

    private fun operation(
        index: Int,
        bytes: Int,
    ): ContentProviderOperation =
        ContentProviderOperation.newInsert(TvContract.Programs.CONTENT_URI).withValues(values(index, bytes)).build()

    private fun values(
        index: Int,
        bytes: Int,
    ) = ContentValues().apply {
        put(TvContract.Programs.COLUMN_TITLE, "番組$index")
        put(TvContract.Programs.COLUMN_CHANNEL_ID, 1L)
        put(TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA, ByteArray(bytes))
    }

    private class BatchProvider : ContentProvider() {
        val batches = mutableListOf<List<ContentProviderOperation>>()
        private var nextId = 1L

        override fun applyBatch(operations: ArrayList<ContentProviderOperation>): Array<ContentProviderResult> {
            batches += operations.toList()
            return Array(operations.size) {
                ContentProviderResult(ContentUris.withAppendedId(TvContract.Programs.CONTENT_URI, nextId++))
            }
        }

        override fun onCreate() = true

        override fun query(
            uri: Uri,
            projection: Array<out String>?,
            selection: String?,
            selectionArgs: Array<out String>?,
            sortOrder: String?,
        ): Cursor? = null

        override fun getType(uri: Uri): String? = null

        override fun insert(
            uri: Uri,
            values: ContentValues?,
        ): Uri? = null

        override fun update(
            uri: Uri,
            values: ContentValues?,
            selection: String?,
            selectionArgs: Array<out String>?,
        ) = 0

        override fun delete(
            uri: Uri,
            selection: String?,
            selectionArgs: Array<out String>?,
        ) = 0
    }
}

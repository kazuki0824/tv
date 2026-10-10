// 試験の行IDと可視性期待値を具体値で照合する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import android.content.ContentProvider
import android.content.ContentProviderOperation
import android.content.ContentProviderResult
import android.content.ContentUris
import android.content.ContentValues
import android.content.pm.ProviderInfo
import android.database.Cursor
import android.database.sqlite.SQLiteDatabase
import android.media.tv.TvContract
import android.net.Uri
import org.junit.After
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.shadows.ShadowContentResolver

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class TvProviderVisibilityTransactionTest {
    private lateinit var provider: VisibilityProvider
    private lateinit var writer: TvProviderWriter

    @Before
    fun setUp() {
        val context = RuntimeEnvironment.getApplication()
        provider = VisibilityProvider()
        provider.attachInfo(context, ProviderInfo().apply { authority = TvContract.AUTHORITY })
        ShadowContentResolver.registerProviderInternal(TvContract.AUTHORITY, provider)
        writer = TvProviderWriter(context, "visibility-test")
        for (id in 1L..3L) {
            val values =
                ContentValues().apply {
                    put(TvContract.Channels._ID, id)
                    put(TvContract.Channels.COLUMN_BROWSABLE, 0)
                    put(TvContract.Channels.COLUMN_INTERNAL_PROVIDER_FLAG1, 1L)
                }
            context.contentResolver.insert(TvContract.Channels.CONTENT_URI, values)
        }
    }

    @After
    fun tearDown() {
        provider.database.close()
    }

    @Test
    fun missingLastRowRollsBackExistingAndNewPendingUpdates() {
        assertMissingRowRollsBack(missingId = 3L, insertedId = 2L)
        check(provider.updatedIds == listOf(1L, 2L, 3L))
    }

    @Test
    fun missingMiddleRowRollsBackExistingPendingUpdate() {
        assertMissingRowRollsBack(missingId = 2L, insertedId = 3L)
        check(provider.updatedIds == listOf(1L, 2L))
    }

    @Test
    fun successfulBatchMakesAllPendingRowsBrowsable() {
        check(writer.finalizeSetupChannels(true, setOf(2L), setOf(1L, 2L, 3L)).isSuccess)
        for (id in 1L..3L) check(rowState(id) == (1 to 0L))
    }

    private fun assertMissingRowRollsBack(
        missingId: Long,
        insertedId: Long,
    ) {
        // pending照会時には全行が存在し、その後に既存行だけが削除される競合。
        for (id in 1L..3L) check(rowState(id) == (0 to 1L))
        RuntimeEnvironment.getApplication().contentResolver.delete(TvContract.buildChannelUri(missingId), null, null)
        val result = writer.finalizeSetupChannels(true, setOf(insertedId), setOf(1L, 2L, 3L))
        check(result.isFailure)
        check(result.exceptionOrNull() is android.content.OperationApplicationException)
        check(rowState(1L) == (0 to 1L))
        check(rowState(missingId) == null)
        check(rowState(insertedId) == null)
        check(provider.committedBatches == 1)
    }

    private fun rowState(id: Long): Pair<Int, Long>? {
        val cursor =
            checkNotNull(
                RuntimeEnvironment.getApplication().contentResolver.query(
                    TvContract.buildChannelUri(id),
                    arrayOf(TvContract.Channels.COLUMN_BROWSABLE, TvContract.Channels.COLUMN_INTERNAL_PROVIDER_FLAG1),
                    null,
                    null,
                    null,
                ),
            )
        return cursor.use { if (it.moveToFirst()) it.getInt(0) to it.getLong(1) else null }
    }

    // AOSP TvProviderと同じSQLite transaction境界で、実ContentProviderOperationを実行する。
    class VisibilityProvider : ContentProvider() {
        lateinit var database: SQLiteDatabase
        val updatedIds = mutableListOf<Long>()
        var committedBatches = 0

        override fun onCreate(): Boolean {
            database = SQLiteDatabase.create(null)
            database.execSQL(
                "CREATE TABLE channels (_id INTEGER PRIMARY KEY, browsable INTEGER, internal_provider_flag1 INTEGER)",
            )
            return true
        }

        override fun applyBatch(operations: ArrayList<ContentProviderOperation>): Array<ContentProviderResult> {
            database.beginTransaction()
            try {
                val results = super.applyBatch(operations)
                database.setTransactionSuccessful()
                committedBatches++
                return results
            } finally {
                database.endTransaction()
            }
        }

        override fun insert(
            uri: Uri,
            values: ContentValues?,
        ): Uri = TvContract.buildChannelUri(database.insertOrThrow("channels", null, values))

        override fun update(
            uri: Uri,
            values: ContentValues?,
            selection: String?,
            selectionArgs: Array<out String>?,
        ): Int {
            val id = ContentUris.parseId(uri)
            updatedIds += id
            return database.update("channels", values, "_id=?", arrayOf(id.toString()))
        }

        override fun delete(
            uri: Uri,
            selection: String?,
            selectionArgs: Array<out String>?,
        ): Int = database.delete("channels", "_id=?", arrayOf(ContentUris.parseId(uri).toString()))

        override fun query(
            uri: Uri,
            projection: Array<out String>?,
            selection: String?,
            selectionArgs: Array<out String>?,
            sortOrder: String?,
        ): Cursor =
            database.query(
                "channels",
                projection,
                "_id=?",
                arrayOf(ContentUris.parseId(uri).toString()),
                null,
                null,
                null,
            )

        override fun getType(uri: Uri): String = TvContract.Channels.CONTENT_ITEM_TYPE
    }
}

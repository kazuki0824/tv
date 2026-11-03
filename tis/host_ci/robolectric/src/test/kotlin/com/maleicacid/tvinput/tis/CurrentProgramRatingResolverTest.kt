package com.maleicacid.tvinput.tis

import android.database.MatrixCursor
import android.media.tv.TvContract
import com.maleicacid.tvinput.aribsi.AribRatingMapper
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import kotlin.test.assertEquals
import kotlin.test.assertTrue

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class CurrentProgramRatingResolverTest {
    private fun candidateCursor(): MatrixCursor =
        MatrixCursor(
            arrayOf(
                TvContract.Programs._ID,
                TvContract.Programs.COLUMN_EVENT_ID,
                TvContract.Programs.COLUMN_START_TIME_UTC_MILLIS,
                TvContract.Programs.COLUMN_END_TIME_UTC_MILLIS,
                TvContract.Programs.COLUMN_CONTENT_RATING,
                TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA,
            ),
        )

    @Test
    fun firstEligibleRowWinsWithoutReadingLaterMalformedRow() {
        val cursor =
            candidateCursor().apply {
                addRow(arrayOf(2L, 21, 900L, 1_100L, null, null))
                // 先頭が適格なら後続行のeventIdは評価されない。
                addRow(arrayOf(1L, "invalid-event-id", 800L, 1_200L, null, null))
            }
        val channel = TvContract.buildChannelUri(7L)
        val resolver =
            CurrentProgramRatingResolver { uri, projection, selection, args, order ->
                assertEquals(null, selection)
                assertEquals(null, args)
                assertEquals(
                    "${TvContract.Programs.COLUMN_START_TIME_UTC_MILLIS} DESC, " +
                        "${TvContract.Programs.COLUMN_END_TIME_UTC_MILLIS} ASC, ${TvContract.Programs._ID} DESC",
                    order,
                )
                assertEquals(6, projection.size)
                assertTrue(uri.toString().contains("program"))
                cursor
            }
        val result =
            resolver.resolveDetailed(
                channelUri = channel,
                serviceKey = null,
                latestEvents = emptyList(),
                ratingProfile = AribRatingMapper.BroadcastProfile.UNRESOLVED,
                nowMillis = 1_000L,
            )
        val ratings = (result as CurrentProgramRatingResolver.ResolveResult.Ratings).ratingSet
        assertEquals(CurrentProgramRatingResolver.Source.TV_PROVIDER_CURRENT_PROGRAM, ratings.source)
        assertEquals(21, ratings.eventId)
        assertTrue(cursor.isClosed)
    }

    @Test
    fun emptyProviderCursorReturnsUnratedFallbackAndClosesCursor() {
        val cursor = candidateCursor()
        val resolver = CurrentProgramRatingResolver { _, _, _, _, _ -> cursor }
        val result =
            resolver.resolveDetailed(
                channelUri = TvContract.buildChannelUri(7L),
                serviceKey = null,
                latestEvents = emptyList(),
                ratingProfile = AribRatingMapper.BroadcastProfile.UNRESOLVED,
                nowMillis = 1_000L,
            )
        val ratings = (result as CurrentProgramRatingResolver.ResolveResult.Ratings).ratingSet
        assertEquals(CurrentProgramRatingResolver.Source.UNRATED_FALLBACK, ratings.source)
        assertTrue(cursor.isClosed)
    }
}

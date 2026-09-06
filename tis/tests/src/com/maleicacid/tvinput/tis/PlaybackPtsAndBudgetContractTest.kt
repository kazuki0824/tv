package com.maleicacid.tvinput.tis

import org.junit.Test

class PlaybackPtsAndBudgetContractTest {
    @Test fun firstPtsUsesSharedHalfPeriodSeedAndTrackLocalWrap() {
        val modulus = 1L shl 33
        val half = 1L shl 32
        val values = PlaybackPipeline.normalizedPtsTicksForTest(
            listOf(
                "video" to (modulus - 1L),
                "video" to 0L,
                "video" to (modulus - 1L),
            ),
        )
        check(values == listOf(half, half + 1L, half))
    }

    @Test fun laterTrackJoinsSharedEpochInEitherArrivalOrder() {
        val modulus = 1L shl 33
        val half = 1L shl 32
        val videoFirst = PlaybackPipeline.normalizedPtsTicksForTest(
            listOf("video" to (modulus - 100L), "audio" to 50L),
        )
        check(videoFirst == listOf(half, half + 150L))

        val audioFirst = PlaybackPipeline.normalizedPtsTicksForTest(
            listOf("audio" to 50L, "video" to (modulus - 100L)),
        )
        check(audioFirst == listOf(half, half - 150L))
    }

    @Test fun signedHalfPeriodDifferenceIsAlwaysNegative() {
        val half = 1L shl 32
        val values = PlaybackPipeline.normalizedPtsTicksForTest(
            listOf("video" to 0L, "audio" to half),
        )
        check(values == listOf(half, 0L))
    }

    @Test fun backpressureFailureRequiresContinuousDeadlineExpiry() {
        check(!PlaybackPipeline.backpressureDeadlineReachedForTest(1_000L, 2_999L, 2_000L))
        check(PlaybackPipeline.backpressureDeadlineReachedForTest(1_000L, 3_000L, 2_000L))
        check(!PlaybackPipeline.backpressureDeadlineReachedForTest(3_000L, 2_000L, 2_000L))
        check(!PlaybackPipeline.backpressureDeadlineReachedForTest(1_000L, 10_000L, 0L))
    }
}

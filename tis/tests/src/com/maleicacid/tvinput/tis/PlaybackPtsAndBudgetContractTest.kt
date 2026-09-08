package com.maleicacid.tvinput.tis

import org.junit.Test

class PlaybackPtsAndBudgetContractTest {
    @Test fun avcBroadcastProfileUsesAndroidCapabilitiesWithoutPromotingUnknownValues() {
        val high = com.maleicacid.tvinput.aribsi.AribAvcSignaling(100, 0, 40)
        check(CodecFormatPolicy.avcProfileLevel(high) == CodecFormatPolicy.AvcProfileLevel(
            android.media.MediaCodecInfo.CodecProfileLevel.AVCProfileHigh,
            android.media.MediaCodecInfo.CodecProfileLevel.AVCLevel4))
        check(CodecFormatPolicy.avcProfileLevel(high.copy(profileIdc = 255)) == null)
        check(CodecFormatPolicy.avcProfileLevel(high.copy(levelIdc = 255)) == null)
        check(CodecFormatPolicy.avcProfileLevel(high.copy(constraintFlags = 3)) == null)
        val baseline1b = com.maleicacid.tvinput.aribsi.AribAvcSignaling(66, 0x10, 11)
        check(CodecFormatPolicy.avcProfileLevel(baseline1b)?.level == android.media.MediaCodecInfo.CodecProfileLevel.AVCLevel1b)
    }

    @Test fun alsAndUnresolvedAudioRemainMetadataEvenWithAnAdtsStreamType() {
        val stream = com.maleicacid.tvinput.aribsi.AribElementaryStream(
            com.maleicacid.tvinput.common.TsPid(0x101), 0x0f, null, null, null,
            codec = "MPEG-4-ALS", codecKind = "AUDIO")
        check(TunerSelectionPolicy.selectAudio(listOf(stream)) == null)
        check(TunerSelectionPolicy.selectAudio(listOf(stream.copy(codec = "AAC-LC"))) != null)
        check(TunerSelectionPolicy.selectAudio(listOf(stream.copy(codec = "AAC-LC",
            codecFacts = com.maleicacid.tvinput.aribsi.AribCodecFacts(resolved = false)))) == null)
        check(TunerSelectionPolicy.selectAudio(listOf(stream.copy(streamType = 0x11, codec = "HE-AAC"))) == null)
    }

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

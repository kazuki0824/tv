// テストの入力・期待値を本体の定数と独立した具体値で記述する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import org.junit.Test

class PlaybackPtsAndBudgetContractTest {
    @Test fun pceBootstrapsDualMonoThroughTheSharedNativeParser() {
        fun bytes(hex: String) = hex.chunked(2).map { it.toInt(16).toByte() }.toByteArray()
        val adts = bytes("fff14c00021ffca0990000000201abe0")
        val format =
            requireNotNull(
                CodecFormatPolicy.adtsAudioFormat(
                    adts,
                    com.maleicacid.tvinput.aribsi
                        .AribCodecFacts(),
                    "AAC",
                ),
            )
        check(format.getInteger(android.media.MediaFormat.KEY_CHANNEL_COUNT) == 2)
        check(format.getInteger(android.media.MediaFormat.KEY_SAMPLE_RATE) == 48000)
        val config = requireNotNull(format.getByteBuffer("csd-0"))
        val actual = ByteArray(config.remaining()).also(config::get)
        check(actual.contentEquals(bytes("118004c80000001001ab")))
        check(
            CodecFormatPolicy.adtsAudioFormat(
                adts.copyOf(13),
                com.maleicacid.tvinput.aribsi
                    .AribCodecFacts(),
                "AAC",
            ) == null,
        )
        check(
            runCatching {
                CodecFormatPolicy.adtsAudioFormat(
                    ByteArray(65537),
                    com.maleicacid.tvinput.aribsi
                        .AribCodecFacts(),
                    "AAC",
                )
            }.exceptionOrNull() is UnsupportedCodecFormat,
        )
    }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    @Test
    fun adtsConfigurationUsesExactAscAndAdvertisedHeAacProfile() {
        val bytes = byteArrayOf(0xff.toByte(), 0xf1.toByte(), 0x58, 0x80.toByte(), 1, 0x3f, 0xfc.toByte())
        val facts =
            com.maleicacid.tvinput.aribsi.AribCodecFacts(
                audioConfigHex = "2b118800",
                audioConfigHeader =
                    com.maleicacid.tvinput.aribsi
                        .AribAudioConfigHeader(5, 24000, 2, 48000, 2),
            )
        val format = requireNotNull(CodecFormatPolicy.adtsAudioFormat(bytes, facts, "HE-AAC"))
        check(format.getInteger(android.media.MediaFormat.KEY_SAMPLE_RATE) == 48000)
        check(format.getInteger(android.media.MediaFormat.KEY_CHANNEL_COUNT) == 2)
        check(format.getInteger(android.media.MediaFormat.KEY_PROFILE) == android.media.MediaCodecInfo.CodecProfileLevel.AACObjectHE)
        val csd = requireNotNull(format.getByteBuffer("csd-0"))
        check(csd.get() == 0x2b.toByte() && csd.get() == 0x11.toByte() && csd.get() == 0x88.toByte() && csd.get() == 0.toByte())
        val mismatch = facts.copy(audioConfigHex = "1210")
        check(runCatching { CodecFormatPolicy.adtsAudioFormat(bytes, mismatch, "HE-AAC") }.exceptionOrNull() is UnsupportedCodecFormat)
    }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    @Test
    fun adtsSevenByteProbeKeepsEightChannelConfigAndWaitsForPce() {
        val bytes = byteArrayOf(0xff.toByte(), 0xf1.toByte(), 0x4d, 0xc0.toByte(), 1, 0x3f, 0xfc.toByte())
        val facts =
            com.maleicacid.tvinput.aribsi
                .AribCodecFacts()
        val format = requireNotNull(CodecFormatPolicy.adtsAudioFormat(bytes, facts, "AAC"))
        check(format.getInteger(android.media.MediaFormat.KEY_CHANNEL_COUNT) == 8)
        check(format.getInteger(android.media.MediaFormat.KEY_AAC_PROFILE) == android.media.MediaCodecInfo.CodecProfileLevel.AACObjectLC)
        val pce =
            bytes.copyOf().also {
                it[2] = 0x4c
                it[3] = 0
            }
        check(CodecFormatPolicy.adtsAudioFormat(pce, facts, "AAC") == null)
        check(CodecFormatPolicy.adtsAudioFormat(bytes.copyOf(6), facts, "AAC") == null)
        val reservedFrequency = bytes.copyOf().also { it[2] = 0x7d }
        check(CodecFormatPolicy.adtsAudioFormat(reservedFrequency, facts, "AAC") == null)
    }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    @Test
    fun avcBroadcastProfileUsesAndroidCapabilitiesWithoutPromotingUnknownValues() {
        val high =
            com.maleicacid.tvinput.aribsi
                .AribAvcSignaling(100, 0, 40)
        check(
            CodecFormatPolicy.avcProfileLevel(high) ==
                CodecFormatPolicy.AvcProfileLevel(
                    android.media.MediaCodecInfo.CodecProfileLevel.AVCProfileHigh,
                    android.media.MediaCodecInfo.CodecProfileLevel.AVCLevel4,
                ),
        )
        check(CodecFormatPolicy.avcProfileLevel(high.copy(profileIdc = 255)) == null)
        check(CodecFormatPolicy.avcProfileLevel(high.copy(levelIdc = 255)) == null)
        check(CodecFormatPolicy.avcProfileLevel(high.copy(constraintFlags = 3)) == null)
        val baseline1b =
            com.maleicacid.tvinput.aribsi
                .AribAvcSignaling(66, 0x10, 11)
        check(CodecFormatPolicy.avcProfileLevel(baseline1b)?.level == android.media.MediaCodecInfo.CodecProfileLevel.AVCLevel1b)
    }

    @Test fun alsAndUnresolvedAudioRemainMetadataEvenWithAnAdtsStreamType() {
        val stream =
            com.maleicacid.tvinput.aribsi.AribElementaryStream(
                com.maleicacid.tvinput.common
                    .TsPid(0x101),
                0x0f,
                null,
                null,
                null,
                codec = "MPEG-4-ALS",
                codecKind = "AUDIO",
            )
        check(TunerSelectionPolicy.selectAudio(listOf(stream)) == null)
        check(TunerSelectionPolicy.selectAudio(listOf(stream.copy(codec = "AAC-LC"))) != null)
        check(
            TunerSelectionPolicy.selectAudio(
                listOf(
                    stream.copy(
                        codec = "AAC-LC",
                        codecFacts =
                            com.maleicacid.tvinput.aribsi
                                .AribCodecFacts(resolved = false),
                    ),
                ),
            ) == null,
        )
        check(TunerSelectionPolicy.selectAudio(listOf(stream.copy(streamType = 0x11, codec = "HE-AAC"))) == null)
    }

    @Test fun firstPtsUsesSharedHalfPeriodSeedAndTrackLocalWrap() {
        val modulus = 1L shl 33
        val half = 1L shl 32
        val values =
            PlaybackPipeline.normalizedPtsTicksForTest(
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
        val videoFirst =
            PlaybackPipeline.normalizedPtsTicksForTest(
                listOf("video" to (modulus - 100L), "audio" to 50L),
            )
        check(videoFirst == listOf(half, half + 150L))

        val audioFirst =
            PlaybackPipeline.normalizedPtsTicksForTest(
                listOf("audio" to 50L, "video" to (modulus - 100L)),
            )
        check(audioFirst == listOf(half, half - 150L))
    }

    @Test fun signedHalfPeriodDifferenceIsAlwaysNegative() {
        val half = 1L shl 32
        val values =
            PlaybackPipeline.normalizedPtsTicksForTest(
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

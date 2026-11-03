// テスト専用の観測・fixtureは製品source setへ含めない。
package com.maleicacid.tvinput.tis

fun PlaybackPipeline.currentPlaybackGenerationForTest(): Long =
    javaClass.getDeclaredField("playbackGeneration").apply { isAccessible = true }.getLong(this)

fun PlaybackPipeline.Companion.normalizedPtsTicksForTest(samples: List<Pair<String, Long>>): List<Long> {
    val coordinator = PlaybackPipeline.PtsEpochCoordinator()
    return samples.map { (track, rawPts) ->
        coordinator.normalizeTicks(
            when (track) {
                "video" -> PlaybackPipeline.PtsTrack.VIDEO
                "audio" -> PlaybackPipeline.PtsTrack.AUDIO
                "subtitle", "caption" -> PlaybackPipeline.PtsTrack.CAPTION
                "superimpose" -> PlaybackPipeline.PtsTrack.SUPERIMPOSE
                else -> throw IllegalArgumentException("不明なPTS trackです: $track")
            },
            rawPts,
        )
    }
}

fun h264DimensionsForTest(spsWithStartCode: ByteArray): Pair<Int, Int>? =
    PlaybackPipeline.EsHeaderParser.parseAvcSpsDimensions(spsWithStartCode)?.let { it.width to it.height }

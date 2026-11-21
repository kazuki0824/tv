// テストの入力・期待値を本体の定数と独立した具体値で記述する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import org.junit.Test

class PlaybackMediaEventBoundsTest {
    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    @Test
    fun mediaEventBoundsRejectMalformedOversizedAndOutOfBoundsBeforeCopy() {
        check(PlaybackPipeline.mediaEventBoundsDecision(-1, 1, 16) == PlaybackPipeline.MediaEventBoundsDecision.MALFORMED)
        check(PlaybackPipeline.mediaEventBoundsDecision(0, 0, 16) == PlaybackPipeline.MediaEventBoundsDecision.MALFORMED)
        check(
            PlaybackPipeline.mediaEventBoundsDecision(Long.MAX_VALUE, 1, Long.MAX_VALUE) ==
                PlaybackPipeline.MediaEventBoundsDecision.MALFORMED,
        )
        check(
            PlaybackPipeline.mediaEventBoundsDecision(0, Int.MAX_VALUE.toLong() + 1L, Int.MAX_VALUE.toLong() + 1L) ==
                PlaybackPipeline.MediaEventBoundsDecision.OVERSIZED,
        )
        check(PlaybackPipeline.mediaEventBoundsDecision(8, 8, 15) == PlaybackPipeline.MediaEventBoundsDecision.OUT_OF_BOUNDS)
        check(PlaybackPipeline.mediaEventBoundsDecision(8, 8, 16) == PlaybackPipeline.MediaEventBoundsDecision.ACCEPT)
    }
}

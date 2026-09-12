// テストの入力・期待値を本体の定数と独立した具体値で記述する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import org.junit.Test

class PlaybackMediaEventBoundsTest {
    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    @Test
    fun mediaEventBoundsRejectMalformedOversizedAndOutOfBoundsBeforeCopy() {
        check(PlaybackPipeline.mediaEventBoundsDecisionForTest(-1, 1, 16) == PlaybackPipeline.MediaEventBoundsDecision.MALFORMED)
        check(PlaybackPipeline.mediaEventBoundsDecisionForTest(0, 0, 16) == PlaybackPipeline.MediaEventBoundsDecision.MALFORMED)
        check(
            PlaybackPipeline.mediaEventBoundsDecisionForTest(Long.MAX_VALUE, 1, Long.MAX_VALUE) ==
                PlaybackPipeline.MediaEventBoundsDecision.MALFORMED,
        )
        check(
            PlaybackPipeline.mediaEventBoundsDecisionForTest(0, Int.MAX_VALUE.toLong() + 1L, Int.MAX_VALUE.toLong() + 1L) ==
                PlaybackPipeline.MediaEventBoundsDecision.OVERSIZED,
        )
        check(PlaybackPipeline.mediaEventBoundsDecisionForTest(8, 8, 15) == PlaybackPipeline.MediaEventBoundsDecision.OUT_OF_BOUNDS)
        check(PlaybackPipeline.mediaEventBoundsDecisionForTest(8, 8, 16) == PlaybackPipeline.MediaEventBoundsDecision.ACCEPT)
    }
}

package com.maleicacid.tvinput.tis

import org.junit.Test

class PlaybackMediaEventBoundsTest {
    @Test fun mediaEventBoundsRejectMalformedOversizedAndOutOfBoundsBeforeCopy() {
        check(PlaybackPipeline.mediaEventBoundsDecisionForTest(-1, 1, 16) == PlaybackPipeline.MediaEventBoundsDecision.MALFORMED)
        check(PlaybackPipeline.mediaEventBoundsDecisionForTest(0, 0, 16) == PlaybackPipeline.MediaEventBoundsDecision.MALFORMED)
        check(PlaybackPipeline.mediaEventBoundsDecisionForTest(Long.MAX_VALUE, 1, Long.MAX_VALUE) == PlaybackPipeline.MediaEventBoundsDecision.MALFORMED)
        check(PlaybackPipeline.mediaEventBoundsDecisionForTest(0, Int.MAX_VALUE.toLong() + 1L, Int.MAX_VALUE.toLong() + 1L) == PlaybackPipeline.MediaEventBoundsDecision.OVERSIZED)
        check(PlaybackPipeline.mediaEventBoundsDecisionForTest(8, 8, 15) == PlaybackPipeline.MediaEventBoundsDecision.OUT_OF_BOUNDS)
        check(PlaybackPipeline.mediaEventBoundsDecisionForTest(8, 8, 16) == PlaybackPipeline.MediaEventBoundsDecision.ACCEPT)
    }
}

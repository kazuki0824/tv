package com.maleicacid.tvinput.tis

import org.junit.Test

class PlaybackDecoderBackpressureTest {
    @Test fun directBlockModelRejectsInvalidRangesWithoutByteBufferSizingFallback() {
        check(
            PlaybackPipeline.mediaEventBoundsDecisionForTest(0, 16, 16) ==
                PlaybackPipeline.MediaEventBoundsDecision.ACCEPT,
        )
        check(
            PlaybackPipeline.mediaEventBoundsDecisionForTest(0, 17, 16) ==
                PlaybackPipeline.MediaEventBoundsDecision.OUT_OF_BOUNDS,
        )
        check(
            PlaybackPipeline.mediaEventBoundsDecisionForTest(0, Int.MAX_VALUE.toLong() + 1, Long.MAX_VALUE) ==
                PlaybackPipeline.MediaEventBoundsDecision.OVERSIZED,
        )
    }
}

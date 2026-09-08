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
    @Test fun noInputExpiresWithoutAnyQueuePressure() {
        val deadline = DecoderStartupDeadline(100L, 3_000L)
        check(deadline.expire(3_099L) == null)
        check(deadline.expire(3_100L) == DecoderStartupDeadline.Stage.CONFIGURATION)
        check(deadline.expire(4_000L) == null)
    }

    @Test fun configuredButSilentAudioOrVideoStillExpires() {
        val deadline = DecoderStartupDeadline(100L, 3_000L)
        deadline.onConfigured()
        check(deadline.expire(3_100L) == DecoderStartupDeadline.Stage.FIRST_OUTPUT)
    }

    @Test fun firstOutputAndTeardownDisarmOldDeadline() {
        val playing = DecoderStartupDeadline(100L, 3_000L)
        playing.onConfigured()
        playing.onFirstOutput()
        check(playing.firstOutputSeen)
        check(playing.expire(9_000L) == null)
        val closed = DecoderStartupDeadline(100L, 3_000L)
        closed.close()
        closed.onFirstOutput()
        check(!closed.firstOutputSeen)
        check(closed.expire(9_000L) == null)
    }
}

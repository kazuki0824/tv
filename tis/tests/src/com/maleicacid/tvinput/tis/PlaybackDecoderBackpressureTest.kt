package com.maleicacid.tvinput.tis

import org.junit.Test

class PlaybackDecoderBackpressureTest {
    @Test fun oversizedSampleDecisionDoesNotQueueBytesBeyondInputBuffer() {
        check(!PlaybackPipeline.shouldDropOversizedSampleForTest(sampleSize = 16, inputRemaining = 16))
        check(PlaybackPipeline.queuedInputSizeForSampleForTest(sampleSize = 16, inputRemaining = 16) == 16)
        check(PlaybackPipeline.shouldDropOversizedSampleForTest(sampleSize = 17, inputRemaining = 16))
        check(PlaybackPipeline.queuedInputSizeForSampleForTest(sampleSize = 17, inputRemaining = 16) == 0)
    }
}

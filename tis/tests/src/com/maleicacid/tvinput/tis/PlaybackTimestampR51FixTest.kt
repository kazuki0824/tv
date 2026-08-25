package com.maleicacid.tvinput.tis

import org.junit.Test

class PlaybackTimestampR51FixTest {
    @Test fun authoritativePtsIsRequiredWithoutConsultingProvenanceBit() {
        check(PlaybackPipeline.shouldQueueMediaEventForPtsForTest(isPtsPresent = true, pts90k = 90_000L))
        check(PlaybackPipeline.shouldQueueMediaEventForPtsForTest(isPtsPresent = false, pts90k = 90_000L))
        check(!PlaybackPipeline.shouldQueueMediaEventForPtsForTest(isPtsPresent = true, pts90k = null))
        check(!PlaybackPipeline.shouldQueueMediaEventForPtsForTest(isPtsPresent = false, pts90k = -1L))
    }
}

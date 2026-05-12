package com.maleicacid.tvinput.tis

import org.junit.Test

class PlaybackTimestampR51FixTest {
    @Test fun ptsFallbackIsReportedSeparately() {
        check(!PlaybackPipeline.normalizedPresentationTimeForTest(90_000L).fallbackUsed)
        check(PlaybackPipeline.normalizedPresentationTimeForTest(null).fallbackUsed)
        check(PlaybackPipeline.normalizedPresentationTimeForTest(-1L).fallbackUsed)
    }
}

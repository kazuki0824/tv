package com.maleicacid.tvinput.tis

import android.media.tv.tuner.filter.AvSettings
import org.junit.Test

class PlaybackAudioSinkR51FixTest {
    @Test fun negativeAudioWriteResultIsError() {
        check(PlaybackPipeline.isAudioWriteErrorForTest(-3))
        check(!PlaybackPipeline.isAudioWriteErrorForTest(0))
        check(!PlaybackPipeline.isAudioWriteErrorForTest(128))
    }

    @Test fun r51SupportsAdtsAacButNotLatmLoasAudioStreamType() {
        check(PlaybackPipeline.isSupportedAudioStreamTypeForTest(0x0f))
        check(!PlaybackPipeline.isSupportedAudioStreamTypeForTest(0x11))
        check(PlaybackPipeline.normalizedAudioStreamTypeForTest(0x0f) == AvSettings.AUDIO_STREAM_TYPE_AAC_ADTS)
        check(PlaybackPipeline.normalizedAudioStreamTypeForTest(0x11) == AvSettings.AUDIO_STREAM_TYPE_UNDEFINED)
    }

    @Test fun oversizedDecoderSampleIsDroppedInsteadOfPrefixQueued() {
        check(!PlaybackPipeline.shouldDropOversizedSampleForTest(sampleSize = 188, inputRemaining = 188))
        check(PlaybackPipeline.shouldDropOversizedSampleForTest(sampleSize = 189, inputRemaining = 188))
        check(PlaybackPipeline.queuedInputSizeForSampleForTest(sampleSize = 188, inputRemaining = 188) == 188)
        check(PlaybackPipeline.queuedInputSizeForSampleForTest(sampleSize = 189, inputRemaining = 188) == 0)
    }

    @Test fun partialAudioWriteWithTransientZeroRetriesUntilRemainingPcmIsWritten() {
        val (lastResult, partialWrites) = PlaybackPipeline.simulateAudioWriteFullyForTest(intArrayOf(128, 0, 0, 128), size = 256)
        check(lastResult == 128)
        check(partialWrites == 3)
    }

    @Test fun repeatedZeroAudioWriteReturnsErrorInsteadOfSpinningForever() {
        val (lastResult, partialWrites) = PlaybackPipeline.simulateAudioWriteFullyForTest(intArrayOf(128), size = 256)
        check(lastResult < 0)
        check(partialWrites > 0)
    }
}

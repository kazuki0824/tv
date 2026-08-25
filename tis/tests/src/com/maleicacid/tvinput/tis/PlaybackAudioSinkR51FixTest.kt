package com.maleicacid.tvinput.tis

import android.media.tv.tuner.filter.AvSettings
import org.junit.Test

class PlaybackAudioSinkR51FixTest {
    @Test fun r51SupportsAdtsAacButNotLatmLoasAudioStreamType() {
        check(PlaybackPipeline.isSupportedAudioStreamTypeForTest(0x0f))
        check(!PlaybackPipeline.isSupportedAudioStreamTypeForTest(0x11))
        check(PlaybackPipeline.normalizedAudioStreamTypeForTest(0x0f) == AvSettings.AUDIO_STREAM_TYPE_AAC_ADTS)
        check(PlaybackPipeline.normalizedAudioStreamTypeForTest(0x11) == AvSettings.AUDIO_STREAM_TYPE_UNDEFINED)
    }
}

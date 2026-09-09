package com.maleicacid.tvinput.tis

import android.media.AudioTrack
import android.media.tv.tuner.filter.AvSettings
import org.junit.Test

class PlaybackAudioSinkR51FixTest {
    @Test fun routeChangeRestartsOnlyCurrentTrackAndGeneration() {
        val oldTrack = Any()
        var currentTrack: Any? = oldTrack
        var generation = 7L
        var released = false
        var restarts = 0
        val gate = AudioRouteChangeGate(generation, oldTrack, null)
        val restart: () -> Unit = {
            released = true
            currentTrack = null
            generation++
            check(released)
            currentTrack = Any()
            restarts++
        }
        gate.onRouteChanged(generation, currentTrack, 1, restart)
        gate.onRouteChanged(generation, currentTrack, 1, restart)
        check(restarts == 0)
        gate.onRouteChanged(generation, currentTrack, 2, restart)
        check(released && generation == 8L && currentTrack !== oldTrack && restarts == 1)
        gate.onRouteChanged(generation, currentTrack, 3, restart)
        gate.onRouteChanged(7L, currentTrack, 3, restart)
        gate.onRouteChanged(generation, oldTrack, 3, restart)
        check(restarts == 1)
    }

    @Test fun r51SupportsAdtsAacButNotLatmLoasAudioStreamType() {
        check(PlaybackPipeline.isSupportedAudioStreamTypeForTest(0x0f))
        check(!PlaybackPipeline.isSupportedAudioStreamTypeForTest(0x11))
        check(PlaybackPipeline.normalizedAudioStreamTypeForTest(0x0f) == AvSettings.AUDIO_STREAM_TYPE_AAC_ADTS)
        check(PlaybackPipeline.normalizedAudioStreamTypeForTest(0x11) == AvSettings.AUDIO_STREAM_TYPE_UNDEFINED)
        check(PlaybackPipeline.isAribDualMonoComponentTypeForTest(0x02))
        check(PlaybackPipeline.isAribDualMonoComponentTypeForTest(0x22))
        check(!PlaybackPipeline.isAribDualMonoComponentTypeForTest(0x03))
        check(PlaybackPipeline.dualMonoModeForTest(PlaybackPipeline.DualMonoPresentation.MAIN) == AudioTrack.DUAL_MONO_MODE_LL)
        check(PlaybackPipeline.dualMonoModeForTest(PlaybackPipeline.DualMonoPresentation.SUB) == AudioTrack.DUAL_MONO_MODE_RR)
        check(PlaybackPipeline.dualMonoModeForTest(PlaybackPipeline.DualMonoPresentation.MAIN_SUB) == AudioTrack.DUAL_MONO_MODE_LR)
        check(PlaybackPipeline.channelMaskForPcmOutputForTest(3) == (android.media.AudioFormat.CHANNEL_OUT_STEREO or android.media.AudioFormat.CHANNEL_OUT_FRONT_CENTER))
        check(PlaybackPipeline.channelMaskForPcmOutputForTest(4) == android.media.AudioFormat.CHANNEL_OUT_QUAD)
        check(PlaybackPipeline.channelMaskForPcmOutputForTest(5) == (android.media.AudioFormat.CHANNEL_OUT_QUAD or android.media.AudioFormat.CHANNEL_OUT_FRONT_CENTER))
        check(PlaybackPipeline.aribChannelMaskForComponentTypeForTest(0x04) == (android.media.AudioFormat.CHANNEL_OUT_STEREO or android.media.AudioFormat.CHANNEL_OUT_BACK_CENTER))
        check(PlaybackPipeline.aribChannelMaskForComponentTypeForTest(0x05) == (android.media.AudioFormat.CHANNEL_OUT_STEREO or android.media.AudioFormat.CHANNEL_OUT_FRONT_CENTER))
        check(PlaybackPipeline.aribChannelMaskForComponentTypeForTest(0x06) == android.media.AudioFormat.CHANNEL_OUT_QUAD)
        check(PlaybackPipeline.aribChannelMaskForComponentTypeForTest(0x07) == android.media.AudioFormat.CHANNEL_OUT_SURROUND)
        check(PlaybackPipeline.resolvePcmChannelMaskForTest(android.media.AudioFormat.CHANNEL_OUT_5POINT1, 6, 0x08) == android.media.AudioFormat.CHANNEL_OUT_5POINT1)
        check(PlaybackPipeline.resolvePcmChannelMaskForTest(null, 3, 0x04) == (android.media.AudioFormat.CHANNEL_OUT_STEREO or android.media.AudioFormat.CHANNEL_OUT_BACK_CENTER))
        check(PlaybackPipeline.resolvePcmChannelMaskForTest(android.media.AudioFormat.CHANNEL_OUT_STEREO, 6, 0x09) == null)
    }
}

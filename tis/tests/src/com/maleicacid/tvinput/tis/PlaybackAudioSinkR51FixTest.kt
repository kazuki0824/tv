// テストの入力・期待値を本体の定数と独立した具体値で記述する。
@file:Suppress("MagicNumber")

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

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    @Test
    fun r51SupportsAdtsAacButNotLatmLoasAudioStreamType() {
        check(TunerSelectionPolicy.isSupportedAudioStreamType(0x0f))
        check(!TunerSelectionPolicy.isSupportedAudioStreamType(0x11))
        check(PlaybackPipeline.audioFilterStreamType(0x0f) == AvSettings.AUDIO_STREAM_TYPE_AAC_ADTS)
        check(PlaybackPipeline.audioFilterStreamType(0x11) == AvSettings.AUDIO_STREAM_TYPE_UNDEFINED)
        check(PlaybackPipeline.audioTrackDualMonoMode(PlaybackPipeline.DualMonoPresentation.MAIN) == AudioTrack.DUAL_MONO_MODE_LL)
        check(PlaybackPipeline.audioTrackDualMonoMode(PlaybackPipeline.DualMonoPresentation.SUB) == AudioTrack.DUAL_MONO_MODE_RR)
        check(PlaybackPipeline.audioTrackDualMonoMode(PlaybackPipeline.DualMonoPresentation.MAIN_SUB) == AudioTrack.DUAL_MONO_MODE_LR)
        check(
            PlaybackPipeline.PcmChannelMaskPolicy.canonicalForCount(3) ==
                (android.media.AudioFormat.CHANNEL_OUT_STEREO or android.media.AudioFormat.CHANNEL_OUT_FRONT_CENTER),
        )
        check(PlaybackPipeline.PcmChannelMaskPolicy.canonicalForCount(4) == android.media.AudioFormat.CHANNEL_OUT_QUAD)
        check(
            PlaybackPipeline.PcmChannelMaskPolicy.canonicalForCount(5) ==
                (android.media.AudioFormat.CHANNEL_OUT_QUAD or android.media.AudioFormat.CHANNEL_OUT_FRONT_CENTER),
        )
        check(
            PlaybackPipeline.PcmChannelMaskPolicy.fromChannelConfiguration("2/1") ==
                (android.media.AudioFormat.CHANNEL_OUT_STEREO or android.media.AudioFormat.CHANNEL_OUT_BACK_CENTER),
        )
        check(
            PlaybackPipeline.PcmChannelMaskPolicy.fromChannelConfiguration("3/0") ==
                (android.media.AudioFormat.CHANNEL_OUT_STEREO or android.media.AudioFormat.CHANNEL_OUT_FRONT_CENTER),
        )
        check(PlaybackPipeline.PcmChannelMaskPolicy.fromChannelConfiguration("2/2") == android.media.AudioFormat.CHANNEL_OUT_QUAD)
        check(PlaybackPipeline.PcmChannelMaskPolicy.fromChannelConfiguration("3/1") == android.media.AudioFormat.CHANNEL_OUT_SURROUND)
        check(
            PlaybackPipeline.PcmChannelMaskPolicy.resolve(android.media.AudioFormat.CHANNEL_OUT_5POINT1, 6, "3/2") ==
                android.media.AudioFormat.CHANNEL_OUT_5POINT1,
        )
        check(
            PlaybackPipeline.PcmChannelMaskPolicy.resolve(null, 3, "2/1") ==
                (android.media.AudioFormat.CHANNEL_OUT_STEREO or android.media.AudioFormat.CHANNEL_OUT_BACK_CENTER),
        )
        check(PlaybackPipeline.PcmChannelMaskPolicy.resolve(android.media.AudioFormat.CHANNEL_OUT_STEREO, 6, "3/2+LFE") == null)
    }
}

// 診断専用counterは再生の成功判定・資源寿命を変更しない。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import org.junit.Test

class PlaybackDataPathProgressTest {
    @Test
    fun audioOutputCannotMasqueradeAsVideoOutputAndResetClearsBoth() {
        val progress = PlaybackDataPathProgress()
        check(
            PlaybackDataPathProgress.Stage.entries.all { stage ->
                !listOf(false, true).any { progress.count(stage, it) != 0L }
            },
        )
        progress.record(PlaybackDataPathProgress.Stage.MEDIA_EVENT, true)
        progress.record(PlaybackDataPathProgress.Stage.CODEC_INPUT, true)
        progress.record(PlaybackDataPathProgress.Stage.CODEC_INPUT_AVAILABLE, true)
        progress.record(PlaybackDataPathProgress.Stage.CODEC_INPUT_RETURNED, true)
        progress.record(PlaybackDataPathProgress.Stage.CODEC_OUTPUT, true)
        progress.record(PlaybackDataPathProgress.Stage.MEDIASYNC_AUDIO_CONSUMED, true)
        progress.record(PlaybackDataPathProgress.Stage.MEDIA_EVENT, false)
        progress.record(PlaybackDataPathProgress.Stage.OVERFLOW, false)
        progress.record(PlaybackDataPathProgress.Stage.FLUSH_FAILED, false)
        check(progress.count(PlaybackDataPathProgress.Stage.CODEC_OUTPUT, true) == 1L)
        check(progress.count(PlaybackDataPathProgress.Stage.CODEC_OUTPUT, false) == 0L)
        check(progress.count(PlaybackDataPathProgress.Stage.CODEC_INPUT_AVAILABLE, true) == 1L)
        check(progress.count(PlaybackDataPathProgress.Stage.CODEC_INPUT_RETURNED, true) == 1L)
        check(progress.count(PlaybackDataPathProgress.Stage.MEDIASYNC_AUDIO_CONSUMED, true) == 1L)
        check(progress.count(PlaybackDataPathProgress.Stage.MEDIASYNC_AUDIO_CONSUMED, false) == 0L)
        check(progress.count(PlaybackDataPathProgress.Stage.FLUSH_FAILED, true) == 0L)
        check(progress.count(PlaybackDataPathProgress.Stage.FLUSH_FAILED, false) == 1L)
        check(progress.summary().contains("AUDIO.CODEC_OUTPUT=1"))
        check(progress.summary().contains("VIDEO.CODEC_OUTPUT=0"))
        progress.reset()
        check(
            PlaybackDataPathProgress.Stage.entries.all { stage ->
                !listOf(false, true).any { progress.count(stage, it) != 0L }
            },
        )
    }
}

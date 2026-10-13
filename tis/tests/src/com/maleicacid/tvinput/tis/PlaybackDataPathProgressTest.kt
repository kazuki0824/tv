// 診断専用counterは再生の成功判定・資源寿命を変更しない。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import org.junit.Test

class PlaybackDataPathProgressTest {
    @Test
    fun countersTrackEachStageWithoutInventingProgress() {
        val progress = PlaybackDataPathProgress()
        check(PlaybackDataPathProgress.Stage.entries.all { progress.count(it) == 0L })
        progress.record(PlaybackDataPathProgress.Stage.MEDIA_EVENT)
        progress.record(PlaybackDataPathProgress.Stage.MEDIA_EVENT)
        progress.record(PlaybackDataPathProgress.Stage.VALID_SAMPLE)
        progress.record(PlaybackDataPathProgress.Stage.OVERFLOW)
        check(progress.count(PlaybackDataPathProgress.Stage.MEDIA_EVENT) == 2L)
        check(progress.count(PlaybackDataPathProgress.Stage.VALID_SAMPLE) == 1L)
        check(progress.count(PlaybackDataPathProgress.Stage.OVERFLOW) == 1L)
        check(progress.count(PlaybackDataPathProgress.Stage.CODEC_INPUT) == 0L)
        check(progress.summary().contains("MEDIA_EVENT=2"))
        check(progress.summary().contains("CODEC_INPUT=0"))
        progress.reset()
        check(PlaybackDataPathProgress.Stage.entries.all { progress.count(it) == 0L })
    }
}

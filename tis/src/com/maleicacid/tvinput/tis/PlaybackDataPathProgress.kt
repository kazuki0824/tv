package com.maleicacid.tvinput.tis

/**
 * 同一playback generation内のデータ経路の進捗を集計する診断専用値。
 * 再生開始・停止・回復の判定には使用せず、既存playback executorだけが更新する。
 */
internal class PlaybackDataPathProgress {
    enum class Stage {
        MEDIA_EVENT,
        VALID_SAMPLE,
        CODEC_INPUT,
        CODEC_OUTPUT,
        FRAME_RENDERED,
        OVERFLOW,
        FLUSH_SUCCEEDED,
        FLUSH_FAILED,
        MISSING_LINEAR_BLOCK,
        INPUT_REJECTED,
    }

    private val counts = LongArray(Stage.entries.size)

    fun record(stage: Stage) {
        val index = stage.ordinal
        if (counts[index] != Long.MAX_VALUE) counts[index]++
    }

    fun count(stage: Stage): Long = counts[stage.ordinal]

    fun reset() {
        counts.fill(0L)
    }

    fun summary(): String =
        Stage.entries.joinToString(separator = " ") { stage -> "${stage.name}=${count(stage)}" }
}

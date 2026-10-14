package com.maleicacid.tvinput.tis

/**
 * 同一playback generation内の映像・音声それぞれのデータ経路進捗を集計する診断専用値。
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

    private val videoCounts = LongArray(Stage.entries.size)
    private val audioCounts = LongArray(Stage.entries.size)

    fun record(
        stage: Stage,
        isAudio: Boolean,
    ) {
        val counts = if (isAudio) audioCounts else videoCounts
        val index = stage.ordinal
        if (counts[index] != Long.MAX_VALUE) counts[index]++
    }

    fun count(
        stage: Stage,
        isAudio: Boolean,
    ): Long = (if (isAudio) audioCounts else videoCounts)[stage.ordinal]

    fun reset() {
        videoCounts.fill(0L)
        audioCounts.fill(0L)
    }

    fun summary(): String =
        Stage.entries.joinToString(separator = " ") { stage ->
            "VIDEO.${stage.name}=${count(stage, false)} AUDIO.${stage.name}=${count(stage, true)}"
        }
}

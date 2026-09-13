package com.maleicacid.tvinput.tis

/** 入力量に依存しない、1 decoder世代の起動期限。呼出元の再生executorだけで使用する。 */
internal class DecoderStartupDeadline(
    private val startedAtMs: Long,
    private val deadlineMs: Long,
) {
    enum class Stage { CONFIGURATION, FIRST_OUTPUT, STARTED, CLOSED }

    private var stage = Stage.CONFIGURATION
    val firstOutputSeen: Boolean get() = stage == Stage.STARTED

    init {
        require(startedAtMs >= 0L && deadlineMs > 0L)
    }

    fun onConfigured() {
        if (stage == Stage.CONFIGURATION) stage = Stage.FIRST_OUTPUT
    }

    fun onFirstOutput() {
        if (stage == Stage.CONFIGURATION || stage == Stage.FIRST_OUTPUT) stage = Stage.STARTED
    }

    // 入力拒否・未準備・失敗を発生点で返し、成功経路を深い入れ子にしない。
    @Suppress("ReturnCount")
    fun expire(nowMs: Long): Stage? {
        if (stage != Stage.CONFIGURATION && stage != Stage.FIRST_OUTPUT) return null
        if (nowMs < startedAtMs || nowMs - startedAtMs < deadlineMs) return null
        val expiredStage = stage
        stage = Stage.CLOSED
        return expiredStage
    }

    fun close() {
        stage = Stage.CLOSED
    }
}

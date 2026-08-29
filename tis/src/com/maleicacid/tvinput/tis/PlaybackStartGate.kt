package com.maleicacid.tvinput.tis

import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.TsPid

data class AvPlaybackSignature(
    val serviceKey: ServiceKey,
    val pcrPid: TsPid?,
    val videoPid: TsPid?,
    val videoStreamType: Int?,
    val audioPid: TsPid?,
    val audioStreamType: Int?,
    val clear: Boolean,
    val keyTokenAvailable: Boolean,
    val subtitlePid: TsPid? = null,
    val subtitleDataComponentId: Int? = null,
)

/** LiveSession が一つだけ所有する AV 再生 lifecycle。 */
sealed class PlaybackStartState {
    object Idle : PlaybackStartState()
    data class Starting(val signature: AvPlaybackSignature) : PlaybackStartState()
    data class WaitingFirstOutput(
        val signature: AvPlaybackSignature,
        val pipelineGeneration: Long,
    ) : PlaybackStartState()
    data class Started(
        val signature: AvPlaybackSignature,
        val pipelineGeneration: Long,
    ) : PlaybackStartState()
    data class Failed(
        val signature: AvPlaybackSignature,
        val pipelineGeneration: Long?,
    ) : PlaybackStartState()
    object Stopped : PlaybackStartState()
}

/**
 * [PlaybackStartState] を保持しない純粋な遷移判定。
 *
 * section 更新は頻繁に発生するため、同じ AV 署名の開始済み・開始中・失敗済み状態では
 * 再試行しない。Surface 再接続など外部条件が変化した場合だけ [allowRetry] で Idle へ戻す。
 */
object PlaybackStartTransitions {
    fun shouldAttempt(state: PlaybackStartState, signature: AvPlaybackSignature): Boolean = when (state) {
        PlaybackStartState.Idle,
        PlaybackStartState.Stopped,
        -> true
        is PlaybackStartState.Starting -> state.signature != signature
        is PlaybackStartState.WaitingFirstOutput -> state.signature != signature
        is PlaybackStartState.Started -> state.signature != signature
        is PlaybackStartState.Failed -> state.signature != signature
    }

    fun allowRetry(state: PlaybackStartState): PlaybackStartState = when (state) {
        PlaybackStartState.Stopped,
        is PlaybackStartState.Failed,
        -> PlaybackStartState.Idle
        else -> state
    }

    fun afterSuccessfulRestart(
        signature: AvPlaybackSignature,
        pipelineGeneration: Long,
        firstOutputPending: Boolean,
    ): PlaybackStartState = if (firstOutputPending) {
        PlaybackStartState.WaitingFirstOutput(signature, pipelineGeneration)
    } else {
        PlaybackStartState.Started(signature, pipelineGeneration)
    }

    fun failCurrentGeneration(
        state: PlaybackStartState,
        failedGeneration: Long,
    ): PlaybackStartState {
        val signature = signature(state) ?: return state
        return if (pipelineGeneration(state) == failedGeneration) {
            PlaybackStartState.Failed(signature, failedGeneration)
        } else {
            state
        }
    }

    fun signature(state: PlaybackStartState): AvPlaybackSignature? = when (state) {
        is PlaybackStartState.Starting -> state.signature
        is PlaybackStartState.WaitingFirstOutput -> state.signature
        is PlaybackStartState.Started -> state.signature
        is PlaybackStartState.Failed -> state.signature
        PlaybackStartState.Idle,
        PlaybackStartState.Stopped,
        -> null
    }

    fun pipelineGeneration(state: PlaybackStartState): Long? = when (state) {
        is PlaybackStartState.WaitingFirstOutput -> state.pipelineGeneration
        is PlaybackStartState.Started -> state.pipelineGeneration
        is PlaybackStartState.Failed -> state.pipelineGeneration
        PlaybackStartState.Idle,
        is PlaybackStartState.Starting,
        PlaybackStartState.Stopped,
        -> null
    }
}

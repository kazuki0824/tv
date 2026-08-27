package com.maleicacid.tvinput.tis

import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.TsPid
import org.junit.Test

class PlaybackStartGateTest {
    private val key = ServiceKey(originalNetworkId = 4, transportStreamId = 0x4010, serviceId = 101)

    @Test fun repeatedSectionUpdatesAfterFailedStartDoNotRetrySameSignature() {
        val signature = signature(videoPid = TsPid(0x0101), audioPid = TsPid(0x0102))
        val state: PlaybackStartState = PlaybackStartState.Failed(signature, pipelineGeneration = null)

        check(!PlaybackStartTransitions.shouldAttempt(state, signature)) {
            "失敗後に同一AV署名でPlaybackPipeline.start()を再実行してはなりません"
        }
    }

    @Test fun eitCatEcmEmmUpdatesWithSameSignatureStayNoopAfterStarted() {
        val signature = signature(videoPid = TsPid(0x0101), audioPid = TsPid(0x0102))
        val state: PlaybackStartState = PlaybackStartState.Started(signature, pipelineGeneration = 7L)

        repeat(5) {
            check(!PlaybackStartTransitions.shouldAttempt(state, signature)) {
                "metadataだけのsection更新で再生を再起動してはなりません"
            }
        }
    }

    @Test fun pmtPidChangeAllowsExactlyOneNewAttempt() {
        val first = signature(videoPid = TsPid(0x0101), audioPid = TsPid(0x0102))
        val changed = signature(videoPid = TsPid(0x0201), audioPid = TsPid(0x0202))
        var state: PlaybackStartState = PlaybackStartState.Started(first, pipelineGeneration = 7L)

        check(PlaybackStartTransitions.shouldAttempt(state, changed))
        state = PlaybackStartState.Starting(changed)
        check(!PlaybackStartTransitions.shouldAttempt(state, changed))
        state = PlaybackStartState.Started(changed, pipelineGeneration = 8L)
        check(!PlaybackStartTransitions.shouldAttempt(state, changed))
    }

    @Test fun surfaceReattachAllowsRetryingPreviouslyFailedSignature() {
        val signature = signature(videoPid = TsPid(0x0101), audioPid = null)
        var state: PlaybackStartState = PlaybackStartState.Failed(signature, pipelineGeneration = null)

        check(!PlaybackStartTransitions.shouldAttempt(state, signature))
        state = PlaybackStartTransitions.allowRetry(state)
        check(PlaybackStartTransitions.shouldAttempt(state, signature)) {
            "新しいSurfaceまたは外部条件変更では1回の再試行を許可する必要があります"
        }
    }

    @Test fun newTuneResetsUnifiedStateToIdle() {
        val signature = signature(videoPid = TsPid(0x0101), audioPid = TsPid(0x0102))
        var state: PlaybackStartState = PlaybackStartState.Started(signature, pipelineGeneration = 7L)

        check(!PlaybackStartTransitions.shouldAttempt(state, signature))
        state = PlaybackStartState.Idle
        check(PlaybackStartTransitions.shouldAttempt(state, signature))
    }

    private fun signature(videoPid: TsPid, audioPid: TsPid?): AvPlaybackSignature = AvPlaybackSignature(
        serviceKey = key,
        pcrPid = TsPid(0x0100),
        videoPid = videoPid,
        videoStreamType = 0x1b,
        audioPid = audioPid,
        audioStreamType = audioPid?.let { 0x0f },
        clear = true,
        keyTokenAvailable = false,
    )
}

package com.maleicacid.tvinput.tis

import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.TsPid
import org.junit.Test

class PlaybackStartGateTest {
    private val key = ServiceKey(originalNetworkId = 4, transportStreamId = 0x4010, serviceId = 101)

    @Test fun repeatedSectionUpdatesAfterFailedStartDoNotRetrySameSignature() {
        val gate = PlaybackStartGate()
        val signature = signature(videoPid = TsPid(0x0101), audioPid = TsPid(0x0102))

        check(gate.shouldAttempt(signature))
        gate.recordAttempt(signature)
        gate.recordResult(signature, startedVideo = false)

        check(!gate.shouldAttempt(signature)) {
            "失敗後に同一AV署名でPlaybackPipeline.start()を再実行してはなりません"
        }
    }

    @Test fun eitCatEcmEmmUpdatesWithSameSignatureStayNoopAfterStarted() {
        val gate = PlaybackStartGate()
        val signature = signature(videoPid = TsPid(0x0101), audioPid = TsPid(0x0102))

        check(gate.shouldAttempt(signature))
        gate.recordAttempt(signature)
        gate.recordResult(signature, startedVideo = true)

        repeat(5) {
            check(!gate.shouldAttempt(signature)) {
                "metadataだけのsection更新で再生を再起動してはなりません"
            }
        }
    }

    @Test fun pmtPidChangeAllowsExactlyOneNewAttempt() {
        val gate = PlaybackStartGate()
        val first = signature(videoPid = TsPid(0x0101), audioPid = TsPid(0x0102))
        val changed = signature(videoPid = TsPid(0x0201), audioPid = TsPid(0x0202))

        gate.recordAttempt(first)
        gate.recordResult(first, startedVideo = true)

        check(gate.shouldAttempt(changed))
        gate.recordAttempt(changed)
        gate.recordResult(changed, startedVideo = true)
        check(!gate.shouldAttempt(changed))
    }

    @Test fun surfaceReattachAllowsRetryingPreviouslyFailedSignature() {
        val gate = PlaybackStartGate()
        val signature = signature(videoPid = TsPid(0x0101), audioPid = null)

        gate.recordAttempt(signature)
        gate.recordResult(signature, startedVideo = false)
        check(!gate.shouldAttempt(signature))

        gate.allowRetry()
        check(gate.shouldAttempt(signature)) {
            "新しいSurfaceまたは外部条件変更では1回の再試行を許可する必要があります"
        }
    }

    @Test fun newTuneResetsAllGateState() {
        val gate = PlaybackStartGate()
        val signature = signature(videoPid = TsPid(0x0101), audioPid = TsPid(0x0102))

        gate.recordAttempt(signature)
        gate.recordResult(signature, startedVideo = true)
        check(!gate.shouldAttempt(signature))

        gate.reset()
        check(gate.shouldAttempt(signature))
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

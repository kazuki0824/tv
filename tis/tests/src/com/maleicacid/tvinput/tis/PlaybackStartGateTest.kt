package com.maleicacid.tvinput.tis

import com.maleicacid.tvinput.common.ServiceKey
import org.junit.Test

class PlaybackStartGateTest {
    private val key = ServiceKey(originalNetworkId = 4, transportStreamId = 0x4010, serviceId = 101)

    @Test fun repeatedSectionUpdatesAfterFailedStartDoNotRetrySameSignature() {
        val gate = PlaybackStartGate()
        val signature = signature(videoPid = 0x0101, audioPid = 0x0102)

        check(gate.shouldAttempt(signature))
        gate.recordAttempt(signature)
        gate.recordResult(signature, startedVideo = false)

        check(!gate.shouldAttempt(signature)) {
            "same AV signature must not invoke PlaybackPipeline.start() again after a failed attempt"
        }
    }

    @Test fun eitCatEcmEmmUpdatesWithSameSignatureStayNoopAfterStarted() {
        val gate = PlaybackStartGate()
        val signature = signature(videoPid = 0x0101, audioPid = 0x0102)

        check(gate.shouldAttempt(signature))
        gate.recordAttempt(signature)
        gate.recordResult(signature, startedVideo = true)

        repeat(5) {
            check(!gate.shouldAttempt(signature)) {
                "metadata-only section refresh must not restart playback"
            }
        }
    }

    @Test fun pmtPidChangeAllowsExactlyOneNewAttempt() {
        val gate = PlaybackStartGate()
        val first = signature(videoPid = 0x0101, audioPid = 0x0102)
        val changed = signature(videoPid = 0x0201, audioPid = 0x0202)

        gate.recordAttempt(first)
        gate.recordResult(first, startedVideo = true)

        check(gate.shouldAttempt(changed))
        gate.recordAttempt(changed)
        gate.recordResult(changed, startedVideo = true)
        check(!gate.shouldAttempt(changed))
    }

    @Test fun surfaceReattachAllowsRetryingPreviouslyFailedSignature() {
        val gate = PlaybackStartGate()
        val signature = signature(videoPid = 0x0101, audioPid = null)

        gate.recordAttempt(signature)
        gate.recordResult(signature, startedVideo = false)
        check(!gate.shouldAttempt(signature))

        gate.allowRetry()
        check(gate.shouldAttempt(signature)) {
            "new Surface / external condition change must permit one retry"
        }
    }

    @Test fun newTuneResetsAllGateState() {
        val gate = PlaybackStartGate()
        val signature = signature(videoPid = 0x0101, audioPid = 0x0102)

        gate.recordAttempt(signature)
        gate.recordResult(signature, startedVideo = true)
        check(!gate.shouldAttempt(signature))

        gate.reset()
        check(gate.shouldAttempt(signature))
    }

    private fun signature(videoPid: Int, audioPid: Int?): AvPlaybackSignature = AvPlaybackSignature(
        serviceKey = key,
        pcrPid = 0x0100,
        videoPid = videoPid,
        videoStreamType = 0x1b,
        audioPid = audioPid,
        audioStreamType = audioPid?.let { 0x0f },
        clear = true,
        keyTokenAvailable = false,
    )
}

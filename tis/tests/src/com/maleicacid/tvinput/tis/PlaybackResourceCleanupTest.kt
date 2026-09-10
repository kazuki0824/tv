package com.maleicacid.tvinput.tis

import org.junit.Test

class PlaybackResourceCleanupTest {
    @Test fun failedReleaseRetainsResourceAndOtherReleasesStillRunBeforeRetry() {
        val cleanup = PlaybackResourceCleanup()
        val calls = mutableListOf<String>()
        var fail = true
        cleanup.release("decoder") {
            calls += "decoder"
            if (fail) error("失敗")
        }
        cleanup.release("filter") { calls += "filter" }
        check(calls == listOf("decoder", "filter"))
        check(cleanup.hasPending)
        val failure = runCatching { cleanup.requireComplete() }.exceptionOrNull()
        check(failure is IllegalStateException)
        check(failure.suppressed.single().message == "失敗")
        fail = false
        cleanup.retry()
        cleanup.requireComplete()
        check(!cleanup.hasPending)
        check(calls == listOf("decoder", "filter", "decoder"))
    }

    @Test fun scanAdmissionRequiresPlaybackStopIndependentlyOfLiveSessionCount() {
        val boot = ChannelScanManager.bootEpgSyncStartDecisionForTest(0, false, false, playbackPipelineRunning = true)
        check(!boot.allowed && boot.reason == "PLAYBACK_PIPELINE_RUNNING")
        val maintenance = ChannelScanManager.backgroundMaintenanceStartDecisionForTest(0, false, false, playbackPipelineRunning = true)
        check(!maintenance.allowed && maintenance.reason == "PLAYBACK_PIPELINE_RUNNING")
        check(ChannelScanManager.bootEpgSyncStartDecisionForTest(0, false, false, playbackPipelineRunning = false).allowed)
        check(ChannelScanManager.backgroundMaintenanceStartDecisionForTest(0, false, false, playbackPipelineRunning = false).allowed)
    }
}

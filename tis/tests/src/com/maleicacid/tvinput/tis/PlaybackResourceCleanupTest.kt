package com.maleicacid.tvinput.tis

import org.junit.Test

class PlaybackResourceCleanupTest {
    @Test fun failedReleaseRetainsResourceAndOtherReleasesStillRunBeforeRetry() {
        val cleanup = ResourceCleanup()
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

    @Test fun retuneCleanupInvalidatesBeforeEveryFailureAndBlocksNextTuneUntilRetry() {
        for (failedResource in listOf("playback", "filter", "CAS", "caption")) {
            var accepted = true
            var current: Long? = 1L
            var newTunes = 0
            var reject = true
            val calls = mutableListOf<String>()
            val owned = linkedSetOf("playback", "filter", "CAS", "caption")
            fun reset() = TunerController.completeRetuneReset(
                invalidate = { accepted = false; current = null },
                *listOf("playback", "filter", "CAS", "caption").map { resource -> {
                    check(!accepted && current == null)
                    check(TunerController.updateCasIfCurrent(1, 1, accepted) { error("stale section/CAS accepted") } == null)
                    calls += resource
                    if (resource in owned) {
                        if (reject && resource == failedResource) error(resource)
                        owned.remove(resource)
                    }
                    Unit
                } }.toTypedArray(),
            )
            check(runCatching { reset(); newTunes++ }.isFailure)
            check(!accepted && current == null && newTunes == 0)
            check(calls == listOf("playback", "filter", "CAS", "caption"))
            check(owned == setOf(failedResource))
            reject = false
            reset()
            newTunes++
            check(owned.isEmpty() && newTunes == 1)
        }
    }

    @Test fun initialFilterFailureNeverCommitsAndRetainsRollbackFailure() {
        var accepted = false
        var committed = 0
        var rollbackCalls = 0
        var filterOwned = true
        var rejectClose = true
        fun initialize() = TunerController.completeTuneInitialization(
            prepare = { check(!accepted); error("initial filter start failed") },
            commit = { accepted = true; committed++ },
            rollback = {
                rollbackCalls++
                TunerController.completeRetuneReset(
                    invalidate = { accepted = false },
                    { if (rejectClose) error("filter rollback close failed") else filterOwned = false },
                )
            },
        )
        val failure = runCatching { initialize() }.exceptionOrNull()
        check(failure?.message == "initial filter start failed")
        check(requireNotNull(failure).suppressed.single().message == "filter rollback close failed")
        check(!accepted && committed == 0 && rollbackCalls == 1 && filterOwned)
        rejectClose = false
        check(runCatching { initialize() }.isFailure)
        check(!filterOwned && !accepted && committed == 0 && rollbackCalls == 2)
        TunerController.completeTuneInitialization({}, { accepted = true; committed++ }, { error("unexpected rollback") })
        check(accepted && committed == 1)
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

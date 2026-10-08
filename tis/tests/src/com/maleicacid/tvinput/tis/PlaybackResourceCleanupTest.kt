// テストの入力・期待値を本体の定数と独立した具体値で記述する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import org.junit.Test
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

// callback失敗通知とstop時の所有解放を同じ資源寿命の契約群で検証する。
@Suppress("TooManyFunctions")
class PlaybackResourceCleanupTest {
    @Test
    fun currentOutputReleaseFailureNotifiesSessionAndCompletesDataTask() {
        val cleanup = ResourceCleanup()
        val notifications = mutableListOf<PlaybackPipeline.PlaybackUnavailable>()
        val executor = LifecycleSerialExecutor("output失敗試験")
        val completed = CountDownLatch(1)
        var reject = true
        var owned = true
        try {
            executor.executeData {
                PlaybackPipeline.completeCurrentDecoderOutputAction(
                    onFailure = { error ->
                        check(error is IllegalStateException)
                        PlaybackPipeline.completePlaybackFailureAction(7L, notifications::add) {
                            cleanup.requireComplete()
                        }
                    },
                ) {
                    check(
                        PlaybackPipeline.releaseDecoderOutput(cleanup) {
                            if (reject) error("output解放失敗")
                            owned = false
                        },
                    ) { "解放未完了" }
                }
                completed.countDown()
            }
            check(completed.await(1, TimeUnit.SECONDS))
            check(owned && cleanup.hasPending)
            check(notifications.single().generation == 7L)
            check(notifications.single().reason == PlaybackPipeline.PlaybackUnavailableReason.PLAYBACK_RECOVERY_FAILED)
            reject = false
            cleanup.retry()
            cleanup.requireComplete()
            check(!owned)
        } finally {
            executor.shutdownNow()
        }
    }

    @Test
    fun currentDecoderOutputReleaseFailuresRemainOwnedAndRejectCompletion() {
        val cleanup = ResourceCleanup()
        var owned = true
        var reject = true
        var calls = 0
        val release = {
            calls++
            if (reject) error("current output解放失敗")
            owned = false
        }
        val failure =
            runCatching {
                check(PlaybackPipeline.releaseDecoderOutput(cleanup, release)) { "解放未完了" }
            }.exceptionOrNull()
        check(failure is IllegalStateException && owned && cleanup.hasPending && calls == 1)
        reject = false
        cleanup.retry()
        cleanup.requireComplete()
        check(!owned && calls == 2)
        check(PlaybackPipeline.releaseDecoderOutput(cleanup) { calls++ })
        check(calls == 3 && !cleanup.hasPending)
    }

    @Test
    fun staleDecoderOutputReleaseRetainsCallbackOwnershipUntilStopRetry() {
        val cleanup = ResourceCleanup()
        var owned = true
        var reject = true
        var calls = 0
        PlaybackPipeline.releaseDecoderOutput(cleanup) {
            calls++
            if (reject) error("stale output release失敗")
            owned = false
        }
        check(owned && cleanup.hasPending && calls == 1)
        check(runCatching { cleanup.requireComplete() }.isFailure)
        reject = false
        cleanup.retry()
        cleanup.requireComplete()
        check(!owned && !cleanup.hasPending && calls == 2)
    }

    @Test fun codecRecoveryIsBoundedAndReclaimedAlwaysTerminates() {
        check(PlaybackPipeline.codecRecoveryDelay(false, true, false, false) == 0L)
        check(PlaybackPipeline.codecRecoveryDelay(false, false, true, false) == 100L)
        check(PlaybackPipeline.codecRecoveryDelay(false, false, false, false) == null)
        for (recoverable in listOf(false, true)) {
            for (transient in listOf(false, true)) {
                check(PlaybackPipeline.codecRecoveryDelay(true, recoverable, transient, false) == null)
                check(PlaybackPipeline.codecRecoveryDelay(false, recoverable, transient, true) == null)
            }
        }
    }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    @Test
    fun audioSinkAttachAndListenerFailureNeverCommitAndTransferCleanupOwnership() {
        for (failure in listOf("volume", "attach", "listener")) {
            val calls = mutableListOf<String>()
            var published = false
            var cleanupOwned = false
            val error =
                runCatching {
                    PlaybackPipeline.preparePlaybackResource(
                        prepare = {
                            for (step in listOf("volume", "attach", "listener")) {
                                calls += step
                                if (step == failure) error(step)
                            }
                        },
                        commit = { published = true },
                        rollback = { cleanupOwned = true },
                    )
                }.exceptionOrNull()
            check(error?.message == failure)
            check(!published && cleanupOwned)
            check(calls.last() == failure)
        }
        val calls = mutableListOf<String>()
        PlaybackPipeline.preparePlaybackResource({ calls += "attached" }, { calls += "committed" }, { error("unexpected rollback") })
        check(calls == listOf("attached", "committed"))
    }

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

        val owned = linkedMapOf(7 to "codec-output")
        val ownedCleanup = ResourceCleanup()
        var rejectOwnedRelease = true
        val releaseOwned = {
            val value = requireNotNull(owned[7])
            ownedCleanup.release(value) {
                if (rejectOwnedRelease) error("codec出力のreleaseに失敗しました")
                owned.remove(7)
            }
        }
        releaseOwned()
        check(owned[7] == "codec-output" && ownedCleanup.hasPending)
        rejectOwnedRelease = false
        ownedCleanup.retry()
        ownedCleanup.requireComplete()
        check(owned.isEmpty())
    }

    @Test fun retuneCleanupInvalidatesBeforeEveryFailureAndBlocksNextTuneUntilRetry() {
        for (failedResource in listOf("playback", "filter", "CAS", "caption")) {
            var accepted = true
            var current: Long? = 1L
            var newTunes = 0
            var reject = true
            val calls = mutableListOf<String>()
            val owned = linkedSetOf("playback", "filter", "CAS", "caption")

            // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
            // 動的な引数列を既存の可変長APIへ渡すため、一時配列のコピーを許容する。
            @Suppress("MaxLineLength", "SpreadOperator")
            fun reset() =
                TunerController.completeRetuneReset(
                    invalidate = {
                        accepted = false
                        current = null
                    },
                    *listOf("playback", "filter", "CAS", "caption")
                        .map { resource ->
                            {
                                check(!accepted && current == null)
                                check(TunerController.updateCasIfCurrent(1, 1, accepted) { error("stale section/CAS accepted") } == null)
                                calls += resource
                                if (resource in owned) {
                                    if (reject && resource == failedResource) error(resource)
                                    owned.remove(resource)
                                }
                                Unit
                            }
                        }.toTypedArray(),
                )
            check(
                runCatching {
                    reset()
                    newTunes++
                }.isFailure,
            )
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

        fun initialize() =
            TunerController.completeTuneInitialization(
                prepare = {
                    check(!accepted)
                    error("initial filter start failed")
                },
                commit = {
                    accepted = true
                    committed++
                },
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
        TunerController.completeTuneInitialization({}, {
            accepted = true
            committed++
        }, { error("unexpected rollback") })
        check(accepted && committed == 1)
    }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    @Test
    fun scanAdmissionRequiresPlaybackStopIndependentlyOfLiveSessionCount() {
        val boot = ChannelScanManager.bootEpgSyncStartDecisionForTest(0, false, false, playbackPipelineRunning = true)
        check(!boot.allowed && boot.reason == "PLAYBACK_PIPELINE_RUNNING")
        val maintenance = ChannelScanManager.backgroundMaintenanceStartDecisionForTest(0, false, false, playbackPipelineRunning = true)
        check(!maintenance.allowed && maintenance.reason == "PLAYBACK_PIPELINE_RUNNING")
        check(ChannelScanManager.bootEpgSyncStartDecisionForTest(0, false, false, playbackPipelineRunning = false).allowed)
        check(ChannelScanManager.backgroundMaintenanceStartDecisionForTest(0, false, false, playbackPipelineRunning = false).allowed)
    }
}

package com.maleicacid.tvinput.tis

import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.TsPid
import org.junit.Test

class PlaybackStartGateTest {
    private val key = ServiceKey(originalNetworkId = 4, transportStreamId = 0x4010, serviceId = 101)

    @Test fun requestedStartAndSwitchResultsCommitBeforeFailureNotification() {
        val signature = signature(TsPid(0x101), TsPid(0x102))
        for (next in listOf(
            PlaybackStartState.Failed(signature, null),
            PlaybackStartState.Failed(signature, 9L),
            PlaybackStartState.WaitingFirstOutput(signature, 9L),
            PlaybackStartState.Started(signature, 9L),
        )) {
            var state: PlaybackStartState = PlaybackStartState.Starting(signature)
            val calls = mutableListOf<String>()
            MaleicacidLiveSession.commitPlaybackStartResult(next,
                accept = { state = it; calls += "commit" },
                notifyUnavailable = {
                    check(state == next && state is PlaybackStartState.Failed)
                    check(it == android.media.tv.TvInputManager.VIDEO_UNAVAILABLE_REASON_UNKNOWN)
                    calls += "notify"
                },
            )
            check(state == next)
            check(calls == if (next is PlaybackStartState.Failed) listOf("commit", "notify") else listOf("commit"))
            if (PlaybackStartTransitions.pipelineGeneration(next) == 9L) {
                check(PlaybackStartTransitions.acceptsUnavailable(next, 9L) == (next !is PlaybackStartState.Failed))
                check(!PlaybackStartTransitions.acceptsUnavailable(next, 7L))
            }
        }
    }

    @Test fun failedRestartResultCommitsSessionBeforeNotifyingUnavailable() {
        for (waiting in listOf(false, true)) for (audioOnly in listOf(false, true)) {
            val signature = signature(if (audioOnly) null else TsPid(0x101), TsPid(0x102))
            var state: PlaybackStartState = if (waiting) PlaybackStartState.WaitingFirstOutput(signature, 7L)
                else PlaybackStartState.Started(signature, 7L)
            val calls = mutableListOf<String>()
            val failure = PlaybackPipeline.StartResult.failedAfterRestart(9L,
                listOf(if (audioOnly) "audio filter start failed" else "video filter start failed"))
            // filter開始時の先行通知は旧Session世代には届かない。
            check(!PlaybackStartTransitions.acceptsGeneration(state, failure.generation))
            MaleicacidLiveSession.acceptPlaybackGenerationRestart(
                state, PlaybackPipeline.PlaybackGenerationRestart(7L, failure),
                accept = { state = it; calls += "commit" },
                notifyUnavailable = {
                    check(state == PlaybackStartState.Failed(signature, 9L))
                    check(it == android.media.tv.TvInputManager.VIDEO_UNAVAILABLE_REASON_UNKNOWN)
                    calls += "notify"
                },
            )
            check(calls == listOf("commit", "notify"))
            MaleicacidLiveSession.acceptPlaybackGenerationRestart(
                state, PlaybackPipeline.PlaybackGenerationRestart(7L, failure),
                accept = { error("stale restart accepted") },
                notifyUnavailable = { error("stale restart notified") },
            )
            val success = PlaybackPipeline.StartResult(!audioOnly, audioOnly,
                firstFramePending = !audioOnly, generation = 11L)
            MaleicacidLiveSession.acceptPlaybackGenerationRestart(
                state, PlaybackPipeline.PlaybackGenerationRestart(9L, success),
                accept = { state = it },
                notifyUnavailable = { error("successful restart notified unavailable") },
            )
            check(PlaybackStartTransitions.acceptsGeneration(state, 11L))
            check(state !is PlaybackStartState.Failed)
        }
    }

    @Test fun terminalCodecErrorsRetainCleanupAndNotifyOriginalGeneration() {
        val signature = signature(TsPid(0x101), TsPid(0x102))
        for ((reclaimed, recoverable, attempted) in listOf(
            Triple(false, false, false), Triple(true, false, false), Triple(false, true, true),
        )) for (waiting in listOf(false, true)) for (isAudio in listOf(false, true)) {
            check(PlaybackPipeline.codecRecoveryDelay(reclaimed, recoverable, false, attempted) == null)
            val cleanup = ResourceCleanup()
            var owned = true
            var reject = true
            var generation = 7L
            var newPlayback = 0
            var notifications = 0
            var state: PlaybackStartState = if (waiting) PlaybackStartState.WaitingFirstOutput(signature, 7L)
                else PlaybackStartState.Started(signature, 7L)
            val execution = runCatching {
                PlaybackPipeline.completePlaybackFailureAction(7L, onUnavailable = {
                    check(it.reason == PlaybackPipeline.PlaybackUnavailableReason.PLAYBACK_RECOVERY_FAILED)
                    check(it.generation == 7L && generation != it.generation)
                    check(PlaybackStartTransitions.acceptsGeneration(state, it.generation))
                    state = PlaybackStartTransitions.failCurrentGeneration(state, it.generation)
                    notifications++
                }) {
                    // 音声縮退のstart内、または映像終端のstop内での解放失敗。
                    generation++
                    cleanup.release("filter") { if (reject) error("filter close") else owned = false }
                    cleanup.requireComplete()
                    if (isAudio) newPlayback++
                }
            }
            check(execution.isSuccess && newPlayback == 0 && notifications == 1)
            check(state == PlaybackStartState.Failed(signature, 7L))
            check(owned && cleanup.hasPending)
            reject = false
            cleanup.retry()
            cleanup.requireComplete()
            check(!owned && !cleanup.hasPending)
        }
    }

    @Test fun codecRecoveryCleanupFailureNotifiesOriginalSessionAndRetainsResources() {
        val signature = signature(TsPid(0x101), TsPid(0x102))
        for (waiting in listOf(false, true)) for (failedResource in listOf("filter", "decoder", "MediaSync")) {
            val cleanup = ResourceCleanup()
            val owned = linkedSetOf("filter", "decoder", "MediaSync")
            var rejectRelease = true
            var generation = 7L
            var contextPresent = true
            var scheduled = 0
            var restarted = 0
            var notifications = 0
            var state: PlaybackStartState = if (waiting) PlaybackStartState.WaitingFirstOutput(signature, 7L)
                else PlaybackStartState.Started(signature, 7L)
            val result = runCatching {
                PlaybackPipeline.recoverCodecGeneration(
                    originGeneration = 7L,
                    stop = {
                        generation++
                        contextPresent = false
                        for (resource in owned.toList()) cleanup.release(resource) {
                            if (resource == failedResource && rejectRelease) error(resource)
                            owned.remove(resource)
                        }
                        cleanup.requireComplete()
                        generation
                    },
                    schedule = { scheduled++; true },
                    isCurrent = { it == generation },
                    restart = { restarted++ },
                    onUnavailable = {
                        notifications++
                        check(it.reason == PlaybackPipeline.PlaybackUnavailableReason.PLAYBACK_RECOVERY_FAILED)
                        check(it.generation == 7L && it.generation != generation)
                        check(PlaybackStartTransitions.acceptsGeneration(state, it.generation))
                        state = PlaybackStartTransitions.failCurrentGeneration(state, it.generation)
                    },
                )
            }
            check(result.isSuccess && !contextPresent)
            check(scheduled == 0 && restarted == 0 && notifications == 1)
            check(state == PlaybackStartState.Failed(signature, 7L))
            check(cleanup.hasPending && owned == setOf(failedResource))
            rejectRelease = false
            cleanup.retry()
            cleanup.requireComplete()
            check(owned.isEmpty() && !cleanup.hasPending)
        }
    }

    @Test fun codecRecoveryReservationAndDeferredFailureUseOriginAndStaleRetryIsRejected() {
        for (scenario in listOf("reservation failure", "restart failure", "stale", "success")) {
            var generation = 7L
            var pending: (() -> Unit)? = null
            var restarted = 0
            val failures = mutableListOf<PlaybackPipeline.PlaybackUnavailable>()
            PlaybackPipeline.recoverCodecGeneration(
                originGeneration = 7L,
                stop = { ++generation },
                schedule = { pending = it; scenario != "reservation failure" },
                isCurrent = { it == generation },
                restart = { restarted++; generation++; if (scenario == "restart failure") error("restart cleanup") },
                onUnavailable = { failures += it },
            )
            if (scenario == "stale") generation++
            if (scenario != "reservation failure") check(runCatching { requireNotNull(pending).invoke() }.isSuccess)
            when (scenario) {
                "reservation failure", "restart failure" -> {
                    check(failures.single().generation == 7L)
                    check(failures.single().reason == PlaybackPipeline.PlaybackUnavailableReason.PLAYBACK_RECOVERY_FAILED)
                    check(restarted == if (scenario == "restart failure") 1 else 0)
                }
                "stale" -> check(restarted == 0 && failures.isEmpty())
                "success" -> check(restarted == 1 && failures.isEmpty())
            }
        }
    }

    @Test fun samePidCodecConfigurationChangesRestartButDiagnosticsDoNot() {
        val video = com.maleicacid.tvinput.aribsi.AribElementaryStream(TsPid(0x101), 0x1b, null, null, null,
            codec = "AVC", codecFacts = com.maleicacid.tvinput.aribsi.AribCodecFacts(
                avc = com.maleicacid.tvinput.aribsi.AribAvcSignaling(100, 0, 40)))
        val audio = video.copy(elementaryPid = TsPid(0x102), streamType = 0x0f, codec = "AAC",
            codecFacts = com.maleicacid.tvinput.aribsi.AribCodecFacts(audioConfigHex = "1190"))
        val original = signature(video.elementaryPid, audio.elementaryPid).copy(
            videoConfiguration = DecoderConfigurationIdentity.from(video), audioConfiguration = DecoderConfigurationIdentity.from(audio))
        val state = PlaybackStartState.Started(original, 7L)
        val videoChanged = original.copy(videoConfiguration = DecoderConfigurationIdentity.from(video.copy(
            codecFacts = video.codecFacts.copy(avc = com.maleicacid.tvinput.aribsi.AribAvcSignaling(100, 0, 41)))))
        val audioChanged = original.copy(audioConfiguration = DecoderConfigurationIdentity.from(audio.copy(
            codecFacts = audio.codecFacts.copy(audioConfigHex = "1210"))))
        for (changed in listOf(videoChanged, audioChanged)) {
            check(PlaybackStartTransitions.shouldAttempt(state, changed))
            val restarted = PlaybackStartTransitions.afterSuccessfulRestart(changed, 8L, true)
            check(PlaybackStartTransitions.acceptsGeneration(restarted, 8L))
            check(!PlaybackStartTransitions.shouldAttempt(restarted, changed))
        }
        val diagnosticOnly = original.copy(videoConfiguration = DecoderConfigurationIdentity.from(video.copy(
            codecFacts = video.codecFacts.copy(rawDescriptorsHex = "changed", profileLevel = "diagnostic only"))))
        check(!PlaybackStartTransitions.shouldAttempt(state, diagnosticOnly))
    }

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

    @Test fun audioSwitchUsesRestartedGenerationAndWaitsForNewFirstOutput() {
        val signature = signature(videoPid = TsPid(0x0101), audioPid = TsPid(0x0202))
        val state = PlaybackStartTransitions.afterSuccessfulRestart(
            signature,
            pipelineGeneration = 9L,
            firstOutputPending = true,
        )

        check(state == PlaybackStartState.WaitingFirstOutput(signature, pipelineGeneration = 9L))
    }

    @Test fun failedAudioSwitchDoesNotKeepOldStartedGeneration() {
        val oldSignature = signature(videoPid = TsPid(0x0101), audioPid = TsPid(0x0102))
        val newSignature = signature(videoPid = TsPid(0x0101), audioPid = TsPid(0x0202))
        val oldState = PlaybackStartState.Started(oldSignature, pipelineGeneration = 7L)

        check(
            PlaybackStartTransitions.afterRestartResult(
                oldState,
                newSignature,
                pipelineGeneration = 9L,
                firstOutputPending = true,
                started = false,
            ) == PlaybackStartState.Failed(newSignature, pipelineGeneration = 9L),
        )
        check(
            PlaybackStartTransitions.afterRestartResult(
                oldState,
                newSignature,
                pipelineGeneration = -1L,
                firstOutputPending = false,
                started = false,
            ) == oldState,
        ) {
            "restart前の入力拒否ではcurrent playback stateを変更してはなりません"
        }
    }

    @Test fun everyFailureAfterRestartCarriesTheIssuedGeneration() {
        val generation = 9L
        listOf(
            "unsupported-video",
            "unsupported-audio-only",
            "invalid-surface",
            "media-sync-init",
            "video-filter-start",
            "audio-only-filter-start",
        ).forEach { failure ->
            val result = PlaybackPipeline.StartResult.failedAfterRestart(generation, listOf(failure))
            check(result.generation == generation)
            check(!result.startedVideo && !result.startedAudio)
        }
    }

    @Test fun audioOnlyFatalFailureStopsAdvertisingStartedGeneration() {
        val signature = signature(videoPid = null, audioPid = TsPid(0x0102))
        val started = PlaybackStartState.Started(signature, pipelineGeneration = 7L)

        check(
            PlaybackStartTransitions.failCurrentGeneration(started, failedGeneration = 7L) ==
                PlaybackStartState.Failed(signature, pipelineGeneration = 7L),
        )
        check(PlaybackStartTransitions.acceptsGeneration(started, generation = 7L))
        check(!PlaybackStartTransitions.acceptsGeneration(started, generation = 6L)) {
            "旧generationの失敗通知ではstate・caption・外部通知を変更してはなりません"
        }
        check(PlaybackStartTransitions.failCurrentGeneration(started, failedGeneration = 6L) == started) {
            "旧generationの失敗通知でcurrent playback stateを変更してはなりません"
        }
    }

    private fun signature(videoPid: TsPid?, audioPid: TsPid?): AvPlaybackSignature = AvPlaybackSignature(
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

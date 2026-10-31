// テストの入力・期待値を本体の定数と独立した具体値で記述する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.TsPid
import org.junit.Test

// 一つの契約の試験集合・時系列を保持し、検証シナリオを分断しない。
@Suppress("TooManyFunctions")
class PlaybackStartGateTest {
    private val key = ServiceKey(originalNetworkId = 4, transportStreamId = 0x4010, serviceId = 101)

    @Test
    fun temporaryCasLinkageLossIsBufferingNotCasFailure() {
        val pending =
            com.maleicacid.tvinput.aribsi.ServicePolicyDecision(
                serviceKey = key,
                registrationReady = true,
                requiresCas = true,
                caDescriptorsResolved = false,
                reasons = listOf("PMT"),
            )
        val resolved = pending.copy(caDescriptorsResolved = true, reasons = emptyList())
        val buffering = android.media.tv.TvInputManager.VIDEO_UNAVAILABLE_REASON_BUFFERING
        val terminal = android.media.tv.TvInputManager.VIDEO_UNAVAILABLE_REASON_CAS_UNKNOWN
        check(MaleicacidLiveSession.casGateUnavailableReason(pending, false, false) == buffering)
        check(MaleicacidLiveSession.casGateUnavailableReason(resolved, false, false) == buffering)
        check(MaleicacidLiveSession.casGateUnavailableReason(resolved, true, false) == null)
        check(MaleicacidLiveSession.casGateUnavailableReason(resolved, true, true) == terminal)
        val clear = resolved.copy(requiresCas = false)
        check(MaleicacidLiveSession.casGateUnavailableReason(clear, false, false) == null)
    }

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
            MaleicacidLiveSession.commitPlaybackStartResult(
                next,
                accept = {
                    state = it
                    calls += "commit"
                },
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
        for (waiting in listOf(false, true)) {
            for (audioOnly in listOf(false, true)) {
                val signature = signature(if (audioOnly) null else TsPid(0x101), TsPid(0x102))
                var state: PlaybackStartState =
                    if (waiting) {
                        PlaybackStartState.WaitingFirstOutput(signature, 7L)
                    } else {
                        PlaybackStartState.Started(signature, 7L)
                    }
                val calls = mutableListOf<String>()
                val failure =
                    PlaybackPipeline.StartResult.failedAfterRestart(
                        9L,
                        listOf(if (audioOnly) "audio filter start failed" else "video filter start failed"),
                    )
                // filter開始時の先行通知は旧Session世代には届かない。
                check(!PlaybackStartTransitions.acceptsGeneration(state, failure.generation))
                MaleicacidLiveSession.acceptPlaybackGenerationRestart(
                    state,
                    PlaybackPipeline.PlaybackGenerationRestart(7L, failure),
                    accept = {
                        state = it
                        calls += "commit"
                    },
                    notifyUnavailable = {
                        check(state == PlaybackStartState.Failed(signature, 9L))
                        check(it == android.media.tv.TvInputManager.VIDEO_UNAVAILABLE_REASON_UNKNOWN)
                        calls += "notify"
                    },
                )
                check(calls == listOf("commit", "notify"))
                MaleicacidLiveSession.acceptPlaybackGenerationRestart(
                    state,
                    PlaybackPipeline.PlaybackGenerationRestart(7L, failure),
                    accept = { error("stale restart accepted") },
                    notifyUnavailable = { error("stale restart notified") },
                )
                val success =
                    PlaybackPipeline.StartResult(
                        !audioOnly,
                        audioOnly,
                        firstFramePending = !audioOnly,
                        generation = 11L,
                    )
                MaleicacidLiveSession.acceptPlaybackGenerationRestart(
                    state,
                    PlaybackPipeline.PlaybackGenerationRestart(9L, success),
                    accept = { state = it },
                    notifyUnavailable = { error("successful restart notified unavailable") },
                )
                check(PlaybackStartTransitions.acceptsGeneration(state, 11L))
                check(state !is PlaybackStartState.Failed)
            }
        }
    }

    // 候補の処理と入れ子の資源寿命を同じ手順内で確認できる構造を保つ。
    @Suppress("NestedBlockDepth")
    @Test
    fun terminalCodecErrorsRetainCleanupAndNotifyOriginalGeneration() {
        val signature = signature(TsPid(0x101), TsPid(0x102))
        for (waiting in listOf(false, true)) {
            for (isAudio in listOf(false, true)) {
                val cleanup = ResourceCleanup()
                var owned = true
                var reject = true
                var generation = 7L
                var newPlayback = 0
                var notifications = 0
                var state: PlaybackStartState =
                    if (waiting) {
                        PlaybackStartState.WaitingFirstOutput(signature, 7L)
                    } else {
                        PlaybackStartState.Started(signature, 7L)
                    }
                val execution =
                    runCatching {
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
    }

    // 一つの契約の試験集合・時系列を保持し、検証シナリオを分断しない。
    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("LongMethod", "MaxLineLength")
    @Test
    fun samePidCodecConfigurationChangesRestartButDiagnosticsDoNot() {
        val video =
            com.maleicacid.tvinput.aribsi.AribElementaryStream(
                TsPid(0x101),
                0x1b,
                null,
                null,
                null,
                codec = "AVC",
                codecFacts =
                    com.maleicacid.tvinput.aribsi
                        .AribCodecFacts(
                            avc =
                                com.maleicacid.tvinput.aribsi
                                    .AribAvcSignaling(100, 0, 40),
                        ),
            )
        val audio =
            video.copy(
                elementaryPid = TsPid(0x102),
                streamType = 0x0f,
                codec = "AAC",
                codecFacts =
                    com.maleicacid.tvinput.aribsi
                        .AribCodecFacts(audioConfigHex = "1190"),
            )
        val original =
            signature(
                video.elementaryPid,
                audio.elementaryPid,
            ).copy(
                videoConfiguration = DecoderConfigurationIdentity.from(video),
                audioConfiguration = DecoderConfigurationIdentity.from(audio),
            )
        val state = PlaybackStartState.Started(original, 7L)
        val videoChanged =
            original.copy(
                videoConfiguration =
                    DecoderConfigurationIdentity.from(
                        video.copy(
                            codecFacts =
                                video.codecFacts.copy(
                                    avc =
                                        com.maleicacid.tvinput.aribsi
                                            .AribAvcSignaling(100, 0, 41),
                                ),
                        ),
                    ),
            )
        val audioChanged =
            original.copy(
                audioConfiguration =
                    DecoderConfigurationIdentity.from(
                        audio.copy(codecFacts = audio.codecFacts.copy(audioConfigHex = "1210")),
                    ),
            )
        for (changed in listOf(videoChanged, audioChanged)) {
            check(PlaybackStartTransitions.shouldAttempt(state, changed))
            val restarted = PlaybackStartTransitions.afterSuccessfulRestart(changed, 8L, true)
            check(PlaybackStartTransitions.acceptsGeneration(restarted, 8L))
            check(!PlaybackStartTransitions.shouldAttempt(restarted, changed))
        }
        val diagnosticOnly =
            original.copy(
                videoConfiguration =
                    DecoderConfigurationIdentity.from(
                        video.copy(codecFacts = video.codecFacts.copy(rawDescriptorsHex = "changed", profileLevel = "diagnostic only")),
                    ),
            )
        check(!PlaybackStartTransitions.shouldAttempt(state, diagnosticOnly))
    }

    @Test
    fun retriesOnlyRecoverablePlaybackFailures() {
        val allowed =
            setOf(
                PlaybackPipeline.PlaybackUnavailableReason.CODEC_CONFIG_TIMEOUT,
                PlaybackPipeline.PlaybackUnavailableReason.FIRST_FRAME_TIMEOUT,
                PlaybackPipeline.PlaybackUnavailableReason.VIDEO_OUTPUT_RENDER_FAILED,
                PlaybackPipeline.PlaybackUnavailableReason.RECOVERABLE_CODEC_ERROR,
                PlaybackPipeline.PlaybackUnavailableReason.COMPRESSED_INPUT_LOSS,
                PlaybackPipeline.PlaybackUnavailableReason.AUDIO_FALLBACK_REQUIRED,
            )
        PlaybackPipeline.PlaybackUnavailableReason.entries.forEach { reason ->
            check(MaleicacidLiveSession.isRecoverablePlaybackFailure(reason) == (reason in allowed))
        }
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
        val state =
            PlaybackStartTransitions.afterSuccessfulRestart(
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

    @Test
    fun frameworkControlCallbackSurvivesSaturatedDataQueue() {
        val executor = LifecycleSerialExecutor("framework-tif-control-test", maxPendingDataTasks = 1)
        val started = java.util.concurrent.CountDownLatch(1)
        val unblock = java.util.concurrent.CountDownLatch(1)
        val controlCompleted = java.util.concurrent.CountDownLatch(1)
        val dataCompleted = java.util.concurrent.CountDownLatch(1)
        val order = java.util.concurrent.CopyOnWriteArrayList<String>()
        try {
            executor.executeControl {
                started.countDown()
                check(unblock.await(5, java.util.concurrent.TimeUnit.SECONDS))
            }
            check(started.await(5, java.util.concurrent.TimeUnit.SECONDS))
            // data受付枠を使い切っても、ユーザー操作のcontrol callbackは受理可能。
            executor.executeData {
                order.add("data")
                dataCompleted.countDown()
            }
            executor.executeCallback(
                control = true,
                isReleased = { false },
                onFailure = { throw it },
            ) {
                order.add("control")
                controlCompleted.countDown()
            }
            unblock.countDown()
            check(controlCompleted.await(5, java.util.concurrent.TimeUnit.SECONDS))
            check(dataCompleted.await(5, java.util.concurrent.TimeUnit.SECONDS))
            check(order == listOf("control", "data"))
        } finally {
            unblock.countDown()
            executor.shutdownNow()
        }
    }

    private fun signature(
        videoPid: TsPid?,
        audioPid: TsPid?,
    ): AvPlaybackSignature =
        AvPlaybackSignature(
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

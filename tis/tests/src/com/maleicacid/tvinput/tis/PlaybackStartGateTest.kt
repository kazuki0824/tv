package com.maleicacid.tvinput.tis

import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.TsPid
import org.junit.Test

class PlaybackStartGateTest {
    private val key = ServiceKey(originalNetworkId = 4, transportStreamId = 0x4010, serviceId = 101)

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

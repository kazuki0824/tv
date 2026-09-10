package com.maleicacid.tvinput.tis

import android.media.MediaSync
import android.media.tv.tuner.Tuner
import com.maleicacid.tvinput.aribsi.AribElementaryStream
import com.maleicacid.tvinput.common.FrequencyHz
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.StreamSelector
import com.maleicacid.tvinput.common.TsPid
import java.util.concurrent.atomic.AtomicBoolean
import org.junit.Test
import sun.misc.Unsafe

class PlaybackFailureCallbacksTest {
    @Test fun mediaSyncAudioFailureRetainsResourcesAndNotifiesOriginalSession() {
        for (waiting in listOf(false, true)) for (audioOnly in listOf(false, true)) {
            val fixture = Fixture(waiting, audioOnly)
            val sync = MediaSync()
            fixture.set("mediaSync", sync)
            fixture.invoke("handleMediaSyncError", sync, 7L, MediaSync.MEDIASYNC_ERROR_AUDIOTRACK_FAIL, 0)
            fixture.checkTerminalCleanupFailure()
        }
    }

    @Test fun audioFailureAndRouteOrFormatRestartShareTerminalCleanupHandling() {
        for (waiting in listOf(false, true)) {
            for (reason in listOf(PlaybackPipeline.PlaybackUnavailableReason.AUDIO_UNAVAILABLE,
                PlaybackPipeline.PlaybackUnavailableReason.UNSUPPORTED_AUDIO_STREAM)) {
                val fixture = Fixture(waiting, false)
                fixture.invoke("handleAudioFailure", reason, "audio output/configuration/deadline failure", false)
                fixture.checkTerminalCleanupFailure()
            }
            val fixture = Fixture(waiting, false)
            fixture.invoke("restartCurrentPlaybackGeneration", 7L)
            fixture.checkTerminalCleanupFailure()
        }
    }

    @Test fun requestedStartAndAudioSwitchReturnCleanupFailureToTheirCaller() {
        for (switchAudio in listOf(false, true)) {
            val fixture = Fixture(false, false)
            val generation: Long
            val diagnostics: List<String>
            if (switchAudio) {
                val result = fixture.pipeline.switchAudio(fixture.tuner, fixture.selection)
                check(!result.switchedAudio && !result.firstFramePending)
                generation = result.generation
                diagnostics = result.diagnostics
            } else {
                val result = fixture.pipeline.start(fixture.tuner, fixture.channel, fixture.selection)
                check(!result.startedVideo && !result.startedAudio && !result.firstFramePending)
                generation = result.generation
                diagnostics = result.diagnostics
            }
            check(generation == 8L && diagnostics.any { it.contains("injected filter") })
            check(fixture.failures.isEmpty() && fixture.notifications == 0)
            val next = PlaybackStartTransitions.afterRestartResult(fixture.state, fixture.signature,
                generation, firstOutputPending = false, started = false)
            MaleicacidLiveSession.commitPlaybackStartResult(next, { fixture.state = it }) {
                check(fixture.state == PlaybackStartState.Failed(fixture.signature, generation))
                fixture.notifications++
            }
            check(fixture.notifications == 1)
            fixture.checkRetainedThenReleased()
        }
    }

    @Test fun audioOnlyStopsBeforeNotifyingAndFailedVideoOnlyResultReachesSession() {
        for (audioOnly in listOf(false, true)) {
            val fixture = Fixture(false, audioOnly, failCleanup = false)
            fixture.invoke("handleAudioFailure", PlaybackPipeline.PlaybackUnavailableReason.AUDIO_UNAVAILABLE,
                "audio output failure", audioOnly)
            check(fixture.notifications == 1 && fixture.state is PlaybackStartState.Failed)
            if (audioOnly) {
                check(fixture.restarts.isEmpty())
                check(fixture.failures.single().generation == 7L)
                check(fixture.failures.single().reason == PlaybackPipeline.PlaybackUnavailableReason.AUDIO_UNAVAILABLE)
                check(fixture.pipeline.currentPlaybackGenerationForTest() == 8L)
            } else {
                val restart = fixture.restarts.single()
                check(restart.videoOnly && restart.originGeneration == 7L && restart.result.generation == 9L)
                check(!restart.result.startedVideo && !restart.result.firstFramePending)
                check(PlaybackStartTransitions.signature(fixture.state)?.audioPid == null)
                check(PlaybackStartTransitions.pipelineGeneration(fixture.state) == 9L)
            }
        }
    }

    @Test fun staleMediaSyncAndRestartCallbacksDoNotTouchCurrentGeneration() {
        val fixture = Fixture(false, false)
        val sync = MediaSync()
        fixture.set("mediaSync", sync)
        fixture.invoke("handleMediaSyncError", sync, 6L, MediaSync.MEDIASYNC_ERROR_AUDIOTRACK_FAIL, 0)
        fixture.invoke("handleMediaSyncError", MediaSync(), 7L, MediaSync.MEDIASYNC_ERROR_AUDIOTRACK_FAIL, 0)
        fixture.invoke("restartCurrentPlaybackGeneration", 6L)
        check(fixture.notifications == 0 && fixture.failures.isEmpty() && fixture.restarts.isEmpty())
        check(fixture.pipeline.currentPlaybackGenerationForTest() == 7L && fixture.releaseAttempts == 1)
        fixture.rejectRelease = false
        fixture.cleanup.retry()
        fixture.cleanup.requireComplete()
    }

    private class Fixture(waiting: Boolean, audioOnly: Boolean, failCleanup: Boolean = true) {
        // Androidのthread/native初期化だけを省く。本番の停止・開始・エラー処理を直接実行する。
        private val unsafe = Unsafe::class.java.getDeclaredField("theUnsafe").run {
            isAccessible = true
            get(null) as Unsafe
        }
        val pipeline = unsafe.allocateInstance(PlaybackPipeline::class.java) as PlaybackPipeline
        val cleanup = ResourceCleanup()
        val tuner = unsafe.allocateInstance(Tuner::class.java) as Tuner
        private val key = ServiceKey(4, 0x4010, 101)
        val channel = TunerController.ResolvedChannel(null, "test", key, if (audioOnly) 2 else 1,
            "test", "1", "ISDB_T", FrequencyHz(473_000_000L), StreamSelector.NONE, null, null)
        private val video = AribElementaryStream(TsPid(0x101), 0x1b, null, null, null)
        private val audio = AribElementaryStream(TsPid(0x102), 0x0f, null, null, null, codec = "AAC")
        val selection = TunerController.AvStreamSelection(key, TsPid(0x100), if (audioOnly) null else video, audio)
        val signature = AvPlaybackSignature(key, TsPid(0x100), selection.video?.elementaryPid,
            selection.video?.streamType, audio.elementaryPid, audio.streamType, true, false)
        var state: PlaybackStartState = if (waiting) PlaybackStartState.WaitingFirstOutput(signature, 7L)
            else PlaybackStartState.Started(signature, 7L)
        val failures = mutableListOf<PlaybackPipeline.PlaybackUnavailable>()
        val restarts = mutableListOf<PlaybackPipeline.PlaybackGenerationRestart>()
        var notifications = 0
        var releaseAttempts = 0
        var rejectRelease = failCleanup
        private var resourceOwned = failCleanup

        init {
            set("inputId", "test")
            set("playbackExecutorThread", Thread.currentThread())
            set("playbackGeneration", 7L)
            set("released", AtomicBoolean(false))
            set("videoAvailableNotified", AtomicBoolean(!waiting))
            set("resourceCleanup", cleanup)
            set("outstandingAudioOutputs", linkedMapOf<Int, Any>())
            val ptsType = field("ptsEpochCoordinator").type
            set("ptsEpochCoordinator", ptsType.getDeclaredConstructor().apply { isAccessible = true }.newInstance())
            set("activeTuner", tuner)
            set("activeChannel", channel)
            set("activeSelection", selection)
            set("audioPathExpected", true)
            set("onVideoUnavailable", { failure: PlaybackPipeline.PlaybackUnavailable ->
                failures += failure
                if (PlaybackStartTransitions.acceptsUnavailable(state, failure.generation)) {
                    state = PlaybackStartTransitions.failCurrentGeneration(state, failure.generation)
                    notifications++
                }
            })
            set("onPlaybackGenerationRestarted", { restart: PlaybackPipeline.PlaybackGenerationRestart ->
                restarts += restart
                MaleicacidLiveSession.acceptPlaybackGenerationRestart(state, restart, { state = it }) {
                    check(state is PlaybackStartState.Failed)
                    notifications++
                }
            })
            if (failCleanup) cleanup.release("injected filter") {
                releaseAttempts++
                if (rejectRelease) error("filter close rejected")
                resourceOwned = false
            }
        }

        fun checkTerminalCleanupFailure() {
            val failure = failures.single()
            check(failure.reason == PlaybackPipeline.PlaybackUnavailableReason.PLAYBACK_RECOVERY_FAILED)
            check(failure.generation == 7L && failure.detail.contains("injected filter"))
            check(pipeline.currentPlaybackGenerationForTest() == 8L)
            check(notifications == 1 && state == PlaybackStartState.Failed(signature, 7L))
            check(restarts.isEmpty())
            checkRetainedThenReleased()
        }

        fun checkRetainedThenReleased() {
            check(cleanup.hasPending && resourceOwned && releaseAttempts == 2)
            check(field("activeTuner").get(pipeline) == null && field("activeChannel").get(pipeline) == null)
            rejectRelease = false
            pipeline.stop()
            check(!cleanup.hasPending && !resourceOwned && releaseAttempts == 3)
        }

        fun set(name: String, value: Any?) { field(name).set(pipeline, value) }
        private fun field(name: String) = PlaybackPipeline::class.java.getDeclaredField(name).apply { isAccessible = true }
        fun invoke(name: String, vararg args: Any) {
            val method = PlaybackPipeline::class.java.declaredMethods.single { it.name == name }
            method.isAccessible = true
            method.invoke(pipeline, *args)
        }
    }
}

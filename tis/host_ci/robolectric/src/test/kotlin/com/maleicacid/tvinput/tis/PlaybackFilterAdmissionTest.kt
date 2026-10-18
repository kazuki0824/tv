// 実SDK callback lock・時計・MediaEvent寿命を使い、HAL/native境界だけを置換する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import android.content.Context
import android.graphics.SurfaceTexture
import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaFormat
import android.media.tv.tuner.Tuner
import android.media.tv.tuner.TunerUtils
import android.media.tv.tuner.filter.Filter
import android.media.tv.tuner.filter.FilterCallback
import android.media.tv.tuner.filter.FilterConfiguration
import android.media.tv.tuner.filter.FilterEvent
import android.media.tv.tuner.filter.MediaEvent
import android.media.tv.tuner.filter.PesEvent
import android.media.tv.tuner.filter.RestartEvent
import android.os.Looper
import android.view.Surface
import com.maleicacid.tvinput.aribsi.AribCodecFacts
import com.maleicacid.tvinput.aribsi.AribElementaryStream
import com.maleicacid.tvinput.common.FrequencyHz
import com.maleicacid.tvinput.common.LogTags
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.StreamSelector
import com.maleicacid.tvinput.common.TsPid
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows
import org.robolectric.annotation.Config
import org.robolectric.annotation.Implementation
import org.robolectric.annotation.Implements
import org.robolectric.shadow.api.Shadow
import org.robolectric.shadows.MediaCodecInfoBuilder
import org.robolectric.shadows.ShadowLog
import org.robolectric.shadows.ShadowMediaCodec
import org.robolectric.shadows.ShadowMediaCodecList
import org.robolectric.shadows.ShadowSystemClock
import org.robolectric.util.ReflectionHelpers
import java.nio.ByteBuffer
import java.time.Duration
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executor
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.test.assertContentEquals
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

@RunWith(RobolectricTestRunner::class)
@Config(
    sdk = [35],
    shadows = [
        PlaybackFilterAdmissionTest.NativeTuner::class,
        PlaybackFilterAdmissionTest.NativeFilter::class,
        PlaybackFilterAdmissionTest.NativeMediaEvent::class,
        PlaybackFilterAdmissionTest.NativeLinearBlock::class,
        PlaybackFilterAdmissionTest.NativeCodec::class,
    ],
)
// 同じproduction ownerとAndroid境界fixtureを共有する。
@Suppress("TooManyFunctions", "LargeClass")
class PlaybackFilterAdmissionTest {
    @Test
    fun avOverflowDiscardsPreFlushAndInFlightInputAndAcceptsPostFlushInput() {
        for ((audio, afterClear) in listOf(false to false, false to true, true to false, true to true)) {
            val newInput = mappedMediaEvent()
            Fixture().use { fixture ->
                fixture.decoder(audio)
                val filter = fixture.avFilter(audio)
                val decoderField = if (audio) "audioDecoder" else "videoDecoder"
                val decoder = ReflectionHelpers.getField<Any>(fixture.pipeline, decoderField)
                val native = Shadow.extract<NativeFilter>(filter)
                val pending = mappedMediaEvent()
                deliver(filter, pending)
                fixture.drain()
                assertEquals(listOf(pending), fixture.pendingEvents(audio))
                val old = mappedMediaEvent()
                val beforeExecution = mappedMediaEvent()
                val duringFlush = mappedMediaEvent()
                val (started, allow) = native.blockNextReclamation(afterClear)
                try {
                    fixture.whileOwnerBlocked {
                        deliver(filter, old)
                        status(filter, Filter.STATUS_OVERFLOW)
                        deliver(filter, beforeExecution)
                    }
                    assertTrue(started.await(5, TimeUnit.SECONDS))
                    deliver(filter, duringFlush)
                } finally {
                    allow.countDown()
                }
                fixture.drain()
                assertEquals(emptyList(), fixture.pendingEvents(audio))
                for (event in listOf(old, beforeExecution, duringFlush)) {
                    assertEquals(0, Shadow.extract<NativeMediaEvent>(event).blockReads)
                    assertEquals(1, Shadow.extract<NativeMediaEvent>(event).releases)
                }
                assertEquals(1, Shadow.extract<NativeMediaEvent>(pending).releases)
                // 回復後に新しいsampleが届くまで、さらにOVERFLOWを受けても同じflushは行わない。
                status(filter, Filter.STATUS_OVERFLOW)
                fixture.drain()
                assertEquals(1, native.flushes)
                deliver(filter, newInput)
                fixture.drain()
                assertEquals(listOf(newInput), fixture.pendingEvents(audio))
                assertEquals(1, Shadow.extract<NativeMediaEvent>(newInput).blockReads)
                assertEquals(0, Shadow.extract<NativeMediaEvent>(newInput).releases)
                assertEquals(7L, fixture.pipeline.currentPlaybackGenerationForTest())
                assertTrue(decoder === ReflectionHelpers.getField<Any>(fixture.pipeline, decoderField))
                assertEquals(0, native.closes)
                status(filter, Filter.STATUS_OVERFLOW)
                fixture.drain()
                // 新しい入力を受け取った後の別OVERFLOWは、一度だけ回復できる。
                assertEquals(2, native.flushes)
                val progress = fixture.progress()
                assertTrue(progress.count(PlaybackDataPathProgress.Stage.OVERFLOW, audio) > 0L)
                assertEquals(0L, progress.count(PlaybackDataPathProgress.Stage.CODEC_OUTPUT, audio))
                assertEquals(0L, progress.count(PlaybackDataPathProgress.Stage.FLUSH_FAILED, audio))
                assertEquals(2L, progress.count(PlaybackDataPathProgress.Stage.VALID_SAMPLE, audio))
                assertTrue(fixture.failures.isEmpty())
            }
            assertEquals(1, Shadow.extract<NativeMediaEvent>(newInput).releases)
        }
    }

    @Test
    fun failedAvFlushKeepsInputClosedUntilSuccessfulFlush() {
        for (audio in listOf(false, true)) {
            Fixture().use { fixture ->
                fixture.decoder(audio)
                val filter = fixture.avFilter(audio)
                val native = Shadow.extract<NativeFilter>(filter)
                native.flushResult = Tuner.RESULT_UNKNOWN_ERROR
                status(filter, Filter.STATUS_OVERFLOW)
                fixture.drain()
                val rejected = mappedMediaEvent()
                deliver(filter, rejected)
                fixture.drain()
                assertEquals(emptyList(), fixture.pendingEvents(audio))
                assertEquals(0, Shadow.extract<NativeMediaEvent>(rejected).blockReads)
                assertEquals(1, Shadow.extract<NativeMediaEvent>(rejected).releases)
                val progress = fixture.progress()
                assertTrue(progress.count(PlaybackDataPathProgress.Stage.FLUSH_FAILED, audio) > 0L)
                assertEquals(0L, progress.count(PlaybackDataPathProgress.Stage.FLUSH_FAILED, !audio))
                assertTrue(fixture.unavailable.any { it.detail.contains("FLUSH_FAILED=") })
                native.flushResult = Tuner.RESULT_SUCCESS
                status(filter, Filter.STATUS_OVERFLOW)
                fixture.drain()
                val accepted = mappedMediaEvent()
                deliver(filter, accepted)
                fixture.drain()
                assertEquals(listOf(accepted), fixture.pendingEvents(audio))
                assertTrue(progress.count(PlaybackDataPathProgress.Stage.FLUSH_SUCCEEDED, audio) > 0L)
            }
        }
    }

    @Test
    fun normalVideoAudioCaptionAndStatusBurstsReturnWithoutClosingPlayback() {
        Fixture().use { fixture ->
            val video = fixture.avFilter(false)
            val audio = fixture.avFilter(true)
            val caption = fixture.captionFilter()
            val events = List(130) { mediaEvent(1L) }
            fixture.whileOwnerBlocked {
                repeat(65) { index ->
                    deliver(video, events[index])
                    deliver(audio, events[index + 65])
                    deliver(caption, restartEvent())
                    status(video, Filter.STATUS_DATA_READY)
                    status(audio, Filter.STATUS_DATA_READY)
                    status(caption, Filter.STATUS_DATA_READY)
                }
                assertFalse(fixture.released())
            }
            fixture.drain()
            assertEquals(65, fixture.captionDiscontinuities)
            events.forEach {
                val native = Shadow.extract<NativeMediaEvent>(it)
                assertEquals(1, native.blockReads)
                assertEquals(1, native.releases)
                assertTrue(native.releaseThread.orEmpty().startsWith("maleicacid-playback-"))
            }
            assertEquals(7L, fixture.pipeline.currentPlaybackGenerationForTest())
            assertTrue(fixture.failures.isEmpty())
            for (filter in listOf(video, audio, caption)) assertEquals(0, Shadow.extract<NativeFilter>(filter).closes)
        }
    }

    @Test
    fun progressCallbackRunsWhileFilterDataBudgetIsFull() {
        Fixture().use { fixture ->
            val video = fixture.avFilter(false)
            val progress = CountDownLatch(1)
            val samples = listOf(4_194_304L, 4_194_304L, 4_194_304L, 4_186_112L).map(::mediaEvent)
            val releasedSamples = CountDownLatch(samples.size)
            samples.forEach { Shadow.extract<NativeMediaEvent>(it).onRelease = { releasedSamples.countDown() } }
            // 共有16,384 slotをAVC単一sample上限内の入力だけで満たす。
            fixture.whileOwnerBlocked {
                samples.forEach { deliver(video, it) }
                fixture.invoke("enqueuePlaybackAction", { progress.countDown() })
                assertFalse(fixture.released())
            }
            assertTrue(progress.await(5, TimeUnit.SECONDS))
            assertTrue(releasedSamples.await(5, TimeUnit.SECONDS))
            // 最後のdata taskのfinallyでpermitを返してからdrain用dataを受理させる。
            fixture.executor.callControl(5_000L) { Unit }
            fixture.drain()
            samples.forEach {
                val native = Shadow.extract<NativeMediaEvent>(it)
                assertEquals(1, native.blockReads)
                assertEquals(1, native.releases)
            }
            assertFalse(fixture.released())
        }
    }

    @Test
    fun transientVideoAdmissionLossRecoversAndPersistentLossUsesExistingDeadline() {
        Fixture().use { fixture ->
            fixture.decoder()
            val video = fixture.avFilter(false)
            val first = fixture.capacityLoss(video)
            assertTrue(fixture.failures.isEmpty())
            assertFalse(fixture.released())
            assertEquals(1, Shadow.extract<NativeMediaEvent>(first).releases)
            // 再開後の本番RestartEventは既存decoderの入力停滞を解除する。
            deliver(video, restartEvent())
            fixture.drain()
            ShadowSystemClock.advanceBy(Duration.ofSeconds(6))
            val recovered = mediaEvent(1L)
            deliver(video, recovered)
            fixture.drain()
            assertEquals(1, Shadow.extract<NativeMediaEvent>(recovered).blockReads)
            assertTrue(fixture.failures.isEmpty())
            val second = fixture.capacityLoss(video)
            assertTrue(fixture.failures.isEmpty())
            ShadowSystemClock.advanceBy(Duration.ofSeconds(6))
            val decoder = ReflectionHelpers.getField<Any>(fixture.pipeline, "videoDecoder")
            val accepted = mappedMediaEvent()
            val rejected = mappedMediaEvent()
            fixture.whileOwnerBlocked {
                deliver(video, accepted)
                // 1-byte eventの3 slot以外を既存permitで予約し、本番callbackの拒否を起こす。
                fixture.executor.executeCallback(
                    isReleased = { false },
                    onFailure = { throw it },
                    dataSlots = 16 * 1024 - 3,
                ) {}
                deliver(video, rejected)
            }
            fixture.executor.callControl(5_000L) { Unit }
            fixture.drain()
            assertEquals(1, fixture.failures.size)
            assertEquals(PlaybackPipeline.PlaybackUnavailableReason.VIDEO_CODEC_ERROR, fixture.failures.single().first)
            assertTrue(
                fixture.failures
                    .single()
                    .second
                    .contains("OWNER_INPUT_FULL"),
            )
            assertFalse(fixture.released())
            fixture.assertStoppedVideo(decoder, video, listOf(accepted, rejected))
            for (event in listOf(first, second)) {
                assertEquals(1, Shadow.extract<NativeMediaEvent>(event).releases)
            }
        }
    }

    @Test
    fun decoderQueueDeadlineStopsFilterAndRejectsFurtherInput() {
        Fixture().use { fixture ->
            fixture.decoder()
            val video = fixture.avFilter(false)
            val decoder = ReflectionHelpers.getField<Any>(fixture.pipeline, "videoDecoder")
            // AVC startup budgetの32件を満たし、次の入力で既存decoderの停滞時計を開始する。
            val samples = List(33) { mappedMediaEvent() }
            samples.forEach { deliver(video, it) }
            fixture.drain()
            assertEquals(32, fixture.pendingEvents(false).size)
            assertTrue(fixture.failures.isEmpty())
            ShadowSystemClock.advanceBy(Duration.ofSeconds(6))
            val expired = mappedMediaEvent()
            deliver(video, expired)
            fixture.drain()
            assertEquals(1, fixture.failures.size)
            assertTrue(
                fixture.failures
                    .single()
                    .second
                    .contains("PENDING_QUEUE_FULL"),
            )
            fixture.assertStoppedVideo(decoder, video, samples + expired)
            for (sample in samples + expired) {
                val native = Shadow.extract<NativeMediaEvent>(sample)
                assertEquals(1, Shadow.extract<NativeLinearBlock>(requireNotNull(native.block)).recycles)
            }
        }
    }

    private fun Fixture.assertStoppedVideo(
        decoder: Any,
        filter: Filter,
        events: List<MediaEvent>,
    ) {
        assertTrue(pipeline.currentPlaybackGenerationForTest() != 7L)
        assertEquals(null, ReflectionHelpers.getField<Any?>(pipeline, "videoFilter"))
        assertEquals(null, ReflectionHelpers.getField<Any?>(pipeline, "videoDecoder"))
        val pending = ReflectionHelpers.getField<java.util.ArrayDeque<*>>(decoder, "pendingSamples")
        assertTrue(pending.isEmpty())
        assertEquals(null, ReflectionHelpers.getField<Any?>(decoder, "codec"))
        assertEquals(1, Shadow.extract<NativeFilter>(filter).closes)
        val late = mappedMediaEvent()
        deliver(filter, late)
        drain()
        assertTrue(pending.isEmpty())
        assertEquals(null, ReflectionHelpers.getField<Any?>(decoder, "codec"))
        assertEquals(0, Shadow.extract<NativeMediaEvent>(late).blockReads)
        for (event in events + late) assertEquals(1, Shadow.extract<NativeMediaEvent>(event).releases)
    }

    @Test
    fun captionCapacityLossAndStaleFilterDoNotTerminateCurrentVideo() {
        Fixture().use { fixture ->
            val video = fixture.avFilter(false)
            val caption = fixture.captionFilter()
            // fixture作成時間と実SDK callbackの非待機境界を混同しない。
            val burst = Array(16 * 1024) { restartEvent() }
            fixture.whileOwnerBlocked { deliver(caption, burst) }
            fixture.drain()
            assertEquals(1, fixture.captionDiscontinuities)
            assertEquals(1, Shadow.extract<NativeFilter>(caption).closes)
            val replacement = fixture.avFilter(false)
            val stale = mediaEvent(1L)
            deliver(video, stale)
            fixture.drain()
            assertEquals(0, Shadow.extract<NativeMediaEvent>(stale).blockReads)
            assertEquals(1, Shadow.extract<NativeMediaEvent>(stale).releases)
            assertFalse(fixture.released())
            assertTrue(fixture.failures.isEmpty())
            assertEquals(0, Shadow.extract<NativeFilter>(replacement).closes)
        }
    }

    @Test
    fun captionCapacityRecoveryRetiresQueuedPesBeforeReadingNewInput() {
        Fixture().use { fixture ->
            val video = fixture.avFilter(false)
            val caption = fixture.captionFilter()
            val nativeCaption = Shadow.extract<NativeFilter>(caption)
            // 先に受理したPESは、旧Filter退役後に新しいFMQ入力を読んではならない。
            fixture.whileOwnerBlocked {
                nativeCaption.offer(ByteArray(12) { 0x55 })
                deliver(caption, pesEvent(12))
                deliver(caption, Array(16 * 1024) { restartEvent() })
            }
            fixture.drain()
            assertEquals(emptyList(), nativeCaption.readSizes)
            assertEquals(1, nativeCaption.closes)
            assertEquals(0, nativeCaption.flushes)
            val currentCaption = fixture.currentCaptionFilter()
            assertTrue(currentCaption !== caption)
            val nativeCurrent = Shadow.extract<NativeFilter>(currentCaption)
            val currentPes = captionPes()
            nativeCurrent.offer(currentPes)
            deliver(currentCaption, pesEvent(currentPes.size))
            fixture.drain()
            assertEquals(listOf(currentPes.size), nativeCurrent.readSizes)
            assertEquals(1, fixture.captionPayloads.size)
            assertContentEquals(currentPes.copyOfRange(9, currentPes.size), fixture.captionPayloads.single())
            val videoEvent = mediaEvent(1L)
            deliver(video, videoEvent)
            fixture.drain()
            assertEquals(1, Shadow.extract<NativeMediaEvent>(videoEvent).blockReads)
            assertFalse(fixture.released())
        }
    }

    @Test
    fun captionPesBeforeBufferClearDuringRecoveryCannotReachNewInput() = captionPesDuringRecovery(false)

    @Test
    fun captionPesAfterBufferClearBeforeRecoveryReturnsCannotReachNewInput() = captionPesDuringRecovery(true)

    private fun captionPesDuringRecovery(afterClear: Boolean) {
        Fixture().use { fixture ->
            val video = fixture.avFilter(false)
            val caption = fixture.captionFilter()
            val nativeCaption = Shadow.extract<NativeFilter>(caption)
            val (recoveryStarted, allowRecovery) = nativeCaption.blockNextReclamation(afterClear)
            fixture.whileOwnerBlocked {
                nativeCaption.offer(ByteArray(12) { 0x55 })
                deliver(caption, pesEvent(12))
                deliver(caption, Array(16 * 1024) { restartEvent() })
            }
            assertTrue(recoveryStarted.await(5, TimeUnit.SECONDS))
            // 消去前/消去後return前の旧FMQ入力Aを、復帰後の通知Bへ読み替えない。
            val arrivingPes = captionPes().also { it[11] = 0x22 }
            try {
                nativeCaption.offer(arrivingPes)
                deliver(caption, pesEvent(arrivingPes.size))
            } finally {
                allowRecovery.countDown()
            }
            fixture.drain()
            assertEquals(emptyList(), nativeCaption.readSizes)
            assertEquals(1, nativeCaption.closes)
            val currentCaption = fixture.currentCaptionFilter()
            assertTrue(currentCaption !== caption)
            val nativeCurrent = Shadow.extract<NativeFilter>(currentCaption)
            val currentPes = captionPes().also { it[11] = 0x33 }
            nativeCurrent.offer(currentPes)
            deliver(currentCaption, pesEvent(currentPes.size))
            fixture.drain()
            assertEquals(listOf(currentPes.size), nativeCurrent.readSizes)
            assertEquals(0, nativeCurrent.unreadBytes)
            assertEquals(emptyList(), nativeCaption.readSizes)
            assertContentEquals(currentPes.copyOfRange(9, currentPes.size), fixture.captionPayloads.single())
            val videoEvent = mediaEvent(1L)
            deliver(video, videoEvent)
            fixture.drain()
            val nativeVideo = Shadow.extract<NativeMediaEvent>(videoEvent)
            assertEquals(1, nativeVideo.blockReads)
            assertEquals(1, nativeVideo.releases)
            assertFalse(fixture.released())
        }
    }

    @Test
    fun captionRecoveryCloseFailureKeepsOwnershipAndDoesNotOpenReplacement() {
        Fixture().use { fixture ->
            val video = fixture.avFilter(false)
            val caption = fixture.captionFilter()
            val nativeCaption = Shadow.extract<NativeFilter>(caption)
            nativeCaption.closeResult = Tuner.RESULT_UNKNOWN_ERROR
            try {
                fixture.whileOwnerBlocked { deliver(caption, Array(16 * 1024) { restartEvent() }) }
                fixture.drain()
                assertEquals(null, ReflectionHelpers.getField<Filter?>(fixture.pipeline, "subtitleFilter"))
                assertEquals(1, nativeCaption.closes)
                assertTrue(ReflectionHelpers.getField<ResourceCleanup>(fixture.pipeline, "resourceCleanup").hasPending)
                val videoEvent = mediaEvent(1L)
                deliver(video, videoEvent)
                fixture.drain()
                assertEquals(1, Shadow.extract<NativeMediaEvent>(videoEvent).blockReads)
                assertFalse(fixture.released())
            } finally {
                nativeCaption.closeResult = Tuner.RESULT_SUCCESS
            }
        }
    }

    @Test
    fun actualOwnerExecutionFailureNotifiesOnceAndCleansOnSameOwner() {
        Fixture().use { fixture ->
            val video = fixture.avFilter(false)
            fixture.invoke("enqueuePlaybackAction", { error("actual callback failure") })
            assertTrue(fixture.executor.awaitTermination(5, TimeUnit.SECONDS))
            assertTrue(fixture.released())
            assertEquals(1, fixture.unavailable.size)
            assertEquals(
                PlaybackPipeline.PlaybackUnavailableReason.PLAYBACK_RECOVERY_FAILED,
                fixture.unavailable.single().reason,
            )
            val native = Shadow.extract<NativeFilter>(video)
            assertEquals(1, native.closes)
            assertTrue(native.closeThread.orEmpty().startsWith("maleicacid-playback-"))
        }
    }

    // MediaCodecのJava callback配送を使用し、ネイティブqueueだけをhost shadowへ置き換える。
    @Test
    fun videoOutputStallAfterFirstFrameIsNotTreatedAsContinuousPlayback() {
        Fixture().use { fixture ->
            ReflectionHelpers.setField(fixture.pipeline, "videoPathExpected", true)
            ReflectionHelpers.getField<AtomicBoolean>(fixture.pipeline, "videoAvailableNotified").set(true)
            fixture.invoke("armVideoOutputStallWatchdog", 7L)
            Shadows.shadowOf(Looper.getMainLooper()).idleFor(Duration.ofMillis(6_001L))
            fixture.drain()
            assertTrue(
                fixture.unavailable.any {
                    it.reason == PlaybackPipeline.PlaybackUnavailableReason.VIDEO_OUTPUT_RENDER_FAILED
                },
            )
            assertTrue(fixture.unavailable.any { it.detail.contains("映像描画callback") })
        }
    }

    @Test
    fun staleRenderWatchdogCannotFailAnUpdatedFrameDeadline() {
        Fixture().use { fixture ->
            ReflectionHelpers.setField(fixture.pipeline, "videoPathExpected", true)
            ReflectionHelpers.getField<AtomicBoolean>(fixture.pipeline, "videoAvailableNotified").set(true)
            fixture.invoke("armVideoOutputStallWatchdog", 7L)
            val stale = ReflectionHelpers.getField<Runnable>(fixture.pipeline, "videoOutputStallTask")
            fixture.invoke("armVideoOutputStallWatchdog", 7L)
            val current = ReflectionHelpers.getField<Runnable>(fixture.pipeline, "videoOutputStallTask")
            assertFalse(stale === current)
            // 旧Runnableは既にmainで実行を開始していた可能性がある。
            stale.run()
            fixture.drain()
            assertTrue(fixture.unavailable.isEmpty())
            assertEquals(current, ReflectionHelpers.getField<Runnable>(fixture.pipeline, "videoOutputStallTask"))
        }
    }

    @Test
    fun stockAndroid15WithoutPlatformPatchSelectsCodecCallbackFallback() {
        // Robolectric Android 15はMediaSyncの任意private拡張を持たない。
        // 実装はこのAPIを必須にせず公開MediaCodec callbackを使用する。
        assertFalse(MediaSyncFirstOutputBridge.isAvailable())
    }

    @Test
    fun codecBlockModelCallbackReportsVideoInputOutputAndRenderedFrameSeparately() {
        Fixture().use { fixture ->
            fixture.decoder()
            val codec = fixture.attachCodec()
            val nativeCodec = Shadow.extract<NativeCodec>(codec)
            val video = fixture.avFilter(false)
            val packet = mappedMediaEvent()
            deliver(video, packet)
            fixture.drain()
            assertEquals(1L, fixture.progress().count(PlaybackDataPathProgress.Stage.MEDIA_EVENT, false))
            assertEquals(0L, fixture.progress().count(PlaybackDataPathProgress.Stage.CODEC_INPUT, false))
            postCodecCallback(codec, 1, 0, null)
            assertTrue(nativeCodec.inputQueued.await(5, TimeUnit.SECONDS))
            fixture.drain()
            assertEquals(1L, fixture.progress().count(PlaybackDataPathProgress.Stage.CODEC_INPUT, false))
            assertEquals(1, Shadow.extract<NativeMediaEvent>(packet).releases)
            assertEquals(
                1,
                Shadow.extract<NativeLinearBlock>(requireNotNull(Shadow.extract<NativeMediaEvent>(packet).block)).recycles,
            )
            assertEquals(0L, fixture.progress().count(PlaybackDataPathProgress.Stage.CODEC_OUTPUT, false))
            val output = MediaCodec.BufferInfo().apply { set(0, 1, 90_000L, 0) }
            postCodecCallback(codec, 2, 0, output)
            assertTrue(nativeCodec.outputReleased.await(5, TimeUnit.SECONDS))
            fixture.drain()
            assertEquals(1L, fixture.progress().count(PlaybackDataPathProgress.Stage.CODEC_OUTPUT, false))
            assertEquals(0L, fixture.progress().count(PlaybackDataPathProgress.Stage.FRAME_RENDERED, false))
            assertEquals(0L, fixture.progress().count(PlaybackDataPathProgress.Stage.CODEC_OUTPUT, true))
            val listener =
                ReflectionHelpers.getField<MediaCodec.OnFrameRenderedListener>(
                    codec,
                    "mOnFrameRenderedListener",
                )
            val logCountBefore = availabilityLogMessages().size
            val armedAt = System.nanoTime()
            fixture.armCompatibilityAvailability(7L, armedAt)
            listener.onFrameRendered(codec, 90_000L, armedAt + 1L)
            fixture.drain()
            assertEquals(1L, fixture.progress().count(PlaybackDataPathProgress.Stage.FRAME_RENDERED, false))
            assertEquals(listOf(7L), fixture.available)
            val committed = availabilityLogMessages()
            assertEquals(logCountBefore + 1, committed.size)
            assertTrue(committed.last().contains("generation=7 mode=MEDIA_CODEC_TO_MEDIASYNC_INPUT_COMPAT"))
            assertTrue(committed.last().contains("VIDEO.CODEC_OUTPUT=1"))
            assertTrue(committed.last().contains("AUDIO.CODEC_OUTPUT=0"))
            assertTrue(committed.last().contains("VIDEO.FRAME_RENDERED=1"))
            // 同じframeの再通知では、既存の一回限りのavailability確定を再実行しない。
            listener.onFrameRendered(codec, 90_000L, armedAt + 2L)
            fixture.drain()
            assertEquals(listOf(7L), fixture.available)
            assertEquals(logCountBefore + 1, availabilityLogMessages().size)
            // 次の世代が始まった後は旧Decoder callbackが成功通知も計数も行わない。
            fixture.armCompatibilityAvailability(8L, System.nanoTime())
            listener.onFrameRendered(codec, 90_000L, System.nanoTime())
            fixture.drain()
            assertEquals(listOf(7L), fixture.available)
            assertEquals(logCountBefore + 1, availabilityLogMessages().size)
            assertEquals(2L, fixture.progress().count(PlaybackDataPathProgress.Stage.FRAME_RENDERED, false))
        }
    }

    @Test
    fun audioCodecCallbackDoesNotMasqueradeAsVideoOutputOrCountStaleOutput() {
        Fixture().use { fixture ->
            fixture.decoder(audio = true)
            val codec = fixture.attachCodec(audio = true)
            val nativeCodec = Shadow.extract<NativeCodec>(codec)
            val audio = fixture.avFilter(true)
            deliver(audio, mappedMediaEvent())
            fixture.drain()
            postCodecCallback(codec, 1, 0, null)
            assertTrue(nativeCodec.inputQueued.await(5, TimeUnit.SECONDS))
            fixture.drain()
            assertEquals(1L, fixture.progress().count(PlaybackDataPathProgress.Stage.CODEC_INPUT, true))
            postCodecCallback(codec, 2, 0, MediaCodec.BufferInfo().apply { set(0, 1, 90_000L, 0) })
            assertTrue(nativeCodec.outputReleased.await(5, TimeUnit.SECONDS))
            fixture.drain()
            assertEquals(1L, fixture.progress().count(PlaybackDataPathProgress.Stage.CODEC_OUTPUT, true))
            assertEquals(0L, fixture.progress().count(PlaybackDataPathProgress.Stage.CODEC_OUTPUT, false))
            // 終了済み世代のCodec callbackは、現世代の映像・音声進捗へ算入しない。
            fixture.executor.callControl(5_000L) {
                ReflectionHelpers.setField(fixture.pipeline, "playbackGeneration", 8L)
            }
            postCodecCallback(codec, 2, 0, MediaCodec.BufferInfo().apply { set(0, 1, 90_001L, 0) })
            fixture.drain()
            assertEquals(1L, fixture.progress().count(PlaybackDataPathProgress.Stage.CODEC_OUTPUT, true))
            assertEquals(0L, fixture.progress().count(PlaybackDataPathProgress.Stage.CODEC_OUTPUT, false))
        }
    }

    @Test
    fun codecInputWithoutOutputIsReportedAtRealFirstFrameDeadline() {
        Fixture().use { fixture ->
            fixture.decoder()
            val codec = fixture.attachCodec()
            val nativeCodec = Shadow.extract<NativeCodec>(codec)
            val video = fixture.avFilter(false)
            deliver(video, mappedMediaEvent())
            fixture.drain()
            postCodecCallback(codec, 1, 0, null)
            assertTrue(nativeCodec.inputQueued.await(5, TimeUnit.SECONDS))
            fixture.drain()
            fixture.invoke("scheduleFirstFrameTimeout", 7L)
            Shadows.shadowOf(Looper.getMainLooper()).idleFor(10, TimeUnit.SECONDS)
            fixture.drain()
            val failure = fixture.unavailable.single()
            assertEquals(PlaybackPipeline.PlaybackUnavailableReason.FIRST_FRAME_TIMEOUT, failure.reason)
            assertTrue(failure.detail.contains("VIDEO.MEDIA_EVENT=1"))
            assertTrue(failure.detail.contains("VIDEO.CODEC_INPUT=1"))
            assertTrue(failure.detail.contains("VIDEO.CODEC_OUTPUT=0"))
            assertTrue(failure.detail.contains("AUDIO.CODEC_OUTPUT=0"))
            assertTrue(failure.detail.contains("VIDEO.FRAME_RENDERED=0"))
        }
    }

    @Test
    fun decoderStartupDeadlineRecordsCodecInputAndRestartResetsDiagnosticGeneration() {
        Fixture().use { fixture ->
            fixture.decoder()
            val decoder = ReflectionHelpers.getField<Any>(fixture.pipeline, "videoDecoder")
            val arm =
                decoder.javaClass.superclass
                    .getDeclaredMethod("armStartupDeadline")
                    .apply { isAccessible = true }
            fixture.executor.callControl(5_000L) { arm.invoke(decoder) }
            val codec = fixture.attachCodec()
            val nativeCodec = Shadow.extract<NativeCodec>(codec)
            val video = fixture.avFilter(false)
            deliver(video, mappedMediaEvent())
            fixture.drain()
            postCodecCallback(codec, 1, 0, null)
            assertTrue(nativeCodec.inputQueued.await(5, TimeUnit.SECONDS))
            fixture.drain()
            Shadows.shadowOf(Looper.getMainLooper()).idleFor(5, TimeUnit.SECONDS)
            fixture.drain()
            val failure = fixture.unavailable.single()
            assertEquals(PlaybackPipeline.PlaybackUnavailableReason.FIRST_FRAME_TIMEOUT, failure.reason)
            assertTrue(failure.detail.contains("DECODER_STARTUP_TIMEOUT stage=FIRST_OUTPUT"))
            assertTrue(failure.detail.contains("VIDEO.CODEC_INPUT=1"))
            assertTrue(failure.detail.contains("VIDEO.CODEC_OUTPUT=0"))
            val beforeRestart = fixture.pipeline.currentPlaybackGenerationForTest()
            val service = ServiceKey(4, 0x4010, 101)
            val channel =
                TunerController.ResolvedChannel(
                    null,
                    "admission-test",
                    service,
                    1,
                    "test",
                    "1",
                    "ISDB_T",
                    FrequencyHz(473_000_000L),
                    StreamSelector.NONE,
                    null,
                    null,
                )
            val selection =
                TunerController.AvStreamSelection(
                    service,
                    TsPid(0x100),
                    AribElementaryStream(TsPid(0x101), 0x1b, null, null, null),
                    null,
                )
            val result = fixture.pipeline.start(fixture.tuner, channel, selection)
            assertTrue(result.generation > beforeRestart)
            assertEquals(0L, fixture.progress().count(PlaybackDataPathProgress.Stage.CODEC_INPUT, false))
            assertEquals(0L, fixture.progress().count(PlaybackDataPathProgress.Stage.CODEC_OUTPUT, false))
            assertEquals(0L, fixture.progress().count(PlaybackDataPathProgress.Stage.CODEC_INPUT, true))
        }
    }

    private fun availabilityLogMessages(): List<String> =
        ShadowLog
            .getLogsForTag(LogTags.TIS)
            .map { it.msg }
            .filter { it.startsWith("VIDEO_AVAILABILITY_COMMITTED inputId=admission-test ") }

    private fun Fixture.attachCodec(audio: Boolean = false): MediaCodec {
        val codecName = if (audio) "maleicacid.test.mpeg.decoder" else "maleicacid.test.avc.decoder"
        val caps =
            if (audio) {
                MediaFormat.createAudioFormat(MediaFormat.MIMETYPE_AUDIO_MPEG, 44_100, 2)
            } else {
                MediaFormat.createVideoFormat(MediaFormat.MIMETYPE_VIDEO_AVC, 4096, 4096)
            }
        val capabilityBuilder =
            MediaCodecInfoBuilder.CodecCapabilitiesBuilder
                .newBuilder()
                .setMediaFormat(caps)
        if (!audio) {
            capabilityBuilder.setColorFormats(
                intArrayOf(MediaCodecInfo.CodecCapabilities.COLOR_FormatYUV420Flexible),
            )
        }
        val codecInfo =
            MediaCodecInfoBuilder
                .newBuilder()
                .setName(codecName)
                .setCapabilities(capabilityBuilder.build())
                .build()
        ShadowMediaCodecList.addCodec(codecInfo)
        val decoder = ReflectionHelpers.getField<Any>(pipeline, if (audio) "audioDecoder" else "videoDecoder")
        if (!audio) {
            val texture = SurfaceTexture(0)
            val surface = Surface(texture)
            ReflectionHelpers.setField(decoder, "outputSurface", surface)
            codecSurface = surface
            codecTexture = texture
        }
        val method =
            decoder.javaClass.superclass
                .getDeclaredMethod("createBlockModelDecoder", MediaFormat::class.java)
                .apply { isAccessible = true }
        val format =
            if (audio) {
                MediaFormat.createAudioFormat(MediaFormat.MIMETYPE_AUDIO_MPEG, 44_100, 2)
            } else {
                MediaFormat.createVideoFormat(MediaFormat.MIMETYPE_VIDEO_AVC, 32, 32)
            }
        val codec = method.invoke(decoder, format) as MediaCodec
        ReflectionHelpers.setField(decoder, "codec", codec)
        return codec
    }

    private fun postCodecCallback(
        codec: MediaCodec,
        kind: Int,
        index: Int,
        info: MediaCodec.BufferInfo?,
    ) {
        ReflectionHelpers.callInstanceMethod<Unit>(
            codec,
            "postEventFromNative",
            ReflectionHelpers.ClassParameter.from(Int::class.javaPrimitiveType, 1),
            ReflectionHelpers.ClassParameter.from(Int::class.javaPrimitiveType, kind),
            ReflectionHelpers.ClassParameter.from(Int::class.javaPrimitiveType, index),
            ReflectionHelpers.ClassParameter.from(Any::class.java, info),
        )
    }

    private inner class Fixture : AutoCloseable {
        val pipeline = PlaybackPipeline("admission-test", null)
        val executor = ReflectionHelpers.getField<LifecycleSerialExecutor>(pipeline, "executor")
        val tuner = Tuner(RuntimeEnvironment.getApplication(), null, 0)
        val failures = mutableListOf<Pair<PlaybackPipeline.PlaybackUnavailableReason, String>>()
        val unavailable = mutableListOf<PlaybackPipeline.PlaybackUnavailable>()
        val captionPayloads = mutableListOf<ByteArray>()
        val available = mutableListOf<Long>()
        var codecSurface: Surface? = null
        var codecTexture: SurfaceTexture? = null
        var captionDiscontinuities = 0

        init {
            ReflectionHelpers.setField(pipeline, "playbackGeneration", 7L)
            ReflectionHelpers.setField(pipeline, "onVideoAvailable", { generation: Long ->
                available += generation
            })
            ReflectionHelpers.setField(
                pipeline,
                "onVideoUnavailable",
                { failure: PlaybackPipeline.PlaybackUnavailable -> unavailable += failure },
            )
            ReflectionHelpers.setField(pipeline, "onSubtitleContinuityLost", { _: Long, _: String ->
                captionDiscontinuities++
            })
            ReflectionHelpers.setField(
                pipeline,
                "onSubtitlePes",
                { _: Long, _: String, payload: ByteArray, _: Any -> captionPayloads += payload },
            )
        }

        fun released(): Boolean = ReflectionHelpers.getField<AtomicBoolean>(pipeline, "released").get()

        fun progress(): PlaybackDataPathProgress = ReflectionHelpers.getField(pipeline, "dataPathProgress")

        // 未パッチOSのcompatibility modeを既存のavailability arm契約のまま再現する。
        fun armCompatibilityAvailability(
            generation: Long,
            armedAtNanoTime: Long,
        ) {
            val armType = PlaybackPipeline::class.java.declaredClasses.single { it.simpleName == "AvailabilityArm" }
            val arm =
                armType.declaredConstructors
                    .single()
                    .apply { isAccessible = true }
                    .newInstance(generation, 1L, armedAtNanoTime)
            val mode =
                PlaybackPipeline::class.java.declaredClasses
                    .single { it.simpleName == "VideoAvailabilityMode" }
                    .enumConstants
                    .single { it.toString() == "MEDIA_CODEC_TO_MEDIASYNC_INPUT_COMPAT" }
            executor.callControl(5_000L) {
                ReflectionHelpers.setField(pipeline, "surface", requireNotNull(codecSurface))
                ReflectionHelpers.setField(pipeline, "playbackGeneration", generation)
                ReflectionHelpers.setField(pipeline, "waitingAvailabilityArm", arm)
                ReflectionHelpers.setField(pipeline, "videoAvailabilityMode", mode)
                ReflectionHelpers.getField<AtomicBoolean>(pipeline, "videoAvailableNotified").set(false)
            }
        }

        fun currentCaptionFilter(): Filter = ReflectionHelpers.getField(pipeline, "subtitleFilter")

        fun avFilter(audio: Boolean): Filter {
            val stream =
                AribElementaryStream(
                    TsPid(if (audio) 0x102 else 0x101),
                    if (audio) 0x03 else 0x1b,
                    null,
                    null,
                    null,
                )
            val filter = invoke("createAndStartAvFilter", tuner, stream, audio) as Filter
            ReflectionHelpers.setField(pipeline, if (audio) "audioFilter" else "videoFilter", filter)
            return filter
        }

        fun captionFilter(): Filter {
            val stream = AribElementaryStream(TsPid(0x103), 0x06, null, null, null, isCaption = true)
            val filter = invoke("createAndStartCaptionPesFilter", tuner, stream, "caption", false) as Filter
            ReflectionHelpers.setField(pipeline, "subtitleFilter", filter)
            return filter
        }

        fun decoder(audio: Boolean = false) {
            val name = if (audio) "AudioDecoderPipeline" else "VideoDecoderPipeline"
            val type = Class.forName("com.maleicacid.tvinput.tis.PlaybackPipeline\$$name")
            val constructor = type.declaredConstructors.single().apply { isAccessible = true }
            val sink: (PlaybackPipeline.PlaybackUnavailableReason, String) -> Unit = { reason, detail ->
                failures += reason to detail
            }
            val decoder =
                if (audio) {
                    val kind =
                        Class
                            .forName("com.maleicacid.tvinput.tis.PlaybackPipeline\$AudioCodecKind")
                            .enumConstants
                            .single { it.toString() == "MPEG1" }
                    constructor.newInstance(
                        pipeline,
                        kind,
                        AribElementaryStream(TsPid(0x102), 0x03, null, null, null),
                        null,
                        false,
                        PlaybackPipeline.DualMonoPresentation.MAIN,
                        1.0f,
                        7L,
                        sink,
                    )
                } else {
                    constructor.newInstance(
                        pipeline,
                        PlaybackPipeline.VideoCodecKind.AVC,
                        AribCodecFacts(),
                        Surface(),
                        7L,
                        sink,
                    )
                }
            ReflectionHelpers.setField(pipeline, if (audio) "audioDecoder" else "videoDecoder", decoder)
        }

        fun pendingEvents(audio: Boolean): List<MediaEvent> {
            val decoder = ReflectionHelpers.getField<Any>(pipeline, if (audio) "audioDecoder" else "videoDecoder")
            val pending = ReflectionHelpers.getField<java.util.ArrayDeque<*>>(decoder, "pendingSamples")
            return pending.map { ReflectionHelpers.getField(it, "event") }
        }

        fun capacityLoss(filter: Filter): MediaEvent {
            // 各4MiB入力はAVCの単一sample予算内。owner停止中の累積だけで拒否する。
            val samples = List(4) { mediaEvent(4L * 1024 * 1024) }
            whileOwnerBlocked { samples.forEach { deliver(filter, it) } }
            drain()
            samples.take(3).forEach { assertEquals(1, Shadow.extract<NativeMediaEvent>(it).blockReads) }
            samples.forEach { assertEquals(1, Shadow.extract<NativeMediaEvent>(it).releases) }
            assertEquals(0, Shadow.extract<NativeMediaEvent>(samples.last()).blockReads)
            return samples.last()
        }

        fun whileOwnerBlocked(callback: () -> Unit) {
            val entered = CountDownLatch(1)
            val resume = CountDownLatch(1)
            executor.executeControl {
                entered.countDown()
                resume.await()
            }
            try {
                assertTrue(entered.await(5, TimeUnit.SECONDS))
                val started = System.nanoTime()
                callback()
                assertTrue(System.nanoTime() - started < TimeUnit.SECONDS.toNanos(1))
            } finally {
                resume.countDown()
            }
        }

        fun drain() {
            val done = CountDownLatch(1)
            executor.executeData { done.countDown() }
            assertTrue(done.await(5, TimeUnit.SECONDS))
            // owner taskのfinallyでpermitを返す時点まで確認する。
            executor.callControl(5_000L) { Unit }
        }

        // 本番にtest入口を追加せず、同じfixtureから既存private操作を呼び出す。
        @Suppress("SpreadOperator")
        fun invoke(
            name: String,
            vararg arguments: Any,
        ): Any? =
            PlaybackPipeline::class.java.declaredMethods
                .single { it.name == name || it.name.startsWith("$name-") }
                .apply { isAccessible = true }
                .invoke(pipeline, *arguments)

        override fun close() {
            try {
                pipeline.close()
            } finally {
                codecSurface?.release()
                codecTexture?.release()
            }
        }
    }

    private fun mappedMediaEvent(): MediaEvent =
        mediaEvent(1L).also {
            Shadow.extract<NativeMediaEvent>(it).block =
                ReflectionHelpers.callConstructor(MediaCodec.LinearBlock::class.java)
        }

    private fun mediaEvent(length: Long): MediaEvent =
        MediaEvent::class.java.declaredConstructors
            .single { it.parameterCount == 15 }
            .apply { isAccessible = true }
            .newInstance(
                0,
                false,
                0L,
                false,
                0L,
                length,
                0L,
                null,
                false,
                0L,
                0,
                false,
                0,
                null,
                emptyList<Any>(),
            ) as MediaEvent

    private fun restartEvent(): RestartEvent =
        RestartEvent::class.java.declaredConstructors
            .single { it.parameterCount == 1 }
            .apply { isAccessible = true }
            .newInstance(
                0,
            ) as RestartEvent

    private fun pesEvent(length: Int): PesEvent =
        PesEvent::class.java.declaredConstructors
            .single { it.parameterCount == 3 }
            .apply { isAccessible = true }
            .newInstance(0, length, 0) as PesEvent

    private fun captionPes(): ByteArray =
        ByteArray(20).also { pes ->
            pes[2] = 1
            pes[3] = 0xbd.toByte()
            pes[5] = 14
            pes[6] = 0x80.toByte()
            pes[9] = 0x80.toByte()
            pes[10] = 0xff.toByte()
            for (index in 11 until pes.size) pes[index] = index.toByte()
        }

    private fun deliver(
        filter: Filter,
        event: FilterEvent,
    ) = deliver(filter, arrayOf(event))

    private fun deliver(
        filter: Filter,
        events: Array<out FilterEvent>,
    ) {
        ReflectionHelpers.callInstanceMethod<Unit>(
            filter,
            "onFilterEvent",
            ReflectionHelpers.ClassParameter.from(Array<FilterEvent>::class.java, events),
        )
    }

    private fun status(
        filter: Filter,
        value: Int,
    ) {
        ReflectionHelpers.callInstanceMethod<Unit>(
            filter,
            "onFilterStatus",
            ReflectionHelpers.ClassParameter.from(Int::class.javaPrimitiveType, value),
        )
    }

    @Implements(Tuner::class)
    class NativeTuner {
        @Implementation(methodName = "__constructor__")
        @Suppress("UNUSED_PARAMETER")
        private fun construct(
            context: Context,
            sessionId: String?,
            useCase: Int,
        ) = Unit

        @Implementation
        fun openFilter(
            type: Int,
            subtype: Int,
            bufferSize: Long,
            executor: Executor,
            callback: FilterCallback,
        ): Filter {
            assertTrue(bufferSize > 0L)
            val filter =
                ReflectionHelpers.callConstructor(
                    Filter::class.java,
                    ReflectionHelpers.ClassParameter.from(Long::class.javaPrimitiveType, 1L),
                )
            ReflectionHelpers.setField(filter, "mMainType", type)
            val sdkSubtype =
                ReflectionHelpers.callStaticMethod<Int>(
                    TunerUtils::class.java,
                    "getFilterSubtype",
                    ReflectionHelpers.ClassParameter.from(Int::class.javaPrimitiveType, type),
                    ReflectionHelpers.ClassParameter.from(Int::class.javaPrimitiveType, subtype),
                )
            ReflectionHelpers.setField(filter, "mSubtype", sdkSubtype)
            filter.setCallback(callback, executor)
            return filter
        }
    }

    @Implements(Filter::class)
    class NativeFilter {
        var closes = 0
        var flushes = 0
        var closeThread: String? = null
        var closeResult: Int = Tuner.RESULT_SUCCESS
        var flushResult: Int = Tuner.RESULT_SUCCESS
        val readSizes = mutableListOf<Int>()
        private val pendingBytes = ArrayDeque<Byte>()
        val unreadBytes: Int get() = pendingBytes.size
        private var reclamationBlock: Pair<CountDownLatch, CountDownLatch>? = null
        private var blockAfterClear = false

        fun offer(bytes: ByteArray) {
            bytes.forEach(pendingBytes::addLast)
        }

        fun blockNextReclamation(afterClear: Boolean): Pair<CountDownLatch, CountDownLatch> {
            val block = Pair(CountDownLatch(1), CountDownLatch(1))
            reclamationBlock = block
            blockAfterClear = afterClear
            return block
        }

        private fun reclaimBytes() {
            if (blockAfterClear) pendingBytes.clear()
            reclamationBlock?.let { (started, allow) ->
                started.countDown()
                check(allow.await(5, TimeUnit.SECONDS)) { "test reclamation was not released" }
                reclamationBlock = null
            }
            if (!blockAfterClear) pendingBytes.clear()
        }

        @Implementation
        @Suppress("UNUSED_PARAMETER")
        fun nativeConfigureFilter(
            type: Int,
            subtype: Int,
            configuration: FilterConfiguration,
        ): Int = Tuner.RESULT_SUCCESS

        @Implementation
        fun nativeStartFilter(): Int = Tuner.RESULT_SUCCESS

        @Implementation
        fun nativeStopFilter(): Int = Tuner.RESULT_SUCCESS

        @Implementation
        fun nativeFlushFilter(): Int {
            flushes++
            reclaimBytes()
            return flushResult
        }

        @Implementation
        fun nativeRead(
            buffer: ByteArray,
            offset: Long,
            size: Long,
        ): Int {
            val count = minOf(size.toInt(), pendingBytes.size)
            repeat(count) { index -> buffer[offset.toInt() + index] = pendingBytes.removeFirst() }
            readSizes += count
            return count
        }

        @Implementation
        fun nativeClose(): Int {
            closes++
            closeThread = Thread.currentThread().name
            if (closeResult == Tuner.RESULT_SUCCESS) reclaimBytes()
            return closeResult
        }
    }

    @Implements(MediaEvent::class)
    class NativeMediaEvent {
        var block: MediaCodec.LinearBlock? = null
        var releases = 0
        var blockReads = 0
        var releaseThread: String? = null
        var onRelease: () -> Unit = {}

        @Implementation
        fun nativeGetLinearBlock(): MediaCodec.LinearBlock? {
            blockReads++
            return block
        }

        @Implementation
        fun nativeFinalize() {
            releases++
            releaseThread = Thread.currentThread().name
            onRelease()
        }
    }

    // JNI互換のnative引数数とRobolectric既存override可視性は変更できない。
    @Suppress("LongParameterList", "ProtectedMemberInFinalClass")
    @Implements(MediaCodec::class)
    class NativeCodec : ShadowMediaCodec() {
        val inputQueued = CountDownLatch(1)
        val outputReleased = CountDownLatch(1)

        @Implementation
        protected override fun native_start() = Unit

        @Implementation
        @Suppress("UNUSED_PARAMETER")
        protected fun native_queueLinearBlock(
            index: Int,
            block: MediaCodec.LinearBlock,
            cryptoInfos: Array<Any>?,
            bufferInfos: Array<Any>,
            keys: ArrayList<String>,
            values: ArrayList<Any>,
        ) {
            assertEquals(1, bufferInfos.size)
            inputQueued.countDown()
        }

        @Implementation
        @Suppress("UNUSED_PARAMETER")
        protected override fun releaseOutputBuffer(
            index: Int,
            renderTimestampNs: Long,
        ) {
            outputReleased.countDown()
        }

        @Implementation
        @Suppress("UNUSED_PARAMETER")
        protected override fun releaseOutputBuffer(
            index: Int,
            render: Boolean,
        ) {
            outputReleased.countDown()
        }
    }

    @Implements(MediaCodec.LinearBlock::class)
    class NativeLinearBlock {
        var recycles = 0

        @Implementation
        fun map(): ByteBuffer = ByteBuffer.wrap(byteArrayOf(0))

        @Implementation
        fun recycle() {
            recycles++
        }
    }
}

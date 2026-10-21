// 実SDK callback lock・時計・MediaEvent寿命を使い、HAL/native境界だけを置換する。
@file:Suppress("MagicNumber", "MaxLineLength")

package com.maleicacid.tvinput.tis

import android.content.Context
import android.graphics.SurfaceTexture
import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaFormat
import android.media.MediaSync
import android.media.tv.TvInputService
import android.media.tv.TvTrackInfo
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
import kotlin.test.assertNull
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
    fun secondOverflowMustInvalidatePartialHeader() {
        Fixture().use { fixture ->
            fixture.decoder(videoKind = PlaybackPipeline.VideoCodecKind.MPEG2)
            val filter = fixture.avFilter(false)
            status(filter, Filter.STATUS_OVERFLOW)
            fixture.drain()
            val event = mediaEvent(4L)
            val block = ReflectionHelpers.callConstructor(MediaCodec.LinearBlock::class.java)
            Shadow.extract<NativeLinearBlock>(block).data = byteArrayOf(0, 0, 1, 0xb3.toByte())
            Shadow.extract<NativeMediaEvent>(event).block = block
            deliver(filter, event)
            fixture.drain()
            status(filter, Filter.STATUS_OVERFLOW)
            fixture.drain()
            val decoder = ReflectionHelpers.getField<Any>(fixture.pipeline, "videoDecoder")
            val getProbe =
                decoder.javaClass.superclass
                    .getDeclaredMethod("getConfigurationProbe")
                    .apply { isAccessible = true }
            val probe = getProbe.invoke(decoder) as CodecHeaderProbe
            var fabricated = false
            fixture.executor.callControl(5000L) {
                probe.find(ByteBuffer.wrap(byteArrayOf(0x5a, 4, 0x38, 0x30))) {
                    fabricated = true
                    true
                }
            }
            assertFalse(fabricated, "overflowを跨いだ非連続バイトからsequence headerを構成してはならない")
        }
    }

    @Test
    fun mpegAudioProbeRejectsReservedHeadersAndContinuesToValidFormat() {
        val probe = CodecHeaderProbe(CodecHeaderProbe.Kind.MPEG_AUDIO)
        val formats = mutableListOf<MediaFormat?>()

        fun accept(candidate: ByteArray): Boolean {
            val format = PlaybackPipeline.EsHeaderParser.mpegAudioFormat(candidate)
            formats.add(format)
            return format != null
        }
        assertFalse(probe.find(ByteBuffer.wrap(byteArrayOf(0xff.toByte(), 0xeb.toByte(), 0x90.toByte(), 0x64)), ::accept))
        assertFalse(probe.find(ByteBuffer.wrap(byteArrayOf(0xff.toByte(), 0xf9.toByte(), 0x90.toByte(), 0x64)), ::accept))
        assertEquals(listOf<MediaFormat?>(null, null), formats)
        assertFalse(probe.find(ByteBuffer.wrap(byteArrayOf(0xff.toByte(), 0xfb.toByte())), ::accept))
        assertTrue(probe.find(ByteBuffer.wrap(byteArrayOf(0x90.toByte(), 0x64)), ::accept))
        val format = requireNotNull(formats.last())
        assertEquals(MediaFormat.MIMETYPE_AUDIO_MPEG, format.getString(MediaFormat.KEY_MIME))
        assertEquals(44100, format.getInteger(MediaFormat.KEY_SAMPLE_RATE))
        assertEquals(2, format.getInteger(MediaFormat.KEY_CHANNEL_COUNT))
        assertEquals(3, formats.size)
    }

    // flush前後と次の入力欠落を同じFilterの一連の操作として検査する。
    @Suppress("LongMethod")
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
                // 構成前でも新しい入力を受理した後の欠落は再び回収する。
                assertEquals(2, native.flushes)
                assertEquals(emptyList(), fixture.pendingEvents(audio))
                assertEquals(1, Shadow.extract<NativeMediaEvent>(newInput).releases)
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
    fun overflowWithConfiguredCodecRetiresItsReferencesInsteadOfContinuingAfterFilterFlush() {
        Fixture().use { fixture ->
            fixture.decoder()
            fixture.attachCodec()
            val filter = fixture.avFilter(false)
            val nativeFilter = Shadow.extract<NativeFilter>(filter)
            val pending = mappedMediaEvent()
            deliver(filter, pending)
            fixture.drain()
            status(filter, Filter.STATUS_OVERFLOW)
            fixture.drain()
            assertEquals(0, nativeFilter.flushes)
            assertNull(ReflectionHelpers.getField<Any?>(fixture.pipeline, "videoDecoder"))
            assertNull(ReflectionHelpers.getField<Filter?>(fixture.pipeline, "videoFilter"))
            assertEquals(1, Shadow.extract<NativeMediaEvent>(pending).releases)
            // fixtureには再開対象を設定していないため、元世代へ回復不能を通知する。
            assertTrue(
                fixture.unavailable.any {
                    it.reason == PlaybackPipeline.PlaybackUnavailableReason.PLAYBACK_RECOVERY_FAILED && it.generation == 7L
                },
            )
        }
        Fixture().use { fixture ->
            fixture.decoder()
            fixture.attachCodec()
            fixture.audioSelection()
            ReflectionHelpers.setField(fixture.pipeline, "activeTuner", fixture.tuner)
            val filter = fixture.avFilter(false)
            status(filter, Filter.STATUS_OVERFLOW)
            fixture.drain()
            assertTrue(ReflectionHelpers.getField<Boolean>(fixture.pipeline, "codecRecoveryAttempted"))
            assertNull(ReflectionHelpers.getField<Any?>(fixture.pipeline, "videoDecoder"))
            assertTrue(fixture.unavailable.isEmpty())
            // stop後にmain handlerの再生成予約が届いても旧選局を再開しない。
            fixture.pipeline.stop()
            Shadows.shadowOf(Looper.getMainLooper()).idle()
            fixture.drain()
            assertNull(ReflectionHelpers.getField<Filter?>(fixture.pipeline, "videoFilter"))
            assertTrue(fixture.unavailable.isEmpty())
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
    fun singleMaxSizedHevcEventIsAcceptedIntoAnOtherwiseEmptyCallbackQueue() {
        Fixture().use { fixture ->
            fixture.decoder(videoKind = PlaybackPipeline.VideoCodecKind.HEVC)
            val filter = fixture.avFilter(false)
            val event = mediaEvent(16L * 1024 * 1024)
            fixture.whileOwnerBlocked { deliver(filter, event) }
            fixture.drain()
            // 実LinearBlockがないfixtureでも、受付した場合だけgetLinearBlockまで進む。
            assertEquals(1, Shadow.extract<NativeMediaEvent>(event).blockReads)
            assertEquals(1, Shadow.extract<NativeMediaEvent>(event).releases)
            assertFalse(fixture.released())
        }
    }

    @Test
    fun callbackBatchBeyondTheSharedLimitIsNotAcceptedByClampingItsCharge() {
        Fixture().use { fixture ->
            val filter = fixture.avFilter(false)
            val oversized = arrayOf(mediaEvent(12L * 1024 * 1024), mediaEvent(12L * 1024 * 1024))
            fixture.whileOwnerBlocked { deliver(filter, oversized) }
            fixture.drain()
            oversized.forEach { event ->
                assertEquals(0, Shadow.extract<NativeMediaEvent>(event).blockReads)
                assertEquals(1, Shadow.extract<NativeMediaEvent>(event).releases)
            }
        }
    }

    @Test
    fun compressedInputRecoveryStopsPriorGenerationExactlyOnceBeforeRestart() {
        Fixture().use { fixture ->
            fixture.audioSelection()
            val initialGeneration = fixture.pipeline.currentPlaybackGenerationForTest()
            fixture.executor.callControl(5_000L) {
                ReflectionHelpers.setField(fixture.pipeline, "activeTuner", fixture.tuner)
                fixture.invoke("recoverAfterCompressedInputLoss", initialGeneration, "injected input loss")
            }
            assertEquals(initialGeneration + 1L, fixture.pipeline.currentPlaybackGenerationForTest())
            Shadows.shadowOf(Looper.getMainLooper()).idle()
            fixture.drain()
            // stop済み世代から新世代を一度だけ開始する。Surface不足での開始失敗は本試験の対象外。
            assertEquals(initialGeneration + 2L, fixture.pipeline.currentPlaybackGenerationForTest())
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
    fun sustainedMpegVideoInputBeyondThreeSecondsDoesNotTriggerAdmissionRecovery() {
        Fixture().use { fixture ->
            fixture.decoder(videoKind = PlaybackPipeline.VideoCodecKind.MPEG2)
            val codec = fixture.attachCodec(videoKind = PlaybackPipeline.VideoCodecKind.MPEG2)
            val nativeCodec = Shadow.extract<NativeCodec>(codec)
            val outputsReturned = CountDownLatch(120)
            nativeCodec.onOutputRelease = { outputsReturned.countDown() }
            val rendered = ReflectionHelpers.getField<MediaCodec.OnFrameRenderedListener>(codec, "mOnFrameRenderedListener")
            val video = fixture.avFilter(false)
            val samples = mutableListOf<MediaEvent>()
            // 約4秒間、29.97 fps相当のPTSで入力する。Codecからinput indexが戻る限り
            // 累積PTSの幅そのものを過負荷扱いしないことをproduction経路で固定する。
            repeat(120) { frame ->
                val event =
                    mediaEvent(1L, 90_000L + frame * 3_003L).also {
                        Shadow.extract<NativeMediaEvent>(it).block =
                            ReflectionHelpers.callConstructor(MediaCodec.LinearBlock::class.java)
                    }
                val previous = samples.lastOrNull()
                val released = CountDownLatch(1)
                previous?.let { Shadow.extract<NativeMediaEvent>(it).onRelease = { released.countDown() } }
                samples += event
                deliver(video, event)
                fixture.drain()
                postCodecCallback(codec, 1, 0, null)
                if (previous == null) {
                    assertTrue(nativeCodec.inputQueued.await(5, TimeUnit.SECONDS))
                } else {
                    assertTrue(released.await(5, TimeUnit.SECONDS), "old input must be returned")
                }
                fixture.drain()
                // Android側callbackによる出力・描画進捗も同じdecoder generationで120件継続させる。
                val ptsUs = frame * 33_367L
                postCodecCallback(codec, 2, frame % 4, MediaCodec.BufferInfo().apply { set(0, 1, ptsUs, 0) })
                rendered.onFrameRendered(codec, ptsUs, System.nanoTime())
            }
            assertTrue(outputsReturned.await(5, TimeUnit.SECONDS), "mock Codec must return 120 video outputs")
            returnCodecInputAndAwaitEventRelease(codec, samples.last())
            fixture.drain()
            assertEquals(120L, fixture.progress().count(PlaybackDataPathProgress.Stage.CODEC_INPUT, false))
            assertEquals(120L, fixture.progress().count(PlaybackDataPathProgress.Stage.CODEC_OUTPUT, false))
            assertEquals(120L, fixture.progress().count(PlaybackDataPathProgress.Stage.FRAME_RENDERED, false))
            assertEquals(121L, fixture.progress().count(PlaybackDataPathProgress.Stage.CODEC_INPUT_AVAILABLE, false))
            assertEquals(120L, fixture.progress().count(PlaybackDataPathProgress.Stage.CODEC_INPUT_RETURNED, false))
            assertEquals(0L, fixture.progress().count(PlaybackDataPathProgress.Stage.INPUT_REJECTED, false))
            assertEquals(7L, fixture.pipeline.currentPlaybackGenerationForTest())
            assertTrue(fixture.failures.isEmpty())
            assertTrue(fixture.unavailable.isEmpty())
            assertEquals(0, Shadow.extract<NativeFilter>(video).closes)
            samples.forEach { assertEquals(1, Shadow.extract<NativeMediaEvent>(it).releases) }
        }
    }

    @Test
    fun sparseAudioPtsBeyondThreeSecondsDoesNotRejectBoundedCodecInput() {
        Fixture().use { fixture ->
            fixture.decoder(audio = true)
            fixture.attachCodec(audio = true)
            val filter = fixture.avFilter(true)
            // MediaEventは8件・8byteだけ。PTSの離隔は処理遅延や確保メモリ量ではない。
            val samples =
                List(8) { index ->
                    mediaEvent(1L, 90_000L + index * 45_000L).also { event ->
                        Shadow.extract<NativeMediaEvent>(event).block =
                            ReflectionHelpers.callConstructor(MediaCodec.LinearBlock::class.java)
                    }
                }
            samples.forEach { event ->
                deliver(filter, event)
                fixture.drain()
            }
            assertEquals(8, fixture.pendingEvents(true).size)
            assertEquals(0L, fixture.progress().count(PlaybackDataPathProgress.Stage.INPUT_REJECTED, true))
            assertEquals(7L, fixture.pipeline.currentPlaybackGenerationForTest())
            assertTrue(fixture.unavailable.isEmpty())
            assertTrue(fixture.failures.isEmpty())
            assertEquals(0, Shadow.extract<NativeFilter>(filter).closes)
            // 停止処理では受付済みeventを重複解放せずすべて返す。
            fixture.pipeline.stop()
            samples.forEach { assertEquals(1, Shadow.extract<NativeMediaEvent>(it).releases) }
        }
    }

    @Test
    fun mpeg2HeaderProbeAdvancesBeyondTheFirst256KiB() {
        Fixture().use { fixture ->
            fixture.decoder(videoKind = PlaybackPipeline.VideoCodecKind.MPEG2)
            val decoder = ReflectionHelpers.getField<Any>(fixture.pipeline, "videoDecoder")
            val type = PlaybackPipeline::class.java.declaredClasses.single { it.simpleName == "MediaSample" }
            val constructor = type.declaredConstructors.single { it.parameterCount == 6 }.apply { isAccessible = true }
            val probe =
                decoder.javaClass.superclass
                    .getDeclaredMethod("probeHeader", type)
                    .apply { isAccessible = true }
            repeat(7) { chunk ->
                val bytes = ByteArray(64 * 1024)
                if (chunk == 6) {
                    bytes[10] = 0
                    bytes[11] = 0
                    bytes[12] = 1
                    bytes[13] = 0xb3.toByte()
                    bytes[14] = 0x5a
                    bytes[15] = 0x04
                    bytes[16] = 0x38
                    bytes[17] = 0x30
                }
                val event = mediaEvent(bytes.size.toLong())
                val block = ReflectionHelpers.callConstructor(MediaCodec.LinearBlock::class.java)
                Shadow.extract<NativeLinearBlock>(block).data = bytes
                val sample = constructor.newInstance(event, block, 0, bytes.size, 0L, false)
                val format = probe.invoke(decoder, sample) as MediaFormat?
                if (chunk < 6) {
                    assertEquals(null, format)
                } else {
                    assertEquals(1440, requireNotNull(format).getInteger(MediaFormat.KEY_WIDTH))
                    assertEquals(1080, format.getInteger(MediaFormat.KEY_HEIGHT))
                }
            }
        }
    }

    @Test
    fun decoderStartupKeepsAtMostThreeEventsAndFailsAtTheFiniteDeadline() {
        Fixture().use { fixture ->
            fixture.decoder()
            val video = fixture.avFilter(false)
            val decoder = ReflectionHelpers.getField<Any>(fixture.pipeline, "videoDecoder")
            val samples = List(33) { mappedMediaEvent() }
            samples.forEach { deliver(video, it) }
            fixture.drain()
            // 未構成decoderはHALの8スロットを占有し続けず、後続headerを探せる。
            assertEquals(3, fixture.pendingEvents(false).size)
            samples.take(30).forEach {
                assertEquals(1, Shadow.extract<NativeMediaEvent>(it).releases)
            }
            assertTrue(fixture.failures.isEmpty())
            val arm =
                decoder.javaClass.superclass
                    .getDeclaredMethod("armStartupDeadline")
                    .apply { isAccessible = true }
            fixture.executor.callControl(5_000L) { arm.invoke(decoder) }
            Shadows.shadowOf(Looper.getMainLooper()).idleFor(6, TimeUnit.SECONDS)
            fixture.drain()
            assertTrue(fixture.unavailable.any { it.reason == PlaybackPipeline.PlaybackUnavailableReason.CODEC_CONFIG_TIMEOUT })
            fixture.assertStoppedVideo(decoder, video, samples)
            samples.forEach {
                val native = Shadow.extract<NativeMediaEvent>(it)
                assertEquals(1, native.releases)
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
            fixture.selectExactMode()
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
            fixture.selectExactMode()
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
    fun exactOutputFollowedByCompatibilityDowngradeCancelsFinalOutputDeadline() {
        Fixture().use { fixture ->
            fixture.decoder()
            val codec = fixture.attachCodec()
            fixture.armCompatibilityAvailability(7L, exact = true)
            val arm = ReflectionHelpers.getField<Any>(fixture.pipeline, "waitingAvailabilityArm")
            val sequence = ReflectionHelpers.getField<Long>(arm, "armSequence")
            // 初回Exact成功を本番入口へ配送し、stock SDKで次のprivate armが成立しない経路を通す。
            fixture.invoke("commitVideoAvailability", requireNotNull(fixture.availabilitySync), 7L, sequence)
            fixture.drain()
            assertEquals(listOf(7L), fixture.available)
            assertEquals(
                "MEDIA_CODEC_TO_MEDIASYNC_INPUT_COMPAT",
                ReflectionHelpers.getField<Any>(fixture.pipeline, "videoAvailabilityMode").toString(),
            )
            assertEquals(null, ReflectionHelpers.getField<Runnable?>(fixture.pipeline, "videoOutputStallTask"))
            val listener = ReflectionHelpers.getField<MediaCodec.OnFrameRenderedListener>(codec, "mOnFrameRenderedListener")
            repeat(7) {
                listener.onFrameRendered(codec, 90_000L + it, System.nanoTime())
                fixture.drain()
                Shadows.shadowOf(Looper.getMainLooper()).idleFor(Duration.ofSeconds(1))
                fixture.drain()
            }
            assertTrue(fixture.unavailable.isEmpty())
            assertEquals(7L, ReflectionHelpers.getField<Long>(fixture.pipeline, "playbackGeneration"))
            assertEquals(listOf(7L), fixture.available)
        }
    }

    @Test
    fun stockAndroid15WithoutPlatformPatchSelectsCodecCallbackFallback() {
        // Robolectric Android 15はMediaSyncの任意private拡張を持たない。
        // 実装はこのAPIを必須にせず公開MediaCodec callbackを使用する。
        assertFalse(MediaSyncFirstOutputBridge.isAvailable())
    }

    @Test
    fun codecReleaseReturnsInputWhoseIndexWasNotReturned() {
        Fixture().use { fixture ->
            fixture.decoder()
            val codec = fixture.attachCodec()
            val filter = fixture.avFilter(false)
            val packet = mappedMediaEvent()
            deliver(filter, packet)
            fixture.drain()
            postCodecCallback(codec, 1, 0, null)
            assertTrue(Shadow.extract<NativeCodec>(codec).inputQueued.await(5, TimeUnit.SECONDS))
            fixture.drain()
            val event = Shadow.extract<NativeMediaEvent>(packet)
            val block = Shadow.extract<NativeLinearBlock>(requireNotNull(event.block))
            assertEquals(0, event.releases)
            assertEquals(0, block.recycles)
            fixture.pipeline.stop()
            assertEquals(1, event.releases)
            assertEquals(1, block.recycles)
            fixture.pipeline.stop()
            assertEquals(1, event.releases)
            assertEquals(1, block.recycles)
        }
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
            assertEquals(0, Shadow.extract<NativeMediaEvent>(packet).releases)
            val block = requireNotNull(Shadow.extract<NativeMediaEvent>(packet).block)
            assertEquals(0, Shadow.extract<NativeLinearBlock>(block).recycles)
            // Codecが同じ入力indexを返すまではHALの共有slotを再利用させない。
            returnCodecInputAndAwaitEventRelease(codec, packet)
            fixture.drain()
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
            fixture.armCompatibilityAvailability(7L)
            // 中間SurfaceのCCodec通知はrelease時のPTS(ns)を返す。monotonic時刻ではない。
            listener.onFrameRendered(codec, 90_000L, 90_000_000L)
            fixture.drain()
            assertEquals(listOf(7L), fixture.available)
            assertEquals(logCountBefore + 1, availabilityLogMessages().size)
            assertTrue(availabilityLogMessages().last().contains("COMPAT"))
            assertEquals(null, ReflectionHelpers.getField<Runnable?>(fixture.pipeline, "videoOutputStallTask"))
            listener.onFrameRendered(codec, 90_000L, Long.MAX_VALUE)
            fixture.drain()
            assertEquals(listOf(7L), fixture.available)
            fixture.armCompatibilityAvailability(8L)
            listener.onFrameRendered(codec, 90_000L, System.nanoTime())
            fixture.drain()
            assertEquals(listOf(7L), fixture.available)
            assertEquals(2L, fixture.progress().count(PlaybackDataPathProgress.Stage.FRAME_RENDERED, false))
        }
    }

    @Test
    fun compatibilityRejectsCallbackReceivedBeforeRearm() {
        Fixture().use { fixture ->
            fixture.decoder()
            val codec = fixture.attachCodec()
            fixture.armCompatibilityAvailability(7L)
            val listener = ReflectionHelpers.getField<MediaCodec.OnFrameRenderedListener>(codec, "mOnFrameRenderedListener")
            val entered = CountDownLatch(1)
            val resume = CountDownLatch(1)
            fixture.executor.executeControl {
                entered.countDown()
                check(resume.await(5, TimeUnit.SECONDS))
                fixture.armCompatibilityAvailability(7L)
            }
            try {
                assertTrue(entered.await(5, TimeUnit.SECONDS))
                // PTSが大きくても、re-armより前に受け取った通知を後から採用しない。
                listener.onFrameRendered(codec, 90_000L, Long.MAX_VALUE)
            } finally {
                resume.countDown()
            }
            fixture.drain()
            assertTrue(fixture.available.isEmpty())
            listener.onFrameRendered(codec, 90_000L, 90_000_000L)
            fixture.drain()
            assertEquals(listOf(7L), fixture.available)
        }
    }

    @Test
    fun compatibilityRejectsKnownSurfaceFailureAndExactModeRejectsCodecOnlyEvidence() {
        for (mode in listOf("surface-failed", "surface-invalid", "exact")) {
            Fixture().use { fixture ->
                fixture.decoder()
                val codec = fixture.attachCodec()
                val armedAt = fixture.armCompatibilityAvailability(7L, exact = mode == "exact")
                if (mode == "surface-failed") {
                    fixture.invoke(
                        "handleMediaSyncError",
                        requireNotNull(fixture.availabilitySync),
                        7L,
                        MediaSync.MEDIASYNC_ERROR_SURFACE_FAIL,
                        0,
                    )
                } else if (mode == "surface-invalid") {
                    requireNotNull(fixture.codecSurface).release()
                }
                val listener = ReflectionHelpers.getField<MediaCodec.OnFrameRenderedListener>(codec, "mOnFrameRenderedListener")
                listener.onFrameRendered(codec, 90_000L, armedAt + 1L)
                fixture.drain()
                assertTrue(fixture.available.isEmpty(), mode)
                assertEquals(null, ReflectionHelpers.getField<Runnable?>(fixture.pipeline, "videoOutputStallTask"), mode)
            }
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

    @Test
    fun sameAudioSelectionAndPidOnlySwitchKeepVideoAndMediaSync() {
        Fixture().use { fixture ->
            fixture.decoder(false)
            fixture.decoder(true)
            val video = fixture.avFilter(false)
            val oldAudio = fixture.avFilter(true)
            val decoder = ReflectionHelpers.getField<Any>(fixture.pipeline, "videoDecoder")
            val audioDecoder = ReflectionHelpers.getField<Any>(fixture.pipeline, "audioDecoder")
            val selection = fixture.audioSelection()
            assertTrue(fixture.pipeline.switchAudio(fixture.tuner, selection).switchedAudio)
            assertEquals(0, Shadow.extract<NativeFilter>(oldAudio).closes)
            val next = selection.copy(audio = selection.audio!!.copy(elementaryPid = TsPid(0x111)))
            assertTrue(fixture.pipeline.switchAudio(fixture.tuner, next).switchedAudio)
            assertEquals(1, Shadow.extract<NativeFilter>(oldAudio).closes)
            assertEquals(0, Shadow.extract<NativeFilter>(video).closes)
            assertTrue(decoder === ReflectionHelpers.getField<Any>(fixture.pipeline, "videoDecoder"))
            assertTrue(audioDecoder === ReflectionHelpers.getField<Any>(fixture.pipeline, "audioDecoder"))
            assertEquals(7L, fixture.pipeline.currentPlaybackGenerationForTest())
            val stale = mappedMediaEvent()
            deliver(oldAudio, stale)
            fixture.drain()
            assertEquals(0, Shadow.extract<NativeMediaEvent>(stale).blockReads)
            assertEquals(1, Shadow.extract<NativeMediaEvent>(stale).releases)
        }
    }

    @Test
    fun failedAudioPidSwitchCanRecoverSameSelectionWithoutReplacingVideo() {
        for (stage in listOf("close", "open", "configure", "start")) {
            Fixture().use { fixture ->
                fixture.decoder(false)
                fixture.decoder(true)
                val video = fixture.avFilter(false)
                val oldAudio = fixture.avFilter(true)
                val decoder = ReflectionHelpers.getField<Any>(fixture.pipeline, "audioDecoder")
                val selection = fixture.audioSelection()
                val next = selection.copy(audio = selection.audio!!.copy(elementaryPid = TsPid(0x111)))
                val nativeTuner = Shadow.extract<NativeTuner>(fixture.tuner)
                val nativeOld = Shadow.extract<NativeFilter>(oldAudio)
                if (stage == "close") {
                    nativeOld.closeResult = Tuner.RESULT_UNKNOWN_ERROR
                } else {
                    nativeTuner.failNextOpenAt = stage
                }
                val session = fixture.audioSession(selection, next)
                val oldTrack = TunerSelectionPolicy.trackIdForAudio(selection.audio!!)
                val newTrack = TunerSelectionPolicy.trackIdForAudio(next.audio!!)
                assertFalse(fixture.selectAudio(session, newTrack), stage)
                assertEquals(oldTrack, ReflectionHelpers.getField<String>(session, "preferredAudioTrackId"), stage)
                assertTrue(ReflectionHelpers.getField<PlaybackStartState>(session, "playbackState") is PlaybackStartState.Started, stage)
                assertEquals(null, ReflectionHelpers.getField<Filter?>(fixture.pipeline, "audioFilter"), stage)
                assertEquals(selection, ReflectionHelpers.getField<Any>(fixture.pipeline, "activeSelection"), stage)
                val cleanup = ReflectionHelpers.getField<ResourceCleanup>(fixture.pipeline, "resourceCleanup")
                assertEquals(stage == "close", cleanup.hasPending, stage)
                val stale = mappedMediaEvent()
                deliver(oldAudio, stale)
                fixture.drain()
                assertEquals(0, Shadow.extract<NativeMediaEvent>(stale).blockReads, stage)
                assertEquals(1, Shadow.extract<NativeMediaEvent>(stale).releases, stage)
                nativeOld.closeResult = Tuner.RESULT_SUCCESS
                // Sessionが復元した旧選択も実Filter不在ならnoopにせず再取得する。
                assertTrue(fixture.selectAudio(session, oldTrack), stage)
                assertTrue(ReflectionHelpers.getField<PlaybackStartState>(session, "playbackState") is PlaybackStartState.Started, stage)
                assertFalse(cleanup.hasPending, stage)
                val recovered = ReflectionHelpers.getField<Filter>(fixture.pipeline, "audioFilter")
                assertTrue(recovered !== oldAudio, stage)
                val sample = mappedMediaEvent()
                deliver(recovered, sample)
                fixture.drain()
                assertEquals(1, Shadow.extract<NativeMediaEvent>(sample).blockReads, stage)
                assertEquals(0, Shadow.extract<NativeFilter>(video).closes, stage)
                assertTrue(decoder === ReflectionHelpers.getField<Any>(fixture.pipeline, "audioDecoder"), stage)
                assertEquals(7L, fixture.pipeline.currentPlaybackGenerationForTest(), stage)
                assertTrue(fixture.selectAudio(session, newTrack), stage)
                assertEquals(next, ReflectionHelpers.getField<Any>(fixture.pipeline, "activeSelection"), stage)
            }
        }
    }

    private fun availabilityLogMessages(): List<String> =
        ShadowLog
            .getLogsForTag(LogTags.TIS)
            .map { it.msg }
            .filter { it.startsWith("VIDEO_AVAILABILITY_COMMITTED inputId=admission-test ") }

    private fun Fixture.attachCodec(
        audio: Boolean = false,
        videoKind: PlaybackPipeline.VideoCodecKind = PlaybackPipeline.VideoCodecKind.AVC,
    ): MediaCodec {
        val codecName = if (audio) "maleicacid.test.mpeg.decoder" else "maleicacid.test.${videoKind.name.lowercase()}.decoder"
        val caps =
            if (audio) {
                MediaFormat.createAudioFormat(MediaFormat.MIMETYPE_AUDIO_MPEG, 44_100, 2)
            } else {
                MediaFormat.createVideoFormat(videoKind.mime, 4096, 4096)
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
            // このfixtureの出力callback試験は起動済み再生を扱う。pause中の試験は別fixtureで行う。
            ReflectionHelpers.setField(pipeline, "mediaSyncStarted", true)
        }
        val method =
            decoder.javaClass.superclass
                .getDeclaredMethod("createBlockModelDecoder", MediaFormat::class.java)
                .apply { isAccessible = true }
        val format =
            if (audio) {
                MediaFormat.createAudioFormat(MediaFormat.MIMETYPE_AUDIO_MPEG, 44_100, 2)
            } else {
                MediaFormat.createVideoFormat(videoKind.mime, 32, 32)
            }
        val codec = method.invoke(decoder, format) as MediaCodec
        ReflectionHelpers.setField(decoder, "codec", codec)
        return codec
    }

    private fun returnCodecInputAndAwaitEventRelease(
        codec: MediaCodec,
        event: MediaEvent,
    ) {
        val released = CountDownLatch(1)
        Shadow.extract<NativeMediaEvent>(event).onRelease = { released.countDown() }
        postCodecCallback(codec, 1, 0, null)
        // CodecのHandlerからownerへ届く前に、ownerだけのdrainが完了し得る。
        assertTrue(released.await(5, TimeUnit.SECONDS), "returned Codec input must release its MediaEvent")
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

        private val sessionCaptionExecutors = mutableListOf<LifecycleSerialExecutor>()

        // Android Session通知と本番Session→Controller→Pipelineを通し、字幕native初期化だけを省く。
        // 同じSessionの選択・通知・字幕寿命を一つのfixtureとして初期化する。
        @Suppress("LongMethod")
        fun audioSession(
            selection: TunerController.AvStreamSelection,
            next: TunerController.AvStreamSelection,
        ): MaleicacidLiveSession {
            val unsafeType = Class.forName("sun.misc.Unsafe")
            val unsafe = unsafeType.getDeclaredField("theUnsafe").apply { isAccessible = true }.get(null)
            val allocate = unsafeType.getMethod("allocateInstance", Class::class.java)
            val session = allocate.invoke(unsafe, MaleicacidLiveSession::class.java) as MaleicacidLiveSession
            val controller = allocate.invoke(unsafe, TunerController::class.java) as TunerController
            ReflectionHelpers.setField(controller, "currentTune", ReflectionHelpers.getField<Any>(pipeline, "activeChannel"))
            ReflectionHelpers.setField(controller, "tuner", tuner)
            ReflectionHelpers.setField(controller, "playbackPipeline", pipeline)
            ReflectionHelpers.setField(session, "tunerController", controller)
            ReflectionHelpers.setField(session, "currentService", selection.serviceKey)
            ReflectionHelpers.setField(session, "dualMonoPresentation", PlaybackPipeline.DualMonoPresentation.MAIN)
            ReflectionHelpers.setField(session, "preferredAudioTrackId", TunerSelectionPolicy.trackIdForAudio(selection.audio!!))
            val programs =
                com.maleicacid.tvinput.aribsi.ProgramPublishSnapshot(
                    discoveryProfile = 0,
                    ingestSequence = 0L,
                    events = emptyList(),
                    updateWindows = emptyList(),
                    semanticFactsByServiceKey = emptyMap(),
                    descriptorDiagnostics = emptyList(),
                    parserDiagnostics = emptyList(),
                )
            val service =
                com.maleicacid.tvinput.aribsi.AribService(
                    selection.serviceKey,
                    "test",
                    serviceType = 1,
                    pcrPid = selection.pcrPid,
                    streams = listOfNotNull(selection.video, selection.audio, next.audio),
                )
            ReflectionHelpers.setField(
                session,
                "latestLiveSnapshot",
                com.maleicacid.tvinput.aribsi.LivePlaybackSnapshot(
                    0L,
                    programs,
                    0L,
                    listOf(service),
                    emptyList(),
                    emptyMap(),
                    emptyList(),
                    emptyMap(),
                    emptyList(),
                    emptyList(),
                ),
            )
            val signature =
                AvPlaybackSignature(
                    selection.serviceKey,
                    selection.pcrPid,
                    selection.video?.elementaryPid,
                    selection.video?.streamType,
                    selection.audio?.elementaryPid,
                    selection.audio?.streamType,
                    true,
                    false,
                )
            ReflectionHelpers.setField(session, "playbackState", PlaybackStartState.Started(signature, 7L))
            for (name in listOf("captionController", "superimposeController")) {
                val caption = allocate.invoke(unsafe, AribCaptionController::class.java) as AribCaptionController
                ReflectionHelpers.setField(caption, "released", AtomicBoolean(true))
                ReflectionHelpers.setField(caption, "pesAdmissionLock", Any())
                val captionExecutor = LifecycleSerialExecutor("closed audio-switch caption fixture")
                sessionCaptionExecutors += captionExecutor
                ReflectionHelpers.setField(caption, "executor", captionExecutor)
                ReflectionHelpers.setField(session, name, caption)
            }
            TvInputService.Session::class.java
                .getDeclaredField("mLock")
                .apply { isAccessible = true }
                .set(session, Any())
            TvInputService.Session::class.java
                .getDeclaredField(
                    "mPendingActions",
                ).apply { isAccessible = true }
                .set(session, mutableListOf<Runnable>())
            return session
        }

        fun selectAudio(
            session: MaleicacidLiveSession,
            id: String,
        ): Boolean =
            MaleicacidLiveSession::class.java
                .getDeclaredMethod(
                    "onSelectTrackOnSessionExecutor",
                    Int::class.javaPrimitiveType,
                    String::class.java,
                ).apply { isAccessible = true }
                .invoke(session, TvTrackInfo.TYPE_AUDIO, id) as Boolean

        fun audioSelection(): TunerController.AvStreamSelection {
            val key = ServiceKey(4, 0x4010, 101)
            val channel =
                TunerController.ResolvedChannel(
                    null,
                    "admission-test",
                    key,
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
                    key,
                    TsPid(0x100),
                    AribElementaryStream(TsPid(0x101), 0x1b, null, null, null),
                    AribElementaryStream(TsPid(0x110), 0x0f, null, null, null),
                )
            executor.callControl(5_000L) {
                ReflectionHelpers.setField(pipeline, "activeChannel", channel)
                ReflectionHelpers.setField(pipeline, "activeSelection", selection)
            }
            return selection
        }

        fun released(): Boolean = ReflectionHelpers.getField<AtomicBoolean>(pipeline, "released").get()

        fun progress(): PlaybackDataPathProgress = ReflectionHelpers.getField(pipeline, "dataPathProgress")

        var availabilitySync: MediaSync? = null

        // native初期化だけを省略し、stockでのmode選択とarmは本番処理を通す。
        fun armCompatibilityAvailability(
            generation: Long,
            exact: Boolean = false,
        ): Long {
            val sync =
                availabilitySync ?: run {
                    val type = Class.forName("sun.misc.Unsafe")
                    val unsafe = type.getDeclaredField("theUnsafe").apply { isAccessible = true }.get(null)
                    (type.getMethod("allocateInstance", Class::class.java).invoke(unsafe, MediaSync::class.java) as MediaSync)
                        .also { availabilitySync = it }
                }
            return executor.callControl(5_000L) {
                ReflectionHelpers.setField(pipeline, "surface", requireNotNull(codecSurface))
                ReflectionHelpers.setField(pipeline, "videoPathExpected", true)
                ReflectionHelpers.setField(pipeline, "mediaSync", sync)
                ReflectionHelpers.setField(pipeline, "playbackGeneration", generation)
                invoke("armVideoAvailability", sync, generation, true)
                val selected = ReflectionHelpers.getField<Any>(pipeline, "videoAvailabilityMode")
                assertEquals("MEDIA_CODEC_TO_MEDIASYNC_INPUT_COMPAT", selected.toString())
                if (exact) {
                    selectExactMode()
                }
                val arm = ReflectionHelpers.getField<Any>(pipeline, "waitingAvailabilityArm")
                ReflectionHelpers.getField<Long>(arm, "armedAtNanoTime")
            }
        }

        fun selectExactMode() {
            val type = PlaybackPipeline::class.java.getDeclaredField("videoAvailabilityMode").type
            val mode = requireNotNull(type.enumConstants).single { it.toString() == "MEDIA_SYNC_FINAL_OUTPUT_EXACT" }
            ReflectionHelpers.setField(pipeline, "videoAvailabilityMode", mode)
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

        fun decoder(
            audio: Boolean = false,
            videoKind: PlaybackPipeline.VideoCodecKind = PlaybackPipeline.VideoCodecKind.AVC,
        ) {
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
                        videoKind,
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
            sessionCaptionExecutors.forEach { it.shutdownNow() }
            // このfixtureのMediaSyncはnative資源を確保していない。
            if (availabilitySync != null) ReflectionHelpers.setField(pipeline, "mediaSync", null)
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

    private fun mediaEvent(
        length: Long,
        pts90k: Long = 0L,
    ): MediaEvent =
        MediaEvent::class.java.declaredConstructors
            .single { it.parameterCount == 15 }
            .apply { isAccessible = true }
            .newInstance(
                0,
                pts90k != 0L,
                pts90k,
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
        var failNextOpenAt: String? = null

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
        ): Filter? {
            val failure = failNextOpenAt
            failNextOpenAt = null
            if (failure == "open") return null
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
            val native = Shadow.extract<NativeFilter>(filter)
            if (failure == "configure") native.configureResult = Tuner.RESULT_UNKNOWN_ERROR
            if (failure == "start") native.startResult = Tuner.RESULT_UNKNOWN_ERROR
            return filter
        }
    }

    @Implements(Filter::class)
    class NativeFilter {
        var configureResult = Tuner.RESULT_SUCCESS
        var startResult = Tuner.RESULT_SUCCESS
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
        ): Int = configureResult

        @Implementation
        fun nativeStartFilter(): Int = startResult

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
        var onOutputRelease: () -> Unit = {}

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
            onOutputRelease()
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
        var data: ByteArray = byteArrayOf(0)

        @Implementation
        fun map(): ByteBuffer = ByteBuffer.wrap(data)

        @Implementation
        fun recycle() {
            recycles++
        }
    }
}

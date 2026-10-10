// 実SDK callback lock・時計・MediaEvent寿命を使い、HAL/native境界だけを置換する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import android.content.Context
import android.media.MediaCodec
import android.media.tv.tuner.Tuner
import android.media.tv.tuner.TunerUtils
import android.media.tv.tuner.filter.Filter
import android.media.tv.tuner.filter.FilterCallback
import android.media.tv.tuner.filter.FilterConfiguration
import android.media.tv.tuner.filter.FilterEvent
import android.media.tv.tuner.filter.MediaEvent
import android.media.tv.tuner.filter.PesEvent
import android.media.tv.tuner.filter.RestartEvent
import android.view.Surface
import com.maleicacid.tvinput.aribsi.AribCodecFacts
import com.maleicacid.tvinput.aribsi.AribElementaryStream
import com.maleicacid.tvinput.common.TsPid
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.annotation.Implementation
import org.robolectric.annotation.Implements
import org.robolectric.shadow.api.Shadow
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
                deliver(filter, newInput)
                fixture.drain()
                assertEquals(listOf(newInput), fixture.pendingEvents(audio))
                assertEquals(1, Shadow.extract<NativeMediaEvent>(newInput).blockReads)
                assertEquals(0, Shadow.extract<NativeMediaEvent>(newInput).releases)
                assertEquals(7L, fixture.pipeline.currentPlaybackGenerationForTest())
                assertTrue(decoder === ReflectionHelpers.getField<Any>(fixture.pipeline, decoderField))
                assertEquals(0, native.closes)
                // 対象Android 15のFilter.onFilterStatusはcallbackを2回配送する。
                assertEquals(2, native.flushes)
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
                native.flushResult = Tuner.RESULT_SUCCESS
                status(filter, Filter.STATUS_OVERFLOW)
                fixture.drain()
                val accepted = mappedMediaEvent()
                deliver(filter, accepted)
                fixture.drain()
                assertEquals(listOf(accepted), fixture.pendingEvents(audio))
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
            val third = fixture.capacityLoss(video)
            assertEquals(1, fixture.failures.size)
            assertEquals(PlaybackPipeline.PlaybackUnavailableReason.VIDEO_CODEC_ERROR, fixture.failures.single().first)
            assertTrue(
                fixture.failures
                    .single()
                    .second
                    .contains("OWNER_INPUT_FULL"),
            )
            assertFalse(fixture.released())
            for (event in listOf(first, second, third)) {
                assertEquals(1, Shadow.extract<NativeMediaEvent>(event).releases)
            }
        }
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

    private inner class Fixture : AutoCloseable {
        val pipeline = PlaybackPipeline("admission-test", null)
        val executor = ReflectionHelpers.getField<LifecycleSerialExecutor>(pipeline, "executor")
        val tuner = Tuner(RuntimeEnvironment.getApplication(), null, 0)
        val failures = mutableListOf<Pair<PlaybackPipeline.PlaybackUnavailableReason, String>>()
        val unavailable = mutableListOf<PlaybackPipeline.PlaybackUnavailable>()
        val captionPayloads = mutableListOf<ByteArray>()
        var captionDiscontinuities = 0

        init {
            ReflectionHelpers.setField(pipeline, "playbackGeneration", 7L)
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
            pipeline.close()
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

    @Implements(MediaCodec.LinearBlock::class)
    class NativeLinearBlock {
        @Implementation
        fun map(): ByteBuffer = ByteBuffer.wrap(byteArrayOf(0))
    }
}

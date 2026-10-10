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
import java.time.Duration
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executor
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
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
    ],
)
// 同じproduction ownerとAndroid境界fixtureを共有する。
@Suppress("TooManyFunctions", "LargeClass")
class PlaybackFilterAdmissionTest {
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
    fun transientVideoAdmissionLossRecoversAndPersistentLossUsesExistingDeadline() {
        Fixture().use { fixture ->
            fixture.decoder()
            val video = fixture.avFilter(false)
            val first = mediaEvent(16L * 1024 * 1024)
            fixture.whileOwnerBlocked { deliver(video, first) }
            fixture.drain()
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
            val second = mediaEvent(16L * 1024 * 1024)
            deliver(video, second)
            fixture.drain()
            assertTrue(fixture.failures.isEmpty())
            ShadowSystemClock.advanceBy(Duration.ofSeconds(6))
            val third = mediaEvent(16L * 1024 * 1024)
            deliver(video, third)
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
            for (event in listOf(first, second, third)) assertEquals(1, Shadow.extract<NativeMediaEvent>(event).releases)
        }
    }

    @Test
    fun captionCapacityLossAndStaleFilterDoNotTerminateCurrentVideo() {
        Fixture().use { fixture ->
            val video = fixture.avFilter(false)
            val caption = fixture.captionFilter()
            fixture.whileOwnerBlocked {
                // 同じcallback内のevent帳簿だけでも有限予算を超える入力は局所回収する。
                deliver(caption, Array(16 * 1024) { restartEvent() })
            }
            fixture.drain()
            assertEquals(1, fixture.captionDiscontinuities)
            assertEquals(1, Shadow.extract<NativeFilter>(caption).flushes)
            val replacement = fixture.avFilter(false)
            val stale = mediaEvent(16L * 1024 * 1024)
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
    fun actualOwnerExecutionFailureNotifiesOnceAndCleansOnSameOwner() {
        Fixture().use { fixture ->
            val video = fixture.avFilter(false)
            fixture.invoke("enqueuePlaybackAction", { error("actual callback failure") })
            assertTrue(fixture.executor.awaitTermination(5, TimeUnit.SECONDS))
            assertTrue(fixture.released())
            assertEquals(1, fixture.unavailable.size)
            assertEquals(PlaybackPipeline.PlaybackUnavailableReason.PLAYBACK_RECOVERY_FAILED, fixture.unavailable.single().reason)
            val native = Shadow.extract<NativeFilter>(video)
            assertEquals(1, native.closes)
            assertTrue(native.closeThread.orEmpty().startsWith("maleicacid-playback-"))
        }
    }

    private class Fixture : AutoCloseable {
        val pipeline = PlaybackPipeline("admission-test", null)
        val executor = ReflectionHelpers.getField<LifecycleSerialExecutor>(pipeline, "executor")
        val tuner = Tuner(RuntimeEnvironment.getApplication(), null, 0)
        val failures = mutableListOf<Pair<PlaybackPipeline.PlaybackUnavailableReason, String>>()
        val unavailable = mutableListOf<PlaybackPipeline.PlaybackUnavailable>()
        var captionDiscontinuities = 0

        init {
            ReflectionHelpers.setField(pipeline, "playbackGeneration", 7L)
            ReflectionHelpers.setField(pipeline, "onVideoUnavailable", { failure: PlaybackPipeline.PlaybackUnavailable ->
                unavailable +=
                    failure
            })
            ReflectionHelpers.setField(pipeline, "onSubtitleContinuityLost", { _: Long, _: String -> captionDiscontinuities++ })
        }

        fun released(): Boolean = ReflectionHelpers.getField<AtomicBoolean>(pipeline, "released").get()

        fun avFilter(audio: Boolean): Filter {
            val stream = AribElementaryStream(TsPid(if (audio) 0x102 else 0x101), if (audio) 0x0f else 0x1b, null, null, null)
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

        fun decoder() {
            val type = Class.forName("com.maleicacid.tvinput.tis.PlaybackPipeline\$VideoDecoderPipeline")
            val constructor = type.declaredConstructors.single().apply { isAccessible = true }
            val sink: (PlaybackPipeline.PlaybackUnavailableReason, String) -> Unit = { reason, detail -> failures += reason to detail }
            val decoder = constructor.newInstance(pipeline, PlaybackPipeline.VideoCodecKind.AVC, AribCodecFacts(), Surface(), 7L, sink)
            ReflectionHelpers.setField(pipeline, "videoDecoder", decoder)
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

    private fun mediaEvent(length: Long): MediaEvent =
        MediaEvent::class.java.declaredConstructors
            .single { it.parameterCount == 15 }
            .apply { isAccessible = true }
            .newInstance(0, false, 0L, false, 0L, length, 0L, null, false, 0L, 0, false, 0, null, emptyList<Any>()) as MediaEvent

    private fun restartEvent(): RestartEvent =
        RestartEvent::class.java.declaredConstructors.single { it.parameterCount == 1 }.apply { isAccessible = true }.newInstance(
            0,
        ) as RestartEvent

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
            return Tuner.RESULT_SUCCESS
        }

        @Implementation
        fun nativeClose(): Int {
            closes++
            closeThread = Thread.currentThread().name
            return Tuner.RESULT_SUCCESS
        }
    }

    @Implements(MediaEvent::class)
    class NativeMediaEvent {
        var releases = 0
        var blockReads = 0
        var releaseThread: String? = null

        @Implementation
        fun nativeGetLinearBlock(): MediaCodec.LinearBlock? {
            blockReads++
            return null
        }

        @Implementation
        fun nativeFinalize() {
            releases++
            releaseThread = Thread.currentThread().name
        }
    }
}

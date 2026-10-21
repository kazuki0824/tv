package com.maleicacid.tvinput.tis

import android.graphics.SurfaceTexture
import android.media.MediaCodec
import android.media.MediaSync
import android.media.PlaybackParams
import android.os.Handler
import android.view.Surface
import com.maleicacid.tvinput.aribsi.AribCodecFacts
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.Implementation
import org.robolectric.annotation.Implements
import org.robolectric.shadow.api.Shadow
import org.robolectric.shadows.ShadowMediaCodec
import org.robolectric.util.ReflectionHelpers
import java.util.IdentityHashMap
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertTrue

@RunWith(RobolectricTestRunner::class)
@Config(
    sdk = [35],
    shadows = [PlaybackMediaSyncReleaseTest.NativeMediaSync::class, PlaybackMediaSyncReleaseTest.NativeCodec::class],
)
class PlaybackMediaSyncReleaseTest {
    @Before
    fun reset() {
        NativeMediaSync.owners.clear()
        NativeCodec.sync = null
    }

    @Test
    fun stoppedMediaSyncDisconnectsBeforeReleaseAndSurfaceCanBeReused() {
        withAttachedSync { pipeline, native, surface ->
            pipeline.stop()
            assertEquals(listOf("connect", "flush", "disconnect", "release"), native.events)
            assertFalse(NativeMediaSync.owners.containsKey(surface))

            val nextSync = MediaSync()
            nextSync.setSurface(surface)
            assertTrue(NativeMediaSync.owners.containsKey(surface))
            nextSync.setSurface(null)
            nextSync.release()
        }
    }

    @Test
    fun failedSurfaceDisconnectRetainsReleaseObligationUntilRetry() {
        withAttachedSync { pipeline, native, surface ->
            native.failNextDisconnect = true
            native.failNextRelease = true
            assertFailsWith<IllegalStateException> { pipeline.stop() }
            assertEquals(listOf("connect", "flush"), native.events)
            assertTrue(NativeMediaSync.owners.containsKey(surface))
            assertTrue(ReflectionHelpers.getField<ResourceCleanup>(pipeline, "resourceCleanup").hasPending)

            val attemptedSync = MediaSync()
            assertFailsWith<IllegalArgumentException> { attemptedSync.setSurface(surface) }
            attemptedSync.release()

            pipeline.stop()
            assertEquals(listOf("connect", "flush", "disconnect", "release"), native.events)
            assertFalse(NativeMediaSync.owners.containsKey(surface))
            assertFalse(ReflectionHelpers.getField<ResourceCleanup>(pipeline, "resourceCleanup").hasPending)
        }
    }

    @Test
    fun mediaSyncIsReleasedBeforeDecoderStop() {
        withAttachedSync { pipeline, native, surface ->
            attachVideoDecoder(pipeline, native, surface)
            pipeline.stop()
            val expected = listOf("connect", "flush", "disconnect", "release", "codec-stop", "codec-release")
            assertEquals(expected, native.events)
            assertFalse(NativeMediaSync.owners.containsKey(surface))
        }
    }

    @Test
    fun flushFailureStillReleasesSyncBeforeReturningDecoderResources() {
        withAttachedSync { pipeline, native, surface ->
            attachVideoDecoder(pipeline, native, surface)
            native.failNextFlush = true
            pipeline.stop()
            assertEquals(listOf("connect", "disconnect", "release", "codec-stop", "codec-release"), native.events)
            assertFalse(NativeMediaSync.owners.containsKey(surface))
            assertFalse(ReflectionHelpers.getField<ResourceCleanup>(pipeline, "resourceCleanup").hasPending)
        }
    }

    @Test
    fun failedFlushAndReleaseKeepDecoderUntilRetrySucceeds() {
        withAttachedSync { pipeline, native, surface ->
            val decoder = attachVideoDecoder(pipeline, native, surface)
            repeat(2) {
                native.failNextFlush = true
                native.failNextRelease = true
                assertFailsWith<IllegalStateException> { pipeline.stop() }
                assertFalse("codec-stop" in native.events)
                assertFalse("codec-release" in native.events)
                assertTrue(decoder === ReflectionHelpers.getField<Any>(pipeline, "videoDecoder"))
            }
            pipeline.stop()
            assertEquals(1, native.events.count { it == "codec-release" })
            assertTrue(native.events.indexOf("release") < native.events.indexOf("codec-stop"))
            assertFalse(ReflectionHelpers.getField<ResourceCleanup>(pipeline, "resourceCleanup").hasPending)
        }
    }

    @Test
    fun casPendingStopDoesNotAdvanceGenerationWhenPlaybackAlreadyStopped() {
        withAttachedSync { pipeline, native, _ ->
            val before = pipeline.currentPlaybackGenerationForTest()
            assertTrue(pipeline.stopIfActive())
            val after = pipeline.currentPlaybackGenerationForTest()
            assertTrue(after > before)
            assertFalse(pipeline.stopIfActive())
            assertEquals(after, pipeline.currentPlaybackGenerationForTest())
            assertEquals(1, native.events.count { it == "flush" })
        }
    }

    @Test
    fun pausedVideoOutputDoesNotBlockStop() {
        withAttachedSync { pipeline, native, surface ->
            val decoder = attachVideoDecoder(pipeline, native, surface)
            val executor = ReflectionHelpers.getField<LifecycleSerialExecutor>(pipeline, "executor")
            executor.executeControl { videoOutput(decoder) }
            try {
                // 停止自身がflushへ到達できることを検査する。模擬native境界はpause中なら待機する。
                executor.callControl(1_000L) { pipeline.stop() }
                assertEquals(1L, native.renderEntered.count)
                assertTrue("codec-release" in native.events)
            } finally {
                native.resumeRender.countDown()
            }
        }
    }

    @Test
    fun activeVideoRenderWaitDoesNotOccupyPlaybackControlOwner() {
        withAttachedSync { pipeline, native, surface ->
            val decoder = attachVideoDecoder(pipeline, native, surface)
            val executor = ReflectionHelpers.getField<LifecycleSerialExecutor>(pipeline, "executor")
            ReflectionHelpers.setField(pipeline, "mediaSyncStarted", true)
            native.playing = true
            native.blockActiveRender = true
            executor.executeControl { videoOutput(decoder) }
            assertTrue(native.renderEntered.await(2, TimeUnit.SECONDS))
            try {
                // releaseOutputBufferのnative待機と同時でも、単一playback ownerはcontrolを進める。
                assertEquals(1L, executor.callControl(1_000L) { pipeline.currentPlaybackGenerationForTest() })
                assertEquals(1L, native.resumeRender.count)
            } finally {
                native.resumeRender.countDown()
            }
        }
    }

    @Test
    fun activeVideoRenderWaitCanBeWokenBeforeStopReachesOwner() {
        withAttachedSync { pipeline, native, surface ->
            val decoder = attachVideoDecoder(pipeline, native, surface)
            val executor = ReflectionHelpers.getField<LifecycleSerialExecutor>(pipeline, "executor")
            ReflectionHelpers.setField(pipeline, "mediaSyncStarted", true)
            native.playing = true
            native.blockActiveRender = true
            executor.executeControl { videoOutput(decoder) }
            assertTrue(native.renderEntered.await(2, TimeUnit.SECONDS))
            try {
                pipeline.stop()
                assertEquals(0L, native.resumeRender.count)
                assertTrue(native.events.indexOf("render") < native.events.indexOf("codec-stop"))
                assertTrue(native.events.indexOf("release") < native.events.indexOf("codec-stop"))
            } finally {
                native.resumeRender.countDown()
            }
        }
    }

    @Test
    fun activeVideoRenderWaitCanBeWokenBeforeReleaseReachesOwner() {
        withAttachedSync { pipeline, native, surface ->
            val decoder = attachVideoDecoder(pipeline, native, surface)
            val executor = ReflectionHelpers.getField<LifecycleSerialExecutor>(pipeline, "executor")
            ReflectionHelpers.setField(pipeline, "mediaSyncStarted", true)
            native.playing = true
            native.blockActiveRender = true
            executor.executeControl { videoOutput(decoder) }
            assertTrue(native.renderEntered.await(2, TimeUnit.SECONDS))
            try {
                pipeline.release()
                assertEquals(0L, native.resumeRender.count)
                assertTrue(native.events.indexOf("release") < native.events.indexOf("codec-stop"))
            } finally {
                native.resumeRender.countDown()
            }
        }
    }

    @Test
    fun pausedVideoOutputIsRenderedOnlyAfterStartupGateOpens() {
        withAttachedSync { pipeline, native, surface ->
            val decoder = attachVideoDecoder(pipeline, native, surface)
            val executor = ReflectionHelpers.getField<LifecycleSerialExecutor>(pipeline, "executor")
            executor.executeControl { videoOutput(decoder) }
            try {
                executor.callControl(1_000L) {
                    assertEquals(1L, native.renderEntered.count)
                    ReflectionHelpers.setField(pipeline, "videoPathExpected", true)
                    ReflectionHelpers.setField(pipeline, "videoInputQueued", true)
                    ReflectionHelpers.callInstanceMethod<Unit>(pipeline, "maybeStartMediaSync")
                }
                assertTrue(native.renderCompleted.await(2, TimeUnit.SECONDS))
                assertTrue(native.events.indexOf("play") < native.events.indexOf("render"))
            } finally {
                native.resumeRender.countDown()
            }
        }
    }

    private fun attachVideoDecoder(
        pipeline: PlaybackPipeline,
        native: NativeMediaSync,
        surface: Surface,
    ): Any {
        val type = Class.forName("com.maleicacid.tvinput.tis.PlaybackPipeline\$VideoDecoderPipeline")
        val constructor = type.declaredConstructors.single().apply { isAccessible = true }
        val sink: (PlaybackPipeline.PlaybackUnavailableReason, String) -> Unit = { _, _ -> }
        val decoder =
            constructor.newInstance(
                pipeline,
                PlaybackPipeline.VideoCodecKind.MPEG2,
                AribCodecFacts(),
                surface,
                1L,
                sink,
            )
        val codec = MediaCodec.createByCodecName("c2.android.mpeg2.decoder")
        ReflectionHelpers.setField(decoder, "codec", codec)
        ReflectionHelpers.setField(pipeline, "videoDecoder", decoder)
        NativeCodec.sync = native
        ReflectionHelpers.setField(pipeline, "playbackGeneration", 1L)
        return decoder
    }

    private fun videoOutput(decoder: Any) {
        val codec = ReflectionHelpers.getField<MediaCodec>(decoder, "codec")
        ReflectionHelpers.callInstanceMethod<Unit>(
            decoder,
            "onOutput",
            ReflectionHelpers.ClassParameter.from(MediaCodec::class.java, codec),
            ReflectionHelpers.ClassParameter.from(Int::class.javaPrimitiveType, 0),
            ReflectionHelpers.ClassParameter.from(
                MediaCodec.BufferInfo::class.java,
                MediaCodec.BufferInfo().apply { set(0, 1, 90_000L, 0) },
            ),
        )
    }

    @Implements(MediaCodec::class)
    class NativeCodec : ShadowMediaCodec() {
        companion object {
            var sync: NativeMediaSync? = null
        }

        @Implementation
        protected override fun releaseOutputBuffer(
            index: Int,
            renderTimestampNs: Long,
        ) {
            val owner = requireNotNull(sync)
            owner.renderEntered.countDown()
            if (!owner.playing || owner.blockActiveRender) check(owner.resumeRender.await(5, TimeUnit.SECONDS))
            owner.events += "render"
            owner.renderCompleted.countDown()
        }

        @Implementation(methodName = "stop")
        fun stopDecoder() {
            val owner = requireNotNull(sync)
            check("release" in owner.events) { "MediaSync still holds decoder output" }
            owner.events += "codec-stop"
        }

        @Implementation(methodName = "release")
        fun releaseDecoder() {
            requireNotNull(sync).events += "codec-release"
        }
    }

    private fun withAttachedSync(action: (PlaybackPipeline, NativeMediaSync, Surface) -> Unit) {
        val texture = SurfaceTexture(0)
        val output = Surface(texture)
        val pipeline = PlaybackPipeline("mediasync-release-test", null)
        val sync = MediaSync()
        val native = Shadow.extract<NativeMediaSync>(sync)
        try {
            sync.setSurface(output)
            ReflectionHelpers.setField(pipeline, "mediaSync", sync)
            action(pipeline, native, output)
        } finally {
            native.failNextDisconnect = false
            runCatching { pipeline.release() }
            NativeMediaSync.owners.clear()
            output.release()
            texture.release()
        }
    }

    @Suppress("UNUSED_PARAMETER")
    @Implements(MediaSync::class)
    class NativeMediaSync {
        companion object {
            val owners = IdentityHashMap<Surface, NativeMediaSync>()
        }

        val events = mutableListOf<String>()
        val renderEntered = CountDownLatch(1)
        val renderCompleted = CountDownLatch(1)
        val resumeRender = CountDownLatch(1)
        var playing = false
        var blockActiveRender = false
        var failNextDisconnect = false
        var failNextRelease = false
        var failNextFlush = false
        private var connectedSurface: Surface? = null

        @Implementation(methodName = "__constructor__")
        private fun construct() = Unit

        @Implementation
        fun setSurface(surface: Surface?) {
            if (surface == null) {
                if (failNextDisconnect) {
                    failNextDisconnect = false
                    error("MediaSync surface disconnect failed")
                }
                connectedSurface?.let(owners::remove)
                connectedSurface = null
                events += "disconnect"
                return
            }
            require(owners[surface] == null) { "surface is already connected: -22" }
            connectedSurface = surface
            owners[surface] = this
            events += "connect"
        }

        @Implementation
        fun setPlaybackParams(params: PlaybackParams) {
            playing = params.speed != 0.0f
            if (playing) events += "play"
        }

        @Implementation
        fun setCallback(
            callback: MediaSync.Callback?,
            handler: Handler?,
        ) = Unit

        @Implementation
        fun setOnErrorListener(
            listener: MediaSync.OnErrorListener?,
            handler: Handler?,
        ) = Unit

        @Implementation
        fun flush() {
            resumeRender.countDown()
            if (failNextFlush) {
                failNextFlush = false
                error("MediaSync flush failed")
            }
            events += "flush"
        }

        @Implementation
        fun release() {
            if (failNextRelease) {
                failNextRelease = false
                error("MediaSync release failed")
            }
            connectedSurface?.let(owners::remove)
            connectedSurface = null
            events += "release"
        }
    }
}

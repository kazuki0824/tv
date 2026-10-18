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
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertTrue

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], shadows = [PlaybackMediaSyncReleaseTest.NativeMediaSync::class, PlaybackMediaSyncReleaseTest.NativeCodec::class])
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
            val failure = assertFailsWith<IllegalStateException> { pipeline.stop() }
            assertTrue(failure.message.orEmpty().contains("MediaSync"))
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
    fun blockedVideoConsumerIsUnblockedBeforeDecoderStop() {
        withAttachedSync { pipeline, native, surface ->
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
            pipeline.stop()
            assertEquals(listOf("connect", "flush", "disconnect", "release", "codec-stop", "codec-release"), native.events)
            assertFalse(NativeMediaSync.owners.containsKey(surface))
        }
    }

    @Test
    fun flushFailureStillReleasesSyncBeforeReturningDecoderResources() {
        withAttachedSync { pipeline, native, surface ->
            native.failNextFlush = true
            pipeline.stop()
            assertEquals(listOf("connect", "disconnect", "release"), native.events)
            assertFalse(NativeMediaSync.owners.containsKey(surface))
            assertFalse(ReflectionHelpers.getField<ResourceCleanup>(pipeline, "resourceCleanup").hasPending)
        }
    }

    @Implements(MediaCodec::class)
    class NativeCodec : ShadowMediaCodec() {
        companion object {
            var sync: NativeMediaSync? = null
        }

        @Implementation(methodName = "stop")
        fun stopDecoder() {
            val owner = requireNotNull(sync)
            check("flush" in owner.events && "release" in owner.events) { "MediaSync still holds decoder output" }
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
        fun setPlaybackParams(params: PlaybackParams) = Unit

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

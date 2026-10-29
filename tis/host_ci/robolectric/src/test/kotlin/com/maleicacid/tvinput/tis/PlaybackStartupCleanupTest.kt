package com.maleicacid.tvinput.tis

import android.graphics.SurfaceTexture
import android.media.MediaSync
import android.media.PlaybackParams
import android.media.tv.tuner.Tuner
import android.media.tv.tuner.filter.Filter
import android.os.Handler
import android.os.Looper
import android.os.Message
import android.view.Surface
import com.maleicacid.tvinput.aribsi.AribElementaryStream
import com.maleicacid.tvinput.common.FrequencyHz
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.StreamSelector
import com.maleicacid.tvinput.common.TsPid
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.annotation.Implementation
import org.robolectric.annotation.Implements
import org.robolectric.shadow.api.Shadow
import org.robolectric.util.ReflectionHelpers
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

@RunWith(RobolectricTestRunner::class)
@Config(
    sdk = [35],
    shadows = [
        PlaybackStartupCleanupTest.NativeMediaSync::class,
        PlaybackFilterAdmissionTest.NativeTuner::class,
        PlaybackFilterAdmissionTest.NativeFilter::class,
    ],
)
// 同じ開始transactionの正常回収と二次失敗を一つのfixtureで検査する。
@Suppress("MagicNumber")
class PlaybackStartupCleanupTest {
    @Test
    fun rejectedStartupDeadlineReclaimsPartiallyStartedResources() {
        assertRejectedStartupCleanup(false)
    }

    @Test
    fun rejectedStartupDeadlinePreservesSecondaryCleanupFailureUntilRetry() {
        assertRejectedStartupCleanup(true)
    }

    // 同じ開始transactionの資源所有と二次失敗を一続きで照合する。
    @Suppress("LongMethod")
    private fun assertRejectedStartupCleanup(rejectDisconnect: Boolean) {
        val texture = SurfaceTexture(0)
        val surface = Surface(texture)
        val pipeline = PlaybackPipeline("startup-cleanup-test", null)
        val tuner = Tuner(RuntimeEnvironment.getApplication(), null, 0)
        val service = ServiceKey(4, 0x4010, 101)
        val channel =
            TunerController.ResolvedChannel(
                null,
                "startup-cleanup-test",
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
        var filter: Filter? = null
        var sync: MediaSync? = null
        var decoder: Any? = null
        ReflectionHelpers.setField(
            pipeline,
            "mainHandler",
            object : Handler(Looper.getMainLooper()) {
                override fun sendMessageAtTime(
                    message: Message,
                    uptimeMillis: Long,
                ): Boolean {
                    // 実開始がFilterとDecoderを所有した後、postDelayed=falseだけを注入する。
                    filter = ReflectionHelpers.getField(pipeline, "videoFilter")
                    decoder = ReflectionHelpers.getField(pipeline, "videoDecoder")
                    sync = ReflectionHelpers.getField(pipeline, "mediaSync")
                    Shadow.extract<NativeMediaSync>(checkNotNull(sync)).rejectCleanup = rejectDisconnect
                    return false
                }
            },
        )
        try {
            pipeline.setSurface(surface)
            val result = pipeline.start(tuner, channel, selection)
            assertFalse(result.startedVideo || result.startedAudio)
            assertTrue(result.diagnostics.any { it.contains("decoder起動期限を予約できません") })
            assertTrue(decoder != null)
            assertEquals(1, Shadow.extract<PlaybackFilterAdmissionTest.NativeFilter>(checkNotNull(filter)).closes)
            assertEquals(null, ReflectionHelpers.getField<Any?>(pipeline, "videoDecoder"))
            assertEquals(null, ReflectionHelpers.getField<Any?>(pipeline, "videoFilter"))
            assertEquals(null, ReflectionHelpers.getField<Any?>(pipeline, "mediaSync"))
            val cleanup = ReflectionHelpers.getField<ResourceCleanup>(pipeline, "resourceCleanup")
            val native = Shadow.extract<NativeMediaSync>(checkNotNull(sync))
            assertEquals(rejectDisconnect, cleanup.hasPending)
            if (rejectDisconnect) {
                assertTrue(result.diagnostics.any { it.contains("資源の解放が未完了") })
                assertFalse(native.events.contains("release"))
                assertTrue(native.connected)
            } else {
                assertTrue(native.events.contains("release"))
                assertFalse(native.connected)
            }
            native.rejectCleanup = false
            pipeline.stop()
            assertFalse(cleanup.hasPending)
            assertFalse(native.connected)
            assertEquals(1, native.events.count { it == "release" })
        } finally {
            runCatching { pipeline.release() }
            runCatching { tuner.close() }
            surface.release()
            texture.release()
        }
    }

    @Suppress("UNUSED_PARAMETER")
    @Implements(MediaSync::class)
    class NativeMediaSync {
        val events = mutableListOf<String>()
        var rejectCleanup = false
        var connected = false

        @Implementation(methodName = "__constructor__")
        private fun construct() = Unit

        @Implementation
        fun setSurface(surface: Surface?) {
            if (surface == null) check(!rejectCleanup) { "MediaSync切断失敗を注入" }
            connected = surface != null
        }

        @Implementation
        fun createInputSurface(): Surface = Surface(SurfaceTexture(0))

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
        fun flush() = Unit

        @Implementation
        fun release() {
            check(!rejectCleanup) { "MediaSync解放失敗を注入" }
            connected = false
            events += "release"
        }
    }
}

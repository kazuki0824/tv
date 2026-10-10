@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import android.media.tv.TvTrackInfo
import com.maleicacid.tvinput.aribsi.NativeAribCaptionRenderer
import com.maleicacid.tvinput.common.CaptionTimestamp
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
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.test.assertEquals
import kotlin.test.assertTrue

@RunWith(RobolectricTestRunner::class)
@Config(
    sdk = [35],
    instrumentedPackages = ["com.maleicacid.tvinput.aribsi"],
    shadows = [CaptionInputContinuityTest.RendererBoundary::class],
)
class CaptionInputContinuityTest {
    @Test
    fun timedPesAcceptedBeforeResetDoesNotReturnToPendingAndNewPesIsAcceptedOnce() {
        fixture { controller, owner ->
            blocked(owner) {
                controller.onBroadcastTimedPesData("caption", byteArrayOf(0x22), AribBroadcastClock.StatementTime(1L))
                controller.flushForSubtitleContinuityLoss()
                controller.onBroadcastTimedPesData("caption", byteArrayOf(0x33), AribBroadcastClock.StatementTime(2L))
            }
            drain(owner)
            val scheduler =
                ReflectionHelpers.getField<BroadcastTimedPesScheduler>(controller, "broadcastTimedPesScheduler")
            val pending = ReflectionHelpers.getField<Map<*, *>>(scheduler, "pending")
            assertEquals(1, pending.size)
            val data = ReflectionHelpers.getField<ByteArray>(pending.values.single(), "pesData")
            assertEquals(listOf(0x33.toByte()), data.toList())
            controller.onBroadcastTimedPesData("caption", byteArrayOf(0x44), AribBroadcastClock.StatementTime(3L))
            drain(owner)
            assertEquals(2, pending.size)
            blocked(owner) {
                controller.beginPlaybackGeneration(8L, false)
                controller.onBroadcastTimedPesData("caption", byteArrayOf(0x55), AribBroadcastClock.StatementTime(4L))
            }
            drain(owner)
            assertEquals(1, pending.size)
            assertEquals(8L, ReflectionHelpers.getField<Long>(controller, "playbackGeneration"))
            assertEquals(8L, ReflectionHelpers.getField<Long>(pending.values.single(), "playbackGeneration"))
            assertEquals(
                listOf(0x55.toByte()),
                ReflectionHelpers.getField<ByteArray>(pending.values.single(), "pesData").toList(),
            )
        }
    }

    @Test
    fun ordinaryPesAcceptedBeforeResetDoesNotDecodeAndNewPesDecodesOnce() {
        fixture { controller, owner ->
            val renderer = NativeAribCaptionRenderer(8, true)
            blocked(owner) {
                controller.onPesData("caption", byteArrayOf(0x22), CaptionTimestamp.NoPts)
                controller.flushForSubtitleContinuityLoss()
                // reset controlの後、旧dataの前に次rendererを設定し、誤decodeを隠さない。
                owner.executeControl { ReflectionHelpers.setField(controller, "renderer", renderer) }
                controller.onPesData("caption", byteArrayOf(0x33), CaptionTimestamp.NoPts)
            }
            drain(owner)
            val boundary = Shadow.extract<RendererBoundary>(renderer)
            assertEquals(listOf(0x33.toByte()), boundary.decoded)
            controller.onPesData("caption", byteArrayOf(0x44), CaptionTimestamp.NoPts)
            drain(owner)
            assertEquals(listOf(0x33.toByte(), 0x44.toByte()), boundary.decoded)
            val nextRenderer = NativeAribCaptionRenderer(8, true)
            blocked(owner) {
                controller.beginPlaybackGeneration(8L, false)
                // 世代切替はdata FIFO。切替後のrenderer/viewport境界をBより先に供給する。
                owner.submit {
                    ReflectionHelpers.setField(controller, "renderer", nextRenderer)
                    ReflectionHelpers.setField(
                        controller,
                        "viewport",
                        AribCaptionController.CaptionViewport(1, 1, 0, 0, 1, 1),
                    )
                }
                controller.onPesData("caption", byteArrayOf(0x55), CaptionTimestamp.NoPts)
            }
            drain(owner)
            assertEquals(8L, ReflectionHelpers.getField<Long>(controller, "playbackGeneration"))
            assertEquals(listOf(0x55.toByte()), Shadow.extract<RendererBoundary>(nextRenderer).decoded)
        }
    }

    @Test
    fun trackAndEnableChangesFenceOldPesButAcceptFollowingPes() {
        fixture { controller, owner ->
            blocked(owner) {
                controller.onBroadcastTimedPesData("caption", byteArrayOf(0x22), AribBroadcastClock.StatementTime(1L))
                controller.setEnabled(false)
                controller.selectTrack(
                    TunerController.TisTrack("next", TvTrackInfo.TYPE_SUBTITLE, TsPid(0x104), 6, null, null, null),
                )
                controller.setEnabled(true)
                controller.onBroadcastTimedPesData("next", byteArrayOf(0x33), AribBroadcastClock.StatementTime(2L))
            }
            drain(owner)
            val scheduler =
                ReflectionHelpers.getField<BroadcastTimedPesScheduler>(controller, "broadcastTimedPesScheduler")
            val pending = ReflectionHelpers.getField<Map<*, *>>(scheduler, "pending")
            assertEquals(1, pending.size)
            assertEquals(
                listOf(0x33.toByte()),
                ReflectionHelpers.getField<ByteArray>(pending.values.single(), "pesData").toList(),
            )
        }
    }

    @Test
    fun unchangedRequestsDoNotDiscardAlreadyAcceptedPes() {
        fixture { controller, owner ->
            blocked(owner) {
                controller.onBroadcastTimedPesData("caption", byteArrayOf(0x22), AribBroadcastClock.StatementTime(1L))
                controller.beginPlaybackGeneration(7L, false)
                controller.setEnabled(true)
                controller.selectTrack(
                    TunerController.TisTrack("caption", TvTrackInfo.TYPE_SUBTITLE, TsPid(0x103), 6, null, null, null),
                )
                controller.onBroadcastTimedPesData("caption", byteArrayOf(0x33), AribBroadcastClock.StatementTime(2L))
            }
            drain(owner)
            val scheduler =
                ReflectionHelpers.getField<BroadcastTimedPesScheduler>(controller, "broadcastTimedPesScheduler")
            val pending = ReflectionHelpers.getField<Map<*, *>>(scheduler, "pending")
            assertEquals(2, pending.size)
        }
    }

    @Test
    fun exhaustedAdmissionSequenceFencesOwnerWithoutWrapping() {
        fixture { controller, owner ->
            blocked(owner) {
                ReflectionHelpers.setField(controller, "pesAdmissionSequence", Long.MAX_VALUE)
                controller.flushForSubtitleContinuityLoss()
                assertTrue(ReflectionHelpers.getField<AtomicBoolean>(controller, "released").get())
                assertEquals(Long.MAX_VALUE, ReflectionHelpers.getField<Long>(controller, "pesAdmissionSequence"))
            }
            assertTrue(owner.awaitTermination(5, TimeUnit.SECONDS))
        }
    }

    private fun fixture(action: (AribCaptionController, LifecycleSerialExecutor) -> Unit) {
        val controller =
            AribCaptionController(
                CaptionOverlayView(RuntimeEnvironment.getApplication()),
                mediaClock = { null },
                allowNoPts = true,
                broadcastDeadline = { _, _ -> AribBroadcastClock.Deadline(1L, 30_000L) },
            )
        val owner = ReflectionHelpers.getField<LifecycleSerialExecutor>(controller, "executor")
        try {
            owner.callControl(5_000L) {
                ReflectionHelpers.setField(controller, "enabled", true)
                ReflectionHelpers.setField(controller, "playbackGeneration", 7L)
                ReflectionHelpers.setField(
                    controller,
                    "selectedTrack",
                    TunerController.TisTrack("caption", TvTrackInfo.TYPE_SUBTITLE, TsPid(0x103), 6, null, null, null),
                )
                ReflectionHelpers.setField(
                    controller,
                    "viewport",
                    AribCaptionController.CaptionViewport(1, 1, 0, 0, 1, 1),
                )
            }
            action(controller, owner)
        } finally {
            controller.close()
        }
    }

    private fun drain(owner: LifecycleSerialExecutor) = owner.submit {}.get(5, TimeUnit.SECONDS)

    private fun blocked(
        owner: LifecycleSerialExecutor,
        action: () -> Unit,
    ) {
        val entered = CountDownLatch(1)
        val release = CountDownLatch(1)
        val task =
            owner.submit {
                entered.countDown()
                check(release.await(5, TimeUnit.SECONDS))
            }
        try {
            assertTrue(entered.await(5, TimeUnit.SECONDS))
            action()
        } finally {
            release.countDown()
        }
        task.get(5, TimeUnit.SECONDS)
    }

    // 本番controller/executorを実行し、JNI renderer境界だけを置換する。
    @Implements(value = NativeAribCaptionRenderer::class, isInAndroidSdk = false)
    class RendererBoundary {
        val decoded = mutableListOf<Byte>()

        @Implementation(methodName = "__constructor__")
        @Suppress("UNUSED_PARAMETER")
        fun construct(
            dataComponentId: Int,
            superimpose: Boolean,
            languageId: Int,
        ) = Unit

        @Implementation
        @Suppress("UNUSED_PARAMETER")
        fun decodePes(
            pesData: ByteArray,
            timestamp: CaptionTimestamp,
        ): NativeAribCaptionRenderer.DecodeResult {
            decoded += pesData.single()
            return NativeAribCaptionRenderer.DecodeResult.NoOutput
        }

        @Implementation
        fun flush() = Unit

        @Implementation
        fun close() = Unit
    }
}

// queueの保持を具体的な容量超過入力で検査する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import com.maleicacid.tvinput.aribsi.NativeAribCaptionRenderer
import org.junit.Test
import sun.misc.Unsafe
import java.util.PriorityQueue

class CaptionPresentationCapacityTest {
    @Test
    fun oversizedSamePtsReplacementPreservesAcceptedDisplayAndClear() {
        assertRejectedReplacementPreservesQueue(boundaryCount = 2, incomingBytes = 8_388_609)
    }

    @Test
    fun samePtsReplacementAboveBoundaryLimitPreservesEveryAcceptedBoundary() {
        assertRejectedReplacementPreservesQueue(boundaryCount = PRESENTATION_BOUNDARY_LIMIT, incomingBytes = 4)
    }

    // 時計確定前の容量反例と従属Clearの除去を同じ本番queueで検査する。
    @Suppress("LongMethod")
    @Test
    fun clockAcquisitionDiscardsDistantFramesAndTheirClearBoundaries() {
        val unsafe =
            Unsafe::class.java
                .getDeclaredField("theUnsafe")
                .apply { isAccessible = true }
                .get(null) as Unsafe
        val controller = unsafe.allocateInstance(AribCaptionController::class.java) as AribCaptionController
        val viewport = AribCaptionController.CaptionViewport(1, 1, 0, 0, 1, 1)
        val image = NativeAribCaptionRenderer.RenderedCaptionImage(0, 0, 1, 1, 4, ByteArray(4))
        val frame = NativeAribCaptionRenderer.RenderedCaptionFrame(120_000L, null, listOf(image))
        val queue = PriorityQueue<Any>(compareBy { it.hashCode() })
        val diagnostics = mutableListOf<AribCaptionController.CaptionDiagnostic.Reason>()
        var clock: PlaybackPipeline.MediaClockSnapshot? = null

        fun set(
            name: String,
            value: Any,
        ) {
            AribCaptionController::class.java
                .getDeclaredField(name)
                .apply { isAccessible = true }
                .set(controller, value)
        }
        set("boundaries", queue)
        set("mediaClock", { clock })
        set("onDiagnostic", { diagnostic: AribCaptionController.CaptionDiagnostic -> diagnostics += diagnostic.reason })
        val enqueue =
            AribCaptionController::class.java
                .getDeclaredMethod("enqueueFrame", frame.javaClass, viewport.javaClass)
                .apply { isAccessible = true }
        val arm = AribCaptionController::class.java.getDeclaredMethod("armNextBoundary").apply { isAccessible = true }
        repeat(PRESENTATION_BOUNDARY_LIMIT) { enqueue.invoke(controller, frame.copy(ptsMillis = 120_000L + it), viewport) }
        check(queue.size == PRESENTATION_BOUNDARY_LIMIT && diagnostics.isEmpty())
        clock = PlaybackPipeline.MediaClockSnapshot(0L, System.nanoTime(), 0.0f)
        arm.invoke(controller)
        check(queue.isEmpty())
        check(diagnostics.size == PRESENTATION_BOUNDARY_LIMIT)
        check(diagnostics.all { it == AribCaptionController.CaptionDiagnostic.Reason.PRESENTATION_HORIZON_EXCEEDED })
        diagnostics.clear()
        enqueue.invoke(controller, frame.copy(ptsMillis = 1_000L), viewport)
        check(queue.size == 1 && diagnostics.isEmpty())
        queue.clear()
        clock = null
        enqueue.invoke(controller, frame.copy(ptsMillis = 59_000L, durationMillis = 2_000L), viewport)
        enqueue.invoke(controller, frame.copy(ptsMillis = 59_001L, durationMillis = 999L), viewport)
        check(queue.size == 4)
        clock = PlaybackPipeline.MediaClockSnapshot(0L, System.nanoTime(), 0.0f)
        arm.invoke(controller)
        check(queue.size == 2)
        check(diagnostics == listOf(AribCaptionController.CaptionDiagnostic.Reason.PRESENTATION_HORIZON_EXCEEDED))
        val retainedTimes =
            queue
                .map { boundary ->
                    boundary.javaClass
                        .getDeclaredField("mediaTimeMillis")
                        .apply { isAccessible = true }
                        .getLong(boundary)
                }.toSet()
        check(retainedTimes == setOf(59_001L, 60_000L))
    }

    // 同じqueueの拒否・受理・従属境界を一続きに検査し、別fixtureへ状態を移さない。
    @Suppress("LongMethod", "CyclomaticComplexMethod")
    private fun assertRejectedReplacementPreservesQueue(
        boundaryCount: Int,
        incomingBytes: Int,
    ) {
        val unsafe =
            Unsafe::class.java
                .getDeclaredField("theUnsafe")
                .apply { isAccessible = true }
                .get(null) as Unsafe
        val controller = unsafe.allocateInstance(AribCaptionController::class.java) as AribCaptionController
        val viewport = AribCaptionController.CaptionViewport(1, 1, 0, 0, 1, 1)
        val image = NativeAribCaptionRenderer.RenderedCaptionImage(0, 0, 1, 1, 4, ByteArray(4))
        val frame =
            NativeAribCaptionRenderer.RenderedCaptionFrame(100L, if (boundaryCount == 2) 100L else null, listOf(image))
        val display =
            Class
                .forName("com.maleicacid.tvinput.tis.AribCaptionController" + "$" + "Boundary" + "$" + "Display")
                .declaredConstructors
                .single()
                .apply { isAccessible = true }
                .newInstance(100L, 1L, frame, viewport)
        val clearConstructor =
            Class
                .forName("com.maleicacid.tvinput.tis.AribCaptionController" + "$" + "Boundary" + "$" + "Clear")
                .declaredConstructors
                .single()
                .apply { isAccessible = true }
        val queue = PriorityQueue<Any>(compareBy { it.hashCode() })
        queue += display
        val firstClearToken = if (boundaryCount == 2) 1L else 2L
        repeat(boundaryCount - 1) { queue += clearConstructor.newInstance(200L + it, firstClearToken + it) }
        val original = queue.toSet()
        val diagnostics = mutableListOf<AribCaptionController.CaptionDiagnostic.Reason>()

        fun set(
            name: String,
            value: Any,
        ) {
            AribCaptionController::class.java
                .getDeclaredField(name)
                .apply { isAccessible = true }
                .set(controller, value)
        }
        set("nextFrameToken", boundaryCount.toLong())
        set("boundaries", queue)
        set("mediaClock", { null })
        set("onDiagnostic", { diagnostic: AribCaptionController.CaptionDiagnostic -> diagnostics += diagnostic.reason })
        val incoming =
            frame.copy(durationMillis = 100L, images = listOf(image.copy(rgba8888 = ByteArray(incomingBytes))))
        AribCaptionController::class.java
            .getDeclaredMethod("enqueueFrame", frame.javaClass, viewport.javaClass)
            .apply { isAccessible = true }
            .invoke(controller, incoming, viewport)
        check(queue.size == boundaryCount && queue.toSet() == original)
        check(diagnostics == listOf(AribCaptionController.CaptionDiagnostic.Reason.PRESENTATION_QUEUE_OVERFLOW))
        diagnostics.clear()
        val retainedClearBoundaries =
            original
                .filter { boundary ->
                    boundary != display &&
                        boundary.javaClass
                            .getDeclaredField("frameToken")
                            .apply { isAccessible = true }
                            .getLong(boundary) != 1L
                }.toSet()
        // 現行budgetの最小反例を使う。budget自体の最適性や新しい閾値は定義しない。
        repeat(PRESENTATION_BOUNDARY_LIMIT + 1) { iteration ->
            val accepted =
                frame.copy(
                    durationMillis = null,
                    images = listOf(image.copy(rgba8888 = ByteArray(4) { iteration.toByte() })),
                )
            AribCaptionController::class.java
                .getDeclaredMethod("enqueueFrame", frame.javaClass, viewport.javaClass)
                .apply { isAccessible = true }
                .invoke(controller, accepted, viewport)
            val displays = queue.filter { it.javaClass == display.javaClass }
            check(queue.size == retainedClearBoundaries.size + 1 && displays.size == 1)
            check(queue.containsAll(retainedClearBoundaries))
            val queuedFrame =
                display.javaClass
                    .getDeclaredField("frame")
                    .apply { isAccessible = true }
                    .get(displays.single()) as NativeAribCaptionRenderer.RenderedCaptionFrame
            check(queuedFrame === accepted)
            check(queuedFrame.images.sumOf { it.rgba8888.size } == 4)
        }
        queue.clear()
        repeat(PRESENTATION_BOUNDARY_LIMIT + 1) { iteration ->
            val accepted = frame.copy(durationMillis = 100L, images = listOf(image.copy(rgba8888 = ByteArray(4))))
            AribCaptionController::class.java
                .getDeclaredMethod("enqueueFrame", frame.javaClass, viewport.javaClass)
                .apply { isAccessible = true }
                .invoke(controller, accepted, viewport)
            val currentDisplay = queue.single { it.javaClass == display.javaClass }
            val currentClear = queue.single { it.javaClass == clearConstructor.declaringClass }
            check(queue.size == 2)
            val displayToken =
                currentDisplay.javaClass
                    .getDeclaredField("frameToken")
                    .apply { isAccessible = true }
                    .getLong(currentDisplay)
            val clearToken =
                currentClear.javaClass
                    .getDeclaredField("frameToken")
                    .apply { isAccessible = true }
                    .getLong(currentClear)
            check(displayToken == clearToken && displayToken > boundaryCount + iteration)
            val queuedFrame =
                currentDisplay.javaClass
                    .getDeclaredField("frame")
                    .apply { isAccessible = true }
                    .get(currentDisplay)
                    as NativeAribCaptionRenderer.RenderedCaptionFrame
            check(queuedFrame === accepted && queuedFrame.images.sumOf { it.rgba8888.size } == 4)
        }
        check(diagnostics.isEmpty())
    }

    companion object {
        private const val PRESENTATION_BOUNDARY_LIMIT = 64
    }
}

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
        assertRejectedReplacementPreservesQueue(boundaryCount = 64, incomingBytes = 4)
    }

    @Suppress("LongMethod")
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
        val frame = NativeAribCaptionRenderer.RenderedCaptionFrame(100L, 100L, listOf(image))
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
        repeat(boundaryCount - 1) { queue += clearConstructor.newInstance(200L + it, 1L + it) }
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
        set("boundaries", queue)
        set("mediaClock", { null })
        set("onDiagnostic", { diagnostic: AribCaptionController.CaptionDiagnostic -> diagnostics += diagnostic.reason })
        val incoming = frame.copy(images = listOf(image.copy(rgba8888 = ByteArray(incomingBytes))))
        AribCaptionController::class.java
            .getDeclaredMethod("enqueueFrame", frame.javaClass, viewport.javaClass)
            .apply { isAccessible = true }
            .invoke(controller, incoming, viewport)
        check(queue.size == boundaryCount && queue.toSet() == original)
        check(diagnostics == listOf(AribCaptionController.CaptionDiagnostic.Reason.PRESENTATION_QUEUE_OVERFLOW))
    }
}

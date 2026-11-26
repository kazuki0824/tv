// 固定TSのpacket/PES識別子と合成data groupを具体値で照合する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import android.graphics.Bitmap
import android.graphics.Canvas
import android.os.Looper
import com.maleicacid.tvinput.aribsi.NativeAribCaptionFactParser
import com.maleicacid.tvinput.aribsi.NativeAribCaptionRenderer
import com.maleicacid.tvinput.common.CaptionPtsMillis
import com.maleicacid.tvinput.common.CaptionTimestamp
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import org.robolectric.util.ReflectionHelpers
import java.io.ByteArrayOutputStream
import java.io.File
import java.util.PriorityQueue
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class RealTsCaptionFactTest {
    @Test
    fun broadcastCaptionManagementLanguageSurvivesJni() {
        val fixture = File(checkNotNull(System.getProperty("realTs.fixtureDirectory")), "test.ts")
        NativeAribCaptionFactParser(superimpose = false).use { parser ->
            val management = captionPayloads(fixture.readBytes()).mapNotNull { parser.ingest(it.payload)?.management }
            check(management.size == 11)
            check(management.all { it.languages.single().iso639LanguageCode == "jpn" })
            check(management.all { it.languages.single().languageTag == 0 })
        }
    }

    @Test
    fun superimposeTimeSurvivesJniWithAndWithoutManagement() {
        val management = "00000000070001006a706e1051dd"
        val statement = "04000000064012000512306219"
        NativeAribCaptionFactParser(superimpose = true).use { parser ->
            val timeOnly = checkNotNull(parser.ingest(hex("81ff00" + statement)))
            check(timeOnly.management == null)
            check(timeOnly.statementTime == NativeAribCaptionFactParser.StatementTime(1, 43_205_123))
            parser.reset()
            val both = checkNotNull(parser.ingest(hex("81ff00" + management + statement)))
            check(both.statementTime == timeOnly.statementTime)
            val language = checkNotNull(both.management).languages.single()
            check(language.iso639LanguageCode == "jpn")
        }
    }

    // 固定入力の製品JNI出力を、本番queueとオフスクリーンのOverlay描画に流す。
    @Test
    fun broadcastCaptionRgbaFitsPresentationQueueAndDrawsOffscreen() {
        val fixture = File(checkNotNull(System.getProperty("realTs.fixtureDirectory")), "test.ts")
        val frames =
            NativeAribCaptionRenderer(0x0008, superimpose = false).use { renderer ->
                check(renderer.setViewport(WIDTH, HEIGHT))
                captionPayloads(fixture.readBytes()).mapNotNull { pes ->
                    val timestamp = CaptionTimestamp.Pts(CaptionPtsMillis(pes.ptsMillis))
                    val decoded = renderer.decodePes(pes.payload, timestamp)
                    (decoded as? NativeAribCaptionRenderer.DecodeResult.Rendered)?.frame
                }
            }
        check(frames.size == 3 && frames.all { it.images.isNotEmpty() })
        verifyPresentation(frames)
    }

    @Suppress("LongMethod")
    private fun verifyPresentation(frames: List<NativeAribCaptionRenderer.RenderedCaptionFrame>) {
        var now = (checkNotNull(frames.first().ptsMillis) - 1L).coerceAtLeast(0L)
        val overlay = CaptionOverlayView(RuntimeEnvironment.getApplication())
        overlay.layout(0, 0, WIDTH, HEIGHT)
        val diagnostics = mutableListOf<AribCaptionController.CaptionDiagnostic.Reason>()
        AribCaptionController(
            overlayView = overlay,
            mediaClock = { PlaybackPipeline.MediaClockSnapshot(now, System.nanoTime(), 0.0f) },
            onDiagnostic = { diagnostics += it.reason },
        ).use { controller ->
            val executor = ReflectionHelpers.getField<LifecycleSerialExecutor>(controller, "executor")
            val initialized = CountDownLatch(1)
            executor.executeData { initialized.countDown() }
            check(initialized.await(5, TimeUnit.SECONDS))
            val viewport = AribCaptionController.CaptionViewport(WIDTH, HEIGHT, 0, 0, WIDTH, HEIGHT)
            ReflectionHelpers.setField(controller, "viewport", viewport)
            val queue = ReflectionHelpers.getField<PriorityQueue<*>>(controller, "boundaries")
            val enqueue =
                controller.javaClass.getDeclaredMethod("enqueueFrame", frames.first().javaClass, viewport.javaClass)
            enqueue.isAccessible = true
            var peak = 0L
            for (frame in frames) {
                enqueue.invoke(controller, frame, viewport)
                val bytes = frame.images.sumOf { it.rgba8888.size.toLong() }
                val pendingBytes =
                    queue.sumOf { boundary ->
                        val display = boundary?.javaClass?.declaredFields?.firstOrNull { it.name == "frame" }
                        display?.isAccessible = true
                        val pending = display?.get(boundary) as? NativeAribCaptionRenderer.RenderedCaptionFrame
                        pending?.images?.sumOf { it.rgba8888.size.toLong() } ?: 0L
                    }
                peak = maxOf(peak, pendingBytes)
                println(
                    "字幕受付 pts=${frame.ptsMillis} duration=${frame.durationMillis} RGBA=$bytes pending=$pendingBytes",
                )
            }
            check(diagnostics.isEmpty()) { diagnostics.toString() }
            check(peak in 1L..8L * 1024 * 1024)
            val times =
                queue
                    .map { boundary ->
                        val field = checkNotNull(boundary).javaClass.getDeclaredField("mediaTimeMillis")
                        field.isAccessible = true
                        field.getLong(boundary)
                    }.toSortedSet()
            val drawn =
                times.map { time ->
                    now = time
                    ReflectionHelpers.callInstanceMethod<Unit>(controller, "armNextBoundary")
                    Shadows.shadowOf(Looper.getMainLooper()).idle()
                    val pixels = visiblePixelHash(overlay)
                    println("字幕表示 pts=$time pixelHash=$pixels peakRGBA=$peak")
                    checkNotNull(pixels)
                }
            check(drawn.size == frames.size && drawn.toSet().size == frames.size)
            check(queue.isEmpty() && diagnostics.isEmpty())
        }
        // 終端の製品Clearで同じOverlayの画素を消す。
        Shadows.shadowOf(Looper.getMainLooper()).idle()
        val afterClear = visiblePixelHash(overlay)
        println("字幕Clear pts=$now pixelHash=$afterClear")
        check(afterClear == null)
    }

    private fun visiblePixelHash(overlay: CaptionOverlayView): Int? {
        val bitmap = Bitmap.createBitmap(WIDTH, HEIGHT, Bitmap.Config.ARGB_8888)
        try {
            overlay.draw(Canvas(bitmap))
            val pixels = IntArray(WIDTH * HEIGHT)
            bitmap.getPixels(pixels, 0, WIDTH, 0, 0, WIDTH, HEIGHT)
            return pixels.contentHashCode().takeIf { pixels.any { it ushr 24 != 0 } }
        } finally {
            bitmap.recycle()
        }
    }

    private data class CaptionPes(
        val payload: ByteArray,
        val ptsMillis: Long,
    )

    // 固定fixtureのTBSフルセグ字幕PIDから長さ指定のprivate_stream_1 PESを取り出す。
    private fun captionPayloads(ts: ByteArray): List<CaptionPes> {
        val result = mutableListOf<CaptionPes>()
        val pes = ByteArrayOutputStream()

        fun collect() {
            val bytes = pes.toByteArray()
            if (bytes.isEmpty()) return
            check(bytes.size >= 9 && unsigned(bytes[3]) == 0xbd)
            val end = 6 + (unsigned(bytes[4]) shl 8) + unsigned(bytes[5])
            check(unsigned(bytes[7]) and 0x80 != 0)
            val pts =
                ((unsigned(bytes[9]) and 0x0e).toLong() shl 29) or
                    (unsigned(bytes[10]).toLong() shl 22) or
                    ((unsigned(bytes[11]) and 0xfe).toLong() shl 14) or
                    (unsigned(bytes[12]).toLong() shl 7) or (unsigned(bytes[13]).toLong() shr 1)
            result += CaptionPes(bytes.copyOfRange(9 + unsigned(bytes[8]), end), pts / 90L)
            pes.reset()
        }
        for (offset in 0..(ts.size - 188) step 188) {
            val flags = unsigned(ts[offset + 1])
            val pid = ((flags and 0x1f) shl 8) + unsigned(ts[offset + 2])
            val control = unsigned(ts[offset + 3])
            if (pid != 0x0114 || control and 0x10 == 0) continue
            if (flags and 0x40 != 0) collect()
            val payload = offset + 4 + if (control and 0x20 != 0) 1 + unsigned(ts[offset + 4]) else 0
            pes.write(ts, payload, offset + 188 - payload)
        }
        collect()
        return result
    }

    private fun unsigned(value: Byte): Int = value.toInt() and 0xff

    companion object {
        private const val WIDTH = 1920
        private const val HEIGHT = 1080
    }

    private fun hex(value: String): ByteArray =
        value
            .chunked(2)
            .map { it.toInt(16).toByte() }
            .toByteArray()
}

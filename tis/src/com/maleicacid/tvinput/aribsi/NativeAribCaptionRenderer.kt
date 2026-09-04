package com.maleicacid.tvinput.aribsi

import com.maleicacid.tvinput.common.CaptionTimestamp
import java.nio.ByteBuffer
import java.nio.ByteOrder

/** libaribcaption decoder/rendererを一つの字幕generationに閉じ込めるJNI境界。 */
class NativeAribCaptionRenderer(
    dataComponentId: Int,
    private val superimpose: Boolean,
    languageId: Int = 1,
) : AutoCloseable {
    init {
        System.loadLibrary("maleicacid_arib_caption_jni")
    }

    private var handle: Long = nativeCreateRenderer(dataComponentId, superimpose, languageId)

    sealed interface DecodeResult {
        data class Rendered(val frame: RenderedCaptionFrame) : DecodeResult
        data object NoPtsRejected : DecodeResult
        data object NoOutput : DecodeResult
    }

    data class RenderedCaptionFrame(
        val ptsMillis: Long?,
        val durationMillis: Long?,
        val images: List<RenderedCaptionImage>,
    )

    data class RenderedCaptionImage(
        val dstX: Int,
        val dstY: Int,
        val width: Int,
        val height: Int,
        val stride: Int,
        val rgba8888: ByteArray,
    )

    fun setViewport(width: Int, height: Int): Boolean {
        val current = handle.takeIf { it != 0L } ?: return false
        if (width <= 0 || height <= 0) return false
        return nativeSetViewport(current, width, height)
    }

    fun decodePes(pesData: ByteArray, timestamp: CaptionTimestamp): DecodeResult {
        val pts = when (timestamp) {
            is CaptionTimestamp.Pts -> timestamp.ptsMillis.value
            CaptionTimestamp.NoPts -> if (superimpose) NO_PTS_SENTINEL else return DecodeResult.NoPtsRejected
        }
        val current = handle.takeIf { it != 0L } ?: return DecodeResult.NoOutput
        if (pesData.isEmpty()) return DecodeResult.NoOutput
        val packet = nativeDecodeAndRender(current, pesData, pts)
            ?: return DecodeResult.NoOutput
        val frame = decodeFramePacket(packet) ?: return DecodeResult.NoOutput
        return DecodeResult.Rendered(frame)
    }

    fun flush() {
        handle.takeIf { it != 0L }?.let(::nativeFlush)
    }

    override fun close() {
        val current = handle
        handle = 0L
        if (current != 0L) nativeReleaseRenderer(current)
    }

    private external fun nativeCreateRenderer(dataComponentId: Int, superimpose: Boolean, languageId: Int): Long
    private external fun nativeSetViewport(handle: Long, width: Int, height: Int): Boolean
    private external fun nativeDecodeAndRender(handle: Long, pesData: ByteArray, ptsMillis: Long): ByteArray?
    private external fun nativeFlush(handle: Long)
    private external fun nativeReleaseRenderer(handle: Long)

    companion object {
        private const val FRAME_HEADER_BYTES = 8 + 8 + 4
        private const val IMAGE_HEADER_BYTES = 6 * 4
        private const val MAX_IMAGES_PER_FRAME = 256
        private const val NO_PTS_SENTINEL = Long.MIN_VALUE

        internal fun decodeFramePacket(packet: ByteArray): RenderedCaptionFrame? {
            if (packet.size < FRAME_HEADER_BYTES) return null
            val buffer = ByteBuffer.wrap(packet).order(ByteOrder.LITTLE_ENDIAN)
            val ptsMillis = buffer.getLong()
            val durationRaw = buffer.getLong()
            val imageCount = buffer.getInt()
            if ((ptsMillis < 0L && ptsMillis != NO_PTS_SENTINEL) || durationRaw < -1L || imageCount !in 0..MAX_IMAGES_PER_FRAME) return null
            val images = ArrayList<RenderedCaptionImage>(imageCount)
            repeat(imageCount) {
                if (buffer.remaining() < IMAGE_HEADER_BYTES) return null
                val dstX = buffer.getInt()
                val dstY = buffer.getInt()
                val width = buffer.getInt()
                val height = buffer.getInt()
                val stride = buffer.getInt()
                val byteCount = buffer.getInt()
                val requiredBytes = stride.toLong() * height.toLong()
                if (!validImageBuffer(width, height, stride, byteCount) ||
                    byteCount.toLong() != requiredBytes || buffer.remaining() < byteCount) {
                    return null
                }
                val rgba = ByteArray(byteCount)
                buffer.get(rgba)
                images += RenderedCaptionImage(dstX, dstY, width, height, stride, rgba)
            }
            if (buffer.hasRemaining()) return null
            return RenderedCaptionFrame(
                ptsMillis = ptsMillis.takeUnless { it == NO_PTS_SENTINEL },
                durationMillis = durationRaw.takeIf { it >= 0L },
                images = images,
            )
        }

        fun validImageBuffer(width: Int, height: Int, stride: Int, byteCount: Int): Boolean {
            if (width <= 0 || height <= 0 || stride <= 0 || byteCount <= 0) return false
            val minimumStride = width.toLong() * 4L
            if (stride.toLong() < minimumStride) return false
            val required = stride.toLong() * height.toLong()
            return required in 1..Int.MAX_VALUE.toLong() && byteCount.toLong() >= required
        }
    }
}

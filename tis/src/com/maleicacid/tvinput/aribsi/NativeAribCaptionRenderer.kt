package com.maleicacid.tvinput.aribsi

import com.maleicacid.tvinput.common.CaptionTimestamp

/** libaribcaption decoder/rendererを一つの字幕generationに閉じ込めるJNI境界。 */
class NativeAribCaptionRenderer(
    dataComponentId: Int,
    superimpose: Boolean,
) : AutoCloseable {
    init {
        System.loadLibrary("maleicacid_arib_caption_jni")
    }

    private var handle: Long = nativeCreateRenderer(dataComponentId, superimpose)

    sealed interface DecodeResult {
        data class Rendered(val frame: RenderedCaptionFrame) : DecodeResult
        data object NoPtsRejected : DecodeResult
        data object NoOutput : DecodeResult
    }

    data class RenderedCaptionFrame(
        val ptsMillis: Long,
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
        val pts = (timestamp as? CaptionTimestamp.Pts)?.ptsMillis?.value
            ?: return DecodeResult.NoPtsRejected
        val current = handle.takeIf { it != 0L } ?: return DecodeResult.NoOutput
        if (pesData.isEmpty()) return DecodeResult.NoOutput
        val frameHandle = nativeDecodeAndRender(current, pesData, pts).takeIf { it != 0L }
            ?: return DecodeResult.NoOutput
        return try {
            val info = nativeFrameInfo(frameHandle)?.takeIf { it.size == FRAME_INFO_SIZE }
                ?: return DecodeResult.NoOutput
            val imageCount = info[2].takeIf { it in 0..MAX_IMAGES_PER_FRAME.toLong() }?.toInt()
                ?: return DecodeResult.NoOutput
            val images = ArrayList<RenderedCaptionImage>(imageCount)
            repeat(imageCount) { index ->
                val imageInfo = nativeImageInfo(frameHandle, index)?.takeIf { it.size == IMAGE_INFO_SIZE }
                    ?: return DecodeResult.NoOutput
                val rgba = nativeImageRgba(frameHandle, index) ?: return DecodeResult.NoOutput
                val width = imageInfo[2]
                val height = imageInfo[3]
                val stride = imageInfo[4]
                if (!validImageBuffer(width, height, stride, rgba.size)) return DecodeResult.NoOutput
                images += RenderedCaptionImage(
                    dstX = imageInfo[0],
                    dstY = imageInfo[1],
                    width = width,
                    height = height,
                    stride = stride,
                    rgba8888 = rgba,
                )
            }
            DecodeResult.Rendered(
                RenderedCaptionFrame(
                    ptsMillis = info[0],
                    durationMillis = info[1].takeIf { it >= 0L },
                    images = images,
                ),
            )
        } finally {
            nativeReleaseFrame(frameHandle)
        }
    }

    fun flush() {
        handle.takeIf { it != 0L }?.let(::nativeFlush)
    }

    override fun close() {
        val current = handle
        handle = 0L
        if (current != 0L) nativeReleaseRenderer(current)
    }

    private external fun nativeCreateRenderer(dataComponentId: Int, superimpose: Boolean): Long
    private external fun nativeSetViewport(handle: Long, width: Int, height: Int): Boolean
    private external fun nativeDecodeAndRender(handle: Long, pesData: ByteArray, ptsMillis: Long): Long
    private external fun nativeFrameInfo(frameHandle: Long): LongArray?
    private external fun nativeImageInfo(frameHandle: Long, imageIndex: Int): IntArray?
    private external fun nativeImageRgba(frameHandle: Long, imageIndex: Int): ByteArray?
    private external fun nativeReleaseFrame(frameHandle: Long)
    private external fun nativeFlush(handle: Long)
    private external fun nativeReleaseRenderer(handle: Long)

    companion object {
        private const val FRAME_INFO_SIZE = 3
        private const val IMAGE_INFO_SIZE = 5
        private const val MAX_IMAGES_PER_FRAME = 256

        fun validImageBuffer(width: Int, height: Int, stride: Int, byteCount: Int): Boolean {
            if (width <= 0 || height <= 0 || stride <= 0 || byteCount <= 0) return false
            val minimumStride = width.toLong() * 4L
            if (stride.toLong() < minimumStride) return false
            val required = stride.toLong() * height.toLong()
            return required in 1..Int.MAX_VALUE.toLong() && byteCount.toLong() >= required
        }
    }
}

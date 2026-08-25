package com.maleicacid.tvinput.tis

import android.content.Context
import android.graphics.Bitmap
import android.graphics.Canvas
import android.view.View
import com.maleicacid.tvinput.aribsi.NativeAribCaptionRenderer

/** libaribcaptionのRGBA imageをrenderer viewport原点からそのまま重ねる表示層。 */
class CaptionOverlayView(context: Context) : View(context) {
    data class BitmapImage(val bitmap: Bitmap, val dstX: Int, val dstY: Int)

    private var images: List<BitmapImage> = emptyList()
    private var contentLeftPx: Int = 0
    private var contentTopPx: Int = 0
    private var onOverlaySizeChanged: (Int, Int) -> Unit = { _, _ -> }

    fun setOnOverlaySizeChangedListener(listener: (Int, Int) -> Unit) {
        onOverlaySizeChanged = listener
        if (width > 0 && height > 0) listener(width, height)
    }

    fun showCaptionFrame(
        frameImages: List<NativeAribCaptionRenderer.RenderedCaptionImage>,
        viewportLeftPx: Int,
        viewportTopPx: Int,
    ): Boolean {
        val converted = mutableListOf<BitmapImage>()
        frameImages.forEach { image ->
            val bitmap = bitmapFromRgba(image)
            if (bitmap == null) {
                converted.forEach { convertedImage -> convertedImage.bitmap.recycle() }
                return false
            }
            converted += BitmapImage(bitmap, image.dstX, image.dstY)
        }
        recycleImages()
        images = converted
        contentLeftPx = viewportLeftPx
        contentTopPx = viewportTopPx
        invalidate()
        return true
    }

    fun clearCaption() {
        recycleImages()
        images = emptyList()
        invalidate()
    }

    override fun onSizeChanged(width: Int, height: Int, oldWidth: Int, oldHeight: Int) {
        super.onSizeChanged(width, height, oldWidth, oldHeight)
        onOverlaySizeChanged(width, height)
    }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        images.forEach { image ->
            canvas.drawBitmap(
                image.bitmap,
                (contentLeftPx + image.dstX).toFloat(),
                (contentTopPx + image.dstY).toFloat(),
                null,
            )
        }
    }

    override fun onDetachedFromWindow() {
        clearCaption()
        super.onDetachedFromWindow()
    }

    private fun recycleImages() {
        images.forEach { image -> if (!image.bitmap.isRecycled) image.bitmap.recycle() }
    }

    companion object {
        fun bitmapFromRgba(image: NativeAribCaptionRenderer.RenderedCaptionImage): Bitmap? {
            if (!NativeAribCaptionRenderer.validImageBuffer(image.width, image.height, image.stride, image.rgba8888.size)) {
                return null
            }
            val pixels = IntArray(image.width * image.height)
            var pixelIndex = 0
            repeat(image.height) { y ->
                var offset = y * image.stride
                repeat(image.width) {
                    val red = image.rgba8888[offset].toInt() and 0xff
                    val green = image.rgba8888[offset + 1].toInt() and 0xff
                    val blue = image.rgba8888[offset + 2].toInt() and 0xff
                    val alpha = image.rgba8888[offset + 3].toInt() and 0xff
                    pixels[pixelIndex++] = (alpha shl 24) or (red shl 16) or (green shl 8) or blue
                    offset += 4
                }
            }
            return Bitmap.createBitmap(pixels, image.width, image.height, Bitmap.Config.ARGB_8888)
        }
    }
}

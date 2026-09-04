package com.maleicacid.tvinput.tis

import android.content.Context
import android.graphics.Bitmap
import android.graphics.Canvas
import android.view.View
import com.maleicacid.tvinput.aribsi.NativeAribCaptionRenderer

/** libaribcaptionのRGBA imageをrenderer viewport原点からそのまま重ねる表示層。 */
class CaptionOverlayView(context: Context) : View(context) {
    data class BitmapImage(val bitmap: Bitmap, val dstX: Int, val dstY: Int)

    private data class Layer(
        val images: List<BitmapImage>,
        val contentLeftPx: Int,
        val contentTopPx: Int,
    )

    private val layers = linkedMapOf<String, Layer>()
    private val sizeListeners = linkedMapOf<String, (Int, Int) -> Unit>()

    fun setOnOverlaySizeChangedListener(listener: (Int, Int) -> Unit) =
        setOnOverlaySizeChangedListener(DEFAULT_LAYER_ID, listener)

    fun setOnOverlaySizeChangedListener(layerId: String, listener: (Int, Int) -> Unit) {
        sizeListeners[layerId] = listener
        if (width > 0 && height > 0) listener(width, height)
    }

    fun showCaptionFrame(
        frameImages: List<NativeAribCaptionRenderer.RenderedCaptionImage>,
        viewportLeftPx: Int,
        viewportTopPx: Int,
    ): Boolean = showCaptionFrame(DEFAULT_LAYER_ID, frameImages, viewportLeftPx, viewportTopPx)

    fun showCaptionFrame(
        layerId: String,
        frameImages: List<NativeAribCaptionRenderer.RenderedCaptionImage>,
        viewportLeftPx: Int,
        viewportTopPx: Int,
    ): Boolean {
        val converted = mutableListOf<BitmapImage>()
        frameImages.forEach { image ->
            val bitmap = bitmapFromRgba(image)
            if (bitmap == null) {
                recycleImages(converted)
                return false
            }
            converted += BitmapImage(bitmap, image.dstX, image.dstY)
        }
        layers.remove(layerId)?.let { recycleImages(it.images) }
        layers[layerId] = Layer(converted, viewportLeftPx, viewportTopPx)
        invalidate()
        return true
    }

    fun clearCaption() = clearCaptionLayer(DEFAULT_LAYER_ID)

    fun clearCaptionLayer(layerId: String) {
        layers.remove(layerId)?.let { recycleImages(it.images) }
        invalidate()
    }

    private fun clearAllLayers() {
        layers.values.forEach { recycleImages(it.images) }
        layers.clear()
        invalidate()
    }

    override fun onSizeChanged(width: Int, height: Int, oldWidth: Int, oldHeight: Int) {
        super.onSizeChanged(width, height, oldWidth, oldHeight)
        sizeListeners.values.toList().forEach { listener -> listener(width, height) }
    }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        layers.values.forEach { layer ->
            layer.images.forEach { image ->
                canvas.drawBitmap(
                    image.bitmap,
                    (layer.contentLeftPx + image.dstX).toFloat(),
                    (layer.contentTopPx + image.dstY).toFloat(),
                    null,
                )
            }
        }
    }

    override fun onDetachedFromWindow() {
        clearAllLayers()
        super.onDetachedFromWindow()
    }

    private fun recycleImages(images: List<BitmapImage>) {
        images.forEach { image -> if (!image.bitmap.isRecycled) image.bitmap.recycle() }
    }

    companion object {
        private const val DEFAULT_LAYER_ID = "caption"

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

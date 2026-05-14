package com.maleicacid.tvinput.tis

import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.RectF
import android.view.View

/** TvInputService overlay 上でARIB字幕を描画する最小表示層。 */
class CaptionOverlayView(context: Context) : View(context) {
    private val textPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.WHITE
        textSize = 34f
        setShadowLayer(4f, 2f, 2f, Color.BLACK)
    }
    private val backgroundPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = 0x99000000.toInt()
    }
    private var captionText: String? = null

    fun showCaption(text: String) {
        captionText = text.takeIf { it.isNotBlank() }
        invalidate()
    }

    fun clearCaption() {
        captionText = null
        invalidate()
    }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        val text = captionText ?: return
        val lines = text.lines().filter { it.isNotBlank() }
        if (lines.isEmpty()) return
        val lineHeight = textPaint.fontMetrics.let { it.descent - it.ascent + 8f }
        val blockHeight = lineHeight * lines.size + 24f
        val top = (height - blockHeight - 48f).coerceAtLeast(0f)
        val left = 48f
        val right = (width - 48f).coerceAtLeast(left + 1f)
        canvas.drawRoundRect(RectF(left - 16f, top, right + 16f, top + blockHeight), 12f, 12f, backgroundPaint)
        var y = top + 18f - textPaint.fontMetrics.ascent
        lines.forEach { line ->
            canvas.drawText(line.take(80), left, y, textPaint)
            y += lineHeight
        }
    }
}

package com.maleicacid.tvinput.tis

import android.os.Handler
import android.os.Looper
import android.util.Log
import com.maleicacid.tvinput.aribsi.NativeAribCaptionRenderer
import com.maleicacid.tvinput.common.CaptionTimestamp
import com.maleicacid.tvinput.common.LogTags

/** ARIB字幕track選択、libaribcaption JNI呼び出し、overlay描画を束ねる。 */
class AribCaptionController(
    private val overlayView: CaptionOverlayView,
) : AutoCloseable {
    private val renderer = NativeAribCaptionRenderer()
    private val mainHandler = Handler(Looper.getMainLooper())
    private var enabled = false
    private var selectedTrack: TunerController.TisTrack? = null

    fun setEnabled(enabled: Boolean) {
        this.enabled = enabled
        if (!enabled) mainHandler.post { overlayView.clearCaption() }
    }

    fun selectTrack(track: TunerController.TisTrack?) {
        selectedTrack = track?.takeIf { it.type == android.media.tv.TvTrackInfo.TYPE_SUBTITLE }
        if (!enabled) mainHandler.post { overlayView.clearCaption() }
    }

    fun onPesData(trackId: String, pesData: ByteArray, timestamp: CaptionTimestamp) {
        val track = selectedTrack ?: return
        if (!enabled || track.id != trackId) return
        val decoded = runCatching {
            renderer.decodePes(
                pesData = pesData,
                timestamp = timestamp,
                dataComponentId = track.dataComponentId,
                superimpose = track.captionServiceKind == "superimpose",
            )
        }.onFailure { error ->
            Log.w(LogTags.TIS, "ARIB字幕PES処理に失敗しました trackId=$trackId", error)
        }.getOrNull() ?: return
        mainHandler.post { overlayView.showCaption(decoded.text) }
    }

    override fun close() {
        mainHandler.post { overlayView.clearCaption() }
        renderer.close()
    }

    companion object {
        fun shouldDrawCaptionForTest(enabled: Boolean, selectedTrackId: String?, incomingTrackId: String): Boolean =
            enabled && selectedTrackId != null && selectedTrackId == incomingTrackId
    }
}

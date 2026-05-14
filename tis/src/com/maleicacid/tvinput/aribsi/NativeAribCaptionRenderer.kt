package com.maleicacid.tvinput.aribsi

/**
 * ARIB字幕PESを libaribcaption C API へ渡すJNI境界。
 * TIS Kotlinは字幕本文を自前ARIB文字列decoderへ渡さず、この境界から返る表示文字列だけを描画層へ渡す。
 */
class NativeAribCaptionRenderer : AutoCloseable {
    init { System.loadLibrary("maleicacid_arib_caption_jni") }

    private var handle: Long = nativeCreateRenderer()

    fun decodePes(
        pesData: ByteArray,
        ptsMillis: Long,
        dataComponentId: Int?,
        superimpose: Boolean,
    ): DecodedCaption? {
        val h = handle.takeIf { it != 0L } ?: return null
        if (pesData.isEmpty()) return null
        val text = nativeDecodePes(h, pesData, ptsMillis, dataComponentId ?: 0x0008, superimpose)
            ?.takeIf { it.isNotBlank() }
            ?: return null
        return DecodedCaption(text = text, ptsMillis = ptsMillis)
    }

    override fun close() {
        val h = handle
        handle = 0L
        if (h != 0L) nativeReleaseRenderer(h)
    }

    data class DecodedCaption(
        val text: String,
        val ptsMillis: Long,
    )

    private external fun nativeCreateRenderer(): Long
    private external fun nativeReleaseRenderer(handle: Long)
    private external fun nativeDecodePes(handle: Long, pesData: ByteArray, ptsMillis: Long, dataComponentId: Int, superimpose: Boolean): String?
}

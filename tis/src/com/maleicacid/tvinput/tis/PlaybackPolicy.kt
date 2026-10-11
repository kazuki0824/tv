package com.maleicacid.tvinput.tis

/** ライブ再生の純粋判定。Session lifecycleとAndroid資源はMaleicacidLiveSessionが所有する。 */
object PlaybackPolicy {
    private const val SERVICE_TYPE_DIGITAL_AUDIO = 0x02

    fun isAudioOnlyService(serviceType: Int?): Boolean = serviceType == SERVICE_TYPE_DIGITAL_AUDIO

    // 0xC0単独ではデータ放送とワンセグを区別できない。
    // NIT Partial Reception Descriptorで確定した場合だけ、通常TVと同じ映像選択条件を使う。
    @Suppress("MagicNumber")
    fun shouldRejectSelection(
        serviceType: Int,
        selection: TunerController.AvStreamSelection,
        partialReception: Boolean = false,
    ): Boolean =
        when {
            serviceType == 0x01 -> selection.video == null
            serviceType == 0xc0 && partialReception -> selection.video == null
            serviceType == SERVICE_TYPE_DIGITAL_AUDIO -> selection.audio == null
            else -> true
        }
}

/** 同じ利用不能理由の連続通知だけを抑え、選局変更・映像復帰時にresetする。 */
internal class VideoUnavailableNotificationGate {
    private var lastReason: Int? = null

    @Synchronized
    fun shouldNotify(reason: Int): Boolean {
        if (lastReason == reason) return false
        lastReason = reason
        return true
    }

    @Synchronized
    fun reset() {
        lastReason = null
    }
}

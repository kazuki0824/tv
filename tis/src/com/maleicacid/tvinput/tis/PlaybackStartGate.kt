package com.maleicacid.tvinput.tis

import com.maleicacid.tvinput.common.ServiceKey

/**
 * AV パイプラインの過剰な再起動を抑止する純粋な状態機械。
 *
 * section 更新は頻繁に発生するため、PMT/CAT/EIT/ECM 更新だけでは再生を再起動しない。
 * AV に関係する署名が変わった場合、または Surface 再設定や再選局など外部条件が明示的に
 * 前回失敗した開始試行の再試行を許可した場合だけ再起動する。
 */
data class AvPlaybackSignature(
    val serviceKey: ServiceKey,
    val pcrPid: Int?,
    val videoPid: Int,
    val videoStreamType: Int,
    val audioPid: Int?,
    val audioStreamType: Int?,
    val clear: Boolean,
    val keyTokenAvailable: Boolean,
)

class PlaybackStartGate {
    private var lastAttemptedSignature: AvPlaybackSignature? = null
    private var lastStartedSignature: AvPlaybackSignature? = null

    /** 呼び出し側が PlaybackPipeline.start() を実行してよい場合だけ真を返す。 */
    fun shouldAttempt(signature: AvPlaybackSignature): Boolean = signature != lastAttemptedSignature

    /** start() 呼び出し前に記録し、開始失敗後に section 更新ごとの無限再試行を防ぐ。 */
    fun recordAttempt(signature: AvPlaybackSignature) {
        lastAttemptedSignature = signature
    }

    fun recordResult(signature: AvPlaybackSignature, startedVideo: Boolean) {
        if (startedVideo) lastStartedSignature = signature
    }

    /** 現在の AV 署名が初回フレーム到達済みとして記録されているかを返す。 */
    fun isStartedSignature(signature: AvPlaybackSignature): Boolean = signature == lastStartedSignature

    /** 再選局または release 後に全状態を消去する。 */
    fun reset() {
        lastAttemptedSignature = null
        lastStartedSignature = null
    }

    /**
     * 外部条件が変化した後に同じ AV 署名の再試行を許可する。たとえば、
     * 前回 SURFACE_NOT_SET で失敗した後に新しい Surface が接続された場合が該当する。
     */
    fun allowRetry() {
        lastAttemptedSignature = null
    }
}

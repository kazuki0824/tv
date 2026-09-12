package com.maleicacid.tvinput.tis

import android.media.MediaCodecInfo.CodecProfileLevel
import android.media.MediaFormat
import com.maleicacid.tvinput.aribsi.AribAvcSignaling
import com.maleicacid.tvinput.aribsi.AribCodecFacts
import java.nio.ByteBuffer

internal class UnsupportedCodecFormat(
    message: String,
    cause: Throwable? = null,
) : IllegalArgumentException(message, cause)

/** 放送のprofile値をAndroidの能力照合へ渡す。対応機器の実測認定は行わない。 */
internal object CodecFormatPolicy {
    data class AvcProfileLevel(
        val profile: Int,
        val level: Int,
    )

    // 同じ入力に対する分岐・項目写像を保持し、処理分割による状態の受け渡しを増やさない。
    // この処理の規格値・ビット幅・単位換算・固定上限をリテラルのまま照合できる形に保つ。
    // 入力拒否・未準備・失敗を発生点で返し、成功経路を深い入れ子にしない。
    @Suppress("CyclomaticComplexMethod", "MagicNumber", "ReturnCount")
    fun avcProfileLevel(value: AribAvcSignaling): AvcProfileLevel? {
        if (value.constraintFlags and 3 != 0) return null
        val profile =
            when (value.profileIdc) {
                66 -> CodecProfileLevel.AVCProfileBaseline
                77 -> CodecProfileLevel.AVCProfileMain
                88 -> CodecProfileLevel.AVCProfileExtended
                100 -> CodecProfileLevel.AVCProfileHigh
                110 -> CodecProfileLevel.AVCProfileHigh10
                122 -> CodecProfileLevel.AVCProfileHigh422
                244 -> CodecProfileLevel.AVCProfileHigh444
                else -> return null
            }
        val level =
            if (value.levelIdc == 11 && value.profileIdc in setOf(66, 77, 88) && value.constraintFlags and 0x10 != 0) {
                CodecProfileLevel.AVCLevel1b
            } else {
                when (value.levelIdc) {
                    10 -> CodecProfileLevel.AVCLevel1
                    11 -> CodecProfileLevel.AVCLevel11
                    12 -> CodecProfileLevel.AVCLevel12
                    13 -> CodecProfileLevel.AVCLevel13
                    20 -> CodecProfileLevel.AVCLevel2
                    21 -> CodecProfileLevel.AVCLevel21
                    22 -> CodecProfileLevel.AVCLevel22
                    30 -> CodecProfileLevel.AVCLevel3
                    31 -> CodecProfileLevel.AVCLevel31
                    32 -> CodecProfileLevel.AVCLevel32
                    40 -> CodecProfileLevel.AVCLevel4
                    41 -> CodecProfileLevel.AVCLevel41
                    42 -> CodecProfileLevel.AVCLevel42
                    50 -> CodecProfileLevel.AVCLevel5
                    51 -> CodecProfileLevel.AVCLevel51
                    52 -> CodecProfileLevel.AVCLevel52
                    60 -> CodecProfileLevel.AVCLevel6
                    61 -> CodecProfileLevel.AVCLevel61
                    62 -> CodecProfileLevel.AVCLevel62
                    else -> return null
                }
            }
        return AvcProfileLevel(profile, level)
    }

    fun configureAvc(
        format: MediaFormat,
        facts: AribCodecFacts,
        sps: AribAvcSignaling,
    ) {
        if (!facts.resolved || (facts.avc != null && facts.avc != sps)) {
            throw UnsupportedCodecFormat("AVC記述子が不正、またはSPSと不一致です")
        }
        val profile =
            avcProfileLevel(sps)
                ?: throw UnsupportedCodecFormat("未対応のAVC profile/levelです: $sps")
        format.setInteger(MediaFormat.KEY_PROFILE, profile.profile)
        format.setInteger(MediaFormat.KEY_LEVEL, profile.level)
    }

    // この処理の規格値・ビット幅・単位換算・固定上限をリテラルのまま照合できる形に保つ。
    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    // 失敗の発生点ごとに既存の例外種別と原因を保ち、判定順を変えない。
    @Suppress("MagicNumber", "MaxLineLength", "ThrowsCount")
    fun adtsAudioFormat(
        bytes: ByteArray,
        facts: AribCodecFacts,
        signaledCodec: String?,
    ): MediaFormat? {
        if (!facts.resolved) throw UnsupportedCodecFormat("未解決のAAC記述子です")
        val config =
            try {
                com.maleicacid.tvinput.aribsi.NativeAribSiParser
                    .probeAacConfiguration(bytes, facts.audioConfigHex)
            } catch (error: IllegalArgumentException) {
                throw UnsupportedCodecFormat(error.message ?: "AAC構成が不正です", error)
            } ?: return null
        val profile =
            when {
                config.audioObjectType == 29 || signaledCodec == "HE-AAC-v2" -> throw UnsupportedCodecFormat("HE-AAC-v2の入力構成は未対応です")
                config.audioObjectType == 5 || signaledCodec == "HE-AAC" -> CodecProfileLevel.AACObjectHE
                else -> CodecProfileLevel.AACObjectLC
            }
        return MediaFormat
            .createAudioFormat(
                MediaFormat.MIMETYPE_AUDIO_AAC,
                config.extensionSamplingFrequency ?: config.samplingFrequency,
                config.channelCount,
            ).apply {
                setInteger(MediaFormat.KEY_IS_ADTS, 1)
                setInteger(MediaFormat.KEY_PROFILE, profile)
                setInteger(MediaFormat.KEY_AAC_PROFILE, profile)
                setByteBuffer("csd-0", ByteBuffer.wrap(config.audioSpecificConfig))
            }
    }
}

package com.maleicacid.tvinput.tis

import android.media.MediaCodecInfo.CodecProfileLevel
import android.media.MediaFormat
import com.maleicacid.tvinput.aribsi.AribAvcSignaling
import com.maleicacid.tvinput.aribsi.AribCodecFacts
import java.nio.ByteBuffer

internal class UnsupportedCodecFormat(message: String) : IllegalArgumentException(message)

/** 放送のprofile値をAndroidの能力照合へ渡す。対応機器の実測認定は行わない。 */
internal object CodecFormatPolicy {
    data class AvcProfileLevel(val profile: Int, val level: Int)

    fun avcProfileLevel(value: AribAvcSignaling): AvcProfileLevel? {
        if (value.constraintFlags and 3 != 0) return null
        val profile = when (value.profileIdc) {
            66 -> CodecProfileLevel.AVCProfileBaseline
            77 -> CodecProfileLevel.AVCProfileMain
            88 -> CodecProfileLevel.AVCProfileExtended
            100 -> CodecProfileLevel.AVCProfileHigh
            110 -> CodecProfileLevel.AVCProfileHigh10
            122 -> CodecProfileLevel.AVCProfileHigh422
            244 -> CodecProfileLevel.AVCProfileHigh444
            else -> return null
        }
        val level = if (value.levelIdc == 11 && value.profileIdc in setOf(66, 77, 88) && value.constraintFlags and 0x10 != 0) {
            CodecProfileLevel.AVCLevel1b
        } else when (value.levelIdc) {
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
        return AvcProfileLevel(profile, level)
    }

    fun configureAvc(format: MediaFormat, facts: AribCodecFacts, sps: AribAvcSignaling) {
        if (!facts.resolved || (facts.avc != null && facts.avc != sps)) {
            throw UnsupportedCodecFormat("AVC記述子が不正、またはSPSと不一致です")
        }
        val profile = avcProfileLevel(sps)
            ?: throw UnsupportedCodecFormat("未対応のAVC profile/levelです: $sps")
        format.setInteger(MediaFormat.KEY_PROFILE, profile.profile)
        format.setInteger(MediaFormat.KEY_LEVEL, profile.level)
    }

    private data class AdtsHeader(val objectType: Int, val frequencyIndex: Int, val channelConfiguration: Int)
    private val aacSampleRates = intArrayOf(96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350)

    private fun adtsHeader(bytes: ByteArray): AdtsHeader? {
        for (offset in 0..bytes.size - 7) {
            if (bytes[offset].toInt() and 0xff != 0xff || bytes[offset + 1].toInt() and 0xf6 != 0xf0) continue
            val protectionAbsent = bytes[offset + 1].toInt() and 1 != 0
            val headerSize = if (protectionAbsent) 7 else 9
            if (bytes.size - offset < headerSize) return null
            val frameLength = ((bytes[offset + 3].toInt() and 3) shl 11) or
                ((bytes[offset + 4].toInt() and 0xff) shl 3) or ((bytes[offset + 5].toInt() and 0xe0) ushr 5)
            val frequencyIndex = (bytes[offset + 2].toInt() ushr 2) and 15
            if (frameLength < headerSize || frequencyIndex !in aacSampleRates.indices) continue
            return AdtsHeader(
                ((bytes[offset + 2].toInt() ushr 6) and 3) + 1,
                frequencyIndex,
                ((bytes[offset + 2].toInt() and 1) shl 2) or ((bytes[offset + 3].toInt() ushr 6) and 3),
            )
        }
        return null
    }

    fun adtsAudioFormat(bytes: ByteArray, facts: AribCodecFacts, signaledCodec: String?): MediaFormat? {
        val header = adtsHeader(bytes) ?: return null
        if (!facts.resolved || header.objectType != CodecProfileLevel.AACObjectLC) {
            throw UnsupportedCodecFormat("未対応または未解決のADTS audio object typeです")
        }
        val channelCount = when (header.channelConfiguration) {
            in 1..6 -> header.channelConfiguration
            7 -> 8
            else -> throw UnsupportedCodecFormat("PCEによるADTS channel構成を確定できません")
        }
        val baseFrequency = aacSampleRates[header.frequencyIndex]
        val ascHeader = facts.audioConfigHeader
        if (ascHeader != null && (ascHeader.samplingFrequency != baseFrequency ||
                ascHeader.channelConfiguration != header.channelConfiguration ||
                ascHeader.audioObjectType !in setOf(2, 5) ||
                (ascHeader.audioObjectType == 5 && ascHeader.coreAudioObjectType != 2))) {
            throw UnsupportedCodecFormat("PMTのAudioSpecificConfigとADTSが不一致、または未対応です")
        }
        val profile = when {
            ascHeader?.audioObjectType == 5 || signaledCodec == "HE-AAC" -> CodecProfileLevel.AACObjectHE
            signaledCodec == "HE-AAC-v2" -> throw UnsupportedCodecFormat("HE-AAC-v2の入力構成は未対応です")
            else -> CodecProfileLevel.AACObjectLC
        }
        val config = facts.audioConfigHex?.let { value ->
            if (ascHeader == null || value.isEmpty() || value.length % 2 != 0) {
                throw UnsupportedCodecFormat("AudioSpecificConfigが不正です")
            }
            value.chunked(2).map { pair ->
                (pair.toIntOrNull(16) ?: throw UnsupportedCodecFormat("AudioSpecificConfigのhexが不正です")).toByte()
            }.toByteArray()
        } ?: byteArrayOf(
            ((header.objectType shl 3) or (header.frequencyIndex ushr 1)).toByte(),
            (((header.frequencyIndex and 1) shl 7) or (header.channelConfiguration shl 3)).toByte(),
        )
        return MediaFormat.createAudioFormat(MediaFormat.MIMETYPE_AUDIO_AAC,
            ascHeader?.extensionSamplingFrequency ?: baseFrequency, channelCount).apply {
            setInteger(MediaFormat.KEY_IS_ADTS, 1)
            setInteger(MediaFormat.KEY_PROFILE, profile)
            setInteger(MediaFormat.KEY_AAC_PROFILE, profile)
            setByteBuffer("csd-0", ByteBuffer.wrap(config))
        }
    }
}

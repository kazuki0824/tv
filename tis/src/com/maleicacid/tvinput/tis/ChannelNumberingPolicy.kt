package com.maleicacid.tvinput.tis

import com.maleicacid.tvinput.aribsi.AribService
import com.maleicacid.tvinput.db.ChannelRecord

/**
 * 日本向け ISDB サービス の安定した表示番号方針。
 * 地上波は リモコンキー を優先し、得られない場合は service_id 由来の安定値を使う。
 * BS/110CS は地上波と同じ リモコンキー 意味論を持たないため、scan 候補ラベルと service_id を使う。
 */
object ChannelNumberingPolicy {
    fun displayNumber(service: AribService, remoteKey: Int?, candidate: ScanCandidate): String {
        val base = when (candidate.deliverySystem) {
            ChannelRecord.DELIVERY_SYSTEM_ISDB_T -> terrestrialBase(service, remoteKey)
            ChannelRecord.DELIVERY_SYSTEM_ISDB_S -> satelliteBase(service, candidate)
            else -> candidate.displayChannel.ifBlank { service.serviceKey.serviceId.toString() }
        }
        return base.replace(Regex("[^0-9A-Za-z_.-]"), "-")
    }

    private fun terrestrialBase(service: AribService, remoteKey: Int?): String {
        val key = remoteKey?.takeIf { it in 1..12 } ?: return service.serviceKey.serviceId.toString()
        val serviceId = service.serviceKey.serviceId
        // ARIB TR-B14 Vol.7 encodes service type in b8..b7 and service number in b2..b0.
        val serviceType = (serviceId ushr 7) and 0x03
        val serviceNumber = serviceId and 0x07
        val threeDigitNumber = serviceType * 200 + key * 10 + (serviceNumber + 1)
        return threeDigitNumber.toString().padStart(3, '0')
    }

    private fun satelliteBase(service: AribService, candidate: ScanCandidate): String {
        val prefix = when (candidate.satelliteBand) {
            "BS" -> "BS"
            "110CS" -> "CS"
            else -> candidate.displayChannel.takeIf { it.isNotBlank() } ?: "SAT"
        }
        return "$prefix-${service.serviceKey.serviceId}"
    }

}

package com.maleicacid.tvinput.tis

import com.maleicacid.tvinput.aribsi.AribService
import com.maleicacid.tvinput.db.ChannelRecord

/**
 * 日本向け ISDB service の安定した表示番号方針。
 * 地上波は リモコンキー を優先し、得られない場合は service_id 由来の安定値を使う。
 * BS/110CS は地上波と同じ リモコンキー 意味論を持たないため、scan 候補ラベルと service_id を使う。
 */
object ChannelNumberingPolicy {
    fun displayNumber(service: AribService, remoteKey: Int?, candidate: ScanCandidate): String {
        val base = when (candidate.deliverySystem) {
            ChannelRecord.DELIVERY_SYSTEM_ISDB_T -> terrestrialBase(service, remoteKey, candidate)
            ChannelRecord.DELIVERY_SYSTEM_ISDB_S -> satelliteBase(service, candidate)
            else -> candidate.displayChannel.ifBlank { service.serviceKey.serviceId.toString() }
        }
        return base.replace(Regex("[^0-9A-Za-z_.-]"), "-")
    }

    private fun terrestrialBase(service: AribService, remoteKey: Int?, candidate: ScanCandidate): String {
        val key = remoteKey ?: candidate.physicalChannel ?: return service.serviceKey.serviceId.toString()
        val ordinal = stableServiceOrdinal(service.serviceKey.serviceId)
        return if (ordinal == 0) key.toString() else "$key.$ordinal"
    }

    private fun satelliteBase(service: AribService, candidate: ScanCandidate): String {
        val prefix = when (candidate.satelliteBand) {
            "BS" -> "BS"
            "110CS" -> "CS"
            else -> candidate.displayChannel.takeIf { it.isNotBlank() } ?: "SAT"
        }
        return "$prefix-${service.serviceKey.serviceId}"
    }

    private fun stableServiceOrdinal(serviceId: Int): Int {
        val suffix = serviceId % 100
        return if (suffix == 0) 0 else suffix
    }
}

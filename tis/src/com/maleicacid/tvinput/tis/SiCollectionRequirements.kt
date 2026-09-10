package com.maleicacid.tvinput.tis

import com.maleicacid.tvinput.aribsi.EpgSectionPolicy
import com.maleicacid.tvinput.aribsi.ServiceRegistrationSnapshot
import com.maleicacid.tvinput.aribsi.SiDiscoveryProfile
import com.maleicacid.tvinput.aribsi.TransportKey
import com.maleicacid.tvinput.common.ServiceKey

/** 有限な走査操作の必要集合。放送由来の完成状態は同じbulk snapshotから取得する。 */
internal class SiCollectionRequirements(
    private val mode: ChannelScanController.PublishMode,
    private val profile: Int,
    private val fixedServiceKeys: Set<ServiceKey> = emptySet(),
) {
    data class Key(val component: String, val onid: Int?, val tsid: Int?, val sid: Int?)
    data class Status(val requirements: Map<Key, Boolean>) {
        val complete: Boolean get() = requirements.isNotEmpty() && requirements.values.all { it }
        val missing: Set<Key> get() = requirements.filterValues { !it }.keys
    }

    val requiresEit: Boolean get() = mode == ChannelScanController.PublishMode.BOOT_EPG_SYNC ||
        mode == ChannelScanController.PublishMode.BACKGROUND_CHANNEL_MAINTENANCE

    fun evaluate(snapshot: ServiceRegistrationSnapshot): Status {
        val targets = if (mode == ChannelScanController.PublishMode.SETUP_SCAN) {
            snapshot.services.map { it.serviceKey }.filterTo(linkedSetOf()) {
                TransportKey(it.originalNetwork, it.transportStream) in snapshot.actualTransports
            }
        } else {
            fixedServiceKeys
        }
        val transports = targets.mapTo(linkedSetOf()) { it.originalNetworkId to it.transportStreamId }
        val result = linkedMapOf<Key, Boolean>()
        fun require(key: Key, complete: Boolean) {
            result[key] = result[key] != false && complete
        }
        fun requireTable(component: String, onid: Int? = null, tsid: Int? = null, sid: Int? = null) {
            val rows = snapshot.tableRequirements.filter {
                it.component == component && it.originalNetworkId == onid &&
                    it.transportStreamId == tsid && it.serviceId == sid
            }
            require(Key(component, onid, tsid, sid), rows.isNotEmpty() && rows.all { it.complete })
        }
        requireTable("PAT")
        for ((onid, tsid) in transports) {
            requireTable("SDT", onid, tsid)
            requireTable("NIT", onid, tsid)
        }
        val supplemental = when (profile) {
            SiDiscoveryProfile.ISDB_T -> emptyList()
            SiDiscoveryProfile.BS -> listOf("SDT-other")
            SiDiscoveryProfile.CS110 -> listOf("SDT-other", "NIT-other")
            else -> error("未対応のSI収集profileです: $profile")
        }
        for (component in supplemental) {
            val rows = snapshot.tableRequirements.filter { it.component == component && it.required }
            if (rows.isEmpty()) require(Key(component, null, null, null), false)
        }
        for (table in snapshot.tableRequirements.filter { it.required }) {
            val scope = table.originalNetworkId to table.transportStreamId
            if (table.component == "PMT" && table.serviceId != null && targets.isNotEmpty() &&
                targets.none { it.originalNetworkId == scope.first && it.transportStreamId == scope.second && it.serviceId == table.serviceId }) continue
            if (table.component in setOf("SDT", "NIT") && table.transportStreamId != null &&
                transports.isNotEmpty() && scope !in transports) continue
            require(Key(table.component, table.originalNetworkId, table.transportStreamId, table.serviceId), table.complete)
        }
        if (targets.isEmpty()) require(Key("TARGET_SERVICE", null, null, null), false)
        for (key in targets) {
            requireTable("PMT", key.originalNetworkId, key.transportStreamId, key.serviceId)
            require(Key("SERVICE_FACTS", key.originalNetworkId, key.transportStreamId, key.serviceId), key in snapshot.semanticFactsByServiceKey)
            if (requiresEit) {
                val instances = snapshot.eitInstances.filter { it.tableId == 0x4e && it.serviceKey == key && it.currentNextIndicator }
                require(Key("EIT_PF_ACTUAL", key.originalNetworkId, key.transportStreamId, key.serviceId),
                    instances.isNotEmpty() && instances.all { EpgSectionPolicy.isComplete(profile, it) })
            }
        }
        return Status(result)
    }
}

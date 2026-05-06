package com.maleicacid.tvinput.aribsi

import com.maleicacid.tvinput.common.ServiceKey

/**
 * TvProvider 登録用のサービス snapshot を構築する。
 * 完了判定は transport 全体ではなく service 単位で行う。
 * 実波では PAT/PMT/SDT/NIT の到達間隔が異なるため、
 * 公開方針がサービスごとの視聴可能性を判断する。
 */
class ServiceListBuilder(private val engine: AribSiEngine) {
    data class ServiceCompleteness(
        val serviceKey: ServiceKey,
        val hasPmt: Boolean,
        val hasStreams: Boolean,
        val hasVideo: Boolean,
        val hasAudio: Boolean,
        val reasons: List<String>,
    ) {
        val isComplete: Boolean get() = hasPmt && hasStreams
        val isViewable: Boolean get() = hasPmt && hasVideo
        fun signatureToken(): String = listOf(
            serviceKey.originalNetworkId,
            serviceKey.transportStreamId,
            serviceKey.serviceId,
            hasPmt,
            hasStreams,
            hasVideo,
            hasAudio,
        ).joinToString(":")
    }

    data class ServiceSnapshotSummary(
        val totalKeys: Set<ServiceKey>,
        val completeKeys: Set<ServiceKey>,
        val viewableKeys: Set<ServiceKey>,
        val completeness: List<ServiceCompleteness>,
    ) {
        val total: Int get() = totalKeys.size
        val complete: Int get() = completeKeys.size
        val viewable: Int get() = viewableKeys.size
        fun stableSignature(): String = completeness
            .sortedWith(compareBy<ServiceCompleteness> { it.serviceKey.originalNetworkId }
                .thenBy { it.serviceKey.transportStreamId }
                .thenBy { it.serviceKey.serviceId })
            .joinToString("|") { it.signatureToken() }
    }

    fun snapshot(): List<AribService> = engine.snapshotServices()

    fun completenessSummary(): ServiceSnapshotSummary {
        val completeness = snapshot().map { completenessFor(it) }
        return ServiceSnapshotSummary(
            totalKeys = completeness.map { it.serviceKey }.toSet(),
            completeKeys = completeness.filter { it.isComplete }.map { it.serviceKey }.toSet(),
            viewableKeys = completeness.filter { it.isViewable }.map { it.serviceKey }.toSet(),
            completeness = completeness,
        )
    }

    fun publishableSnapshot(requireComplete: Boolean = true): List<AribService> {
        val publishability = engine.snapshotPublishabilityDiagnostics().associateBy { it.serviceKey }
        return engine.snapshotServices().filter { service ->
            val publishedByRust = publishability[service.serviceKey]?.publishable == true
            if (requireComplete) publishedByRust else publishedByRust || isServiceComplete(service)
        }
    }

    fun publishableViewableSnapshot(): List<AribService> = publishableSnapshot(requireComplete = true).filter { completenessFor(it).isViewable }

    fun viewableSnapshot(): List<AribService> = publishableViewableSnapshot()

    fun incompleteReasons(): Map<ServiceKey, List<String>> {
        val rustReasons = engine.snapshotPublishabilityDiagnostics()
            .filter { !it.publishable }
            .associate { it.serviceKey to it.missingComponents }
        val localReasons = completenessSummary()
            .completeness
            .filter { !it.isViewable }
            .associate { it.serviceKey to it.reasons }
        return (rustReasons.keys + localReasons.keys).associateWith { key ->
            ((rustReasons[key].orEmpty()) + (localReasons[key].orEmpty())).distinct()
        }
    }

    fun isServicePublishable(service: AribService): Boolean = engine.snapshotPublishabilityDiagnostics().any { it.serviceKey == service.serviceKey && it.publishable }

    fun isServiceComplete(service: AribService): Boolean = completenessFor(service).isComplete

    fun isServiceViewable(service: AribService): Boolean = isServicePublishable(service) && completenessFor(service).isViewable

    fun completenessFor(service: AribService): ServiceCompleteness {
        val hasVideo = service.streams.any { it.streamType == 0x02 || it.streamType == 0x1b || it.streamType == 0x24 }
        val hasAudio = service.streams.any { it.streamType == 0x03 || it.streamType == 0x04 || it.streamType == 0x0f || it.streamType == 0x11 }
        val reasons = mutableListOf<String>()
        if (service.pmtPid == null) reasons += "PMT"
        if (service.streams.isEmpty()) reasons += "ES"
        if (!hasVideo) reasons += "VIDEO"
        if (!hasAudio) reasons += "AUDIO_OPTIONAL"
        return ServiceCompleteness(
            serviceKey = service.serviceKey,
            hasPmt = service.pmtPid != null,
            hasStreams = service.streams.isNotEmpty(),
            hasVideo = hasVideo,
            hasAudio = hasAudio,
            reasons = reasons,
        )
    }
}

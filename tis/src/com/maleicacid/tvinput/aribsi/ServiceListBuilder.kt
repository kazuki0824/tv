package com.maleicacid.tvinput.aribsi

import com.maleicacid.tvinput.common.ServiceKey

/**
 * TvProvider 登録用のサービス snapshot を構築する。
 * readiness / EPG 公開可否 / 平文ライブ視聴 視聴可否は Rust 側 診断 を SSOT とし、
 * Kotlin 側では再計算しない。
 */
class ServiceListBuilder(private val engine: AribSiEngine) {
    data class ServiceCompleteness(
        val serviceKey: ServiceKey,
        val publishable: Boolean,
        val channelRegistrationReady: Boolean,
        val epgPublishable: Boolean,
        val clearLivePlaybackSupported: Boolean,
        val requiresCas: Boolean,
        val unsupportedCas: Boolean,
        val missingComponents: List<String>,
        val reasons: List<String>,
        val registrationReasons: List<String>,
        val epgReasons: List<String>,
    ) {
        val isComplete: Boolean get() = publishable
        val isRegistrationReady: Boolean get() = channelRegistrationReady
        val isEpgPublishable: Boolean get() = epgPublishable
        val isClearLivePlaybackSupported: Boolean get() = clearLivePlaybackSupported
        fun signatureToken(): String = listOf(
            serviceKey.originalNetworkId,
            serviceKey.transportStreamId,
            serviceKey.serviceId,
            publishable,
            channelRegistrationReady,
            epgPublishable,
            clearLivePlaybackSupported,
            requiresCas,
            unsupportedCas,
            missingComponents.joinToString(","),
            reasons.joinToString(","),
            registrationReasons.joinToString(","),
            epgReasons.joinToString(","),
        ).joinToString(":")
    }

    data class ServiceSnapshotSummary(
        val totalKeys: Set<ServiceKey>,
        val completeKeys: Set<ServiceKey>,
        val clearLivePlaybackSupportedKeys: Set<ServiceKey>,
        val registrationReadyKeys: Set<ServiceKey>,
        val epgPublishableKeys: Set<ServiceKey>,
        val completeness: List<ServiceCompleteness>,
    ) {
        val total: Int get() = totalKeys.size
        val complete: Int get() = completeKeys.size
        val clearLivePlaybackSupported: Int get() = clearLivePlaybackSupportedKeys.size
        val registrationReady: Int get() = registrationReadyKeys.size
        val epgPublishable: Int get() = epgPublishableKeys.size
        fun stableSignature(): String = completeness
            .sortedWith(compareBy<ServiceCompleteness> { it.serviceKey.originalNetworkId }
                .thenBy { it.serviceKey.transportStreamId }
                .thenBy { it.serviceKey.serviceId })
            .joinToString("|") { it.signatureToken() }
    }

    fun snapshot(): List<AribService> = engine.serviceRegistrationSnapshot().services

    fun completenessSummary(): ServiceSnapshotSummary {
        val transaction = engine.serviceRegistrationSnapshot()
        val publishability = transaction.publishabilityByServiceKey
        val completeness = transaction.services.map { completenessForModel(it, publishability[it.serviceKey]) }
        return ServiceSnapshotSummary(
            totalKeys = completeness.map { it.serviceKey }.toSet(),
            completeKeys = completeness.filter { it.isComplete }.map { it.serviceKey }.toSet(),
            clearLivePlaybackSupportedKeys = completeness.filter { it.isClearLivePlaybackSupported }.map { it.serviceKey }.toSet(),
            registrationReadyKeys = completeness.filter { it.isRegistrationReady }.map { it.serviceKey }.toSet(),
            epgPublishableKeys = completeness.filter { it.isEpgPublishable }.map { it.serviceKey }.toSet(),
            completeness = completeness,
        )
    }

    fun epgPublishableSnapshot(): List<AribService> {
        val transaction = engine.serviceRegistrationSnapshot()
        val publishability = transaction.publishabilityByServiceKey
        return transaction.services.filter { service -> publishability[service.serviceKey]?.epgPublishable == true }
    }

    fun registrationReadySnapshot(): List<AribService> {
        val transaction = engine.serviceRegistrationSnapshot()
        val publishability = transaction.publishabilityByServiceKey
        return transaction.services.filter { service -> publishability[service.serviceKey]?.channelRegistrationReady == true }
    }

    fun clearLivePlaybackSupportedSnapshot(): List<AribService> {
        val transaction = engine.serviceRegistrationSnapshot()
        val publishability = transaction.publishabilityByServiceKey
        return transaction.services.filter { service -> publishability[service.serviceKey]?.clearLivePlaybackSupported == true }
    }

    fun incompleteReasons(): Map<ServiceKey, List<String>> {
        val transaction = engine.serviceRegistrationSnapshot()
        val publishability = transaction.publishabilityByServiceKey
        val completeness = transaction.services.map { completenessForModel(it, publishability[it.serviceKey]) }
        val reasons = completeness
            .filter { !it.isRegistrationReady }
            .associate { it.serviceKey to (it.missingComponents + it.registrationReasons + it.reasons).distinct() }
        return reasons
    }

    fun isServicePublishable(service: AribService): Boolean =
        engine.serviceRegistrationSnapshot().publishabilityByServiceKey[service.serviceKey]?.publishable == true

    fun isServiceComplete(service: AribService): Boolean = completenessFor(service).isComplete

    fun isServiceClearLivePlaybackSupported(service: AribService): Boolean =
        engine.serviceRegistrationSnapshot().publishabilityByServiceKey[service.serviceKey]?.clearLivePlaybackSupported == true

    fun completenessFor(service: AribService): ServiceCompleteness = completenessForModel(
        service = service,
        publishability = engine.serviceRegistrationSnapshot().publishabilityByServiceKey[service.serviceKey],
    )

    companion object {
        fun completenessForModel(
            service: AribService,
            publishability: ServicePublishabilityDiagnostic?,
        ): ServiceCompleteness {
            val diagnostic = publishability ?: ServicePublishabilityDiagnostic(
                serviceKey = service.serviceKey,
                publishable = false,
                channelRegistrationReady = false,
                epgPublishable = false,
                clearLivePlaybackSupported = false,
                requiresCas = service.requiresCas || service.freeCaMode == true,
                unsupportedCas = service.requiresCas || service.freeCaMode == true,
                pmtPidResolved = service.pmtPid != null,
                pmtParsed = service.pmtPid != null && service.pcrPid != null,
                caStateResolved = service.freeCaMode != null || service.requiresCas,
                freeCaModeResolved = service.freeCaMode != null,
                missingComponents = emptyList(),
                reasons = listOf("NO_RUST_PUBLISHABILITY_DIAGNOSTIC"),
                registrationReasons = listOf("NO_RUST_PUBLISHABILITY_DIAGNOSTIC"),
                epgReasons = listOf("NO_RUST_PUBLISHABILITY_DIAGNOSTIC"),
            )
            return ServiceCompleteness(
                serviceKey = service.serviceKey,
                publishable = diagnostic.publishable,
                channelRegistrationReady = diagnostic.channelRegistrationReady,
                epgPublishable = diagnostic.epgPublishable,
                clearLivePlaybackSupported = diagnostic.clearLivePlaybackSupported,
                requiresCas = diagnostic.requiresCas,
                unsupportedCas = diagnostic.unsupportedCas,
                missingComponents = diagnostic.missingComponents,
                reasons = diagnostic.reasons,
                registrationReasons = diagnostic.registrationReasons,
                epgReasons = diagnostic.epgReasons,
            )
        }
    }
}

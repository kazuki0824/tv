package com.maleicacid.tvinput.aribsi

import com.maleicacid.tvinput.common.ServiceKey

/**
 * TvProvider 登録用のサービス snapshot を構築する。
 * Rust が返す放送由来の意味事実に、現行 product capability を適用する。
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
        val completeness = transaction.services.map {
            completenessForModel(it, transaction.semanticFactsByServiceKey[it.serviceKey])
        }
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
        return transaction.services.filter { service ->
            ServicePolicyEvaluator.evaluate(transaction.semanticFactsByServiceKey[service.serviceKey])
                .epgPublishable
        }
    }

    fun registrationReadySnapshot(): List<AribService> {
        val transaction = engine.serviceRegistrationSnapshot()
        return transaction.services.filter { service ->
            ServicePolicyEvaluator.evaluate(transaction.semanticFactsByServiceKey[service.serviceKey])
                .channelRegistrationReady
        }
    }

    fun clearLivePlaybackSupportedSnapshot(): List<AribService> {
        val transaction = engine.serviceRegistrationSnapshot()
        return transaction.services.filter { service ->
            ServicePolicyEvaluator.evaluate(transaction.semanticFactsByServiceKey[service.serviceKey])
                .clearLivePlaybackSupported
        }
    }

    fun incompleteReasons(): Map<ServiceKey, List<String>> {
        val transaction = engine.serviceRegistrationSnapshot()
        val completeness = transaction.services.map {
            completenessForModel(it, transaction.semanticFactsByServiceKey[it.serviceKey])
        }
        val reasons = completeness
            .filter { !it.isRegistrationReady }
            .associate { it.serviceKey to (it.missingComponents + it.registrationReasons + it.reasons).distinct() }
        return reasons
    }

    fun isServicePublishable(service: AribService): Boolean =
        completenessFor(service).publishable

    fun isServiceComplete(service: AribService): Boolean = completenessFor(service).isComplete

    fun isServiceClearLivePlaybackSupported(service: AribService): Boolean =
        completenessFor(service).clearLivePlaybackSupported

    fun completenessFor(service: AribService): ServiceCompleteness = completenessForModel(
        service = service,
        facts = engine.serviceRegistrationSnapshot().semanticFactsByServiceKey[service.serviceKey],
    )

    companion object {
        fun completenessForModel(
            service: AribService,
            facts: ServiceSemanticFacts?,
            expectedSmdBroadcastingIdentifier: Int? = null,
        ): ServiceCompleteness {
            val diagnostic = ServicePolicyEvaluator.evaluate(
                facts = facts,
                fallbackKey = service.serviceKey,
                expectedSmdBroadcastingIdentifier = expectedSmdBroadcastingIdentifier,
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

object ServicePolicyEvaluator {
    private const val SERVICE_TYPE_DIGITAL_TV = 0x01
    private const val SERVICE_TYPE_DIGITAL_AUDIO = 0x02
    private const val SUPPORTED_SMD = "SUPPORTED_BROADCAST"
    private val SUPPORTED_VIDEO_STREAM_TYPES = setOf(0x02, 0x1b)
    private val SUPPORTED_AUDIO_STREAM_TYPES = setOf(0x03, 0x04, 0x0f)
    private val RECOGNIZED_UNSUPPORTED_VIDEO_STREAM_TYPES = setOf(0x24)
    private val RECOGNIZED_UNSUPPORTED_AUDIO_STREAM_TYPES = setOf(0x11)

    fun evaluate(
        facts: ServiceSemanticFacts?,
        fallbackKey: ServiceKey? = facts?.serviceKey,
        hasPhysicalTune: Boolean = true,
        hasInternalTuneKey: Boolean = true,
        expectedSmdBroadcastingIdentifier: Int? = null,
    ): ServicePublishabilityDiagnostic {
        val key = facts?.serviceKey ?: fallbackKey ?: ServiceKey(0, 0, 0)
        if (facts == null) {
            return ServicePublishabilityDiagnostic(
                serviceKey = key,
                publishable = false,
                channelRegistrationReady = false,
                epgPublishable = false,
                clearLivePlaybackSupported = false,
                requiresCas = false,
                unsupportedCas = false,
                missingComponents = listOf("SERVICE_SEMANTIC_FACTS"),
                reasons = listOf("NO_CURRENT_SERVICE_SEMANTIC_FACTS"),
                registrationReasons = listOf("NO_CURRENT_SERVICE_SEMANTIC_FACTS"),
                epgReasons = listOf("NO_CURRENT_SERVICE_SEMANTIC_FACTS"),
            )
        }

        val registrationReasons = mutableListOf<String>()
        registrationReasons += facts.missingComponents
        if (facts.serviceType !in setOf(SERVICE_TYPE_DIGITAL_TV, SERVICE_TYPE_DIGITAL_AUDIO)) {
            registrationReasons += "UNSUPPORTED_OR_UNRESOLVED_SERVICE_TYPE"
        }
        if (!facts.pmtPidResolved) registrationReasons += "NO_PMT_PID"
        if (!facts.pmtParsed) registrationReasons += "NO_VALID_PMT"
        if (!facts.pcrPidResolved) registrationReasons += "NO_PCR_PID"
        val streamTypes = facts.elementaryStreams.map { it.streamType }.toSet()
        when (facts.serviceType) {
            SERVICE_TYPE_DIGITAL_TV -> if (streamTypes.none(SUPPORTED_VIDEO_STREAM_TYPES::contains)) {
                registrationReasons += if (streamTypes.any(RECOGNIZED_UNSUPPORTED_VIDEO_STREAM_TYPES::contains)) {
                    "NO_SUPPORTED_VIDEO_CODEC"
                } else {
                    "NO_VIDEO_ES"
                }
            }
            SERVICE_TYPE_DIGITAL_AUDIO -> if (streamTypes.none(SUPPORTED_AUDIO_STREAM_TYPES::contains)) {
                registrationReasons += if (streamTypes.any(RECOGNIZED_UNSUPPORTED_AUDIO_STREAM_TYPES::contains)) {
                    "NO_SUPPORTED_AUDIO_CODEC"
                } else {
                    "NO_AUDIO_ES"
                }
            }
        }
        if (facts.smd.semanticState != SUPPORTED_SMD) {
            registrationReasons += facts.smd.semanticState
        } else if (
            expectedSmdBroadcastingIdentifier != null &&
            facts.smd.broadcastingIdentifier != expectedSmdBroadcastingIdentifier
        ) {
            registrationReasons += "UNSUPPORTED_BROADCAST_SYSTEM"
        }
        if (!hasPhysicalTune) registrationReasons += "NO_PHYSICAL_TUNE"
        if (!hasInternalTuneKey) registrationReasons += "NO_INTERNAL_TUNE_KEY"
        val normalizedRegistrationReasons = registrationReasons.distinct().sorted()
        val registrationReady = normalizedRegistrationReasons.isEmpty()
        val unsupportedCas = facts.requiresCas
        val clearLivePlaybackSupported = registrationReady && !unsupportedCas
        val reasons = (normalizedRegistrationReasons + facts.semanticDiagnostics +
            if (unsupportedCas) listOf("CAS_NOT_IMPLEMENTED") else emptyList()).distinct().sorted()
        return ServicePublishabilityDiagnostic(
            serviceKey = key,
            publishable = registrationReady,
            channelRegistrationReady = registrationReady,
            epgPublishable = registrationReady,
            clearLivePlaybackSupported = clearLivePlaybackSupported,
            requiresCas = facts.requiresCas,
            unsupportedCas = unsupportedCas,
            pmtPidResolved = facts.pmtPidResolved,
            pmtParsed = facts.pmtParsed,
            caStateResolved = facts.caDescriptorsResolved,
            freeCaModeResolved = facts.freeCaMode != null,
            missingComponents = facts.missingComponents,
            reasons = reasons,
            registrationReasons = normalizedRegistrationReasons,
            epgReasons = normalizedRegistrationReasons,
        )
    }
}

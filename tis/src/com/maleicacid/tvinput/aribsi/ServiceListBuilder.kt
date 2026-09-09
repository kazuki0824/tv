package com.maleicacid.tvinput.aribsi

import com.maleicacid.tvinput.common.ServiceKey

/**
 * TvProvider 登録用のサービス snapshot を構築する。
 * Rust が返す放送由来の意味事実に、現行 product capability を適用する。
 */
class ServiceListBuilder(private val engine: AribSiEngine) {
    data class ServiceCompleteness(
        val decision: ServicePolicyDecision,
    ) {
        val serviceKey: ServiceKey get() = decision.serviceKey
        val registrationReady: Boolean get() = decision.registrationReady
        val clearLivePlaybackStaticallyEligible: Boolean get() = decision.clearLivePlaybackStaticallyEligible
        val requiresCas: Boolean get() = decision.requiresCas
        val reasons: List<String> get() = decision.reasons
        fun signatureToken(): String = listOf(
            serviceKey.originalNetworkId,
            serviceKey.transportStreamId,
            serviceKey.serviceId,
            decision.registrationReady,
            requiresCas,
            reasons.joinToString(","),
        ).joinToString(":")
    }

    data class ServiceSnapshotSummary(
        val completeness: List<ServiceCompleteness>,
    ) {
        val totalKeys: Set<ServiceKey> get() = completeness.mapTo(linkedSetOf()) { it.serviceKey }
        val clearLivePlaybackStaticallyEligibleKeys: Set<ServiceKey>
            get() = completeness.filterTo(mutableListOf()) { it.clearLivePlaybackStaticallyEligible }.mapTo(linkedSetOf()) { it.serviceKey }
        val registrationReadyKeys: Set<ServiceKey>
            get() = completeness.filterTo(mutableListOf()) { it.registrationReady }.mapTo(linkedSetOf()) { it.serviceKey }
        val total: Int get() = totalKeys.size
        val clearLivePlaybackStaticallyEligible: Int get() = clearLivePlaybackStaticallyEligibleKeys.size
        val registrationReady: Int get() = registrationReadyKeys.size
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
            completeness = completeness,
        )
    }

    fun registrationReadySnapshot(): List<AribService> {
        val transaction = engine.serviceRegistrationSnapshot()
        return transaction.services.filter { service ->
            ServicePolicyEvaluator.evaluate(transaction.semanticFactsByServiceKey[service.serviceKey])
                .registrationReady
        }
    }

    fun clearLivePlaybackStaticallyEligibleSnapshot(): List<AribService> {
        val transaction = engine.serviceRegistrationSnapshot()
        return transaction.services.filter { service ->
            ServicePolicyEvaluator.evaluate(transaction.semanticFactsByServiceKey[service.serviceKey])
                .clearLivePlaybackStaticallyEligible
        }
    }

    fun incompleteReasons(): Map<ServiceKey, List<String>> {
        val transaction = engine.serviceRegistrationSnapshot()
        val completeness = transaction.services.map {
            completenessForModel(it, transaction.semanticFactsByServiceKey[it.serviceKey])
        }
        val reasons = completeness
            .filter { !it.registrationReady }
            .associate { it.serviceKey to it.reasons }
        return reasons
    }

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
            return ServiceCompleteness(diagnostic)
        }
    }
}

object ServicePolicyEvaluator {
    private const val SERVICE_TYPE_DIGITAL_TV = 0x01
    private const val SERVICE_TYPE_DIGITAL_AUDIO = 0x02
    private const val SUPPORTED_SMD = "SUPPORTED_BROADCAST"
    private val RECOGNIZED_UNSUPPORTED_VIDEO_STREAM_TYPES = setOf(0x24)
    private val RECOGNIZED_UNSUPPORTED_AUDIO_STREAM_TYPES = setOf(0x11)

    fun expectedSmdBroadcastingIdentifier(profile: Int): Int? = when (profile) {
        SiDiscoveryProfile.ISDB_T -> 0b000011
        SiDiscoveryProfile.BS -> 0b000010
        SiDiscoveryProfile.CS110 -> 0b000100
        else -> null
    }

    fun evaluateLive(snapshot: LivePlaybackSnapshot?, key: ServiceKey?): ServicePolicyDecision = evaluate(
        facts = snapshot?.semanticFactsByServiceKey?.get(key),
        fallbackKey = key,
        expectedSmdBroadcastingIdentifier = snapshot?.programs?.discoveryProfile?.let(::expectedSmdBroadcastingIdentifier),
    )

    fun evaluate(
        facts: ServiceSemanticFacts?,
        fallbackKey: ServiceKey? = facts?.serviceKey,
        hasPhysicalTune: Boolean = true,
        hasInternalTuneKey: Boolean = true,
        expectedSmdBroadcastingIdentifier: Int? = null,
    ): ServicePublishabilityDiagnostic {
        val key = facts?.serviceKey ?: fallbackKey ?: ServiceKey(0, 0, 0)
        if (facts == null) {
            return ServicePolicyDecision(
                serviceKey = key,
                registrationReady = false,
                requiresCas = false,
                reasons = listOf("NO_CURRENT_SERVICE_SEMANTIC_FACTS"),
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
            SERVICE_TYPE_DIGITAL_TV -> if (facts.elementaryStreams.none(com.maleicacid.tvinput.tis.TunerSelectionPolicy::isSupportedVideoStream)) {
                registrationReasons += if (streamTypes.any { com.maleicacid.tvinput.tis.TunerSelectionPolicy.isSupportedVideoStreamType(it) || it in RECOGNIZED_UNSUPPORTED_VIDEO_STREAM_TYPES }) {
                    "NO_SUPPORTED_VIDEO_CODEC"
                } else {
                    "NO_VIDEO_ES"
                }
            }
            SERVICE_TYPE_DIGITAL_AUDIO -> if (facts.elementaryStreams.none(com.maleicacid.tvinput.tis.TunerSelectionPolicy::isSupportedAudioStream)) {
                registrationReasons += if (streamTypes.any { com.maleicacid.tvinput.tis.TunerSelectionPolicy.isSupportedAudioStreamType(it) || it in RECOGNIZED_UNSUPPORTED_AUDIO_STREAM_TYPES }) {
                    "NO_SUPPORTED_AUDIO_CODEC"
                } else {
                    "NO_AUDIO_ES"
                }
            }
        }
        if (!facts.caDescriptorsResolved) registrationReasons += "CA_DESCRIPTOR_UNRESOLVED"
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
        return ServicePolicyDecision(
            serviceKey = key,
            registrationReady = registrationReady,
            requiresCas = facts.requiresCas,
            reasons = (
                normalizedRegistrationReasons +
                    facts.semanticDiagnostics +
                    if (facts.requiresCas) listOf("CAS_NOT_IMPLEMENTED") else emptyList()
                ).distinct().sorted(),
        )
    }
}

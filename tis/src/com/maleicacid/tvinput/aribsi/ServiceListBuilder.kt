package com.maleicacid.tvinput.aribsi

import com.maleicacid.tvinput.common.ServiceKey

/**
 * TvProvider 登録用のサービス snapshot を構築する。
 * Rust が返す放送由来の意味事実に、現行 product capability を適用する。
 */
class ServiceListBuilder(
    private val engine: AribSiEngine,
) {
    data class ServiceCompleteness(
        val decision: ServicePolicyDecision,
    ) {
        val serviceKey: ServiceKey get() = decision.serviceKey
        val registrationReady: Boolean get() = decision.registrationReady
        val clearLivePlaybackStaticallyEligible: Boolean get() = decision.clearLivePlaybackStaticallyEligible
        val requiresCas: Boolean get() = decision.requiresCas
        val reasons: List<String> get() = decision.reasons

        fun signatureToken(): String =
            listOf(
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
            // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
            @Suppress("MaxLineLength")
            get() = completeness.filterTo(mutableListOf()) { it.clearLivePlaybackStaticallyEligible }.mapTo(linkedSetOf()) { it.serviceKey }
        val registrationReadyKeys: Set<ServiceKey>
            // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
            @Suppress("MaxLineLength")
            get() = completeness.filterTo(mutableListOf()) { it.registrationReady }.mapTo(linkedSetOf()) { it.serviceKey }
        val total: Int get() = totalKeys.size
        val clearLivePlaybackStaticallyEligible: Int get() = clearLivePlaybackStaticallyEligibleKeys.size
        val registrationReady: Int get() = registrationReadyKeys.size

        fun stableSignature(): String =
            completeness
                .sortedWith(
                    compareBy<ServiceCompleteness> { it.serviceKey.originalNetworkId }
                        .thenBy { it.serviceKey.transportStreamId }
                        .thenBy { it.serviceKey.serviceId },
                ).joinToString("|") { it.signatureToken() }
    }

    fun snapshot(): List<AribService> = engine.serviceRegistrationSnapshot().services

    companion object {
        fun completenessForModel(
            service: AribService,
            facts: ServiceSemanticFacts?,
            expectedSmdBroadcastSystem: BroadcastSystem? = null,
        ): ServiceCompleteness {
            val diagnostic =
                ServicePolicyEvaluator.evaluate(
                    facts = facts,
                    fallbackKey = service.serviceKey,
                    expectedSmdBroadcastSystem = expectedSmdBroadcastSystem,
                )
            return ServiceCompleteness(diagnostic)
        }
    }
}

object ServicePolicyEvaluator {
    private const val SERVICE_TYPE_DIGITAL_TV = 0x01
    private const val SERVICE_TYPE_DIGITAL_AUDIO = 0x02

    // この処理の規格値・ビット幅・単位換算・固定上限をリテラルのまま照合できる形に保つ。
    @Suppress("MagicNumber")
    private val RECOGNIZED_UNSUPPORTED_VIDEO_STREAM_TYPES = setOf(0x24)

    // この処理の規格値・ビット幅・単位換算・固定上限をリテラルのまま照合できる形に保つ。
    @Suppress("MagicNumber")
    private val RECOGNIZED_UNSUPPORTED_AUDIO_STREAM_TYPES = setOf(0x11)

    fun expectedSmdBroadcastSystem(profile: Int): BroadcastSystem? =
        when (profile) {
            SiDiscoveryProfile.ISDB_T -> BroadcastSystem.ISDB_T
            SiDiscoveryProfile.BS -> BroadcastSystem.ISDB_S_BS
            SiDiscoveryProfile.CS110 -> BroadcastSystem.ISDB_S_110CS
            else -> null
        }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    fun evaluateLive(
        snapshot: LivePlaybackSnapshot?,
        key: ServiceKey?,
    ): ServicePolicyDecision =
        evaluate(
            facts = snapshot?.semanticFactsByServiceKey?.get(key),
            fallbackKey = key,
            expectedSmdBroadcastSystem = snapshot?.programs?.discoveryProfile?.let(::expectedSmdBroadcastSystem),
        )

    // 同じ入力に対する分岐・項目写像を保持し、処理分割による状態の受け渡しを増やさない。
    // 同じ入力と資源寿命を扱う手順を一続きに確認できる形に保つ。
    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("CyclomaticComplexMethod", "LongMethod", "MaxLineLength")
    fun evaluate(
        facts: ServiceSemanticFacts?,
        fallbackKey: ServiceKey? = facts?.serviceKey,
        hasPhysicalTune: Boolean = true,
        hasInternalTuneKey: Boolean = true,
        expectedSmdBroadcastSystem: BroadcastSystem? = null,
    ): ServicePublishabilityDiagnostic {
        val key = facts?.serviceKey ?: fallbackKey ?: ServiceKey(0, 0, 0)
        if (facts == null) {
            return ServicePolicyDecision(
                serviceKey = key,
                registrationReady = false,
                requiresCas = false,
                caDescriptorsResolved = false,
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
            SERVICE_TYPE_DIGITAL_TV -> {
                if (facts.elementaryStreams.none(com.maleicacid.tvinput.tis.TunerSelectionPolicy::isSupportedVideoStream)) {
                    registrationReasons +=
                        if (streamTypes.any {
                                com.maleicacid.tvinput.tis.TunerSelectionPolicy
                                    .isSupportedVideoStreamType(it) ||
                                    it in RECOGNIZED_UNSUPPORTED_VIDEO_STREAM_TYPES
                            }
                        ) {
                            "NO_SUPPORTED_VIDEO_CODEC"
                        } else {
                            "NO_VIDEO_ES"
                        }
                }
            }

            SERVICE_TYPE_DIGITAL_AUDIO -> {
                if (facts.elementaryStreams.none(com.maleicacid.tvinput.tis.TunerSelectionPolicy::isSupportedAudioStream)) {
                    registrationReasons +=
                        if (streamTypes.any {
                                com.maleicacid.tvinput.tis.TunerSelectionPolicy
                                    .isSupportedAudioStreamType(it) ||
                                    it in RECOGNIZED_UNSUPPORTED_AUDIO_STREAM_TYPES
                            }
                        ) {
                            "NO_SUPPORTED_AUDIO_CODEC"
                        } else {
                            "NO_AUDIO_ES"
                        }
                }
            }
        }
        if (facts.smd.semanticState != SmdSemanticState.SUPPORTED_BROADCAST) {
            registrationReasons += facts.smd.semanticState.wireValue
        } else if (
            expectedSmdBroadcastSystem != null &&
            facts.smd.broadcastSystem != expectedSmdBroadcastSystem
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
            caDescriptorsResolved = facts.caDescriptorsResolved,
            reasons =
                (
                    normalizedRegistrationReasons +
                        facts.semanticDiagnostics +
                        (if (!facts.caDescriptorsResolved) listOf("CA_DESCRIPTOR_UNRESOLVED") else emptyList()) +
                        if (facts.requiresCas) listOf("CAS_NOT_IMPLEMENTED") else emptyList()
                ).distinct().sorted(),
        )
    }
}

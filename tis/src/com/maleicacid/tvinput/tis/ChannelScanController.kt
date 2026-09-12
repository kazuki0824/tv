package com.maleicacid.tvinput.tis

import android.content.Context
import android.media.tv.TvInputService
import android.media.tv.tuner.Tuner
import android.util.Log
import com.maleicacid.tvinput.aribsi.AribRatingMapper
import com.maleicacid.tvinput.aribsi.AribService
import com.maleicacid.tvinput.aribsi.AribSiEngine
import com.maleicacid.tvinput.aribsi.EventModelMapper
import com.maleicacid.tvinput.aribsi.PmtCatCaMetadataMapper
import com.maleicacid.tvinput.aribsi.SectionIngestController
import com.maleicacid.tvinput.aribsi.ServiceListBuilder
import com.maleicacid.tvinput.aribsi.ServicePolicyEvaluator
import com.maleicacid.tvinput.aribsi.SiDiscoveryProfile
import com.maleicacid.tvinput.aribsi.SiDiscoveryStage
import com.maleicacid.tvinput.aribsi.TransportKey
import com.maleicacid.tvinput.common.LogTags
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.TsPid
import com.maleicacid.tvinput.db.ChannelRecord
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong

// 走査状態と公開処理の所有を一か所に保ち、関数数だけを理由に別の所有者へ分散しない。
@Suppress("LargeClass", "TooManyFunctions")
class ChannelScanController(
    private val context: Context,
    private val inputId: String,
    private val engine: AribSiEngine,
    scanPurpose: ScanPurpose,
    private val cancelRequested: AtomicBoolean = AtomicBoolean(false),
) : AutoCloseable {
    data class ScanDiagnostic(
        val candidate: ScanCandidate,
        val message: String,
    )

    data class ScanResult(
        val scanned: Int,
        val published: Int,
        val diagnostics: List<ScanDiagnostic>,
        val successfulCandidates: Int = 0,
        val terminalCancelObserved: Boolean = false,
        val terminalResourceLostObserved: Boolean = false,
        val committedServiceKeys: Set<ServiceKey> = emptySet(),
    )

    enum class SiCollectionOutcome {
        COMPLETE,
        STABLE_PARTIAL,
        TIMEOUT_PARTIAL,
        INCOMPLETE_NO_REGISTRATION_READY_SERVICE,
        CANCELLED,
        RESOURCE_LOST,
    }

    data class SiCollectionResult(
        val outcome: SiCollectionOutcome,
        val diagnostic: ScanDiagnostic?,
        val clearLivePlaybackStaticallyEligibleServices: Int,
        val registrationReadyServices: Int = clearLivePlaybackStaticallyEligibleServices,
    ) {
        val mayPublishChannels: Boolean
            get() =
                outcome != SiCollectionOutcome.CANCELLED &&
                    outcome != SiCollectionOutcome.RESOURCE_LOST &&
                    outcome != SiCollectionOutcome.INCOMPLETE_NO_REGISTRATION_READY_SERVICE &&
                    registrationReadyServices > 0
    }

    data class SiCollectionPolicy(
        val minWaitMs: Long = 2_000L,
        val maxWaitMs: Long = 12_000L,
        val stableWaitMs: Long = 1_200L,
        val pollIntervalMs: Long = 200L,
    )

    enum class PublishMode {
        SETUP_SCAN,
        LIVE_TUNE_REFRESH,
        BOOT_EPG_SYNC,
        BACKGROUND_CHANNEL_MAINTENANCE,
        DIAGNOSTIC_ONLY,
    }

    private data class ServiceCounts(
        val discoveryStage: Int,
        val collectionStatus: SiCollectionRequirements.Status,
        val total: Int,
        val clearLivePlaybackStaticallyEligible: Int,
        val registrationReady: Int,
        val signature: String,
        val incompleteReasons: Map<ServiceKey, List<String>>,
    )

    private data class PublishSnapshotResult(
        val changed: Int,
        val failures: List<TvProviderWriter.Diagnostic> = emptyList(),
        val hasCommittedProgramTarget: Boolean = false,
        val committedServiceKeys: Set<ServiceKey> = emptySet(),
    ) {
        val success: Boolean get() = failures.isEmpty()
    }

    private val tunerController =
        TunerController(
            context,
            inputId,
            tunerPriorityHintUseCase(scanPurpose),
        )
    private val ingestController = SectionIngestController(engine)
    private val tvProviderWriter = TvProviderWriter(context, inputId)
    private val programPublishCoordinator = ProgramPublishCoordinator(tvProviderWriter)
    private val caMapper = PmtCatCaMetadataMapper()
    private val casController = CasController()
    private val cancelled = cancelRequested
    private var terminalCancelObserved: Boolean = false
    private val resourceLossFence = ResourceLossFence()
    private val terminalResourceLostObserved: Boolean get() = resourceLossFence.terminalObserved
    private var skippedUnresolvedTransportCount: Int = 0
    private var currentCandidate: ScanCandidate? = null

    init {
        tunerController.setSectionIngestController(ingestController)
        tunerController.setCasController(casController)
        tunerController.setOnSectionIngestedCallback { refreshDynamicSectionFilters() }
        tunerController.setOnTunerResourceLostCallback { lostGeneration ->
            resourceLossFence.onLost(lostGeneration)
        }
    }

    // 候補展開から収集・公開まで同じ走査状態を使うため、分岐数と長さによる機械的分割を避ける。
    // 入れ子はBS候補展開と世代のfinallyを表し、各breakは後続候補を止める異なる終端理由を保持する。
    @Suppress("LongMethod", "CyclomaticComplexMethod", "NestedBlockDepth", "LoopWithTooManyJumpStatements", "MaxLineLength")
    fun startInitialScan(candidates: List<ScanCandidate> = JapanIsdbScanPlan.defaultInitialScan()): ScanResult {
        if (!cancelled.get()) cancelled.set(false)
        terminalCancelObserved = cancelled.get()
        resetResourceLostState()
        skippedUnresolvedTransportCount = 0
        val diagnostics = mutableListOf<ScanDiagnostic>()
        var published = 0
        var successfulCandidates = 0
        var scannedCandidates = 0
        var bsCandidateSource: BsCandidateSource? = null

        // 選局失敗、公開不可、資源喪失を発生点で返し、finallyによる世代の後始末を共通に保つ。
        @Suppress("ReturnCount")
        fun scanExecutionCandidate(candidate: ScanCandidate): Boolean {
            if (cancelled.get() || terminalResourceLostObserved) return false
            engine.reset(discoveryProfile(candidate.kind))
            currentCandidate = candidate
            val tune = tunerController.tuneForScan(candidate)
            if (!tune.success) {
                diagnostics += ScanDiagnostic(candidate, "選局に失敗しました result=${tune.resultCode} ${tune.message}")
                return true
            }
            activateScanGeneration(tune.generation)
            try {
                val collection =
                    collectSiForCandidate(
                        candidate,
                        SiCollectionRequirements(PublishMode.SETUP_SCAN, discoveryProfile(candidate.kind)),
                        tune.generation,
                    )
                collection.diagnostic?.let { diagnostics += it }
                if (!collection.mayPublishChannels) {
                    Log.w(
                        LogTags.TIS,
                        "SI discovery 未完了のため TvProvider channel 登録を省略します candidate=$candidate " +
                            "outcome=${collection.outcome} registrationReady=${collection.registrationReadyServices} " +
                            "clearLivePlaybackStaticallyEligible=${collection.clearLivePlaybackStaticallyEligibleServices} " +
                            "diagnostic=${collection.diagnostic?.message}",
                    )
                    return collection.outcome != SiCollectionOutcome.RESOURCE_LOST
                }
                val publishResult = publishScanSnapshotIfCurrent(tune.generation, PublishMode.SETUP_SCAN)
                if (publishResult == null) {
                    diagnostics += resourceLostDiagnostic(candidate, tune.generation)
                    return false
                }
                if (
                    collection.outcome == SiCollectionOutcome.COMPLETE &&
                    collection.registrationReadyServices > 0 &&
                    publishResult.success
                ) {
                    successfulCandidates++
                }
                published += publishResult.changed
                return true
            } finally {
                clearActiveScanGeneration(tune.generation)
            }
        }

        scanLoop@ for (candidate in candidates) {
            if (cancelled.get() || terminalResourceLostObserved) break
            val executionCandidates =
                if (
                    candidate.kind == ScanCandidateKind.ISDB_S_BS &&
                    candidate.streamSelector == com.maleicacid.tvinput.common.StreamSelector.NONE
                ) {
                    if (bsCandidateSource == null) {
                        val selection = tunerController.prepareBsCandidateSource()
                        if (!selection.success) {
                            diagnostics +=
                                ScanDiagnostic(
                                    candidate,
                                    "BS frontend選択に失敗しました result=${selection.resultCode} message=${selection.message}",
                                )
                            break@scanLoop
                        }
                        bsCandidateSource = requireNotNull(selection.source)
                    }
                    when (bsCandidateSource) {
                        BsCandidateSource.DYNAMIC_STREAM_ID_LIST -> {
                            val discovery = tunerController.discoverIsdbsStreamIds(candidate)
                            discovery.generation?.let { activateScanGeneration(it) }
                            if (discovery.resourceLost || terminalResourceLostObserved) {
                                discovery.generation?.let { resourceLossFence.onLost(it) }
                                diagnostics +=
                                    ScanDiagnostic(
                                        candidate,
                                        "BS探索中のTUNER_RESOURCE_LOSTにより後続選局を停止します",
                                    )
                                break@scanLoop
                            }
                            discovery.generation?.let { clearActiveScanGeneration(it) }
                            val discovered = discovery.candidatesFor(candidate)
                            if (discovery.success && discovered.isNotEmpty()) {
                                discovered
                            } else {
                                diagnostics +=
                                    ScanDiagnostic(
                                        candidate,
                                        "BS dynamic stream-ID discovery失敗 result=${discovery.resultCode} " +
                                            "message=${discovery.message}",
                                    )
                                emptyList()
                            }
                        }

                        BsCandidateSource.STATIC_TSID_TABLE -> {
                            JapanIsdbScanPlan.staticBsCandidatesFor(candidate)
                        }

                        null -> {
                            error("BS候補sourceが確定していません")
                        }
                    }
                } else {
                    listOf(candidate)
                }
            scannedCandidates += executionCandidates.size
            for (executionCandidate in executionCandidates) {
                if (!scanExecutionCandidate(executionCandidate)) break@scanLoop
            }
        }
        currentCandidate = null
        return ScanResult(
            scannedCandidates,
            published,
            diagnostics,
            successfulCandidates = successfulCandidates,
            terminalCancelObserved = terminalCancelObserved,
            terminalResourceLostObserved = terminalResourceLostObserved,
        )
    }

    fun startBootEpgSync(targetChannels: List<ChannelRecord>): ScanResult =
        runMaintenanceScan(
            targetChannels = targetChannels,
            mode = PublishMode.BOOT_EPG_SYNC,
            failurePrefix = "boot後EPG同期",
        )

    fun startBackgroundChannelMaintenance(): ScanResult {
        val channels = tvProviderWriter.existingChannelsResult().getOrThrow()
        return runMaintenanceScan(
            targetChannels = channels,
            mode = PublishMode.BACKGROUND_CHANNEL_MAINTENANCE,
            failurePrefix = "background channel maintenance",
        )
    }

    // 必須サービス集合、収集結果、公開確定を同じ候補処理で照合するため、状態を別の処理へ分散しない。
    // try/finallyの内側でも資源喪失は全体終了、通常の選局・収集失敗は次候補へ進む区別を保つ。
    // 成功判定の4条件は収集完了・登録可能・公開成功・確定対象ありを全て要求し、省略しない。
    @Suppress(
        "LongMethod",
        "CyclomaticComplexMethod",
        "NestedBlockDepth",
        "LoopWithTooManyJumpStatements",
        "MaxLineLength",
    )
    private fun runMaintenanceScan(
        targetChannels: List<ChannelRecord>,
        mode: PublishMode,
        failurePrefix: String,
    ): ScanResult {
        val allowedServiceKeys = targetChannels.map { it.serviceKey }.toSet()
        val targetsByTune =
            targetChannels
                .mapNotNull { channel ->
                    scanCandidateFromChannel(channel)?.let { it to channel.serviceKey }
                }.groupBy { it.first.tuneKey }
        val candidates = targetsByTune.values.map { it.first().first }
        if (!cancelled.get()) cancelled.set(false)
        terminalCancelObserved = cancelled.get()
        resetResourceLostState()
        skippedUnresolvedTransportCount = 0
        val diagnostics = mutableListOf<ScanDiagnostic>()
        val committedServiceKeys = linkedSetOf<ServiceKey>()
        var updated = 0
        var successfulCandidates = 0
        for (candidate in candidates) {
            if (cancelled.get() || terminalResourceLostObserved) break
            engine.reset(discoveryProfile(candidate.kind))
            currentCandidate = candidate
            val tune = tunerController.tuneForScan(candidate)
            if (!tune.success) {
                diagnostics += ScanDiagnostic(candidate, "${failurePrefix}の選局に失敗しました result=${tune.resultCode} ${tune.message}")
                continue
            }
            activateScanGeneration(tune.generation)
            try {
                val requiredServiceKeys = targetsByTune.getValue(candidate.tuneKey).mapTo(linkedSetOf()) { it.second }
                val collection =
                    collectSiForCandidate(
                        candidate,
                        SiCollectionRequirements(mode, discoveryProfile(candidate.kind), requiredServiceKeys),
                        tune.generation,
                    )
                collection.diagnostic?.let { diagnostics += it }
                if (!collection.mayPublishChannels) {
                    Log.w(
                        LogTags.TIS,
                        "$failurePrefix SI discovery 未完了のため Programs publish/delete を省略します " +
                            "candidate=$candidate outcome=${collection.outcome} " +
                            "registrationReady=${collection.registrationReadyServices}",
                    )
                    if (collection.outcome == SiCollectionOutcome.RESOURCE_LOST) break
                    continue
                }
                val publishResult = publishScanSnapshotIfCurrent(tune.generation, mode, allowedServiceKeys)
                if (publishResult == null) {
                    diagnostics += resourceLostDiagnostic(candidate, tune.generation)
                    break
                }
                committedServiceKeys += publishResult.committedServiceKeys
                val candidateSuccessfullyPublished =
                    collection.outcome == SiCollectionOutcome.COMPLETE &&
                        collection.registrationReadyServices > 0 &&
                        publishResult.success &&
                        publishResult.hasCommittedProgramTarget
                if (candidateSuccessfullyPublished) {
                    successfulCandidates++
                }
                updated += publishResult.changed
            } finally {
                clearActiveScanGeneration(tune.generation)
            }
        }
        currentCandidate = null
        return ScanResult(
            candidates.size,
            updated,
            diagnostics,
            successfulCandidates = successfulCandidates,
            terminalCancelObserved = terminalCancelObserved,
            terminalResourceLostObserved = terminalResourceLostObserved,
            committedServiceKeys = committedServiceKeys,
        )
    }

    fun cancelScan() {
        cancelled.set(true)
        terminalCancelObserved = true
    }

    fun beginSiIngestAfterTune() {
        if (tunerController.beginSiIngestAfterTune()) {
            refreshDynamicSectionFilters()
            publishCurrentServiceSnapshot(PublishMode.LIVE_TUNE_REFRESH)
        }
    }

    /** 完全な section を受ける入口。byte array は 生 TS packet ではない。 */
    fun onSection(
        pid: Int,
        section: ByteArray,
    ) {
        val tsPid = TsPid.fromOrNull(pid) ?: return
        tunerController.onSection(tsPid, section)
        refreshDynamicSectionFilters()
        publishCurrentServiceSnapshot(PublishMode.LIVE_TUNE_REFRESH)
    }

    fun refreshDynamicSectionFilters() {
        if (terminalResourceLostObserved) return
        val generation = tunerController.currentGeneration()
        val transaction = engine.casDiscoverySnapshot()
        val servicesForCas = transaction.services
        val allCaMetadata = if (ENABLE_CAS_ORCHESTRATION) transaction.caMetadata else emptyList()
        val serviceScopedCa =
            allCaMetadata.filter {
                it.source != com.maleicacid.tvinput.aribsi.CaMetadataSource.CAT &&
                    it.serviceKey != null
            }
        val catCa = allCaMetadata.filter { it.source == com.maleicacid.tvinput.aribsi.CaMetadataSource.CAT }
        val caMetadata = caMapper.expandProgramLevelToElementaryStreams(serviceScopedCa + catCa, servicesForCas)
        val pmtPids = transaction.pmtPids.values.toSet()
        val unsupported = caMapper.unsupportedForB25B1(caMetadata, CasController.SupportedCasSystemIds.B25_B1)
        unsupported.forEach { Log.w(LogTags.TIS, "対象外 CA情報 を無視します caSystemId=${it.caSystemId}") }
        tunerController.updateCasMetadataAndFilters(caMetadata, pmtPids, generation, casDecisionReady = true)
    }

    // 一つの受信snapshotから登録・公開する経路と早期終了を維持する。長い型・診断項目だけ行長を許容する。
    @Suppress("LongMethod", "ReturnCount", "MaxLineLength")
    private fun publishCurrentServiceSnapshot(
        mode: PublishMode,
        allowedServiceKeys: Set<ServiceKey>? = null,
    ): PublishSnapshotResult {
        if (mode == PublishMode.DIAGNOSTIC_ONLY) return PublishSnapshotResult(0)
        if (mode == PublishMode.LIVE_TUNE_REFRESH || mode == PublishMode.BOOT_EPG_SYNC ||
            mode == PublishMode.BACKGROUND_CHANNEL_MAINTENANCE
        ) {
            val result = publishProgramsForRegisteredServices(mode, allowedServiceKeys)
            Log.d(
                LogTags.TIS,
                "$mode では新規 channel row を追加しません changed=${result.changed} " +
                    "skippedNoChannel=${result.skippedNoChannel} skippedUnchanged=${result.skippedUnchanged}",
            )
            return PublishSnapshotResult(
                changed = result.changed,
                failures = result.failures,
                hasCommittedProgramTarget = result.hasCommittedTarget,
                committedServiceKeys = result.committedServiceKeys,
            )
        }
        val candidate = currentCandidate ?: return PublishSnapshotResult(0)
        val transaction = engine.serviceRegistrationSnapshot()
        val transportRemoteKeys =
            transaction.actualTransportMetadata.associate { transport ->
                TransportKey(transport.originalNetwork, transport.transportStream) to transport.remoteControlKeyId
            }
        val diagnostics =
            transaction.semanticFactsByServiceKey.mapValues { (key, facts) ->
                ServicePolicyEvaluator.evaluate(
                    facts = facts,
                    fallbackKey = key,
                    hasPhysicalTune = candidate.frequencyHz.value > 0L,
                    hasInternalTuneKey =
                        candidate.streamSelector.value != null ||
                            candidate.streamSelector == com.maleicacid.tvinput.common.StreamSelector.NONE,
                    expectedSmdBroadcastingIdentifier = expectedSmdBroadcastingIdentifier(candidate),
                )
            }
        val registrationReadyServices =
            transaction.services.filter { service ->
                diagnostics[service.serviceKey]?.registrationReady == true
            }
        val services = filterServicesForCurrentCandidate(registrationReadyServices, transaction.actualTransports)
        val channels =
            services.mapNotNull { service ->
                val serviceType = service.serviceType ?: return@mapNotNull null
                val remoteKey = transportRemoteKeys[TransportKey(service.serviceKey.originalNetwork, service.serviceKey.transportStream)]
                ChannelRecord(
                    serviceKey = service.serviceKey,
                    displayNumber = ChannelNumberingPolicy.displayNumber(service, remoteKey, candidate),
                    displayName =
                        service.name?.takeIf { it.isNotEmpty() }
                            ?: run {
                                "service-${service.serviceKey.originalNetworkId}-" +
                                    "${service.serviceKey.transportStreamId}-${service.serviceKey.serviceId}"
                            },
                    frequencyHz = candidate.frequencyHz,
                    deliverySystem = candidate.deliverySystem,
                    streamSelector = candidate.streamSelector,
                    physicalChannel = candidate.physicalChannel,
                    backendHint = candidate.backendHint,
                    satelliteBand = candidate.satelliteBand,
                    remoteControlKeyId = remoteKey,
                    serviceType = serviceType,
                    requiresCas = transaction.semanticFactsByServiceKey[service.serviceKey]?.requiresCas == true,
                    casFactsCanonicalJson = transaction.semanticFactsByServiceKey[service.serviceKey]?.casFactsCanonicalJson,
                )
            }
        if (channels.isEmpty()) {
            val incomplete =
                diagnostics
                    .filterValues { !it.registrationReady }
                    .mapValues { (_, decision) -> decision.reasons }
            Log.d(
                LogTags.TIS,
                "registration-ready なサービスがないため channel snapshot 登録を省略します candidate=" +
                    "$candidate stage=${transaction.discoveryStage} incomplete=$incomplete",
            )
            return PublishSnapshotResult(0)
        }
        val channelResult = tvProviderWriter.upsertChannels(channels)
        if (channelResult.failures.isNotEmpty()) Log.w(LogTags.TIS, "TvProvider channel 登録失敗=${channelResult.failures}")
        val programResult =
            publishProgramsForRegisteredServices(PublishMode.SETUP_SCAN, allowedServiceKeys = channels.map { it.serviceKey }.toSet())
        return PublishSnapshotResult(
            changed = channelResult.inserted + channelResult.updated,
            failures = channelResult.failures + programResult.failures,
        )
    }

    // TransportKeyの完全な照合式と診断を一続きに読めるようにする。
    @Suppress("MaxLineLength")
    private fun filterServicesForCurrentCandidate(
        services: List<AribService>,
        actualTransportKeys: Set<TransportKey>,
    ): List<AribService> {
        val actualTransports = actualTransportKeys
        if (actualTransports.size != 1) {
            skippedUnresolvedTransportCount += services.size
            Log.w(
                LogTags.TIS,
                "current candidate の SDT actual TransportKey が一意に確定していないため channel 登録を省略します actualTransports=$actualTransports",
            )
            return emptyList()
        }
        val actualTransport = actualTransports.single()
        val filtered = services.filter { TransportKey(it.serviceKey.originalNetwork, it.serviceKey.transportStream) == actualTransport }
        skippedUnresolvedTransportCount += services.size - filtered.size
        return filtered
    }

    // 0x4eは受信table IDそのもの。長い型付き引数と診断はこの公開処理だけ行長を許容する。
    @Suppress("MagicNumber", "MaxLineLength")
    private fun publishProgramsForRegisteredServices(
        mode: PublishMode,
        allowedServiceKeys: Set<ServiceKey>?,
    ): ProgramPublishCoordinator.ProgramPublishResult {
        val transaction = engine.takeProgramPublishSnapshot()
        val allPrograms =
            EventModelMapper().toProgramRecords(
                profile = transaction.discoveryProfile,
                events = transaction.events,
                semanticFactsByServiceKey = transaction.semanticFactsByServiceKey,
                malformedCaDescriptorCountByServiceId = transaction.malformedCaDescriptorCountByServiceId,
                ratingProfileByServiceKey =
                    transaction.events.associate { event ->
                        event.serviceKey to AribRatingMapper.profileForDeliverySystem(currentCandidate?.deliverySystem)
                    },
            )
        val updateWindows = transaction.updateWindows.map(ProgramPublishCoordinator::EpgUpdateWindow)
        val verifiedEmptyServiceKeys =
            transaction.eitInstances
                .filter { instance ->
                    instance.serviceKey in transaction.authoritativeProgramKeysByService &&
                        transaction.events.none { it.source.tableId == 0x4e && it.serviceKey == instance.serviceKey } &&
                        ServicePolicyEvaluator
                            .evaluate(
                                facts = transaction.semanticFactsByServiceKey[instance.serviceKey],
                                expectedSmdBroadcastingIdentifier = currentCandidate?.let(::expectedSmdBroadcastingIdentifier),
                            ).registrationReady
                }.mapTo(linkedSetOf()) { it.serviceKey }
        val result =
            programPublishCoordinator.publishWithUpdates(
                mode,
                allPrograms,
                updateWindows,
                allowedServiceKeys,
                verifiedEmptyServiceKeys,
            )
        if (result.skippedNoChannel > 0) Log.d(LogTags.TIS, "$mode で未登録channelのeventをskipしました skipped=${result.skippedNoChannel}")
        if (result.failures.isNotEmpty()) Log.w(LogTags.TIS, "TvProvider program 登録失敗=${result.failures}")
        return result
    }

    private fun expectedSmdBroadcastingIdentifier(candidate: ScanCandidate): Int =
        requireNotNull(ServicePolicyEvaluator.expectedSmdBroadcastingIdentifier(discoveryProfile(candidate.kind)))

    private fun discoveryProfile(kind: ScanCandidateKind): Int =
        when (kind) {
            ScanCandidateKind.ISDB_T_UHF, ScanCandidateKind.ISDB_T_CATV -> SiDiscoveryProfile.ISDB_T
            ScanCandidateKind.ISDB_S_BS -> SiDiscoveryProfile.BS
            ScanCandidateKind.ISDB_S_110CS -> SiDiscoveryProfile.CS110
        }

    private fun serviceCounts(
        candidate: ScanCandidate,
        requirements: SiCollectionRequirements,
    ): ServiceCounts {
        val transaction = engine.serviceRegistrationSnapshot()
        val expectedSmdIdentifier = expectedSmdBroadcastingIdentifier(candidate)
        val completeness =
            transaction.services.map { service ->
                ServiceListBuilder.completenessForModel(
                    service = service,
                    facts = transaction.semanticFactsByServiceKey[service.serviceKey],
                    expectedSmdBroadcastingIdentifier = expectedSmdIdentifier,
                )
            }
        val summary = ServiceListBuilder.ServiceSnapshotSummary(completeness)
        return ServiceCounts(
            discoveryStage = transaction.discoveryStage,
            collectionStatus = requirements.evaluate(transaction),
            total = summary.total,
            clearLivePlaybackStaticallyEligible = summary.clearLivePlaybackStaticallyEligible,
            registrationReady = summary.registrationReady,
            signature = summary.stableSignature(),
            incompleteReasons =
                completeness
                    .filter { !it.registrationReady }
                    .associate { it.serviceKey to it.reasons },
        )
    }

    // 安定待ち・期限・取消し・資源喪失の優先順位と、終了後のfilter解放を同じ収集処理で保持する。
    // 各breakは異なる終了理由を確定する。部分完了の4条件はEIT不要・最短待機・登録可能・安定待機の全てを要求する。
    @Suppress("LongMethod", "CyclomaticComplexMethod", "LoopWithTooManyJumpStatements", "MaxLineLength")
    private fun collectSiForCandidate(
        candidate: ScanCandidate,
        requirements: SiCollectionRequirements,
        tuneGeneration: Long,
    ): SiCollectionResult {
        val policy = DEFAULT_SI_POLICY
        val startedAt = android.os.SystemClock.elapsedRealtime()
        var lastCounts: ServiceCounts? = null
        var stableSince = startedAt
        var outcome = SiCollectionOutcome.TIMEOUT_PARTIAL

        val collectionFailure =
            runCatching {
                SectionFilterPolicy.completeCleanup({
                    while (!cancelled.get() && !resourceLostFor(tuneGeneration)) {
                        refreshDynamicSectionFilters()
                        if (resourceLostFor(tuneGeneration)) break
                        val now = android.os.SystemClock.elapsedRealtime()
                        val counts = serviceCounts(candidate, requirements)
                        if (counts.discoveryStage != lastCounts?.discoveryStage || counts.signature != lastCounts?.signature ||
                            counts.collectionStatus != lastCounts?.collectionStatus
                        ) {
                            lastCounts = counts
                            stableSince = now
                        }
                        val elapsed = now - startedAt
                        val stableFor = now - stableSince
                        if (counts.collectionStatus.complete && elapsed >= policy.minWaitMs && stableFor >= policy.stableWaitMs) {
                            outcome = SiCollectionOutcome.COMPLETE
                            break
                        }
                        val registrationReadySnapshotAvailable = counts.registrationReady > 0
                        val stablePartialCollectionReady =
                            !requirements.requiresEit && elapsed >= policy.minWaitMs && registrationReadySnapshotAvailable &&
                                stableFor >= policy.stableWaitMs
                        if (stablePartialCollectionReady) {
                            outcome = SiCollectionOutcome.STABLE_PARTIAL
                            break
                        }
                        if (elapsed >= policy.maxWaitMs) {
                            outcome =
                                if (registrationReadySnapshotAvailable) {
                                    SiCollectionOutcome.TIMEOUT_PARTIAL
                                } else {
                                    SiCollectionOutcome.INCOMPLETE_NO_REGISTRATION_READY_SERVICE
                                }
                            break
                        }
                        runCatching { Thread.sleep(policy.pollIntervalMs) }
                    }
                }, { tunerController.closeSectionFilters() })
            }.exceptionOrNull()
        if (resourceLossFence.finishCollection(tuneGeneration, collectionFailure) { failure ->
                Log.w(LogTags.TIS, "resource-lost後のSI collection cleanupに失敗しました generation=$tuneGeneration", failure)
            } == SiCollectionOutcome.RESOURCE_LOST
        ) {
            val counts = lastCounts
            val message = "Tuner resource lostによりscan generationを失効しました generation=$tuneGeneration; 以後のSI snapshot/publishを拒否します"
            Log.w(LogTags.TIS, "scan候補をresource lostで終了します candidate=$candidate $message")
            return SiCollectionResult(
                outcome = SiCollectionOutcome.RESOURCE_LOST,
                diagnostic = ScanDiagnostic(candidate, message),
                clearLivePlaybackStaticallyEligibleServices = counts?.clearLivePlaybackStaticallyEligible ?: 0,
                registrationReadyServices = counts?.registrationReady ?: 0,
            )
        }
        if (cancelled.get()) {
            terminalCancelObserved = true
            outcome = SiCollectionOutcome.CANCELLED
        }
        val finalCounts = serviceCounts(candidate, requirements)
        val complete = finalCounts.collectionStatus.complete
        if (outcome == SiCollectionOutcome.COMPLETE && !complete) outcome = SiCollectionOutcome.TIMEOUT_PARTIAL
        val finalRegistrationReadySnapshotAvailable = finalCounts.registrationReady > 0
        if (outcome == SiCollectionOutcome.TIMEOUT_PARTIAL &&
            !finalRegistrationReadySnapshotAvailable
        ) {
            outcome = SiCollectionOutcome.INCOMPLETE_NO_REGISTRATION_READY_SERVICE
        }
        val elapsed = android.os.SystemClock.elapsedRealtime() - startedAt
        val message =
            if (outcome == SiCollectionOutcome.COMPLETE) {
                null
            } else {
                "SI 収集が完全完了していません outcome=$outcome stage=${finalCounts.discoveryStage} " +
                    "services=${finalCounts.total} " +
                    "clearLivePlaybackStaticallyEligibleServices=${finalCounts.clearLivePlaybackStaticallyEligible} " +
                    "registrationReadyServices=${finalCounts.registrationReady} incomplete=${finalCounts.incompleteReasons} " +
                    "missingInstances=${finalCounts.collectionStatus.missing} " +
                    "sections=${ingestController.diagnosticSummary()} elapsedMs=$elapsed"
            }
        Log.i(LogTags.TIS, "scan 候補の SI 収集結果 candidate=$candidate outcome=$outcome complete=$complete counts=$finalCounts message=$message")
        return SiCollectionResult(
            outcome = outcome,
            diagnostic = message?.let { ScanDiagnostic(candidate, it) },
            clearLivePlaybackStaticallyEligibleServices = finalCounts.clearLivePlaybackStaticallyEligible,
            registrationReadyServices = finalCounts.registrationReady,
        )
    }

    private fun scanCandidateFromChannel(channel: ChannelRecord): ScanCandidate? =
        runCatching {
            ScanCandidate(
                deliverySystem = channel.deliverySystem,
                frequencyHz = channel.frequencyHz,
                streamSelector = channel.streamSelector,
                displayChannel = channel.displayNumber.ifBlank { channel.displayName },
                physicalChannel = channel.physicalChannel,
                backendHint = channel.backendHint,
                satelliteBand = channel.satelliteBand,
            )
        }.onFailure { error ->
            Log.w(LogTags.TIS, "既存 channel から scan candidate を復元できません channel=$channel", error)
        }.getOrNull()

    override fun close() {
        cancelScan()
        // CASのcloseもTunerControllerが所有する。同じCASを二つのownerから閉じない。
        tunerController.release()
    }

    fun terminalCancelObservedForLastTask(): Boolean = terminalCancelObserved

    fun terminalResourceLostObservedForLastTask(): Boolean = terminalResourceLostObserved

    fun skippedUnresolvedTransportCountForDiagnostic(): Int = skippedUnresolvedTransportCount

    private fun resetResourceLostState() = resourceLossFence.reset()

    private fun activateScanGeneration(generation: Long) = resourceLossFence.activate(generation)

    private fun clearActiveScanGeneration(generation: Long) = resourceLossFence.clearActive(generation)

    private fun resourceLostFor(generation: Long): Boolean = resourceLossFence.isLost(generation)

    private fun publishScanSnapshotIfCurrent(
        generation: Long,
        mode: PublishMode,
        allowedServiceKeys: Set<ServiceKey>? = null,
    ): PublishSnapshotResult? =
        resourceLossFence.publishIfCurrent(generation) {
            publishCurrentServiceSnapshot(mode, allowedServiceKeys)
        }

    /** scanが既に所有していたgenerationと公開lockをまとめる。別の世代は作らない。 */
    internal class ResourceLossFence {
        @Volatile var terminalObserved = false
            private set
        private val activeGeneration = AtomicLong(-1L)
        private val lostGeneration = AtomicLong(-1L)
        private val publicationLock = Any()

        fun reset() {
            terminalObserved = false
            activeGeneration.set(-1L)
            lostGeneration.set(-1L)
        }

        fun activate(generation: Long) {
            activeGeneration.set(generation)
            if (isLost(generation)) terminalObserved = true
        }

        fun clearActive(generation: Long) {
            activeGeneration.compareAndSet(generation, -1L)
        }

        fun isLost(generation: Long): Boolean = lostGeneration.get() == generation

        fun onLost(generation: Long) =
            synchronized(publicationLock) {
                val active = activeGeneration.get()
                if (active != -1L && active != generation) return@synchronized
                lostGeneration.set(generation)
                if (activeGeneration.get() == generation) terminalObserved = true
            }

        fun finishCollection(
            generation: Long,
            failure: Throwable?,
            reportFailure: (Throwable) -> Unit,
        ): SiCollectionOutcome? {
            if (isLost(generation)) {
                terminalObserved = true
                failure?.let(reportFailure)
                return SiCollectionOutcome.RESOURCE_LOST
            }
            failure?.let { throw it }
            return null
        }

        fun <T> publishIfCurrent(
            generation: Long,
            publish: () -> T,
        ): T? =
            synchronized(publicationLock) {
                if (isLost(generation)) {
                    terminalObserved = true
                    return@synchronized null
                }
                val result = publish()
                if (isLost(generation)) terminalObserved = true
                result.takeUnless { terminalObserved }
            }
    }

    // 世代と公開拒否理由を含む単一の診断式を維持する。
    @Suppress("MaxLineLength")
    private fun resourceLostDiagnostic(
        candidate: ScanCandidate,
        generation: Long,
    ): ScanDiagnostic =
        ScanDiagnostic(candidate, "Tuner resource lostによりscan generationを失効しました generation=$generation; TvProvider publishを拒否します")

    companion object {
        private val DEFAULT_SI_POLICY = SiCollectionPolicy()
        private const val ENABLE_CAS_ORCHESTRATION = true

        // 公開方針の既存入口への委譲を一つの式に保つ。
        @Suppress("MaxLineLength")
        fun filterProgramServiceKeysForPublishModeForTest(
            mode: PublishMode,
            allServiceKeys: Iterable<ServiceKey>,
            existingServiceKeys: Set<ServiceKey>,
            allowedServiceKeys: Set<ServiceKey>?,
        ): Set<ServiceKey> =
            ProgramPublishCoordinator.filterServiceKeysForMode(mode, allServiceKeys, existingServiceKeys, allowedServiceKeys)

        fun validProgramKeysForUpdateForTest(update: com.maleicacid.tvinput.aribsi.AribEpgUpdateWindow): Set<String> =
            validProgramKeysForUpdate(update)

        private fun validProgramKeysForUpdate(update: com.maleicacid.tvinput.aribsi.AribEpgUpdateWindow): Set<String> =
            update.validProgramStableIdentities.toSet()

        // 収集結果の独立した判定入力を明示し、試験用の呼出し形状と条件式を維持する。
        @Suppress("LongParameterList", "MaxLineLength")
        fun siCollectionOutcomeForTest(
            discoveryComplete: Boolean,
            cancelled: Boolean,
            elapsedMs: Long,
            stableForMs: Long,
            registrationReadyServices: Int,
            policy: SiCollectionPolicy,
            resourceLost: Boolean = false,
        ): SiCollectionOutcome =
            when {
                resourceLost -> {
                    SiCollectionOutcome.RESOURCE_LOST
                }

                cancelled -> {
                    SiCollectionOutcome.CANCELLED
                }

                discoveryComplete && elapsedMs >= policy.minWaitMs -> {
                    SiCollectionOutcome.COMPLETE
                }

                elapsedMs >= policy.minWaitMs && registrationReadyServices > 0 && stableForMs >= policy.stableWaitMs -> {
                    SiCollectionOutcome.STABLE_PARTIAL
                }

                elapsedMs >= policy.maxWaitMs && registrationReadyServices > 0 -> {
                    SiCollectionOutcome.TIMEOUT_PARTIAL
                }

                elapsedMs >= policy.maxWaitMs -> {
                    SiCollectionOutcome.INCOMPLETE_NO_REGISTRATION_READY_SERVICE
                }

                else -> {
                    SiCollectionOutcome.TIMEOUT_PARTIAL
                }
            }
    }
}

internal fun tunerPriorityHintUseCase(purpose: ScanPurpose): Int =
    when (purpose) {
        ScanPurpose.SETUP_SCAN -> TvInputService.PRIORITY_HINT_USE_CASE_TYPE_SCAN

        ScanPurpose.BOOT_EPG_SYNC,
        ScanPurpose.BACKGROUND_MAINTENANCE,
        -> TvInputService.PRIORITY_HINT_USE_CASE_TYPE_BACKGROUND
    }

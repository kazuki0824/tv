package com.maleicacid.tvinput.tis

import android.content.Context
import android.media.tv.TvInputService
import android.util.Log
import com.maleicacid.tvinput.aribsi.AribService
import com.maleicacid.tvinput.aribsi.AribSiEngine
import com.maleicacid.tvinput.aribsi.AribRatingMapper
import com.maleicacid.tvinput.aribsi.EventModelMapper
import com.maleicacid.tvinput.aribsi.PmtCatCaMetadataMapper
import com.maleicacid.tvinput.aribsi.ServiceListBuilder
import com.maleicacid.tvinput.aribsi.ServicePolicyEvaluator
import com.maleicacid.tvinput.aribsi.TransportKey
import com.maleicacid.tvinput.aribsi.SectionIngestController
import com.maleicacid.tvinput.aribsi.SiDiscoveryStage
import com.maleicacid.tvinput.aribsi.SiDiscoveryProfile
import com.maleicacid.tvinput.common.LogTags
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.TsPid
import com.maleicacid.tvinput.db.ChannelRecord
import java.util.concurrent.atomic.AtomicBoolean

class ChannelScanController(
    private val context: Context,
    private val inputId: String,
    private val engine: AribSiEngine,
    scanPurpose: ScanPurpose,
    private val cancelRequested: AtomicBoolean = AtomicBoolean(false),
) : AutoCloseable {
    data class ScanDiagnostic(val candidate: ScanCandidate, val message: String)
    data class ScanResult(
        val scanned: Int,
        val published: Int,
        val diagnostics: List<ScanDiagnostic>,
        val successfulCandidates: Int = 0,
        val terminalCancelObserved: Boolean = false,
        val committedServiceKeys: Set<ServiceKey> = emptySet(),
    )
    enum class SiCollectionOutcome { COMPLETE, STABLE_PARTIAL, TIMEOUT_PARTIAL, INCOMPLETE_NO_REGISTRATION_READY_SERVICE, CANCELLED }
    data class SiCollectionResult(
        val outcome: SiCollectionOutcome,
        val diagnostic: ScanDiagnostic?,
        val clearLivePlaybackSupportedServices: Int,
        val registrationReadyServices: Int = clearLivePlaybackSupportedServices,
    ) {
        val mayPublishChannels: Boolean
            get() = outcome != SiCollectionOutcome.CANCELLED &&
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
        val total: Int,
        val clearLivePlaybackSupported: Int,
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

    private val tunerController = TunerController(
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
    private var skippedUnresolvedTransportCount: Int = 0
    private var currentCandidate: ScanCandidate? = null

    init {
        tunerController.setSectionIngestController(ingestController)
        tunerController.setCasController(casController)
        tunerController.setOnSectionIngestedCallback { refreshDynamicSectionFilters() }
    }

    fun startInitialScan(candidates: List<ScanCandidate> = JapanIsdbScanPlan.defaultInitialScan()): ScanResult {
        if (!cancelled.get()) cancelled.set(false)
        terminalCancelObserved = cancelled.get()
        skippedUnresolvedTransportCount = 0
        val diagnostics = mutableListOf<ScanDiagnostic>()
        var published = 0
        var successfulCandidates = 0
        val executionCandidates = candidates.flatMap { candidate ->
            if (candidate.kind == ScanCandidateKind.ISDB_S_BS && candidate.streamSelector == com.maleicacid.tvinput.common.StreamSelector.NONE) {
                val discovery = tunerController.discoverIsdbsStreamIds(candidate)
                val discovered = JapanIsdbScanPlan.explicitBsCandidatesFromScan(candidate, discovery.streamIds)
                if (discovered.isNotEmpty()) {
                    discovered
                } else {
                    val fallback = JapanIsdbScanPlan.fallbackBsCandidates(candidate)
                    diagnostics += ScanDiagnostic(candidate, "AOSP BS scanでstream IDを取得できないためversioned TSID fallbackを使用します result=${discovery.resultCode} message=${discovery.message} fallbackCount=${fallback.size}")
                    fallback
                }
            } else {
                listOf(candidate)
            }
        }
        executionCandidates.forEach { candidate ->
            if (cancelled.get()) return@forEach
            engine.reset(discoveryProfile(candidate.kind))
            currentCandidate = candidate
            val tune = tunerController.tuneForScan(candidate)
            if (!tune.success) {
                diagnostics += ScanDiagnostic(candidate, "選局に失敗しました result=${tune.resultCode} ${tune.message}")
                return@forEach
            }
            val collection = collectSiForCandidate(candidate)
            collection.diagnostic?.let { diagnostics += it }
            if (!collection.mayPublishChannels) {
                Log.w(LogTags.TIS, "SI discovery 未完了のため TvProvider channel 登録を省略します candidate=$candidate outcome=${collection.outcome} registrationReady=${collection.registrationReadyServices} clearLivePlaybackSupported=${collection.clearLivePlaybackSupportedServices} diagnostic=${collection.diagnostic?.message}")
                return@forEach
            }
            val publishResult = publishCurrentServiceSnapshot(PublishMode.SETUP_SCAN)
            if (collection.outcome == SiCollectionOutcome.COMPLETE && collection.registrationReadyServices > 0 && publishResult.success) successfulCandidates++
            published += publishResult.changed
        }
        currentCandidate = null
        return ScanResult(executionCandidates.size, published, diagnostics, successfulCandidates = successfulCandidates, terminalCancelObserved = terminalCancelObserved)
    }

    fun startBootEpgSync(targetChannels: List<ChannelRecord>): ScanResult = runMaintenanceScan(
        candidates = maintenanceCandidates(targetChannels),
        mode = PublishMode.BOOT_EPG_SYNC,
        failurePrefix = "boot後EPG同期",
        allowedServiceKeys = targetChannels.map { it.serviceKey }.toSet(),
    )

    fun startBackgroundChannelMaintenance(): ScanResult {
        val channels = existingMaintenanceChannels()
        return runMaintenanceScan(
            candidates = maintenanceCandidates(channels),
            mode = PublishMode.BACKGROUND_CHANNEL_MAINTENANCE,
            failurePrefix = "background channel maintenance",
            allowedServiceKeys = channels.map { it.serviceKey }.toSet(),
        )
    }

    private fun runMaintenanceScan(
        candidates: List<ScanCandidate>,
        mode: PublishMode,
        failurePrefix: String,
        allowedServiceKeys: Set<ServiceKey>,
    ): ScanResult {
        if (!cancelled.get()) cancelled.set(false)
        terminalCancelObserved = cancelled.get()
        skippedUnresolvedTransportCount = 0
        val diagnostics = mutableListOf<ScanDiagnostic>()
        val committedServiceKeys = linkedSetOf<ServiceKey>()
        var updated = 0
        var successfulCandidates = 0
        candidates.forEach { candidate ->
            if (cancelled.get()) return@forEach
            engine.reset(discoveryProfile(candidate.kind))
            currentCandidate = candidate
            val tune = tunerController.tuneForScan(candidate)
            if (!tune.success) {
                diagnostics += ScanDiagnostic(candidate, "${failurePrefix}の選局に失敗しました result=${tune.resultCode} ${tune.message}")
                return@forEach
            }
            val collection = collectSiForCandidate(candidate)
            collection.diagnostic?.let { diagnostics += it }
            if (!collection.mayPublishChannels) {
                Log.w(LogTags.TIS, "${failurePrefix} SI discovery 未完了のため Programs publish/delete を省略します candidate=$candidate outcome=${collection.outcome} registrationReady=${collection.registrationReadyServices}")
                return@forEach
            }
            val publishResult = publishCurrentServiceSnapshot(mode, allowedServiceKeys)
            committedServiceKeys += publishResult.committedServiceKeys
            if (collection.outcome == SiCollectionOutcome.COMPLETE && collection.registrationReadyServices > 0 && publishResult.success && publishResult.hasCommittedProgramTarget) successfulCandidates++
            updated += publishResult.changed
        }
        currentCandidate = null
        return ScanResult(
            candidates.size,
            updated,
            diagnostics,
            successfulCandidates = successfulCandidates,
            terminalCancelObserved = terminalCancelObserved,
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
    fun onSection(pid: Int, section: ByteArray) {
        val tsPid = TsPid.fromOrNull(pid) ?: return
        tunerController.onSection(tsPid, section)
        refreshDynamicSectionFilters()
        publishCurrentServiceSnapshot(PublishMode.LIVE_TUNE_REFRESH)
    }
    fun refreshDynamicSectionFilters() {
        val transaction = engine.casDiscoverySnapshot()
        val servicesForCas = transaction.services
        val allCaMetadata = if (ENABLE_CAS_ORCHESTRATION) transaction.caMetadata else emptyList()
        val serviceScopedCa = allCaMetadata.filter { it.source != com.maleicacid.tvinput.aribsi.CaMetadataSource.CAT && it.serviceKey != null }
        val catCa = allCaMetadata.filter { it.source == com.maleicacid.tvinput.aribsi.CaMetadataSource.CAT }
        val caMetadata = caMapper.expandProgramLevelToElementaryStreams(serviceScopedCa + catCa, servicesForCas)
        val pmtPids = transaction.pmtPids.values.toSet()
        val ecmPids = caMetadata.mapNotNull { it.ecmPid }.toSet()
        val emmPids = caMetadata.mapNotNull { it.emmPid }.toSet()
        tunerController.openDynamicFiltersFromCurrentSi(pmtPids, ecmPids, emmPids)
        if (caMetadata.isEmpty()) {
            casController.clearForClearService()
            return
        }
        val unsupported = caMapper.unsupportedForB25B1(caMetadata, CasController.SupportedCasSystemIds.B25_B1)
        unsupported.forEach { Log.w(LogTags.TIS, "対象外 CA情報 を無視します caSystemId=${it.caSystemId}") }
        val bridge = if (serviceScopedCa.isEmpty()) null else tunerController.createDescramblerBridge()
        casController.updateFromCaMetadata(caMetadata, bridge)
    }

    private fun publishCurrentServiceSnapshot(
        mode: PublishMode,
        allowedServiceKeys: Set<ServiceKey>? = null,
    ): PublishSnapshotResult {
        if (mode == PublishMode.DIAGNOSTIC_ONLY) return PublishSnapshotResult(0)
        if (mode == PublishMode.LIVE_TUNE_REFRESH || mode == PublishMode.BOOT_EPG_SYNC || mode == PublishMode.BACKGROUND_CHANNEL_MAINTENANCE) {
            val result = publishProgramsForRegisteredServices(mode, allowedServiceKeys)
            Log.d(LogTags.TIS, "${mode} では新規 channel row を追加しません changed=${result.changed} skippedNoChannel=${result.skippedNoChannel} skippedUnchanged=${result.skippedUnchanged}")
            return PublishSnapshotResult(
                changed = result.changed,
                failures = result.failures,
                hasCommittedProgramTarget = result.hasCommittedTarget,
                committedServiceKeys = result.committedServiceKeys,
            )
        }
        val candidate = currentCandidate ?: return PublishSnapshotResult(0)
        val transaction = engine.serviceRegistrationSnapshot()
        val transportRemoteKeys = transaction.actualTransportMetadata.associate { transport ->
            TransportKey(transport.originalNetwork, transport.transportStream) to transport.remoteControlKeyId
        }
        val diagnostics = transaction.semanticFactsByServiceKey.mapValues { (key, facts) ->
            ServicePolicyEvaluator.evaluate(
                facts = facts,
                fallbackKey = key,
                hasPhysicalTune = candidate.frequencyHz.value > 0L,
                hasInternalTuneKey = candidate.streamSelector.value != null || candidate.streamSelector == com.maleicacid.tvinput.common.StreamSelector.NONE,
                expectedSmdBroadcastingIdentifier = expectedSmdBroadcastingIdentifier(candidate),
            )
        }
        val registrationReadyServices = transaction.services.filter { service ->
            diagnostics[service.serviceKey]?.registrationReady == true
        }
        val services = filterServicesForCurrentCandidate(registrationReadyServices, transaction.actualTransports)
        val channels = services.mapNotNull { service ->
            val serviceType = service.serviceType ?: return@mapNotNull null
            val remoteKey = transportRemoteKeys[TransportKey(service.serviceKey.originalNetwork, service.serviceKey.transportStream)]
            val diagnostic = diagnostics[service.serviceKey]
            ChannelRecord(
                serviceKey = service.serviceKey,
                displayNumber = ChannelNumberingPolicy.displayNumber(service, remoteKey, candidate),
                displayName = service.name.ifEmpty { "service-${service.serviceKey.originalNetworkId}-${service.serviceKey.transportStreamId}-${service.serviceKey.serviceId}" },
                frequencyHz = candidate.frequencyHz,
                deliverySystem = candidate.deliverySystem,
                streamSelector = candidate.streamSelector,
                physicalChannel = candidate.physicalChannel,
                backendHint = candidate.backendHint,
                satelliteBand = candidate.satelliteBand,
                remoteControlKeyId = remoteKey,
                serviceType = serviceType,
                requiresCas = diagnostic?.requiresCas == true,
            )
        }
        if (channels.isEmpty()) {
            val incomplete = diagnostics.filterValues { !it.registrationReady }
                .mapValues { (_, decision) -> decision.reasons }
            Log.d(LogTags.TIS, "registration-ready なサービスがないため channel snapshot 登録を省略します candidate=$candidate stage=${transaction.discoveryStage} incomplete=$incomplete")
            return PublishSnapshotResult(0)
        }
        val channelResult = tvProviderWriter.upsertChannels(channels)
        if (channelResult.failures.isNotEmpty()) Log.w(LogTags.TIS, "TvProvider channel 登録失敗=${channelResult.failures}")
        val programResult = publishProgramsForRegisteredServices(PublishMode.SETUP_SCAN, allowedServiceKeys = channels.map { it.serviceKey }.toSet())
        return PublishSnapshotResult(
            changed = channelResult.inserted + channelResult.updated,
            failures = channelResult.failures + programResult.failures,
        )
    }

    private fun filterServicesForCurrentCandidate(
        services: List<AribService>,
        actualTransportKeys: Set<TransportKey>,
    ): List<AribService> {
        val actualTransports = actualTransportKeys
        if (actualTransports.size != 1) {
            skippedUnresolvedTransportCount += services.size
            Log.w(LogTags.TIS, "current candidate の SDT actual TransportKey が一意に確定していないため channel 登録を省略します actualTransports=$actualTransports")
            return emptyList()
        }
        val actualTransport = actualTransports.single()
        val filtered = services.filter { TransportKey(it.serviceKey.originalNetwork, it.serviceKey.transportStream) == actualTransport }
        skippedUnresolvedTransportCount += services.size - filtered.size
        return filtered
    }

    private fun publishProgramsForRegisteredServices(mode: PublishMode, allowedServiceKeys: Set<ServiceKey>?): ProgramPublishCoordinator.ProgramPublishResult {
        val transaction = engine.takeProgramPublishSnapshot()
        val allPrograms = EventModelMapper().toProgramRecords(
            events = transaction.events,
            semanticFactsByServiceKey = transaction.semanticFactsByServiceKey,
            malformedCaDescriptorCountByServiceId = transaction.malformedCaDescriptorCountByServiceId,
            ratingProfileByServiceKey = transaction.events.associate { event ->
                event.serviceKey to AribRatingMapper.profileForDeliverySystem(currentCandidate?.deliverySystem)
            },
        )
        val updateWindows = transaction.updateWindows.map { update ->
            ProgramPublishCoordinator.EpgUpdateWindow(
                serviceKey = update.serviceKey,
                windowStartMs = update.windowStartMillis,
                windowEndMs = update.windowEndMillis,
                validProgramKeys = validProgramKeysForUpdate(update),
                deletionAuthoritative = update.deletionAuthoritative,
            )
        }
        val result = programPublishCoordinator.publishWithUpdates(mode, allPrograms, updateWindows, allowedServiceKeys)
        if (result.skippedNoChannel > 0) Log.d(LogTags.TIS, "${mode} で未登録channelのeventをskipしました skipped=${result.skippedNoChannel}")
        if (result.failures.isNotEmpty()) Log.w(LogTags.TIS, "TvProvider program 登録失敗=${result.failures}")
        return result
    }

    private fun expectedSmdBroadcastingIdentifier(candidate: ScanCandidate): Int = when (candidate.kind) {
        ScanCandidateKind.ISDB_T_UHF, ScanCandidateKind.ISDB_T_CATV -> 0b000011
        ScanCandidateKind.ISDB_S_BS -> 0b000010
        ScanCandidateKind.ISDB_S_110CS -> 0b000100
    }

    private fun discoveryProfile(kind: ScanCandidateKind): Int = when (kind) {
        ScanCandidateKind.ISDB_T_UHF, ScanCandidateKind.ISDB_T_CATV -> SiDiscoveryProfile.ISDB_T
        ScanCandidateKind.ISDB_S_BS -> SiDiscoveryProfile.BS
        ScanCandidateKind.ISDB_S_110CS -> SiDiscoveryProfile.CS110
    }

    private fun serviceCounts(candidate: ScanCandidate): ServiceCounts {
        val transaction = engine.serviceRegistrationSnapshot()
        val expectedSmdIdentifier = expectedSmdBroadcastingIdentifier(candidate)
        val completeness = transaction.services.map { service ->
            ServiceListBuilder.completenessForModel(
                service = service,
                facts = transaction.semanticFactsByServiceKey[service.serviceKey],
                expectedSmdBroadcastingIdentifier = expectedSmdIdentifier,
            )
        }
        val summary = ServiceListBuilder.ServiceSnapshotSummary(completeness)
        return ServiceCounts(
            discoveryStage = transaction.discoveryStage,
            total = summary.total,
            clearLivePlaybackSupported = summary.clearLivePlaybackSupported,
            registrationReady = summary.registrationReady,
            signature = summary.stableSignature(),
            incompleteReasons = completeness
                .filter { !it.registrationReady }
                .associate { it.serviceKey to it.reasons },
        )
    }

    private fun collectSiForCandidate(candidate: ScanCandidate): SiCollectionResult {
        val policy = DEFAULT_SI_POLICY
        val startedAt = System.currentTimeMillis()
        var lastCounts = serviceCounts(candidate)
        var lastStage = lastCounts.discoveryStage
        var stableSince = startedAt
        var outcome = SiCollectionOutcome.TIMEOUT_PARTIAL

        while (!cancelled.get()) {
            refreshDynamicSectionFilters()
            val now = System.currentTimeMillis()
            val counts = serviceCounts(candidate)
            val stage = counts.discoveryStage
            if (stage != lastStage || counts.signature != lastCounts.signature) {
                lastStage = stage
                lastCounts = counts
                stableSince = now
            }
            val elapsed = now - startedAt
            val stableFor = now - stableSince
            if (stage == SiDiscoveryStage.COMPLETE && elapsed >= policy.minWaitMs) {
                outcome = SiCollectionOutcome.COMPLETE
                break
            }
            val registrationReadySnapshotAvailable = counts.registrationReady > 0
            if (elapsed >= policy.minWaitMs && registrationReadySnapshotAvailable && stableFor >= policy.stableWaitMs) {
                outcome = SiCollectionOutcome.STABLE_PARTIAL
                break
            }
            if (elapsed >= policy.maxWaitMs) {
                outcome = if (registrationReadySnapshotAvailable) SiCollectionOutcome.TIMEOUT_PARTIAL else SiCollectionOutcome.INCOMPLETE_NO_REGISTRATION_READY_SERVICE
                break
            }
            runCatching { Thread.sleep(policy.pollIntervalMs) }
        }
        if (cancelled.get()) {
            terminalCancelObserved = true
            outcome = SiCollectionOutcome.CANCELLED
        }
        val finalCounts = serviceCounts(candidate)
        val complete = finalCounts.discoveryStage == SiDiscoveryStage.COMPLETE
        if (!cancelled.get() && complete) outcome = SiCollectionOutcome.COMPLETE
        val finalRegistrationReadySnapshotAvailable = finalCounts.registrationReady > 0
        if (outcome == SiCollectionOutcome.TIMEOUT_PARTIAL && !finalRegistrationReadySnapshotAvailable) outcome = SiCollectionOutcome.INCOMPLETE_NO_REGISTRATION_READY_SERVICE
        val elapsed = System.currentTimeMillis() - startedAt
        val message = if (outcome == SiCollectionOutcome.COMPLETE) {
            null
        } else {
            "SI 収集が完全完了していません outcome=$outcome stage=${finalCounts.discoveryStage} services=${finalCounts.total} clearLivePlaybackSupportedServices=${finalCounts.clearLivePlaybackSupported} registrationReadyServices=${finalCounts.registrationReady} incomplete=${finalCounts.incompleteReasons} sections=${ingestController.diagnosticSummary()} elapsedMs=$elapsed"
        }
        Log.i(LogTags.TIS, "scan 候補の SI 収集結果 candidate=$candidate outcome=$outcome complete=$complete counts=$finalCounts message=$message")
        return SiCollectionResult(
            outcome = outcome,
            diagnostic = message?.let { ScanDiagnostic(candidate, it) },
            clearLivePlaybackSupportedServices = finalCounts.clearLivePlaybackSupported,
            registrationReadyServices = finalCounts.registrationReady,
        )
    }

    private fun existingMaintenanceChannels(): List<ChannelRecord> =
        tvProviderWriter.existingChannelsResult().getOrElse { error ->
            Log.w(LogTags.TIS, "既存 channel 復元失敗のため boot/background scan candidate を作成できません", error)
            emptyList()
        }

    private fun maintenanceCandidates(channels: List<ChannelRecord>): List<ScanCandidate> = channels
        .mapNotNull(::scanCandidateFromChannel)
        .distinctBy { candidate ->
            listOf(
                candidate.deliverySystem,
                candidate.frequencyHz.value,
                candidate.streamSelector.type,
                candidate.streamSelector.value,
                candidate.satelliteBand,
            )
        }

    private fun scanCandidateFromChannel(channel: ChannelRecord): ScanCandidate? = runCatching {
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
        casController.close()
        tunerController.release()
    }

    fun terminalCancelObservedForLastTask(): Boolean = terminalCancelObserved
    fun skippedUnresolvedTransportCountForDiagnostic(): Int = skippedUnresolvedTransportCount

    companion object {
        private val DEFAULT_SI_POLICY = SiCollectionPolicy()
        private const val ENABLE_CAS_ORCHESTRATION = true

        fun filterProgramServiceKeysForPublishModeForTest(
            mode: PublishMode,
            allServiceKeys: Iterable<ServiceKey>,
            existingServiceKeys: Set<ServiceKey>,
            allowedServiceKeys: Set<ServiceKey>?,
        ): Set<ServiceKey> = ProgramPublishCoordinator.filterServiceKeysForMode(mode, allServiceKeys, existingServiceKeys, allowedServiceKeys)

        fun validProgramKeysForUpdateForTest(update: com.maleicacid.tvinput.aribsi.AribEpgUpdateWindow): Set<String> =
            validProgramKeysForUpdate(update)

        private fun validProgramKeysForUpdate(update: com.maleicacid.tvinput.aribsi.AribEpgUpdateWindow): Set<String> =
            update.validProgramStableIdentities.toSet()

        fun siCollectionOutcomeForTest(
            discoveryComplete: Boolean,
            cancelled: Boolean,
            elapsedMs: Long,
            stableForMs: Long,
            registrationReadyServices: Int,
            policy: SiCollectionPolicy,
        ): SiCollectionOutcome = when {
            cancelled -> SiCollectionOutcome.CANCELLED
            discoveryComplete && elapsedMs >= policy.minWaitMs -> SiCollectionOutcome.COMPLETE
            elapsedMs >= policy.minWaitMs && registrationReadyServices > 0 && stableForMs >= policy.stableWaitMs -> SiCollectionOutcome.STABLE_PARTIAL
            elapsedMs >= policy.maxWaitMs && registrationReadyServices > 0 -> SiCollectionOutcome.TIMEOUT_PARTIAL
            elapsedMs >= policy.maxWaitMs -> SiCollectionOutcome.INCOMPLETE_NO_REGISTRATION_READY_SERVICE
            else -> SiCollectionOutcome.TIMEOUT_PARTIAL
        }
    }
}

internal fun tunerPriorityHintUseCase(purpose: ScanPurpose): Int = when (purpose) {
    ScanPurpose.SETUP_SCAN -> TvInputService.PRIORITY_HINT_USE_CASE_TYPE_SCAN
    ScanPurpose.BOOT_EPG_SYNC,
    ScanPurpose.BACKGROUND_MAINTENANCE -> TvInputService.PRIORITY_HINT_USE_CASE_TYPE_BACKGROUND
}

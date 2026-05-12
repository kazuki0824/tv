package com.maleicacid.tvinput.tis

import android.content.Context
import android.media.tv.TvInputService
import android.util.Log
import com.maleicacid.tvinput.aribsi.AribService
import com.maleicacid.tvinput.aribsi.AribSiEngine
import com.maleicacid.tvinput.aribsi.EventModelMapper
import com.maleicacid.tvinput.aribsi.PmtCatCaMetadataMapper
import com.maleicacid.tvinput.aribsi.ProviderDataBridge
import com.maleicacid.tvinput.aribsi.SectionIngestController
import com.maleicacid.tvinput.common.LogTags
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.db.ChannelRecord
import java.util.concurrent.atomic.AtomicBoolean

class ChannelScanController(
    private val context: Context,
    private val inputId: String,
    private val engine: AribSiEngine,
    private val cancelRequested: AtomicBoolean = AtomicBoolean(false),
) : AutoCloseable {
    data class ScanDiagnostic(val candidate: ScanCandidate, val message: String)
    data class ScanResult(val scanned: Int, val published: Int, val diagnostics: List<ScanDiagnostic>, val successfulCandidates: Int = 0, val terminalCancelObserved: Boolean = false)
    enum class SiCollectionOutcome { COMPLETE, STABLE_PARTIAL, TIMEOUT_PARTIAL, INCOMPLETE_NO_REGISTRATION_READY_SERVICE, CANCELLED }
    data class SiCollectionResult(
        val outcome: SiCollectionOutcome,
        val diagnostic: ScanDiagnostic?,
        val countsSignature: String,
        val clearLivePlaybackSupportedServices: Int,
        val registrationReadyServices: Int = clearLivePlaybackSupportedServices,
        val registrationReadySnapshotAvailable: Boolean = false,
    ) {
        val mayPublishChannels: Boolean
            get() = outcome != SiCollectionOutcome.CANCELLED &&
                outcome != SiCollectionOutcome.INCOMPLETE_NO_REGISTRATION_READY_SERVICE &&
                registrationReadySnapshotAvailable
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
        val total: Int,
        val complete: Int,
        val clearLivePlaybackSupported: Int,
        val registrationReady: Int,
        val signature: String,
        val incompleteReasons: Map<ServiceKey, List<String>>,
    )

    private data class PublishSnapshotResult(
        val changed: Int,
        val failures: List<TvProviderWriter.Diagnostic> = emptyList(),
    ) {
        val success: Boolean get() = failures.isEmpty()
    }

    private val tunerController = TunerController(context, inputId, TvInputService.PRIORITY_HINT_USE_CASE_TYPE_SCAN)
    private val ingestController = SectionIngestController(engine)
    private val tvProviderWriter = TvProviderWriter(context, inputId)
    private val programPublishCoordinator = ProgramPublishCoordinator(tvProviderWriter)
    private val caMapper = PmtCatCaMetadataMapper()
    private val casController = CasController()
    // B-14: ChannelScanManager と共有する cancel token。
    // controller / engine close は manager executor 上に閉じる一方、scan 実行中の
    // collectSiForCandidate() には executor queue を待たず即時に cancel を観測させる。
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
        candidates.forEach { candidate ->
            if (cancelled.get()) return@forEach
            engine.reset()
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
        return ScanResult(candidates.size, published, diagnostics, successfulCandidates = successfulCandidates, terminalCancelObserved = terminalCancelObserved)
    }

    fun startBootEpgSync(candidates: List<ScanCandidate> = bootEpgSyncCandidates()): ScanResult = runMaintenanceScan(
        candidates = candidates,
        mode = PublishMode.BOOT_EPG_SYNC,
        failurePrefix = "boot後EPG同期",
    )

    fun startBackgroundChannelMaintenance(candidates: List<ScanCandidate> = backgroundMaintenanceCandidates()): ScanResult = runMaintenanceScan(
        candidates = candidates,
        mode = PublishMode.BACKGROUND_CHANNEL_MAINTENANCE,
        failurePrefix = "background channel maintenance",
    )

    private fun runMaintenanceScan(candidates: List<ScanCandidate>, mode: PublishMode, failurePrefix: String): ScanResult {
        if (!cancelled.get()) cancelled.set(false)
        terminalCancelObserved = cancelled.get()
        skippedUnresolvedTransportCount = 0
        val diagnostics = mutableListOf<ScanDiagnostic>()
        var updated = 0
        var successfulCandidates = 0
        candidates.forEach { candidate ->
            if (cancelled.get()) return@forEach
            engine.reset()
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
            val publishResult = publishCurrentServiceSnapshot(mode)
            // Phase C/B-28: TvProvider query failure is a publish failure, not channel absence.
            // It must not contribute to boot pending clear / success diagnostics, even when SI
            // collection itself completed and registration-ready services exist. Program count may
            // still be zero; provider failure is the only blocker here.
            if (collection.outcome == SiCollectionOutcome.COMPLETE && collection.registrationReadyServices > 0 && publishResult.success) successfulCandidates++
            updated += publishResult.changed
        }
        currentCandidate = null
        return ScanResult(candidates.size, updated, diagnostics, successfulCandidates = successfulCandidates, terminalCancelObserved = terminalCancelObserved)
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
        tunerController.onSection(pid, section)
        refreshDynamicSectionFilters()
        publishCurrentServiceSnapshot(PublishMode.LIVE_TUNE_REFRESH)
    }

    fun refreshDynamicSectionFilters() {
        val transaction = engine.takeProgramPublishSnapshot(takeUpdateWindows = false)
        val servicesForCas = transaction.servicesForCasDiscovery
        val allCaMetadata = if (ENABLE_CAS_ORCHESTRATION) transaction.caMetadataForCasDiscovery else emptyList()
        val serviceScopedCa = allCaMetadata.filter { it.source != com.maleicacid.tvinput.aribsi.CaMetadataSource.CAT && it.serviceKey != null }
        val catCa = allCaMetadata.filter { it.source == com.maleicacid.tvinput.aribsi.CaMetadataSource.CAT }
        val caMetadata = caMapper.expandProgramLevelToElementaryStreams(serviceScopedCa + catCa, servicesForCas)
        val pmtPids = transaction.pmtPidsForSectionFilters.filter { it in 0..0x1fff }.toSet()
        val ecmPids = caMetadata.mapNotNull { it.ecmPid }.filter { it in 0..0x1fff }.toSet()
        val emmPids = caMetadata.mapNotNull { it.emmPid }.filter { it in 0..0x1fff }.toSet()
        tunerController.openDynamicFiltersFromCurrentSi(pmtPids, ecmPids, emmPids)
        if (caMetadata.isEmpty()) {
            casController.clearForClearService()
            return
        }
        val unsupported = caMapper.unsupportedForB25B1(caMetadata, CasController.SupportedCasSystemIds.B25_B1)
        unsupported.forEach { Log.w(LogTags.TIS, "対象外 CA metadata を無視します caSystemId=${it.caSystemId}") }
        val bridge = if (serviceScopedCa.isEmpty()) null else tunerController.createDescramblerBridge()
        casController.updateFromCaMetadata(caMetadata, bridge)
    }

    private fun publishCurrentServiceSnapshot(mode: PublishMode): PublishSnapshotResult {
        if (mode == PublishMode.DIAGNOSTIC_ONLY) return PublishSnapshotResult(0)
        if (mode == PublishMode.LIVE_TUNE_REFRESH || mode == PublishMode.BOOT_EPG_SYNC || mode == PublishMode.BACKGROUND_CHANNEL_MAINTENANCE) {
            val result = publishProgramsForRegisteredServices(mode, allowedServiceKeys = null)
            Log.d(LogTags.TIS, "${mode} では新規 channel row を追加しません changed=${result.changed} skippedNoChannel=${result.skippedNoChannel} skippedUnchanged=${result.skippedUnchanged}")
            return PublishSnapshotResult(result.changed, result.failures)
        }
        val candidate = currentCandidate ?: return PublishSnapshotResult(0)
        val transaction = engine.takeProgramPublishSnapshot(takeUpdateWindows = false)
        val transportRemoteKeys = transaction.transports.associateBy({ it.originalNetworkId to it.transportStreamId }, { it.remoteControlKeyId })
        val diagnostics = transaction.publishabilityDiagnostics.associateBy { it.serviceKey }
        val registrationReadyServices = transaction.services.filter { service ->
            diagnostics[service.serviceKey]?.channelRegistrationReady == true
        }
        val services = filterServicesForCurrentCandidate(registrationReadyServices, transaction.sdtActualTransports)
        val channels = services.map { service ->
            val remoteKey = transportRemoteKeys[service.serviceKey.originalNetworkId to service.serviceKey.transportStreamId]
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
                requiresCas = diagnostic?.requiresCas == true,
                unsupportedCas = diagnostic?.unsupportedCas == true,
                clearLivePlaybackSupported = diagnostic?.clearLivePlaybackSupported == true,
                channelRegistrationReady = diagnostic?.channelRegistrationReady == true,
                epgPublishable = diagnostic?.epgPublishable == true,
            )
        }
        if (channels.isEmpty()) {
            val incomplete = diagnostics.filterValues { !it.channelRegistrationReady }
                .mapValues { (_, d) -> (d.missingComponents + d.registrationReasons + d.reasons).distinct() }
            Log.d(LogTags.TIS, "registration-ready なサービスがないため channel snapshot 登録を省略します candidate=$candidate stage=${engine.discoveryStage()} incomplete=$incomplete")
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
        sdtActualTransports: List<com.maleicacid.tvinput.aribsi.AribTransport>,
    ): List<AribService> {
        // B-20/N-10: registration は同一 snapshot transaction 内の SDT actual で確定した
        // 現在 TS の TransportKey に完全一致する service だけに限定する。
        // PMT mapping / SDT-other / NIT-other / BAT 由来 transport は、現在 candidate の物理情報へ紐づけない。
        val actualTransports = sdtActualTransports
            .map { it.originalNetworkId to it.transportStreamId }
            .toSet()
        if (actualTransports.size != 1) {
            skippedUnresolvedTransportCount += services.size
            Log.w(LogTags.TIS, "current candidate の SDT actual TransportKey が一意に確定していないため channel 登録を省略します actualTransports=$actualTransports")
            return emptyList()
        }
        val actualTransport = actualTransports.single()
        val filtered = services.filter { (it.serviceKey.originalNetworkId to it.serviceKey.transportStreamId) == actualTransport }
        skippedUnresolvedTransportCount += services.size - filtered.size
        return filtered
    }

    private fun publishProgramsForRegisteredServices(mode: PublishMode, allowedServiceKeys: Set<com.maleicacid.tvinput.common.ServiceKey>?): ProgramPublishCoordinator.ProgramPublishResult {
        val channelFallbackResult = tvProviderWriter.existingChannelsResult()
        if (channelFallbackResult.isFailure) {
            val diagnostic = TvProviderWriter.Diagnostic(null, "existing-channels-query", channelFallbackResult.exceptionOrNull()?.message.orEmpty())
            Log.w(LogTags.TIS, "既存 channel 復元失敗のため Programs publish を中止します mode=$mode diagnostic=$diagnostic")
            return ProgramPublishCoordinator.ProgramPublishResult(0, 0, failures = listOf(diagnostic))
        }
        val transaction = engine.takeProgramPublishSnapshot(takeUpdateWindows = true)
        val publishabilityByServiceKey = transaction.publishabilityDiagnostics.associateBy { it.serviceKey }
        val channelFallbackByServiceKey = channelFallbackResult.getOrThrow().associateBy { it.serviceKey }
        val allPrograms = EventModelMapper().toProgramRecords(
            events = transaction.events,
            publishabilityByServiceKey = publishabilityByServiceKey,
            channelFallbackByServiceKey = channelFallbackByServiceKey,
        )
        val updateWindows = transaction.epgUpdateWindows.map { update ->
            val validProgramKeys = allPrograms
                .filter { program ->
                    program.serviceKey == update.serviceKey &&
                        program.startTimeMillis < update.windowEndMillis &&
                        program.startTimeMillis + program.durationMillis > update.windowStartMillis
                }
                .map { ProviderDataBridge.buildProgramKey(it) }
                .toSet()
            ProgramPublishCoordinator.EpgUpdateWindow(
                serviceKey = update.serviceKey,
                windowStartMs = update.windowStartMillis,
                windowEndMs = update.windowEndMillis,
                validProgramKeys = validProgramKeys,
                deletionAuthoritative = update.deletionAuthoritative,
            )
        }
        // Phase C/B-07: even when this parser snapshot has no new events/windows,
        // ProgramPublishCoordinator may have process-local retry windows from a prior
        // provider failure. Always enter the coordinator so retry state can drain.
        val result = programPublishCoordinator.publishWithUpdates(mode, allPrograms, updateWindows, allowedServiceKeys)
        // Phase C/B-28: do not issue an additional TvProvider query after the
        // coordinator has already separated query failure from channel absence.
        // A second query failure used only for logging would otherwise bypass
        // ProgramPublishResult.failures and let boot pending clear treat the
        // candidate as successful.
        if (result.skippedNoChannel > 0) Log.d(LogTags.TIS, "${mode} で未登録channelのeventをskipしました skipped=${result.skippedNoChannel}")
        if (result.failures.isNotEmpty()) Log.w(LogTags.TIS, "TvProvider program 登録失敗=${result.failures}")
        return result
    }

    private fun serviceCounts(): ServiceCounts {
        val transaction = engine.takeProgramPublishSnapshot(takeUpdateWindows = false)
        val publishability = transaction.publishabilityDiagnostics.associateBy { it.serviceKey }
        val completeness = transaction.services.map { service ->
            ServiceListBuilder.completenessForModel(service, publishability[service.serviceKey])
        }
        val summary = ServiceListBuilder.ServiceSnapshotSummary(
            totalKeys = completeness.map { it.serviceKey }.toSet(),
            completeKeys = completeness.filter { it.isComplete }.map { it.serviceKey }.toSet(),
            clearLivePlaybackSupportedKeys = completeness.filter { it.isClearLivePlaybackSupported }.map { it.serviceKey }.toSet(),
            registrationReadyKeys = completeness.filter { it.isRegistrationReady }.map { it.serviceKey }.toSet(),
            epgPublishableKeys = completeness.filter { it.isEpgPublishable }.map { it.serviceKey }.toSet(),
            completeness = completeness,
        )
        return ServiceCounts(
            total = summary.total,
            complete = summary.complete,
            clearLivePlaybackSupported = summary.clearLivePlaybackSupported,
            registrationReady = summary.registrationReady,
            signature = summary.stableSignature(),
            incompleteReasons = completeness
                .filter { !it.isRegistrationReady }
                .associate { it.serviceKey to (it.missingComponents + it.registrationReasons + it.reasons).distinct() },
        )
    }

    private fun collectSiForCandidate(candidate: ScanCandidate): SiCollectionResult {
        val policy = DEFAULT_SI_POLICY
        val startedAt = System.currentTimeMillis()
        var lastStage = engine.discoveryStage()
        var lastCounts = serviceCounts()
        var stableSince = startedAt
        var outcome = SiCollectionOutcome.TIMEOUT_PARTIAL

        while (!cancelled.get()) {
            refreshDynamicSectionFilters()
            val now = System.currentTimeMillis()
            val stage = engine.discoveryStage()
            val counts = serviceCounts()
            if (stage != lastStage || counts.signature != lastCounts.signature) {
                lastStage = stage
                lastCounts = counts
                stableSince = now
            }
            val elapsed = now - startedAt
            val stableFor = now - stableSince
            if (engine.isDiscoveryComplete() && elapsed >= policy.minWaitMs) {
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
        val complete = engine.isDiscoveryComplete()
        if (!cancelled.get() && complete) outcome = SiCollectionOutcome.COMPLETE
        val finalCounts = serviceCounts()
        val finalRegistrationReadySnapshotAvailable = finalCounts.registrationReady > 0
        if (outcome == SiCollectionOutcome.TIMEOUT_PARTIAL && !finalRegistrationReadySnapshotAvailable) outcome = SiCollectionOutcome.INCOMPLETE_NO_REGISTRATION_READY_SERVICE
        val elapsed = System.currentTimeMillis() - startedAt
        val message = if (outcome == SiCollectionOutcome.COMPLETE) {
            null
        } else {
            "SI 収集が完全完了していません outcome=$outcome stage=${engine.discoveryStage()} services=${finalCounts.total} completeServices=${finalCounts.complete} clearLivePlaybackSupportedServices=${finalCounts.clearLivePlaybackSupported} registrationReadyServices=${finalCounts.registrationReady} incomplete=${finalCounts.incompleteReasons} sections=${ingestController.diagnosticSummary()} elapsedMs=$elapsed"
        }
        Log.i(LogTags.TIS, "scan 候補の SI 収集結果 candidate=$candidate outcome=$outcome complete=$complete counts=$finalCounts message=$message")
        return SiCollectionResult(
            outcome = outcome,
            diagnostic = message?.let { ScanDiagnostic(candidate, it) },
            countsSignature = finalCounts.signature,
            clearLivePlaybackSupportedServices = finalCounts.clearLivePlaybackSupported,
            registrationReadyServices = finalCounts.registrationReady,
            registrationReadySnapshotAvailable = finalRegistrationReadySnapshotAvailable,
        )
    }


    private fun bootEpgSyncCandidates(): List<ScanCandidate> {
        val channels = tvProviderWriter.existingChannelsResult().getOrElse { error ->
            Log.w(LogTags.TIS, "既存 channel 復元失敗のため boot/background scan candidate を作成できません", error)
            return emptyList()
        }
        return channels.mapNotNull { channel -> scanCandidateFromChannel(channel) }
    }

    private fun backgroundMaintenanceCandidates(): List<ScanCandidate> = bootEpgSyncCandidates()

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

        fun siCollectionOutcomeForTest(
            discoveryComplete: Boolean,
            cancelled: Boolean,
            elapsedMs: Long,
            stableForMs: Long,
            clearLivePlaybackSupportedServices: Int,
            policy: SiCollectionPolicy,
            registrationReadyServices: Int = clearLivePlaybackSupportedServices,
            registrationReadySnapshotAvailable: Boolean = false,
        ): SiCollectionOutcome = when {
            cancelled -> SiCollectionOutcome.CANCELLED
            discoveryComplete && elapsedMs >= policy.minWaitMs -> SiCollectionOutcome.COMPLETE
            elapsedMs >= policy.minWaitMs && registrationReadySnapshotAvailable && stableForMs >= policy.stableWaitMs -> SiCollectionOutcome.STABLE_PARTIAL
            elapsedMs >= policy.maxWaitMs && registrationReadySnapshotAvailable -> SiCollectionOutcome.TIMEOUT_PARTIAL
            elapsedMs >= policy.maxWaitMs -> SiCollectionOutcome.INCOMPLETE_NO_REGISTRATION_READY_SERVICE
            else -> SiCollectionOutcome.TIMEOUT_PARTIAL
        }
    }
}

package com.maleicacid.tvinput.tis

import android.content.Context
import android.media.tv.TvInputService
import android.util.Log
import com.maleicacid.tvinput.aribsi.AribService
import com.maleicacid.tvinput.aribsi.AribSiEngine
import com.maleicacid.tvinput.aribsi.EventModelMapper
import com.maleicacid.tvinput.aribsi.PmtCatCaMetadataMapper
import com.maleicacid.tvinput.aribsi.SectionIngestController
import com.maleicacid.tvinput.aribsi.ServiceListBuilder
import com.maleicacid.tvinput.common.LogTags
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.db.ChannelRecord
import java.util.concurrent.atomic.AtomicBoolean

class ChannelScanController(
    private val context: Context,
    private val inputId: String,
    private val engine: AribSiEngine,
) : AutoCloseable {
    data class ScanDiagnostic(val candidate: ScanCandidate, val message: String)
    data class ScanResult(val scanned: Int, val published: Int, val diagnostics: List<ScanDiagnostic>)
    data class SiCollectionPolicy(
        val minWaitMs: Long = 2_000L,
        val maxWaitMs: Long = 12_000L,
        val stableWaitMs: Long = 1_200L,
        val pollIntervalMs: Long = 200L,
    )

    enum class PublishMode {
        SETUP_SCAN,
        LIVE_TUNE_REFRESH,
        DIAGNOSTIC_ONLY,
    }
    private data class ServiceCounts(
        val total: Int,
        val complete: Int,
        val viewable: Int,
        val signature: String,
        val incompleteReasons: Map<ServiceKey, List<String>>,
    )

    private val tunerController = TunerController(context, inputId, TvInputService.PRIORITY_HINT_USE_CASE_TYPE_SCAN)
    private val ingestController = SectionIngestController(engine)
    private val serviceListBuilder = ServiceListBuilder(engine)
    private val tvProviderWriter = TvProviderWriter(context, inputId)
    private val caMapper = PmtCatCaMetadataMapper()
    private val casController = CasController()
    private val cancelled = AtomicBoolean(false)
    private var currentCandidate: ScanCandidate? = null

    init {
        tunerController.setSectionIngestController(ingestController)
        tunerController.setCasController(casController)
        tunerController.setOnSectionIngestedCallback { refreshDynamicSectionFilters() }
    }

    fun startInitialScan(candidates: List<ScanCandidate> = JapanIsdbScanPlan.defaultInitialScan()): ScanResult {
        cancelled.set(false)
        val diagnostics = mutableListOf<ScanDiagnostic>()
        var published = 0
        candidates.forEach { candidate ->
            if (cancelled.get()) return@forEach
            engine.reset()
            currentCandidate = candidate
            val tune = tunerController.tuneForScan(candidate)
            if (!tune.success) {
                diagnostics += ScanDiagnostic(candidate, "選局に失敗しました result=${tune.resultCode} ${tune.message}")
                return@forEach
            }
            val incompleteDiagnostic = collectSiForCandidate(candidate)
            if (incompleteDiagnostic != null) {
                diagnostics += incompleteDiagnostic
                Log.w(LogTags.TIS, "SI 収集未完了のため TvProvider 登録を省略します candidate=$candidate diagnostic=${incompleteDiagnostic.message}")
                return@forEach
            }
            published += publishCurrentServiceSnapshot(PublishMode.SETUP_SCAN)
        }
        currentCandidate = null
        return ScanResult(candidates.size, published, diagnostics)
    }

    fun cancelScan() {
        cancelled.set(true)
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
        val caMetadata = if (ENABLE_CAS_ORCHESTRATION) caMapper.expandProgramLevelToElementaryStreams(engine.snapshotCaMetadata(), engine.snapshotServices()) else emptyList()
        val pmtPids = engine.snapshotPmtPids().map { it.pmtPid }.filter { it in 0..0x1fff }.toSet()
        val ecmPids = caMetadata.mapNotNull { it.ecmPid }.filter { it in 0..0x1fff }.toSet()
        val emmPids = caMetadata.mapNotNull { it.emmPid }.filter { it in 0..0x1fff }.toSet()
        tunerController.openDynamicFiltersFromCurrentSi(pmtPids, ecmPids, emmPids)
        if (caMetadata.isEmpty()) {
            casController.clearForClearService()
            return
        }
        val unsupported = caMapper.unsupportedForB25B1(caMetadata, CasController.SupportedCasSystemIds.B25_B1)
        unsupported.forEach { Log.w(LogTags.TIS, "対象外 CA metadata を無視します caSystemId=${it.caSystemId}") }
        casController.updateFromCaMetadata(caMetadata, tunerController.createDescramblerBridge())
    }

    private fun publishCurrentServiceSnapshot(mode: PublishMode): Int {
        if (mode == PublishMode.DIAGNOSTIC_ONLY) return 0
        if (mode == PublishMode.LIVE_TUNE_REFRESH) {
            publishProgramsForViewableServices()
            Log.d(LogTags.TIS, "live tune refresh では新規 channel row を追加しません")
            return 0
        }
        val candidate = currentCandidate ?: return 0
        if (!engine.isDiscoveryComplete()) {
            Log.w(LogTags.TIS, "SI discovery 未完了のため channel 登録を省略します mode=$mode stage=${engine.discoveryStage()} incomplete=${serviceListBuilder.incompleteReasons()}")
            return 0
        }
        val transportRemoteKeys = engine.snapshotTransports().associateBy({ it.originalNetworkId to it.transportStreamId }, { it.remoteControlKeyId })
        val services = serviceListBuilder.publishableViewableSnapshot()
        val channels = services.map { service ->
            val remoteKey = transportRemoteKeys[service.serviceKey.originalNetworkId to service.serviceKey.transportStreamId]
            ChannelRecord(
                serviceKey = service.serviceKey,
                displayNumber = ChannelNumberingPolicy.displayNumber(service, remoteKey, candidate),
                displayName = service.name.ifEmpty { "service-${service.serviceKey.serviceId}" },
                frequencyHz = candidate.frequencyHz,
                deliverySystem = candidate.deliverySystem,
                streamSelector = candidate.streamSelector,
                physicalChannel = candidate.physicalChannel,
                backendHint = candidate.backendHint,
                satelliteBand = candidate.satelliteBand,
                remoteControlKeyId = remoteKey,
            )
        }
        if (channels.isEmpty()) {
            Log.d(LogTags.TIS, "publishable かつ視聴可能なサービスがないため channel snapshot 登録を省略します candidate=$candidate stage=${engine.discoveryStage()} incomplete=${serviceListBuilder.incompleteReasons()}")
            return 0
        }
        val channelResult = tvProviderWriter.upsertChannels(channels)
        if (channelResult.failures.isNotEmpty()) Log.w(LogTags.TIS, "TvProvider channel 登録失敗=${channelResult.failures}")
        publishProgramsForViewableServices()
        return channelResult.inserted + channelResult.updated
    }

    private fun publishProgramsForViewableServices() {
        val programs = EventModelMapper().toProgramRecords(engine.snapshotEvents())
        if (programs.isEmpty()) return
        val result = tvProviderWriter.upsertPrograms(programs)
        if (result.failures.isNotEmpty()) Log.w(LogTags.TIS, "TvProvider program 登録失敗=${result.failures}")
    }

    private fun isServiceViewable(service: AribService): Boolean = serviceListBuilder.isServiceViewable(service)

    private fun serviceCounts(): ServiceCounts {
        val summary = serviceListBuilder.completenessSummary()
        return ServiceCounts(
            total = summary.total,
            complete = summary.complete,
            viewable = summary.viewable,
            signature = summary.stableSignature(),
            incompleteReasons = serviceListBuilder.incompleteReasons(),
        )
    }

    private fun collectSiForCandidate(candidate: ScanCandidate): ScanDiagnostic? {
        val policy = DEFAULT_SI_POLICY
        val startedAt = System.currentTimeMillis()
        var lastStage = engine.discoveryStage()
        var lastCounts = serviceCounts()
        var stableSince = startedAt

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
            if (engine.isDiscoveryComplete() && elapsed >= policy.minWaitMs) break
            if (elapsed >= policy.maxWaitMs) break
            runCatching { Thread.sleep(policy.pollIntervalMs) }
        }
        val complete = engine.isDiscoveryComplete()
        val finalCounts = serviceCounts()
        val elapsed = System.currentTimeMillis() - startedAt
        val message = if (complete) {
            null
        } else {
            "SI 収集が完了しません stage=${engine.discoveryStage()} services=${finalCounts.total} completeServices=${finalCounts.complete} viewableServices=${finalCounts.viewable} incomplete=${finalCounts.incompleteReasons} elapsedMs=$elapsed"
        }
        Log.i(LogTags.TIS, "scan 候補の SI 収集結果 candidate=$candidate complete=$complete counts=$finalCounts message=$message")
        return message?.let { ScanDiagnostic(candidate, it) }
    }

    override fun close() {
        cancelScan()
        casController.close()
        tunerController.release()
    }

    companion object {
        private val DEFAULT_SI_POLICY = SiCollectionPolicy()
        private const val ENABLE_CAS_ORCHESTRATION = true
    }
}

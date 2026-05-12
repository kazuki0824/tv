package com.maleicacid.tvinput.aribsi

import android.content.Context
import android.util.Log
import com.maleicacid.tvinput.common.LogTags

class AribSiEngine(private val context: Context) : AutoCloseable {
    data class SnapshotTransaction(
        val services: List<AribService>,
        val servicesForCasDiscovery: List<AribService>,
        val caMetadata: List<CaMetadata>,
        val caMetadataForCasDiscovery: List<CaMetadata>,
        val pmtPidMappings: List<PmtPidMapping>,
        val pmtPidsForSectionFilters: List<Int>,
        val transports: List<AribTransport>,
        val sdtActualTransports: List<AribTransport>,
        val privateSections: List<PrivateSection>,
        val events: List<AribEvent>,
        val epgUpdateWindows: List<AribEpgUpdateWindow>,
        val publishabilityDiagnostics: List<ServicePublishabilityDiagnostic>,
    )

    private val lock = Any()
    private var nativeParser = NativeAribSiParser()

    fun ingestSection(pid: Int, section: ByteArray): SiIngestResult = synchronized(lock) {
        val status = nativeParser.ingestSection(pid, section)
        Log.d(LogTags.ARIBSI, "section 取り込み pid=$pid size=${section.size} status=$status")
        SiIngestResult(pid = pid, status = status)
    }

    fun discoveryStage(): Int = synchronized(lock) { nativeParser.discoveryStage() }

    fun isDiscoveryComplete(): Boolean = synchronized(lock) { nativeParser.isDiscoveryComplete() }

    fun takeProgramPublishSnapshot(takeUpdateWindows: Boolean = false): SnapshotTransaction = synchronized(lock) {
        val snapshot = nativeParser.snapshotBulk(takeUpdateWindows)
        SnapshotTransaction(
            services = snapshot.services,
            servicesForCasDiscovery = snapshot.servicesForCasDiscovery,
            caMetadata = snapshot.caMetadata,
            caMetadataForCasDiscovery = snapshot.caMetadataForCasDiscovery,
            pmtPidMappings = snapshot.pmtPidMappings,
            pmtPidsForSectionFilters = snapshot.pmtPidsForSectionFilters,
            transports = snapshot.transports,
            sdtActualTransports = snapshot.sdtActualTransports,
            privateSections = snapshot.privateSections,
            events = snapshot.events,
            epgUpdateWindows = snapshot.epgUpdateWindows,
            publishabilityDiagnostics = snapshot.publishabilityDiagnostics,
        )
    }

    @Deprecated("production code must use takeProgramPublishSnapshot(); snapshotTransaction is kept only for old tests", level = DeprecationLevel.ERROR)
    fun snapshotTransaction(takeUpdateWindows: Boolean = false): SnapshotTransaction =
        takeProgramPublishSnapshot(takeUpdateWindows)

    @Deprecated("production code must use takeProgramPublishSnapshot() so all values come from one native snapshot", level = DeprecationLevel.ERROR)
    fun snapshotServices(): List<AribService> = synchronized(lock) { nativeParser.snapshotServicesBulk() }

    @Deprecated("production code must use takeProgramPublishSnapshot() so all values come from one native snapshot", level = DeprecationLevel.ERROR)
    fun publishabilityDiagnosticsForTestOnly(): List<ServicePublishabilityDiagnostic> = synchronized(lock) { nativeParser.publishabilityDiagnosticsForTestOnly() }

    @Deprecated("production code must use takeProgramPublishSnapshot() so all values come from one native snapshot", level = DeprecationLevel.ERROR)
    fun snapshotPmtPids(): List<PmtPidMapping> = synchronized(lock) { nativeParser.snapshotPmtPidsBulk() }

    @Deprecated("production code must use takeProgramPublishSnapshot() so all values come from one native snapshot", level = DeprecationLevel.ERROR)
    fun snapshotPmtPidsForSectionFilters(): List<Int> = synchronized(lock) { nativeParser.snapshotPmtPidsForSectionFiltersBulk() }

    @Deprecated("production code must use takeProgramPublishSnapshot() so all values come from one native snapshot", level = DeprecationLevel.ERROR)
    fun snapshotTransports(): List<AribTransport> = synchronized(lock) { nativeParser.snapshotTransportsBulk() }

    @Deprecated("production code must use takeProgramPublishSnapshot() so all values come from one native snapshot", level = DeprecationLevel.ERROR)
    fun snapshotSdtActualTransports(): List<AribTransport> = synchronized(lock) { nativeParser.snapshotSdtActualTransportsBulk() }

    @Deprecated("production code must use takeProgramPublishSnapshot() so all values come from one native snapshot", level = DeprecationLevel.ERROR)
    fun snapshotServicesForCasDiscovery(): List<AribService> = synchronized(lock) { nativeParser.snapshotServicesForCasDiscoveryBulk() }

    @Deprecated("production code must use takeProgramPublishSnapshot() so all values come from one native snapshot", level = DeprecationLevel.ERROR)
    fun snapshotCaMetadata(): List<CaMetadata> = synchronized(lock) { nativeParser.snapshotCaMetadataBulk() }

    @Deprecated("production code must use takeProgramPublishSnapshot() so all values come from one native snapshot", level = DeprecationLevel.ERROR)
    fun snapshotCaMetadataForCasDiscovery(): List<CaMetadata> = synchronized(lock) { nativeParser.snapshotCaMetadataForCasDiscoveryBulk() }

    @Deprecated("production code must use takeProgramPublishSnapshot() so all values come from one native snapshot", level = DeprecationLevel.ERROR)
    fun snapshotPrivateSections(): List<PrivateSection> = synchronized(lock) { nativeParser.snapshotPrivateSectionsBulk() }

    fun decodeAribString(bytes: ByteArray): String = synchronized(lock) { nativeParser.decodeAribString(bytes) }

    fun decodeAribStringDiagnosticSummary(bytes: ByteArray): String = synchronized(lock) { nativeParser.decodeAribStringDiagnosticSummary(bytes) }

    @Deprecated("production code must use takeProgramPublishSnapshot() so all values come from one native snapshot", level = DeprecationLevel.ERROR)
    fun eventsForTestOnly(): List<AribEvent> = synchronized(lock) { nativeParser.eventsForTestOnly() }

    /**
     * Phase C/B-07: this is a drain API, not a stable snapshot.
     * Each returned window is removed from the native parser state exactly once.
     */
    @Deprecated("production code must use takeProgramPublishSnapshot() so all values come from one native snapshot", level = DeprecationLevel.ERROR)
    fun drainEpgWindowsForTestOnly(): List<AribEpgUpdateWindow> = synchronized(lock) { nativeParser.drainEpgWindowsForTestOnly() }

    fun snapshotEventDiagnostics(): List<AribEventDiagnostic> = synchronized(lock) {
        nativeParser.snapshotBulk(takeUpdateWindows = false).events.map { event ->
            AribEventDiagnostic(event.serviceKey, event.stableIdentity, event.eventId, event.diagnosticText, event.diagnosticDescriptorJson)
        }
    }

    fun reset() = synchronized(lock) {
        nativeParser.close()
        nativeParser = NativeAribSiParser()
    }

    override fun close() = synchronized(lock) {
        nativeParser.close()
    }
}

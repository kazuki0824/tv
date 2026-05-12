package com.maleicacid.tvinput.aribsi

import android.content.Context
import android.util.Log
import com.maleicacid.tvinput.common.LogTags

class AribSiEngine(private val context: Context) : AutoCloseable {
    private val lock = Any()
    private var nativeParser = NativeAribSiParser()

    fun ingestSection(pid: Int, section: ByteArray): SiIngestResult = synchronized(lock) {
        val status = nativeParser.ingestSection(pid, section)
        Log.d(LogTags.ARIBSI, "section 取り込み pid=$pid size=${section.size} status=$status")
        SiIngestResult(pid = pid, status = status)
    }

    fun discoveryStage(): Int = synchronized(lock) { nativeParser.discoveryStage() }

    fun isDiscoveryComplete(): Boolean = synchronized(lock) { nativeParser.isDiscoveryComplete() }

    fun snapshotServices(): List<AribService> = synchronized(lock) { nativeParser.snapshotServicesBulk() }

    fun snapshotPublishabilityDiagnostics(): List<ServicePublishabilityDiagnostic> = synchronized(lock) { nativeParser.snapshotPublishabilityDiagnosticsBulk() }

    fun snapshotPmtPids(): List<PmtPidMapping> = synchronized(lock) { nativeParser.snapshotPmtPidsBulk() }

    fun snapshotPmtPidsForSectionFilters(): List<Int> = synchronized(lock) { nativeParser.snapshotPmtPidsForSectionFiltersBulk() }

    fun snapshotTransports(): List<AribTransport> = synchronized(lock) { nativeParser.snapshotTransportsBulk() }

    fun snapshotServicesForCasDiscovery(): List<AribService> = synchronized(lock) { nativeParser.snapshotServicesForCasDiscoveryBulk() }

    fun snapshotCaMetadata(): List<CaMetadata> = synchronized(lock) { nativeParser.snapshotCaMetadataBulk() }

    fun snapshotCaMetadataForCasDiscovery(): List<CaMetadata> = synchronized(lock) { nativeParser.snapshotCaMetadataForCasDiscoveryBulk() }

    fun snapshotPrivateSections(): List<PrivateSection> = synchronized(lock) { nativeParser.snapshotPrivateSectionsBulk() }

    fun decodeAribString(bytes: ByteArray): String = synchronized(lock) { nativeParser.decodeAribString(bytes) }

    fun decodeAribStringDiagnosticSummary(bytes: ByteArray): String = synchronized(lock) { nativeParser.decodeAribStringDiagnosticSummary(bytes) }

    fun snapshotEvents(): List<AribEvent> = synchronized(lock) { nativeParser.snapshotEventsBulk() }

    fun snapshotEpgUpdateWindows(): List<AribEpgUpdateWindow> = synchronized(lock) { nativeParser.takeEpgUpdateWindowsBulk() }

    fun snapshotEventDiagnostics(): List<AribEventDiagnostic> = synchronized(lock) {
        nativeParser.snapshotEventsBulk().map { event ->
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

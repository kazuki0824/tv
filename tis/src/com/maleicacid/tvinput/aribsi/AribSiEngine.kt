package com.maleicacid.tvinput.aribsi

import android.content.Context
import android.util.Log
import com.maleicacid.tvinput.common.LogTags

class AribSiEngine(private val context: Context) : AutoCloseable {
    private var nativeParser = NativeAribSiParser()

    fun ingestSection(pid: Int, section: ByteArray): SiIngestResult {
        val status = nativeParser.ingestSection(pid, section)
        Log.d(LogTags.ARIBSI, "section 取り込み pid=$pid size=${section.size} status=$status")
        return SiIngestResult(pid = pid, status = status)
    }

    fun discoveryStage(): Int = nativeParser.discoveryStage()

    fun isDiscoveryComplete(): Boolean = nativeParser.isDiscoveryComplete()

    fun snapshotServices(): List<AribService> = nativeParser.snapshotServices()

    fun snapshotPublishabilityDiagnostics(): List<ServicePublishabilityDiagnostic> = nativeParser.snapshotPublishabilityDiagnostics()

    fun snapshotPmtPids(): List<PmtPidMapping> = nativeParser.snapshotPmtPids()

    fun snapshotTransports(): List<AribTransport> = nativeParser.snapshotTransports()

    fun snapshotCaMetadata(): List<CaMetadata> = nativeParser.snapshotCaMetadata()

    fun snapshotPrivateSections(): List<PrivateSection> = nativeParser.snapshotPrivateSections()

    fun decodeAribString(bytes: ByteArray): String = nativeParser.decodeAribString(bytes)

    fun decodeAribStringDiagnosticSummary(bytes: ByteArray): String = nativeParser.decodeAribStringDiagnosticSummary(bytes)

    fun snapshotEvents(): List<AribEvent> = nativeParser.snapshotEvents()

    fun snapshotEventDiagnostics(): List<AribEventDiagnostic> = nativeParser.snapshotEvents().map { event ->
        AribEventDiagnostic(event.serviceKey, event.stableIdentity, event.eventId, event.diagnosticText, event.diagnosticDescriptorJson)
    }

    @Synchronized
    fun reset() {
        nativeParser.close()
        nativeParser = NativeAribSiParser()
    }

    override fun close() {
        nativeParser.close()
    }
}

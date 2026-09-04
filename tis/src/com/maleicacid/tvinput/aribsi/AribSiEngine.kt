package com.maleicacid.tvinput.aribsi

import android.content.Context
import android.util.Log
import com.maleicacid.tvinput.common.LogTags
import com.maleicacid.tvinput.common.TsPid

class AribSiEngine(private val context: Context) : AutoCloseable {
    private val lock = Any()
    private var nativeParser = NativeAribSiParser()

    fun ingestSection(pid: TsPid, section: ByteArray): SiIngestResult = synchronized(lock) {
        val status = nativeParser.ingestSection(pid, section)
        Log.d(LogTags.ARIBSI, "section 取り込み pid=${pid.value} size=${section.size} status=$status")
        SiIngestResult(pid = pid, status = status)
    }

    fun takeProgramPublishSnapshot(): ProgramPublishSnapshot = synchronized(lock) {
        nativeParser.takeProgramPublishSnapshot()
    }

    fun programStateSnapshot(): ProgramPublishSnapshot = synchronized(lock) {
        nativeParser.programStateSnapshot()
    }

    fun serviceRegistrationSnapshot(): ServiceRegistrationSnapshot = synchronized(lock) {
        nativeParser.serviceRegistrationSnapshot()
    }

    fun casDiscoverySnapshot(): CasDiscoverySnapshot = synchronized(lock) {
        nativeParser.casDiscoverySnapshot()
    }

    fun livePlaybackSnapshot(): LivePlaybackSnapshot = synchronized(lock) {
        nativeParser.livePlaybackSnapshot()
    }

    fun broadcastClockSnapshot(): AribBroadcastClockFact? = synchronized(lock) {
        nativeParser.broadcastClockSnapshot()
    }

    fun decodeAribString(bytes: ByteArray): String = synchronized(lock) { nativeParser.decodeAribString(bytes) }
    fun decodeAribStringDiagnosticSummary(bytes: ByteArray): String = synchronized(lock) { nativeParser.decodeAribStringDiagnosticSummary(bytes) }

    fun reset(discoveryProfile: Int = SiDiscoveryProfile.ISDB_T) = synchronized(lock) {
        nativeParser.close()
        nativeParser = NativeAribSiParser()
        nativeParser.setDiscoveryProfile(discoveryProfile)
    }

    fun setDiscoveryProfile(discoveryProfile: Int) = synchronized(lock) {
        nativeParser.setDiscoveryProfile(discoveryProfile)
    }

    override fun close() = synchronized(lock) { nativeParser.close() }
}

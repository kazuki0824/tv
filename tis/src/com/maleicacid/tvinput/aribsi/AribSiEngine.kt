package com.maleicacid.tvinput.aribsi

import android.content.Context
import android.util.Log
import com.maleicacid.tvinput.common.LogTags
import com.maleicacid.tvinput.common.TsPid
import java.util.concurrent.locks.ReentrantLock
import kotlin.concurrent.withLock

// 同じ状態・境界を扱う操作群を一つの所有者に保つ。
@Suppress("TooManyFunctions")
class AribSiEngine(
    @Suppress("UNUSED_PARAMETER") context: Context,
) : AutoCloseable {
    private val lock = ReentrantLock()
    private var nativeParser = NativeAribSiParser()

    fun ingestSection(
        pid: TsPid,
        section: ByteArray,
    ): SiIngestResult =
        lock.withLock {
            val status = nativeParser.ingestSection(pid, section)
            Log.d(LogTags.ARIBSI, "section 取り込み pid=${pid.value} size=${section.size} status=$status")
            SiIngestResult(pid = pid, status = status)
        }

    fun takeProgramPublishSnapshot(): ProgramPublishSnapshot =
        lock.withLock {
            nativeParser.takeProgramPublishSnapshot()
        }

    fun serviceRegistrationSnapshot(): ServiceRegistrationSnapshot =
        lock.withLock {
            nativeParser.serviceRegistrationSnapshot()
        }

    fun tryServiceRegistrationSnapshot(): ServiceRegistrationSnapshot? {
        if (!lock.tryLock()) return null
        return try {
            nativeParser.tryServiceRegistrationSnapshot()
        } finally {
            lock.unlock()
        }
    }

    // CAS discovery APIは設計契約として保持する。利用側の変更だけを理由に削除しない。
    @Suppress("unused")
    fun casDiscoverySnapshot(): CasDiscoverySnapshot =
        lock.withLock {
            nativeParser.casDiscoverySnapshot()
        }

    fun pmtPidsForSectionFilters(): Set<TsPid> =
        lock.withLock {
            nativeParser.pmtPidsForSectionFilters()
        }

    fun livePlaybackSnapshot(): LivePlaybackSnapshot =
        lock.withLock {
            nativeParser.livePlaybackSnapshot()
        }

    fun broadcastClockSnapshot(): AribBroadcastClockFact? =
        lock.withLock {
            nativeParser.broadcastClockSnapshot()
        }

    fun reset(discoveryProfile: Int = SiDiscoveryProfile.ISDB_T) =
        lock.withLock {
            nativeParser.close()
            nativeParser = NativeAribSiParser()
            nativeParser.setDiscoveryProfile(discoveryProfile)
        }

    fun setDiscoveryProfile(discoveryProfile: Int) =
        lock.withLock {
            nativeParser.setDiscoveryProfile(discoveryProfile)
        }

    override fun close() = lock.withLock { nativeParser.close() }
}

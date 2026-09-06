package com.maleicacid.tvinput.aribsi

import com.maleicacid.tvinput.common.TsPid

object WellKnownSectionPid {
    val PAT: TsPid = TsPid.PAT
    val CAT: TsPid = TsPid.CAT
    val NIT: TsPid = TsPid.NIT
    val SDT_BAT: TsPid = TsPid.SDT_BAT
    val EIT: TsPid = TsPid.EIT
    val TDT: TsPid = TsPid.TDT
}

data class SectionIngestCounter(
    val pid: TsPid,
    val tableId: Int,
    val status: Int,
    val acceptedCount: Int,
    val crcMismatchCount: Int,
    val malformedCount: Int,
    val lastErrorTimeMillis: Long,
)

class SectionIngestController(private val engine: AribSiEngine) {
    private data class MutableCounter(
        var accepted: Int = 0,
        var crcMismatch: Int = 0,
        var malformed: Int = 0,
        var lastErrorTimeMillis: Long = 0L,
    )

    private val counters = linkedMapOf<Triple<TsPid, Int, Int>, MutableCounter>()

    fun onSection(pid: TsPid, section: ByteArray): SiIngestResult {
        val tableId = section.firstOrNull()?.toInt()?.and(0xff) ?: -1
        val result = engine.ingestSection(pid, section)
        record(pid, tableId, result.status)
        return result
    }

    @Synchronized
    fun diagnostics(): List<SectionIngestCounter> = counters.map { (key, value) ->
        SectionIngestCounter(
            pid = key.first,
            tableId = key.second,
            status = key.third,
            acceptedCount = value.accepted,
            crcMismatchCount = value.crcMismatch,
            malformedCount = value.malformed,
            lastErrorTimeMillis = value.lastErrorTimeMillis,
        )
    }

    fun broadcastClockSnapshot(): AribBroadcastClockFact? = engine.broadcastClockSnapshot()

    fun diagnosticSummary(): String = diagnostics().joinToString("; ") { c ->
        "pid=${c.pid.value} table=${c.tableId} status=${c.status} ok=${c.acceptedCount} crc=${c.crcMismatchCount} malformed=${c.malformedCount} lastError=${c.lastErrorTimeMillis}"
    }

    @Synchronized
    private fun record(pid: TsPid, tableId: Int, status: Int) {
        val counter = counters.getOrPut(Triple(pid, tableId, status)) { MutableCounter() }
        when (statusBucketForTest(status)) {
            "accepted" -> counter.accepted++
            "crc" -> {
                counter.crcMismatch++
                counter.lastErrorTimeMillis = System.currentTimeMillis()
            }
            else -> {
                counter.malformed++
                counter.lastErrorTimeMillis = System.currentTimeMillis()
            }
        }
    }

    companion object {
        fun statusBucketForTest(status: Int): String = when (status) {
            SiStatus.OK -> "accepted"
            SiStatus.INVALID_SECTION -> "crc"
            else -> "malformed"
        }
    }
}

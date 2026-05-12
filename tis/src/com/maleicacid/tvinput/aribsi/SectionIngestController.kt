package com.maleicacid.tvinput.aribsi

object WellKnownSectionPid {
    const val PAT = 0x0000
    const val CAT = 0x0001
    const val NIT = 0x0010
    const val SDT_BAT = 0x0011
    const val EIT = 0x0012
}

data class SectionIngestCounter(
    val pid: Int,
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

    private val counters = linkedMapOf<Triple<Int, Int, Int>, MutableCounter>()

    fun onSection(pid: Int, section: ByteArray): SiIngestResult {
        require(pid in 0..0x1fff) { "PID が範囲外です: $pid" }
        val tableId = section.firstOrNull()?.toInt()?.and(0xff) ?: -1
        val result = engine.ingestSection(pid, section)
        record(pid, tableId, result.status)
        return result
    }

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

    fun diagnosticSummary(): String = diagnostics().joinToString("; ") { c ->
        "pid=${c.pid} table=${c.tableId} status=${c.status} ok=${c.acceptedCount} crc=${c.crcMismatchCount} malformed=${c.malformedCount} lastError=${c.lastErrorTimeMillis}"
    }

    private fun record(pid: Int, tableId: Int, status: Int) {
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

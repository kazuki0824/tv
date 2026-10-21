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
    val invalidSectionCount: Int,
    val malformedCount: Int,
    val lastErrorTimeMillis: Long,
)

data class SectionTransportLossSnapshot(
    val inputDeliveryLoss: Int,
    val filterOverflow: Int,
)

class SectionIngestController(
    private val engine: AribSiEngine,
) {
    private data class MutableCounter(
        var accepted: Int = 0,
        var invalidSection: Int = 0,
        var malformed: Int = 0,
        var lastErrorTimeMillis: Long = 0L,
    )

    @Volatile var inputDeliveryLossCount: Int = 0
        private set

    @Volatile var filterOverflowCount: Int = 0
        private set

    private val droppedByPid = linkedMapOf<TsPid, Int>()
    private var sdtActualReadCount = 0
    private var sdtOtherReadCount = 0
    private var sdtActualDroppedCount = 0
    private var sdtOtherDroppedCount = 0

    // 同一Session内の別選局でSDT診断値を混同しない。
    @Synchronized
    fun resetForTune() {
        inputDeliveryLossCount = 0
        filterOverflowCount = 0
        droppedByPid.clear()
        sdtActualReadCount = 0
        sdtOtherReadCount = 0
        sdtActualDroppedCount = 0
        sdtOtherDroppedCount = 0
        counters.clear()
    }

    // 正常長のfilter.read()が得たSDT-actual/otherをcontroller queue投入前に観測する。
    // これでHAL未配送・queue投入拒否・Rust解析拒否を区別できる。
    @Suppress("MagicNumber")
    @Synchronized
    fun recordSdtRead(
        pid: TsPid,
        tableId: Int,
    ) {
        if (pid != WellKnownSectionPid.SDT_BAT) return
        when (tableId) {
            0x42 -> sdtActualReadCount++
            0x46 -> sdtOtherReadCount++
        }
    }

    @Suppress("MagicNumber")
    @Synchronized
    fun recordInputDeliveryLoss(
        pid: TsPid? = null,
        tableId: Int? = null,
    ) {
        inputDeliveryLossCount++
        if (pid != null) droppedByPid[pid] = (droppedByPid[pid] ?: 0) + 1
        if (pid == WellKnownSectionPid.SDT_BAT) {
            when (tableId) {
                0x42 -> sdtActualDroppedCount++
                0x46 -> sdtOtherDroppedCount++
            }
        }
    }

    // SDK FilterのoverflowはControllerの入場拒否と別の入力喪失として観測する。
    @Synchronized
    fun recordFilterOverflow() {
        filterOverflowCount++
    }

    @Synchronized
    fun transportLossSnapshot(): SectionTransportLossSnapshot =
        SectionTransportLossSnapshot(inputDeliveryLossCount, filterOverflowCount)

    @Suppress("MagicNumber")
    @Synchronized
    fun sdtDeliveryDiagnostic(): String {
        val pid = WellKnownSectionPid.SDT_BAT
        val actual =
            counters.entries
                .filter { it.key.first == pid && it.key.second == 0x42 && it.key.third == SiStatus.OK }
                .sumOf { it.value.accepted }
        val other =
            counters.entries
                .filter { it.key.first == pid && it.key.second == 0x46 && it.key.third == SiStatus.OK }
                .sumOf { it.value.accepted }
        return "sdtActualAccepted=$actual sdtOtherAccepted=$other sdtPidDropped=${droppedByPid[pid] ?: 0} " +
            "sdtActualRead=$sdtActualReadCount sdtOtherRead=$sdtOtherReadCount " +
            "sdtActualDropped=$sdtActualDroppedCount sdtOtherDropped=$sdtOtherDroppedCount"
    }

    private val counters = linkedMapOf<Triple<TsPid, Int, Int>, MutableCounter>()

    // この処理の規格値・ビット幅・単位換算・固定上限をリテラルのまま照合できる形に保つ。
    @Suppress("MagicNumber")
    fun onSection(
        pid: TsPid,
        section: ByteArray,
    ): SiIngestResult {
        val tableId = section.firstOrNull()?.toInt()?.and(0xff) ?: -1
        val result = engine.ingestSection(pid, section)
        record(pid, tableId, result.status)
        return result
    }

    @Synchronized
    fun diagnostics(): List<SectionIngestCounter> =
        counters.map { (key, value) ->
            SectionIngestCounter(
                pid = key.first,
                tableId = key.second,
                status = key.third,
                acceptedCount = value.accepted,
                invalidSectionCount = value.invalidSection,
                malformedCount = value.malformed,
                lastErrorTimeMillis = value.lastErrorTimeMillis,
            )
        }

    @Synchronized
    private fun lossByPidSummary(): String =
        droppedByPid.entries.sortedBy { it.key.value }.joinToString(",") { "${it.key.value}:${it.value}" }

    fun broadcastClockSnapshot(): AribBroadcastClockFact? = engine.broadcastClockSnapshot()

    fun diagnosticSummary(): String =
        "inputDeliveryLoss=$inputDeliveryLossCount filterOverflow=$filterOverflowCount lostByPid=${lossByPidSummary()}; " +
            diagnostics().joinToString("; ") { c ->
                "pid=${c.pid.value} table=${c.tableId} status=${c.status} ok=" +
                    "${c.acceptedCount} invalidSection=${c.invalidSectionCount} malformed=${c.malformedCount} " +
                    "lastError=${c.lastErrorTimeMillis}"
            }

    @Synchronized
    private fun record(
        pid: TsPid,
        tableId: Int,
        status: Int,
    ) {
        val counter = counters.getOrPut(Triple(pid, tableId, status)) { MutableCounter() }
        when (statusBucketForTest(status)) {
            "accepted" -> {
                counter.accepted++
            }

            "invalid_section" -> {
                counter.invalidSection++
                counter.lastErrorTimeMillis = System.currentTimeMillis()
            }

            else -> {
                counter.malformed++
                counter.lastErrorTimeMillis = System.currentTimeMillis()
            }
        }
    }

    companion object {
        fun statusBucketForTest(status: Int): String =
            when (status) {
                SiStatus.OK -> "accepted"
                SiStatus.INVALID_SECTION -> "invalid_section"
                else -> "malformed"
            }
    }
}

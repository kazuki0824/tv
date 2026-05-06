package com.maleicacid.tvinput.aribsi

object WellKnownSectionPid {
    const val PAT = 0x0000
    const val CAT = 0x0001
    const val NIT = 0x0010
    const val SDT_BAT = 0x0011
    const val EIT = 0x0012
}

class SectionIngestController(private val engine: AribSiEngine) {
    fun onSection(pid: Int, section: ByteArray): SiIngestResult {
        require(pid in 0..0x1fff) { "PID が範囲外です: $pid" }
        return engine.ingestSection(pid, section)
    }
}

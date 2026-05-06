package com.maleicacid.tvinput.common

enum class FrontendProfile {
    ISDB_T,
    ISDB_S,
}

data class TuneRequest(
    val profile: FrontendProfile,
    val frequencyHz: Long,
    val streamSelector: StreamSelector = StreamSelector.NONE,
)

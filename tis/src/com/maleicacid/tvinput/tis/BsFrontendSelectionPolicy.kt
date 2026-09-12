package com.maleicacid.tvinput.tis

internal enum class BsCandidateSource {
    DYNAMIC_STREAM_ID_LIST,
    STATIC_TSID_TABLE,
}

internal data class BsFrontendCapability(
    val frontendId: Int,
    val isIsdbs: Boolean,
    val supportsStreamIdList: Boolean,
)

/** AOSP公開frontend能力だけでBS候補sourceの優先順位を決める。同一能力内ではframework列挙順を保持する。 */
internal object BsFrontendSelectionPolicy {
    fun orderedCandidates(frontends: Collection<BsFrontendCapability>): List<BsFrontendCapability> {
        val isdbs = frontends.filter { it.isIsdbs }
        return isdbs.filter { it.supportsStreamIdList } +
            isdbs.filterNot { it.supportsStreamIdList }
    }

    fun sourceFor(frontend: BsFrontendCapability): BsCandidateSource =
        if (frontend.supportsStreamIdList) {
            BsCandidateSource.DYNAMIC_STREAM_ID_LIST
        } else {
            BsCandidateSource.STATIC_TSID_TABLE
        }
}

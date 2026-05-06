package com.maleicacid.tvinput.common

data class ServiceKey(
    val originalNetworkId: Int,
    val transportStreamId: Int,
    val serviceId: Int,
)

enum class StreamSelectorType { NONE, TSID, RELATIVE }

data class StreamSelector(
    val type: StreamSelectorType = StreamSelectorType.NONE,
    val value: Int? = null,
) {
    init {
        when (type) {
            StreamSelectorType.NONE -> require(value == null) { "NONE selector は値を持てません" }
            StreamSelectorType.TSID -> require(value != null && value in 0..0xffff) { "TSID selector は 0..0xffff の値が必要です" }
            StreamSelectorType.RELATIVE -> require(value != null && value in 0..7) { "相対 TS 番号 selector は 0..7 の値が必要です" }
        }
    }

    companion object {
        val NONE = StreamSelector()
        fun tsid(value: Int): StreamSelector = StreamSelector(StreamSelectorType.TSID, value)
        fun relative(value: Int): StreamSelector = StreamSelector(StreamSelectorType.RELATIVE, value)
        fun fromStored(typeName: String?, valueText: String?): StreamSelector {
            val type = runCatching { StreamSelectorType.valueOf(typeName.orEmpty()) }.getOrNull() ?: StreamSelectorType.NONE
            val value = valueText?.toIntOrNull()
            return when (type) {
                StreamSelectorType.NONE -> NONE
                StreamSelectorType.TSID -> tsid(requireNotNull(value) { "TSID selector value がありません" })
                StreamSelectorType.RELATIVE -> relative(requireNotNull(value) { "相対 TS selector value がありません" })
            }
        }
    }
}

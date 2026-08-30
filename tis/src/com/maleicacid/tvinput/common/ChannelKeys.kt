package com.maleicacid.tvinput.common

private val UINT16_RANGE = 0x0000..0xffff
private val TS_PID_RANGE = 0x0000..0x1fff
private val TOKEN_LENGTH_RANGE = 1..16
private val RELATIVE_STREAM_RANGE = 0..7

@JvmInline
value class NetworkId16(val value: Int) {
    init { require(value in UINT16_RANGE) { "original_network_id は 0..0xffff でなければなりません: $value" } }
    companion object { fun fromOrNull(value: Int?): NetworkId16? = value?.takeIf { it in UINT16_RANGE }?.let(::NetworkId16) }
}

@JvmInline
value class TransportStreamId16(val value: Int) {
    init { require(value in UINT16_RANGE) { "transport_stream_id は 0..0xffff でなければなりません: $value" } }
    companion object { fun fromOrNull(value: Int?): TransportStreamId16? = value?.takeIf { it in UINT16_RANGE }?.let(::TransportStreamId16) }
}

@JvmInline
value class ServiceId16(val value: Int) {
    init { require(value in UINT16_RANGE) { "service_id は 0..0xffff でなければなりません: $value" } }
    companion object { fun fromOrNull(value: Int?): ServiceId16? = value?.takeIf { it in UINT16_RANGE }?.let(::ServiceId16) }
}

data class ServiceKey private constructor(
    val originalNetwork: NetworkId16,
    val transportStream: TransportStreamId16,
    val service: ServiceId16,
) {
    val originalNetworkId: Int get() = originalNetwork.value
    val transportStreamId: Int get() = transportStream.value
    val serviceId: Int get() = service.value

    override fun toString(): String = "ServiceKey(originalNetworkId=$originalNetworkId, transportStreamId=$transportStreamId, serviceId=$serviceId)"

    companion object {
        operator fun invoke(originalNetworkId: Int, transportStreamId: Int, serviceId: Int): ServiceKey = ServiceKey(
            NetworkId16(originalNetworkId),
            TransportStreamId16(transportStreamId),
            ServiceId16(serviceId),
        )

        fun fromOrNull(originalNetworkId: Int?, transportStreamId: Int?, serviceId: Int?): ServiceKey? {
            val onid = NetworkId16.fromOrNull(originalNetworkId) ?: return null
            val tsid = TransportStreamId16.fromOrNull(transportStreamId) ?: return null
            val sid = ServiceId16.fromOrNull(serviceId) ?: return null
            return ServiceKey(onid, tsid, sid)
        }
    }
}

@JvmInline
value class TsPid(val value: Int) : Comparable<TsPid> {
    init { require(value in TS_PID_RANGE) { "TS PID は 0x0000..0x1fff でなければなりません: $value" } }
    override fun compareTo(other: TsPid): Int = value.compareTo(other.value)
    override fun toString(): String = value.toString()

    companion object {
        val PAT = TsPid(0x0000)
        val CAT = TsPid(0x0001)
        val NIT = TsPid(0x0010)
        val SDT_BAT = TsPid(0x0011)
        val EIT = TsPid(0x0012)
        fun fromOrNull(value: Int?): TsPid? = value?.takeIf { it in TS_PID_RANGE }?.let(::TsPid)
    }
}

class TunerKeyToken private constructor(private val bytes: ByteArray) {
    init {
        require(bytes.size in TOKEN_LENGTH_RANGE) { "Tuner key token は 1..16 byte でなければなりません: ${bytes.size}" }
        require(!(bytes.size == 1 && bytes[0] == 0.toByte())) { "Tuner key token に VOID [0x00] は使用できません" }
    }
    fun toByteArray(): ByteArray = bytes.copyOf()
    val size: Int get() = bytes.size

    override fun equals(other: Any?): Boolean = other is TunerKeyToken && bytes.contentEquals(other.bytes)
    override fun hashCode(): Int = bytes.contentHashCode()
    override fun toString(): String = "TunerKeyToken(size=$size)"

    companion object {
        operator fun invoke(raw: ByteArray): TunerKeyToken = TunerKeyToken(raw.copyOf())
        fun fromOrNull(raw: ByteArray?): TunerKeyToken? = raw?.takeIf { it.size in TOKEN_LENGTH_RANGE && !(it.size == 1 && it[0] == 0.toByte()) }?.let(::TunerKeyToken)
    }
}

@JvmInline
value class FrequencyHz(val value: Long) {
    init { require(value > 0L) { "frequencyHz は正数でなければなりません: $value" } }
    companion object { fun fromOrNull(value: Long?): FrequencyHz? = value?.takeIf { it > 0L }?.let(::FrequencyHz) }
}

@JvmInline
value class RelativeStreamNumber(val value: Int) {
    init { require(value in RELATIVE_STREAM_RANGE) { "相対ストリーム番号は 0..7 でなければなりません: $value" } }
    companion object { fun fromOrNull(value: Int?): RelativeStreamNumber? = value?.takeIf { it in RELATIVE_STREAM_RANGE }?.let(::RelativeStreamNumber) }
}

enum class StreamSelectorType { NONE, TSID, RELATIVE }

sealed class StreamSelector {
    abstract val type: StreamSelectorType
    abstract val value: Int?

    object None : StreamSelector() {
        override val type: StreamSelectorType = StreamSelectorType.NONE
        override val value: Int? = null
    }

    data class Tsid(val tsid: TransportStreamId16) : StreamSelector() {
        override val type: StreamSelectorType = StreamSelectorType.TSID
        override val value: Int = tsid.value
    }

    data class Relative(val number: RelativeStreamNumber) : StreamSelector() {
        override val type: StreamSelectorType = StreamSelectorType.RELATIVE
        override val value: Int = number.value
    }

    companion object {
        val NONE: StreamSelector = None
        fun tsid(value: Int): StreamSelector = Tsid(TransportStreamId16(value))
        fun relative(value: Int): StreamSelector = Relative(RelativeStreamNumber(value))
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

@JvmInline
value class PesPts90k(val value: Long) {
    init { require(value >= 0L) { "PES PTS は 0 以上でなければなりません: $value" } }
    fun toCaptionPtsMillis(): CaptionPtsMillis = CaptionPtsMillis(value / 90L)
    companion object { fun fromOrNull(value: Long?): PesPts90k? = value?.takeIf { it >= 0L }?.let(::PesPts90k) }
}

@JvmInline
value class CaptionPtsMillis(val value: Long) {
    init { require(value >= 0L) { "字幕 PTS millisecond は 0 以上でなければなりません: $value" } }
}

sealed class CaptionTimestamp {
    data class Pts(val ptsMillis: CaptionPtsMillis) : CaptionTimestamp()
    object NoPts : CaptionTimestamp()
}

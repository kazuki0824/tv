package com.maleicacid.tvinput.tis

import com.maleicacid.tvinput.common.FrequencyHz
import com.maleicacid.tvinput.common.StreamSelector
import com.maleicacid.tvinput.common.TransportStreamId16
import com.maleicacid.tvinput.common.StreamSelectorType
import com.maleicacid.tvinput.db.ChannelRecord

enum class ScanCandidateKind { ISDB_T_UHF, ISDB_T_CATV, ISDB_S_BS, ISDB_S_110CS }

data class ScanCandidate(
    val deliverySystem: String,
    val frequencyHz: FrequencyHz,
    val streamSelector: StreamSelector = StreamSelector.NONE,
    val displayChannel: String,
    val physicalChannel: Int? = null,
    val backendHint: String? = null,
    val satelliteBand: String? = null,
    val kind: ScanCandidateKind = when (deliverySystem) {
        ChannelRecord.DELIVERY_SYSTEM_ISDB_T -> ScanCandidateKind.ISDB_T_UHF
        else -> if (satelliteBand == "110CS") ScanCandidateKind.ISDB_S_110CS else ScanCandidateKind.ISDB_S_BS
    },
) {
    init {
        require(deliverySystem == ChannelRecord.DELIVERY_SYSTEM_ISDB_T || deliverySystem == ChannelRecord.DELIVERY_SYSTEM_ISDB_S) { "対象外 deliverySystem=$deliverySystem" }
        if (deliverySystem == ChannelRecord.DELIVERY_SYSTEM_ISDB_T) require(streamSelector.type == StreamSelectorType.NONE) { "ISDB-T は stream selector を持てません" }
        if (kind == ScanCandidateKind.ISDB_S_110CS) require(streamSelector.type == StreamSelectorType.NONE) { "CS110 は TSID/relative stream selector による frontend 選局を行いません" }
        if (kind == ScanCandidateKind.ISDB_S_BS) {
            val discoverySeed = backendHint == JapanIsdbScanPlan.BS_DISCOVERY_BACKEND_HINT && streamSelector.type == StreamSelectorType.NONE
            val explicitTune = streamSelector.type == StreamSelectorType.TSID || (backendHint == "px4" && streamSelector.type == StreamSelectorType.RELATIVE)
            require(discoverySeed || explicitTune) { "BS はscan discovery seed(NONE)または明示TSIDを使用します" }
        }
    }
}

object JapanIsdbScanPlan {
    const val BS_DISCOVERY_BACKEND_HINT = "jp-bs-discovery"
    private data class BsTsidEntry(val frequencyHz: FrequencyHz, val tsid: TransportStreamId16, val label: String, val physical: Int)

    fun isdbtUhf13To62(): List<ScanCandidate> = (13..62).map { ch ->
        ScanCandidate(ChannelRecord.DELIVERY_SYSTEM_ISDB_T, FrequencyHz(473_142_857L + (ch - 13) * 6_000_000L), displayChannel = ch.toString(), physicalChannel = ch, backendHint = "jp-uhf", kind = ScanCandidateKind.ISDB_T_UHF)
    }

    /**
     * 日本CATV C13〜C63をTIS側SSOTとして固定する。
     * VHF 1〜12chは開発規則により恒久スコープ外であり、この候補表には含めない。
     */
    fun isdbtCatvC13ToC63(): List<ScanCandidate> {
        val mid = (13..22).map { ch ->
            val frequency = if (ch == 22) FrequencyHz(167_142_857L) else FrequencyHz(111_142_857L + (ch - 13) * 6_000_000L)
            ScanCandidate(ChannelRecord.DELIVERY_SYSTEM_ISDB_T, frequency, displayChannel = "C$ch", physicalChannel = ch, backendHint = "jp-catv", kind = ScanCandidateKind.ISDB_T_CATV)
        }
        val shb = (23..63).map { ch ->
            ScanCandidate(ChannelRecord.DELIVERY_SYSTEM_ISDB_T, FrequencyHz(225_142_857L + (ch - 23) * 6_000_000L), displayChannel = "C$ch", physicalChannel = ch, backendHint = "jp-catv", kind = ScanCandidateKind.ISDB_T_CATV)
        }
        return mid + shb
    }

    /** AOSP frontend scan用。TSIDを事前決め打ちせず、BS物理RFだけを列挙する。 */
    fun isdbsBsBands(): List<ScanCandidate> = bsTsidEntries
        .distinctBy { entry -> entry.frequencyHz.value to entry.physical }
        .map { entry ->
            ScanCandidate(
                ChannelRecord.DELIVERY_SYSTEM_ISDB_S,
                entry.frequencyHz,
                streamSelector = StreamSelector.NONE,
                displayChannel = "BS${entry.physical.toString().padStart(2, '0')}",
                physicalChannel = entry.physical,
                backendHint = BS_DISCOVERY_BACKEND_HINT,
                satelliteBand = "BS",
                kind = ScanCandidateKind.ISDB_S_BS,
            )
        }

    /** scan callbackがstream IDを返せないfrontend向けcompatibility fallback。 */
    fun isdbsBsTsidStreams(backendHint: String = "earth_pt1"): List<ScanCandidate> = bsTsidEntries.map { entry ->
        ScanCandidate(ChannelRecord.DELIVERY_SYSTEM_ISDB_S, entry.frequencyHz, streamSelector = StreamSelector.Tsid(entry.tsid), displayChannel = entry.label, physicalChannel = entry.physical, backendHint = backendHint, satelliteBand = "BS", kind = ScanCandidateKind.ISDB_S_BS)
    }

    fun explicitBsCandidatesFromScan(seed: ScanCandidate, inputStreamIds: Collection<Int>): List<ScanCandidate> {
        require(seed.kind == ScanCandidateKind.ISDB_S_BS && seed.streamSelector.type == StreamSelectorType.NONE)
        return inputStreamIds
            .asSequence()
            .filter { it in 0..0xfffe }
            .distinct()
            .sorted()
            .map { tsid ->
                ScanCandidate(
                    deliverySystem = ChannelRecord.DELIVERY_SYSTEM_ISDB_S,
                    frequencyHz = seed.frequencyHz,
                    streamSelector = StreamSelector.tsid(tsid),
                    displayChannel = "${seed.displayChannel}-$tsid",
                    physicalChannel = seed.physicalChannel,
                    backendHint = "aosp-scan",
                    satelliteBand = "BS",
                    kind = ScanCandidateKind.ISDB_S_BS,
                )
            }
            .toList()
    }

    fun fallbackBsCandidates(seed: ScanCandidate): List<ScanCandidate> = bsTsidEntries
        .filter { it.frequencyHz == seed.frequencyHz && it.physical == seed.physicalChannel }
        .map { entry ->
            ScanCandidate(ChannelRecord.DELIVERY_SYSTEM_ISDB_S, entry.frequencyHz, StreamSelector.Tsid(entry.tsid), entry.label, entry.physical, "bs-tsid-fallback", "BS", ScanCandidateKind.ISDB_S_BS)
        }

    fun isdbs110CsBands(): List<ScanCandidate> {
        val baseIf = (0 until 12).map { index -> FrequencyHz(1_613_000_000L + index * 40_000_000L) }
        return baseIf.mapIndexed { index, frequency ->
            ScanCandidate(ChannelRecord.DELIVERY_SYSTEM_ISDB_S, frequency, displayChannel = "CS${index + 1}", physicalChannel = index + 13, backendHint = "jp-110cs-band", satelliteBand = "110CS", kind = ScanCandidateKind.ISDB_S_110CS)
        }
    }

    fun isdbs110CsServiceIdentityCandidate(frequencyHz: FrequencyHz, tsid: TransportStreamId16, label: String, physical: Int): ScanCandidate {
        // CS110 では TSID を frontend selector へ渡さず、service identity 候補の値域検証だけをここで完了する。
        tsid.value
        return ScanCandidate(ChannelRecord.DELIVERY_SYSTEM_ISDB_S, frequencyHz, displayChannel = label, physicalChannel = physical, backendHint = "jp-110cs-band", satelliteBand = "110CS", kind = ScanCandidateKind.ISDB_S_110CS)
    }

    fun defaultInitialScan(): List<ScanCandidate> = isdbtUhf13To62() + isdbtCatvC13ToC63() + isdbsBsBands() + isdbs110CsBands()

    private val bsTsidEntries = listOf(
        BsTsidEntry(FrequencyHz(1_049_480_000L), TransportStreamId16(16400), "BS01-16400", 1), BsTsidEntry(FrequencyHz(1_049_480_000L), TransportStreamId16(16401), "BS01-16401", 1), BsTsidEntry(FrequencyHz(1_049_480_000L), TransportStreamId16(16402), "BS01-16402", 1),
        BsTsidEntry(FrequencyHz(1_087_840_000L), TransportStreamId16(16432), "BS03-16432", 3), BsTsidEntry(FrequencyHz(1_087_840_000L), TransportStreamId16(17969), "BS03-17969", 3), BsTsidEntry(FrequencyHz(1_087_840_000L), TransportStreamId16(17970), "BS03-17970", 3),
        BsTsidEntry(FrequencyHz(1_126_200_000L), TransportStreamId16(17488), "BS05-17488", 5), BsTsidEntry(FrequencyHz(1_126_200_000L), TransportStreamId16(17489), "BS05-17489", 5),
        BsTsidEntry(FrequencyHz(1_202_920_000L), TransportStreamId16(16528), "BS09-16528", 9), BsTsidEntry(FrequencyHz(1_202_920_000L), TransportStreamId16(16530), "BS09-16530", 9),
        BsTsidEntry(FrequencyHz(1_279_640_000L), TransportStreamId16(16592), "BS13-16592", 13), BsTsidEntry(FrequencyHz(1_279_640_000L), TransportStreamId16(16593), "BS13-16593", 13), BsTsidEntry(FrequencyHz(1_279_640_000L), TransportStreamId16(18130), "BS13-18130", 13),
        BsTsidEntry(FrequencyHz(1_318_000_000L), TransportStreamId16(16625), "BS15-16625", 15), BsTsidEntry(FrequencyHz(1_318_000_000L), TransportStreamId16(16626), "BS15-16626", 15), BsTsidEntry(FrequencyHz(1_318_000_000L), TransportStreamId16(18675), "BS15-18675", 15),
        BsTsidEntry(FrequencyHz(1_394_720_000L), TransportStreamId16(18224), "BS19-18224", 19), BsTsidEntry(FrequencyHz(1_394_720_000L), TransportStreamId16(18225), "BS19-18225", 19), BsTsidEntry(FrequencyHz(1_394_720_000L), TransportStreamId16(18226), "BS19-18226", 19), BsTsidEntry(FrequencyHz(1_394_720_000L), TransportStreamId16(18227), "BS19-18227", 19),
        BsTsidEntry(FrequencyHz(1_433_080_000L), TransportStreamId16(18256), "BS21-18256", 21), BsTsidEntry(FrequencyHz(1_433_080_000L), TransportStreamId16(18257), "BS21-18257", 21), BsTsidEntry(FrequencyHz(1_433_080_000L), TransportStreamId16(18258), "BS21-18258", 21),
        BsTsidEntry(FrequencyHz(1_471_440_000L), TransportStreamId16(18288), "BS23-18288", 23), BsTsidEntry(FrequencyHz(1_471_440_000L), TransportStreamId16(18801), "BS23-18801", 23), BsTsidEntry(FrequencyHz(1_471_440_000L), TransportStreamId16(18803), "BS23-18803", 23),
    )
}

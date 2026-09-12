@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import com.maleicacid.tvinput.common.FrequencyHz
import com.maleicacid.tvinput.common.StreamSelector
import com.maleicacid.tvinput.common.StreamSelectorType
import com.maleicacid.tvinput.common.TransportStreamId16
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
    val kind: ScanCandidateKind =
        when (deliverySystem) {
            ChannelRecord.DELIVERY_SYSTEM_ISDB_T -> ScanCandidateKind.ISDB_T_UHF
            else ->
                if (satelliteBand == "110CS") {
                    ScanCandidateKind.ISDB_S_110CS
                } else {
                    ScanCandidateKind.ISDB_S_BS
                }
        },
) {
    init {
        require(
            deliverySystem == ChannelRecord.DELIVERY_SYSTEM_ISDB_T ||
                deliverySystem == ChannelRecord.DELIVERY_SYSTEM_ISDB_S,
        ) {
            "対象外 deliverySystem=$deliverySystem"
        }
        if (deliverySystem == ChannelRecord.DELIVERY_SYSTEM_ISDB_T) {
            require(streamSelector.type == StreamSelectorType.NONE) {
                "ISDB-T は stream selector を持てません"
            }
        }
        if (kind == ScanCandidateKind.ISDB_S_110CS) {
            require(streamSelector.type == StreamSelectorType.NONE) {
                "CS110 は TSID/relative stream selector による frontend 選局を行いません"
            }
        }
        if (kind == ScanCandidateKind.ISDB_S_BS) {
            val discoverySeed =
                backendHint == JapanIsdbScanPlan.BS_DISCOVERY_BACKEND_HINT &&
                    streamSelector.type == StreamSelectorType.NONE
            val explicitTune = streamSelector.type == StreamSelectorType.TSID
            require(discoverySeed || explicitTune) {
                "BS はscan discovery seed(NONE)または明示TSIDを使用します"
            }
        }
    }
}

internal data class ScanTuneKey(
    val deliverySystem: String,
    val frequencyHz: FrequencyHz,
    val streamSelector: StreamSelector,
    val satelliteBand: String?,
)

internal val ScanCandidate.tuneKey: ScanTuneKey
    get() = ScanTuneKey(deliverySystem, frequencyHz, streamSelector, satelliteBand)

object JapanIsdbScanPlan {
    const val BS_DISCOVERY_BACKEND_HINT = "jp-bs-discovery"
    private const val BS_FIRST_IF_HZ = 1_049_480_000L
    private const val BS_TRANSPONDER_STEP_HZ = 38_360_000L

    private data class BsTsidEntry(
        val physicalChannel: Int,
        val frequencyHz: Long,
        val tsid: Int,
    )

    /*
     * STREAM_ID_LISTを公開しないfrontendでBSをexplicit tuneするための製品候補表。
     * この表は選局候補だけを所有し、受信後のONID/TSID/SIDのauthorityにはしない。
     */
    private val bsStaticTsidEntries =
        listOf(
            BsTsidEntry(1, 1_049_480_000L, 16400),
            BsTsidEntry(1, 1_049_480_000L, 16401),
            BsTsidEntry(1, 1_049_480_000L, 16402),
            BsTsidEntry(3, 1_087_840_000L, 16432),
            BsTsidEntry(3, 1_087_840_000L, 17969),
            BsTsidEntry(3, 1_087_840_000L, 17970),
            BsTsidEntry(5, 1_126_200_000L, 17488),
            BsTsidEntry(5, 1_126_200_000L, 17489),
            BsTsidEntry(9, 1_202_920_000L, 16528),
            BsTsidEntry(9, 1_202_920_000L, 16530),
            BsTsidEntry(13, 1_279_640_000L, 16592),
            BsTsidEntry(13, 1_279_640_000L, 16593),
            BsTsidEntry(13, 1_279_640_000L, 18130),
            BsTsidEntry(15, 1_318_000_000L, 16625),
            BsTsidEntry(15, 1_318_000_000L, 16626),
            BsTsidEntry(15, 1_318_000_000L, 18675),
            BsTsidEntry(19, 1_394_720_000L, 18224),
            BsTsidEntry(19, 1_394_720_000L, 18225),
            BsTsidEntry(19, 1_394_720_000L, 18226),
            BsTsidEntry(19, 1_394_720_000L, 18227),
            BsTsidEntry(21, 1_433_080_000L, 18256),
            BsTsidEntry(21, 1_433_080_000L, 18257),
            BsTsidEntry(21, 1_433_080_000L, 18258),
            BsTsidEntry(23, 1_471_440_000L, 18288),
            BsTsidEntry(23, 1_471_440_000L, 18801),
            BsTsidEntry(23, 1_471_440_000L, 18803),
        )

    fun isdbtUhf13To62(): List<ScanCandidate> =
        (13..62).map { ch ->
            ScanCandidate(
                ChannelRecord.DELIVERY_SYSTEM_ISDB_T,
                FrequencyHz(473_142_857L + (ch - 13) * 6_000_000L),
                displayChannel = ch.toString(),
                physicalChannel = ch,
                backendHint = "jp-uhf",
                kind = ScanCandidateKind.ISDB_T_UHF,
            )
        }

    /**
     * 日本CATV C13〜C63をTIS側SSOTとして固定する。
     * VHF 1〜12chは開発規則により恒久スコープ外であり、この候補表には含めない。
     */
    fun isdbtCatvC13ToC63(): List<ScanCandidate> {
        val mid =
            (13..22).map { ch ->
                val frequency =
                    if (ch == 22) {
                        FrequencyHz(167_142_857L)
                    } else {
                        FrequencyHz(111_142_857L + (ch - 13) * 6_000_000L)
                    }
                ScanCandidate(
                    ChannelRecord.DELIVERY_SYSTEM_ISDB_T,
                    frequency,
                    displayChannel = "C$ch",
                    physicalChannel = ch,
                    backendHint = "jp-catv",
                    kind = ScanCandidateKind.ISDB_T_CATV,
                )
            }
        val shb =
            (23..63).map { ch ->
                ScanCandidate(
                    ChannelRecord.DELIVERY_SYSTEM_ISDB_T,
                    FrequencyHz(225_142_857L + (ch - 23) * 6_000_000L),
                    displayChannel = "C$ch",
                    physicalChannel = ch,
                    backendHint = "jp-catv",
                    kind = ScanCandidateKind.ISDB_T_CATV,
                )
            }
        return mid + shb
    }

    /** AOSP frontend scan用。TSIDを事前決め打ちせず、BS物理RFだけを列挙する。 */
    fun isdbsBsBands(): List<ScanCandidate> =
        (0 until 12).map { index ->
            val physical = index * 2 + 1
            ScanCandidate(
                ChannelRecord.DELIVERY_SYSTEM_ISDB_S,
                FrequencyHz(BS_FIRST_IF_HZ + index * BS_TRANSPONDER_STEP_HZ),
                streamSelector = StreamSelector.NONE,
                displayChannel = "BS${physical.toString().padStart(2, '0')}",
                physicalChannel = physical,
                backendHint = BS_DISCOVERY_BACKEND_HINT,
                satelliteBand = "BS",
                kind = ScanCandidateKind.ISDB_S_BS,
            )
        }

    fun staticBsStreamIdsFor(seed: ScanCandidate): Set<Int> {
        require(
            seed.kind == ScanCandidateKind.ISDB_S_BS &&
                seed.streamSelector.type == StreamSelectorType.NONE,
        )
        return bsStaticTsidEntries
            .asSequence()
            .filter { entry ->
                entry.frequencyHz == seed.frequencyHz.value &&
                    entry.physicalChannel == seed.physicalChannel
            }.map { it.tsid }
            .toCollection(linkedSetOf())
    }

    fun staticBsCandidatesFor(seed: ScanCandidate): List<ScanCandidate> =
        explicitBsCandidatesFromScan(seed, staticBsStreamIdsFor(seed))

    fun explicitBsCandidatesFromScan(
        seed: ScanCandidate,
        inputStreamIds: Collection<Int>,
    ): List<ScanCandidate> {
        require(
            seed.kind == ScanCandidateKind.ISDB_S_BS &&
                seed.streamSelector.type == StreamSelectorType.NONE,
        )
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
            }.toList()
    }

    fun isdbs110CsBands(): List<ScanCandidate> {
        val baseIf =
            (0 until 12).map { index ->
                FrequencyHz(1_613_000_000L + index * 40_000_000L)
            }
        return baseIf.mapIndexed { index, frequency ->
            ScanCandidate(
                ChannelRecord.DELIVERY_SYSTEM_ISDB_S,
                frequency,
                displayChannel = "CS${index + 1}",
                physicalChannel = index + 13,
                backendHint = "jp-110cs-band",
                satelliteBand = "110CS",
                kind = ScanCandidateKind.ISDB_S_110CS,
            )
        }
    }

    fun isdbs110CsServiceIdentityCandidate(
        frequencyHz: FrequencyHz,
        tsid: TransportStreamId16,
        label: String,
        physical: Int,
    ): ScanCandidate {
        // CS110 では TSID を frontend selector へ渡さず、service identity 候補の値域検証だけをここで完了する。
        tsid.value
        return ScanCandidate(
            ChannelRecord.DELIVERY_SYSTEM_ISDB_S,
            frequencyHz,
            displayChannel = label,
            physicalChannel = physical,
            backendHint = "jp-110cs-band",
            satelliteBand = "110CS",
            kind = ScanCandidateKind.ISDB_S_110CS,
        )
    }

    fun defaultInitialScan(): List<ScanCandidate> =
        isdbtUhf13To62() + isdbtCatvC13ToC63() + isdbsBsBands() + isdbs110CsBands()
}

package com.maleicacid.tvinput.tis

import com.maleicacid.tvinput.common.FrequencyHz
import com.maleicacid.tvinput.common.StreamSelector
import com.maleicacid.tvinput.common.StreamSelectorType
import com.maleicacid.tvinput.common.TransportStreamId16
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class ScanPlanPolicyTest {

    @Test
    fun catvScanC13ToC63IsTisSideSsotAndDoesNotIncludeVhf() {
        val catv = JapanIsdbScanPlan.isdbtCatvC13ToC63()
        assertEquals(51, catv.size)
        assertEquals("C13", catv.first().displayChannel)
        assertEquals(111_142_857L, catv.first().frequencyHz.value)
        assertEquals("C22", catv[9].displayChannel)
        assertEquals(167_142_857L, catv[9].frequencyHz.value)
        assertEquals("C23", catv[10].displayChannel)
        assertEquals(225_142_857L, catv[10].frequencyHz.value)
        assertEquals("C63", catv.last().displayChannel)
        assertEquals(465_142_857L, catv.last().frequencyHz.value)
        assertTrue(catv.all { it.kind == ScanCandidateKind.ISDB_T_CATV })
        assertTrue(catv.all { it.displayChannel.startsWith("C") })
        assertFalse(catv.any { it.displayChannel in (1..12).map(Int::toString) })
    }

    @Test
    fun defaultScanIncludesCatvAndUsesRfDiscoverySeedsForBs() {
        val scan = JapanIsdbScanPlan.defaultInitialScan()
        assertTrue(scan.any { it.kind == ScanCandidateKind.ISDB_T_CATV && it.displayChannel == "C13" })
        val bs = scan.filter { it.kind == ScanCandidateKind.ISDB_S_BS }
        assertTrue(bs.isNotEmpty())
        assertTrue(bs.all { it.streamSelector.type == StreamSelectorType.NONE })
        assertTrue(bs.all { it.backendHint == JapanIsdbScanPlan.BS_DISCOVERY_BACKEND_HINT })
    }

    @Test
    fun versionedBsCandidatesAreExplicitTsidsForOneUnsupportedRfSeed() {
        val seed = JapanIsdbScanPlan.isdbsBsBands().first()
        val candidates = JapanIsdbScanPlan.versionedBsCandidatesForUnsupportedDynamicDiscovery(seed)
        assertTrue(candidates.isNotEmpty())
        assertTrue(candidates.all { it.frequencyHz == seed.frequencyHz })
        assertTrue(candidates.all { it.physicalChannel == seed.physicalChannel })
        assertTrue(candidates.all { it.streamSelector.type == StreamSelectorType.TSID })
        assertEquals(setOf(16400, 16401, 16402), candidates.mapNotNull { it.streamSelector.value }.toSet())
    }

    @Test
    fun bsDynamicDiscoveryUsesOnlyReportedStreamIds() {
        val seed = JapanIsdbScanPlan.isdbsBsBands().first()
        val discovered = JapanIsdbScanPlan.explicitBsCandidatesFromScan(
            seed,
            listOf(18288, 18801, 18803, 18803, -1, 0xffff),
        )
        assertEquals(setOf(18288, 18801, 18803), discovered.mapNotNull { it.streamSelector.value }.toSet())
        assertTrue(discovered.all { it.streamSelector.type == StreamSelectorType.TSID })
    }

    @Test
    fun bsDynamicDiscoveryWithNoReportedStreamIdsIsEmpty() {
        val seed = JapanIsdbScanPlan.isdbsBsBands().first()
        assertTrue(JapanIsdbScanPlan.explicitBsCandidatesFromScan(seed, emptyList()).isEmpty())
    }

    @Test
    fun bsCandidateRejectsRelativeSelectorEvenWithPx4Hint() {
        val seed = JapanIsdbScanPlan.isdbsBsBands().first()
        assertFailsWith<IllegalArgumentException> {
            seed.copy(
                streamSelector = StreamSelector.relative(0),
                backendHint = "px4",
            )
        }
    }

    @Test
    fun cs110ScanBandsDoNotCarryFrontendStreamSelector() {
        val cs = JapanIsdbScanPlan.isdbs110CsBands()
        assertTrue(cs.isNotEmpty())
        assertTrue(cs.all { it.satelliteBand == "110CS" })
        assertTrue(cs.all { it.streamSelector.type == StreamSelectorType.NONE })
    }

    @Test
    fun cs110ServiceIdentityCandidateStillDoesNotCarryFrontendSelector() {
        val candidate = JapanIsdbScanPlan.isdbs110CsServiceIdentityCandidate(
            frequencyHz = FrequencyHz(1_613_000_000L),
            tsid = TransportStreamId16(0x6020),
            label = "CS-test",
            physical = 13,
        )
        assertEquals(StreamSelectorType.NONE, candidate.streamSelector.type)
        assertEquals("110CS", candidate.satelliteBand)
    }
}

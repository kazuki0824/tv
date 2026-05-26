package com.maleicacid.tvinput.tis

import com.maleicacid.tvinput.common.FrequencyHz
import com.maleicacid.tvinput.common.StreamSelectorType
import kotlin.test.Test
import kotlin.test.assertEquals
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
    fun defaultScanIncludesCatvAndUsesOnlyTsidCandidatesForBs() {
        val scan = JapanIsdbScanPlan.defaultInitialScan()
        assertTrue(scan.any { it.kind == ScanCandidateKind.ISDB_T_CATV && it.displayChannel == "C13" })
        assertTrue(scan.filter { it.kind == ScanCandidateKind.ISDB_S_BS }.all { it.streamSelector.type == StreamSelectorType.TSID })
    }
    @Test
    fun defaultScanKeepsBsTsidAsFirstClassSelector() {
        val bs = JapanIsdbScanPlan.isdbsBsTsidStreams()
        assertTrue(bs.isNotEmpty())
        assertTrue(bs.all { it.streamSelector.type == StreamSelectorType.TSID })
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
            tsid = 0x6020,
            label = "CS-test",
            physical = 13,
        )
        assertEquals(StreamSelectorType.NONE, candidate.streamSelector.type)
        assertEquals("110CS", candidate.satelliteBand)
    }
}

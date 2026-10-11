@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import com.maleicacid.tvinput.aribsi.WellKnownSectionPid
import com.maleicacid.tvinput.common.TsPid
import org.junit.Test

class ChannelScanSectionRefreshTest {
    @Test
    fun scanOnlyRefreshesDynamicPmtFiltersAfterPatAndNotAfterUnrelatedSi() {
        check(ChannelScanController.shouldRefreshPmtFiltersForSection(WellKnownSectionPid.PAT))
        listOf(
            WellKnownSectionPid.CAT,
            WellKnownSectionPid.NIT,
            WellKnownSectionPid.SDT_BAT,
            WellKnownSectionPid.EIT,
            WellKnownSectionPid.TDT,
            TsPid(0x100),
        ).forEach { pid -> check(!ChannelScanController.shouldRefreshPmtFiltersForSection(pid)) }
    }
}

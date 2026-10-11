// テストの入力・期待値を本体の定数と独立した具体値で記述する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import org.junit.Test

class TunerControllerSectionBoundsTest {
    @Test fun clockFiltersSelectDisjointTablesWithTheRequiredCrcPolicy() {
        val settings = TunerController.sectionSettingsForPid(com.maleicacid.tvinput.aribsi.WellKnownSectionPid.TDT)
        check(settings.size == 2)
        for (tableId in 0..255) {
            val selected =
                settings.filter { setting ->
                    val mask = setting.mask.single().toInt() and 0xff
                    check(setting.mode.contentEquals(byteArrayOf(0)))
                    (tableId and mask) == (setting.filterBytes.single().toInt() and mask)
                }
            when (tableId) {
                0x70 -> check(!selected.single().isCrcEnabled)
                0x73 -> check(selected.single().isCrcEnabled)
                else -> check(selected.isEmpty())
            }
        }
    }

    @Test fun ordinarySectionFiltersKeepCrcEnabled() {
        val setting =
            TunerController
                .sectionSettingsForPid(
                    com.maleicacid.tvinput.common
                        .TsPid(0),
                ).single()
        check(setting.isCrcEnabled)
        check(setting.isRepeat)
        check(!setting.isRaw)
        check(setting.lengthFieldBitWidth == 12)
    }

    @Test fun controllerSectionAdmissionBudgetTracksAdvertisedFilterCapacityWithHardBound() {
        check(TunerController.sectionDataSlotLimitForFilterCapacity(0) == 16)
        check(TunerController.sectionDataSlotLimitForFilterCapacity(1) == 16)
        check(TunerController.sectionDataSlotLimitForFilterCapacity(2) == 32)
        check(TunerController.sectionDataSlotLimitForFilterCapacity(16) == 256)
        check(TunerController.sectionDataSlotLimitForFilterCapacity(32) == 256)
        check(TunerController.sectionDataSlotLimitForFilterCapacity(Int.MAX_VALUE) == 256)
    }

    @Test fun sectionEventDataLengthDecisionIsFixedAt4096Bytes() {
        check(SectionFilterPolicy.dataLengthDecision(0) == SectionFilterPolicy.DataLengthDecision.MALFORMED)
        check(SectionFilterPolicy.dataLengthDecision(-1) == SectionFilterPolicy.DataLengthDecision.MALFORMED)
        check(SectionFilterPolicy.dataLengthDecision(1) == SectionFilterPolicy.DataLengthDecision.ACCEPT)
        check(SectionFilterPolicy.dataLengthDecision(4096) == SectionFilterPolicy.DataLengthDecision.ACCEPT)
        check(SectionFilterPolicy.dataLengthDecision(4097) == SectionFilterPolicy.DataLengthDecision.OVERSIZED)
    }
}

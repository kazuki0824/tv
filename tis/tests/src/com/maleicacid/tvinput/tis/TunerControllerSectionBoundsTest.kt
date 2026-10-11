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

    @Test fun liveAndSetupEitAcceptOnlyActualPfWithoutChangingEpgMaintenance() {
        val eit = com.maleicacid.tvinput.aribsi.WellKnownSectionPid.EIT
        val live = TunerController.sectionSettingsForPid(eit, eitPfActualOnly = true).single()
        val scan = TunerController.sectionSettingsForPid(eit).single()
        check(live.isRepeat && live.isCrcEnabled)
        check(scan.isRepeat && scan.isCrcEnabled)
        check(live.mask.size == 1 && live.mask.single().toInt() and 0xff == 0xff)
        val tableFilter = live.filterBytes.single().toInt() and 0xff
        val mask = live.mask.single().toInt() and 0xff
        for (tableId in 0x00..0xff) {
            val permitted = (tableId and mask) == (tableFilter and mask)
            check(permitted == (tableId == 0x4e))
        }
        // boot EPG同期・background maintenanceは従来どおりschedule等も収集する。
        check(scan.filterBytes.isEmpty() && scan.mask.isEmpty())
        val pat =
            TunerController
                .sectionSettingsForPid(com.maleicacid.tvinput.aribsi.WellKnownSectionPid.PAT, eitPfActualOnly = true)
                .single()
        check(pat.filterBytes.isEmpty())
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

    @Test fun sectionEventDataLengthDecisionIsFixedAt4096Bytes() {
        check(SectionFilterPolicy.dataLengthDecision(0) == SectionFilterPolicy.DataLengthDecision.MALFORMED)
        check(SectionFilterPolicy.dataLengthDecision(-1) == SectionFilterPolicy.DataLengthDecision.MALFORMED)
        check(SectionFilterPolicy.dataLengthDecision(1) == SectionFilterPolicy.DataLengthDecision.ACCEPT)
        check(SectionFilterPolicy.dataLengthDecision(4096) == SectionFilterPolicy.DataLengthDecision.ACCEPT)
        check(SectionFilterPolicy.dataLengthDecision(4097) == SectionFilterPolicy.DataLengthDecision.OVERSIZED)
    }
}

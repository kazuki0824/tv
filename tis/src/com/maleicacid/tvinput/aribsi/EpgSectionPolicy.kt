package com.maleicacid.tvinput.aribsi

/** 番組公開に使う表とsectionの選択。SI解析器の受理範囲は変更しない。 */
internal object EpgSectionPolicy {
    fun accepts(profile: Int, tableId: Int, sectionNumber: Int): Boolean =
        tableId == 0x4e && (profile == 0 || sectionNumber in 0..1)

    fun requiredLast(profile: Int, instance: EitInstanceState): Int =
        if (profile == SiDiscoveryProfile.ISDB_T) instance.lastSectionNumber else minOf(1, instance.lastSectionNumber)

    fun isComplete(profile: Int, instance: EitInstanceState): Boolean =
        instance.tableId == 0x4e && instance.currentNextIndicator && !instance.inconsistent &&
            (0..requiredLast(profile, instance)).all { it in instance.receivedSections }
}

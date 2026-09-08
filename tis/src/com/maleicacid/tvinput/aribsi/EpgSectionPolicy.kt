package com.maleicacid.tvinput.aribsi

/** 番組公開に使う表とsectionの選択。SI解析器の受理範囲は変更しない。 */
internal object EpgSectionPolicy {
    fun accepts(profile: Int, tableId: Int, sectionNumber: Int): Boolean =
        tableId == 0x4e && (profile == 0 || sectionNumber in 0..1)
}

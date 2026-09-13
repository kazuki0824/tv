package com.maleicacid.tvinput.aribsi

/** 番組公開に使う表とsectionの選択。SI解析器の受理範囲は変更しない。 */
internal object EpgSectionPolicy {
    // この処理の規格値・ビット幅・単位換算・固定上限をリテラルのまま照合できる形に保つ。
    @Suppress("MagicNumber")
    fun accepts(
        profile: Int,
        tableId: Int,
        sectionNumber: Int,
    ): Boolean = tableId == 0x4e && (profile == 0 || sectionNumber in 0..1)

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    fun requiredLast(
        profile: Int,
        instance: EitInstanceState,
    ): Int = if (profile == SiDiscoveryProfile.ISDB_T) instance.lastSectionNumber else minOf(1, instance.lastSectionNumber)

    // この処理の規格値・ビット幅・単位換算・固定上限をリテラルのまま照合できる形に保つ。
    @Suppress("MagicNumber")
    fun isComplete(
        profile: Int,
        instance: EitInstanceState,
    ): Boolean =
        instance.tableId == 0x4e && instance.currentNextIndicator && !instance.inconsistent &&
            (0..requiredLast(profile, instance)).all { it in instance.receivedSections }
}

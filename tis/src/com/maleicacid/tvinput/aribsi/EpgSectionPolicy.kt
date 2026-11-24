package com.maleicacid.tvinput.aribsi

/** 番組公開に使う表とsectionの選択。SI解析器の受理範囲は変更しない。 */
internal object EpgSectionPolicy {
    // この処理の規格値・ビット幅・単位換算・固定上限をリテラルのまま照合できる形に保つ。
    @Suppress("MagicNumber")
    fun accepts(
        profile: Int,
        tableId: Int,
        sectionNumber: Int,
    ): Boolean = (tableId in 0x4e..0x4f && (profile == 0 || sectionNumber in 0..1)) || isSchedule(tableId)

    @Suppress("MagicNumber")
    fun isSchedule(tableId: Int): Boolean = tableId in 0x50..0x6f

    const val PF_OTHER = 0x4f

    @Suppress("MagicNumber")
    fun isExtendedSchedule(tableId: Int): Boolean = tableId in 0x58..0x5f || tableId in 0x68..0x6f

    @Suppress("MagicNumber")
    fun coversOtherTransport(instances: List<EitInstanceState>): Boolean {
        val safe =
            instances.filter {
                it.currentNextIndicator && it.complete && !it.inconsistent &&
                    it.safeSections.containsAll(it.receivedSections)
            }
        return safe.any { it.tableId == PF_OTHER } && safe.any { it.tableId == 0x60 }
    }

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

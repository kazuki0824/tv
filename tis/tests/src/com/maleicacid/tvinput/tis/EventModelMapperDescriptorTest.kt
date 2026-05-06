package com.maleicacid.tvinput.tis

import com.maleicacid.tvinput.aribsi.AribEvent
import com.maleicacid.tvinput.aribsi.AribExtendedItem
import com.maleicacid.tvinput.aribsi.EventModelMapper
import com.maleicacid.tvinput.common.ServiceKey
import org.junit.Test

class EventModelMapperDescriptorTest {
    @Test fun descriptorDetailsArePreservedForTvProviderInternalData() {
        val event = AribEvent(
            serviceKey = ServiceKey(4, 16625, 101),
            stableIdentity = "onid=4;tsid=16625;sid=101;event=10",
            eventId = 10,
            startTimeMillis = 1_700_000_000_000L,
            durationMillis = 1_800_000L,
            title = "番組",
            description = "短い説明\n詳細説明",
            extendedItems = listOf(AribExtendedItem("出演", "A")),
            componentText = "映像",
            audioComponentText = "音声",
            audioLanguage = "jpn",
            canonicalGenre = "NEWS",
            genreSupplementText = "ニュース/報道(0/0)",
            eventGroupText = "sid=101 event=202",
            freeCaText = "無料放送",
            seriesName = "シリーズ",
            diagnosticText = "unknownCount=0",
            diagnosticDescriptorJson = "{\"extendedItems\":[]}",
        )
        val record = EventModelMapper().toProgramRecords(listOf(event)).single()
        check(record.shortDescription == "短い説明")
        check(record.description.contains("短い説明"))
        check(record.description.contains("詳細説明"))
        check(record.description.contains("【出演】A"))
        check(record.description.contains("映像: 映像"))
        check(record.description.contains("音声: 音声"))
        check(record.description.contains("ジャンル: ニュース/報道(0/0)"))
        check(record.description.contains("関連番組: sid=101 event=202"))
        check(record.description.contains("放送種別: 無料放送"))
        check(!record.description.contains("シリーズ: シリーズ"))
        check(record.extendedItemsJson.contains("出演"))
        check(record.componentText == "映像")
        check(record.audioComponentText == "音声")
        check(record.audioLanguage == "jpn")
        check(record.canonicalGenre == "NEWS")
        check(record.genreSupplementText == "ニュース/報道(0/0)")
        check(record.eventGroupText == "sid=101 event=202")
        check(record.freeCaText == "無料放送")
        check(record.seriesName == "シリーズ")
    }
}

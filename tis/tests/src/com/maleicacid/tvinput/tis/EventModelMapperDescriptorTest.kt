package com.maleicacid.tvinput.tis

import com.maleicacid.tvinput.aribsi.AribEvent
import com.maleicacid.tvinput.aribsi.AribEventDescriptors
import com.maleicacid.tvinput.aribsi.AribExtendedItem
import com.maleicacid.tvinput.aribsi.AribFreeCaMode
import com.maleicacid.tvinput.aribsi.AribRelatedItem
import com.maleicacid.tvinput.aribsi.AribSeries
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
            descriptors = AribEventDescriptors(
                extendedItems = listOf(AribExtendedItem("出演", "A")),
                componentText = "映像",
                audioComponentText = "音声",
                audioLanguage = "jpn",
                broadcastGenre = "ARIB(0x0/0x0):ニュース/報道/定時・総合",
                genreSupplementText = "ニュース/報道/定時・総合",
                relatedItems = listOf(AribRelatedItem("shared", 1, 4, 16625, 101, 202)),
                scrambled = false,
                freeCaMode = AribFreeCaMode(raw = 0, scrambled = false, text = "無料放送"),
                seriesId = 100,
                episodeNumber = 3,
                lastEpisodeNumber = 12,
                series = AribSeries(seriesId = 100, episodeNumber = 3, lastEpisodeNumber = 12, name = "シリーズ"),
            ),
        )
        val record = EventModelMapper().toProgramRecords(listOf(event)).single()
        check(record.shortDescription == "短い説明")
        check(record.description.contains("短い説明"))
        check(record.description.contains("詳細説明"))
        check(record.description.contains("【出演】A"))
        check(record.description.contains("映像: 映像"))
        check(record.description.contains("音声: 音声"))
        check(record.description.contains("ジャンル: ニュース/報道/定時・総合"))
        check(!record.description.contains("関連番組:"))
        check(record.description.contains("放送種別: 無料放送"))
        check(!record.description.contains("シリーズ: シリーズ"))
        check(record.descriptors.extendedItems.single().itemDescription == "出演")
        check(record.descriptors.componentText == "映像")
        check(record.descriptors.audioComponentText == "音声")
        check(record.descriptors.audioLanguage == "jpn")
        check(record.canonicalGenres == listOf("NEWS"))
        check(record.descriptors.broadcastGenre == "ARIB(0x0/0x0):ニュース/報道/定時・総合")
        check(record.descriptors.genreSupplementText == "ニュース/報道/定時・総合")
        check(record.descriptors.relatedItems.single().eventId == 202)
        check(record.descriptors.scrambled == false)
        check(record.descriptors.freeCaMode?.text == "無料放送")
        check(record.descriptors.seriesId == 100)
        check(record.descriptors.episodeNumber == 3)
        check(record.descriptors.lastEpisodeNumber == 12)
        check(record.descriptors.series?.name == "シリーズ")
    }
}

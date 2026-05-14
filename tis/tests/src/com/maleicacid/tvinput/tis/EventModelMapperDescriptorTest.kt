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
            broadcastGenre = "ARIB(0x0/0x0):ニュース/報道/定時・総合",
            genreSupplementText = "ニュース/報道/定時・総合",
            relatedItemsJson = """[{"kind":"shared","groupType":1,"originalNetworkId":4,"transportStreamId":16625,"serviceId":101,"eventId":202,"parseStatus":"OK"}]""",
            scrambled = false,
            freeCaModeJson = """{"raw":0,"scrambled":false,"text":"無料放送","parseStatus":"OK"}""",
            seriesId = 100,
            episodeNumber = 3,
            lastEpisodeNumber = 12,
            seriesJson = """{"seriesId":100,"repeatLabel":0,"programPattern":0,"expireDateValid":false,"expireDate":null,"episodeNumber":3,"lastEpisodeNumber":12,"name":"シリーズ","parseStatus":"OK"}""",
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
        check(record.description.contains("ジャンル: ニュース/報道/定時・総合"))
        check(!record.description.contains("関連番組:"))
        check(record.description.contains("放送種別: 無料放送"))
        check(!record.description.contains("シリーズ: シリーズ"))
        check(record.extendedItemsJson.contains("出演"))
        check(record.componentText == "映像")
        check(record.audioComponentText == "音声")
        check(record.audioLanguage == "jpn")
        check(record.canonicalGenres == listOf("NEWS"))
        check(record.broadcastGenre == "ARIB(0x0/0x0):ニュース/報道/定時・総合")
        check(record.genreSupplementText == "ニュース/報道/定時・総合")
        check(record.relatedItemsJson.contains("eventId"))
        check(record.scrambled == false)
        check(record.descriptors.freeCaModeJson.contains("無料放送"))
        check(record.seriesId == 100)
        check(record.episodeNumber == 3)
        check(record.lastEpisodeNumber == 12)
        check(record.descriptors.seriesJson.contains("シリーズ"))
    }
}

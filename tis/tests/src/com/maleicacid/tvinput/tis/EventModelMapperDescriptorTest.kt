package com.maleicacid.tvinput.tis

import com.maleicacid.tvinput.aribsi.AribComponentEntry
import com.maleicacid.tvinput.aribsi.AribComponents
import com.maleicacid.tvinput.aribsi.AribContentGenre
import com.maleicacid.tvinput.aribsi.AribEvent
import com.maleicacid.tvinput.aribsi.AribEventDescriptors
import com.maleicacid.tvinput.aribsi.AribExtendedItem
import com.maleicacid.tvinput.aribsi.AribShortEventText
import com.maleicacid.tvinput.aribsi.AribExtendedEventText
import com.maleicacid.tvinput.aribsi.AribFreeCaMode
import com.maleicacid.tvinput.aribsi.AribEventGroup
import com.maleicacid.tvinput.aribsi.AribEventGroupReference
import com.maleicacid.tvinput.aribsi.AribSeries
import com.maleicacid.tvinput.aribsi.EventModelMapper
import com.maleicacid.tvinput.common.ServiceId16
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.TsPid
import org.junit.Test

class EventModelMapperDescriptorTest {
    @Test fun descriptorDetailsArePreservedForTvProviderInternalData() {
        val event = AribEvent(
            serviceKey = ServiceKey(4, 16625, 101),
            stableIdentity = "{\"kind\":\"arib-event-v1\",\"originalNetworkId\":4,\"transportStreamId\":16625,\"serviceId\":101,\"eventId\":10}",
            eventId = 10,
            startTimeMillis = 1_700_000_000_000L,
            durationMillis = 1_800_000L,
            title = "番組",
            description = "短い説明",
            extendedDescription = "詳細説明",
            descriptors = AribEventDescriptors(
                shortEvents = listOf(
                    AribShortEventText("jpn", "番組", "短い説明"),
                    AribShortEventText("eng", "Program", "English short"),
                ),
                extendedTexts = listOf(
                    AribExtendedEventText("jpn", "詳細説明"),
                    AribExtendedEventText("eng", "English details"),
                ),
                extendedItems = listOf(
                    AribExtendedItem("jpn", "出演", "A"),
                    AribExtendedItem("eng", "Cast", "B"),
                ),
                componentText = "映像",
                audioComponentText = "音声",
                contentGenres = listOf(AribContentGenre(0x0, 0x0, aribName = "ニュース/報道/定時・総合")),
                broadcastGenre = "ARIB(0x0/0x0):ニュース/報道/定時・総合",
                genreSupplementText = "ニュース/報道/定時・総合",
                eventGroups = listOf(AribEventGroup(groupType = 1, events = listOf(AribEventGroupReference(ServiceId16(101), 202)))),
                scrambled = false,
                freeCaMode = AribFreeCaMode(raw = 0, scrambled = false),
                series = AribSeries(seriesId = 100, episodeNumber = 3, lastEpisodeNumber = 12, name = "シリーズ"),
                components = AribComponents(audio = listOf(AribComponentEntry(esPid = TsPid(256), streamType = 0x0f, componentTag = 1, componentType = 3, codec = "AAC", language = "jpn", parseStatus = "OK"))),
            ),
        )
        val record = EventModelMapper().toProgramRecords(listOf(event)).single()
        check(record.shortDescription == "短い説明")
        check(!record.description.contains("短い説明"))
        check(record.description.contains("詳細説明"))
        check(record.description.contains("【出演】A"))
        check(!record.description.contains("English details"))
        check(!record.description.contains("【Cast】B"))
        check(record.descriptors.shortEvents.size == 2)
        check(record.descriptors.extendedTexts.size == 2)
        check(record.description.contains("映像: 映像"))
        check(record.description.contains("音声: 音声"))
        check(record.description.contains("ジャンル: ニュース/報道/定時・総合"))
        check(!record.description.contains("関連番組:"))
        check(record.description.contains("放送種別: 無料放送"))
        check(!record.description.contains("シリーズ: シリーズ"))
        check(record.descriptors.extendedItems.size == 2)
        check(record.descriptors.extendedItems.first { it.languageCode == "jpn" }.itemDescription == "出演")
        check(record.descriptors.extendedItems.first { it.languageCode == "eng" }.itemDescription == "Cast")
        check(record.descriptors.componentText == "映像")
        check(record.descriptors.audioComponentText == "音声")
        check(record.descriptors.components.audio.single().language == "jpn")
        check(record.canonicalGenres == listOf("NEWS"))
        check(record.descriptors.broadcastGenre == "ARIB(0x0/0x0):ニュース/報道/定時・総合")
        check(record.descriptors.genreSupplementText == "ニュース/報道/定時・総合")
        check(record.descriptors.eventGroups.single().groupType == 1)
        check(record.descriptors.eventGroups.single().events.single().eventId == 202)
        check(record.descriptors.eventGroups.single().otherNetworkEvents.isEmpty())
        check(record.descriptors.scrambled == false)
        check(record.descriptors.freeCaMode?.raw == 0)
        check(record.descriptors.freeCaMode?.scrambled == false)
        check(record.descriptors.series?.seriesId == 100)
        check(record.descriptors.series?.episodeNumber == 3)
        check(record.descriptors.series?.lastEpisodeNumber == 12)
        check(record.descriptors.series?.name == "シリーズ")
    }
}

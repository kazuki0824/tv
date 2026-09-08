package com.maleicacid.tvinput.aribsi

import com.maleicacid.tvinput.common.TsPid
import org.json.JSONObject
import org.junit.Test

class NativeAribSiParserCasDiscoveryTest {
    @Test fun codecDescriptorsSurviveNormalSnapshotAndProviderProjection() {
        NativeAribSiParser().use { parser ->
            val body = mutableListOf(
                0x02, 0xb0, 0, 0, 1, 0xc1, 0, 0, 0xe1, 1, 0xf0, 0,
                0x1b, 0xe1, 1, 0xf0, 6, 0x28, 4, 100, 0, 40, 0x3f,
                0x1c, 0xe1, 2, 0xf0, 7, 0x1c, 1, 0xff, 0x2e, 2, 0x71, 0x5a,
                0x0f, 0xe1, 3, 0xf0, 6, 0x2e, 4, 0xf0, 2, 0x11, 0x90,
            )
            setSectionLength(body, 0xb0)
            check(parser.ingestSection(TsPid(PID_PAT), section(PAT_BODY)) == SiStatus.OK)
            check(parser.ingestSection(TsPid(PID_SDT), section(SDT_SCRAMBLED_SERVICE_BODY)) == SiStatus.OK)
            check(parser.ingestSection(TsPid(PID_PMT), section(body.toIntArray())) == SiStatus.OK)
            val service = parser.livePlaybackSnapshot().services.single()
            val avc = service.streams.first()
            check(avc.codecFacts.avc == AribAvcSignaling(100, 0, 40))
            check(service.streams[1].codec == "MPEG-4-ALS")
            val aac = service.streams[2]
            check(aac.codec == "AAC" && aac.codecFacts.audioConfigHex == "1190")
            check(aac.codecFacts.audioConfigHeader?.samplingFrequency == 48000)
            val components = ProviderDataBridge.toComponentsObject(AribComponentProjectionPolicy.componentsForService(service))
            check(components.getJSONArray("video").getJSONObject(0).getString("profileLevel").contains("level_idc=40"))
            val audio = components.getJSONArray("audio")
            check(audio.getJSONObject(0).getString("codec") == "MPEG-4-ALS")
            check(audio.getJSONObject(1).getString("sourceDescriptor").contains("1190"))
        }
    }

    @Test fun eitInstanceCompletionKeepsTransportScopesAndVersionsSeparate() {
        NativeAribSiParser().use { parser ->
            fun body(number: Int, tsid: Int = 0x11, version: Int = 31): IntArray =
                eitWithDescriptors(emptyList()).also {
                    it[5] = 0xc1 or (version shl 1)
                    it[6] = number
                    it[7] = 1
                    it[9] = tsid
                    it[15] = 0x34 + number
                }
            check(parser.ingestSection(TsPid(0x12), section(body(0))) == SiStatus.OK)
            check(parser.ingestSection(TsPid(0x12), section(body(1, tsid = 0x12))) == SiStatus.OK)
            val partial = parser.serviceRegistrationSnapshot().eitInstances
            check(partial.size == 2 && partial.none { it.complete })
            check(partial.first().receivedSections == listOf(0))
            check(partial.first().missingSections == listOf(1))
            check(parser.takeProgramPublishSnapshot().updateWindows.isEmpty())
            check(parser.ingestSection(TsPid(0x12), section(body(1))) == SiStatus.OK)
            val complete = parser.takeProgramPublishSnapshot()
            check(complete.eitInstances.count { it.complete } == 1)
            check(complete.updateWindows.single().deletionAuthoritative)
            check(parser.ingestSection(TsPid(0x12), section(body(0, version = 0))) == SiStatus.OK)
            check(parser.ingestSection(TsPid(0x12), section(body(0, version = 31))) == SiStatus.OK)
            val newer = parser.serviceRegistrationSnapshot().eitInstances.first()
            check(newer.version == 0 && !newer.complete)
            check(parser.takeProgramPublishSnapshot().updateWindows.isEmpty())
        }
    }

    @Test fun caDiscoveryDoesNotDependOnClearLivePlaybackSnapshot() {
        val parser = NativeAribSiParser()
        try {
            check(parser.ingestSection(TsPid(PID_PAT), section(PAT_BODY)) == SiStatus.OK)
            check(parser.ingestSection(TsPid(PID_SDT), section(SDT_SCRAMBLED_SERVICE_BODY)) == SiStatus.OK)
            check(parser.ingestSection(TsPid(PID_PMT), section(PMT_WITH_PROGRAM_AND_ES_CA_BODY)) == SiStatus.OK)
            check(parser.ingestSection(TsPid(PID_CAT), section(CAT_BODY)) == SiStatus.OK)

            // サービス登録 snapshot はチャンネル登録可否判定用に予約する。
            // CAS検出は、そのsnapshotが空かどうかに依存してはならない。

            val snapshot = parser.casDiscoverySnapshot()
            val discoveryServices = snapshot.services
            check(discoveryServices.single().serviceKey.serviceId == SERVICE_ID)

            val liveSnapshot = parser.livePlaybackSnapshot()
            check(liveSnapshot.services == snapshot.services)
            check(liveSnapshot.pmtPids.values.single() == TsPid(PID_PMT))
            check(liveSnapshot.caMetadata == snapshot.caMetadata)
            check(liveSnapshot.caMetadata.any { it.source == CaMetadataSource.PROGRAM && it.ecmPid == TsPid(ECM_PID_PROGRAM) })
            check(liveSnapshot.catEmmPids == listOf(TsPid(EMM_PID)))

            val metadata = snapshot.caMetadata
            check(metadata.any { it.source == CaMetadataSource.PROGRAM && it.ecmPid == TsPid(ECM_PID_PROGRAM) }) {
                "番組単位CA_descriptorはCAS検出から見える必要があります"
            }
            check(metadata.any { it.source == CaMetadataSource.ELEMENTARY_STREAM && it.elementaryPid == TsPid(VIDEO_PID) && it.ecmPid == TsPid(ECM_PID_ES) }) {
                "ES単位CA_descriptorはCAS検出から見える必要があります"
            }
            check(metadata.any { it.source == CaMetadataSource.CAT && it.serviceKey == null && it.emmPid == TsPid(EMM_PID) }) {
                "CAT EMM PIDはサービス行公開と独立して見える必要があります"
            }

            val facts = parser.serviceRegistrationSnapshot().semanticFactsByServiceKey.values.single {
                it.serviceKey.serviceId == SERVICE_ID
            }
            val diagnostic = ServicePolicyEvaluator.evaluate(facts)
            check(!diagnostic.clearLivePlaybackStaticallyEligible)
            check(diagnostic.requiresCas && diagnostic.reasons.contains("CAS_NOT_IMPLEMENTED")) {
                "CAS検出対象サービスは非スクランブルlive未対応診断を保持する必要があります: ${diagnostic.reasons}"
            }
        } finally {
            parser.close()
        }
    }

    @Test fun serviceNamesPreserveAbsentAndPresentEmptyValuesAcrossJni() {
        val parser = NativeAribSiParser()
        try {
            check(parser.ingestSection(TsPid(PID_SDT), section(SDT_SCRAMBLED_SERVICE_BODY)) == SiStatus.OK)
            val present = parser.serviceRegistrationSnapshot()
            check(present.services.single().providerName == "")
            check(present.semanticFactsByServiceKey.values.single().providerName == "")
            val absentBody = mutableListOf(
                0x42, 0xf0, 0, 0, 0x11, 0xc3, 0, 0, 0, 0x22, 0,
                0, 1, 0xfc, 0x80, 0,
            )
            setSectionLength(absentBody, 0xf0)
            check(parser.ingestSection(TsPid(PID_SDT), section(absentBody.toIntArray())) == SiStatus.OK)
            val absent = parser.serviceRegistrationSnapshot()
            check(absent.services.single().name == null)
            check(absent.services.single().providerName == null)
            check(absent.semanticFactsByServiceKey.values.single().name == null)
        } finally {
            parser.close()
        }
    }

    @Test fun nitTransportMetadataSurvivesTheSemanticBoundary() {
        val parser = NativeAribSiParser()
        try {
            val networkName = listOf(0x1b, 0x28, 0x42) + "Network".map { it.code }
            val tsName = listOf(0x1b, 0x28, 0x42) + "Transport".map { it.code }
            val networkDescriptor = listOf(0x40, networkName.size) + networkName
            val tsDescriptor = listOf(0xcd, tsName.size + 2, 7, tsName.size shl 2) + tsName
            val tsLoop = listOf(0, 0x11, 0, 0x22, 0xf0, tsDescriptor.size) + tsDescriptor
            val nit = (listOf(0x40, 0xb0, 0, 0, 0x22, 0xc1, 0, 0, 0xf0, networkDescriptor.size) +
                networkDescriptor + listOf(0xf0, tsLoop.size) + tsLoop).toMutableList()
            setSectionLength(nit, 0xb0)
            check(parser.ingestSection(TsPid(0x10), section(nit.toIntArray())) == SiStatus.OK)
            check(parser.ingestSection(TsPid(PID_SDT), section(SDT_SCRAMBLED_SERVICE_BODY)) == SiStatus.OK)
            val transport = parser.serviceRegistrationSnapshot().actualTransportMetadata.single()
            check(transport.networkName == "Network")
            check(transport.transportStreamName == "Transport")
            check(transport.remoteControlKeyId == 7)
            check(transport.sdtActual)
        } finally {
            parser.close()
        }
    }

    @Test fun eitDescriptorFactsSurviveBulkSnapshotAndProgramProviderData() {
        val parser = NativeAribSiParser()
        try {
            check(parser.ingestSection(TsPid(PID_PAT), section(PAT_BODY)) == SiStatus.OK)
            check(parser.ingestSection(TsPid(PID_SDT), section(SDT_SCRAMBLED_SERVICE_BODY)) == SiStatus.OK)
            check(parser.ingestSection(TsPid(PID_PMT), section(pmtWithComponentTagsBody())) == SiStatus.OK)
            check(parser.ingestSection(TsPid(PID_EIT), section(eitWithDescriptorFactsBody())) == SiStatus.OK)

            val event = parser.programStateSnapshot().events.single()
            val video = event.descriptors.components.video.single()
            check(video.esPid == TsPid(VIDEO_PID))
            check(video.streamType == 0x1b)
            check(video.codec == "H.264")
            check(video.componentType == 0xb3)
            check(video.resolution == "1080")
            check(video.scan == "interlaced")
            check(video.aspect == "16:9")
            check(video.sourceDescriptor == "component_descriptor")

            val audio = event.descriptors.components.audio.single()
            check(audio.esPid == TsPid(AUDIO_PID))
            check(audio.streamType == 0x0f)
            check(audio.codec == "AAC")
            check(audio.componentType == 0x02)
            check(audio.language == "jpn")
            check(audio.secondLanguage == "eng")
            check(audio.channelConfiguration == "1/0+1/0")
            check(audio.samplingInfo == "48kHz")
            check(audio.sourceDescriptor == "audio_component_descriptor")

            check(event.descriptors.series?.expireDateValid == true)
            check(event.descriptors.series?.expireDate == 0xe123)
            check(event.descriptors.linkage.single().privateDataPrefixHex == "aabb")

            val program = EventModelMapper().toProgramRecords(listOf(event)).single()
            val providerData = JSONObject(ProviderDataBridge.buildProgramProviderData(program).json)
            val providerVideo = providerData.getJSONObject("components").getJSONArray("video").getJSONObject(0)
            check(providerVideo.getString("resolution") == "1080")
            check(providerVideo.getString("scan") == "interlaced")
            check(providerVideo.getString("aspect") == "16:9")
            check(providerVideo.getString("sourceDescriptor") == "component_descriptor")
            check(!providerVideo.has("diagnosticCode"))
            check(!providerVideo.has("r51PlaybackSupported"))

            val providerAudio = providerData.getJSONObject("components").getJSONArray("audio").getJSONObject(0)
            check(providerAudio.getString("channelConfiguration") == "1/0+1/0")
            check(providerAudio.getString("samplingInfo") == "48kHz")
            check(providerAudio.getString("sourceDescriptor") == "audio_component_descriptor")
            check(!providerAudio.has("diagnosticCode"))
            check(!providerAudio.has("liveViewableClaim"))

            val providerSeries = providerData.getJSONObject("series")
            check(providerSeries.getBoolean("expireDateValid"))
            check(providerSeries.getInt("expireDate") == 0xe123)
            check(providerData.getJSONArray("linkage").getJSONObject(0).getString("privateDataPrefixHex") == "aabb")
            check(!providerData.getJSONObject("freeCaMode").has("text"))
        } finally {
            parser.close()
        }
    }

    @Test fun rejectedRatingsAndFullUnknownDescriptorsSurviveProductionPublication() {
        val parser = NativeAribSiParser()
        try {
            val valid = listOf(0x55, 4, 0x4a, 0x50, 0x4e, 12)
            val malformed = listOf(0x55, 5, 0x4a, 0x50, 0x4e, 15, 0xaa)
            val unsupported = listOf(0x55, 4, 0xff, 0, 0x58, 0x8f)
            val unknown = listOf(0xfe, 80) + (0 until 80).toList()
            val truncated = listOf(0x55, 4, 0x4a, 0x50)
            val body = eitWithDescriptors(valid + malformed + unsupported + unknown + truncated)
            check(parser.ingestSection(TsPid(PID_EIT), section(body)) == SiStatus.OK)
            val event = parser.programStateSnapshot().events.single()
            check(event.descriptors.parentalRatings == listOf(AribParentalRating("JPN", 12)))
            val facts = JSONObject(requireNotNull(event.descriptors.diagnostics.descriptorFactsCanonicalJson))
            val ratings = facts.getJSONArray("parentalRatingDescriptors")
            check(ratings.length() == 4)
            check(ratings.getJSONObject(0).getString("parseStatus") == "OK")
            check(ratings.getJSONObject(1).getString("parseStatus") == "MalformedLength")
            check(ratings.getJSONObject(1).getJSONArray("entries").length() == 0)
            check(ratings.getJSONObject(2).getString("rawDescriptorHex") == "5504ff00588f")
            check(ratings.getJSONObject(2).getJSONArray("entries").getJSONObject(0).getString("countryCode").map { it.code } == listOf(255, 0, 88))
            check(ratings.getJSONObject(3).getString("parseStatus") == "TruncatedDescriptor")
            val rawUnknown = facts.getJSONArray("unknownDescriptors").getJSONObject(0).getString("rawDescriptorHex")
            check(rawUnknown.length == 164 && rawUnknown.endsWith("4d4e4f"))
            val program = EventModelMapper().toProgramRecords(listOf(event)).single()
            val stored = ProviderDataBridge.buildProgramProviderData(program).json
            val canonical = JSONObject(stored)
            check(canonical.getJSONArray("ratings").length() == 1)
            val savedFacts = canonical.getJSONObject("diagnostics").getJSONObject("descriptorFacts")
            val savedRatings = savedFacts.getJSONArray("parentalRatingDescriptors")
            check(savedRatings.length() == ratings.length())
            for (index in 0 until ratings.length()) {
                val before = ratings.getJSONObject(index)
                val after = savedRatings.getJSONObject(index)
                check(after.getString("rawDescriptorHex") == before.getString("rawDescriptorHex"))
                check(after.getString("parseStatus") == before.getString("parseStatus"))
                val beforeEntries = before.getJSONArray("entries")
                val afterEntries = after.getJSONArray("entries")
                check(beforeEntries.length() == afterEntries.length())
                for (entryIndex in 0 until beforeEntries.length()) {
                    for (key in listOf("countryCode", "rawRatingByte", "parseStatus")) {
                        check(beforeEntries.getJSONObject(entryIndex).get(key) == afterEntries.getJSONObject(entryIndex).get(key))
                    }
                }
            }
            check(savedFacts.getJSONArray("unknownDescriptors").getJSONObject(0).getString("rawDescriptorHex") == rawUnknown)
            check(ProviderDataBridge.normalizeProgramProviderData(stored.toByteArray(Charsets.UTF_8)).json == stored)
        } finally {
            parser.close()
        }
    }

    @Test fun eitDescriptorWithoutMatchingPmtTagRemainsCanonicalProviderData() {
        val parser = NativeAribSiParser()
        try {
            check(parser.ingestSection(TsPid(PID_PAT), section(PAT_BODY)) == SiStatus.OK)
            check(parser.ingestSection(TsPid(PID_SDT), section(SDT_SCRAMBLED_SERVICE_BODY)) == SiStatus.OK)
            check(parser.ingestSection(TsPid(PID_PMT), section(PMT_WITH_PROGRAM_AND_ES_CA_BODY)) == SiStatus.OK)
            check(parser.ingestSection(TsPid(PID_EIT), section(eitWithDescriptorFactsBody())) == SiStatus.OK)

            val event = parser.programStateSnapshot().events.single()
            val eitOnlyVideo = event.descriptors.components.video.single { it.componentTag == 0x10 }
            check(eitOnlyVideo.esPid == null)
            check(eitOnlyVideo.streamType == null)
            check(eitOnlyVideo.codec == null)

            val program = EventModelMapper().toProgramRecords(listOf(event)).single()
            val providerData = JSONObject(ProviderDataBridge.buildProgramProviderData(program).json)
            val videoArray = providerData.getJSONObject("components").getJSONArray("video")
            val providerVideo = (0 until videoArray.length())
                .map { videoArray.getJSONObject(it) }
                .single { it.optInt("componentTag", -1) == 0x10 }
            check(providerVideo.isNull("esPid"))
            check(providerVideo.isNull("streamType"))
            check(providerVideo.isNull("codec"))
            check(providerVideo.getString("sourceDescriptor") == "component_descriptor")
        } finally {
            parser.close()
        }
    }

    companion object {
        private const val SERVICE_ID = 0x0001
        private const val PID_PAT = 0x0000
        private const val PID_CAT = 0x0001
        private const val PID_SDT = 0x0011
        private const val PID_EIT = 0x0012
        private const val PID_PMT = 0x0100
        private const val VIDEO_PID = 0x0101
        private const val AUDIO_PID = 0x0102
        private const val ECM_PID_PROGRAM = 0x0123
        private const val ECM_PID_ES = 0x0124
        private const val EMM_PID = 0x0100

        private val PAT_BODY = intArrayOf(
            0x00, 0xb0, 0x0d, 0x00, 0x11, 0xc1, 0x00, 0x00,
            0x00, 0x01, 0xe1, 0x00,
        )

        private val SDT_SCRAMBLED_SERVICE_BODY = intArrayOf(
            0x42, 0xf0, 0x18, 0x00, 0x11, 0xc1, 0x00, 0x00, 0x00, 0x22, 0x00,
            0x00, 0x01, 0xfc, 0xf0, 0x07,
            0x48, 0x05, 0x01, 0x00, 0x02, 'T'.code, '1'.code,
        )

        private val PMT_WITH_PROGRAM_AND_ES_CA_BODY = intArrayOf(
            0x02, 0xb0, 0x23, 0x00, 0x01, 0xc1, 0x00, 0x00,
            0xe1, 0x01, 0xf0, 0x06,
            0x09, 0x04, 0x00, 0x05, 0xe1, 0x23,
            0x1b, 0xe1, 0x01, 0xf0, 0x06,
            0x09, 0x04, 0x00, 0x05, 0xe1, 0x24,
            0x0f, 0xe1, 0x02, 0xf0, 0x00,
        )

        private val CAT_BODY = intArrayOf(
            0x01, 0xb0, 0x0f, 0x00, 0x01, 0xc1, 0x00, 0x00,
            0x09, 0x04, 0x00, 0x05, 0xe1, 0x00,
        )

        private fun pmtWithComponentTagsBody(): IntArray {
            val body = mutableListOf(
                0x02, 0xb0, 0x00, 0x00, 0x01, 0xc1, 0x00, 0x00,
                0xe1, 0x01, 0xf0, 0x00,
                0x1b, 0xe1, 0x01, 0xf0, 0x03, 0x52, 0x01, 0x10,
                0x0f, 0xe1, 0x02, 0xf0, 0x03, 0x52, 0x01, 0x20,
            )
            setSectionLength(body, 0xb0)
            return body.toIntArray()
        }

        private fun eitWithDescriptorFactsBody(): IntArray {
            val descriptors = mutableListOf(
                0x50, 0x06, 0x01, 0xb3, 0x10, 'j'.code, 'p'.code, 'n'.code,
                0xc4, 0x0c, 0x02, 0x02, 0x20, 0x0f, 0xff, 0xee,
                'j'.code, 'p'.code, 'n'.code, 'e'.code, 'n'.code, 'g'.code,
                0xd5, 0x09, 0x12, 0x34, 0x2b, 0xe1, 0x23, 0x00, 0x03, 0x00, 0x0c,
                0x4a, 0x09, 0x00, 0x11, 0x00, 0x22, 0x00, 0x01, 0x0d, 0xaa, 0xbb,
            )
            return eitWithDescriptors(descriptors)
        }

        private fun eitWithDescriptors(descriptors: List<Int>): IntArray {
            val descriptorLength = descriptors.size
            val body = mutableListOf(
                0x4e, 0xf0, 0x00, 0x00, 0x01, 0xc1, 0x00, 0x00,
                0x00, 0x11, 0x00, 0x22, 0x00, 0x4e,
                0x12, 0x34, 0xee, 0x00, 0x12, 0x00, 0x00,
                0x00, 0x30, 0x00,
                0x80 or ((descriptorLength ushr 8) and 0x0f), descriptorLength and 0xff,
            )
            body += descriptors
            setSectionLength(body, 0xf0)
            return body.toIntArray()
        }

        private fun setSectionLength(body: MutableList<Int>, highBits: Int) {
            val sectionLength = body.size - 3 + 4
            body[1] = highBits or ((sectionLength ushr 8) and 0x0f)
            body[2] = sectionLength and 0xff
        }

        private fun section(body: IntArray): ByteArray {
            val bytes = body.map { it.toByte() }.toMutableList()
            val crc = crc32Mpeg(bytes.map { it.toInt() and 0xff })
            bytes += ((crc ushr 24) and 0xff).toByte()
            bytes += ((crc ushr 16) and 0xff).toByte()
            bytes += ((crc ushr 8) and 0xff).toByte()
            bytes += (crc and 0xff).toByte()
            return bytes.toByteArray()
        }

        private fun crc32Mpeg(bytes: List<Int>): Long {
            var crc = 0xffffffffL
            for (b in bytes) {
                crc = crc xor ((b.toLong() and 0xffL) shl 24)
                repeat(8) {
                    crc = if ((crc and 0x80000000L) != 0L) {
                        ((crc shl 1) xor 0x04c11db7L) and 0xffffffffL
                    } else {
                        (crc shl 1) and 0xffffffffL
                    }
                }
            }
            return crc and 0xffffffffL
        }
    }
}

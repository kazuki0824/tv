package com.maleicacid.tvinput.aribsi

import com.maleicacid.tvinput.common.TsPid
import org.json.JSONObject
import org.junit.Test

class NativeAribSiParserCasDiscoveryTest {
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
            check(video.componentType == 0xb3)
            check(video.resolution == "1080")
            check(video.scan == "interlaced")
            check(video.aspect == "16:9")
            check(video.sourceDescriptor == "component_descriptor")

            val audio = event.descriptors.components.audio.single()
            check(audio.esPid == TsPid(AUDIO_PID))
            check(audio.streamType == 0x0f)
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

// テストの入力・期待値を本体の定数と独立した具体値で記述する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONObject
import org.junit.Test

class ProviderDataAssetsR51ContractTest {
    @Test fun providerDataFailureIsTypedAndCurrentRequestsRequireCasEvidence() {
        val bridge = com.maleicacid.tvinput.aribsi.ProviderDataBridge
        val rejected = bridge.normalizeProgramProviderData(byteArrayOf(0xff.toByte()))
        check(rejected is com.maleicacid.tvinput.aribsi.ProviderDataBridge.Failure)
        check(rejected.errorCode.isNotBlank() && rejected.errorMessage.isNotBlank())
        val key =
            com.maleicacid.tvinput.common
                .ServiceKey(4, 0x4010, 101)
        val program =
            com.maleicacid.tvinput.db.ProgramRecord(
                serviceKey = key,
                eventId = 1,
                stableIdentity = "",
                startTimeMillis = 1_700_000_000_000L,
                durationMillis = 1_800_000L,
                title = "test",
                description = "description",
            )
        check(bridge.buildProgramProviderData(program) is com.maleicacid.tvinput.aribsi.ProviderDataBridge.Failure)
        check(
            bridge.buildProgramProviderData(
                program.copy(casFactsCanonicalJson = testCasFacts()),
            ) is com.maleicacid.tvinput.aribsi.ProviderDataBridge.Success,
        )
        val channel =
            com.maleicacid.tvinput.db.ChannelRecord(
                key,
                1,
                "101",
                "test",
                com.maleicacid.tvinput.common
                    .FrequencyHz(473_142_857L),
            )
        check(bridge.buildChannelProviderData(channel) is com.maleicacid.tvinput.aribsi.ProviderDataBridge.Failure)
        check(
            bridge.buildChannelProviderData(
                channel.copy(casFactsCanonicalJson = testCasFacts()),
            ) is com.maleicacid.tvinput.aribsi.ProviderDataBridge.Success,
        )
    }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    @Test
    fun providerResultEnvelopeRejectsUnknownFieldsAndMalformedFailure() {
        val bridge = com.maleicacid.tvinput.aribsi.ProviderDataBridge
        val failure =
            JSONObject()
                .put("success", false)
                .put("bytes", "")
                .put("schemaVersion", 1)
                .put("truncated", false)
                .put("diagnosticsDroppedCount", 0)
                .put("errorCode", "INVALID")
                .put("errorMessage", "invalid input")
        check((bridge.parseResult(failure.toString()) as com.maleicacid.tvinput.aribsi.ProviderDataBridge.Failure).errorCode == "INVALID")
        failure.put("bytes", "{}")
        check(
            (bridge.parseResult(failure.toString()) as com.maleicacid.tvinput.aribsi.ProviderDataBridge.Failure).errorCode ==
                "PROVIDER_DATA_RESULT_INVALID",
        )
        failure.put("bytes", "").put("unknown", 1)
        check(
            (bridge.parseResult(failure.toString()) as com.maleicacid.tvinput.aribsi.ProviderDataBridge.Failure).errorCode ==
                "PROVIDER_DATA_RESULT_INVALID",
        )
    }

    @Test fun sharedBoundaryCorpusAgreesThroughTheProductionJniBridge() {
        val cases = org.json.JSONArray(assetText("provider_data_boundary_v1/cases.json"))
        for (index in 0 until cases.length()) {
            val case = cases.getJSONObject(index)
            val data = case.getString("data")
            val bytes =
                when (val encoding = case.getString("encoding")) {
                    "UTF8" -> data.toByteArray(Charsets.UTF_8)
                    "HEX" -> data.chunked(2).map { it.toInt(16).toByte() }.toByteArray()
                    else -> error("未知のfixture符号化: $encoding")
                }
            val expected = case.getBoolean("accepted")
            val accepted =
                when (val boundary = case.getString("boundary")) {
                    "PROGRAM" -> {
                        check(
                            (
                                com.maleicacid.tvinput.aribsi.ProviderDataBridge
                                    .extractProgramKeyResult(bytes) != null
                            ) == expected,
                        ) { case.getString("name") }
                        com.maleicacid.tvinput.aribsi.ProviderDataBridge.normalizeProgramProviderData(
                            bytes,
                        ) is com.maleicacid.tvinput.aribsi.ProviderDataBridge.Success
                    }

                    "CHANNEL" -> {
                        com.maleicacid.tvinput.aribsi.ProviderDataBridge
                            .decodeChannelProviderData(bytes) != null
                    }

                    else -> {
                        error("未知のfixture境界: $boundary")
                    }
                }
            check(accepted == expected) { case.getString("name") }
        }
    }

    @Test fun minimalProviderDataFixtureKeepsProgramProviderDataV1Shape() {
        val providerData = providerDataAsset("minimal_clear_program.json")

        check(providerData.getString("schema") == "maleicacid.tv.program")
        check(providerData.getInt("schemaVersion") == 1)
        check(providerData.getJSONObject("programKey").getString("kind") == "arib-event-v1")
        check(providerData.getJSONObject("programKey").getInt("serviceId") == 101)
        check(!providerData.has("serviceKey"))
        check(!providerData.has("audioLanguages"))
        check(!providerData.getJSONObject("timing").has("endUtcMillis"))
        check(providerData.getJSONObject("diagnostics").has("descriptorDiagnostics"))
        check(providerData.getJSONObject("components").has("subtitle"))
        check(providerData.getJSONArray("shortEvents").length() == 0)
        check(providerData.getJSONArray("extendedTexts").length() == 0)
        check(!providerData.has("skippedUnresolvedTransport"))
        check(!providerData.has("programKeyB64"))
        check(!providerData.has("eventGroupText"))
        check(!providerData.has("unsupportedDescriptorDiagnostics"))
    }

    @Test fun unsupportedCodecFixtureKeepsMetadataButDoesNotClaimR51Playback() {
        val providerData = providerDataAsset("unsupported_codec_program.json")
        val components = providerData.getJSONObject("components")
        val video = components.getJSONArray("video").getJSONObject(0)
        val audio = components.getJSONArray("audio").getJSONObject(0)

        check(video.getString("codec") == "HEVC")
        check(video.getInt("streamType") == 0x24)
        check(video.getString("parseStatus") == "OK")
        check(!video.has("diagnosticCode"))
        check(!video.has("r51PlaybackSupported"))
        check(!video.has("liveViewableClaim"))

        check(audio.getString("codec") == "MPEG-4-AAC-LATM")
        check(audio.getInt("streamType") == 0x11)
        check(audio.getString("parseStatus") == "OK")
        check(!audio.has("diagnosticCode"))
        check(!audio.has("r51PlaybackSupported"))
        check(!audio.has("liveViewableClaim"))
    }

    @Test fun descriptorDiagnosticFixtureIsElementSchemaNotLegacyWrapper() {
        val diagnostic = JSONObject(assetText("descriptor_diagnostic_v1/malformed_length.json"))

        check(diagnostic.getString("schema") == "maleicacid.tv.descriptorDiagnostic")
        check(diagnostic.getInt("schemaVersion") == 1)
        check(diagnostic.getString("code") == "MalformedLength")
        check(diagnostic.has("scope"))
        check(diagnostic.has("descriptor"))
        check(!diagnostic.has("diagnostics"))
    }

    private fun providerDataAsset(name: String): JSONObject = JSONObject(assetText("program_provider_data_v1/$name"))

    private fun assetText(path: String): String {
        val hostAssets = System.getProperty("maleicacid.tis.testAssetsRoot")
        if (hostAssets != null) return java.io.File(hostAssets, path).readText(Charsets.UTF_8)
        return InstrumentationRegistry
            .getInstrumentation()
            .context.assets
            .open(path)
            .bufferedReader(Charsets.UTF_8)
            .use { it.readText() }
    }
}

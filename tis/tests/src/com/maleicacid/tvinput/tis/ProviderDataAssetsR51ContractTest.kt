package com.maleicacid.tvinput.tis

import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONObject
import org.junit.Test

class ProviderDataAssetsR51ContractTest {
    @Test fun minimalProviderDataFixtureKeepsProgramProviderDataV1Shape() {
        val providerData = providerDataAsset("minimal_clear_program.json")

        check(providerData.getString("schema") == "maleicacid.tv.program")
        check(providerData.getInt("schemaVersion") == 1)
        check(providerData.getJSONObject("programKey").getString("kind") == "arib-event-v1")
        check(providerData.getJSONObject("serviceKey").getInt("serviceId") == 101)
        check(providerData.getJSONObject("diagnostics").has("descriptorDiagnostics"))
        check(providerData.getJSONObject("components").has("subtitle"))
        check(!providerData.getBoolean("skippedUnresolvedTransport"))
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
        check(video.getString("diagnosticCode") == "UNSUPPORTED_R51_CODEC")
        check(video.getString("parseStatus") == "UNSUPPORTED_R51")
        check(!video.has("r51PlaybackSupported"))
        check(!video.has("liveViewableClaim"))

        check(audio.getString("codec") == "MPEG-4-AAC-LATM")
        check(audio.getInt("streamType") == 0x11)
        check(audio.getString("diagnosticCode") == "UNSUPPORTED_R51_CODEC")
        check(audio.getString("parseStatus") == "UNSUPPORTED_R51")
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

    private fun providerDataAsset(name: String): JSONObject =
        JSONObject(assetText("program_provider_data_v1/$name"))

    private fun assetText(path: String): String =
        InstrumentationRegistry.getInstrumentation().context.assets
            .open(path)
            .bufferedReader(Charsets.UTF_8)
            .use { it.readText() }
}

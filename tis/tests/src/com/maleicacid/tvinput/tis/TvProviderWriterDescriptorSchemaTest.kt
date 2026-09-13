// テストの入力・期待値を本体の定数と独立した具体値で記述する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONObject
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class TvProviderWriterDescriptorSchemaTest {
    @Test fun descriptorDiagnosticAssetFixtureUsesCanonicalElementSchema() {
        val fixture = assetText("descriptor_diagnostic_v1/malformed_length.json")
        val diagnostic = JSONObject(fixture)

        check(diagnostic.getString("schema") == "maleicacid.tv.descriptorDiagnostic")
        check(diagnostic.getInt("schemaVersion") == 1)
        check(diagnostic.getString("code") == "MalformedLength")
        check(diagnostic.has("scope"))
        check(diagnostic.has("descriptor"))

        val descriptor = diagnostic.getJSONObject("descriptor")
        check(descriptor.getInt("tag") == 77)
        check(descriptor.getInt("declaredLength") == 6)
        check(descriptor.getInt("actualRemainingLength") == 3)
        check(descriptor.getString("rawPrefixHex") == "4d06ffffff")
    }

    @Test fun programProviderDataAssetFixtureUsesDescriptorDiagnosticsArrayUnderDiagnostics() {
        val fixture = assetText("program_provider_data_v1/minimal_clear_program.json")
        val providerData = JSONObject(fixture)

        check(providerData.getString("schema") == "maleicacid.tv.program")
        check(providerData.getInt("schemaVersion") == 1)
        check(!providerData.has("descriptorDiagnostics"))

        val diagnostics = providerData.getJSONObject("diagnostics")
        check(diagnostics.has("descriptorDiagnostics"))
        check(diagnostics.getJSONArray("descriptorDiagnostics").length() == 0)
        check(diagnostics.has("publishDiagnostics"))
        check(diagnostics.has("parserDiagnostics"))
    }

    private fun assetText(path: String): String =
        InstrumentationRegistry
            .getInstrumentation()
            .context.assets
            .open(path)
            .bufferedReader(Charsets.UTF_8)
            .use { it.readText() }
}

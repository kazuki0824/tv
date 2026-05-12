package com.maleicacid.tvinput.tis

import android.media.tv.TvContract
import androidx.test.platform.app.InstrumentationRegistry
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.db.ProgramRecord
import org.json.JSONObject
import org.junit.Test

class TvProviderWriterDescriptorSchemaTest {
    private val key = ServiceKey(4, 16625, 101)

    @Test fun descriptorDiagnosticAssetFixtureNormalizesToCanonicalProviderDataSchema() {
        val fixture = InstrumentationRegistry.getInstrumentation().context.assets
            .open("descriptor_diagnostic_v1/malformed_length.json")
            .bufferedReader(Charsets.UTF_8)
            .use { it.readText() }
        check(JSONObject(fixture).getInt("schemaVersion") == 1)

        val writer = TvProviderWriter("input.test", object : TvProviderWriter.ChannelStore {
            override fun findExistingChannelId(key: ServiceKey): Result<Long?> = Result.success(1L)
            override fun insertChannel(values: android.content.ContentValues): Result<Long?> = Result.success(1L)
            override fun updateChannel(channelId: Long, values: android.content.ContentValues): Result<Int> = Result.success(1)
        }, testOnly = true)
        val record = ProgramRecord(
            serviceKey = key,
            eventId = 10,
            stableIdentity = "onid=4;tsid=16625;sid=101;event=10",
            startTimeMillis = 1_700_000_000_000L,
            durationMillis = 1_800_000L,
            title = "番組",
            description = "説明",
            diagnosticDescriptorJson = fixture,
            extendedItemsJson = "[{\"description\":\"出演\",\"text\":\"A\"}]",
        )

        val values = writer.programValuesForTest(1L, record)
        val providerData = JSONObject(values.getAsByteArray(TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA).toString(Charsets.UTF_8))
        val descriptorDiagnostics = providerData.getJSONObject("descriptorDiagnostics")
        check(descriptorDiagnostics.getInt("schemaVersion") == 1)
        val diagnostics = descriptorDiagnostics.getJSONArray("diagnostics")
        check(diagnostics.length() == 1)
        val first = diagnostics.getJSONObject(0)
        check(first.getString("parseStatus") == "MalformedLength")
        check(first.getInt("tag") == 77)
        check(first.getString("rawPrefix") == "4d06ffffff")
        val extendedItem = providerData.getJSONArray("extendedItems").getJSONObject(0)
        check(extendedItem.getString("description") == "出演")
        check(extendedItem.getString("text") == "A")
    }
}

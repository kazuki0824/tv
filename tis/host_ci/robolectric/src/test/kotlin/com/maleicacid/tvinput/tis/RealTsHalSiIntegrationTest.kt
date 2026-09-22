// テストの入力・期待値と試験専用の入出力形式を具体値で照合する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import androidx.test.core.app.ApplicationProvider
import com.maleicacid.tvinput.aribsi.AribService
import com.maleicacid.tvinput.aribsi.AribSiEngine
import com.maleicacid.tvinput.aribsi.CaDescriptorScope
import com.maleicacid.tvinput.aribsi.SectionIngestController
import com.maleicacid.tvinput.aribsi.ServiceListBuilder
import com.maleicacid.tvinput.aribsi.SiDiscoveryProfile
import com.maleicacid.tvinput.aribsi.SiStatus
import com.maleicacid.tvinput.common.TsPid
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import java.io.DataInputStream
import java.io.File
import java.nio.file.Files
import java.security.MessageDigest
import java.util.concurrent.TimeUnit

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class RealTsHalSiIntegrationTest {
    private data class Section(val pid: Int, val bytes: ByteArray)

    @Test(timeout = 60000)
    fun realTsProducesServiceAndProgramFacts() {
        val directory = File(checkNotNull(System.getProperty("realTs.fixtureDirectory")))
        val expected = JSONObject(File(directory, "expected.json").readText())
        val input = File(directory, expected.getString("fixture"))
        assertEquals(expected.getLong("size_bytes"), input.length())
        assertEquals(expected.getString("sha256"), sha256(input.readBytes()))
        val references = objects(expected.getJSONArray("section_reference"))
        val sections = runHal(input, expected, references)
        verifySections(sections, references)
        AribSiEngine(ApplicationProvider.getApplicationContext()).use { engine ->
            engine.setDiscoveryProfile(SiDiscoveryProfile.ISDB_T)
            verifyIngest(engine, sections)
            verifyServices(engine, expected)
            verifyPrograms(engine, expected)
        }
    }

    private fun runHal(input: File, expected: JSONObject, references: List<JSONObject>): List<Section> {
        val diagnostics = File(checkNotNull(System.getProperty("realTs.diagnosticsDirectory")))
        Files.createDirectories(diagnostics.toPath())
        val directory = Files.createTempDirectory(diagnostics.toPath(), "real-ts-").toFile()
        val output = File(directory, "sections.bin")
        val log = File(directory, "hal.log")
        val executable = File(checkNotNull(System.getProperty("realTs.executable")))
        check(executable.canExecute()) { "HAL試験実行器がありません: $executable" }
        val command = listOf(executable.absolutePath, input.absolutePath, output.absolutePath) +
            references.map { "${it.getInt("pid")}:${it.getInt("table_id")}" }
        val process = ProcessBuilder(command).redirectErrorStream(true).redirectOutput(log).start()
        try {
            check(process.waitFor(30, TimeUnit.SECONDS)) { "HAL試験が時間切れです: $log" }
            assertEquals(log.readText(), 0, process.exitValue())
        } finally {
            if (process.isAlive) {
                process.destroyForcibly()
                check(process.waitFor(5, TimeUnit.SECONDS)) { "HAL試験プロセスを回収できません: $log" }
            }
        }
        check(output.length() in 20..1048576) { "HAL出力の長さが不正です" }
        return readSections(output, expected, references.sumOf { it.getInt("section_count") }, references.size)
    }

    private fun readSections(file: File, expected: JSONObject, count: Int, filters: Int): List<Section> =
        DataInputStream(file.inputStream().buffered()).use { stream ->
            val sections = (0 until count).map { sequence ->
                assertEquals("配送順序", sequence, stream.readInt())
                val pid = stream.readInt()
                checkNotNull(TsPid.fromOrNull(pid))
                val length = stream.readInt()
                check(length.toLong() in 3..SectionFilterPolicy.MAX_SECTION_EVENT_BYTES)
                val bytes = ByteArray(length)
                stream.readFully(bytes)
                Section(pid, bytes)
            }
            assertEquals("終了報告", -1, stream.readInt())
            assertEquals(expected.getInt("complete_packets"), stream.readInt())
            assertEquals(expected.getInt("trailing_bytes"), stream.readInt())
            assertEquals(count, stream.readInt())
            assertEquals("照合・解放したキュー数", filters, stream.readInt())
            assertEquals("終了報告後の余分なデータ", -1, stream.read())
            sections
        }

    private fun verifySections(sections: List<Section>, references: List<JSONObject>) {
        val groups = sections.groupBy { it.pid to (it.bytes.first().toInt() and 255) }
        assertEquals(references.map { it.getInt("pid") to it.getInt("table_id") }.toSet(), groups.keys)
        references.forEach { reference ->
            val key = reference.getInt("pid") to reference.getInt("table_id")
            val group = groups.getValue(key)
            assertEquals("$key セクション数", reference.getInt("section_count"), group.size)
            assertEquals("$key バイト数", reference.getInt("payload_bytes"), group.sumOf { it.bytes.size })
            val hashes = objects(reference.getJSONArray("unique_sections")).associate {
                it.getString("sha256") to it.getInt("count")
            }
            assertEquals("$key セクション内容", hashes, group.groupingBy { sha256(it.bytes) }.eachCount())
        }
    }

    private fun verifyIngest(engine: AribSiEngine, sections: List<Section>) {
        val controller = SectionIngestController(engine)
        val ignored = mutableListOf<Pair<Int, Int>>()
        sections.forEach { section ->
            val status = controller.onSection(checkNotNull(TsPid.fromOrNull(section.pid)), section.bytes).status
            assertTrue("pid=${section.pid} status=$status", status == SiStatus.OK ||
                status == SiStatus.IGNORED_UNSUPPORTED_PID_OR_TABLE)
            if (status == SiStatus.IGNORED_UNSUPPORTED_PID_OR_TABLE) {
                ignored += section.pid to (section.bytes.first().toInt() and 255)
            }
        }
        // 元TSの397番packetにPMTがあり、最初のPATは418番packetにある。
        assertEquals(listOf(257 to 2), ignored)
    }

    private fun verifyServices(engine: AribSiEngine, expected: JSONObject) {
        val builder = ServiceListBuilder(engine)
        val actual = builder.snapshot().associateBy { it.serviceKey.serviceId }
        val services = objects(expected.getJSONArray("services"))
        assertEquals(services.map { it.getInt("service_id") }.toSet(), actual.keys)
        services.forEach { reference ->
            val service = actual.getValue(reference.getInt("service_id"))
            assertEquals(expected.getInt("original_network_id"), service.serviceKey.originalNetworkId)
            assertEquals(expected.getInt("transport_stream_id"), service.serviceKey.transportStreamId)
            verifyService(service, reference)
        }
        assertEquals(setOf(1048, 1049), builder.registrationReadySnapshot().map { it.serviceKey.serviceId }.toSet())
    }

    private fun verifyService(service: AribService, reference: JSONObject) {
        assertEquals(reference.getString("name"), service.name)
        assertEquals(reference.getInt("service_type"), service.serviceType)
        assertEquals(reference.getInt("pmt_pid"), service.pmtPid?.value)
        val pcr = if (reference.isNull("pcr_pid")) null else reference.getInt("pcr_pid")
        assertEquals(pcr, service.pcrPid?.value)
        assertEquals(reference.getBoolean("free_ca_mode"), service.freeCaMode)
        val streams = objects(reference.getJSONArray("elementary_streams")).map {
            it.getInt("pid") to it.getInt("stream_type")
        }.sortedBy { it.first }
        assertEquals(streams, service.streams.map { it.elementaryPid.value to it.streamType }.sortedBy { it.first })
        val descriptors = objects(reference.getJSONArray("program_ca_descriptors")).map {
            it.getInt("ca_system_id") to it.getInt("ca_pid")
        }
        assertEquals(descriptors, service.serviceScopedCaDescriptors.filter {
            it.scope == CaDescriptorScope.PROGRAM
        }.map { it.caSystemId to it.caPid?.value })
    }

    private fun verifyPrograms(engine: AribSiEngine, expected: JSONObject) {
        val events = engine.programStateSnapshot().events.filter {
            it.serviceKey.serviceId == expected.getInt("selected_service_id") && it.source.tableId == 0x4e
        }
        val references = objects(expected.getJSONArray("selected_service_pf_events"))
        assertEquals(references.map { it.getInt("event_id") }.sorted(), events.map { it.eventId }.sorted())
        references.forEach { reference ->
            val event = events.single { it.eventId == reference.getInt("event_id") }
            assertEquals(reference.getLong("start_time_unix_ms"), event.startTimeMillis)
            assertEquals(reference.getLong("duration_ms"), event.durationMillis)
        }
    }

    private fun objects(array: JSONArray): List<JSONObject> = (0 until array.length()).map(array::getJSONObject)

    private fun sha256(bytes: ByteArray): String =
        MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { "%02x".format(it.toInt() and 255) }
}

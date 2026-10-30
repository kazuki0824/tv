// テストの入力・期待値と試験専用の入出力形式を具体値で照合する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import android.media.tv.tuner.filter.Filter
import android.media.tv.tuner.filter.SectionEvent
import androidx.test.core.app.ApplicationProvider
import com.maleicacid.tvinput.aribsi.AribService
import com.maleicacid.tvinput.aribsi.AribSiEngine
import com.maleicacid.tvinput.aribsi.CaDescriptorScope
import com.maleicacid.tvinput.aribsi.SectionIngestController
import com.maleicacid.tvinput.aribsi.ServiceListBuilder
import com.maleicacid.tvinput.aribsi.ServicePolicyEvaluator
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
import org.robolectric.shadow.api.Shadow
import org.robolectric.util.ReflectionHelpers
import java.io.DataInputStream
import java.io.File
import java.nio.file.Files
import java.security.MessageDigest
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

@RunWith(RobolectricTestRunner::class)
@Config(
    sdk = [35],
    shadows = [
        TunerFrontendLeaseTransitionTest.NativeTuner::class,
        TunerFrontendLeaseTransitionTest.NativeTrm::class,
        TunerFrontendLeaseTransitionTest.NativeFilter::class,
    ],
)
class RealTsHalSiIntegrationTest {
    // Rust JNIの未対応PID/table通知値。製品の公開定数を試験のために復活させない。
    private val ignoredUnsupportedPidOrTable = 1

    private data class Section(
        val pid: Int,
        val bytes: ByteArray,
    )

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

    // 実TS/HAL/JNIを共用し、SDKのnative I/Oだけを既存Shadowへ接続する。
    // 同じownerの飽和から候補切替までの時系列とfinallyによる解放を一続きで検査する。
    @Suppress("LongMethod", "NestedBlockDepth", "MaxLineLength")
    @Test(timeout = 60000)
    fun realTsBurstLossAndRetransmissionStayWithinCandidate() {
        val directory = File(checkNotNull(System.getProperty("realTs.fixtureDirectory")))
        val expected = JSONObject(File(directory, "expected.json").readText())
        val input = File(directory, expected.getString("fixture"))
        assertEquals(expected.getString("sha256"), sha256(input.readBytes()))
        val references = objects(expected.getJSONArray("section_reference"))
        val sections = runHal(input, expected, references)
        verifySections(sections, references)
        AribSiEngine(ApplicationProvider.getApplicationContext()).use { engine ->
            val tuner = TunerFrontendLeaseTransitionTest.controller()
            val ingest = SectionIngestController(engine)
            tuner.setSectionIngestController(ingest)
            TunerFrontendLeaseTransitionTest.NativeTuner.successfulTunes = true
            val owner = ReflectionHelpers.getField<ControllerSerialExecutor>(tuner, "sectionExecutor")
            try {
                check(tuner.tuneForScan(TunerFrontendLeaseTransitionTest.terrestrial()).success)
                val pat = sections.first { it.pid == 0 }
                deliver(tuner, pat)
                awaitIdle(owner)
                tuner.updatePmtFilters(engine.pmtPidsForSectionFilters(), tuner.currentGeneration())
                val baseline = ingest.transportLossSnapshot()
                val entered = CountDownLatch(1)
                val resume = CountDownLatch(1)
                val started = System.nanoTime()
                owner.executeControl {
                    entered.countDown()
                    check(resume.await(10, TimeUnit.SECONDS))
                }
                try {
                    check(entered.await(5, TimeUnit.SECONDS))
                    // EIT先行で既存16枠を占有し、他FilterのPAT/PMT/SDT/NITを含むburstを同時配送する。
                    val eit = sections.first { it.pid == 18 }
                    repeat(16) { deliver(tuner, eit) }
                    assertEquals(16, owner.queue.size)
                    val groups = sections.groupBy { it.pid }
                    val callbacks = Executors.newFixedThreadPool(groups.size)
                    try {
                        val futures = groups.values.map { group -> callbacks.submit { group.forEach { deliver(tuner, it) } } }
                        futures.forEach { it.get(5, TimeUnit.SECONDS) }
                    } finally {
                        callbacks.shutdownNow()
                        check(callbacks.awaitTermination(5, TimeUnit.SECONDS))
                    }
                    assertEquals(sections.size, ingest.inputDeliveryLossCount)
                    assertEquals(16, owner.queue.size)
                } finally {
                    resume.countDown()
                }
                awaitIdle(owner)
                val blockedNanos = System.nanoTime() - started
                assertEquals(
                    "candidateInputDeliveryLoss=${sections.size} candidateFilterOverflow=0",
                    ChannelScanController.candidateTransportLossDetail(baseline, ingest.transportLossSnapshot()),
                )
                assertTrue(!collectionComplete(engine))
                val replayStarted = System.nanoTime()
                replay(tuner, owner, sections)
                assertTrue(collectionComplete(engine))
                verifyServices(engine, expected)
                verifyPrograms(engine, expected)
                // 回復しても同じ候補内の喪失履歴は消さない。
                assertEquals(sections.size, ingest.inputDeliveryLossCount)
                val replayNanos = System.nanoTime() - replayStarted
                val staleFilter = filters(tuner).getValue(TsPid(17)).first()
                val staleHandle = handles(tuner).getValue(TsPid(17))
                val beforeReplacement = ingest.diagnostics()
                tuner.closeSectionFilter(TsPid(17))
                check(tuner.openSectionFilter(TsPid(17)).isOpen)
                val sdt = sections.first { it.pid == 17 }
                deliverStale(tuner, staleFilter, staleHandle, sdt)
                awaitIdle(owner)
                assertEquals(beforeReplacement, ingest.diagnostics())
                assertEquals(sections.size, ingest.inputDeliveryLossCount)
                // 候補境界は実scanと同じ旧Filter停止後resetを使用する。
                check(
                    tuner
                        .tuneForScan(TunerFrontendLeaseTransitionTest.terrestrial()) {
                            engine.reset(SiDiscoveryProfile.ISDB_T)
                            ingest.resetForTune()
                        }.success,
                )
                val nextBaseline = ingest.transportLossSnapshot()
                deliverStale(tuner, staleFilter, staleHandle, sdt)
                awaitIdle(owner)
                assertTrue(engine.serviceRegistrationSnapshot().services.isEmpty())
                deliver(tuner, pat)
                awaitIdle(owner)
                tuner.updatePmtFilters(engine.pmtPidsForSectionFilters(), tuner.currentGeneration())
                replay(tuner, owner, sections)
                assertTrue(collectionComplete(engine))
                assertEquals(null, ChannelScanController.candidateTransportLossDetail(nextBaseline, ingest.transportLossSnapshot()))
                verifyServices(engine, expected)
                val report = File(checkNotNull(System.getProperty("realTs.diagnosticsDirectory")), "section-burst.txt")
                report.writeText(
                    "sha256=${expected.getString(
                        "sha256",
                    )} sections=${sections.size} filters=${sections.map { it.pid }.distinct().size}\n" +
                        "assertedExpectations: peakPending=16 rejected=${sections.size} filterOverflow=0\n" +
                        "observedHostWallTime: blockedAndDrainNs=$blockedNanos replayNs=$replayNanos\n" +
                        "assertedExpectations: beforeReplayComplete=false afterReplayComplete=true " +
                        "nextCandidateLoss=0 nextCandidateComplete=true\n" +
                        "scope=syntheticOwnerSaturationAndManualReplay; realScanDeadlineAndAutonomousRecovery=notTested\n",
                )
            } finally {
                tuner.release()
            }
        }
    }

    private fun collectionComplete(engine: AribSiEngine): Boolean {
        val snapshot = engine.serviceRegistrationSnapshot()
        val complete =
            SiCollectionRequirements(ChannelScanController.PublishMode.SETUP_SCAN, SiDiscoveryProfile.ISDB_T)
                .evaluate(snapshot)
                .complete
        val outcome =
            ChannelScanController.collectionOutcome(
                complete,
                2_000,
                1_200,
                snapshot.semanticFactsByServiceKey.size,
                ChannelScanController.SiCollectionPolicy(),
                allowStablePartial = false,
            )
        return outcome == ChannelScanController.SiCollectionOutcome.COMPLETE
    }

    // 既存ownerの読み取り専用観測。長い型名を一式で示す。
    @Suppress("MaxLineLength")
    private fun filters(tuner: TunerController): Map<TsPid, List<Filter>> = ReflectionHelpers.getField(tuner, "sectionFilters")

    private fun handles(tuner: TunerController): Map<TsPid, TunerController.SectionFilterHandle> =
        ReflectionHelpers.getField(tuner, "sectionFilterHandles")

    private fun deliver(
        tuner: TunerController,
        section: Section,
    ) {
        val filter = filters(tuner).getValue(TsPid(section.pid)).first()
        Shadow.extract<TunerFrontendLeaseTransitionTest.NativeFilter>(filter).offer(section.bytes)
        val event =
            ReflectionHelpers.callConstructor(
                SectionEvent::class.java,
                ReflectionHelpers.ClassParameter.from(Int::class.javaPrimitiveType, section.bytes[0].toInt() and 255),
                ReflectionHelpers.ClassParameter.from(Int::class.javaPrimitiveType, 0),
                ReflectionHelpers.ClassParameter.from(Int::class.javaPrimitiveType, 0),
                ReflectionHelpers.ClassParameter.from(Long::class.javaPrimitiveType, section.bytes.size.toLong()),
            )
        checkNotNull(TunerFrontendLeaseTransitionTest.NativeTuner.current)
            .filterCallbacks
            .getValue(filter)
            .onFilterEvent(filter, arrayOf(event))
    }

    private fun deliverStale(
        tuner: TunerController,
        filter: Filter,
        handle: TunerController.SectionFilterHandle,
        section: Section,
    ) {
        // close前にread済みだったbytesの後着。製品にテスト入口を追加しない。
        val method = TunerController::class.java.declaredMethods.single { it.name.startsWith("deliverSectionRead") }
        method.isAccessible = true
        method.invoke(tuner, filter, section.bytes, section.bytes.size, handle)
    }

    private fun replay(
        tuner: TunerController,
        owner: ControllerSerialExecutor,
        sections: List<Section>,
    ) {
        sections.forEach {
            deliver(tuner, it)
            awaitIdle(owner)
        }
    }

    private fun awaitIdle(owner: ControllerSerialExecutor) {
        val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(5)
        while (owner.queue.isNotEmpty() || owner.activeCount != 0) {
            check(System.nanoTime() < deadline) { "controllerが停止しません" }
            Thread.sleep(1)
        }
    }

    private fun runHal(
        input: File,
        expected: JSONObject,
        references: List<JSONObject>,
    ): List<Section> {
        val diagnostics = File(checkNotNull(System.getProperty("realTs.diagnosticsDirectory")))
        Files.createDirectories(diagnostics.toPath())
        val directory = Files.createTempDirectory(diagnostics.toPath(), "real-ts-").toFile()
        val output = File(directory, "sections.bin")
        val log = File(directory, "hal.log")
        val executable = File(checkNotNull(System.getProperty("realTs.executable")))
        check(executable.canExecute()) { "HAL試験実行器がありません: $executable" }
        val command =
            listOf(executable.absolutePath, input.absolutePath, output.absolutePath) +
                references.map { "${it.getInt("pid")}:${it.getInt("table_id")}" }
        val process = ProcessBuilder(command).redirectErrorStream(true).redirectOutput(log).start()
        val cleanup =
            AutoCloseable {
                if (process.isAlive) {
                    process.destroyForcibly()
                    check(process.waitFor(5, TimeUnit.SECONDS)) { "HAL試験プロセスを回収できません: $log" }
                }
            }
        cleanup.use {
            check(process.waitFor(30, TimeUnit.SECONDS)) { "HAL試験が時間切れです: $log" }
            assertEquals(log.readText(), 0, process.exitValue())
        }
        check(output.length() in 20..1048576) { "HAL出力の長さが不正です" }
        return readSections(output, expected, references.sumOf { it.getInt("section_count") }, references.size)
    }

    private fun readSections(
        file: File,
        expected: JSONObject,
        count: Int,
        filters: Int,
    ): List<Section> =
        DataInputStream(file.inputStream().buffered()).use { stream ->
            val sections =
                (0 until count).map { sequence ->
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

    private fun verifySections(
        sections: List<Section>,
        references: List<JSONObject>,
    ) {
        val groups = sections.groupBy { it.pid to (it.bytes.first().toInt() and 255) }
        assertEquals(references.map { it.getInt("pid") to it.getInt("table_id") }.toSet(), groups.keys)
        references.forEach { reference ->
            val key = reference.getInt("pid") to reference.getInt("table_id")
            val group = groups.getValue(key)
            assertEquals("$key セクション数", reference.getInt("section_count"), group.size)
            assertEquals("$key バイト数", reference.getInt("payload_bytes"), group.sumOf { it.bytes.size })
            val hashes =
                objects(reference.getJSONArray("unique_sections")).associate {
                    it.getString("sha256") to it.getInt("count")
                }
            assertEquals("$key セクション内容", hashes, group.groupingBy { sha256(it.bytes) }.eachCount())
        }
    }

    private fun verifyIngest(
        engine: AribSiEngine,
        sections: List<Section>,
    ) {
        val controller = SectionIngestController(engine)
        val ignored = mutableListOf<Pair<Int, Int>>()
        sections.forEach { section ->
            val status = controller.onSection(checkNotNull(TsPid.fromOrNull(section.pid)), section.bytes).status
            assertTrue(
                "pid=${section.pid} status=$status",
                status == SiStatus.OK || status == ignoredUnsupportedPidOrTable,
            )
            if (status == ignoredUnsupportedPidOrTable) {
                ignored += section.pid to (section.bytes.first().toInt() and 255)
            }
        }
        // 元TSの397番packetにPMTがあり、最初のPATは418番packetにある。
        assertEquals(listOf(257 to 2), ignored)
    }

    private fun verifyServices(
        engine: AribSiEngine,
        expected: JSONObject,
    ) {
        val builder = ServiceListBuilder(engine)
        val snapshot = builder.snapshot()
        val actual = snapshot.associateBy { it.serviceKey.serviceId }
        val services = objects(expected.getJSONArray("services"))
        assertEquals(services.size, snapshot.size)
        assertEquals(services.map { it.getInt("service_id") }.toSet(), actual.keys)
        services.forEach { reference ->
            val service = actual.getValue(reference.getInt("service_id"))
            assertEquals(expected.getInt("original_network_id"), service.serviceKey.originalNetworkId)
            assertEquals(expected.getInt("transport_stream_id"), service.serviceKey.transportStreamId)
            verifyService(service, reference)
        }
        val registration = engine.serviceRegistrationSnapshot()
        services.forEach { reference ->
            val service = actual.getValue(reference.getInt("service_id"))
            val facts = checkNotNull(registration.semanticFactsByServiceKey[service.serviceKey])
            assertEquals(reference.getBoolean("partial_reception"), facts.partialReception)
        }
        val broadcastSystem = ServicePolicyEvaluator.expectedSmdBroadcastSystem(SiDiscoveryProfile.ISDB_T)
        val ready =
            registration.services.filter { service ->
                val decision =
                    ServicePolicyEvaluator.evaluate(
                        facts = registration.semanticFactsByServiceKey[service.serviceKey],
                        fallbackKey = service.serviceKey,
                        expectedSmdBroadcastSystem = broadcastSystem,
                    )
                decision.registrationReady
            }
        assertEquals(setOf(1048, 1049, 1432), ready.map { it.serviceKey.serviceId }.toSet())
    }

    private fun verifyService(
        service: AribService,
        reference: JSONObject,
    ) {
        assertEquals(reference.getString("name"), service.name)
        assertEquals(reference.getInt("service_type"), service.serviceType)
        assertEquals(reference.getInt("pmt_pid"), service.pmtPid?.value)
        val pcr = if (reference.isNull("pcr_pid")) null else reference.getInt("pcr_pid")
        assertEquals(pcr, service.pcrPid?.value)
        assertEquals(reference.getBoolean("free_ca_mode"), service.freeCaMode)
        val streams =
            objects(reference.getJSONArray("elementary_streams"))
                .map { it.getInt("pid") to it.getInt("stream_type") }
                .sortedBy { it.first }
        assertEquals(streams, service.streams.map { it.elementaryPid.value to it.streamType }.sortedBy { it.first })
        val descriptors =
            objects(reference.getJSONArray("program_ca_descriptors")).map {
                it.getInt("ca_system_id") to it.getInt("ca_pid")
            }
        val actualDescriptors =
            service.serviceScopedCaDescriptors
                .filter { it.scope == CaDescriptorScope.PROGRAM }
                .map { it.caSystemId to it.caPid?.value }
        assertEquals(descriptors, actualDescriptors)
    }

    private fun verifyPrograms(
        engine: AribSiEngine,
        expected: JSONObject,
    ) {
        val events =
            engine.livePlaybackSnapshot().programs.events.filter {
                it.serviceKey.serviceId == expected.getInt("selected_service_id") && it.source.tableId == 0x4e
            }
        val references = objects(expected.getJSONArray("selected_service_pf_events"))
        assertEquals(references.map { it.getInt("event_id") }.sorted(), events.map { it.eventId }.sorted())
        references.forEach { reference ->
            val event = events.single { it.eventId == reference.getInt("event_id") }
            assertEquals(expected.getInt("original_network_id"), event.serviceKey.originalNetworkId)
            assertEquals(expected.getInt("transport_stream_id"), event.serviceKey.transportStreamId)
            assertEquals(reference.getLong("start_time_unix_ms"), event.startTimeMillis)
            assertEquals(reference.getLong("duration_ms"), event.durationMillis)
        }
    }

    private fun objects(array: JSONArray): List<JSONObject> = (0 until array.length()).map(array::getJSONObject)

    private fun sha256(bytes: ByteArray): String =
        MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { "%02x".format(it.toInt() and 255) }
}

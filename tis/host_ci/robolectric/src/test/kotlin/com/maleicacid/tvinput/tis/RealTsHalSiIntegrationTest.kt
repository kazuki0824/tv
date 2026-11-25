// テストの入力・期待値と試験専用の入出力形式を具体値で照合する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import android.content.ContentProvider
import android.content.ContentUris
import android.content.ContentValues
import android.content.Context
import android.content.pm.ProviderInfo
import android.database.MatrixCursor
import android.media.tv.TvContract
import android.media.tv.tuner.filter.Filter
import android.media.tv.tuner.filter.SectionEvent
import android.net.Uri
import androidx.test.core.app.ApplicationProvider
import com.maleicacid.tvinput.aribsi.AribRatingMapper
import com.maleicacid.tvinput.aribsi.AribService
import com.maleicacid.tvinput.aribsi.AribSiEngine
import com.maleicacid.tvinput.aribsi.CaDescriptorScope
import com.maleicacid.tvinput.aribsi.SectionIngestController
import com.maleicacid.tvinput.aribsi.ServiceListBuilder
import com.maleicacid.tvinput.aribsi.ServicePolicyEvaluator
import com.maleicacid.tvinput.aribsi.SiDiscoveryProfile
import com.maleicacid.tvinput.aribsi.SiStatus
import com.maleicacid.tvinput.common.ServiceKey
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
import org.robolectric.shadows.ShadowContentResolver
import org.robolectric.util.ReflectionHelpers
import java.io.DataInputStream
import java.io.File
import java.nio.ByteBuffer
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
        verifyLiveProgramPublication(sections, expected)
    }

    // 実TS取得を既存試験と共用し、製品Sessionの公開入口からProvider境界まで検査する。
    @Suppress("LongMethod")
    private fun verifyLiveProgramPublication(
        sections: List<Section>,
        expected: JSONObject,
    ) {
        val key =
            ServiceKey(
                expected.getInt("original_network_id"),
                expected.getInt("transport_stream_id"),
                expected.getInt("selected_service_id"),
            )
        val context = ApplicationProvider.getApplicationContext<Context>()
        val provider = LiveProgramProvider(key)
        provider.attachInfo(context, ProviderInfo().apply { authority = TvContract.AUTHORITY })
        ShadowContentResolver.registerProviderInternal(TvContract.AUTHORITY, provider)
        val session = MaleicacidLiveSession(context, context, "real-ts-test-input", "real-ts-program")
        val executor = ReflectionHelpers.getField<LifecycleSerialExecutor>(session, "sessionExecutor")
        try {
            executor.callControl(15_000L) {
                val engine = ReflectionHelpers.getField<AribSiEngine>(session, "aribSiEngine")
                engine.setDiscoveryProfile(SiDiscoveryProfile.ISDB_T)
                verifyIngest(engine, sections)
                verifyServices(engine, expected)
                verifyPrograms(engine, expected)
                ReflectionHelpers.setField(session, "currentService", key)
                ReflectionHelpers.setField(
                    session,
                    "currentRatingProfile",
                    AribRatingMapper.profileForDeliverySystem("ISDB_T"),
                )
                val ready = engine.livePlaybackSnapshot()
                assertTrue(ServicePolicyEvaluator.evaluateLive(ready, key).registrationReady)
                ReflectionHelpers.setField(session, "latestLiveSnapshot", ready)
                ReflectionHelpers.callInstanceMethod<Unit>(session, "publishLiveProgramsForCurrentService")
            }
            val present =
                objects(expected.getJSONArray("selected_service_pf_events"))
                    .minBy { it.getLong("start_time_unix_ms") }
            val start = present.getLong("start_time_unix_ms")
            val end = start + present.getLong("duration_ms")
            val fixtureNow = start + 1
            assertTrue(provider.rows.all { it.getAsLong(TvContract.Programs.COLUMN_CHANNEL_ID) == 7L })
            val current =
                provider.rows.single {
                    it.getAsLong(TvContract.Programs.COLUMN_START_TIME_UTC_MILLIS) <= fixtureNow &&
                        fixtureNow < it.getAsLong(TvContract.Programs.COLUMN_END_TIME_UTC_MILLIS)
                }
            assertEquals(present.getInt("event_id"), current.getAsInteger(TvContract.Programs.COLUMN_EVENT_ID).toInt())
            assertEquals(start, current.getAsLong(TvContract.Programs.COLUMN_START_TIME_UTC_MILLIS).toLong())
            assertEquals(end, current.getAsLong(TvContract.Programs.COLUMN_END_TIME_UTC_MILLIS).toLong())
            // 元section 0のshort_event_descriptorに含まれる固定の番組名。
            assertTrue(current.getAsString(TvContract.Programs.COLUMN_TITLE).orEmpty().contains("名古屋"))
            val scheduleStart = start + TimeUnit.MINUTES.toMillis(390)
            // 別局には前回取得したp/fの2枠だけが保存されている状態から始める。
            seedPriorProgram(context, 101, "別局の現在番組", start, end)
            seedPriorProgram(context, 102, "別局の次番組", end, scheduleStart)
            assertEquals(null, queryCurrentProgram(context, scheduleStart + 1))
            executor.callControl(15_000L) {
                val engine = ReflectionHelpers.getField<AribSiEngine>(session, "aribSiEngine")
                val ingest = SectionIngestController(engine)
                for (nextDay in listOf(false, true)) {
                    val schedule = otherTransportSchedule(sections, nextDay)
                    assertEquals(SiStatus.OK, ingest.onSection(TsPid.EIT, schedule).status)
                }
                ReflectionHelpers.setField(session, "latestLiveSnapshot", engine.livePlaybackSnapshot())
                ReflectionHelpers.callInstanceMethod<Unit>(session, "publishLiveProgramsForCurrentService")
            }
            // 同一session・同一選局のまま照会時刻を進める。この間の再受信・再公開はない。
            assertEquals(101 to "別局の現在番組", queryCurrentProgram(context, start + 1))
            assertEquals(102 to "別局の次番組", queryCurrentProgram(context, scheduleStart - 1))
            val title = current.getAsString(TvContract.Programs.COLUMN_TITLE)
            assertEquals(0xba00 to title, queryCurrentProgram(context, scheduleStart + TimeUnit.MINUTES.toMillis(30)))
            assertEquals(null, queryCurrentProgram(context, scheduleStart + present.getLong("duration_ms") + 1))
            val tomorrow = provider.rows.single { it.getAsInteger(TvContract.Programs.COLUMN_EVENT_ID) == 0xba01 }
            assertEquals(9L, tomorrow.getAsLong(TvContract.Programs.COLUMN_CHANNEL_ID).toLong())
            val tomorrowStart = tomorrow.getAsLong(TvContract.Programs.COLUMN_START_TIME_UTC_MILLIS).toLong()
            assertEquals(start + TimeUnit.DAYS.toMillis(1), tomorrowStart)
            val titleColumn = TvContract.Programs.COLUMN_TITLE
            assertEquals(current.getAsString(titleColumn), tomorrow.getAsString(titleColumn))
        } finally {
            session.onRelease()
            check(executor.awaitTermination(5, TimeUnit.SECONDS))
        }
    }

    private fun seedPriorProgram(
        context: Context,
        event: Int,
        title: String,
        start: Long,
        end: Long,
    ) {
        val values =
            ContentValues().apply {
                put(TvContract.Programs.COLUMN_CHANNEL_ID, 9L)
                put(TvContract.Programs.COLUMN_EVENT_ID, event)
                put(TvContract.Programs.COLUMN_TITLE, title)
                put(TvContract.Programs.COLUMN_START_TIME_UTC_MILLIS, start)
                put(TvContract.Programs.COLUMN_END_TIME_UTC_MILLIS, end)
            }
        checkNotNull(context.contentResolver.insert(TvContract.Programs.CONTENT_URI, values))
    }

    // Live TVのProgramDataManagerと同じchannel/time/time URIで、Providerが返す現在枠を読む。
    private fun queryCurrentProgram(
        context: Context,
        time: Long,
    ): Pair<Int, String>? {
        val uri = TvContract.buildProgramsUriForChannel(9L, time, time)
        val columns = arrayOf(TvContract.Programs.COLUMN_EVENT_ID, TvContract.Programs.COLUMN_TITLE)
        return checkNotNull(context.contentResolver.query(uri, columns, null, null, null)).use { cursor ->
            if (!cursor.moveToFirst()) return@use null
            val program = cursor.getInt(0) to cursor.getString(1)
            check(!cursor.moveToNext()) { "非視聴局の現在枠が重複しています" }
            program
        }
    }

    // fixtureのp/f sectionを基に、別TSの当日夜と翌日の部分scheduleを生成する。放送実測のscheduleとは区別する。
    private fun otherTransportSchedule(
        sections: List<Section>,
        nextDay: Boolean,
    ): ByteArray {
        val bytes =
            sections
                .first { section ->
                    val data = ByteBuffer.wrap(section.bytes)
                    section.pid == TsPid.EIT.value && data.get(0).toInt() == 0x4e &&
                        data.getShort(3).toInt() == 1048 && data.get(6).toInt() == 0
                }.bytes
                .copyOf()
        val data = ByteBuffer.wrap(bytes)
        data.put(0, 0x60)
        data.put(13, 0x60)
        data.putShort(3, 1049)
        data.putShort(8, 0x7777)
        val sectionNumber: Byte = if (nextDay) 104 else 56
        data.put(6, sectionNumber)
        data.put(7, 104)
        data.put(12, sectionNumber)
        if (nextDay) {
            data.putShort(14, 0xba01.toShort())
            data.putShort(16, (data.getShort(16) + 1).toShort())
        } else {
            data.putShort(14, 0xba00.toShort())
            data.put(18, 0x23)
            data.put(19, 0x30)
        }
        var crc = -1
        for (i in 0 until bytes.size - 4) {
            crc = crc xor ((bytes[i].toInt() and 255) shl 24)
            repeat(8) { crc = if (crc < 0) (crc shl 1) xor 0x04c11db7 else crc shl 1 }
        }
        data.putInt(bytes.size - 4, crc)
        return bytes
    }

    private class LiveProgramProvider(
        private val key: ServiceKey,
    ) : ContentProvider() {
        val rows = mutableListOf<ContentValues>()

        override fun onCreate() = true

        override fun query(
            uri: Uri,
            projection: Array<out String>?,
            selection: String?,
            selectionArgs: Array<out String>?,
            sortOrder: String?,
        ): MatrixCursor =
            MatrixCursor(checkNotNull(projection)).apply {
                if (uri.pathSegments.first() == "channel") {
                    addRow(arrayOf(7L, key.originalNetworkId, key.transportStreamId, key.serviceId))
                    addRow(arrayOf(8L, key.originalNetworkId, key.transportStreamId, 1049))
                    addRow(arrayOf(9L, key.originalNetworkId, 0x7777, 1049))
                } else if (uri.pathSegments.first() == "program") {
                    addCurrentPrograms(uri, this)
                }
            }

        private fun addCurrentPrograms(
            uri: Uri,
            cursor: MatrixCursor,
        ) {
            val start = uri.getQueryParameter("start_time")?.toLong() ?: return
            if (start != uri.getQueryParameter("end_time")?.toLong()) return
            val channel = uri.getQueryParameter("channel")?.toLong()
            rows
                .filter { row ->
                    row.getAsLong(TvContract.Programs.COLUMN_CHANNEL_ID) == channel &&
                        row.getAsLong(TvContract.Programs.COLUMN_START_TIME_UTC_MILLIS) <= start &&
                        start < row.getAsLong(TvContract.Programs.COLUMN_END_TIME_UTC_MILLIS)
                }.forEach { row -> cursor.addRow(cursor.columnNames.map { row.get(it) }.toTypedArray()) }
        }

        override fun getType(uri: Uri): String? = null

        override fun insert(
            uri: Uri,
            values: ContentValues?,
        ): Uri {
            assertEquals(TvContract.Programs.CONTENT_URI, uri)
            rows += ContentValues(checkNotNull(values))
            return ContentUris.withAppendedId(uri, rows.size.toLong())
        }

        override fun update(
            uri: Uri,
            values: ContentValues?,
            selection: String?,
            selectionArgs: Array<out String>?,
        ) = 0

        override fun delete(
            uri: Uri,
            selection: String?,
            selectionArgs: Array<out String>?,
        ) = 0
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
        // 同じ実TS内のCRC正常なscheduleを含め、製品HALの出力を放送順に受け取る。
        val scheduleCounts = mapOf(0x50 to 38, 0x51 to 11, 0x58 to 40, 0x59 to 5)
        val scheduleReferences =
            scheduleCounts.map { (table, count) ->
                JSONObject().put("pid", 18).put("table_id", table).put("section_count", count)
            }
        val sections = runHal(input, expected, references + scheduleReferences)
        val (schedule, original) = sections.partition { it.pid == 18 && it.bytes.first().toInt() in scheduleCounts }
        assertEquals(scheduleCounts, schedule.groupingBy { it.bytes.first().toInt() }.eachCount())
        verifySections(original, references)
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
                    schedule.take(16).forEach { deliver(tuner, it) }
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
                        "scheduleCounts=$scheduleCounts metrics=${owner.metricsSnapshot()}\n" +
                        "assertedExpectations: peakPending=16 rejected=${sections.size} filterOverflow=0\n" +
                        "observedHostWallTime: blockedAndDrainNs=$blockedNanos replayNs=$replayNanos\n" +
                        "assertedExpectations: beforeReplayComplete=false afterReplayComplete=true " +
                        "nextCandidateLoss=0 nextCandidateComplete=true\n" +
                        "scope=syntheticOwnerSaturationAndManualReplay; realScanDeadlineAndAutonomousRecovery=notTested\n",
                )
                // ownerを停止せず放送順に連続配送し、p/f限定とschedule混在を同じ入力で比較する。
                for ((label, stream) in listOf("pf" to original, "mixed" to sections)) {
                    check(
                        tuner
                            .tuneForScan(TunerFrontendLeaseTransitionTest.terrestrial()) {
                                engine.reset(SiDiscoveryProfile.ISDB_T)
                                ingest.resetForTune()
                            }.success,
                    )
                    deliver(tuner, pat)
                    awaitIdle(owner)
                    tuner.updatePmtFilters(engine.pmtPidsForSectionFilters(), tuner.currentGeneration())
                    val window = tuner.beginSectionWindow()
                    try {
                        stream.forEach { deliver(tuner, it) }
                        awaitIdle(owner)
                    } finally {
                        report.appendText("$label ${tuner.finishSectionMetricsWindow(window)}\n")
                    }
                    val completeBeforeReplay = collectionComplete(engine)
                    report.appendText("$label loss=${ingest.transportLossSnapshot()} completeBeforeReplay=$completeBeforeReplay\n")
                    replay(tuner, owner, stream)
                    assertTrue(collectionComplete(engine))
                    report.appendText("$label completeAfterReplay=true\n")
                }
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
        val reached = java.util.concurrent.CountDownLatch(1)
        // dataと同じ優先度の末尾へ入れ、dequeueと実行開始の間も完了と誤認しない。
        while (true) {
            try {
                owner.executeData { reached.countDown() }
                break
            } catch (_: DataCapacityExceededException) {
                check(System.nanoTime() < deadline) { "controllerのdata受付が回復しません" }
                Thread.sleep(1)
            }
        }
        check(reached.await((deadline - System.nanoTime()).coerceAtLeast(0), TimeUnit.NANOSECONDS)) {
            "controllerのdata処理が完了しません"
        }
        // marker自身のpermit返却も完了させてから、次の容量試験へ進む。
        owner.submitControl { }.get(5, TimeUnit.SECONDS)
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

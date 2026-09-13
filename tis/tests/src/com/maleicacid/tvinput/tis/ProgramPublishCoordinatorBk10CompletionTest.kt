// テストの入力・期待値を本体の定数と独立した具体値で記述する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import android.content.ContentValues
import android.media.tv.TvContract
import com.maleicacid.tvinput.common.FrequencyHz
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.db.ChannelRecord
import com.maleicacid.tvinput.db.ProgramRecord
import org.junit.Test

// 一つの契約の試験集合・時系列を保持し、検証シナリオを分断しない。
@Suppress("TooManyFunctions")
class ProgramPublishCoordinatorBk10CompletionTest {
    private val key = ServiceKey(4, 16625, 101)
    private val program =
        ProgramRecord(
            serviceKey = key,
            eventId = 10,
            stableIdentity =
                "{\"kind\":\"arib-event-v1\",\"originalNetworkId\":4,\"transportStreamId\":1" +
                    "6625,\"serviceId\":101,\"eventId\":10}",
            startTimeMillis = 1_700_000_000_000L,
            durationMillis = 1_800_000L,
            title = "News",
            description = "desc",
            casFactsCanonicalJson =
                com.maleicacid.tvinput.tis
                    .testCasFacts(false),
        )

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    @Test
    fun verifiedEmptyEitCommitsOnlyAfterQueriesAndPreservesExistingRows() {
        val store = FakeStore()
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        val coordinator = ProgramPublishCoordinator(writer)
        writer.upsertChannels(
            listOf(
                ChannelRecord(
                    key,
                    0x01,
                    "101",
                    "NHK",
                    FrequencyHz(473_142_857L),
                    casFactsCanonicalJson =
                        com.maleicacid.tvinput.tis
                            .testCasFacts(false),
                ),
            ),
        )
        check(coordinator.publish(ChannelScanController.PublishMode.BOOT_EPG_SYNC, listOf(program), setOf(key)).hasCommittedTarget)
        val before = store.serviceIndexQueries
        repeat(2) {
            val result =
                coordinator.publishWithUpdates(
                    ChannelScanController.PublishMode.BOOT_EPG_SYNC,
                    emptyList(),
                    emptyList(),
                    setOf(key),
                    verifiedEmptyServiceKeys = setOf(key),
                )
            check(result.hasCommittedTarget && result.committedServiceKeys == setOf(key))
            check(result.changed == 0)
        }
        check(store.serviceIndexQueries == before + 2)
        check(store.insertedPrograms == 1 && store.updatedPrograms == 0 && store.deleteCalls == 0)
    }

    @Test fun verifiedEmptyEitDoesNotCommitARequiredProgramQueryFailure() {
        val store = FakeStore(failServiceIndexOnce = true)
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        val coordinator = ProgramPublishCoordinator(writer)
        writer.upsertChannels(
            listOf(
                ChannelRecord(
                    key,
                    0x01,
                    "101",
                    "NHK",
                    FrequencyHz(473_142_857L),
                    casFactsCanonicalJson =
                        com.maleicacid.tvinput.tis
                            .testCasFacts(false),
                ),
            ),
        )

        fun publish() =
            coordinator.publishWithUpdates(
                ChannelScanController.PublishMode.BOOT_EPG_SYNC,
                emptyList(),
                emptyList(),
                setOf(key),
                verifiedEmptyServiceKeys = setOf(key),
            )
        val failed = publish()
        check(!failed.hasCommittedTarget && failed.committedServiceKeys.isEmpty())
        check(failed.failures.single().operation == "program-index-query")
        check(publish().hasCommittedTarget)
        check(store.serviceIndexQueries == 2 && store.deleteCalls == 0)
    }

    @Test fun emptyEitForANonexistentOwnedChannelIsNotACommitTarget() {
        val store = FakeStore()
        val coordinator = ProgramPublishCoordinator(TvProviderWriter("input.test", store, testOnly = true))
        val result =
            coordinator.publishWithUpdates(
                ChannelScanController.PublishMode.BOOT_EPG_SYNC,
                emptyList(),
                emptyList(),
                setOf(key),
                verifiedEmptyServiceKeys = setOf(key),
            )
        check(!result.hasCommittedTarget && result.committedServiceKeys.isEmpty())
        check(store.serviceIndexQueries == 0 && store.deleteCalls == 0)
    }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    @Test
    fun requiredQueryFailureDoesNotUpdateSignatureOrDeleteAndNextSuccessPublishes() {
        val store = FakeStore(failWindowIndexOnce = true)
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        val coordinator = ProgramPublishCoordinator(writer)
        writer.upsertChannels(
            listOf(
                ChannelRecord(
                    key,
                    0x01,
                    "101",
                    "NHK",
                    FrequencyHz(473_142_857L),
                    casFactsCanonicalJson =
                        com.maleicacid.tvinput.tis
                            .testCasFacts(false),
                ),
            ),
        )

        val first =
            coordinator.publish(
                mode = ChannelScanController.PublishMode.LIVE_TUNE_REFRESH,
                allPrograms = listOf(program),
                allowedServiceKeys = null,
            )
        check(first.failures.isNotEmpty()) { first.toString() }
        check(store.insertedPrograms == 0) { "必須問い合わせ失敗時は挿入を止める必要があります" }
        check(store.deleteCalls == 0) { "必須問い合わせ失敗時は廃止行を削除してはなりません" }
        check(coordinator.retryFailureClassesForTest().contains(ProgramPublishCoordinator.FailureClass.REQUIRED_QUERY_FAILED))

        val second =
            coordinator.publish(
                mode = ChannelScanController.PublishMode.LIVE_TUNE_REFRESH,
                allPrograms = listOf(program),
                allowedServiceKeys = null,
            )
        check(second.inserted == 1) { second.toString() }
        check(coordinator.retryWindowCountForTest() == 1) { "通常upsert成功だけではauthoritative再検証要求を消去してはなりません" }
    }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    @Test
    fun failedInsertDoesNotCommitSignatureSoRetryCanPublishSameInput() {
        val store = FakeStore(failInsertOnce = true)
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        val coordinator = ProgramPublishCoordinator(writer)
        writer.upsertChannels(
            listOf(
                ChannelRecord(
                    key,
                    0x01,
                    "101",
                    "NHK",
                    FrequencyHz(473_142_857L),
                    casFactsCanonicalJson =
                        com.maleicacid.tvinput.tis
                            .testCasFacts(false),
                ),
            ),
        )

        val failed = coordinator.publish(ChannelScanController.PublishMode.SETUP_SCAN, listOf(program), allowedServiceKeys = null)
        check(failed.failures.any { it.operation == "program-insert" }) { failed.toString() }
        check(coordinator.retryFailureClassesForTest().contains(ProgramPublishCoordinator.FailureClass.PROGRAM_INSERT_FAILED))

        val retried = coordinator.publish(ChannelScanController.PublishMode.SETUP_SCAN, listOf(program), allowedServiceKeys = null)
        check(retried.inserted == 1) { "公開失敗時は同じ入力を未変更扱いしてはなりません: $retried" }
    }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    @Test
    fun bootSyncDoesNotTreatFingerprintCacheAsCurrentTaskCommit() {
        val store = FakeStore()
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        val coordinator = ProgramPublishCoordinator(writer)
        writer.upsertChannels(
            listOf(
                ChannelRecord(
                    key,
                    0x01,
                    "101",
                    "NHK",
                    FrequencyHz(473_142_857L),
                    casFactsCanonicalJson =
                        com.maleicacid.tvinput.tis
                            .testCasFacts(false),
                ),
            ),
        )

        val first = coordinator.publish(ChannelScanController.PublishMode.BOOT_EPG_SYNC, listOf(program), allowedServiceKeys = setOf(key))
        val second = coordinator.publish(ChannelScanController.PublishMode.BOOT_EPG_SYNC, listOf(program), allowedServiceKeys = setOf(key))

        check(first.inserted == 1 && first.hasCommittedTarget)
        check(second.updated == 1 && second.hasCommittedTarget)
        check(store.updatedPrograms == 1)
    }

    @Test fun nonAuthoritativeWindowWithoutProgramIsNotABootCommitTarget() {
        val store = FakeStore()
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        val coordinator = ProgramPublishCoordinator(writer)
        writer.upsertChannels(
            listOf(
                ChannelRecord(
                    key,
                    0x01,
                    "101",
                    "NHK",
                    FrequencyHz(473_142_857L),
                    casFactsCanonicalJson =
                        com.maleicacid.tvinput.tis
                            .testCasFacts(false),
                ),
            ),
        )
        val window =
            ProgramPublishCoordinator.EpgUpdateWindow(
                serviceKey = key,
                windowStartMs = program.startTimeMillis,
                windowEndMs = program.startTimeMillis + program.durationMillis,
                validProgramKeys = setOf(TvProviderWriter.programKeyForTest(program)),
                deletionAuthoritative = false,
            )

        val result =
            coordinator.publishWithUpdates(
                mode = ChannelScanController.PublishMode.BOOT_EPG_SYNC,
                allPrograms = emptyList(),
                updateWindows = listOf(window),
                allowedServiceKeys = setOf(key),
            )

        check(!result.hasCommittedTarget)
        check(result.eligibleTargetCount == 0)
    }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    @Test
    fun authoritativeDeleteFailureIsRetriedWithObsoleteDeleteClass() {
        val store = FakeStore(failDelete = true)
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        val coordinator = ProgramPublishCoordinator(writer)
        writer.upsertChannels(
            listOf(
                ChannelRecord(
                    key,
                    0x01,
                    "101",
                    "NHK",
                    FrequencyHz(473_142_857L),
                    casFactsCanonicalJson =
                        com.maleicacid.tvinput.tis
                            .testCasFacts(false),
                ),
            ),
        )
        writer.upsertPrograms(listOf(program))

        val window =
            ProgramPublishCoordinator.EpgUpdateWindow(
                serviceKey = key,
                windowStartMs = program.startTimeMillis,
                windowEndMs = program.startTimeMillis + program.durationMillis,
                validProgramKeys = emptySet(),
                deletionAuthoritative = true,
            )
        val result =
            coordinator.publishWithUpdates(
                mode = ChannelScanController.PublishMode.SETUP_SCAN,
                allPrograms = emptyList(),
                updateWindows = listOf(window),
                allowedServiceKeys = null,
            )
        check(result.failures.any { it.operation == "program-delete-obsolete" }) { result.toString() }
        check(coordinator.retryFailureClassesForTest().contains(ProgramPublishCoordinator.FailureClass.OBSOLETE_DELETE_FAILED))
    }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    @Test
    fun retryUsesOneFixedCooldownAndKeepsFailureClassDiagnosticOnly() {
        val store = FakeStore(failDelete = true)
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        val coordinator = ProgramPublishCoordinator(writer)
        writer.upsertChannels(
            listOf(
                ChannelRecord(
                    key,
                    0x01,
                    "101",
                    "NHK",
                    FrequencyHz(473_142_857L),
                    casFactsCanonicalJson =
                        com.maleicacid.tvinput.tis
                            .testCasFacts(false),
                ),
            ),
        )
        val window =
            ProgramPublishCoordinator.EpgUpdateWindow(
                serviceKey = key,
                windowStartMs = program.startTimeMillis,
                windowEndMs = program.startTimeMillis + program.durationMillis,
                validProgramKeys = emptySet(),
                deletionAuthoritative = true,
            )
        val before = System.currentTimeMillis()
        coordinator.publishWithUpdates(
            mode = ChannelScanController.PublishMode.SETUP_SCAN,
            allPrograms = emptyList(),
            updateWindows = listOf(window),
            allowedServiceKeys = null,
        )
        val after = System.currentTimeMillis()

        val notBefore = coordinator.retryNotBeforeMillisForTest().single()
        check(notBefore >= before + ProgramPublishCoordinator.RETRY_COOLDOWN_MS_FOR_TEST)
        check(notBefore <= after + ProgramPublishCoordinator.RETRY_COOLDOWN_MS_FOR_TEST)
        check(coordinator.retryFailureClassesForTest() == setOf(ProgramPublishCoordinator.FailureClass.OBSOLETE_DELETE_FAILED))
    }

    @Test fun dirtyWindowQueueHasOneBoundedLruLimit() {
        val store = FakeStore(failDelete = true)
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        val coordinator = ProgramPublishCoordinator(writer)
        writer.upsertChannels(
            listOf(
                ChannelRecord(
                    key,
                    0x01,
                    "101",
                    "NHK",
                    FrequencyHz(473_142_857L),
                    casFactsCanonicalJson =
                        com.maleicacid.tvinput.tis
                            .testCasFacts(false),
                ),
            ),
        )

        val windows =
            (0 until (ProgramPublishCoordinator.MAX_DIRTY_WINDOWS_FOR_TEST + 1)).map { i ->
                ProgramPublishCoordinator.EpgUpdateWindow(
                    serviceKey = key,
                    windowStartMs = program.startTimeMillis + i * 60_000L,
                    windowEndMs = program.startTimeMillis + i * 60_000L + 30_000L,
                    validProgramKeys = emptySet(),
                    deletionAuthoritative = true,
                )
            }
        coordinator.publishWithUpdates(
            mode = ChannelScanController.PublishMode.SETUP_SCAN,
            allPrograms = emptyList(),
            updateWindows = windows,
            allowedServiceKeys = null,
        )
        check(coordinator.retryWindowCountForTest() == ProgramPublishCoordinator.MAX_DIRTY_WINDOWS_FOR_TEST) {
            "再試行区間は単一の有界LRUに収める必要があります"
        }
        check(coordinator.droppedRetryWindowCountForTest(key) == 1)
    }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    @Test
    fun retryRevalidatesKeysAndHoldsWithoutCurrentCompleteEit() {
        val store = FakeStore(failDelete = true)
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        var now = 1_000L
        val coordinator = ProgramPublishCoordinator(writer) { now }
        writer.upsertChannels(
            listOf(ChannelRecord(key, 1, "101", "test", FrequencyHz(473_142_857L), casFactsCanonicalJson = testCasFacts())),
        )
        val old =
            ProgramPublishCoordinator.EpgUpdateWindow(
                key,
                program.startTimeMillis,
                program.startTimeMillis + program.durationMillis,
                setOf("old-key"),
                true,
            )
        coordinator.publishWithUpdates(ChannelScanController.PublishMode.SETUP_SCAN, emptyList(), listOf(old), setOf(key))
        check(store.deleteCalls == 1 && coordinator.retryWindowCountForTest() == 1)
        now += ProgramPublishCoordinator.RETRY_COOLDOWN_MS_FOR_TEST
        repeat(2) {
            val held = coordinator.publishWithUpdates(ChannelScanController.PublishMode.SETUP_SCAN, emptyList(), emptyList(), setOf(key))
            check(!held.hasCommittedTarget && store.deleteCalls == 1)
            check(coordinator.retryWindowCountForTest() == 1)
        }
        store.failDelete = false
        val retried =
            coordinator.publishWithUpdates(
                ChannelScanController.PublishMode.SETUP_SCAN,
                emptyList(),
                listOf(old.copy(validProgramKeys = setOf("current-key"))),
                setOf(key),
            )
        check(retried.hasCommittedTarget && store.deleteCalls == 2)
        check(store.lastDeleteKeys == setOf("current-key"))
        check(coordinator.retryWindowCountForTest() == 0)
    }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    @Test
    fun nonAuthoritativeSuccessKeepsFailedDeleteUntilAuthoritativeSuccess() {
        val store = FakeStore(failDelete = true)
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        var now = 1_000L
        val coordinator = ProgramPublishCoordinator(writer) { now }
        writer.upsertChannels(
            listOf(ChannelRecord(key, 1, "101", "test", FrequencyHz(473_142_857L), casFactsCanonicalJson = testCasFacts())),
        )
        val currentKey = TvProviderWriter.programKeyForTest(program)
        val old =
            ProgramPublishCoordinator.EpgUpdateWindow(
                key,
                program.startTimeMillis,
                program.startTimeMillis + program.durationMillis,
                setOf("old-key"),
                true,
            )
        val failed = coordinator.publishWithUpdates(ChannelScanController.PublishMode.SETUP_SCAN, emptyList(), listOf(old), setOf(key))
        check(failed.failures.any { it.operation == "program-delete-obsolete" })
        check(store.deleteCalls == 1 && coordinator.retryWindowCountForTest() == 1)
        now += ProgramPublishCoordinator.RETRY_COOLDOWN_MS_FOR_TEST
        store.failDelete = false
        val partial =
            coordinator.publishWithUpdates(
                ChannelScanController.PublishMode.SETUP_SCAN,
                listOf(program),
                listOf(old.copy(validProgramKeys = setOf(currentKey), deletionAuthoritative = false)),
                setOf(key),
            )
        check(partial.failures.isEmpty() && partial.inserted == 1)
        check(store.deleteCalls == 1 && coordinator.retryWindowCountForTest() == 1)
        val complete =
            coordinator.publishWithUpdates(
                ChannelScanController.PublishMode.SETUP_SCAN,
                listOf(program),
                listOf(old.copy(validProgramKeys = setOf(currentKey))),
                setOf(key),
            )
        check(complete.failures.isEmpty() && complete.hasCommittedTarget)
        check(store.deleteCalls == 2 && store.lastDeleteKeys == setOf(currentKey))
        check(coordinator.retryWindowCountForTest() == 0)
    }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    @Test
    fun invalidProviderDataCannotWriteDeleteOrCommitFingerprint() {
        val store = FakeStore()
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        val coordinator = ProgramPublishCoordinator(writer)
        writer.upsertChannels(
            listOf(ChannelRecord(key, 1, "101", "test", FrequencyHz(473_142_857L), casFactsCanonicalJson = testCasFacts())),
        )
        val invalid = program.copy(casFactsCanonicalJson = null)
        val window =
            ProgramPublishCoordinator.EpgUpdateWindow(
                key,
                program.startTimeMillis,
                program.startTimeMillis + program.durationMillis,
                emptySet(),
                true,
            )
        val failed =
            coordinator.publishWithUpdates(
                ChannelScanController.PublishMode.BOOT_EPG_SYNC,
                listOf(invalid),
                listOf(window),
                setOf(key),
            )
        check(
            failed.failures
                .single()
                .message
                .contains("PROGRAM_REQUEST_INVALID"),
        )
        check(!failed.hasCommittedTarget && store.insertedPrograms == 0 && store.deleteCalls == 0)
        val valid =
            coordinator.publishWithUpdates(
                ChannelScanController.PublishMode.BOOT_EPG_SYNC,
                listOf(program),
                listOf(window.copy(validProgramKeys = setOf(TvProviderWriter.programKeyForTest(program)))),
                setOf(key),
            )
        check(valid.hasCommittedTarget && store.insertedPrograms == 1)
    }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    @Test
    fun retryWaitsUntilCurrentWindowCoversTheWholeOldRequest() {
        val store = FakeStore(failDelete = true)
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        var now = 1_000L
        val coordinator = ProgramPublishCoordinator(writer) { now }
        writer.upsertChannels(
            listOf(ChannelRecord(key, 1, "101", "test", FrequencyHz(473_142_857L), casFactsCanonicalJson = testCasFacts())),
        )
        val old = ProgramPublishCoordinator.EpgUpdateWindow(key, 100, 200, setOf("old-key"), true)

        // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
        @Suppress("MaxLineLength")
        fun publish(window: ProgramPublishCoordinator.EpgUpdateWindow) =
            coordinator.publishWithUpdates(ChannelScanController.PublishMode.SETUP_SCAN, emptyList(), listOf(window), setOf(key))
        check(publish(old).failures.isNotEmpty())
        now += ProgramPublishCoordinator.RETRY_COOLDOWN_MS_FOR_TEST
        store.failDelete = false
        for ((start, end) in listOf(300L to 400L, 150L to 250L)) {
            val before = store.deletedWindows.size
            check(publish(old.copy(windowStartMs = start, windowEndMs = end, validProgramKeys = setOf("new-key"))).hasCommittedTarget)
            check(store.deletedWindows.drop(before) == listOf(start to end))
            check(coordinator.retryWindowCountForTest() == 1)
        }
        val before = store.deletedWindows.size
        check(publish(old.copy(windowStartMs = 90, windowEndMs = 210, validProgramKeys = setOf("current-key"))).hasCommittedTarget)
        check(store.deletedWindows.drop(before).toSet() == setOf(90L to 210L, 100L to 200L))
        check(store.lastDeleteKeys == setOf("current-key") && coordinator.retryWindowCountForTest() == 0)
    }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    @Test
    fun eligibleRetryBypassesPreviouslySuccessfulFingerprint() {
        val store = FakeStore()
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        var now = 1_000L
        val coordinator = ProgramPublishCoordinator(writer) { now }
        writer.upsertChannels(
            listOf(ChannelRecord(key, 1, "101", "test", FrequencyHz(473_142_857L), casFactsCanonicalJson = testCasFacts())),
        )
        val window =
            ProgramPublishCoordinator.EpgUpdateWindow(
                key,
                program.startTimeMillis,
                program.startTimeMillis + program.durationMillis,
                setOf(TvProviderWriter.programKeyForTest(program)),
                true,
            )

        fun publish() =
            coordinator.publishWithUpdates(
                ChannelScanController.PublishMode.LIVE_TUNE_REFRESH,
                listOf(program),
                listOf(window),
                setOf(key),
            )
        check(publish().hasCommittedTarget)
        val before = store.deleteCalls
        store.failChannelQuery = true
        check(publish().failures.isNotEmpty() && coordinator.retryWindowCountForTest() == 1)
        store.failChannelQuery = false
        now += ProgramPublishCoordinator.RETRY_COOLDOWN_MS_FOR_TEST
        val recovered = publish()
        check(recovered.hasCommittedTarget && recovered.skippedUnchanged == 0)
        check(store.deleteCalls == before + 1 && coordinator.retryWindowCountForTest() == 0)
        check(publish().skippedUnchanged > 0 && store.deleteCalls == before + 1)
    }

    private class FakeStore(
        private var failWindowIndexOnce: Boolean = false,
        private var failInsertOnce: Boolean = false,
        var failDelete: Boolean = false,
        private var failServiceIndexOnce: Boolean = false,
    ) : TvProviderWriter.ChannelStore {
        private var nextChannelId = 1L
        private var nextProgramId = 100L
        private val channels = linkedMapOf<Long, ContentValues>()
        private val programs = linkedMapOf<Long, ContentValues>()
        var insertedPrograms = 0
        var updatedPrograms = 0
        var deleteCalls = 0
        var lastDeleteKeys: Set<String>? = null
        val deletedWindows = mutableListOf<Pair<Long, Long>>()
        var failChannelQuery = false
        var serviceIndexQueries = 0

        override fun findExistingChannelId(key: ServiceKey): Result<Long?> =
            if (failChannelQuery) {
                Result.failure(IllegalStateException("channel問い合わせ失敗"))
            } else {
                Result.success(channels.keys.firstOrNull())
            }

        override fun insertChannel(values: ContentValues): Result<Long?> {
            val id = nextChannelId++
            channels[id] = ContentValues(values)
            return Result.success(id)
        }

        override fun updateChannel(
            channelId: Long,
            values: ContentValues,
        ): Result<Int> {
            channels[channelId]?.putAll(values)
            return Result.success(if (channels.containsKey(channelId)) 1 else 0)
        }

        override fun indexExistingProgramsForService(channelId: Long): Result<Map<String, Long>> {
            serviceIndexQueries++
            if (failServiceIndexOnce) {
                failServiceIndexOnce = false
                return Result.failure(IllegalStateException("Program問い合わせ失敗"))
            }
            return Result.success(programIndex())
        }

        override fun indexExistingProgramsForWindow(
            channelId: Long,
            windowStartMs: Long,
            windowEndMs: Long,
        ): Result<Map<String, Long>> {
            if (failWindowIndexOnce) {
                failWindowIndexOnce = false
                return Result.failure(IllegalStateException("null cursor"))
            }
            return Result.success(programIndex())
        }

        override fun insertProgram(values: ContentValues): Result<Long?> {
            if (failInsertOnce) {
                failInsertOnce = false
                return Result.failure(IllegalStateException("挿入失敗"))
            }
            val id = nextProgramId++
            programs[id] = ContentValues(values)
            insertedPrograms++
            return Result.success(id)
        }

        override fun updateProgram(
            programId: Long,
            values: ContentValues,
        ): Result<Int> {
            programs[programId]?.putAll(values)
            val updated = if (programs.containsKey(programId)) 1 else 0
            updatedPrograms += updated
            return Result.success(updated)
        }

        // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
        @Suppress("MaxLineLength")
        override fun deleteObsoletePrograms(
            channelId: Long,
            validProgramKeys: Set<String>,
            windowStartMs: Long,
            windowEndMs: Long,
        ): Result<Int> {
            deleteCalls++
            deletedWindows += windowStartMs to windowEndMs
            lastDeleteKeys = validProgramKeys
            if (failDelete) return Result.failure(IllegalStateException("削除失敗"))
            val before = programs.size
            val removeIds =
                programs.mapNotNull { (id, values) ->
                    val key = TvProviderWriter.parseProgramKey(values.getAsByteArray(TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA))
                    if (key == null || key !in validProgramKeys) id else null
                }
            removeIds.forEach { programs.remove(it) }
            return Result.success(before - programs.size)
        }

        // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
        @Suppress("MaxLineLength")
        private fun programIndex(): Map<String, Long> =
            programs
                .mapNotNull { (id, values) ->
                    val key = TvProviderWriter.parseProgramKey(values.getAsByteArray(TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA))
                    key?.let { it to id }
                }.toMap()
    }
}

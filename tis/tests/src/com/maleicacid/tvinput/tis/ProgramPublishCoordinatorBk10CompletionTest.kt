package com.maleicacid.tvinput.tis

import android.content.ContentValues
import android.media.tv.TvContract
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.db.ChannelRecord
import com.maleicacid.tvinput.db.ProgramRecord
import org.junit.Test

class ProgramPublishCoordinatorBk10CompletionTest {
    private val key = ServiceKey(4, 16625, 101)
    private val program = ProgramRecord(
        serviceKey = key,
        eventId = 10,
        stableIdentity = "onid=4;tsid=16625;sid=101;event=10",
        startTimeMillis = 1_700_000_000_000L,
        durationMillis = 1_800_000L,
        title = "News",
        description = "desc",
    )

    @Test fun requiredQueryFailureDoesNotUpdateSignatureOrDeleteAndNextSuccessPublishes() {
        val store = FakeStore(failServiceIndexOnce = true)
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        val coordinator = ProgramPublishCoordinator(writer)
        writer.upsertChannels(listOf(ChannelRecord(key, "101", "NHK", 473_142_857L)))

        val first = coordinator.publish(
            mode = ChannelScanController.PublishMode.LIVE_TUNE_REFRESH,
            allPrograms = listOf(program),
            allowedServiceKeys = null,
        )
        check(first.failures.isNotEmpty()) { first.toString() }
        check(store.insertedPrograms == 0) { "必須問い合わせ失敗時は挿入を止める必要があります" }
        check(store.deleteCalls == 0) { "必須問い合わせ失敗時は廃止行を削除してはなりません" }
        check(coordinator.retryFailureClassesForTest().contains(ProgramPublishCoordinator.FailureClass.REQUIRED_QUERY_FAILED))

        val second = coordinator.publish(
            mode = ChannelScanController.PublishMode.LIVE_TUNE_REFRESH,
            allPrograms = listOf(program),
            allowedServiceKeys = null,
        )
        check(second.inserted == 1) { second.toString() }
        check(coordinator.retryWindowCountForTest() == 0) { "公開成功時は再試行区間を消去する必要があります" }
    }

    @Test fun failedInsertDoesNotCommitSignatureSoRetryCanPublishSameInput() {
        val store = FakeStore(failInsertOnce = true)
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        val coordinator = ProgramPublishCoordinator(writer)
        writer.upsertChannels(listOf(ChannelRecord(key, "101", "NHK", 473_142_857L)))

        val failed = coordinator.publish(ChannelScanController.PublishMode.SETUP_SCAN, listOf(program), allowedServiceKeys = null)
        check(failed.failures.any { it.operation == "program-insert" }) { failed.toString() }
        check(coordinator.retryFailureClassesForTest().contains(ProgramPublishCoordinator.FailureClass.PROGRAM_INSERT_FAILED))

        val retried = coordinator.publish(ChannelScanController.PublishMode.SETUP_SCAN, listOf(program), allowedServiceKeys = null)
        check(retried.inserted == 1) { "公開失敗時は同じ入力を未変更扱いしてはなりません: $retried" }
    }

    @Test fun authoritativeDeleteFailureIsRetriedWithObsoleteDeleteClass() {
        val store = FakeStore(failDelete = true)
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        val coordinator = ProgramPublishCoordinator(writer)
        writer.upsertChannels(listOf(ChannelRecord(key, "101", "NHK", 473_142_857L)))
        writer.upsertPrograms(listOf(program))

        val window = ProgramPublishCoordinator.EpgUpdateWindow(
            serviceKey = key,
            windowStartMs = program.startTimeMillis,
            windowEndMs = program.startTimeMillis + program.durationMillis,
            validProgramKeys = emptySet(),
            deletionAuthoritative = true,
        )
        val result = coordinator.publishWithUpdates(
            mode = ChannelScanController.PublishMode.SETUP_SCAN,
            allPrograms = emptyList(),
            updateWindows = listOf(window),
            allowedServiceKeys = null,
        )
        check(result.failures.any { it.operation == "program-delete-obsolete" }) { result.toString() }
        check(coordinator.retryFailureClassesForTest().contains(ProgramPublishCoordinator.FailureClass.OBSOLETE_DELETE_FAILED))
    }

    @Test fun retryBackoffUsesFixedScheduleJitterAttemptsAndRetention() {
        val one = ProgramPublishCoordinator.retryBackoffMsForTest(1, key, 1_700_000_000_000L, ProgramPublishCoordinator.FailureClass.PROGRAM_INSERT_FAILED)
        val two = ProgramPublishCoordinator.retryBackoffMsForTest(2, key, 1_700_000_000_000L, ProgramPublishCoordinator.FailureClass.PROGRAM_INSERT_FAILED)
        val three = ProgramPublishCoordinator.retryBackoffMsForTest(3, key, 1_700_000_000_000L, ProgramPublishCoordinator.FailureClass.PROGRAM_INSERT_FAILED)
        val four = ProgramPublishCoordinator.retryBackoffMsForTest(4, key, 1_700_000_000_000L, ProgramPublishCoordinator.FailureClass.PROGRAM_INSERT_FAILED)
        check(one in 48_000L..72_000L)
        check(two in 240_000L..360_000L)
        check(three in 720_000L..1_080_000L)
        check(four in 2_880_000L..4_320_000L)
        check(ProgramPublishCoordinator.MAX_RETRY_ATTEMPTS_FOR_TEST == 10)
        check(ProgramPublishCoordinator.RETRY_RETENTION_MS_FOR_TEST == 24L * 60 * 60 * 1000)
    }



    @Test fun retryWindowLimitsMatchDesignAndTrimPerService() {
        val store = FakeStore(failDelete = true)
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        val coordinator = ProgramPublishCoordinator(writer)
        writer.upsertChannels(listOf(ChannelRecord(key, "101", "NHK", 473_142_857L)))

        val windows = (0 until (ProgramPublishCoordinator.MAX_RETRY_WINDOWS_PER_SERVICE_FOR_TEST + 1)).map { i ->
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
        check(coordinator.retryWindowCountForTest() == ProgramPublishCoordinator.MAX_RETRY_WINDOWS_PER_SERVICE_FOR_TEST) {
            "service単位の再試行区間上限は設計値に固定する必要があります"
        }
        check(coordinator.droppedRetryWindowCountForTest(key) == 1)
        check(ProgramPublishCoordinator.MAX_RETRY_WINDOWS_PER_SERVICE_FOR_TEST == 32)
        check(ProgramPublishCoordinator.MAX_RETRY_WINDOWS_TOTAL_FOR_TEST == 512)
    }

    @Test fun expiredRetryWindowIsDroppedInsteadOfKept() {
        val store = FakeStore(failDelete = true)
        val writer = TvProviderWriter("input.test", store, testOnly = true)
        val coordinator = ProgramPublishCoordinator(writer)
        writer.upsertChannels(listOf(ChannelRecord(key, "101", "NHK", 473_142_857L)))

        val expiredFirstFailure = System.currentTimeMillis() - ProgramPublishCoordinator.RETRY_RETENTION_MS_FOR_TEST - 60_000L
        val window = ProgramPublishCoordinator.EpgUpdateWindow(
            serviceKey = key,
            windowStartMs = program.startTimeMillis,
            windowEndMs = program.startTimeMillis + program.durationMillis,
            validProgramKeys = emptySet(),
            deletionAuthoritative = true,
            attempt = 1,
            firstFailureAtMillis = expiredFirstFailure,
            lastFailureAtMillis = expiredFirstFailure,
        )
        coordinator.publishWithUpdates(
            mode = ChannelScanController.PublishMode.SETUP_SCAN,
            allPrograms = emptyList(),
            updateWindows = listOf(window),
            allowedServiceKeys = null,
        )
        check(coordinator.retryWindowCountForTest() == 0) { "期限切れの再試行区間を保持してはなりません" }
        check(coordinator.droppedRetryWindowCountForTest(key) == 1)
    }

    private class FakeStore(
        private var failServiceIndexOnce: Boolean = false,
        private var failInsertOnce: Boolean = false,
        private val failDelete: Boolean = false,
    ) : TvProviderWriter.ChannelStore {
        private var nextChannelId = 1L
        private var nextProgramId = 100L
        private val channels = linkedMapOf<Long, ContentValues>()
        private val programs = linkedMapOf<Long, ContentValues>()
        var insertedPrograms = 0
        var deleteCalls = 0

        override fun findExistingChannelId(key: ServiceKey): Result<Long?> = Result.success(channels.keys.firstOrNull())

        override fun insertChannel(values: ContentValues): Result<Long?> {
            val id = nextChannelId++
            channels[id] = ContentValues(values)
            return Result.success(id)
        }

        override fun updateChannel(channelId: Long, values: ContentValues): Result<Int> {
            channels[channelId]?.putAll(values)
            return Result.success(if (channels.containsKey(channelId)) 1 else 0)
        }

        override fun indexExistingProgramsForService(channelId: Long): Result<Map<String, Long>> {
            if (failServiceIndexOnce) {
                failServiceIndexOnce = false
                return Result.failure(IllegalStateException("null cursor"))
            }
            return Result.success(programIndex())
        }

        override fun indexExistingProgramsForWindow(channelId: Long, windowStartMs: Long, windowEndMs: Long): Result<Map<String, Long>> =
            Result.success(programIndex())

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

        override fun updateProgram(programId: Long, values: ContentValues): Result<Int> {
            programs[programId]?.putAll(values)
            return Result.success(if (programs.containsKey(programId)) 1 else 0)
        }

        override fun deleteObsoletePrograms(channelId: Long, validProgramKeys: Set<String>, windowStartMs: Long, windowEndMs: Long): Result<Int> {
            deleteCalls++
            if (failDelete) return Result.failure(IllegalStateException("削除失敗"))
            val before = programs.size
            val removeIds = programs.mapNotNull { (id, values) ->
                val key = TvProviderWriter.parseProgramKey(values.getAsByteArray(TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA).toString(Charsets.UTF_8))
                if (key == null || key !in validProgramKeys) id else null
            }
            removeIds.forEach { programs.remove(it) }
            return Result.success(before - programs.size)
        }

        private fun programIndex(): Map<String, Long> = programs.mapNotNull { (id, values) ->
            val key = TvProviderWriter.parseProgramKey(values.getAsByteArray(TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA).toString(Charsets.UTF_8))
            key?.let { it to id }
        }.toMap()
    }
}

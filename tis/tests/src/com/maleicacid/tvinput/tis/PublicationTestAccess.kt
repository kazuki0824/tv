// テスト専用の観測と入力fixtureは製品source setへ含めない。
package com.maleicacid.tvinput.tis

import android.content.ContentValues
import com.maleicacid.tvinput.aribsi.ProviderDataBridge
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.db.ProgramRecord

private fun ProgramPublishCoordinator.dirtyWindowsForTest(): Collection<*> =
    (javaClass.getDeclaredField("dirtyWindows").apply { isAccessible = true }.get(this) as Map<*, *>).values

fun ProgramPublishCoordinator.retryWindowCountForTest(): Int = dirtyWindowsForTest().size

fun ProgramPublishCoordinator.retryFailureClassesForTest(): Set<String> =
    dirtyWindowsForTest()
        .map { window ->
            requireNotNull(window)
                .javaClass
                .getDeclaredField("failureClass")
                .apply { isAccessible = true }
                .get(window) as String
        }.toSet()

fun ProgramPublishCoordinator.retryNotBeforeMillisForTest(): List<Long> =
    dirtyWindowsForTest().map { window ->
        requireNotNull(window)
            .javaClass
            .getDeclaredField("notBeforeMs")
            .apply { isAccessible = true }
            .getLong(window)
    }

fun programProviderDataForTest(program: ProgramRecord): String =
    (ProviderDataBridge.buildProgramProviderData(program) as ProviderDataBridge.Success).json

fun TvProviderWriter.programValuesForTest(
    channelId: Long,
    program: ProgramRecord,
): ContentValues =
    programValues(
        channelId,
        program,
        clearAbsentOptionalColumns = true,
        providerData = (ProviderDataBridge.buildProgramProviderData(program) as ProviderDataBridge.Success).bytes,
    )

fun programSignatureForTest(programs: List<ProgramRecord>): List<String> {
    val writer =
        TvProviderWriter(
            "test",
            object : TvProviderWriter.ChannelStore {
                // 既存store契約の型を維持し、試験だけの別型を作らない。
                @Suppress("MaxLineLength")
                override fun indexExistingChannelIds(keys: Set<ServiceKey>): Result<Map<ServiceKey, Long>> = Result.success(emptyMap())

                override fun insertChannel(values: ContentValues): Result<Long?> = Result.success(null)

                override fun updateChannel(
                    channelId: Long,
                    values: ContentValues,
                ): Result<Int> = Result.success(0)
            },
            testOnly = true,
        )
    return programs
        .sortedWith(
            compareBy<ProgramRecord> { it.serviceKey.originalNetworkId }
                .thenBy { it.serviceKey.transportStreamId }
                .thenBy { it.serviceKey.serviceId }
                .thenBy { it.stableIdentity }
                .thenBy { it.eventId },
        ).map { writer.publicationInputSignature(it, emptyList()).getOrThrow() }
}

package com.maleicacid.tvinput.tis

import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.db.ProgramRecord
import com.maleicacid.tvinput.aribsi.ProviderDataBridge

/**
 * TvProvider Programs への反映を公開modeごとに制御する。
 * ライブ更新では既存channelだけを対象にし、同一内容の連続EITは過剰upsertしない。
 */
class ProgramPublishCoordinator(private val tvProviderWriter: TvProviderWriter) {
    data class EpgUpdateWindow(
        val serviceKey: ServiceKey,
        val windowStartMs: Long,
        val windowEndMs: Long,
        val validProgramKeys: Set<String>,
        val deletionAuthoritative: Boolean = false,
        val failureClass: String = FailureClass.NONE,
        val attempt: Int = 0,
        val nextAttemptAtMs: Long = 0L,
        val firstFailureAtMillis: Long = 0L,
        val lastFailureAtMillis: Long = 0L,
    )

    data class ProgramPublishResult(
        val inserted: Int,
        val updated: Int,
        val deleted: Int = 0,
        val skippedUnchanged: Int = 0,
        val skippedNoChannel: Int = 0,
        val failures: List<TvProviderWriter.Diagnostic> = emptyList(),
        val eligibleTargetCount: Int = 0,
        val committedServiceCount: Int = 0,
    ) {
        val changed: Int get() = inserted + updated + deleted
        val hasCommittedTarget: Boolean
            get() = eligibleTargetCount > 0 && committedServiceCount > 0 && failures.isEmpty()
    }

    private data class RetryWindowKey(
        val serviceKey: ServiceKey,
        val windowStartMs: Long,
        val windowEndMs: Long,
        val failureClass: String = FailureClass.NONE,
    )

    object FailureClass {
        const val NONE = "NONE"
        const val REQUIRED_QUERY_FAILED = "REQUIRED_QUERY_FAILED"
        const val PROGRAM_INSERT_FAILED = "PROGRAM_INSERT_FAILED"
        const val PROGRAM_UPDATE_FAILED = "PROGRAM_UPDATE_FAILED"
        const val OBSOLETE_DELETE_FAILED = "OBSOLETE_DELETE_FAILED"
        const val SIGNATURE_BUILD_FAILED = "SIGNATURE_BUILD_FAILED"
        const val PROVIDER_UNAVAILABLE = "PROVIDER_UNAVAILABLE"
    }

    private val lastProgramSignatureByMode = linkedMapOf<ChannelScanController.PublishMode, String>()
    private val retryWindows = linkedMapOf<RetryWindowKey, EpgUpdateWindow>()
    private val droppedRetryWindowCountByService = linkedMapOf<ServiceKey, Int>()

    fun reset() {
        lastProgramSignatureByMode.clear()
        retryWindows.clear()
        droppedRetryWindowCountByService.clear()
    }

    fun publish(
        mode: ChannelScanController.PublishMode,
        allPrograms: List<ProgramRecord>,
        allowedServiceKeys: Set<ServiceKey>?,
    ): ProgramPublishResult = publishWithUpdates(
        mode = mode,
        allPrograms = allPrograms,
        updateWindows = windowsFromPrograms(allPrograms),
        allowedServiceKeys = allowedServiceKeys,
    )

    fun publishWithUpdates(
        mode: ChannelScanController.PublishMode,
        allPrograms: List<ProgramRecord>,
        updateWindows: List<EpgUpdateWindow>,
        allowedServiceKeys: Set<ServiceKey>?,
    ): ProgramPublishResult {
        if (mode == ChannelScanController.PublishMode.DIAGNOSTIC_ONLY) {
            return ProgramPublishResult(0, 0, skippedUnchanged = allPrograms.size)
        }
        // 再試行区間は公開入口入力の一部である。
        // これを確認する前に早期returnしてはならない。EPG区間排出API後の
        // provider失敗で、排出済み区間を失うことを防ぐ。
        val retryServiceKeys = retryWindows.keys.map { it.serviceKey }
        val allServiceKeys = (allPrograms.map { it.serviceKey } + updateWindows.map { it.serviceKey } + retryServiceKeys).toSet()
        if (allPrograms.isEmpty() && updateWindows.isEmpty() && retryWindows.isEmpty()) {
            return ProgramPublishResult(0, 0, skippedUnchanged = 0)
        }
        val existingServiceKeys = if (mode == ChannelScanController.PublishMode.LIVE_TUNE_REFRESH || mode == ChannelScanController.PublishMode.BOOT_EPG_SYNC || mode == ChannelScanController.PublishMode.BACKGROUND_CHANNEL_MAINTENANCE) {
            when (val existingResult = tvProviderWriter.existingServiceKeysResult(allServiceKeys)) {
                is TvProviderWriter.ExistingServiceKeysResult.Success -> existingResult.keys
                is TvProviderWriter.ExistingServiceKeysResult.Failure -> {
                    enqueueRetryWindows(updateWindows, failureClass = FailureClass.REQUIRED_QUERY_FAILED)
                    return ProgramPublishResult(0, 0, failures = existingResult.diagnostics)
                }
            }
        } else {
            emptySet()
        }
        val allowed = filterServiceKeysForMode(mode, allServiceKeys, existingServiceKeys, allowedServiceKeys)
        val retryForAllowed = drainRetryWindowsFor(allowed)
        val programs = allPrograms
            .filter { it.serviceKey in allowed }
        val windows = (retryForAllowed + updateWindows).distinctBy { RetryWindowKey(it.serviceKey, it.windowStartMs, it.windowEndMs, it.failureClass) }
            .filter { it.serviceKey in allowed && it.windowEndMs > it.windowStartMs }
        if (programs.isEmpty() && windows.isEmpty()) return ProgramPublishResult(0, 0, skippedNoChannel = allServiceKeys.size)
        val authoritativeWindows = windows.filter { it.deletionAuthoritative }
        val eligibleTargetCount = programs.size + authoritativeWindows.size
        val eligibleTargetServiceKeys = (programs.map { it.serviceKey } + authoritativeWindows.map { it.serviceKey }).toSet()

        val signature = runCatching { plannedInputSignature(programs, windows) }.getOrElse { error ->
            enqueueRetryWindows(windows, failureClass = FailureClass.SIGNATURE_BUILD_FAILED)
            return ProgramPublishResult(
                0,
                0,
                failures = listOf(TvProviderWriter.Diagnostic(null, "program-signature", error.message.orEmpty())),
            )
        }
        if (mode != ChannelScanController.PublishMode.BOOT_EPG_SYNC && lastProgramSignatureByMode[mode] == signature) {
            return ProgramPublishResult(
                0,
                0,
                skippedUnchanged = allServiceKeys.size,
                eligibleTargetCount = eligibleTargetCount,
                committedServiceCount = eligibleTargetServiceKeys.size,
            )
        }
        val result = tvProviderWriter.upsertProgramsForWindows(programs, windows)
        val failedServiceKeys = result.failures.mapNotNull { it.serviceKey }.toSet()
        val failedWindows = if (result.failures.any { it.serviceKey == null }) {
            windows
        } else {
            windows.filter { it.serviceKey in failedServiceKeys }
        }
        val succeededWindows = windows.filter { it.serviceKey in result.succeededServiceKeys && it.serviceKey !in failedServiceKeys }
        removeRetryWindows(succeededWindows)
        if (result.failures.isEmpty()) {
            lastProgramSignatureByMode[mode] = signature
        } else {
            enqueueFailedWindows(failedWindows, result.failures)
        }
        return ProgramPublishResult(
            inserted = result.inserted,
            updated = result.updated,
            deleted = result.deleted,
            failures = result.failures,
            eligibleTargetCount = eligibleTargetCount,
            committedServiceCount = result.succeededServiceKeys.count { it in eligibleTargetServiceKeys },
        )
    }

    /**
     * 次の公開入口用にprocess内再試行区間を返す。
     * ここではqueueを削除しない。成功時にkeyを削除し、provider失敗時は
     * 次の入口へ残す。
     */
    private fun drainRetryWindowsFor(allowed: Set<ServiceKey>): List<EpgUpdateWindow> {
        val now = System.currentTimeMillis()
        discardExpiredRetryWindows(now)
        return retryWindows
            .filterKeys { it.serviceKey in allowed }
            .values
            .filter { it.nextAttemptAtMs <= now }
            .toList()
    }

    private fun removeRetryWindows(windows: List<EpgUpdateWindow>) {
        windows.forEach { window ->
            retryWindows.keys.filter { it.serviceKey == window.serviceKey && it.windowStartMs == window.windowStartMs && it.windowEndMs == window.windowEndMs }
                .forEach { retryWindows.remove(it) }
        }
    }

    private fun enqueueFailedWindows(windows: List<EpgUpdateWindow>, failures: List<TvProviderWriter.Diagnostic>) {
        val byService = failures.groupBy { it.serviceKey }
        windows.forEach { window ->
            val serviceFailures = byService[window.serviceKey].orEmpty() + byService[null].orEmpty()
            val failureClass = serviceFailures.firstOrNull()?.operation?.let(::failureClassForOperation) ?: FailureClass.PROVIDER_UNAVAILABLE
            enqueueRetryWindows(listOf(window), failureClass = failureClass)
        }
    }

    private fun failureClassForOperation(operation: String): String = when (operation) {
        "program-insert" -> FailureClass.PROGRAM_INSERT_FAILED
        "program-update" -> FailureClass.PROGRAM_UPDATE_FAILED
        "program-delete-obsolete" -> FailureClass.OBSOLETE_DELETE_FAILED
        "program-channel-query", "program-index-query", "channel-query" -> FailureClass.REQUIRED_QUERY_FAILED
        "program-signature" -> FailureClass.SIGNATURE_BUILD_FAILED
        else -> FailureClass.PROVIDER_UNAVAILABLE
    }

    private fun enqueueRetryWindows(windows: List<EpgUpdateWindow>, failureClass: String = FailureClass.PROVIDER_UNAVAILABLE) {
        val now = System.currentTimeMillis()
        discardExpiredRetryWindows(now)
        windows.sortedWith(compareBy<EpgUpdateWindow> { it.serviceKey.originalNetworkId }.thenBy { it.serviceKey.transportStreamId }.thenBy { it.serviceKey.serviceId }.thenBy { it.windowStartMs }.thenBy { it.windowEndMs })
            .forEach { window ->
                if (!window.deletionAuthoritative && failureClass == FailureClass.OBSOLETE_DELETE_FAILED) return@forEach
                if (window.attempt >= MAX_RETRY_ATTEMPTS) {
                    droppedRetryWindowCountByService[window.serviceKey] = (droppedRetryWindowCountByService[window.serviceKey] ?: 0) + 1
                    return@forEach
                }
                val attempt = window.attempt + 1
                val firstFailureAt = if (window.firstFailureAtMillis > 0L) window.firstFailureAtMillis else now
                if (now - firstFailureAt > RETRY_RETENTION_MS) {
                    droppedRetryWindowCountByService[window.serviceKey] = (droppedRetryWindowCountByService[window.serviceKey] ?: 0) + 1
                    return@forEach
                }
                val retry = window.copy(
                    failureClass = failureClass,
                    attempt = attempt,
                    nextAttemptAtMs = now + retryBackoffMs(attempt, window.serviceKey, window.windowStartMs, failureClass),
                    firstFailureAtMillis = firstFailureAt,
                    lastFailureAtMillis = now,
                )
                retryWindows[RetryWindowKey(retry.serviceKey, retry.windowStartMs, retry.windowEndMs, retry.failureClass)] = retry
                trimRetryWindowsForService(retry.serviceKey)
                trimRetryWindowsGlobal()
            }
    }

    private fun discardExpiredRetryWindows(now: Long) {
        retryWindows.entries.removeIf { (_, window) ->
            window.firstFailureAtMillis > 0L && now - window.firstFailureAtMillis > RETRY_RETENTION_MS
        }
    }

    private fun trimRetryWindowsForService(serviceKey: ServiceKey) {
        while (retryWindows.keys.count { it.serviceKey == serviceKey } > MAX_RETRY_WINDOWS_PER_SERVICE) {
            val oldest = retryWindows.keys.firstOrNull { it.serviceKey == serviceKey } ?: return
            retryWindows.remove(oldest)
            droppedRetryWindowCountByService[serviceKey] = (droppedRetryWindowCountByService[serviceKey] ?: 0) + 1
        }
    }

    private fun trimRetryWindowsGlobal() {
        while (retryWindows.size > MAX_RETRY_WINDOWS_TOTAL) {
            val oldest = retryWindows.keys.firstOrNull() ?: return
            retryWindows.remove(oldest)
            droppedRetryWindowCountByService[oldest.serviceKey] = (droppedRetryWindowCountByService[oldest.serviceKey] ?: 0) + 1
        }
    }

    private fun plannedInputSignature(programs: List<ProgramRecord>, windows: List<EpgUpdateWindow>): String {
        val programPart = programSignatureForTest(programs)
        val windowPart = windows.sortedWith(compareBy<EpgUpdateWindow> { it.serviceKey.originalNetworkId }
            .thenBy { it.serviceKey.transportStreamId }
            .thenBy { it.serviceKey.serviceId }
            .thenBy { it.windowStartMs }
            .thenBy { it.windowEndMs }
            .thenBy { it.failureClass })
            .joinToString("|") { window ->
                listOf(
                    window.serviceKey.originalNetworkId,
                    window.serviceKey.transportStreamId,
                    window.serviceKey.serviceId,
                    window.windowStartMs,
                    window.windowEndMs,
                    window.validProgramKeys.sorted().joinToString(","),
                    window.deletionAuthoritative,
                    window.failureClass,
                ).joinToString(":")
            }
        return "programs=$programPart#windows=$windowPart"
    }

    private fun windowsFromPrograms(programs: List<ProgramRecord>): List<EpgUpdateWindow> = programs.groupBy { it.serviceKey }.map { (key, values) ->
        EpgUpdateWindow(
            serviceKey = key,
            windowStartMs = values.minOf { it.startTimeMillis },
            windowEndMs = values.mapNotNull { program ->
                runCatching { Math.addExact(program.startTimeMillis, program.durationMillis) }.getOrNull()
            }.maxOrNull() ?: values.maxOf { it.startTimeMillis },
            validProgramKeys = values.map { programIdentityForCoordinator(it) }.toSet(),
        )
    }

    private fun retryBackoffMs(attempt: Int, serviceKey: ServiceKey, windowStartMs: Long, failureClass: String): Long =
        retryBackoffMsForTest(attempt, serviceKey, windowStartMs, failureClass)

    fun retryWindowCountForTest(): Int {
        discardExpiredRetryWindows(System.currentTimeMillis())
        return retryWindows.size
    }

    fun retryFailureClassesForTest(): Set<String> {
        discardExpiredRetryWindows(System.currentTimeMillis())
        return retryWindows.keys.map { it.failureClass }.toSet()
    }

    fun droppedRetryWindowCountForTest(serviceKey: ServiceKey): Int = droppedRetryWindowCountByService[serviceKey] ?: 0

    companion object {
        const val RETRY_RETENTION_MS_FOR_TEST: Long = 24 * 60 * 60 * 1000L
        const val MAX_RETRY_ATTEMPTS_FOR_TEST: Int = 10
        const val MAX_RETRY_WINDOWS_PER_SERVICE_FOR_TEST: Int = 32
        const val MAX_RETRY_WINDOWS_TOTAL_FOR_TEST: Int = 512

        private const val MAX_RETRY_WINDOWS_PER_SERVICE = MAX_RETRY_WINDOWS_PER_SERVICE_FOR_TEST
        private const val MAX_RETRY_WINDOWS_TOTAL = MAX_RETRY_WINDOWS_TOTAL_FOR_TEST
        private const val MAX_RETRY_ATTEMPTS = MAX_RETRY_ATTEMPTS_FOR_TEST
        private const val RETRY_RETENTION_MS = RETRY_RETENTION_MS_FOR_TEST

        fun retryBackoffMsForTest(attempt: Int, serviceKey: ServiceKey, windowStartMs: Long, failureClass: String): Long {
            val base = when (attempt.coerceAtLeast(1)) {
                1 -> 60_000L
                2 -> 5 * 60_000L
                3 -> 15 * 60_000L
                else -> 60 * 60_000L
            }
            val hash = (serviceKey.hashCode() * 31 + windowStartMs.hashCode() * 17 + failureClass.hashCode()).let { if (it == Int.MIN_VALUE) 0 else kotlin.math.abs(it) }
            val jitterPercent = (hash % 41) - 20 // 決定的な±20%
            return base + (base * jitterPercent / 100)
        }

        fun filterServiceKeysForMode(
            mode: ChannelScanController.PublishMode,
            allServiceKeys: Iterable<ServiceKey>,
            existingServiceKeys: Set<ServiceKey>,
            allowedServiceKeys: Set<ServiceKey>?,
        ): Set<ServiceKey> = when (mode) {
            ChannelScanController.PublishMode.SETUP_SCAN -> allServiceKeys.filter { allowedServiceKeys == null || it in allowedServiceKeys }.toSet()
            ChannelScanController.PublishMode.LIVE_TUNE_REFRESH,
            ChannelScanController.PublishMode.BOOT_EPG_SYNC,
            ChannelScanController.PublishMode.BACKGROUND_CHANNEL_MAINTENANCE -> allServiceKeys.filter { it in existingServiceKeys && (allowedServiceKeys == null || it in allowedServiceKeys) }.toSet()
            ChannelScanController.PublishMode.DIAGNOSTIC_ONLY -> emptySet()
        }

        fun programSignatureForTest(programs: List<ProgramRecord>): String = programs
            .sortedWith(compareBy<ProgramRecord> { it.serviceKey.originalNetworkId }
                .thenBy { it.serviceKey.transportStreamId }
                .thenBy { it.serviceKey.serviceId }
                .thenBy { it.stableIdentity }
                .thenBy { it.eventId })
            .joinToString("|") { program ->
                projectedProgramSignature(program)
            }

        fun programIdentityForTest(program: ProgramRecord): String = programIdentityForCoordinator(program)

        private fun programIdentityForCoordinator(program: ProgramRecord): String = ProviderDataBridge.buildProgramKey(program)

        private fun projectedProgramSignature(program: ProgramRecord): String = TvProviderWriter.signatureForProgramForTest(0L, program)
    }
}

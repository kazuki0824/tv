package com.maleicacid.tvinput.tis

import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.db.ProgramRecord

/**
 * TvProvider Programs への反映を publish mode ごとに制御する。
 * live refresh では既存 channel だけを対象にし、同一内容の連続 EIT は過剰 upsert しない。
 */
class ProgramPublishCoordinator(private val tvProviderWriter: TvProviderWriter) {
    data class EpgUpdateWindow(
        val serviceKey: ServiceKey,
        val windowStartMs: Long,
        val windowEndMs: Long,
        val validProgramKeys: Set<String>,
    )

    data class ProgramPublishResult(
        val inserted: Int,
        val updated: Int,
        val deleted: Int = 0,
        val skippedUnchanged: Int = 0,
        val skippedNoChannel: Int = 0,
        val failures: List<TvProviderWriter.Diagnostic> = emptyList(),
    ) {
        val changed: Int get() = inserted + updated + deleted
    }

    private data class RetryWindowKey(val serviceKey: ServiceKey, val windowStartMs: Long, val windowEndMs: Long, val tableScope: String = "r51")

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
        if (allPrograms.isEmpty() && updateWindows.isEmpty()) {
            return ProgramPublishResult(0, 0, skippedUnchanged = 0)
        }
        val allServiceKeys = (allPrograms.map { it.serviceKey } + updateWindows.map { it.serviceKey }).toSet()
        val existingServiceKeys = if (mode == ChannelScanController.PublishMode.LIVE_TUNE_REFRESH || mode == ChannelScanController.PublishMode.BOOT_EPG_SYNC || mode == ChannelScanController.PublishMode.BACKGROUND_CHANNEL_MAINTENANCE) {
            when (val existingResult = tvProviderWriter.existingServiceKeysResult(allServiceKeys)) {
                is TvProviderWriter.ExistingServiceKeysResult.Success -> existingResult.keys
                is TvProviderWriter.ExistingServiceKeysResult.Failure -> return ProgramPublishResult(0, 0, failures = existingResult.diagnostics)
            }
        } else {
            emptySet()
        }
        val allowed = filterServiceKeysForMode(mode, allServiceKeys, existingServiceKeys, allowedServiceKeys)
        val retryForAllowed = drainRetryWindowsFor(allowed)
        val programs = allPrograms.filter { it.serviceKey in allowed }
        val windows = (retryForAllowed + updateWindows).distinctBy { RetryWindowKey(it.serviceKey, it.windowStartMs, it.windowEndMs) }
            .filter { it.serviceKey in allowed && it.windowEndMs > it.windowStartMs }
        if (programs.isEmpty() && windows.isEmpty()) return ProgramPublishResult(0, 0, skippedNoChannel = allServiceKeys.size)

        val signature = programSignatureForTest(programs) + "#windows=" + windows
            .sortedWith(compareBy<EpgUpdateWindow> { it.serviceKey.originalNetworkId }
                .thenBy { it.serviceKey.transportStreamId }
                .thenBy { it.serviceKey.serviceId }
                .thenBy { it.windowStartMs }
                .thenBy { it.windowEndMs })
            .joinToString("|") { window ->
                val key = window.serviceKey
                listOf(key.originalNetworkId, key.transportStreamId, key.serviceId, window.windowStartMs, window.windowEndMs, window.validProgramKeys.sorted().joinToString(",")).joinToString(":")
            }
        if (lastProgramSignatureByMode[mode] == signature) {
            return ProgramPublishResult(0, 0, skippedUnchanged = allServiceKeys.size)
        }
        val result = tvProviderWriter.upsertProgramsForWindows(programs, windows)
        if (result.failures.isEmpty()) {
            lastProgramSignatureByMode[mode] = signature
            removeRetryWindows(windows)
        } else {
            enqueueRetryWindows(windows)
        }
        return ProgramPublishResult(result.inserted, result.updated, deleted = result.deleted, failures = result.failures)
    }

    private fun drainRetryWindowsFor(allowed: Set<ServiceKey>): List<EpgUpdateWindow> = retryWindows
        .filterKeys { it.serviceKey in allowed }
        .values
        .toList()

    private fun removeRetryWindows(windows: List<EpgUpdateWindow>) {
        windows.forEach { retryWindows.remove(RetryWindowKey(it.serviceKey, it.windowStartMs, it.windowEndMs)) }
    }

    private fun enqueueRetryWindows(windows: List<EpgUpdateWindow>) {
        windows.sortedWith(compareBy<EpgUpdateWindow> { it.serviceKey.originalNetworkId }.thenBy { it.serviceKey.transportStreamId }.thenBy { it.serviceKey.serviceId }.thenBy { it.windowStartMs }.thenBy { it.windowEndMs })
            .forEach { window ->
                retryWindows[RetryWindowKey(window.serviceKey, window.windowStartMs, window.windowEndMs)] = window
                trimRetryWindowsForService(window.serviceKey)
                trimRetryWindowsGlobal()
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

    private fun windowsFromPrograms(programs: List<ProgramRecord>): List<EpgUpdateWindow> = programs.groupBy { it.serviceKey }.map { (key, values) ->
        EpgUpdateWindow(
            serviceKey = key,
            windowStartMs = values.minOf { it.startTimeMillis },
            windowEndMs = values.maxOf { it.startTimeMillis + it.durationMillis },
            validProgramKeys = values.map { programIdentityForCoordinator(it) }.toSet(),
        )
    }

    companion object {
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

        private fun programIdentityForCoordinator(program: ProgramRecord): String = TvProviderWriter.canonicalProgramKey(program)

        private fun projectedProgramSignature(program: ProgramRecord): String = TvProviderWriter.signatureForProgramForTest(0L, program)
    }
}

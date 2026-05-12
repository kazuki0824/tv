package com.maleicacid.tvinput.tis

import java.util.concurrent.atomic.AtomicLong

object BootEpgSyncDiagnostics {
    val lockedBootDeferredCount = AtomicLong(0)
    val pendingDrainStartedCount = AtomicLong(0)
    val pendingDrainSkippedLockedCount = AtomicLong(0)
    val pendingDrainSkippedNoTunerCount = AtomicLong(0)
    val pendingDrainSkippedTvProviderUnavailableCount = AtomicLong(0)
    @Volatile var lastSkippedReason: String = ""
}

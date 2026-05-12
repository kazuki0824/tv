package com.maleicacid.tvinput.tis

import java.util.concurrent.atomic.AtomicLong

object BackgroundChannelMaintenanceDiagnostics {
    val scheduledAfterBootSyncCount = AtomicLong(0)
    val startedCount = AtomicLong(0)
    val skippedActiveLiveSessionCount = AtomicLong(0)
    val skippedScanRunningCount = AtomicLong(0)
    val skippedOtherCount = AtomicLong(0)
    @Volatile var lastSkippedReason: String = ""
}

// テストの入力・期待値を本体の定数と独立した具体値で記述する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import android.content.Intent
import android.media.tv.TvInputService
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class DirectBootGuardR51FixTest {
    @Test fun scheduledEpgUsesNextLocalClockTime() {
        val before = java.time.ZonedDateTime.parse("2026-11-24T22:59:00+09:00[Asia/Tokyo]")
        val time = java.time.LocalTime.of(23, 0)
        val tonight = before.plusMinutes(1)
        check(DailyEpgSchedule.next(before, time) == tonight)
        check(DailyEpgSchedule.next(tonight, time) == tonight.plusDays(1))
    }

    @Test fun lockedBootStoresOnlyPendingState() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        DirectBootGuard.onLockedBootCompleted(context, 1234L, Intent.ACTION_LOCKED_BOOT_COMPLETED)
        val state = DirectBootGuard.pendingState(context)
        check(state.pending)
        check(state.lastLockedBootReceivedAt == 1234L)
        check(state.bootReason == Intent.ACTION_LOCKED_BOOT_COMPLETED)
        check(state.lastSkippedReason == "LOCKED_BOOT_DEFERRED")
    }

    @Test
    fun periodicEpgRefreshCanRunAlongsidePlaybackAtBackgroundPriority() {
        val decision =
            ChannelScanManager.bootEpgSyncStartDecision(
                activeLiveSessionCount = 1,
                scanRunning = false,
                playbackPipelineRunning = true,
                periodic = true,
            )
        check(decision.allowed)
        check(
            tunerPriorityHintUseCase(ScanPurpose.PERIODIC_EPG_REFRESH) ==
                TvInputService.PRIORITY_HINT_USE_CASE_TYPE_BACKGROUND,
        )
    }

    @Test
    fun periodicEpgRefreshYieldsToSessionCreationAndOtherScans() {
        val duringCreation =
            ChannelScanManager.bootEpgSyncStartDecision(
                activeLiveSessionCount = 1,
                scanRunning = false,
                sessionCreationInProgress = true,
                periodic = true,
            )
        check(!duringCreation.allowed)
        check(!ChannelScanManager.bootEpgSyncStartDecision(1, true, periodic = true).allowed)
        val preemption = ChannelScanManager.liveSessionPreemptDecision(true, ScanPurpose.PERIODIC_EPG_REFRESH)
        check(preemption.shouldCancel && !preemption.deferBootEpgSync)
    }
}

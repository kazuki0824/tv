package com.maleicacid.tvinput.tis

import org.junit.Test

class AribBroadcastClockTest {
    private class FakeTimers {
        data class Posted(val runnable: Runnable, val delayMillis: Long)

        val posted = mutableListOf<Posted>()
        val removed = mutableListOf<Runnable>()

        fun post(runnable: Runnable, delayMillis: Long) {
            posted += Posted(runnable, delayMillis)
        }

        fun remove(runnable: Runnable) {
            removed += runnable
        }
    }

    private fun firstAuthority(
        tableId: Int = AribBroadcastClock.TABLE_ID_TDT,
        mjd: Int = 60_000,
        millisOfDay: Long,
        receivedNanoTime: Long,
    ): AribBroadcastClock.AuthoritySample = AribBroadcastClock.updateAuthority(
        null,
        AribBroadcastClock.SourceSample(tableId, mjd, millisOfDay, receivedNanoTime),
    )!!.authority

    @Test
    fun timing10DelayUsesBroadcastClockAndContinuousUpdatesRecalibrate() {
        val first = firstAuthority(
            millisOfDay = 12 * 3600L * 1_000L,
            receivedNanoTime = 1_000_000_000L,
        )
        val statement = AribBroadcastClock.StatementTime((12 * 3600L + 10) * 1_000L)
        check(
            AribBroadcastClock.deadlineUntil(
                statement,
                first,
                nowNanoTime = 1_000_000_000L,
            )?.delayMillis == 10_000L,
        )

        val update = AribBroadcastClock.updateAuthority(
            first,
            AribBroadcastClock.SourceSample(
                AribBroadcastClock.TABLE_ID_TDT,
                60_000,
                (12 * 3600L + 4) * 1_000L,
                5_000_000_000L,
            ),
        )!!
        check(!update.discontinuity)
        check(update.authority.generation == first.generation)
        check(
            AribBroadcastClock.deadlineUntil(
                statement,
                update.authority,
                expectedGeneration = first.generation,
                nowNanoTime = 5_000_000_000L,
            )?.delayMillis == 6_000L,
        )

        val totUpdate = AribBroadcastClock.updateAuthority(
            first,
            AribBroadcastClock.SourceSample(
                AribBroadcastClock.TABLE_ID_TOT,
                60_000,
                (12 * 3600L + 30) * 1_000L,
                31_000_000_000L,
            ),
        )!!
        check(!totUpdate.discontinuity)
        check(totUpdate.authority.tableId == AribBroadcastClock.TABLE_ID_TOT)
        check(totUpdate.authority.generation == first.generation)
        check(AribBroadcastClock.continuityToleranceMillisForTest() == 2_000L)

        // Production scheduler integration: same-generation clock update must remove the old Runnable,
        // arm a new delay, and make a racing old Runnable inert by arm-sequence identity.
        val timers = FakeTimers()
        val due = mutableListOf<Pair<String, ByteArray>>()
        var clockGeneration = 11L
        var delayMillis = 1_000L
        val trackId = "superimpose:512"
        val scheduler = BroadcastTimedPesScheduler(
            resolveDeadline = { _, expectedGeneration ->
                if (expectedGeneration != null && expectedGeneration != clockGeneration) null
                else AribBroadcastClock.Deadline(clockGeneration, delayMillis)
            },
            currentPlaybackGeneration = { 7L },
            currentTrackId = { trackId },
            dispatch = { action -> action() },
            postDelayed = timers::post,
            removeCallbacks = timers::remove,
            onDue = { id, bytes -> due += id to bytes },
        )
        scheduler.submit(trackId, byteArrayOf(1, 2, 3), statement)
        check(timers.posted.size == 1)
        val oldArm = timers.posted.single()
        check(oldArm.delayMillis == 1_000L)

        delayMillis = 400L
        scheduler.onClockChanged()
        check(timers.removed == listOf(oldArm.runnable))
        check(timers.posted.size == 2)
        val rearmed = timers.posted.last()
        check(rearmed.delayMillis == 400L)
        check(rearmed.runnable !== oldArm.runnable)

        // removeCallbacksとmain-loop dispatchが競合して旧Runnableが実行されてもcurrent armを壊さない。
        oldArm.runnable.run()
        check(due.isEmpty())
        check(timers.posted.size == 2)

        delayMillis = 0L
        rearmed.runnable.run()
        check(due.size == 1)
        check(due.single().first == trackId)
        check(due.single().second.contentEquals(byteArrayOf(1, 2, 3)))
    }

    @Test
    fun timing10UsesNearestDayAndMidnightUpdateKeepsGeneration() {
        val first = firstAuthority(
            mjd = 60_000,
            millisOfDay = (23 * 3600L + 59 * 60L + 59) * 1_000L,
            receivedNanoTime = 1_000_000_000L,
        )
        val statement = AribBroadcastClock.StatementTime(1_000L)
        check(
            AribBroadcastClock.deadlineUntil(
                statement,
                first,
                nowNanoTime = 1_000_000_000L,
            )?.delayMillis == 2_000L,
        )

        val update = AribBroadcastClock.updateAuthority(
            first,
            AribBroadcastClock.SourceSample(
                AribBroadcastClock.TABLE_ID_TDT,
                60_001,
                1_000L,
                3_000_000_000L,
            ),
        )!!
        check(!update.discontinuity)
        check(update.authority.generation == first.generation)
    }

    @Test
    fun timing10FailsClosedWithoutAuthorityOrAcrossDiscontinuity() {
        val statement = AribBroadcastClock.StatementTime((12 * 3600L + 6 * 60L) * 1_000L)
        check(AribBroadcastClock.deadlineUntil(statement, null, nowNanoTime = 0L) == null)

        val first = firstAuthority(
            millisOfDay = 12 * 3600L * 1_000L,
            receivedNanoTime = 1_000_000_000L,
        )
        val update = AribBroadcastClock.updateAuthority(
            first,
            AribBroadcastClock.SourceSample(
                AribBroadcastClock.TABLE_ID_TDT,
                60_000,
                (12 * 3600L + 5 * 60L) * 1_000L,
                6_000_000_000L,
            ),
        )!!
        check(update.discontinuity)
        check(update.authority.generation != first.generation)
        check(
            AribBroadcastClock.deadlineUntil(
                statement,
                update.authority,
                expectedGeneration = first.generation,
                nowNanoTime = 6_000_000_000L,
            ) == null,
        )

        // Production scheduler integration: a new clock generation cancels the pending arm.
        // Even if the removed Runnable races and executes, it must not display or re-arm.
        val timers = FakeTimers()
        val due = mutableListOf<String>()
        var clockGeneration = 21L
        val trackId = "superimpose:513"
        val scheduler = BroadcastTimedPesScheduler(
            resolveDeadline = { _, expectedGeneration ->
                if (expectedGeneration != null && expectedGeneration != clockGeneration) null
                else AribBroadcastClock.Deadline(clockGeneration, 1_000L)
            },
            currentPlaybackGeneration = { 8L },
            currentTrackId = { trackId },
            dispatch = { action -> action() },
            postDelayed = timers::post,
            removeCallbacks = timers::remove,
            onDue = { id, _ -> due += id },
        )
        scheduler.submit(trackId, byteArrayOf(9), statement)
        val oldArm = timers.posted.single()
        clockGeneration = 22L
        scheduler.onClockChanged()
        check(timers.removed == listOf(oldArm.runnable))
        check(timers.posted.size == 1)

        oldArm.runnable.run()
        check(due.isEmpty())
        check(timers.posted.size == 1)
    }
}

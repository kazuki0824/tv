package com.maleicacid.tvinput.tis

/**
 * Timing=10文字スーパーのpending STMと遅延Runnableを単一所有する。
 *
 * broadcast clock自体は所有せず、deadline解決はTunerController側のclock authorityへ委譲する。
 * 同一pendingのre-arm時はarm sequenceを更新し、removeCallbacksと競合して旧Runnableが実行されても
 * current armを変更しない。
 */
internal class BroadcastTimedPesScheduler(
    private val resolveDeadline: (AribBroadcastClock.StatementTime, Long?) -> AribBroadcastClock.Deadline?,
    private val currentPlaybackGeneration: () -> Long,
    private val currentTrackId: () -> String?,
    private val dispatch: (() -> Unit) -> Unit,
    private val postDelayed: (Runnable, Long) -> Unit,
    private val removeCallbacks: (Runnable) -> Unit,
    private val onDue: (String, ByteArray) -> Unit,
) {
    private data class Pending(
        val trackId: String,
        val pesData: ByteArray,
        val statementTime: AribBroadcastClock.StatementTime,
        val playbackGeneration: Long,
        val clockGeneration: Long,
    )

    private data class Armed(
        val sequence: Long,
        val runnable: Runnable,
    )

    private val pending = linkedMapOf<Long, Pending>()
    private val armed = linkedMapOf<Long, Armed>()
    private var nextToken: Long = 0L
    private var nextArmSequence: Long = 0L

    fun submit(
        trackId: String,
        pesData: ByteArray,
        statementTime: AribBroadcastClock.StatementTime,
    ) {
        if (currentTrackId() != trackId) return
        val deadline = resolveDeadline(statementTime, null) ?: return
        val token = nextToken()
        pending[token] = Pending(
            trackId = trackId,
            pesData = pesData.copyOf(),
            statementTime = statementTime,
            playbackGeneration = currentPlaybackGeneration(),
            clockGeneration = deadline.clockGeneration,
        )
        arm(token, deadline)
    }

    fun onClockChanged() {
        pending.keys.toList().forEach { token ->
            val item = pending[token] ?: return@forEach
            val deadline = resolveDeadline(item.statementTime, item.clockGeneration)
            if (deadline == null) {
                cancel(token)
            } else {
                arm(token, deadline)
            }
        }
    }

    fun cancelAll() {
        armed.values.forEach { removeCallbacks(it.runnable) }
        armed.clear()
        pending.clear()
    }

    private fun arm(token: Long, deadline: AribBroadcastClock.Deadline) {
        armed.remove(token)?.let { removeCallbacks(it.runnable) }
        val item = pending[token] ?: return
        if (item.playbackGeneration != currentPlaybackGeneration()
            || item.clockGeneration != deadline.clockGeneration
            || item.trackId != currentTrackId()
        ) {
            pending.remove(token)
            return
        }
        if (deadline.delayMillis <= 0L) {
            pending.remove(token)
            onDue(item.trackId, item.pesData)
            return
        }

        val sequence = nextArmSequence()
        lateinit var runnable: Runnable
        runnable = Runnable {
            dispatch {
                val currentArm = armed[token]
                if (currentArm?.sequence != sequence || currentArm.runnable !== runnable) return@dispatch
                armed.remove(token)
                val current = pending[token] ?: return@dispatch
                val remaining = resolveDeadline(current.statementTime, current.clockGeneration)
                if (remaining == null) {
                    pending.remove(token)
                } else {
                    arm(token, remaining)
                }
            }
        }
        armed[token] = Armed(sequence, runnable)
        postDelayed(runnable, deadline.delayMillis)
    }

    private fun cancel(token: Long) {
        armed.remove(token)?.let { removeCallbacks(it.runnable) }
        pending.remove(token)
    }

    private fun nextToken(): Long {
        nextToken = if (nextToken == Long.MAX_VALUE) 1L else nextToken + 1L
        return nextToken
    }

    private fun nextArmSequence(): Long {
        nextArmSequence = if (nextArmSequence == Long.MAX_VALUE) 1L else nextArmSequence + 1L
        return nextArmSequence
    }
}

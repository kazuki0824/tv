package com.maleicacid.tvinput.tis

// 時計・配送・予約・取消しの独立した注入境界を既存の引数として明示する。

/**
 * Timing=10文字スーパーのpending STMと遅延Runnableを単一所有する。
 *
 * broadcast clock自体は所有せず、deadline解決はTunerController側のclock authorityへ委譲する。
 * 同一pendingのre-arm時はarm sequenceを更新し、removeCallbacksと競合して旧Runnableが実行されても
 * current armを変更しない。
 */
@Suppress("LongParameterList")
internal class BroadcastTimedPesScheduler(
    private val resolveDeadline: (AribBroadcastClock.StatementTime, Long?) -> AribBroadcastClock.Deadline?,
    private val currentPlaybackGeneration: () -> Long,
    private val currentTrackId: () -> String?,
    private val dispatch: (() -> Unit) -> Unit,
    private val postDelayed: (Runnable, Long) -> Unit,
    private val removeCallbacks: (Runnable) -> Unit,
    private val onDue: (String, ByteArray) -> Unit,
    private val onDrop: (String) -> Unit = {},
    private val maxPendingItems: Int = DEFAULT_MAX_PENDING_ITEMS,
    private val maxPendingBytes: Long = DEFAULT_MAX_PENDING_BYTES,
    private val maxFutureDelayMillis: Long = DEFAULT_MAX_FUTURE_DELAY_MILLIS,
) {
    private class Pending(
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
    private var pendingBytes: Long = 0L

    @Suppress("ReturnCount")
    fun submit(
        trackId: String,
        pesData: ByteArray,
        statementTime: AribBroadcastClock.StatementTime,
    ) {
        if (currentTrackId() != trackId) return
        val deadline = resolveDeadline(statementTime, null) ?: return
        if (deadline.delayMillis > maxFutureDelayMillis) {
            onDrop("将来時刻の上限を超過しました")
            return
        }
        val copied = pesData.copyOf()
        val byteCount = copied.size.toLong()
        val budgetExceeded =
            pending.size >= maxPendingItems ||
                byteCount > maxPendingBytes ||
                pendingBytes > maxPendingBytes - byteCount
        if (budgetExceeded) {
            onDrop("pending予算を使い切りました")
            return
        }
        val token = nextToken()
        pending[token] =
            Pending(
                trackId = trackId,
                pesData = copied,
                statementTime = statementTime,
                playbackGeneration = currentPlaybackGeneration(),
                clockGeneration = deadline.clockGeneration,
            )
        pendingBytes += byteCount
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
        pendingBytes = 0L
    }

    // 入力拒否・未準備・失敗を発生点で返し、成功経路を深い入れ子にしない。
    @Suppress("ReturnCount")
    private fun arm(
        token: Long,
        deadline: AribBroadcastClock.Deadline,
    ) {
        armed.remove(token)?.let { removeCallbacks(it.runnable) }
        val item = pending[token] ?: return
        if (item.playbackGeneration != currentPlaybackGeneration() ||
            item.clockGeneration != deadline.clockGeneration ||
            item.trackId != currentTrackId()
        ) {
            removePending(token)
            return
        }
        if (deadline.delayMillis > maxFutureDelayMillis) {
            removePending(token)
            onDrop("将来時刻の上限を超過しました")
            return
        }
        if (deadline.delayMillis <= 0L) {
            removePending(token)
            onDue(item.trackId, item.pesData)
            return
        }

        val sequence = nextArmSequence()
        lateinit var runnable: Runnable
        runnable =
            Runnable {
                dispatch {
                    val currentArm = armed[token]
                    if (currentArm?.sequence != sequence || currentArm.runnable !== runnable) return@dispatch
                    armed.remove(token)
                    val current = pending[token] ?: return@dispatch
                    val remaining = resolveDeadline(current.statementTime, current.clockGeneration)
                    if (remaining == null) {
                        removePending(token)
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
        removePending(token)
    }

    private fun removePending(token: Long): Pending? {
        val item = pending.remove(token) ?: return null
        pendingBytes = (pendingBytes - item.pesData.size.toLong()).coerceAtLeast(0L)
        return item
    }

    private fun nextToken(): Long {
        nextToken = if (nextToken == Long.MAX_VALUE) 1L else nextToken + 1L
        return nextToken
    }

    private fun nextArmSequence(): Long {
        nextArmSequence = if (nextArmSequence == Long.MAX_VALUE) 1L else nextArmSequence + 1L
        return nextArmSequence
    }

    private companion object {
        const val DEFAULT_MAX_PENDING_ITEMS = 64
        const val DEFAULT_MAX_PENDING_BYTES = 2L * 1024L * 1024L
        const val DEFAULT_MAX_FUTURE_DELAY_MILLIS = 60_000L
    }
}

package com.maleicacid.tvinput.tis

import kotlin.math.abs

/** Rust側で構造化済みのTDT/TOTとSTMをbroadcast wall-clock domainで相関する。 */
object AribBroadcastClock {
    const val TABLE_ID_TDT: Int = 0x70
    const val TABLE_ID_TOT: Int = 0x73

    data class SourceSample(
        val tableId: Int,
        val mjd: Int,
        val millisOfDay: Long,
        val receivedNanoTime: Long,
    )

    data class AuthoritySample(
        val tableId: Int,
        val mjd: Int,
        val millisOfDay: Long,
        val receivedNanoTime: Long,
        val generation: Long,
    )

    data class AuthorityUpdate(
        val authority: AuthoritySample,
        val discontinuity: Boolean,
    )

    data class StatementTime(
        val millisOfDay: Long,
    )

    data class Deadline(
        val clockGeneration: Long,
        val delayMillis: Long,
    )

    // この処理の規格値・ビット幅・単位換算・固定上限をリテラルのまま照合できる形に保つ。
    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    // 入力拒否・未準備・失敗を発生点で返し、成功経路を深い入れ子にしない。
    @Suppress("MagicNumber", "MaxLineLength", "ReturnCount")
    fun updateAuthority(
        previous: AuthoritySample?,
        incoming: SourceSample,
    ): AuthorityUpdate? {
        if (!validSource(incoming)) return null
        if (previous == null) {
            return AuthorityUpdate(
                AuthoritySample(
                    incoming.tableId,
                    incoming.mjd,
                    incoming.millisOfDay,
                    incoming.receivedNanoTime,
                    1L,
                ),
                discontinuity = false,
            )
        }
        val elapsedNanos = incoming.receivedNanoTime - previous.receivedNanoTime
        val discontinuity =
            if (elapsedNanos < 0L) {
                true
            } else {
                val expectedBroadcastMillis = absoluteMillis(previous.mjd, previous.millisOfDay) + elapsedNanos / 1_000_000L
                val actualBroadcastMillis = absoluteMillis(incoming.mjd, incoming.millisOfDay)
                abs(actualBroadcastMillis - expectedBroadcastMillis) > CONTINUITY_TOLERANCE_MILLIS
            }
        val generation = if (discontinuity) nextGeneration(previous.generation) else previous.generation
        return AuthorityUpdate(
            AuthoritySample(
                incoming.tableId,
                incoming.mjd,
                incoming.millisOfDay,
                incoming.receivedNanoTime,
                generation,
            ),
            discontinuity,
        )
    }

    // この処理の規格値・ビット幅・単位換算・固定上限をリテラルのまま照合できる形に保つ。
    // 入力拒否・未準備・失敗を発生点で返し、成功経路を深い入れ子にしない。
    @Suppress("MagicNumber", "ReturnCount")
    fun deadlineUntil(
        statementTime: StatementTime,
        authority: AuthoritySample?,
        expectedGeneration: Long? = null,
        nowNanoTime: Long = System.nanoTime(),
    ): Deadline? {
        val clock = authority ?: return null
        if (expectedGeneration != null && expectedGeneration != clock.generation) return null
        if (!validAuthority(clock) || statementTime.millisOfDay !in 0 until DAY_MILLIS) return null
        val elapsedNanos = nowNanoTime - clock.receivedNanoTime
        if (elapsedNanos < 0L) return null
        val currentBroadcastMillis = absoluteMillis(clock.mjd, clock.millisOfDay) + elapsedNanos / 1_000_000L
        val currentMjd = Math.floorDiv(currentBroadcastMillis, DAY_MILLIS)
        val target =
            longArrayOf(currentMjd - 1L, currentMjd, currentMjd + 1L)
                .map { day -> day * DAY_MILLIS + statementTime.millisOfDay }
                .minByOrNull { candidate -> abs(candidate - currentBroadcastMillis) }
                ?: return null
        return Deadline(clock.generation, (target - currentBroadcastMillis).coerceAtLeast(0L))
    }

    internal fun continuityToleranceMillisForTest(): Long = CONTINUITY_TOLERANCE_MILLIS

    // この処理の規格値・ビット幅・単位換算・固定上限をリテラルのまま照合できる形に保つ。
    @Suppress("MagicNumber")
    private fun validSource(sample: SourceSample): Boolean =
        sample.tableId in setOf(TABLE_ID_TDT, TABLE_ID_TOT) &&
            sample.mjd in 0..0xffff &&
            sample.millisOfDay in 0 until DAY_MILLIS &&
            sample.receivedNanoTime >= 0L

    // この処理の規格値・ビット幅・単位換算・固定上限をリテラルのまま照合できる形に保つ。
    @Suppress("MagicNumber")
    private fun validAuthority(sample: AuthoritySample): Boolean =
        sample.tableId in setOf(TABLE_ID_TDT, TABLE_ID_TOT) &&
            sample.mjd in 0..0xffff &&
            sample.millisOfDay in 0 until DAY_MILLIS &&
            sample.receivedNanoTime >= 0L &&
            sample.generation > 0L

    private fun absoluteMillis(
        mjd: Int,
        millisOfDay: Long,
    ): Long = mjd.toLong() * DAY_MILLIS + millisOfDay

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    private fun nextGeneration(generation: Long): Long = if (generation == Long.MAX_VALUE) Long.MAX_VALUE else generation + 1L

    private const val CONTINUITY_TOLERANCE_MILLIS = 2_000L
    private const val DAY_MILLIS = 24L * 60L * 60L * 1_000L
}

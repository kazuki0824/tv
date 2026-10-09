@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import org.junit.Test

class RuntimeIdentityTest {
    @Test
    fun checkedIdentityExhaustionAndCollisionSafeReuse() {
        check(
            runCatching { RuntimeIdentity.nextLong(Long.MAX_VALUE, "Long識別子") }
                .exceptionOrNull() is IllegalStateException,
        )
        check(
            runCatching { RuntimeIdentity.nextInt(Int.MAX_VALUE, "Int識別子") }
                .exceptionOrNull() is IllegalStateException,
        )

        check(
            RuntimeIdentity.nextReusablePositiveLong(
                current = Long.MAX_VALUE,
                live = setOf(1L, 2L),
                label = "Longトークン",
            ) == 3L,
        )
    }

    @Test
    fun broadcastClockGenerationExhaustionDoesNotSaturateOrReuse() {
        val previous =
            AribBroadcastClock.AuthoritySample(
                tableId = AribBroadcastClock.TABLE_ID_TDT,
                mjd = 60_000,
                millisOfDay = 0L,
                receivedNanoTime = 0L,
                generation = Long.MAX_VALUE,
            )
        val incoming =
            AribBroadcastClock.SourceSample(
                tableId = AribBroadcastClock.TABLE_ID_TDT,
                mjd = 60_000,
                millisOfDay = 60_000L,
                receivedNanoTime = 1_000_000L,
            )

        check(
            runCatching { AribBroadcastClock.updateAuthority(previous, incoming) }
                .exceptionOrNull() is IllegalStateException,
        )
    }
}

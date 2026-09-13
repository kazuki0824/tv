// テストの入力・期待値を本体の定数と独立した具体値で記述する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.reservation

import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.db.ReservationRecord
import org.junit.Assert.assertTrue
import org.junit.Test

class ReservationPolicyTest {
    @Test
    fun higherPriorityReservationComesFirst() {
        val key = ServiceKey(1, 2, 3)
        val high = ReservationRecord(1, key, null, 1000, 2000, 10)
        val low = ReservationRecord(2, key, null, 1000, 2000, 1)
        assertTrue(ReservationPolicy().comparePriority(high, low) < 0)
    }
}

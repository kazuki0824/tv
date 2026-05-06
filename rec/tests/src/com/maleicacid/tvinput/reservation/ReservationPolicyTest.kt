package com.maleicacid.tvinput.reservation

import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.db.ReservationRecord
import org.junit.Assert.assertTrue
import org.junit.Test

class ReservationPolicyTest {
    @Test
    fun 優先度が高い予約を先に並べる() {
        val key = ServiceKey(1, 2, 3)
        val high = ReservationRecord(1, key, null, 1000, 2000, 10)
        val low = ReservationRecord(2, key, null, 1000, 2000, 1)
        assertTrue(ReservationPolicy().comparePriority(high, low) < 0)
    }
}

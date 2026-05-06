package com.maleicacid.tvinput.reservation

import com.maleicacid.tvinput.db.ReservationRecord

class ReservationPolicy {
    fun comparePriority(left: ReservationRecord, right: ReservationRecord): Int {
        return right.priority.compareTo(left.priority)
    }
}

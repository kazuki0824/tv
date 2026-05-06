package com.maleicacid.tvinput.reservation

import android.content.Context
import android.util.Log
import com.maleicacid.tvinput.common.LogTags
import com.maleicacid.tvinput.db.ReservationRecord

class ReservationScheduler(private val context: Context) {
    fun schedule(record: ReservationRecord) {
        Log.i(
            LogTags.RESERVATION,
            "予約を計画します id=${record.reservationId} start=${record.startTimeMillis} end=${record.endTimeMillis}",
        )
    }
}

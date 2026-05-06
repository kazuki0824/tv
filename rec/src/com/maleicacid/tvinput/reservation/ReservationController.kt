package com.maleicacid.tvinput.reservation

import android.content.Context
import android.util.Log
import com.maleicacid.tvinput.common.LogTags
import com.maleicacid.tvinput.db.ReservationRecord
import com.maleicacid.tvinput.db.TvInputDatabase

class ReservationController(private val context: Context) {
    private val database = TvInputDatabase()
    private val scheduler = ReservationScheduler(context)
    private val recordingClientController = RecordingClientController(context)

    fun addReservation(record: ReservationRecord) {
        database.putReservation(record)
        scheduler.schedule(record)
    }

    fun restoreSchedules() {
        val reservations = database.listReservations()
        Log.i(LogTags.RESERVATION, "予約 ${reservations.size} 件を復元します")
        reservations.forEach { scheduler.schedule(it) }
    }

    fun startRecording(record: ReservationRecord) {
        recordingClientController.start(record)
    }

    fun stopRecording(record: ReservationRecord) {
        recordingClientController.stop(record)
    }
}

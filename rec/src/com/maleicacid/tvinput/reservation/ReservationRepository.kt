package com.maleicacid.tvinput.reservation

import com.maleicacid.tvinput.db.ReservationRecord
import com.maleicacid.tvinput.db.TvInputDatabase

class ReservationRepository(private val database: TvInputDatabase) {
    fun save(record: ReservationRecord) = database.putReservation(record)
    fun list(): List<ReservationRecord> = database.listReservations()
}

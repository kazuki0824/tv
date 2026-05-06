package com.maleicacid.tvinput.db

import com.maleicacid.tvinput.common.ServiceKey

data class ReservationRecord(
    val reservationId: Long,
    val serviceKey: ServiceKey,
    val eventId: Int?,
    val startTimeMillis: Long,
    val endTimeMillis: Long,
    val priority: Int,
)

class TvInputDatabase {
    private val reservations = LinkedHashMap<Long, ReservationRecord>()

    @Synchronized
    fun putReservation(record: ReservationRecord) {
        reservations[record.reservationId] = record
    }

    @Synchronized
    fun listReservations(): List<ReservationRecord> = reservations.values.toList()
}

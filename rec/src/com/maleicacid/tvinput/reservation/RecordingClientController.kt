package com.maleicacid.tvinput.reservation

import android.content.Context
import android.media.tv.TvRecordingClient
import android.util.Log
import com.maleicacid.tvinput.common.LogTags
import com.maleicacid.tvinput.db.ReservationRecord

class RecordingClientController(private val context: Context) {
    private var client: TvRecordingClient? = null

    fun start(record: ReservationRecord) {
        Log.i(LogTags.RESERVATION, "予約録画を開始します reservation=${record.reservationId}")
    }

    fun stop(record: ReservationRecord) {
        Log.i(LogTags.RESERVATION, "予約録画を停止します reservation=${record.reservationId}")
        client?.stopRecording()
    }
}

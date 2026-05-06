package com.maleicacid.tvinput.reservation

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log
import com.maleicacid.tvinput.common.LogTags

class ReservationAlarmReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        Log.i(LogTags.RESERVATION, "予約 alarm を受信しました: ${intent.action}")
        context.startService(Intent(context, ReservationManagerService::class.java).apply {
            action = ACTION_HANDLE_ALARM
        })
    }

    companion object {
        const val ACTION_HANDLE_ALARM = "com.maleicacid.tvinput.reservation.HANDLE_ALARM"
    }
}

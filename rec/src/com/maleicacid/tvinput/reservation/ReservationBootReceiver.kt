package com.maleicacid.tvinput.reservation

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log
import com.maleicacid.tvinput.common.LogTags

class ReservationBootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        Log.i(LogTags.RESERVATION, "起動 event を受信しました: ${intent.action}")
        context.startService(Intent(context, ReservationManagerService::class.java))
    }
}

package com.maleicacid.tvinput.tis

import android.content.BroadcastReceiver
import android.content.Context
import android.content.IntentFilter
import android.os.Build

object ReceiverRegistration {
    fun registerNotExported(context: Context, receiver: BroadcastReceiver, filter: IntentFilter) {
        if (Build.VERSION.SDK_INT >= 33) {
            context.registerReceiver(receiver, filter, Context.RECEIVER_NOT_EXPORTED)
        } else {
            @Suppress("DEPRECATION")
            context.registerReceiver(receiver, filter)
        }
    }
}

package com.maleicacid.tvinput.tis

import android.content.BroadcastReceiver
import android.content.Context
import android.content.IntentFilter

object ReceiverRegistration {
    fun registerNotExported(
        context: Context,
        receiver: BroadcastReceiver,
        filter: IntentFilter,
    ) {
        context.registerReceiver(receiver, filter, Context.RECEIVER_NOT_EXPORTED)
    }
}

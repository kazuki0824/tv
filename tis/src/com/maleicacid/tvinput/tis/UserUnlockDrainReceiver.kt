package com.maleicacid.tvinput.tis

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

class UserUnlockDrainReceiver(private val source: String) : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != Intent.ACTION_USER_UNLOCKED) return
        BootEpgSyncScheduler.scheduleIfEligible(context.applicationContext, source)
    }
}

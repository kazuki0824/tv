package com.maleicacid.tvinput.tis

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

/**
 * r51最小boot後EPG同期入口。
 * LOCKED_BOOT_COMPLETED では device protected storage へ pending だけを残し、TvProvider/Tuner/JNI は起動しない。
 */
class EpgBootSyncReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        when (intent.action) {
            Intent.ACTION_LOCKED_BOOT_COMPLETED -> {
                DirectBootGuard.onLockedBootCompleted(context.applicationContext, System.currentTimeMillis(), intent.action.orEmpty())
            }
            Intent.ACTION_BOOT_COMPLETED -> {
                DirectBootGuard.markBootEpgSyncRequested(context.applicationContext, intent.action.orEmpty())
                BootEpgSyncScheduler.scheduleIfEligible(context.applicationContext, "BOOT_COMPLETED")
            }
        }
    }
}

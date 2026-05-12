package com.maleicacid.tvinput.tis

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log
import com.maleicacid.tvinput.common.AppIds
import com.maleicacid.tvinput.common.LogTags

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
                Log.i(LogTags.TIS, "boot後EPG最小再同期の pending drain を確認します action=${intent.action}")
                if (DirectBootGuard.drainIfUserUnlocked(context.applicationContext, "BOOT_COMPLETED", System.currentTimeMillis()) == DirectBootGuard.DrainDecision.START_BOOT_EPG_SYNC) {
                    ChannelScanManager.startBootEpgSyncIfIdle(context.applicationContext, AppIds.TV_INPUT_SERVICE)
                }
            }
        }
    }
}

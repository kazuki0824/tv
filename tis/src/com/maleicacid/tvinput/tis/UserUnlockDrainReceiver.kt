package com.maleicacid.tvinput.tis

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

class UserUnlockDrainReceiver(
    private val channelScanManager: ChannelScanManager = ChannelScanManager,
    private val nowMillis: () -> Long = { System.currentTimeMillis() },
    private val source: String,
) : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != Intent.ACTION_USER_UNLOCKED) return
        if (DirectBootGuard.drainIfUserUnlocked(context.applicationContext, source, nowMillis()) == DirectBootGuard.DrainDecision.START_BOOT_EPG_SYNC) {
            channelScanManager.startBootEpgSyncIfIdle(context.applicationContext, com.maleicacid.tvinput.common.AppIds.TV_INPUT_SERVICE)
        }
    }
}

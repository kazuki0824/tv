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
            val resolvedInputId = TisInputIdResolver.resolveOwnInputId(context.applicationContext)
            if (resolvedInputId == null) {
                DirectBootGuard.deferPending(context.applicationContext, "TV_INPUT_ID_UNRESOLVED")
            } else {
                channelScanManager.startBootEpgSyncIfIdle(context.applicationContext, resolvedInputId)
            }
        }
    }
}

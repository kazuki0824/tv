package com.maleicacid.tvinput.tis

import android.content.AttributionSource
import android.content.Intent
import android.content.IntentFilter
import android.media.tv.TvInputService
import android.util.Log
import com.maleicacid.tvinput.common.LogTags

class MaleicacidTvInputService : TvInputService() {
    private var userUnlockDrainReceiver: UserUnlockDrainReceiver? = null
    private var userUnlockDrainRegistered: Boolean = false

    override fun onCreate() {
        super.onCreate()
        registerUserUnlockDrainReceiver()
        // Boot EPG sync is intentionally not started from service onCreate().
        // Live-session creation may follow immediately after service creation; boot drain is
        // handled by boot/unlock receivers and after live-session release to avoid tuner races.
    }

    override fun onCreateSession(inputId: String): Session {
        val fallbackSessionId = fallbackSessionId(inputId)
        Log.i(LogTags.TIS, "旧1引数 onCreateSession 経路でライブセッションを作成します inputId=$inputId fallbackSessionId=$fallbackSessionId")
        return createLiveSession(inputId, fallbackSessionId, null)
    }

    override fun onCreateSession(inputId: String, sessionId: String): Session {
        Log.i(LogTags.TIS, "ライブセッションを作成します inputId=$inputId sessionId=$sessionId")
        return createLiveSession(inputId, sessionId, null)
    }

    override fun onCreateSession(inputId: String, sessionId: String, tvAppAttributionSource: AttributionSource): Session {
        Log.i(LogTags.TIS, "ライブセッションを作成します inputId=$inputId sessionId=$sessionId")
        return createLiveSession(inputId, sessionId, tvAppAttributionSource)
    }

    private fun createLiveSession(inputId: String, tvInputSessionId: String, attributionSource: AttributionSource?): Session {
        // r50bk12: boot/background maintenance must not start in the small window
        // between TvInputService.onCreateSession() entry and MaleicacidLiveSession
        // constructor registering an active live session. Mark this boundary explicitly
        // so ChannelScanManager can defer tuner-consuming work until session creation
        // either finishes or fails.
        ChannelScanManager.beginLiveSessionCreation()
        return try {
            MaleicacidLiveSession(this, inputId, tvInputSessionId, attributionSource)
        } finally {
            ChannelScanManager.finishLiveSessionCreation()
        }
    }

    private fun fallbackSessionId(inputId: String): String = legacyFallbackSessionIdForTest(inputId)

    override fun onCreateRecordingSession(inputId: String): RecordingSession? {
        Log.i(LogTags.TIS, "録画はこの TIS APK の対象外です。inputId=$inputId")
        return null
    }

    override fun onDestroy() {
        unregisterUserUnlockDrainReceiver()
        super.onDestroy()
    }

    private fun registerUserUnlockDrainReceiver() {
        if (userUnlockDrainRegistered) return
        val receiver = UserUnlockDrainReceiver(source = "MaleicacidTvInputService.ACTION_USER_UNLOCKED")
        ReceiverRegistration.registerNotExported(this, receiver, IntentFilter(Intent.ACTION_USER_UNLOCKED))
        userUnlockDrainReceiver = receiver
        userUnlockDrainRegistered = true
    }

    private fun unregisterUserUnlockDrainReceiver() {
        if (!userUnlockDrainRegistered) return
        userUnlockDrainReceiver?.let { runCatching { unregisterReceiver(it) } }
        userUnlockDrainReceiver = null
        userUnlockDrainRegistered = false
    }

    companion object {
        fun api30SessionIdForTest(inputId: String, sessionId: String): String = sessionId.also {
            require(inputId.isNotBlank()) { "inputId must not be blank" }
        }

        fun legacyFallbackSessionIdForTest(inputId: String): String = "maleicacid-$inputId-${System.nanoTime()}"
    }
}

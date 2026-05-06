package com.maleicacid.tvinput.tis

import android.content.AttributionSource
import android.media.tv.TvInputService
import android.util.Log
import com.maleicacid.tvinput.common.LogTags

class MaleicacidTvInputService : TvInputService() {
    override fun onCreateSession(inputId: String): Session {
        val fallbackSessionId = fallbackSessionId(inputId)
        Log.i(LogTags.TIS, "ライブセッションを作成します inputId=$inputId fallbackSessionId=$fallbackSessionId")
        return createLiveSession(inputId, fallbackSessionId, null)
    }

    override fun onCreateSession(inputId: String, sessionId: String, tvAppAttributionSource: AttributionSource): Session {
        Log.i(LogTags.TIS, "ライブセッションを作成します inputId=$inputId sessionId=$sessionId")
        return createLiveSession(inputId, sessionId, tvAppAttributionSource)
    }

    private fun createLiveSession(inputId: String, tvInputSessionId: String, attributionSource: AttributionSource?): Session =
        MaleicacidLiveSession(this, inputId, tvInputSessionId, attributionSource)

    private fun fallbackSessionId(inputId: String): String = "maleicacid-$inputId-${System.nanoTime()}"

    override fun onCreateRecordingSession(inputId: String): RecordingSession? {
        Log.i(LogTags.TIS, "録画はこの TIS APK の対象外です。inputId=$inputId")
        return null
    }
}

package com.maleicacid.tvinput.tis

import android.os.Looper
import org.junit.Assert.assertNull
import org.junit.Test

class RecordingDisabledR51Test {
    @Test
    fun r51DoesNotCreateRecordingSession() {
        if (Looper.myLooper() == null) {
            Looper.prepare()
        }
        val service = MaleicacidTvInputService()
        assertNull(service.onCreateRecordingSession("maleicacid-test-input"))
    }
}

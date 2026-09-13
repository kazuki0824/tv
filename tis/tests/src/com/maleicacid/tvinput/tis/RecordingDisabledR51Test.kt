package com.maleicacid.tvinput.tis

import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertNull
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class RecordingDisabledR51Test {
    @Test
    fun r51DoesNotCreateRecordingSession() {
        val service = MaleicacidTvInputService()
        assertNull(service.onCreateRecordingSession("maleicacid-test-input"))
    }
}

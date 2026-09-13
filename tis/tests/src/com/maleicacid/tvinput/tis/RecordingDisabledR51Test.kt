package com.maleicacid.tvinput.tis

import org.junit.Assert.assertNull
import org.junit.Test

class RecordingDisabledR51Test {
    @Test
    fun r51DoesNotCreateRecordingSession() {
        val service = MaleicacidTvInputService()
        assertNull(service.onCreateRecordingSession("maleicacid-test-input"))
    }
}

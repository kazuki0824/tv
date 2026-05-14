package com.maleicacid.tvinput.tis

import org.junit.Assert.assertNull
import org.junit.Test

class RecordingDisabledR51Test {
    @Test
    fun r51では録画セッションを作成しない() {
        val service = MaleicacidTvInputService()
        assertNull(service.onCreateRecordingSession("maleicacid-test-input"))
    }
}

package com.maleicacid.tvinput.tis

import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.db.ProgramDescriptors
import com.maleicacid.tvinput.db.ProgramRecord
import org.junit.Test

class ProgramPublishCoordinatorR51FixTest {
    @Test fun projectedNullClearChangesSignature() {
        val key = ServiceKey(4, 16625, 101)
        val p = ProgramRecord(
            key, 1, "p1", 1_700_000_000_000L, 1_800_000L, "title", "desc",
            descriptors = ProgramDescriptors(audioLanguage = "jpn"),
        )
        val withoutAudio = p.copy(descriptors = p.descriptors.copy(audioLanguage = null))
        check(ProgramPublishCoordinator.programSignatureForTest(listOf(p)) != ProgramPublishCoordinator.programSignatureForTest(listOf(withoutAudio)))
    }
}

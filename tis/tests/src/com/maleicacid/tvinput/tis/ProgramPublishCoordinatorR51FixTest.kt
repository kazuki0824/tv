package com.maleicacid.tvinput.tis

import com.maleicacid.tvinput.aribsi.AribComponentEntry
import com.maleicacid.tvinput.aribsi.AribComponents
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.db.ProgramDescriptors
import com.maleicacid.tvinput.db.ProgramRecord
import org.junit.Test

class ProgramPublishCoordinatorR51FixTest {
    @Test fun projectedNullClearChangesSignature() {
        val key = ServiceKey(4, 16625, 101)
        val p = ProgramRecord(
            key, 1, "p1", 1_700_000_000_000L, 1_800_000L, "title", "desc",
            descriptors = ProgramDescriptors(components = AribComponents(audio = listOf(AribComponentEntry(esPid = 256, streamType = 0x0f, componentTag = 1, componentType = 3, codec = "AAC", language = "jpn", parseStatus = "OK")))),
        )
        val withoutAudio = p.copy(descriptors = p.descriptors.copy(components = AribComponents()))
        check(ProgramPublishCoordinator.programSignatureForTest(listOf(p)) != ProgramPublishCoordinator.programSignatureForTest(listOf(withoutAudio)))
    }
}

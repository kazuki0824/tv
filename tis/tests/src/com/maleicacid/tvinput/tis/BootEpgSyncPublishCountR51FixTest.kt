package com.maleicacid.tvinput.tis

import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.db.ProgramRecord
import org.junit.Test

class BootEpgSyncPublishCountR51FixTest {
    @Test fun changedCountIncludesDelete() {
        val result = ProgramPublishCoordinator.ProgramPublishResult(
            inserted = 1,
            updated = 2,
            deleted = 5,
            skippedUnchanged = 3,
            skippedNoChannel = 4,
        )
        check(result.changed == 8)
        check(result.deleted == 5)
    }

    @Test fun skippedNoChannelDoesNotChangeCount() {
        val result = ProgramPublishCoordinator.ProgramPublishResult(0, 0, skippedNoChannel = 1)
        check(result.changed == 0)
    }
}

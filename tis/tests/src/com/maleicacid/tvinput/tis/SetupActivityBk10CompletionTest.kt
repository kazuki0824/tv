package com.maleicacid.tvinput.tis

import org.junit.Test

class SetupActivityBk10CompletionTest {
    @Test fun scanRequiresNonBlankOwnInputId() {
        check(!SetupActivity.scanStartAllowedForTest(null, isOwnInputId = true))
        check(!SetupActivity.scanStartAllowedForTest("", isOwnInputId = true))
        check(!SetupActivity.scanStartAllowedForTest("other.input", isOwnInputId = false))
        check(SetupActivity.scanStartAllowedForTest("own.input", isOwnInputId = true))
    }

    @Test fun onlyCurrentSetupGenerationCanFinishSetup() {
        val result = ChannelScanController.ScanResult(scanned = 1, published = 1, diagnostics = emptyList(), successfulCandidates = 1)
        val current = ScanState.Completed(result, generation = 7, purpose = ScanPurpose.SETUP_SCAN)
        val stale = ScanState.Completed(result, generation = 6, purpose = ScanPurpose.SETUP_SCAN)
        val boot = ScanState.Completed(result, generation = 7, purpose = ScanPurpose.BOOT_EPG_SYNC)
        val background = ScanState.Completed(result, generation = 7, purpose = ScanPurpose.BACKGROUND_MAINTENANCE)
        val empty = ScanState.Completed(result.copy(published = 0), generation = 7, purpose = ScanPurpose.SETUP_SCAN)

        check(SetupActivity.shouldFinishSetupForStateForTest(current, activeSetupGeneration = 7, invalidInputId = false))
        check(!SetupActivity.shouldFinishSetupForStateForTest(stale, activeSetupGeneration = 7, invalidInputId = false))
        check(!SetupActivity.shouldFinishSetupForStateForTest(boot, activeSetupGeneration = 7, invalidInputId = false))
        check(!SetupActivity.shouldFinishSetupForStateForTest(background, activeSetupGeneration = 7, invalidInputId = false))
        check(!SetupActivity.shouldFinishSetupForStateForTest(empty, activeSetupGeneration = 7, invalidInputId = false))
        check(!SetupActivity.shouldFinishSetupForStateForTest(current, activeSetupGeneration = null, invalidInputId = false))
        check(!SetupActivity.shouldFinishSetupForStateForTest(current, activeSetupGeneration = 7, invalidInputId = true))
    }
}

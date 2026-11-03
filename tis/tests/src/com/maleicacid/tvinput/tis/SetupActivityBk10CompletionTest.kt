package com.maleicacid.tvinput.tis

import org.junit.Test

class SetupActivityBk10CompletionTest {
    @Test fun scanRequiresNonBlankOwnInputId() {
        check(!SetupActivity.scanStartAllowed(null, isOwnInputId = true))
        check(!SetupActivity.scanStartAllowed("", isOwnInputId = true))
        check(!SetupActivity.scanStartAllowed("other.input", isOwnInputId = false))
        check(SetupActivity.scanStartAllowed("own.input", isOwnInputId = true))
    }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    @Test
    fun onlyCurrentSetupGenerationCanFinishSetup() {
        val result = ChannelScanController.ScanResult(scanned = 1, published = 1, diagnostics = emptyList(), successfulCandidates = 1)
        val current = ScanState.Completed(result, generation = 7, purpose = ScanPurpose.SETUP_SCAN)
        val stale = ScanState.Completed(result, generation = 6, purpose = ScanPurpose.SETUP_SCAN)
        val boot = ScanState.Completed(result, generation = 7, purpose = ScanPurpose.BOOT_EPG_SYNC)
        val background = ScanState.Completed(result, generation = 7, purpose = ScanPurpose.BACKGROUND_MAINTENANCE)
        val empty = ScanState.Completed(result.copy(published = 0), generation = 7, purpose = ScanPurpose.SETUP_SCAN)

        check(SetupActivity.shouldFinishSetupForState(current, activeSetupGeneration = 7, invalidInputId = false))
        check(!SetupActivity.shouldFinishSetupForState(stale, activeSetupGeneration = 7, invalidInputId = false))
        check(!SetupActivity.shouldFinishSetupForState(boot, activeSetupGeneration = 7, invalidInputId = false))
        check(!SetupActivity.shouldFinishSetupForState(background, activeSetupGeneration = 7, invalidInputId = false))
        check(!SetupActivity.shouldFinishSetupForState(empty, activeSetupGeneration = 7, invalidInputId = false))
        check(!SetupActivity.shouldFinishSetupForState(current, activeSetupGeneration = null, invalidInputId = false))
        check(!SetupActivity.shouldFinishSetupForState(current, activeSetupGeneration = 7, invalidInputId = true))
    }

    @Test fun setupActivityDoesNotTreatForeignInputAsOwn() {
        check(
            !TisInputIdResolver.isOwnInputInfo(
                infoId = "foreign.input",
                servicePackageName = "foreign.package",
                serviceName = "foreign.Service",
                ownPackageName = "com.maleicacid.tvinput",
                ownClassName = "com.maleicacid.tvinput.tis.MaleicacidTvInputService",
            ),
        )
    }
}

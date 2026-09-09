package com.maleicacid.tvinput.tis

import com.maleicacid.tvinput.common.FrequencyHz
import com.maleicacid.tvinput.common.StreamSelectorType
import com.maleicacid.tvinput.common.TransportStreamId16
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class ScanPlanPolicyTest {

    @Test
    fun bsResourceLossWakesCallerAndRejectsCandidatesAndPublication() {
        val controller = java.util.concurrent.Executors.newSingleThreadExecutor()
        val caller = java.util.concurrent.Executors.newSingleThreadExecutor()
        val fence = ChannelScanController.ResourceLossFence()
        val operation = TunerController.StreamIdDiscoveryOperation(9L)
        val waiting = java.util.concurrent.CountDownLatch(1)
        var notifications = 0
        var tuned = 0
        var collected = 0
        var published = 0
        try {
            val result = caller.submit<TunerController.StreamIdDiscoveryResult> {
                waiting.countDown()
                val completed = operation.await(5000)
                controller.submit<TunerController.StreamIdDiscoveryResult> { operation.result(completed) }.get()
            }
            check(waiting.await(1, java.util.concurrent.TimeUnit.SECONDS))
            controller.submit {
                TunerController.completeResourceLoss(
                    invalidate = { if (!operation.active) false else { operation.loseResources(); true } },
                    cleanup = { operation.cancel() },
                    notifyLost = { notifications++; fence.onLost(operation.generation) },
                )
                operation.reportIds(intArrayOf(16400)) // 喪失後の遅延報告を拒否する。
            }.get(1, java.util.concurrent.TimeUnit.SECONDS)
            val lost = result.get(1, java.util.concurrent.TimeUnit.SECONDS)
            check(lost.resourceLost && !lost.success && lost.message == "TUNER_RESOURCE_LOST")
            fence.activate(requireNotNull(lost.generation))
            for (candidate in lost.candidatesFor(JapanIsdbScanPlan.isdbsBsBands().first())) { tuned++; collected++ }
            fence.publishIfCurrent<Unit>(operation.generation) { published++ }
            check(notifications == 1 && fence.terminalObserved && tuned == 0 && collected == 0 && published == 0)
            val next = TunerController.StreamIdDiscoveryOperation(10L)
            next.reportIds(intArrayOf(16400)); next.complete()
            check(next.result(next.await(1)).success)
        } finally { caller.shutdownNow(); controller.shutdownNow() }
    }

    @Test
    fun scanReleaseRetryPreservesResourceLostTerminalAndRetainsOnlyFailedOwner() {
        val context = android.content.ContextWrapper(null)
        val task = ChannelScanManager.ActiveScanTask(1, ScanPurpose.SETUP_SCAN, context)
        val owner = java.util.concurrent.atomic.AtomicReference<ChannelScanManager.ActiveScanTask?>(task)
        val terminal = ScanState.Failed("TUNER_RESOURCE_LOST", 1, ScanPurpose.SETUP_SCAN)
        // Managerの実stateを検査する。テスト用の本番mutation APIは追加しない。
        val stateField = ChannelScanManager::class.java.getDeclaredField("state").apply { isAccessible = true }
        val previousState = ChannelScanManager.currentState()
        stateField.set(null, terminal)
        var diagnostics = 0
        var controllerCloses = 0
        var engineCloses = 0
        var reject = true
        task.controller = AutoCloseable { controllerCloses++; if (reject) error("close failed") }
        task.engine = AutoCloseable { engineCloses++ }
        try {
            check(!ChannelScanManager.finishScanRelease(task, owner) { diagnostics++ })
            check(owner.get() === task && task.closing && task.controller != null && task.engine == null)
            check(ChannelScanManager.currentState() === terminal && diagnostics == 1 && engineCloses == 1)
            reject = false
            check(ChannelScanManager.finishScanRelease(task, owner) { diagnostics++ })
            check(owner.get() == null && controllerCloses == 2 && engineCloses == 1)
            check(ChannelScanManager.currentState() === terminal && diagnostics == 1)
        } finally { stateField.set(null, previousState) }
    }

    @Test
    fun unavailableDynamicDiscoveryDoesNotInferUnsupportedCapability() {
        val seed = JapanIsdbScanPlan.isdbsBsBands().first()
        val failed = TunerController.StreamIdDiscoveryResult(false, setOf(16400), android.media.tv.tuner.Tuner.RESULT_UNAVAILABLE)
        assertTrue(failed.candidatesFor(seed).isEmpty())
        assertEquals(setOf(16400), failed.copy(success = true, resultCode = android.media.tv.tuner.Tuner.RESULT_SUCCESS)
            .candidatesFor(seed).mapNotNull { it.streamSelector.value }.toSet())
    }

    @Test
    fun catvScanC13ToC63IsTisSideSsotAndDoesNotIncludeVhf() {
        val catv = JapanIsdbScanPlan.isdbtCatvC13ToC63()
        assertEquals(51, catv.size)
        assertEquals("C13", catv.first().displayChannel)
        assertEquals(111_142_857L, catv.first().frequencyHz.value)
        assertEquals("C22", catv[9].displayChannel)
        assertEquals(167_142_857L, catv[9].frequencyHz.value)
        assertEquals("C23", catv[10].displayChannel)
        assertEquals(225_142_857L, catv[10].frequencyHz.value)
        assertEquals("C63", catv.last().displayChannel)
        assertEquals(465_142_857L, catv.last().frequencyHz.value)
        assertTrue(catv.all { it.kind == ScanCandidateKind.ISDB_T_CATV })
        assertTrue(catv.all { it.displayChannel.startsWith("C") })
        assertFalse(catv.any { it.displayChannel in (1..12).map(Int::toString) })
    }

    @Test
    fun defaultScanIncludesCatvAndUsesRfDiscoverySeedsForBs() {
        val scan = JapanIsdbScanPlan.defaultInitialScan()
        assertTrue(scan.any { it.kind == ScanCandidateKind.ISDB_T_CATV && it.displayChannel == "C13" })
        val bs = scan.filter { it.kind == ScanCandidateKind.ISDB_S_BS }
        assertTrue(bs.isNotEmpty())
        assertTrue(bs.all { it.streamSelector.type == StreamSelectorType.NONE })
        assertTrue(bs.all { it.backendHint == JapanIsdbScanPlan.BS_DISCOVERY_BACKEND_HINT })
    }

    @Test
    fun bsDynamicDiscoveryUsesOnlyReportedStreamIds() {
        val seed = JapanIsdbScanPlan.isdbsBsBands().first()
        val discovered = JapanIsdbScanPlan.explicitBsCandidatesFromScan(
            seed,
            listOf(18288, 18801, 18803, 18803, -1, 0xffff),
        )
        assertEquals(setOf(18288, 18801, 18803), discovered.mapNotNull { it.streamSelector.value }.toSet())
        assertTrue(discovered.all { it.streamSelector.type == StreamSelectorType.TSID })
    }

    @Test
    fun bsDynamicDiscoveryWithNoReportedStreamIdsIsEmpty() {
        val seed = JapanIsdbScanPlan.isdbsBsBands().first()
        assertTrue(JapanIsdbScanPlan.explicitBsCandidatesFromScan(seed, emptyList()).isEmpty())
    }

    @Test
    fun cs110ScanBandsDoNotCarryFrontendStreamSelector() {
        val cs = JapanIsdbScanPlan.isdbs110CsBands()
        assertTrue(cs.isNotEmpty())
        assertTrue(cs.all { it.satelliteBand == "110CS" })
        assertTrue(cs.all { it.streamSelector.type == StreamSelectorType.NONE })
    }

    @Test
    fun cs110ServiceIdentityCandidateStillDoesNotCarryFrontendSelector() {
        val candidate = JapanIsdbScanPlan.isdbs110CsServiceIdentityCandidate(
            frequencyHz = FrequencyHz(1_613_000_000L),
            tsid = TransportStreamId16(0x6020),
            label = "CS-test",
            physical = 13,
        )
        assertEquals(StreamSelectorType.NONE, candidate.streamSelector.type)
        assertEquals("110CS", candidate.satelliteBand)
    }
}

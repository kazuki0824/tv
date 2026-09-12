@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import com.maleicacid.tvinput.common.FrequencyHz
import com.maleicacid.tvinput.common.StreamSelectorType
import com.maleicacid.tvinput.common.TransportStreamId16
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

@Suppress("TooManyFunctions")
class ScanPlanPolicyTest {
    @Test
    fun bsLockContinuesTheSameScanExactlyOnceAndWaitsForStopped() {
        val operation = TunerController.StreamIdDiscoveryOperation(25L)
        var continuationCalls = 0
        operation.reportIds(intArrayOf(16400))
        repeat(2) {
            operation.continueAfterLock {
                continuationCalls++
                android.media.tv.tuner.Tuner.RESULT_SUCCESS
            }
        }
        assertEquals(1, continuationCalls)
        assertFalse(operation.await(1))
        operation.complete()
        assertTrue(operation.await(1))
        assertEquals(setOf(16400), operation.result(true).streamIds)
    }

    @Test
    fun bsContinuationFailureIsTerminalAndKeepsTheFailureCode() {
        val operation = TunerController.StreamIdDiscoveryOperation(26L)
        operation.continueAfterLock { android.media.tv.tuner.Tuner.RESULT_UNAVAILABLE }
        assertTrue(operation.await(1))
        val result = operation.result(true)
        assertFalse(result.success)
        assertEquals(android.media.tv.tuner.Tuner.RESULT_UNAVAILABLE, result.resultCode)
    }

    @Test
    fun bsStartFailureSurvivesCleanupFailureAndRetainsOwnerForRetry() {
        for (throws in listOf(false, true)) {
            val operation = TunerController.StreamIdDiscoveryOperation(24L)
            var owner: TunerController.StreamIdDiscoveryOperation? = operation
            var cancelCalls = 0
            operation.start {
                if (throws) {
                    error("scan start exception")
                }
                android.media.tv.tuner.Tuner.RESULT_UNAVAILABLE
            }
            var diagnostics = 0
            val result =
                operation.resultWithCleanup(
                    operation.await(1),
                    cleanup = {
                        if (owner === operation) {
                            operation.cancel {
                                cancelCalls++
                                android.media.tv.tuner.Tuner.RESULT_INVALID_STATE
                            }
                            owner = null
                        }
                    },
                    diagnose = { diagnostics++ },
                )
            assertEquals(operation, owner)
            assertEquals(1, cancelCalls)
            assertEquals(1, diagnostics)
            assertFalse(result.success)
            assertEquals(
                if (throws) {
                    android.media.tv.tuner.Tuner.RESULT_UNKNOWN_ERROR
                } else {
                    android.media.tv.tuner.Tuner.RESULT_UNAVAILABLE
                },
                result.resultCode,
            )
            assertEquals(
                if (throws) {
                    "scan start exception"
                } else {
                    "Tuner.scanに失敗しました result=${result.resultCode}"
                },
                result.message,
            )
            assertEquals(result, operation.result(true))
            operation.cancel { android.media.tv.tuner.Tuner.RESULT_SUCCESS }
            owner = null
            assertEquals(null, owner)
            assertEquals(result, operation.result(true))
        }
    }

    @Test
    fun bsProgressIsNotTerminalAndEveryTerminalRejectsLateIds() {
        val operation = TunerController.StreamIdDiscoveryOperation(21L)
        operation.reportProgress(100)
        check(!operation.await(1) && operation.active)
        operation.reportIds(intArrayOf(16400))
        operation.complete()
        check(operation.await(1))
        val stopped = operation.result(true)
        operation.reportIds(intArrayOf(16401))
        operation.cancel { android.media.tv.tuner.Tuner.RESULT_SUCCESS }
        operation.startFailed(1, "late failure")
        operation.complete()
        check(operation.result(true) == stopped && stopped.streamIds == setOf(16400))
        for (end in listOf("timeout", "failure", "cancel", "lost")) {
            val other = TunerController.StreamIdDiscoveryOperation(22L)
            when (end) {
                "timeout" -> other.result(false)
                "failure" -> other.startFailed(1, "scan failed")
                "cancel" -> other.cancel { android.media.tv.tuner.Tuner.RESULT_SUCCESS }
                "lost" -> other.loseResources()
            }
            val before = other.result(true)
            other.reportIds(intArrayOf(16400))
            other.complete()
            other.cancel { android.media.tv.tuner.Tuner.RESULT_SUCCESS }
            check(other.result(true) == before && !before.success && before.streamIds.isEmpty())
        }
    }

    @Test
    fun bsFailedCancelRetainsResourceLossAdmissionAndTerminalOutcome() {
        for (prior in listOf("scanning", "stopped", "timeout")) {
            val operation = TunerController.StreamIdDiscoveryOperation(23L)
            val fence = ChannelScanController.ResourceLossFence().apply { activate(23L) }
            if (prior == "stopped") {
                operation.reportIds(intArrayOf(16400))
                operation.complete()
            }
            if (prior == "timeout") {
                operation.result(false)
            }
            check(
                runCatching {
                    operation.cancel { android.media.tv.tuner.Tuner.RESULT_UNAVAILABLE }
                }.isFailure,
            )
            check(operation.acceptsResourceLoss)
            var notifications = 0

            fun lose() =
                TunerController.completeResourceLoss(
                    invalidate = {
                        if (!operation.acceptsResourceLoss) {
                            false
                        } else {
                            operation.loseResources()
                            true
                        }
                    },
                    cleanup = {
                        operation.cancel { android.media.tv.tuner.Tuner.RESULT_UNAVAILABLE }
                    },
                    notifyLost = {
                        notifications++
                        fence.onLost(operation.generation)
                    },
                )

            check(runCatching { lose() }.isFailure)
            lose()
            check(notifications == 1 && fence.terminalObserved && operation.await(1))
            val result = operation.result(true)
            check(result.resourceLost == (prior == "scanning"))
            operation.reportIds(intArrayOf(16401))
            operation.cancel { android.media.tv.tuner.Tuner.RESULT_SUCCESS }
            check(result == operation.result(true))
            check(fence.publishIfCurrent<Unit>(23L) { error("lost owner published") } == null)
        }
    }

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
            val result =
                caller.submit<TunerController.StreamIdDiscoveryResult> {
                    waiting.countDown()
                    val completed = operation.await(5000)
                    controller
                        .submit<TunerController.StreamIdDiscoveryResult> {
                            operation.result(completed)
                        }.get()
                }
            check(waiting.await(1, java.util.concurrent.TimeUnit.SECONDS))
            controller
                .submit {
                    TunerController.completeResourceLoss(
                        invalidate = {
                            if (!operation.acceptsResourceLoss) {
                                false
                            } else {
                                operation.loseResources()
                                true
                            }
                        },
                        cleanup = {
                            operation.cancel { android.media.tv.tuner.Tuner.RESULT_SUCCESS }
                        },
                        notifyLost = {
                            notifications++
                            fence.onLost(operation.generation)
                        },
                    )
                    operation.reportIds(intArrayOf(16400)) // 喪失後の遅延報告を拒否する。
                }.get(1, java.util.concurrent.TimeUnit.SECONDS)
            val lost = result.get(1, java.util.concurrent.TimeUnit.SECONDS)
            check(lost.resourceLost && !lost.success && lost.message == "TUNER_RESOURCE_LOST")
            fence.activate(requireNotNull(lost.generation))
            for (candidate in lost.candidatesFor(JapanIsdbScanPlan.isdbsBsBands().first())) {
                tuned++
                collected++
            }
            fence.publishIfCurrent<Unit>(operation.generation) { published++ }
            check(
                notifications == 1 &&
                    fence.terminalObserved &&
                    tuned == 0 &&
                    collected == 0 &&
                    published == 0,
            )
            val next = TunerController.StreamIdDiscoveryOperation(10L)
            next.reportIds(intArrayOf(16400))
            next.complete()
            check(next.result(next.await(1)).success)
        } finally {
            caller.shutdownNow()
            controller.shutdownNow()
        }
    }

    @Test
    fun scanReleaseRetryPreservesResourceLostTerminalAndRetainsOnlyFailedOwner() {
        val context = android.content.ContextWrapper(null)
        val task = ChannelScanManager.ActiveScanTask(1, ScanPurpose.SETUP_SCAN, context)
        val owner = java.util.concurrent.atomic.AtomicReference<ChannelScanManager.ActiveScanTask?>(task)
        val terminal = ScanState.Failed("TUNER_RESOURCE_LOST", 1, ScanPurpose.SETUP_SCAN)
        // Managerの実stateを検査する。テスト用の本番mutation APIは追加しない。
        val stateField =
            ChannelScanManager::class.java.getDeclaredField("state").apply {
                isAccessible = true
            }
        val previousState = ChannelScanManager.currentState()
        stateField.set(null, terminal)
        var diagnostics = 0
        var controllerCloses = 0
        var engineCloses = 0
        var reject = true
        task.controller =
            AutoCloseable {
                controllerCloses++
                if (reject) {
                    error("close failed")
                }
            }
        task.engine = AutoCloseable { engineCloses++ }
        try {
            check(!ChannelScanManager.finishScanRelease(task, owner) { diagnostics++ })
            check(owner.get() === task && task.closing && task.controller != null && task.engine == null)
            check(ChannelScanManager.currentState() === terminal && diagnostics == 1 && engineCloses == 1)
            reject = false
            check(ChannelScanManager.finishScanRelease(task, owner) { diagnostics++ })
            check(owner.get() == null && controllerCloses == 2 && engineCloses == 1)
            check(ChannelScanManager.currentState() === terminal && diagnostics == 1)
        } finally {
            stateField.set(null, previousState)
        }
    }

    @Test
    fun unavailableDynamicDiscoveryDoesNotInferUnsupportedCapability() {
        val seed = JapanIsdbScanPlan.isdbsBsBands().first()
        val failed =
            TunerController.StreamIdDiscoveryResult(
                false,
                setOf(16400),
                android.media.tv.tuner.Tuner.RESULT_UNAVAILABLE,
            )
        assertTrue(failed.candidatesFor(seed).isEmpty())
        assertEquals(
            setOf(16400),
            failed
                .copy(
                    success = true,
                    resultCode = android.media.tv.tuner.Tuner.RESULT_SUCCESS,
                ).candidatesFor(seed)
                .mapNotNull { it.streamSelector.value }
                .toSet(),
        )
        val staticFrontend = BsFrontendCapability(21, isIsdbs = true, supportsStreamIdList = false)
        val dynamicFrontend = BsFrontendCapability(12, isIsdbs = true, supportsStreamIdList = true)
        val nonIsdbs = BsFrontendCapability(10, isIsdbs = false, supportsStreamIdList = true)
        assertEquals(
            listOf(dynamicFrontend, staticFrontend),
            BsFrontendSelectionPolicy.orderedCandidates(
                listOf(staticFrontend, nonIsdbs, dynamicFrontend),
            ),
        )
        assertEquals(
            BsCandidateSource.DYNAMIC_STREAM_ID_LIST,
            BsFrontendSelectionPolicy.sourceFor(dynamicFrontend),
        )
        assertEquals(
            BsCandidateSource.STATIC_TSID_TABLE,
            BsFrontendSelectionPolicy.sourceFor(staticFrontend),
        )
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
        assertTrue(
            scan.any {
                it.kind == ScanCandidateKind.ISDB_T_CATV && it.displayChannel == "C13"
            },
        )
        val bs = scan.filter { it.kind == ScanCandidateKind.ISDB_S_BS }
        assertTrue(bs.isNotEmpty())
        assertTrue(bs.all { it.streamSelector.type == StreamSelectorType.NONE })
        assertTrue(bs.all { it.backendHint == JapanIsdbScanPlan.BS_DISCOVERY_BACKEND_HINT })
        assertEquals((1..23 step 2).toList(), bs.mapNotNull { it.physicalChannel })
        val staticCandidates = bs.flatMap(JapanIsdbScanPlan::staticBsCandidatesFor)
        assertTrue(staticCandidates.isNotEmpty())
        assertTrue(staticCandidates.all { it.streamSelector.type == StreamSelectorType.TSID })
        assertTrue(staticCandidates.all { it.streamSelector.value in 12..0xfffe })
        assertEquals(
            setOf(16400, 16401, 16402),
            JapanIsdbScanPlan.staticBsStreamIdsFor(bs.first()),
        )
        assertTrue(
            bs.filter { it.physicalChannel in setOf(7, 11, 17) }
                .all { JapanIsdbScanPlan.staticBsStreamIdsFor(it).isEmpty() },
        )
    }

    @Test
    fun terminalBsDiscoveryDoesNotRestartOnLateLock() {
        for (end in listOf("stopped", "timeout", "failure", "cancel", "lost")) {
            val operation = TunerController.StreamIdDiscoveryOperation(27L)
            when (end) {
                "stopped" -> operation.complete()
                "timeout" -> operation.result(false)
                "failure" -> operation.startFailed(1, "scan failed")
                "cancel" -> operation.cancel { android.media.tv.tuner.Tuner.RESULT_SUCCESS }
                "lost" -> operation.loseResources()
            }
            val before = operation.result(true)
            var calls = 0
            operation.continueAfterLock {
                calls++
                android.media.tv.tuner.Tuner.RESULT_SUCCESS
            }
            assertEquals(0, calls)
            assertEquals(before, operation.result(true))
        }
    }

    @Test
    fun bsDynamicDiscoveryUsesOnlyReportedStreamIds() {
        val seed = JapanIsdbScanPlan.isdbsBsBands().first()
        val discovered =
            JapanIsdbScanPlan.explicitBsCandidatesFromScan(
                seed,
                listOf(18288, 18801, 18803, 18803, -1, 0xffff),
            )
        assertEquals(
            setOf(18288, 18801, 18803),
            discovered.mapNotNull { it.streamSelector.value }.toSet(),
        )
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
        val candidate =
            JapanIsdbScanPlan.isdbs110CsServiceIdentityCandidate(
                frequencyHz = FrequencyHz(1_613_000_000L),
                tsid = TransportStreamId16(0x6020),
                label = "CS-test",
                physical = 13,
            )
        assertEquals(StreamSelectorType.NONE, candidate.streamSelector.type)
        assertEquals("110CS", candidate.satelliteBand)
    }
}

// テストの入力・期待値を本体の定数と独立した具体値で記述する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import com.maleicacid.tvinput.aribsi.CaMetadata
import com.maleicacid.tvinput.aribsi.CaMetadataSource
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.TsPid
import com.maleicacid.tvinput.common.TunerKeyToken
import org.junit.Test

// 一つの契約の試験集合・時系列を保持し、検証シナリオを分断しない。

/** AndroidJUnitRunner から実行する CasController 状態遷移テスト。 */
@Suppress("TooManyFunctions")
class CasControllerStateTest {
    private val serviceKey = ServiceKey(originalNetworkId = 4, transportStreamId = 16625, serviceId = 101)

    @Test fun ecmFailureSurvivesMetadataRefreshUntilSuccessfulEcm() {
        for (failure in listOf(
            Result.failure<EcmProcessResult>(IllegalStateException("card unavailable")),
            Result.success<EcmProcessResult>(EcmProcessResult.InvalidKeyToken("invalid")),
            Result.success<EcmProcessResult>(EcmProcessResult.DiagnosticOnly("no key")),
        )) {
            val success = Result.success<EcmProcessResult>(EcmProcessResult.RealKeyToken(TunerKeyToken(byteArrayOf(1, 2, 3))))
            var nextResult = success
            val session =
                object : CasController.MediaCasSessionBridge {
                    override fun setPrivateData(privateData: ByteArray) = Result.success(Unit)

                    override fun processEcm(section: ByteArray) = nextResult

                    override fun close() = Unit
                }
            val factory =
                object : CasController.MediaCasBridgeFactory {
                    override fun create(caSystemId: Int) =
                        Result.success(
                            object : CasController.MediaCasBridge {
                                override fun setPrivateData(privateData: ByteArray) = Result.success(Unit)

                                override fun openSession() = Result.success(session)

                                override fun processEmm(section: ByteArray) = Result.success(Unit)

                                override fun close() = Unit
                            },
                        )
                }
            val controller = CasController(mediaCasFactory = factory)
            try {
                val metadata = b25Metadata(TsPid(0x101), TsPid(0x123), TsPid(0x010))
                val descrambler = FakeTunerDescramblerBridge()
                controller.updateFromCaMetadata(metadata) { descrambler }
                controller.onEcmSection(TsPid(0x123), byteArrayOf(1))
                check(controller.currentReadiness() == CasController.Readiness.READY)
                nextResult = failure
                check(controller.onEcmSection(TsPid(0x123), byteArrayOf(1)).isNotEmpty())
                check(controller.currentReadiness() == CasController.Readiness.WAITING_FOR_KEY)
                check(controller.updateFromCaMetadata(metadata) { descrambler }.readiness == CasController.Readiness.WAITING_FOR_KEY)
                nextResult = success
                check(controller.onEcmSection(TsPid(0x123), byteArrayOf(1)).isEmpty())
                check(controller.currentReadiness() == CasController.Readiness.READY)
                check(descrambler.keyTokens.size == 1)
            } finally {
                controller.close()
            }
        }
    }

    @Test fun resourceLossClosesOwnedDescramblerAndRetainsFailuresForRetry() {
        for (closeFails in listOf(false, true)) {
            val old = RecordingDescrambler().apply { failClose = closeFails }
            val next = RecordingDescrambler()
            val controller = CasController(mediaCasFactory = FakeMediaCasBridgeFactory())
            val metadata = b25Metadata(TsPid(0x101), TsPid(0x123), TsPid(0x010))
            var accepted = true
            var notifications = 0
            var creates = 0
            val fence = ChannelScanController.ResourceLossFence().apply { activate(1L) }
            controller.updateFromCaMetadata(metadata) {
                creates++
                old
            }

            fun lose() =
                TunerController.completeResourceLoss(
                    invalidate = {
                        if (!accepted) {
                            false
                        } else {
                            accepted = false
                            true
                        }
                    },
                    cleanup = { controller.clearForResourceLoss() },
                    notifyLost = {
                        notifications++
                        fence.onLost(1L)
                    },
                )
            val failure = runCatching { lose() }.exceptionOrNull()
            check((failure != null) == closeFails)
            check(old.closes == 1 && notifications == 1 && creates == 1)
            lose()
            check(old.closes == 1 && notifications == 1)
            check(fence.publishIfCurrent(1L) { error("lost generation published") } == null)
            check(controller.onEcmSection(TsPid(0x123), byteArrayOf(0x80.toByte())).isEmpty())
            check(old.tokens == 0)
            if (closeFails) {
                check(
                    runCatching {
                        controller.updateFromCaMetadata(metadata) {
                            creates++
                            next
                        }
                    }.isFailure,
                )
                check(creates == 1 && old.closes == 2 && next.closes == 0)
                old.failClose = false
            }
            controller.updateFromCaMetadata(metadata) {
                creates++
                next
            }
            controller.onEcmSection(TsPid(0x123), byteArrayOf(0x80.toByte()))
            check(creates == 2 && next.tokens == 1 && old.tokens == 0)
            next.failClose = true
            check(runCatching { controller.close() }.isFailure)
            next.failClose = false
            controller.close() // close失敗でもexecutorと所有を保持し、再試行できる。
            check(next.closes == 2)
        }
    }

    @Test fun resourceLossRetriesOnlyUnreleasedCasArtifacts() {
        var rejectSessionClose = true
        var sessionCloses = 0
        var pluginCloses = 0
        val sessionFailure = IllegalStateException("session close failed")
        val factory =
            object : CasController.MediaCasBridgeFactory {
                override fun create(caSystemId: Int) =
                    Result.success(
                        object : CasController.MediaCasBridge {
                            override fun setPrivateData(privateData: ByteArray) = Result.success(Unit)

                            override fun processEmm(section: ByteArray) = Result.success(Unit)

                            override fun close() {
                                pluginCloses++
                            }

                            override fun openSession() =
                                Result.success(
                                    object : CasController.MediaCasSessionBridge {
                                        override fun setPrivateData(privateData: ByteArray) = Result.success(Unit)

                                        override fun processEcm(section: ByteArray) =
                                            Result.success<EcmProcessResult>(EcmProcessResult.DiagnosticOnly("test"))

                                        override fun close() {
                                            sessionCloses++
                                            if (rejectSessionClose) throw sessionFailure
                                        }
                                    },
                                )
                        },
                    )
            }
        val bridge = RecordingDescrambler().apply { failClose = true }
        val controller = CasController(mediaCasFactory = factory)
        controller.updateFromCaMetadata(b25Metadata(TsPid(0x101), TsPid(0x123), TsPid(0x010))) { bridge }
        val failure = runCatching { controller.clearForResourceLoss() }.exceptionOrNull()
        check(failure?.cause?.suppressed?.contains(sessionFailure) == true)
        check(sessionCloses == 1 && pluginCloses == 0 && bridge.closes == 1)
        check(controller.onEcmSection(TsPid(0x123), byteArrayOf(1)).isEmpty())
        rejectSessionClose = false
        bridge.failClose = false
        controller.clearForResourceLoss()
        check(sessionCloses == 2 && pluginCloses == 1 && bridge.closes == 2)
        controller.close()
        check(sessionCloses == 2 && pluginCloses == 1 && bridge.closes == 2)
    }

    // 一つの契約の試験集合・時系列を保持し、検証シナリオを分断しない。
    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("LongMethod", "MaxLineLength")
    @Test
    fun casAttachAndResourceLossShareOneControllerTransaction() {
        val executor =
            java.util.concurrent.Executors
                .newSingleThreadExecutor()
        val factoryEntered = java.util.concurrent.CountDownLatch(1)
        val permitAttach = java.util.concurrent.CountDownLatch(1)
        val controller = CasController(mediaCasFactory = FakeMediaCasBridgeFactory())
        val old = RecordingDescrambler()
        val next = RecordingDescrambler()
        val metadata = b25Metadata(TsPid(0x101), TsPid(0x123), TsPid(0x010))
        var generation = 1L
        var accepted = true
        var creates = 0
        var notifications = 0
        try {
            val attaching =
                executor.submit<CasController.UpdateResult?> {
                    TunerController.updateCasIfCurrent(1L, generation, accepted) {
                        controller.updateFromCaMetadata(metadata) {
                            creates++
                            factoryEntered.countDown()
                            check(permitAttach.await(5, java.util.concurrent.TimeUnit.SECONDS))
                            old
                        }
                    }
                }
            check(factoryEntered.await(5, java.util.concurrent.TimeUnit.SECONDS))
            // factoryでbridge取得後にlostを要求しても、attach完了までcontrollerを明け渡さない。
            val lost =
                executor.submit {
                    TunerController.completeResourceLoss(
                        invalidate = {
                            accepted = false
                            true
                        },
                        cleanup = { controller.clearForResourceLoss() },
                        notifyLost = { notifications++ },
                    )
                }
            permitAttach.countDown()
            check(attaching.get(5, java.util.concurrent.TimeUnit.SECONDS) != null)
            lost.get(5, java.util.concurrent.TimeUnit.SECONDS)
            check(old.closes == 1 && notifications == 1)
            val stale =
                executor
                    .submit<CasController.UpdateResult?> {
                        TunerController.updateCasIfCurrent(1L, generation, accepted) {
                            controller.updateFromCaMetadata(metadata) {
                                creates++
                                old
                            }
                        }
                    }.get(5, java.util.concurrent.TimeUnit.SECONDS)
            check(stale == null && creates == 1)
            executor
                .submit {
                    generation = 2L
                    accepted = true
                    check(TunerController.updateCasIfCurrent(1L, generation, accepted) { error("old metadata reattached") } == null)
                    check(
                        TunerController.updateCasIfCurrent(2L, generation, accepted) {
                            controller.updateFromCaMetadata(metadata) {
                                creates++
                                next
                            }
                        } != null,
                    )
                }.get(5, java.util.concurrent.TimeUnit.SECONDS)
            controller.onEcmSection(TsPid(0x123), byteArrayOf(0x80.toByte()))
            check(creates == 2 && next.tokens == 1 && old.tokens == 0)
        } finally {
            permitAttach.countDown()
            executor.shutdownNow()
            controller.close()
        }
    }

    @Test fun closingUnusedDirectDescramblerDoesNotReopenIt() {
        val bridge = DirectTunerDescramblerBridge(null)
        bridge.close()
        bridge.close()
        check(
            bridge
                .setKeyToken(TunerKeyToken(byteArrayOf(1)))
                .exceptionOrNull()
                ?.message
                ?.contains("退役済み") == true,
        )
        check(bridge.addPid(TsPid(0x101)).isFailure)
        check(bridge.removePid(TsPid(0x101)).isFailure)
    }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    @Test
    fun sharedElementaryPidSurvivesOneSystemRetirementAndRemovalFailure() {
        val pid = TsPid(0x101)
        val survivor = FakeTunerDescramblerBridge()
        val removed = mutableListOf<TsPid>()
        var reject = true
        var retiredCloses = 0
        val retiring =
            object : CasController.TunerDescramblerBridge {
                override fun setKeyToken(keyToken: TunerKeyToken) = Result.success(Unit)

                override fun addPid(elementaryPid: TsPid) = Result.success(Unit)

                override fun removePid(elementaryPid: TsPid): Result<Unit> {
                    removed += elementaryPid
                    return if (reject) Result.failure(IllegalStateException("removePid non-SUCCESS")) else Result.success(Unit)
                }

                override fun close() {
                    retiredCloses++
                    check(!reject) { "close failed" }
                }
            }
        val controller = CasController(mediaCasFactory = FakeMediaCasBridgeFactory())
        try {
            val b25 = b25Metadata(pid, TsPid(0x123), TsPid(0x010))
            val b1 = b25.filter { it.source != CaMetadataSource.CAT }.map { it.copy(caSystemId = 1, ecmPid = TsPid(0x124)) }
            var creates = 0
            controller.updateFromCaMetadata(b25 + b1) { if (creates++ == 0) survivor else retiring }
            controller.onEcmSection(TsPid(0x123), byteArrayOf(1))
            controller.onEcmSection(TsPid(0x124), byteArrayOf(1))
            check(creates == 2 && survivor.addedPids == setOf(pid.value))
            check(runCatching { controller.updateFromCaMetadata(b25) }.isFailure)
            check(removed == listOf(pid) && retiredCloses == 1)
            check(survivor.removedPids.isEmpty() && !survivor.closed)
            check(controller.onEcmSection(TsPid(0x123), byteArrayOf(1)).isEmpty())
            reject = false
            val retried = controller.updateFromCaMetadata(b25)
            check(retried.diagnostics.isEmpty() && retried.readiness == CasController.Readiness.READY)
            check(removed == listOf(pid, pid) && retiredCloses == 2)
            check(survivor.removedPids.isEmpty() && !survivor.closed)
            controller.updateFromCaMetadata(b25)
            check(removed == listOf(pid, pid) && retiredCloses == 2)
        } finally {
            reject = false
            controller.close()
        }
        check(survivor.removedPids == setOf(pid.value) && survivor.closed)
    }

    @Test fun clearServiceRetriesOnlyPidsWhoseRemovalFailed() {
        val removed = mutableListOf<TsPid>()
        val p1 = TsPid(0x101)
        val p2 = TsPid(0x102)
        var reject = true
        val bridge =
            object : CasController.TunerDescramblerBridge {
                override fun setKeyToken(keyToken: TunerKeyToken) = Result.success(Unit)

                override fun addPid(elementaryPid: TsPid) = Result.success(Unit)

                override fun removePid(elementaryPid: TsPid): Result<Unit> {
                    removed += elementaryPid
                    return if (reject &&
                        elementaryPid == p1
                    ) {
                        Result.failure(IllegalStateException("remove failed"))
                    } else {
                        Result.success(Unit)
                    }
                }

                override fun close() {
                    check(!reject) { "close failed" }
                }
            }
        CasController(mediaCasFactory = FakeMediaCasBridgeFactory()).use { controller ->
            controller.updateFromCaMetadata(
                b25Metadata(p1, TsPid(0x123), TsPid(0x010)) +
                    b25Metadata(p2, TsPid(0x123), TsPid(0x010)),
            ) { bridge }
            controller.onEcmSection(TsPid(0x123), byteArrayOf(1))
            check(runCatching { controller.clearForClearService() }.isFailure)
            check(removed == listOf(p1, p2))
            check(controller.lastDiagnostic().errorCode == CasController.ErrorCode.DESCRAMBLER_FAILED)
            check(controller.onEcmSection(TsPid(0x123), byteArrayOf(1)).isEmpty())
            reject = false
            controller.clearForClearService()
            check(removed == listOf(p1, p2, p1))
        }
    }

    @Test fun metadataFailureRejectsSurvivorAndEveryObsoleteSystemBeforeFilterCommit() {
        var rejectClose = true
        val pluginCloses = mutableListOf<Int>()
        val deliveredPrivateData = mutableListOf<Int>()
        val factory =
            object : CasController.MediaCasBridgeFactory {
                override fun create(caSystemId: Int) =
                    Result.success(
                        object : CasController.MediaCasBridge {
                            override fun setPrivateData(privateData: ByteArray) = Result.success(Unit)

                            override fun processEmm(section: ByteArray) = Result.success(Unit)

                            override fun close() {
                                pluginCloses += caSystemId
                                if (rejectClose && caSystemId != 5) error("obsolete plugin close failed")
                            }

                            override fun openSession() =
                                Result.success(
                                    object : CasController.MediaCasSessionBridge {
                                        var privateValue = -1

                                        override fun setPrivateData(privateData: ByteArray): Result<Unit> {
                                            privateValue = privateData.single().toInt()
                                            return Result.success(Unit)
                                        }

                                        override fun processEcm(section: ByteArray): Result<EcmProcessResult> {
                                            deliveredPrivateData += privateValue
                                            return Result.success(EcmProcessResult.DiagnosticOnly("test"))
                                        }

                                        override fun close() = Unit
                                    },
                                )
                        },
                    )
            }
        val controller = CasController(setOf(1, 5, 7), factory)

        fun binding(
            system: Int,
            value: Int,
        ) = CaMetadata(
            serviceKey,
            system,
            ecmPid = TsPid(0x123),
            emmPid = null,
            elementaryPid = TsPid(0x101),
            privateData = byteArrayOf(value.toByte()),
            source = CaMetadataSource.ELEMENTARY_STREAM,
        )
        var filters = setOf(TsPid(0x123))
        var commits = 0
        var stopped = false
        try {
            controller.updateFromCaMetadata(listOf(binding(5, 1), binding(1, 1), binding(7, 1))) { FakeTunerDescramblerBridge() }

            // 境界呼出しの失敗を漏らさず扱い、既存の診断・解放・失敗伝播へ渡す。
            @Suppress("TooGenericExceptionCaught")
            fun refresh() =
                SectionFilterPolicy.commitCasAndFilters(
                    updateCas = {
                        try {
                            controller.updateFromCaMetadata(listOf(binding(5, 2))) { FakeTunerDescramblerBridge() }
                        } catch (failure: Exception) {
                            check(controller.onEcmSection(TsPid(0x123), byteArrayOf(1)).isEmpty())
                            throw failure
                        }
                    },
                    commitFilters = {
                        filters = it.ecmPids
                        commits++
                    },
                    reject = {
                        SectionFilterPolicy.completeCleanup(
                            { controller.clearForResourceLoss() },
                            { filters = emptySet() },
                            { stopped = true },
                        )
                    },
                )
            check(runCatching { refresh() }.isFailure)
            check(commits == 0 && filters.isEmpty() && stopped)
            check(pluginCloses.containsAll(listOf(1, 7)))
            check(controller.onEcmSection(TsPid(0x123), byteArrayOf(1)).isEmpty() && deliveredPrivateData.isEmpty())
            rejectClose = false
            refresh()
            check(commits == 1 && filters == setOf(TsPid(0x123)))
            controller.onEcmSection(TsPid(0x123), byteArrayOf(1))
            check(deliveredPrivateData == listOf(2))
        } finally {
            rejectClose = false
            controller.close()
        }
    }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    @Test
    fun metadataDiagnosticAndFilterExceptionBothStopCasDelivery() {
        for (failureStage in listOf("metadata", "filter")) {
            var rejectData = failureStage == "metadata"
            var rejected = 0
            var commits = 0
            var stopped = false
            val factory =
                object : CasController.MediaCasBridgeFactory {
                    override fun create(caSystemId: Int) =
                        Result.success(
                            object : CasController.MediaCasBridge {
                                override fun setPrivateData(privateData: ByteArray) =
                                    if (rejectData) {
                                        Result.failure<Unit>(
                                            IllegalStateException("private data failed"),
                                        )
                                    } else {
                                        Result.success(Unit)
                                    }

                                override fun processEmm(section: ByteArray) = Result.success(Unit)

                                override fun openSession(): Result<CasController.MediaCasSessionBridge> =
                                    error("EMM does not open a session")

                                override fun close() = Unit
                            },
                        )
                }
            CasController(mediaCasFactory = factory).use { controller ->
                val metadata = b25Metadata(TsPid(0x101), TsPid(0x123), TsPid(0x010)).filter { it.source == CaMetadataSource.CAT }
                val result =
                    runCatching {
                        SectionFilterPolicy.commitCasAndFilters(
                            updateCas = { controller.updateFromCaMetadata(metadata) },
                            commitFilters = {
                                commits++
                                error("filter start failed")
                            },
                            reject = {
                                rejected++
                                SectionFilterPolicy.completeCleanup({ controller.clearForResourceLoss() }, { stopped = true })
                            },
                        )
                    }
                check((result.isFailure) == (failureStage == "filter"))
                check(commits == if (failureStage == "filter") 1 else 0)
                check(rejected == 1 && stopped)
                check(controller.onEmmSection(TsPid(0x010), byteArrayOf(1)).isEmpty())
                rejectData = false
                SectionFilterPolicy.commitCasAndFilters(
                    updateCas = { controller.updateFromCaMetadata(metadata) },
                    commitFilters = {},
                    reject = { error("retry rejected") },
                )
                check(controller.onEmmSection(TsPid(0x010), byteArrayOf(1)).isEmpty())
            }
        }
    }

    @Test fun emmOnlyUsesPluginAndPromotesExistingOwnerOnlyWhenEcmAppears() {
        var creates = 0
        var opens = 0
        var emms = 0
        var sessionCloses = 0
        var pluginCloses = 0
        var rejectOpen = true
        val factory =
            object : CasController.MediaCasBridgeFactory {
                override fun create(caSystemId: Int): Result<CasController.MediaCasBridge> {
                    creates++
                    return Result.success(
                        object : CasController.MediaCasBridge {
                            override fun setPrivateData(privateData: ByteArray) = Result.success(Unit)

                            override fun processEmm(section: ByteArray): Result<Unit> {
                                emms++
                                return Result.success(Unit)
                            }

                            override fun close() {
                                pluginCloses++
                            }

                            override fun openSession(): Result<CasController.MediaCasSessionBridge> {
                                opens++
                                if (rejectOpen) return Result.failure(IllegalStateException("no session resource"))
                                return Result.success(
                                    object : CasController.MediaCasSessionBridge {
                                        override fun setPrivateData(privateData: ByteArray) = Result.success(Unit)

                                        override fun processEcm(section: ByteArray) =
                                            Result.success<EcmProcessResult>(EcmProcessResult.DiagnosticOnly("test"))

                                        override fun close() {
                                            sessionCloses++
                                        }
                                    },
                                )
                            }
                        },
                    )
                }
            }
        CasController(mediaCasFactory = factory).use { controller ->
            val full = b25Metadata(TsPid(0x101), TsPid(0x123), TsPid(0x010))
            val cat = full.filter { it.source == CaMetadataSource.CAT }
            check(controller.updateFromCaMetadata(cat).diagnostics.isEmpty())
            controller.onEmmSection(TsPid(0x010), byteArrayOf(1))
            controller.updateFromCaMetadata(cat)
            check(creates == 1 && opens == 0 && emms == 1)
            rejectOpen = false
            check(controller.updateFromCaMetadata(full) { FakeTunerDescramblerBridge() }.diagnostics.isEmpty())
            check(creates == 1 && opens == 1 && sessionCloses == 0)
            controller.updateFromCaMetadata(cat)
            check(sessionCloses == 1 && pluginCloses == 0 && creates == 1)
            controller.onEmmSection(TsPid(0x010), byteArrayOf(1))
            check(emms == 2)
            check(controller.updateFromCaMetadata(full) { FakeTunerDescramblerBridge() }.diagnostics.isEmpty())
            check(opens == 2 && creates == 1 && sessionCloses == 1)
        }
        check(pluginCloses == 1 && sessionCloses == 2)
    }

    private class RecordingDescrambler : CasController.TunerDescramblerBridge {
        var closes = 0
        var tokens = 0
        var failClose = false

        override fun setKeyToken(keyToken: TunerKeyToken): Result<Unit> {
            tokens++
            return Result.success(Unit)
        }

        override fun addPid(elementaryPid: TsPid) = Result.success(Unit)

        override fun removePid(elementaryPid: TsPid) = Result.success(Unit)

        override fun close() {
            closes++
            if (failClose) error("descrambler close failed")
        }
    }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    @Test
    fun pluginSelectionAndEcmEmmDispatch() {
        val factory = FakeMediaCasBridgeFactory()
        val descrambler = FakeTunerDescramblerBridge()
        val controller = CasController(mediaCasFactory = factory)
        val update =
            controller.updateFromCaMetadata(b25Metadata(esPid = TsPid(0x101), ecmPid = TsPid(0x123), emmPid = TsPid(0x010)), {
                descrambler
            })
        check(update.diagnostics.isEmpty()) { update.diagnostics.toString() }
        check(update.ecmPids == setOf(TsPid(0x123)))
        check(update.emmPids == setOf(TsPid(0x010)))
        check(update.readiness == CasController.Readiness.WAITING_FOR_KEY)

        val ecmDiagnostics = controller.onEcmSection(TsPid(0x123), byteArrayOf(0x80.toByte()))
        check(ecmDiagnostics.isEmpty()) { ecmDiagnostics.toString() }
        check(descrambler.keyTokens.single().contentEquals(FakeMediaCasSessionBridge.KEY_TOKEN))
        check(0x101 in descrambler.addedPids)
        check(controller.currentReadiness() == CasController.Readiness.READY)

        val emmDiagnostics = controller.onEmmSection(TsPid(0x010), byteArrayOf(0x82.toByte()))
        check(emmDiagnostics.isEmpty()) { emmDiagnostics.toString() }
        check(factory.created.getValue(CasController.SupportedCasSystemIds.ARIB_STD_B25).processedEmmCount == 1)
    }

    @Test fun emptyMetadataKeepsClearPlaybackPathIdle() {
        val factory = FakeMediaCasBridgeFactory()
        val controller = CasController(mediaCasFactory = factory)
        val update = controller.updateFromCaMetadata(emptyList())
        check(update.diagnostics.isEmpty())
        check(update.ecmPids.isEmpty())
        check(update.emmPids.isEmpty())
        check(update.readiness == CasController.Readiness.CLEAR)
        check(factory.created.isEmpty())
        check(controller.lastDiagnostic().state == CasController.State.IDLE)
    }

    @Test fun b1CatDoesNotCreateCasOrEmmBinding() {
        val factory = FakeMediaCasBridgeFactory()
        CasController(mediaCasFactory = factory).use { controller ->
            val metadata =
                b25Metadata(TsPid(0x101), TsPid(0x123), TsPid(0x010))
                    .filter { it.source == CaMetadataSource.CAT }
                    .map { it.copy(caSystemId = CasController.SupportedCasSystemIds.ARIB_STD_B1) }
            val update = controller.updateFromCaMetadata(metadata)
            check(update.emmPids.isEmpty())
            check(update.diagnostics.isEmpty())
            check(controller.onEmmSection(TsPid(0x010), byteArrayOf(0x82.toByte())).isEmpty())
            check(factory.created.isEmpty())
        }
    }

    @Test fun sharedEmmPidOnlyDispatchesToB25WhileB1EcmRemainsUsable() {
        val factory = FakeMediaCasBridgeFactory()
        val descrambler = FakeTunerDescramblerBridge()
        CasController(mediaCasFactory = factory).use { controller ->
            val b1 =
                b25Metadata(TsPid(0x102), TsPid(0x124), TsPid(0x010))
                    .map { it.copy(caSystemId = CasController.SupportedCasSystemIds.ARIB_STD_B1) }
            val update =
                controller.updateFromCaMetadata(
                    b25Metadata(TsPid(0x101), TsPid(0x123), TsPid(0x010)) + b1,
                    { descrambler },
                )
            check(update.diagnostics.isEmpty())
            check(controller.onEmmSection(TsPid(0x010), byteArrayOf(0x82.toByte())).isEmpty())
            check(factory.created.getValue(CasController.SupportedCasSystemIds.ARIB_STD_B25).processedEmmCount == 1)
            check(factory.created.getValue(CasController.SupportedCasSystemIds.ARIB_STD_B1).processedEmmCount == 0)
            check(controller.onEcmSection(TsPid(0x124), byteArrayOf(0x80.toByte())).isEmpty())
            check(0x102 in descrambler.addedPids)
        }
    }

    @Test fun unsupportedSystemIdIsError() {
        val controller = CasController(mediaCasFactory = FakeMediaCasBridgeFactory())
        val result =
            controller.updateFromCaMetadata(
                listOf(
                    CaMetadata(
                        serviceKey,
                        0x7fff,
                        ecmPid = TsPid(0x123),
                        emmPid = null,
                        elementaryPid = TsPid(0x101),
                        source = CaMetadataSource.ELEMENTARY_STREAM,
                    ),
                ),
                { FakeTunerDescramblerBridge() },
            )
        check(result.diagnostics.any { it.errorCode == CasController.ErrorCode.UNSUPPORTED_SYSTEM_ID })
        check(result.ecmPids.isEmpty())
        check(result.readiness == CasController.Readiness.ERROR)
    }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    @Test
    fun pmtUpdateRemovesOldPidAndAddsNewPid() {
        val controller = CasController(mediaCasFactory = FakeMediaCasBridgeFactory())
        val descrambler = FakeTunerDescramblerBridge()
        controller.updateFromCaMetadata(b25Metadata(esPid = TsPid(0x101), ecmPid = TsPid(0x123), emmPid = TsPid(0x010)), { descrambler })
        controller.onEcmSection(TsPid(0x123), byteArrayOf(0x80.toByte()))
        controller.updateFromCaMetadata(b25Metadata(esPid = TsPid(0x102), ecmPid = TsPid(0x124), emmPid = TsPid(0x010)), { descrambler })
        controller.onEcmSection(TsPid(0x124), byteArrayOf(0x80.toByte()))
        check(0x101 in descrambler.removedPids)
        check(0x102 in descrambler.addedPids)
    }

    @Test fun failedPidAddRemainsPendingAndSameMetadataRetriesIt() {
        val controller = CasController(mediaCasFactory = FakeMediaCasBridgeFactory())
        val descrambler = RetryingTunerDescramblerBridge()
        val initial = b25Metadata(esPid = TsPid(0x101), ecmPid = TsPid(0x123), emmPid = TsPid(0x010))
        controller.updateFromCaMetadata(initial) { descrambler }
        controller.onEcmSection(TsPid(0x123), byteArrayOf(0x80.toByte()))
        check(controller.currentReadiness() == CasController.Readiness.READY)

        val expanded =
            b25MetadataForPids(
                listOf(TsPid(0x101), TsPid(0x102)),
                ecmPid = TsPid(0x123),
                emmPid = TsPid(0x010),
            )
        descrambler.failNextAdd(TsPid(0x102))
        val failed = controller.updateFromCaMetadata(expanded) { descrambler }
        check(failed.diagnostics.any { it.errorCode == CasController.ErrorCode.DESCRAMBLER_FAILED })
        check(failed.readiness != CasController.Readiness.READY)
        check(controller.currentReadiness() == CasController.Readiness.WAITING_FOR_KEY)
        check(descrambler.linkedPids == linkedSetOf(0x101))

        val retried = controller.updateFromCaMetadata(expanded) { descrambler }
        check(retried.diagnostics.isEmpty()) { retried.diagnostics.toString() }
        check(retried.readiness == CasController.Readiness.READY)
        check(descrambler.addAttempts.getValue(0x102) == 2)
        check(descrambler.linkedPids == linkedSetOf(0x101, 0x102))
    }

    @Test fun failedPidRemovalRemainsLinkedAndSameMetadataRetriesIt() {
        val controller = CasController(mediaCasFactory = FakeMediaCasBridgeFactory())
        val descrambler = RetryingTunerDescramblerBridge()
        val initial =
            b25MetadataForPids(
                listOf(TsPid(0x101), TsPid(0x102)),
                ecmPid = TsPid(0x123),
                emmPid = TsPid(0x010),
            )
        controller.updateFromCaMetadata(initial) { descrambler }
        controller.onEcmSection(TsPid(0x123), byteArrayOf(0x80.toByte()))
        check(controller.currentReadiness() == CasController.Readiness.READY)

        val reduced = b25Metadata(esPid = TsPid(0x101), ecmPid = TsPid(0x123), emmPid = TsPid(0x010))
        descrambler.failNextRemove(TsPid(0x102))
        val failed = controller.updateFromCaMetadata(reduced) { descrambler }
        check(failed.diagnostics.any { it.errorCode == CasController.ErrorCode.DESCRAMBLER_FAILED })
        check(failed.readiness != CasController.Readiness.READY)
        check(controller.currentReadiness() == CasController.Readiness.WAITING_FOR_KEY)
        check(descrambler.linkedPids == linkedSetOf(0x101, 0x102))

        val retried = controller.updateFromCaMetadata(reduced) { descrambler }
        check(retried.diagnostics.isEmpty()) { retried.diagnostics.toString() }
        check(retried.readiness == CasController.Readiness.READY)
        check(descrambler.removeAttempts.getValue(0x102) == 2)
        check(descrambler.linkedPids == linkedSetOf(0x101))
    }

    @Test fun initialPartialPidLinkRollsBackAndNeverReportsReady() {
        val controller = CasController(mediaCasFactory = FakeMediaCasBridgeFactory())
        val descrambler = RetryingTunerDescramblerBridge()
        val metadata =
            b25MetadataForPids(
                listOf(TsPid(0x101), TsPid(0x102)),
                ecmPid = TsPid(0x123),
                emmPid = TsPid(0x010),
            )
        controller.updateFromCaMetadata(metadata) { descrambler }
        descrambler.failNextAdd(TsPid(0x102))

        val failed = controller.onEcmSection(TsPid(0x123), byteArrayOf(0x80.toByte()))
        check(failed.any { it.errorCode == CasController.ErrorCode.DESCRAMBLER_FAILED })
        check(controller.currentReadiness() == CasController.Readiness.WAITING_FOR_KEY)
        check(!descrambler.keyLinked)
        check(descrambler.linkedPids.isEmpty())
        check(descrambler.removeAttempts.getValue(0x101) == 1)

        val retried = controller.onEcmSection(TsPid(0x123), byteArrayOf(0x80.toByte()))
        check(retried.isEmpty()) { retried.toString() }
        check(controller.currentReadiness() == CasController.Readiness.READY)
        check(descrambler.keyLinked)
        check(descrambler.linkedPids == linkedSetOf(0x101, 0x102))
    }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    @Test
    fun diagnosticOnlyEcmDoesNotSetKeyTokenOrAddPid() {
        val controller = CasController(mediaCasFactory = DiagnosticOnlyMediaCasBridgeFactory())
        val descrambler = FakeTunerDescramblerBridge()
        val update =
            controller.updateFromCaMetadata(b25Metadata(esPid = TsPid(0x101), ecmPid = TsPid(0x123), emmPid = TsPid(0x010)), {
                descrambler
            })
        check(update.diagnostics.isEmpty()) { update.diagnostics.toString() }

        val ecmDiagnostics = controller.onEcmSection(TsPid(0x123), byteArrayOf(0x80.toByte()))
        check(ecmDiagnostics.any { it.errorCode == CasController.ErrorCode.KEY_TOKEN_MISSING }) { ecmDiagnostics.toString() }
        check(descrambler.keyTokens.isEmpty())
        check(descrambler.addedPids.isEmpty())
        check(controller.currentReadiness() == CasController.Readiness.WAITING_FOR_KEY)
    }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    @Test
    fun pluginUnavailableDoesNotAttachDescramblerToken() {
        val controller = CasController(mediaCasFactory = UnavailableMediaCasBridgeFactory())
        val descrambler = FakeTunerDescramblerBridge()
        val update =
            controller.updateFromCaMetadata(b25Metadata(esPid = TsPid(0x101), ecmPid = TsPid(0x123), emmPid = TsPid(0x010)), {
                descrambler
            })
        check(update.diagnostics.any { it.errorCode == CasController.ErrorCode.PLUGIN_UNAVAILABLE }) { update.diagnostics.toString() }
        check(descrambler.keyTokens.isEmpty())
        check(descrambler.addedPids.isEmpty())
    }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    @Test
    fun sessionOpenFailureIsDistinctFromPluginUnavailable() {
        val factory = SessionFailureMediaCasBridgeFactory()
        val controller = CasController(mediaCasFactory = factory)
        val descrambler = FakeTunerDescramblerBridge()
        val update =
            controller.updateFromCaMetadata(b25Metadata(esPid = TsPid(0x101), ecmPid = TsPid(0x123), emmPid = TsPid(0x010)), {
                descrambler
            })

        check(update.diagnostics.any { it.errorCode == CasController.ErrorCode.SESSION_OPEN_FAILED }) { update.diagnostics.toString() }
        check(update.diagnostics.none { it.errorCode == CasController.ErrorCode.PLUGIN_UNAVAILABLE }) { update.diagnostics.toString() }
        check(factory.bridge.closed)
        check(descrambler.keyTokens.isEmpty())
        check(descrambler.addedPids.isEmpty())
    }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    @Test
    fun closeReleasesDescrambler() {
        val controller = CasController(mediaCasFactory = FakeMediaCasBridgeFactory())
        val descrambler = FakeTunerDescramblerBridge()
        controller.updateFromCaMetadata(b25Metadata(esPid = TsPid(0x101), ecmPid = TsPid(0x123), emmPid = TsPid(0x010)), { descrambler })
        controller.close()
        controller.close()
        check(descrambler.closed)
        check(controller.lastDiagnostic().state == CasController.State.CLOSED)
        val failure = runCatching { controller.updateFromCaMetadata(emptyList()) }.exceptionOrNull()
        check(failure is IllegalStateException) { "close後の新規workは拒否されるべきです: $failure" }
    }

    @Test fun closeUnlinksKeyBeforeClosingDescrambler() {
        val events = mutableListOf<String>()
        val controller = CasController(mediaCasFactory = OrderedMediaCasBridgeFactory(events))
        val descrambler = OrderedDescramblerBridge(events)
        controller.updateFromCaMetadata(
            b25Metadata(esPid = TsPid(0x101), ecmPid = TsPid(0x123), emmPid = TsPid(0x010)),
            { descrambler },
        )
        controller.onEcmSection(TsPid(0x123), byteArrayOf(0x80.toByte()))
        controller.close()
        val remove = events.indexOf("remove:257")
        val clear = events.indexOf("clear-key")
        val descramblerClose = events.indexOf("descrambler-close")
        val sessionClose = events.indexOf("session-close")
        check(remove >= 0 && remove < clear)
        check(clear < descramblerClose)
        check(descramblerClose < sessionClose)
        controller.close()
        check(controller.lastDiagnostic().state == CasController.State.CLOSED)
        check(controller.currentReadiness() == CasController.Readiness.CLOSED)
        val failure = runCatching { controller.updateFromCaMetadata(emptyList()) }.exceptionOrNull()
        check(failure is IllegalStateException) { "close後の新規workは拒否されるべきです: $failure" }
    }

    @Test fun sameSystemDifferentEcmGetsIndependentDescramblers() {
        val factory = MultiSessionMediaCasBridgeFactory()
        val root = ForkingDescramblerBridge()
        val metadata =
            listOf(
                CaMetadata(
                    serviceKey,
                    CasController.SupportedCasSystemIds.ARIB_STD_B25,
                    TsPid(0x120),
                    null,
                    TsPid(0x101),
                    byteArrayOf(0x11),
                    CaMetadataSource.ELEMENTARY_STREAM,
                ),
                CaMetadata(
                    serviceKey,
                    CasController.SupportedCasSystemIds.ARIB_STD_B25,
                    TsPid(0x121),
                    null,
                    TsPid(0x102),
                    byteArrayOf(0x22),
                    CaMetadataSource.ELEMENTARY_STREAM,
                ),
            )
        val controller = CasController(mediaCasFactory = factory)
        val update = controller.updateFromCaMetadata(metadata) { root.newSibling().getOrThrow() }
        check(update.diagnostics.isEmpty()) { update.diagnostics.toString() }
        check(root.children.size == 2)
        controller.onEcmSection(TsPid(0x120), byteArrayOf(0x80.toByte()))
        controller.onEcmSection(TsPid(0x121), byteArrayOf(0x80.toByte()))
        check(root.children[0].addedPids == linkedSetOf(0x101))
        check(root.children[1].addedPids == linkedSetOf(0x102))
        check(root.children[0].keyTokens.size == 1)
        check(root.children[1].keyTokens.size == 1)
    }

    @Test fun b1CatDoesNotProduceEmmFilterPlan() {
        val controller = CasController(mediaCasFactory = FakeMediaCasBridgeFactory())
        val descrambler = FakeTunerDescramblerBridge()
        val metadata =
            listOf(
                CaMetadata(
                    serviceKey,
                    CasController.SupportedCasSystemIds.ARIB_STD_B1,
                    TsPid(0x123),
                    null,
                    TsPid(0x101),
                    byteArrayOf(0x01),
                    CaMetadataSource.ELEMENTARY_STREAM,
                ),
                CaMetadata(
                    null,
                    CasController.SupportedCasSystemIds.ARIB_STD_B1,
                    null,
                    TsPid(0x010),
                    null,
                    byteArrayOf(0x02),
                    CaMetadataSource.CAT,
                ),
            )
        val update = controller.updateFromCaMetadata(metadata) { descrambler }
        check(update.emmPids.isEmpty())
    }

    private fun b25Metadata(
        esPid: TsPid,
        ecmPid: TsPid,
        emmPid: TsPid,
    ): List<CaMetadata> = b25MetadataForPids(listOf(esPid), ecmPid, emmPid)

    private fun b25MetadataForPids(
        esPids: List<TsPid>,
        ecmPid: TsPid,
        emmPid: TsPid,
    ): List<CaMetadata> =
        listOf(
            CaMetadata(
                serviceKey,
                CasController.SupportedCasSystemIds.ARIB_STD_B25,
                ecmPid = ecmPid,
                emmPid = null,
                elementaryPid = null,
                privateData = byteArrayOf(0x01),
                source = CaMetadataSource.PROGRAM,
            ),
        ) +
            esPids.map { esPid ->
                CaMetadata(
                    serviceKey,
                    CasController.SupportedCasSystemIds.ARIB_STD_B25,
                    ecmPid = ecmPid,
                    emmPid = null,
                    elementaryPid = esPid,
                    privateData = byteArrayOf(0x02),
                    source = CaMetadataSource.ELEMENTARY_STREAM,
                )
            } +
            listOf(
                CaMetadata(
                    null,
                    CasController.SupportedCasSystemIds.ARIB_STD_B25,
                    ecmPid = null,
                    emmPid = emmPid,
                    elementaryPid = null,
                    privateData = byteArrayOf(0x03),
                    source = CaMetadataSource.CAT,
                ),
            )

    private class DiagnosticOnlyMediaCasBridgeFactory : CasController.MediaCasBridgeFactory {
        override fun create(caSystemId: Int): Result<CasController.MediaCasBridge> = Result.success(DiagnosticOnlyMediaCasBridge())
    }

    private class DiagnosticOnlyMediaCasBridge : CasController.MediaCasBridge {
        override fun setPrivateData(privateData: ByteArray): Result<Unit> = Result.success(Unit)

        // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
        @Suppress("MaxLineLength")
        override fun openSession(): Result<CasController.MediaCasSessionBridge> = Result.success(DiagnosticOnlyMediaCasSessionBridge())

        override fun processEmm(section: ByteArray): Result<Unit> = Result.success(Unit)

        override fun close() = Unit
    }

    private class DiagnosticOnlyMediaCasSessionBridge : CasController.MediaCasSessionBridge {
        override fun setPrivateData(privateData: ByteArray): Result<Unit> = Result.success(Unit)

        override fun processEcm(section: ByteArray): Result<EcmProcessResult> =
            Result.success(EcmProcessResult.DiagnosticOnly("placeholder CAS は実 key token を返しません"))

        override fun close() = Unit
    }

    private class UnavailableMediaCasBridgeFactory : CasController.MediaCasBridgeFactory {
        override fun create(caSystemId: Int): Result<CasController.MediaCasBridge> =
            Result.failure(IllegalStateException("placeholder CAS plugin は利用できません"))
    }

    private class SessionFailureMediaCasBridgeFactory : CasController.MediaCasBridgeFactory {
        val bridge = SessionFailureMediaCasBridge()

        override fun create(caSystemId: Int): Result<CasController.MediaCasBridge> = Result.success(bridge)
    }

    private class SessionFailureMediaCasBridge : CasController.MediaCasBridge {
        var closed = false

        override fun setPrivateData(privateData: ByteArray): Result<Unit> = Result.success(Unit)

        override fun openSession(): Result<CasController.MediaCasSessionBridge> =
            Result.failure(IllegalStateException("CAS session を開始できません"))

        override fun processEmm(section: ByteArray): Result<Unit> = Result.success(Unit)

        override fun close() {
            closed = true
        }
    }

    private class FakeMediaCasBridgeFactory : CasController.MediaCasBridgeFactory {
        val created = LinkedHashMap<Int, FakeMediaCasBridge>()

        override fun create(caSystemId: Int): Result<CasController.MediaCasBridge> {
            val bridge = created.getOrPut(caSystemId) { FakeMediaCasBridge() }
            return Result.success(bridge)
        }
    }

    private class FakeMediaCasBridge : CasController.MediaCasBridge {
        var processedEmmCount = 0
        var closed = false

        override fun setPrivateData(privateData: ByteArray): Result<Unit> = Result.success(Unit)

        // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
        @Suppress("MaxLineLength")
        override fun openSession(): Result<CasController.MediaCasSessionBridge> = Result.success(FakeMediaCasSessionBridge())

        override fun processEmm(section: ByteArray): Result<Unit> {
            processedEmmCount++
            return Result.success(Unit)
        }

        override fun close() {
            closed = true
        }
    }

    private class FakeMediaCasSessionBridge : CasController.MediaCasSessionBridge {
        companion object {
            val KEY_TOKEN = byteArrayOf(0x11, 0x22, 0x33)
        }

        override fun setPrivateData(privateData: ByteArray): Result<Unit> = Result.success(Unit)

        override fun processEcm(section: ByteArray): Result<EcmProcessResult> =
            Result.success(EcmProcessResult.RealKeyToken(TunerKeyToken(KEY_TOKEN.copyOf())))

        override fun close() = Unit
    }

    private open class FakeTunerDescramblerBridge : CasController.TunerDescramblerBridge {
        val keyTokens = mutableListOf<ByteArray>()
        val addedPids = linkedSetOf<Int>()
        val removedPids = linkedSetOf<Int>()
        var clearKeyCount = 0
        var closed = false

        override fun setKeyToken(keyToken: TunerKeyToken): Result<Unit> {
            keyTokens += keyToken.toByteArray()
            return Result.success(Unit)
        }

        override fun clearKeyToken(): Result<Unit> {
            clearKeyCount++
            return Result.success(Unit)
        }

        override fun addPid(elementaryPid: TsPid): Result<Unit> {
            addedPids += elementaryPid.value
            return Result.success(Unit)
        }

        override fun removePid(elementaryPid: TsPid): Result<Unit> {
            removedPids += elementaryPid.value
            return Result.success(Unit)
        }

        override fun newSibling(): Result<CasController.TunerDescramblerBridge> = Result.success(this)

        override fun close() {
            closed = true
        }
    }

    private class RetryingTunerDescramblerBridge : CasController.TunerDescramblerBridge {
        val linkedPids = linkedSetOf<Int>()
        val addAttempts = linkedMapOf<Int, Int>()
        val removeAttempts = linkedMapOf<Int, Int>()
        var keyLinked = false
        private val addFailuresRemaining = linkedMapOf<Int, Int>()
        private val removeFailuresRemaining = linkedMapOf<Int, Int>()

        fun failNextAdd(pid: TsPid) {
            addFailuresRemaining[pid.value] = addFailuresRemaining.getOrDefault(pid.value, 0) + 1
        }

        fun failNextRemove(pid: TsPid) {
            removeFailuresRemaining[pid.value] = removeFailuresRemaining.getOrDefault(pid.value, 0) + 1
        }

        override fun setKeyToken(keyToken: TunerKeyToken): Result<Unit> {
            keyLinked = true
            return Result.success(Unit)
        }

        override fun clearKeyToken(): Result<Unit> {
            keyLinked = false
            return Result.success(Unit)
        }

        override fun addPid(elementaryPid: TsPid): Result<Unit> {
            val pid = elementaryPid.value
            addAttempts[pid] = addAttempts.getOrDefault(pid, 0) + 1
            val failures = addFailuresRemaining.getOrDefault(pid, 0)
            if (failures > 0) {
                addFailuresRemaining[pid] = failures - 1
                return Result.failure(IllegalStateException("injected addPid failure pid=$pid"))
            }
            linkedPids += pid
            return Result.success(Unit)
        }

        override fun removePid(elementaryPid: TsPid): Result<Unit> {
            val pid = elementaryPid.value
            removeAttempts[pid] = removeAttempts.getOrDefault(pid, 0) + 1
            val failures = removeFailuresRemaining.getOrDefault(pid, 0)
            if (failures > 0) {
                removeFailuresRemaining[pid] = failures - 1
                return Result.failure(IllegalStateException("injected removePid failure pid=$pid"))
            }
            linkedPids -= pid
            return Result.success(Unit)
        }

        override fun newSibling(): Result<CasController.TunerDescramblerBridge> = Result.success(this)

        override fun close() = Unit
    }

    private class ForkingDescramblerBridge : CasController.TunerDescramblerBridge {
        val children = mutableListOf<FakeTunerDescramblerBridge>()

        override fun setKeyToken(keyToken: TunerKeyToken) = Result.failure<Unit>(IllegalStateException("prototype only"))

        override fun addPid(elementaryPid: TsPid) = Result.failure<Unit>(IllegalStateException("prototype only"))

        override fun removePid(elementaryPid: TsPid) = Result.failure<Unit>(IllegalStateException("prototype only"))

        override fun newSibling(): Result<CasController.TunerDescramblerBridge> {
            val child = FakeTunerDescramblerBridge()
            children += child
            return Result.success(child)
        }

        override fun close() = Unit
    }

    private class MultiSessionMediaCasBridgeFactory : CasController.MediaCasBridgeFactory {
        override fun create(caSystemId: Int): Result<CasController.MediaCasBridge> = Result.success(FakeMediaCasBridge())
    }

    private class OrderedMediaCasBridgeFactory(
        private val events: MutableList<String>,
    ) : CasController.MediaCasBridgeFactory {
        override fun create(caSystemId: Int): Result<CasController.MediaCasBridge> =
            Result.success(
                object : CasController.MediaCasBridge {
                    override fun setPrivateData(privateData: ByteArray) = Result.success(Unit)

                    override fun openSession(): Result<CasController.MediaCasSessionBridge> =
                        Result.success(
                            object : CasController.MediaCasSessionBridge {
                                override fun setPrivateData(privateData: ByteArray) = Result.success(Unit)

                                override fun processEcm(section: ByteArray) =
                                    Result.success(EcmProcessResult.RealKeyToken(TunerKeyToken(byteArrayOf(1, 2, 3))))

                                override fun close() {
                                    events += "session-close"
                                }
                            },
                        )

                    override fun processEmm(section: ByteArray) = Result.success(Unit)

                    override fun close() {
                        events += "plugin-close"
                    }
                },
            )
    }

    private class OrderedDescramblerBridge(
        private val events: MutableList<String>,
    ) : CasController.TunerDescramblerBridge {
        override fun setKeyToken(keyToken: TunerKeyToken): Result<Unit> {
            events += "set-key"
            return Result.success(Unit)
        }

        override fun clearKeyToken(): Result<Unit> {
            events += "clear-key"
            return Result.success(Unit)
        }

        override fun addPid(elementaryPid: TsPid): Result<Unit> {
            events += "add:${elementaryPid.value}"
            return Result.success(Unit)
        }

        override fun removePid(elementaryPid: TsPid): Result<Unit> {
            events += "remove:${elementaryPid.value}"
            return Result.success(Unit)
        }

        override fun newSibling(): Result<CasController.TunerDescramblerBridge> = Result.success(this)

        override fun close() {
            events += "descrambler-close"
        }
    }
}

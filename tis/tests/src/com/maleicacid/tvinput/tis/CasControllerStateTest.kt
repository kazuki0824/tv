package com.maleicacid.tvinput.tis

import com.maleicacid.tvinput.aribsi.CaMetadata
import com.maleicacid.tvinput.aribsi.CaMetadataSource
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.TsPid
import com.maleicacid.tvinput.common.TunerKeyToken
import org.junit.Test

/** AndroidJUnitRunner から実行する CasController 状態遷移テスト。 */
class CasControllerStateTest {
    private val serviceKey = ServiceKey(originalNetworkId = 4, transportStreamId = 16625, serviceId = 101)

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
            controller.updateFromCaMetadata(metadata) { creates++; old }
            fun lose() = TunerController.completeResourceLoss(
                invalidate = { if (!accepted) false else { accepted = false; true } },
                cleanup = { controller.clearForResourceLoss() },
                notifyLost = { notifications++; fence.onLost(1L) },
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
                check(runCatching { controller.updateFromCaMetadata(metadata) { creates++; next } }.isFailure)
                check(creates == 1 && old.closes == 2 && next.closes == 0)
                old.failClose = false
            }
            controller.updateFromCaMetadata(metadata) { creates++; next }
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
        val factory = object : CasController.MediaCasBridgeFactory {
            override fun create(caSystemId: Int) = Result.success(object : CasController.MediaCasBridge {
                override fun setPrivateData(privateData: ByteArray) = Result.success(Unit)
                override fun processEmm(section: ByteArray) = Result.success(Unit)
                override fun close() { pluginCloses++ }
                override fun openSession() = Result.success(object : CasController.MediaCasSessionBridge {
                    override fun setPrivateData(privateData: ByteArray) = Result.success(Unit)
                    override fun processEcm(section: ByteArray) = Result.success<EcmProcessResult>(EcmProcessResult.DiagnosticOnly("test"))
                    override fun close() { sessionCloses++; if (rejectSessionClose) throw sessionFailure }
                })
            })
        }
        val bridge = RecordingDescrambler().apply { failClose = true }
        val controller = CasController(mediaCasFactory = factory)
        controller.updateFromCaMetadata(b25Metadata(TsPid(0x101), TsPid(0x123), TsPid(0x010))) { bridge }
        val failure = runCatching { controller.clearForResourceLoss() }.exceptionOrNull()
        check(failure?.cause === sessionFailure && sessionFailure.suppressed.isNotEmpty())
        check(sessionCloses == 1 && pluginCloses == 1 && bridge.closes == 1)
        check(controller.onEcmSection(TsPid(0x123), byteArrayOf(1)).isEmpty())
        rejectSessionClose = false
        bridge.failClose = false
        controller.clearForResourceLoss()
        check(sessionCloses == 2 && pluginCloses == 1 && bridge.closes == 2)
        controller.close()
        check(sessionCloses == 2 && pluginCloses == 1 && bridge.closes == 2)
    }

    @Test fun casAttachAndResourceLossShareOneControllerTransaction() {
        val executor = java.util.concurrent.Executors.newSingleThreadExecutor()
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
            val attaching = executor.submit<CasController.UpdateResult?> {
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
            val lost = executor.submit {
                TunerController.completeResourceLoss(
                    invalidate = { accepted = false; true },
                    cleanup = { controller.clearForResourceLoss() },
                    notifyLost = { notifications++ },
                )
            }
            permitAttach.countDown()
            check(attaching.get(5, java.util.concurrent.TimeUnit.SECONDS) != null)
            lost.get(5, java.util.concurrent.TimeUnit.SECONDS)
            check(old.closes == 1 && notifications == 1)
            val stale = executor.submit<CasController.UpdateResult?> {
                TunerController.updateCasIfCurrent(1L, generation, accepted) {
                    controller.updateFromCaMetadata(metadata) { creates++; old }
                }
            }.get(5, java.util.concurrent.TimeUnit.SECONDS)
            check(stale == null && creates == 1)
            executor.submit {
                generation = 2L
                accepted = true
                check(TunerController.updateCasIfCurrent(1L, generation, accepted) { error("old metadata reattached") } == null)
                check(TunerController.updateCasIfCurrent(2L, generation, accepted) {
                    controller.updateFromCaMetadata(metadata) { creates++; next }
                } != null)
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
        check(bridge.setKeyToken(TunerKeyToken(byteArrayOf(1))).exceptionOrNull()?.message?.contains("退役済み") == true)
        check(bridge.addPid(TsPid(0x101)).isFailure)
        check(bridge.removePid(TsPid(0x101)).isFailure)
    }

    private class RecordingDescrambler : CasController.TunerDescramblerBridge {
        var closes = 0
        var tokens = 0
        var failClose = false
        override fun setKeyToken(keyToken: TunerKeyToken): Result<Unit> { tokens++; return Result.success(Unit) }
        override fun addPid(elementaryPid: TsPid) = Result.success(Unit)
        override fun removePid(elementaryPid: TsPid) = Result.success(Unit)
        override fun close() { closes++; if (failClose) error("descrambler close failed") }
    }

    @Test fun pluginSelectionAndEcmEmmDispatch() {
        val factory = FakeMediaCasBridgeFactory()
        val descrambler = FakeTunerDescramblerBridge()
        val controller = CasController(mediaCasFactory = factory)
        val update = controller.updateFromCaMetadata(b25Metadata(esPid = TsPid(0x101), ecmPid = TsPid(0x123), emmPid = TsPid(0x010)), { descrambler })
        check(update.diagnostics.isEmpty()) { update.diagnostics.toString() }
        check(update.ecmPids == setOf(TsPid(0x123)))
        check(update.emmPids == setOf(TsPid(0x010)))

        val ecmDiagnostics = controller.onEcmSection(TsPid(0x123), byteArrayOf(0x80.toByte()))
        check(ecmDiagnostics.isEmpty()) { ecmDiagnostics.toString() }
        check(descrambler.keyTokens.single().contentEquals(FakeMediaCasSessionBridge.KEY_TOKEN))
        check(0x101 in descrambler.addedPids)

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
        check(factory.created.isEmpty())
        check(controller.lastDiagnostic().state == CasController.State.IDLE)
    }

    @Test fun b1CatDoesNotCreateCasOrEmmBinding() {
        val factory = FakeMediaCasBridgeFactory()
        CasController(mediaCasFactory = factory).use { controller ->
            val metadata = b25Metadata(TsPid(0x101), TsPid(0x123), TsPid(0x010))
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
            val b1 = b25Metadata(TsPid(0x102), TsPid(0x124), TsPid(0x010))
                .map { it.copy(caSystemId = CasController.SupportedCasSystemIds.ARIB_STD_B1) }
            val update = controller.updateFromCaMetadata(
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
        val result = controller.updateFromCaMetadata(
            listOf(CaMetadata(serviceKey, 0x7fff, ecmPid = TsPid(0x123), emmPid = null, elementaryPid = TsPid(0x101), source = CaMetadataSource.ELEMENTARY_STREAM)),
            { FakeTunerDescramblerBridge() },
        )
        check(result.diagnostics.any { it.errorCode == CasController.ErrorCode.UNSUPPORTED_SYSTEM_ID })
        check(result.ecmPids.isEmpty())
    }

    @Test fun pmtUpdateRemovesOldPidAndAddsNewPid() {
        val controller = CasController(mediaCasFactory = FakeMediaCasBridgeFactory())
        val descrambler = FakeTunerDescramblerBridge()
        controller.updateFromCaMetadata(b25Metadata(esPid = TsPid(0x101), ecmPid = TsPid(0x123), emmPid = TsPid(0x010)), { descrambler })
        controller.onEcmSection(TsPid(0x123), byteArrayOf(0x80.toByte()))
        controller.updateFromCaMetadata(b25Metadata(esPid = TsPid(0x102), ecmPid = TsPid(0x124), emmPid = TsPid(0x010)), { descrambler })
        controller.onEcmSection(TsPid(0x124), byteArrayOf(0x80.toByte()))
        check(0x101 in descrambler.removedPids)
        check(0x102 in descrambler.addedPids)
    }

    @Test fun diagnosticOnlyEcmDoesNotSetKeyTokenOrAddPid() {
        val controller = CasController(mediaCasFactory = DiagnosticOnlyMediaCasBridgeFactory())
        val descrambler = FakeTunerDescramblerBridge()
        val update = controller.updateFromCaMetadata(b25Metadata(esPid = TsPid(0x101), ecmPid = TsPid(0x123), emmPid = TsPid(0x010)), { descrambler })
        check(update.diagnostics.isEmpty()) { update.diagnostics.toString() }

        val ecmDiagnostics = controller.onEcmSection(TsPid(0x123), byteArrayOf(0x80.toByte()))
        check(ecmDiagnostics.any { it.errorCode == CasController.ErrorCode.KEY_TOKEN_MISSING }) { ecmDiagnostics.toString() }
        check(descrambler.keyTokens.isEmpty())
        check(descrambler.addedPids.isEmpty())
    }

    @Test fun pluginUnavailableDoesNotAttachDescramblerToken() {
        val controller = CasController(mediaCasFactory = UnavailableMediaCasBridgeFactory())
        val descrambler = FakeTunerDescramblerBridge()
        val update = controller.updateFromCaMetadata(b25Metadata(esPid = TsPid(0x101), ecmPid = TsPid(0x123), emmPid = TsPid(0x010)), { descrambler })
        check(update.diagnostics.any { it.errorCode == CasController.ErrorCode.PLUGIN_UNAVAILABLE }) { update.diagnostics.toString() }
        check(descrambler.keyTokens.isEmpty())
        check(descrambler.addedPids.isEmpty())
    }

    @Test fun sessionOpenFailureIsDistinctFromPluginUnavailable() {
        val factory = SessionFailureMediaCasBridgeFactory()
        val controller = CasController(mediaCasFactory = factory)
        val descrambler = FakeTunerDescramblerBridge()
        val update = controller.updateFromCaMetadata(b25Metadata(esPid = TsPid(0x101), ecmPid = TsPid(0x123), emmPid = TsPid(0x010)), { descrambler })

        check(update.diagnostics.any { it.errorCode == CasController.ErrorCode.SESSION_OPEN_FAILED }) { update.diagnostics.toString() }
        check(update.diagnostics.none { it.errorCode == CasController.ErrorCode.PLUGIN_UNAVAILABLE }) { update.diagnostics.toString() }
        check(factory.bridge.closed)
        check(descrambler.keyTokens.isEmpty())
        check(descrambler.addedPids.isEmpty())
    }

    @Test fun closeReleasesDescrambler() {
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

    private fun b25Metadata(esPid: TsPid, ecmPid: TsPid, emmPid: TsPid): List<CaMetadata> = listOf(
        CaMetadata(serviceKey, CasController.SupportedCasSystemIds.ARIB_STD_B25, ecmPid = ecmPid, emmPid = null, elementaryPid = null, privateData = byteArrayOf(0x01), source = CaMetadataSource.PROGRAM),
        CaMetadata(serviceKey, CasController.SupportedCasSystemIds.ARIB_STD_B25, ecmPid = ecmPid, emmPid = null, elementaryPid = esPid, privateData = byteArrayOf(0x02), source = CaMetadataSource.ELEMENTARY_STREAM),
        CaMetadata(null, CasController.SupportedCasSystemIds.ARIB_STD_B25, ecmPid = null, emmPid = emmPid, elementaryPid = null, privateData = byteArrayOf(0x03), source = CaMetadataSource.CAT),
    )

    private class DiagnosticOnlyMediaCasBridgeFactory : CasController.MediaCasBridgeFactory {
        override fun create(caSystemId: Int): Result<CasController.MediaCasBridge> =
            Result.success(DiagnosticOnlyMediaCasBridge())
    }

    private class DiagnosticOnlyMediaCasBridge : CasController.MediaCasBridge {
        override fun setPrivateData(privateData: ByteArray): Result<Unit> = Result.success(Unit)
        override fun openSession(): Result<CasController.MediaCasSessionBridge> =
            Result.success(DiagnosticOnlyMediaCasSessionBridge())
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
        override fun close() { closed = true }
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
        override fun openSession(): Result<CasController.MediaCasSessionBridge> = Result.success(FakeMediaCasSessionBridge())
        override fun processEmm(section: ByteArray): Result<Unit> { processedEmmCount++; return Result.success(Unit) }
        override fun close() { closed = true }
    }

    private class FakeMediaCasSessionBridge : CasController.MediaCasSessionBridge {
        companion object { val KEY_TOKEN = byteArrayOf(0x11, 0x22, 0x33) }
        override fun setPrivateData(privateData: ByteArray): Result<Unit> = Result.success(Unit)
        override fun processEcm(section: ByteArray): Result<EcmProcessResult> = Result.success(EcmProcessResult.RealKeyToken(TunerKeyToken(KEY_TOKEN.copyOf())))
        override fun close() = Unit
    }

    private class FakeTunerDescramblerBridge : CasController.TunerDescramblerBridge {
        val keyTokens = mutableListOf<ByteArray>()
        val addedPids = linkedSetOf<Int>()
        val removedPids = linkedSetOf<Int>()
        var closed = false
        override fun setKeyToken(keyToken: TunerKeyToken): Result<Unit> { keyTokens += keyToken.toByteArray(); return Result.success(Unit) }
        override fun addPid(elementaryPid: TsPid): Result<Unit> { addedPids += elementaryPid.value; return Result.success(Unit) }
        override fun removePid(elementaryPid: TsPid): Result<Unit> { removedPids += elementaryPid.value; return Result.success(Unit) }
        override fun close() { closed = true }
    }
}

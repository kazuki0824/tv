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

    @Test fun pluginSelectionAndEcmEmmDispatch() {
        val factory = FakeMediaCasBridgeFactory()
        val descrambler = FakeTunerDescramblerBridge()
        val controller = CasController(mediaCasFactory = factory)
        val update = controller.updateFromCaMetadata(
            b25Metadata(esPid = TsPid(0x101), ecmPid = TsPid(0x123), emmPid = TsPid(0x010)),
            descrambler,
        )
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

    @Test fun unsupportedSystemIdIsError() {
        val controller = CasController(mediaCasFactory = FakeMediaCasBridgeFactory())
        val result = controller.updateFromCaMetadata(
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
            FakeTunerDescramblerBridge(),
        )
        check(result.diagnostics.any { it.errorCode == CasController.ErrorCode.UNSUPPORTED_SYSTEM_ID })
        check(result.ecmPids.isEmpty())
        check(result.readiness == CasController.Readiness.ERROR)
    }

    @Test fun pmtUpdateRemovesOldPidAndAddsNewPid() {
        val controller = CasController(mediaCasFactory = FakeMediaCasBridgeFactory())
        val descrambler = FakeTunerDescramblerBridge()
        controller.updateFromCaMetadata(
            b25Metadata(esPid = TsPid(0x101), ecmPid = TsPid(0x123), emmPid = TsPid(0x010)),
            descrambler,
        )
        controller.onEcmSection(TsPid(0x123), byteArrayOf(0x80.toByte()))
        controller.updateFromCaMetadata(
            b25Metadata(esPid = TsPid(0x102), ecmPid = TsPid(0x124), emmPid = TsPid(0x010)),
            descrambler,
        )
        controller.onEcmSection(TsPid(0x124), byteArrayOf(0x80.toByte()))
        check(0x101 in descrambler.removedPids)
        check(0x102 in descrambler.addedPids)
    }

    @Test fun failedPidAddRemainsPendingAndSameMetadataRetriesIt() {
        val controller = CasController(mediaCasFactory = FakeMediaCasBridgeFactory())
        val descrambler = RetryingTunerDescramblerBridge()
        val initial = b25Metadata(esPid = TsPid(0x101), ecmPid = TsPid(0x123), emmPid = TsPid(0x010))
        controller.updateFromCaMetadata(initial, descrambler)
        controller.onEcmSection(TsPid(0x123), byteArrayOf(0x80.toByte()))
        check(controller.currentReadiness() == CasController.Readiness.READY)

        val expanded = b25MetadataForPids(
            listOf(TsPid(0x101), TsPid(0x102)),
            ecmPid = TsPid(0x123),
            emmPid = TsPid(0x010),
        )
        descrambler.failNextAdd(TsPid(0x102))
        val failed = controller.updateFromCaMetadata(expanded, descrambler)
        check(failed.diagnostics.any { it.errorCode == CasController.ErrorCode.DESCRAMBLER_FAILED })
        check(failed.readiness != CasController.Readiness.READY)
        check(controller.currentReadiness() == CasController.Readiness.WAITING_FOR_KEY)
        check(descrambler.linkedPids == linkedSetOf(0x101))

        val retried = controller.updateFromCaMetadata(expanded, descrambler)
        check(retried.diagnostics.isEmpty()) { retried.diagnostics.toString() }
        check(retried.readiness == CasController.Readiness.READY)
        check(descrambler.addAttempts.getValue(0x102) == 2)
        check(descrambler.linkedPids == linkedSetOf(0x101, 0x102))
    }

    @Test fun failedPidRemovalRemainsLinkedAndSameMetadataRetriesIt() {
        val controller = CasController(mediaCasFactory = FakeMediaCasBridgeFactory())
        val descrambler = RetryingTunerDescramblerBridge()
        val initial = b25MetadataForPids(
            listOf(TsPid(0x101), TsPid(0x102)),
            ecmPid = TsPid(0x123),
            emmPid = TsPid(0x010),
        )
        controller.updateFromCaMetadata(initial, descrambler)
        controller.onEcmSection(TsPid(0x123), byteArrayOf(0x80.toByte()))
        check(controller.currentReadiness() == CasController.Readiness.READY)

        val reduced = b25Metadata(esPid = TsPid(0x101), ecmPid = TsPid(0x123), emmPid = TsPid(0x010))
        descrambler.failNextRemove(TsPid(0x102))
        val failed = controller.updateFromCaMetadata(reduced, descrambler)
        check(failed.diagnostics.any { it.errorCode == CasController.ErrorCode.DESCRAMBLER_FAILED })
        check(failed.readiness != CasController.Readiness.READY)
        check(controller.currentReadiness() == CasController.Readiness.WAITING_FOR_KEY)
        check(descrambler.linkedPids == linkedSetOf(0x101, 0x102))

        val retried = controller.updateFromCaMetadata(reduced, descrambler)
        check(retried.diagnostics.isEmpty()) { retried.diagnostics.toString() }
        check(retried.readiness == CasController.Readiness.READY)
        check(descrambler.removeAttempts.getValue(0x102) == 2)
        check(descrambler.linkedPids == linkedSetOf(0x101))
    }

    @Test fun initialPartialPidLinkRollsBackAndNeverReportsReady() {
        val controller = CasController(mediaCasFactory = FakeMediaCasBridgeFactory())
        val descrambler = RetryingTunerDescramblerBridge()
        val metadata = b25MetadataForPids(
            listOf(TsPid(0x101), TsPid(0x102)),
            ecmPid = TsPid(0x123),
            emmPid = TsPid(0x010),
        )
        controller.updateFromCaMetadata(metadata, descrambler)
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

    @Test fun diagnosticOnlyEcmDoesNotSetKeyTokenOrAddPid() {
        val controller = CasController(mediaCasFactory = DiagnosticOnlyMediaCasBridgeFactory())
        val descrambler = FakeTunerDescramblerBridge()
        val update = controller.updateFromCaMetadata(
            b25Metadata(esPid = TsPid(0x101), ecmPid = TsPid(0x123), emmPid = TsPid(0x010)),
            descrambler,
        )
        check(update.diagnostics.isEmpty()) { update.diagnostics.toString() }

        val ecmDiagnostics = controller.onEcmSection(TsPid(0x123), byteArrayOf(0x80.toByte()))
        check(ecmDiagnostics.any { it.errorCode == CasController.ErrorCode.KEY_TOKEN_MISSING }) { ecmDiagnostics.toString() }
        check(descrambler.keyTokens.isEmpty())
        check(descrambler.addedPids.isEmpty())
        check(controller.currentReadiness() == CasController.Readiness.WAITING_FOR_KEY)
    }

    @Test fun pluginUnavailableDoesNotAttachDescramblerToken() {
        val controller = CasController(mediaCasFactory = UnavailableMediaCasBridgeFactory())
        val descrambler = FakeTunerDescramblerBridge()
        val update = controller.updateFromCaMetadata(
            b25Metadata(esPid = TsPid(0x101), ecmPid = TsPid(0x123), emmPid = TsPid(0x010)),
            descrambler,
        )
        check(update.diagnostics.any { it.errorCode == CasController.ErrorCode.PLUGIN_UNAVAILABLE }) { update.diagnostics.toString() }
        check(update.readiness == CasController.Readiness.ERROR)
        check(descrambler.keyTokens.isEmpty())
        check(descrambler.addedPids.isEmpty())
    }

    @Test fun closeUnlinksKeyBeforeClosingDescrambler() {
        val events = mutableListOf<String>()
        val controller = CasController(mediaCasFactory = OrderedMediaCasBridgeFactory(events))
        val descrambler = OrderedDescramblerBridge(events)
        controller.updateFromCaMetadata(
            b25Metadata(esPid = TsPid(0x101), ecmPid = TsPid(0x123), emmPid = TsPid(0x010)),
            descrambler,
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
        val metadata = listOf(
            CaMetadata(serviceKey, CasController.SupportedCasSystemIds.ARIB_STD_B25, TsPid(0x120), null, TsPid(0x101), byteArrayOf(0x11), CaMetadataSource.ELEMENTARY_STREAM),
            CaMetadata(serviceKey, CasController.SupportedCasSystemIds.ARIB_STD_B25, TsPid(0x121), null, TsPid(0x102), byteArrayOf(0x22), CaMetadataSource.ELEMENTARY_STREAM),
        )
        val controller = CasController(mediaCasFactory = factory)
        val update = controller.updateFromCaMetadata(metadata, root)
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
        val metadata = listOf(
            CaMetadata(serviceKey, CasController.SupportedCasSystemIds.ARIB_STD_B1, TsPid(0x123), null, TsPid(0x101), byteArrayOf(0x01), CaMetadataSource.ELEMENTARY_STREAM),
            CaMetadata(null, CasController.SupportedCasSystemIds.ARIB_STD_B1, null, TsPid(0x010), null, byteArrayOf(0x02), CaMetadataSource.CAT),
        )
        val update = controller.updateFromCaMetadata(metadata, descrambler)
        check(update.emmPids.isEmpty())
    }

    private fun b25Metadata(esPid: TsPid, ecmPid: TsPid, emmPid: TsPid): List<CaMetadata> =
        b25MetadataForPids(listOf(esPid), ecmPid, emmPid)

    private fun b25MetadataForPids(
        esPids: List<TsPid>,
        ecmPid: TsPid,
        emmPid: TsPid,
    ): List<CaMetadata> =
        listOf(
            CaMetadata(serviceKey, CasController.SupportedCasSystemIds.ARIB_STD_B25, ecmPid = ecmPid, emmPid = null, elementaryPid = null, privateData = byteArrayOf(0x01), source = CaMetadataSource.PROGRAM),
        ) + esPids.map { esPid ->
            CaMetadata(serviceKey, CasController.SupportedCasSystemIds.ARIB_STD_B25, ecmPid = ecmPid, emmPid = null, elementaryPid = esPid, privateData = byteArrayOf(0x02), source = CaMetadataSource.ELEMENTARY_STREAM)
        } + listOf(
            CaMetadata(null, CasController.SupportedCasSystemIds.ARIB_STD_B25, ecmPid = null, emmPid = emmPid, elementaryPid = null, privateData = byteArrayOf(0x03), source = CaMetadataSource.CAT),
        )

    private class DiagnosticOnlyMediaCasBridgeFactory : CasController.MediaCasBridgeFactory {
        override fun create(caSystemId: Int): Result<CasController.MediaCasBridge> = Result.success(DiagnosticOnlyMediaCasBridge())
    }
    private class DiagnosticOnlyMediaCasBridge : CasController.MediaCasBridge {
        override fun setPrivateData(privateData: ByteArray): Result<Unit> = Result.success(Unit)
        override fun openSession(): Result<CasController.MediaCasSessionBridge> = Result.success(DiagnosticOnlyMediaCasSessionBridge())
        override fun processEmm(section: ByteArray): Result<Unit> = Result.success(Unit)
        override fun close() = Unit
    }
    private class DiagnosticOnlyMediaCasSessionBridge : CasController.MediaCasSessionBridge {
        override fun setPrivateData(privateData: ByteArray): Result<Unit> = Result.success(Unit)
        override fun processEcm(section: ByteArray): Result<EcmProcessResult> = Result.success(EcmProcessResult.DiagnosticOnly("placeholder CAS は実 key token を返しません"))
        override fun close() = Unit
    }

    private class UnavailableMediaCasBridgeFactory : CasController.MediaCasBridgeFactory {
        override fun create(caSystemId: Int): Result<CasController.MediaCasBridge> = Result.failure(IllegalStateException("placeholder CAS plugin は利用できません"))
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

    private open class FakeTunerDescramblerBridge : CasController.TunerDescramblerBridge {
        val keyTokens = mutableListOf<ByteArray>()
        val addedPids = linkedSetOf<Int>()
        val removedPids = linkedSetOf<Int>()
        var clearKeyCount = 0
        var closed = false
        override fun setKeyToken(keyToken: TunerKeyToken): Result<Unit> { keyTokens += keyToken.toByteArray(); return Result.success(Unit) }
        override fun clearKeyToken(): Result<Unit> { clearKeyCount++; return Result.success(Unit) }
        override fun addPid(elementaryPid: TsPid): Result<Unit> { addedPids += elementaryPid.value; return Result.success(Unit) }
        override fun removePid(elementaryPid: TsPid): Result<Unit> { removedPids += elementaryPid.value; return Result.success(Unit) }
        override fun newSibling(): Result<CasController.TunerDescramblerBridge> = Result.success(this)
        override fun close() { closed = true }
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

    private class OrderedMediaCasBridgeFactory(private val events: MutableList<String>) : CasController.MediaCasBridgeFactory {
        override fun create(caSystemId: Int): Result<CasController.MediaCasBridge> = Result.success(object : CasController.MediaCasBridge {
            override fun setPrivateData(privateData: ByteArray) = Result.success(Unit)
            override fun openSession(): Result<CasController.MediaCasSessionBridge> = Result.success(object : CasController.MediaCasSessionBridge {
                override fun setPrivateData(privateData: ByteArray) = Result.success(Unit)
                override fun processEcm(section: ByteArray) = Result.success(EcmProcessResult.RealKeyToken(TunerKeyToken(byteArrayOf(1, 2, 3))))
                override fun close() { events += "session-close" }
            })
            override fun processEmm(section: ByteArray) = Result.success(Unit)
            override fun close() { events += "plugin-close" }
        })
    }

    private class OrderedDescramblerBridge(private val events: MutableList<String>) : CasController.TunerDescramblerBridge {
        override fun setKeyToken(keyToken: TunerKeyToken): Result<Unit> { events += "set-key"; return Result.success(Unit) }
        override fun clearKeyToken(): Result<Unit> { events += "clear-key"; return Result.success(Unit) }
        override fun addPid(elementaryPid: TsPid): Result<Unit> { events += "add:${elementaryPid.value}"; return Result.success(Unit) }
        override fun removePid(elementaryPid: TsPid): Result<Unit> { events += "remove:${elementaryPid.value}"; return Result.success(Unit) }
        override fun newSibling(): Result<CasController.TunerDescramblerBridge> = Result.success(this)
        override fun close() { events += "descrambler-close" }
    }
}

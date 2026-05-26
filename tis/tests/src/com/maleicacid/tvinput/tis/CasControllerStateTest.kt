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
        val update = controller.updateFromCaMetadata(b25Metadata(esPid = TsPid(0x101), ecmPid = TsPid(0x123), emmPid = TsPid(0x010)), descrambler)
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

    @Test fun unsupportedSystemIdIsError() {
        val controller = CasController(mediaCasFactory = FakeMediaCasBridgeFactory())
        val result = controller.updateFromCaMetadata(
            listOf(CaMetadata(serviceKey, 0x7fff, ecmPid = TsPid(0x123), emmPid = null, elementaryPid = TsPid(0x101), source = CaMetadataSource.ELEMENTARY_STREAM)),
            FakeTunerDescramblerBridge(),
        )
        check(result.diagnostics.any { it.errorCode == CasController.ErrorCode.UNSUPPORTED_SYSTEM_ID })
        check(result.ecmPids.isEmpty())
    }

    @Test fun pmtUpdateRemovesOldPidAndAddsNewPid() {
        val controller = CasController(mediaCasFactory = FakeMediaCasBridgeFactory())
        val descrambler = FakeTunerDescramblerBridge()
        controller.updateFromCaMetadata(b25Metadata(esPid = TsPid(0x101), ecmPid = TsPid(0x123), emmPid = TsPid(0x010)), descrambler)
        controller.onEcmSection(TsPid(0x123), byteArrayOf(0x80.toByte()))
        controller.updateFromCaMetadata(b25Metadata(esPid = TsPid(0x102), ecmPid = TsPid(0x124), emmPid = TsPid(0x010)), descrambler)
        controller.onEcmSection(TsPid(0x124), byteArrayOf(0x80.toByte()))
        check(0x101 in descrambler.removedPids)
        check(0x102 in descrambler.addedPids)
    }

    @Test fun diagnosticOnlyEcmDoesNotSetKeyTokenOrAddPid() {
        val controller = CasController(mediaCasFactory = DiagnosticOnlyMediaCasBridgeFactory())
        val descrambler = FakeTunerDescramblerBridge()
        val update = controller.updateFromCaMetadata(b25Metadata(esPid = TsPid(0x101), ecmPid = TsPid(0x123), emmPid = TsPid(0x010)), descrambler)
        check(update.diagnostics.isEmpty()) { update.diagnostics.toString() }

        val ecmDiagnostics = controller.onEcmSection(TsPid(0x123), byteArrayOf(0x80.toByte()))
        check(ecmDiagnostics.any { it.errorCode == CasController.ErrorCode.KEY_TOKEN_MISSING }) { ecmDiagnostics.toString() }
        check(descrambler.keyTokens.isEmpty())
        check(descrambler.addedPids.isEmpty())
    }

    @Test fun pluginUnavailableDoesNotAttachDescramblerToken() {
        val controller = CasController(mediaCasFactory = UnavailableMediaCasBridgeFactory())
        val descrambler = FakeTunerDescramblerBridge()
        val update = controller.updateFromCaMetadata(b25Metadata(esPid = TsPid(0x101), ecmPid = TsPid(0x123), emmPid = TsPid(0x010)), descrambler)
        check(update.diagnostics.any { it.errorCode == CasController.ErrorCode.SESSION_OPEN_FAILED }) { update.diagnostics.toString() }
        check(descrambler.keyTokens.isEmpty())
        check(descrambler.addedPids.isEmpty())
    }

    @Test fun closeReleasesDescrambler() {
        val controller = CasController(mediaCasFactory = FakeMediaCasBridgeFactory())
        val descrambler = FakeTunerDescramblerBridge()
        controller.updateFromCaMetadata(b25Metadata(esPid = TsPid(0x101), ecmPid = TsPid(0x123), emmPid = TsPid(0x010)), descrambler)
        controller.close()
        check(descrambler.closed)
        check(controller.lastDiagnostic().state == CasController.State.CLOSED)
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

// テストの入力・期待値を本体の定数と独立した具体値で記述する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import com.maleicacid.tvinput.aribsi.CaMetadata
import com.maleicacid.tvinput.aribsi.CaMetadataSource
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.TsPid
import com.maleicacid.tvinput.common.TunerKeyToken
import org.junit.Test

/** plugin内のsession所有と、frameworkによるDescrambler閉鎖を検証する。 */
class CasControllerSessionTest {
    private val first =
        CaMetadata(
            ServiceKey(4, 16625, 101),
            5,
            TsPid(0x123),
            null,
            TsPid(0x101),
            byteArrayOf(2),
            CaMetadataSource.ELEMENTARY_STREAM,
        )
    private val cat = CaMetadata(null, 5, null, TsPid(0x010), null, byteArrayOf(3), CaMetadataSource.CAT)

    @Test fun frameworkReclaimClosesCasWithoutVoidOnClosedDescrambler() {
        val factory = Factory()
        val bridge = Descrambler()
        CasController(mediaCasFactory = factory).use { controller ->
            check(controller.updateFromCaMetadata(listOf(first, cat)) { bridge }.diagnostics.isEmpty())
            check(controller.onEcmSection(TsPid(0x123), byteArrayOf(1)).isEmpty())
            // AOSPのreleaseAll完了後にonResourceLostが届く順序を再現する。
            bridge.close()
            controller.onTunerResourcesReclaimed()
            check(bridge.tokens.size == 1 && bridge.closes == 2)
            val old = factory.plugins.single()
            check(old.closed && old.sessions.single().closed)
            check(controller.onEcmSection(TsPid(0x123), byteArrayOf(1)).isEmpty())
            val next = Descrambler()
            check(controller.updateFromCaMetadata(listOf(first, cat)) { next }.diagnostics.isEmpty())
            check(factory.plugins.size == 2)
            check(controller.onEcmSection(TsPid(0x123), byteArrayOf(1)).isEmpty())
            check(next.tokens.size == 1)
        }
    }

    @Test fun sameSystemSeparatesEcmSessionsAndKeepsOneEmmOwner() {
        val second = first.copy(elementaryPid = TsPid(0x102))
        val variants =
            listOf(
                second.copy(ecmPid = TsPid(0x124)),
                second.copy(serviceKey = ServiceKey(5, 16625, 101)),
                second.copy(privateData = byteArrayOf(9)),
            )
        for (binding in variants) {
            val factory = Factory()
            val bridges = mutableListOf<Descrambler>()
            CasController(mediaCasFactory = factory).use { controller ->
                val update =
                    controller.updateFromCaMetadata(listOf(first, binding, cat)) {
                        Descrambler().also { bridges += it }
                    }
                check(update.diagnostics.isEmpty())
                val plugin = factory.plugins.single()
                check(plugin.sessions.size == 2 && bridges.size == 2)
                update.ecmPids.forEach { check(controller.onEcmSection(it, byteArrayOf(1)).isEmpty()) }
                check(!bridges[0].tokens.single().contentEquals(bridges[1].tokens.single()))
                check(bridges[0].pids == setOf(0x101) && bridges[1].pids == setOf(0x102))
                check(controller.onEmmSection(TsPid(0x010), byteArrayOf(1)).isEmpty())
                check(plugin.emms == 1)
                val remaining =
                    controller.updateFromCaMetadata(listOf(binding, cat)) {
                        error("存続sessionのbridgeは再生成しない")
                    }
                check(remaining.diagnostics.isEmpty())
                check(bridges[0].closed && !bridges[1].closed && !plugin.closed)
                check(plugin.sessions[0].closed && !plugin.sessions[1].closed)
                check(controller.onEcmSection(requireNotNull(binding.ecmPid), byteArrayOf(2)).isEmpty())
                check(bridges[1].tokens.size == 2)
            }
        }
    }

    private class Factory : CasController.MediaCasBridgeFactory {
        val plugins = mutableListOf<Plugin>()

        override fun create(caSystemId: Int): Result<CasController.MediaCasBridge> {
            val plugin = Plugin()
            plugins += plugin
            return Result.success(plugin)
        }
    }

    private class Plugin : CasController.MediaCasBridge {
        val sessions = mutableListOf<Session>()
        var emms = 0
        var closed = false

        override fun setPrivateData(privateData: ByteArray) = Result.success(Unit)

        override fun openSession(): Result<CasController.MediaCasSessionBridge> {
            check(!closed)
            return Result.success(Session((sessions.size + 1).toByte()).also { sessions += it })
        }

        override fun processEmm(section: ByteArray): Result<Unit> {
            check(!closed)
            emms++
            return Result.success(Unit)
        }

        override fun close() {
            check(sessions.all { it.closed })
            closed = true
        }
    }

    private class Session(
        private val token: Byte,
    ) : CasController.MediaCasSessionBridge {
        var closed = false

        override fun setPrivateData(privateData: ByteArray) = Result.success(Unit)

        override fun processEcm(section: ByteArray): Result<EcmProcessResult> {
            check(!closed)
            return Result.success(EcmProcessResult.RealKeyToken(TunerKeyToken(byteArrayOf(token))))
        }

        override fun close() {
            closed = true
        }
    }

    private class Descrambler : CasController.TunerDescramblerBridge {
        val tokens = mutableListOf<ByteArray>()
        val pids = linkedSetOf<Int>()
        var closes = 0
        var closed = false

        override fun setKeyToken(keyToken: TunerKeyToken): Result<Unit> {
            check(!closed) { "閉鎖済みDescramblerへkey tokenを送らない" }
            tokens += keyToken.toByteArray()
            return Result.success(Unit)
        }

        override fun addPid(elementaryPid: TsPid): Result<Unit> {
            pids += elementaryPid.value
            return Result.success(Unit)
        }

        override fun removePid(elementaryPid: TsPid): Result<Unit> {
            pids -= elementaryPid.value
            return Result.success(Unit)
        }

        override fun close() {
            closes++
            closed = true
        }
    }
}

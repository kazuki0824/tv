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
                check(bridges[1].tokens.size == 1)
            }
        }
    }

    @Test fun laterEcmFailurePreservesReadyStateAndDoesNotRelink() {
        val factory = Factory()
        val bridge = Descrambler()
        val service = requireNotNull(first.serviceKey)
        val changes = mutableListOf<CasController.ConnectionChange>()
        CasController(mediaCasFactory = factory).use { controller ->
            controller.setOnConnectionChanged { _, change -> changes += change }
            controller.updateFromCaMetadata(listOf(first), 1L) { bridge }
            val session =
                factory.plugins
                    .single()
                    .sessions
                    .single()
            session.ecmFailure = IllegalStateException("ECMの処理失敗")
            val initialFailure = controller.onEcmSection(TsPid(0x123), byteArrayOf(1))
            check(initialFailure.single().errorCode == CasController.ErrorCode.ECM_FAILED)
            check(!controller.isServiceDescramblingReady(service, 1L) && bridge.tokens.isEmpty())
            session.ecmFailure = null
            bridge.rejectToken = true
            val linkFailure = controller.onEcmSection(TsPid(0x123), byteArrayOf(1))
            check(linkFailure.single().errorCode == CasController.ErrorCode.DESCRAMBLER_FAILED)
            check(!controller.isServiceDescramblingReady(service, 1L))
            bridge.rejectToken = false
            check(controller.onEcmSection(TsPid(0x123), byteArrayOf(1)).isEmpty())
            check(controller.isServiceDescramblingReady(service, 1L) && bridge.tokens.size == 1)
            changes.clear()
            session.ecmFailure = IllegalStateException("後続ECMの処理失敗")
            val laterFailure = controller.onEcmSection(TsPid(0x123), byteArrayOf(2))
            check(laterFailure.single().errorCode == CasController.ErrorCode.ECM_FAILED)
            check(controller.isServiceDescramblingReady(service, 1L) && changes.isEmpty())
            session.ecmFailure = null
            bridge.rejectToken = true
            check(controller.onEcmSection(TsPid(0x123), byteArrayOf(3)).isEmpty())
            check(controller.isServiceDescramblingReady(service, 1L) && bridge.tokens.size == 1 && changes.isEmpty())
            bridge.rejectToken = false
            controller.clearForResourceLoss()
            check(!controller.isServiceDescramblingReady(service, 1L) && session.closed)
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

    @Test fun initialCapacityGatesSessionsAndUsesLatestMetadataOnce() {
        val factory = PendingFactory()
        val changes = mutableListOf<CasController.ConnectionChange>()
        val descrambler = Descrambler()
        CasController(mediaCasFactory = factory).use { controller ->
            controller.setOnConnectionChanged { _, change -> changes += change }
            val waiting = controller.updateFromCaMetadata(listOf(first, cat), 7L) { error("容量反映前に生成しない") }
            check(waiting.ecmPids.isEmpty() && waiting.emmPids.isEmpty())
            val plugin = factory.plugins.single()
            plugin.listener.onConstructed()
            controller.updateFromCaMetadata(listOf(first), 7L) { error("構築だけで生成しない") }
            check(plugin.opens == 0 && changes.isEmpty())
            plugin.listener.onCapacity(2)
            plugin.listener.onCapacity(3)
            val next = first.copy(ecmPid = TsPid(0x124))
            val ready = controller.updateFromCaMetadata(listOf(next), 7L) { descrambler }
            check(ready.ecmPids == setOf(TsPid(0x124)) && plugin.opens == 1)
            check(changes == listOf(CasController.ConnectionChange.READY) && plugin.timerCancelled)
            plugin.listener.onCapacity(2)
            controller.updateFromCaMetadata(listOf(next), 7L) { error("重複通知で再生成しない") }
            check(plugin.opens == 1 && changes.size == 1)
        }
    }

    @Test fun lateCapacityAndTimeoutBothTerminateWithoutAutomaticRetry() {
        for (deliverTimer in listOf(false, true)) {
            val factory = PendingFactory()
            val changes = mutableListOf<CasController.ConnectionChange>()
            CasController(mediaCasFactory = factory).use { controller ->
                controller.setOnConnectionChanged { _, change -> changes += change }
                controller.updateFromCaMetadata(listOf(first), 1L)
                val plugin = factory.plugins.single()
                plugin.now = 110L
                if (deliverTimer) requireNotNull(plugin.timeout).invoke() else plugin.listener.onCapacity(1)
                val result = controller.updateFromCaMetadata(listOf(first), 1L)
                check(result.diagnostics.any { it.state == CasController.State.ERROR })
                plugin.listener.onCapacity(Int.MAX_VALUE)
                controller.updateFromCaMetadata(listOf(first), 1L)
                check(plugin.opens == 0 && plugin.closes == 1 && factory.plugins.size == 1)
                check(changes == listOf(CasController.ConnectionChange.INITIALIZATION_FAILED))
            }
        }
    }

    @Test fun invalidCapacityConstructionFailureAndInvalidBudgetRejectInitialization() {
        for (failureKind in 0..3) {
            val factory = PendingFactory(if (failureKind == 3) 0L else 10L)
            CasController(mediaCasFactory = factory).use { controller ->
                controller.updateFromCaMetadata(listOf(first), 1L)
                val plugin = factory.plugins.single()
                when (failureKind) {
                    0 -> plugin.listener.onCapacity(0)
                    1 -> plugin.listener.onCapacity(-1)
                    2 -> plugin.listener.onFailure(IllegalStateException("TRMまたは構築を利用できない"))
                }
                val result = controller.updateFromCaMetadata(listOf(first), 1L)
                check(result.diagnostics.any { it.state == CasController.State.ERROR })
                check(plugin.opens == 0 && plugin.closes == 1)
            }
        }
    }

    @Test fun cancellationRejectsOldCapacityAndResourceLostForReplacementPlugin() {
        val factory = PendingFactory()
        CasController(mediaCasFactory = factory).use { controller ->
            controller.updateFromCaMetadata(listOf(first), 1L)
            val old = factory.plugins.single()
            controller.clearForResourceLoss()
            controller.updateFromCaMetadata(listOf(first), 2L)
            val current = factory.plugins.last()
            old.listener.onCapacity(1)
            old.listener.onResourceLost()
            controller.updateFromCaMetadata(listOf(first), 2L)
            check(current.opens == 0 && old.opens == 0 && old.closes == 1)
            current.listener.onCapacity(1)
            check(controller.updateFromCaMetadata(listOf(first), 2L).diagnostics.isEmpty())
            check(current.opens == 1 && factory.plugins.size == 2)
        }
    }

    @Test fun mediaCasReclaimSkipsSessionCloseAndRetriesOwnedDescramblerAndPlugin() {
        for (failPlugin in listOf(false, true)) {
            val factory = PendingFactory()
            val descrambler = Descrambler()
            CasController(mediaCasFactory = factory).use { controller ->
                controller.updateFromCaMetadata(listOf(first, cat), 1L)
                val plugin = factory.plugins.single()
                plugin.listener.onCapacity(2)
                controller.updateFromCaMetadata(listOf(first, cat), 1L) { descrambler }
                controller.onEcmSection(TsPid(0x123), byteArrayOf(1))
                val tokens = descrambler.tokens.size
                plugin.listener.onResourceLost()
                check(controller.onEcmSection(TsPid(0x123), byteArrayOf(1)).isEmpty())
                check(controller.onEmmSection(TsPid(0x010), byteArrayOf(1)).isEmpty())
                descrambler.failClose = !failPlugin
                plugin.failClose = failPlugin
                check(runCatching { controller.clearForResourceLoss() }.isFailure)
                check(plugin.sessionCloses == 0 && descrambler.tokens.size == tokens)
                check(plugin.emms == 0)
                descrambler.failClose = false
                plugin.failClose = false
                controller.clearForResourceLoss()
                check(descrambler.closed && plugin.closes == if (failPlugin) 2 else 1)
                check(plugin.sessionCloses == 0)
                val rejected = controller.updateFromCaMetadata(listOf(first), 1L)
                check(rejected.diagnostics.any { it.state == CasController.State.ERROR })
            }
        }
    }

    @Test fun lateConstructionAfterCloseRetainsOwnershipUntilCleanup() {
        val factory = PendingFactory()
        val controller = CasController(mediaCasFactory = factory)
        controller.updateFromCaMetadata(listOf(first), 1L)
        val plugin = factory.plugins.single()
        plugin.constructing = true
        check(runCatching { controller.close() }.isFailure)
        plugin.constructing = false
        plugin.listener.onConstructed()
        plugin.listener.onCapacity(1)
        controller.close()
        check(plugin.opens == 0 && plugin.closes == 2)
    }

    @Test fun reclaimIsLocalToItsController() {
        val firstFactory = PendingFactory()
        val secondFactory = PendingFactory()
        CasController(mediaCasFactory = firstFactory).use { a ->
            CasController(mediaCasFactory = secondFactory).use { b ->
                a.updateFromCaMetadata(listOf(first), 1L)
                b.updateFromCaMetadata(listOf(first), 1L)
                val pa = firstFactory.plugins.single()
                val pb = secondFactory.plugins.single()
                pa.listener.onCapacity(2)
                pb.listener.onCapacity(2)
                a.updateFromCaMetadata(listOf(first), 1L)
                b.updateFromCaMetadata(listOf(first), 1L)
                pa.listener.onResourceLost()
                a.clearForResourceLoss()
                check(b.updateFromCaMetadata(listOf(first), 1L).diagnostics.isEmpty())
                check(pb.closes == 0 && pb.opens == 1)
            }
        }
    }

    private class PendingFactory(
        private val budget: Long = 10L,
    ) : CasController.MediaCasBridgeFactory {
        val plugins = mutableListOf<PendingPlugin>()

        override fun create(caSystemId: Int): Result<CasController.MediaCasBridge> =
            Result.success(PendingPlugin(budget).also { plugins += it })
    }

    private class PendingPlugin(
        override val initializationBudgetMillis: Long,
    ) : CasController.InitializingMediaCasBridge {
        lateinit var listener: CasController.ConnectionListener
        var now = 100L
        var timeout: (() -> Unit)? = null
        var timerCancelled = false
        var opens = 0
        var closes = 0
        var sessionCloses = 0
        var emms = 0
        var failClose = false
        var constructing = false

        override fun elapsedRealtime(): Long = now

        override fun initialize(listener: CasController.ConnectionListener) {
            this.listener = listener
        }

        override fun scheduleTimeout(
            delayMillis: Long,
            action: () -> Unit,
        ): () -> Unit {
            check(delayMillis == 10L)
            timeout = action
            return { timerCancelled = true }
        }

        override fun setPrivateData(privateData: ByteArray): Result<Unit> = Result.success(Unit)

        override fun processEmm(section: ByteArray): Result<Unit> {
            emms++
            return Result.success(Unit)
        }

        override fun openSession(): Result<CasController.MediaCasSessionBridge> {
            opens++
            return Result.success(
                object : CasController.MediaCasSessionBridge {
                    override fun setPrivateData(privateData: ByteArray): Result<Unit> = Result.success(Unit)

                    override fun processEcm(section: ByteArray): Result<EcmProcessResult> =
                        Result.success(EcmProcessResult.RealKeyToken(TunerKeyToken(byteArrayOf(1))))

                    override fun close() {
                        sessionCloses++
                    }
                },
            )
        }

        override fun close() {
            closes++
            check(!failClose && !constructing) { "pluginの解放を再試行する" }
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
        var ecmFailure: Throwable? = null

        override fun setPrivateData(privateData: ByteArray) = Result.success(Unit)

        override fun processEcm(section: ByteArray): Result<EcmProcessResult> {
            check(!closed)
            ecmFailure?.let { return Result.failure(it) }
            return Result.success(EcmProcessResult.RealKeyToken(TunerKeyToken(byteArrayOf(token))))
        }

        override fun close() {
            closed = true
        }
    }

    private class Descrambler : CasController.TunerDescramblerBridge {
        var rejectToken = false
        val tokens = mutableListOf<ByteArray>()
        val pids = linkedSetOf<Int>()
        var closes = 0
        var closed = false

        override fun setKeyToken(keyToken: TunerKeyToken): Result<Unit> {
            check(!closed) { "閉鎖済みDescramblerへkey tokenを送らない" }
            if (rejectToken) return Result.failure(IllegalStateException("鍵結合を利用できない"))
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
            check(!failClose) { "Descramblerの解放を再試行する" }
            closed = true
        }

        var failClose = false
    }
}

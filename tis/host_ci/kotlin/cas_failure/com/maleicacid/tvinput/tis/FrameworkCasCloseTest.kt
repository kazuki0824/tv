// テストの入力・期待値を本体の定数と独立した具体値で記述する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import android.media.MediaCas
import com.maleicacid.tvinput.aribsi.CaMetadata
import com.maleicacid.tvinput.aribsi.CaMetadataSource
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.TsPid
import com.maleicacid.tvinput.common.TunerKeyToken
import org.junit.Test

// 同じFramework adapterの操作境界と終了・再接続をまとめて検証する。
@Suppress("TooManyFunctions")
class FrameworkCasCloseTest {
    private val metadata =
        listOf(
            CaMetadata(
                ServiceKey(4, 16625, 101),
                5,
                ecmPid = TsPid(0x123),
                emmPid = null,
                elementaryPid = TsPid(0x101),
                source = CaMetadataSource.ELEMENTARY_STREAM,
            ),
        )

    private val multipleSessions =
        metadata +
            metadata.single().copy(ecmPid = TsPid(0x124), elementaryPid = TsPid(0x102)) +
            CaMetadata(null, 5, null, TsPid(0x010), null, source = CaMetadataSource.CAT)

    @Test fun everyFrameworkInvalidationBoundaryRetiresAllSessionsAndAllowsLaterReconnection() {
        for (operation in MediaCas.Operation.entries) {
            val f = MediaCas.Faults
            f.reset()
            val descramblers = mutableListOf<Descrambler>()
            CasController().use { controller ->
                check(
                    controller
                        .updateFromCaMetadata(multipleSessions) {
                            Descrambler().also { descramblers += it }
                        }.diagnostics
                        .isEmpty(),
                )
                f.invalidateAt = operation
                when (operation) {
                    MediaCas.Operation.OPEN -> {
                        val third = metadata.single().copy(ecmPid = TsPid(0x125), elementaryPid = TsPid(0x103))
                        val result = controller.updateFromCaMetadata(multipleSessions + third)
                        check(result.ecmPids.isEmpty() && result.emmPids.isEmpty())
                    }

                    MediaCas.Operation.PLUGIN_PRIVATE_DATA, MediaCas.Operation.SESSION_PRIVATE_DATA -> {
                        val result = controller.updateFromCaMetadata(multipleSessions)
                        check(result.ecmPids.isEmpty() && result.emmPids.isEmpty())
                    }

                    MediaCas.Operation.ECM -> {
                        controller.onEcmSection(TsPid(0x123), byteArrayOf(1))
                    }

                    MediaCas.Operation.EMM -> {
                        controller.onEmmSection(TsPid(0x010), byteArrayOf(1))
                    }

                    MediaCas.Operation.SESSION_CLOSE -> {
                        controller.clearForResourceLoss()
                    }
                }
                check(controller.lastDiagnostic().errorCode == CasController.ErrorCode.MEDIA_CAS_INVALIDATED)
                check(f.creates == 1)
                val calls = f.calls.toList()
                check(controller.onEcmSection(TsPid(0x124), byteArrayOf(1)).isEmpty())
                check(controller.onEmmSection(TsPid(0x010), byteArrayOf(1)).isEmpty())
                controller.clearForResourceLoss()
                check(f.calls == calls)
                check(f.sessionCloses == if (operation == MediaCas.Operation.SESSION_CLOSE) 1 else 0)
                check(f.pluginCloses == 1 && descramblers.size == 2 && descramblers.all { it.closes == 1 })
                check(controller.lastDiagnostic().errorCode == CasController.ErrorCode.MEDIA_CAS_INVALIDATED)
                check(controller.updateFromCaMetadata(multipleSessions).diagnostics.isEmpty())
                check(f.creates == 2)
            }
        }
    }

    @Test fun invalidationDuringObsoleteSessionCloseRejectsMetadataBeforeCreatingReplacement() {
        val f = MediaCas.Faults
        f.reset()
        CasController().use { controller ->
            controller.updateFromCaMetadata(multipleSessions)
            f.invalidateAt = MediaCas.Operation.SESSION_CLOSE
            check(runCatching { controller.updateFromCaMetadata(metadata) }.isFailure)
            check(f.creates == 1 && f.sessionCloses == 1 && f.pluginCloses == 0)
            controller.clearForResourceLoss()
            check(f.sessionCloses == 1 && f.pluginCloses == 1)
            check(controller.lastDiagnostic().errorCode == CasController.ErrorCode.MEDIA_CAS_INVALIDATED)
            check(controller.updateFromCaMetadata(metadata).diagnostics.isEmpty())
            check(f.creates == 2)
        }
    }

    @Test fun invalidationWhileClosingObsoletePluginOrClearingMetadataRejectsThatUpdate() {
        for (replacement in listOf(emptyList(), metadata.map { it.copy(caSystemId = 1) })) {
            val f = MediaCas.Faults
            f.reset()
            CasController().use { controller ->
                controller.updateFromCaMetadata(metadata)
                f.invalidateAt = MediaCas.Operation.SESSION_CLOSE
                check(runCatching { controller.updateFromCaMetadata(replacement) }.isFailure)
                check(f.creates == 1 && f.sessionCloses == 1 && f.pluginCloses == 1)
                check(controller.lastDiagnostic().errorCode == CasController.ErrorCode.MEDIA_CAS_INVALIDATED)
                check(controller.updateFromCaMetadata(metadata).diagnostics.isEmpty() && f.creates == 2)
            }
        }
    }

    @Test fun ordinaryOpenFailureFollowedByRollbackInvalidationPreservesBothFailures() {
        val f = MediaCas.Faults
        f.reset()
        CasController().use { controller ->
            controller.updateFromCaMetadata(metadata)
            f.openFailure = true
            f.invalidateAt = MediaCas.Operation.SESSION_CLOSE
            val diagnostic = controller.updateFromCaMetadata(multipleSessions).diagnostics.single()
            check(diagnostic.errorCode == CasController.ErrorCode.MEDIA_CAS_INVALIDATED)
            val openFailure = requireNotNull(diagnostic.cause?.cause)
            check(openFailure is UnsupportedOperationException)
            check(openFailure.suppressed.single() is MediaCasInvalidatedException)
            check(f.creates == 1 && f.pluginCloses == 1 && f.sessionCloses == 1)
            f.openFailure = false
            check(controller.updateFromCaMetadata(metadata).diagnostics.isEmpty() && f.creates == 2)
        }
    }

    @Test fun terminalCleanupRetriesOnlyUnfinishedDescramblersAndFrameworkClose() {
        val f = MediaCas.Faults
        f.reset()
        val descramblers = mutableListOf<Descrambler>()
        CasController().use { controller ->
            controller.updateFromCaMetadata(multipleSessions) { Descrambler().also { descramblers += it } }
            f.invalidateAt = MediaCas.Operation.ECM
            controller.onEcmSection(TsPid(0x123), byteArrayOf(1))
            descramblers.first().rejectClose = true
            check(runCatching { controller.clearForResourceLoss() }.isFailure)
            check(descramblers.map { it.closes } == listOf(1, 1))
            check(f.pluginCloses == 0 && f.sessionCloses == 0)
            descramblers.first().rejectClose = false
            f.pluginFailure = true
            check(runCatching { controller.updateFromCaMetadata(metadata) }.isFailure)
            check(descramblers.map { it.closes } == listOf(2, 1))
            check(f.creates == 1 && f.pluginCloses == 1 && f.sessionCloses == 0)
            f.pluginFailure = false
            controller.clearForResourceLoss()
            check(descramblers.map { it.closes } == listOf(2, 1))
            check(f.pluginCloses == 2 && f.sessionCloses == 0)
            check(controller.lastDiagnostic().errorCode == CasController.ErrorCode.MEDIA_CAS_INVALIDATED)
            check(controller.updateFromCaMetadata(metadata).diagnostics.isEmpty())
            check(f.creates == 2)
        }
    }

    @Test fun invalidatedAdapterBlocksAllOrdinaryCallsAcrossItsSessionsButAllowsFrameworkClose() {
        val f = MediaCas.Faults
        f.reset()
        val plugin = FrameworkMediaCasBridgeFactory().create(5).getOrThrow()
        val first = plugin.openSession().getOrThrow()
        val second = plugin.openSession().getOrThrow()
        f.invalidateAt = MediaCas.Operation.ECM
        val failure = first.processEcm(byteArrayOf(1)).exceptionOrNull()
        check(failure is MediaCasInvalidatedException && failure.cause is IllegalStateException)
        val calls = f.calls.toList()
        for (operation in MediaCas.Operation.entries) {
            check(invokeOperation(plugin, second, operation).exceptionOrNull() === failure)
        }
        check(f.calls == calls && f.sessionCloses == 0)
        plugin.close()
        check(f.pluginCloses == 1)
    }

    @Test fun casSpecificStateAndArgumentErrorsDoNotInvalidateFrameworkConnection() {
        for (operation in MediaCas.Operation.entries) {
            for (stateError in listOf(true, false)) {
                val f = MediaCas.Faults
                f.reset()
                val plugin = FrameworkMediaCasBridgeFactory().create(5).getOrThrow()
                val session = plugin.openSession().getOrThrow()
                if (stateError) f.stateFailureAt = operation else f.argumentFailureAt = operation
                val failure = invokeOperation(plugin, session, operation).exceptionOrNull()
                if (stateError) {
                    check(failure is android.media.MediaCasStateException)
                } else {
                    check(failure is IllegalArgumentException)
                }
                f.stateFailureAt = null
                f.argumentFailureAt = null
                check(invokeOperation(plugin, session, operation).isSuccess)
                check(plugin.openSession().isSuccess && f.creates == 1)
                plugin.close()
            }
        }
    }

    @Test fun invalidatingOneControllerLeavesAnotherControllerUsable() {
        MediaCas.Faults.reset()
        CasController().use { first ->
            CasController().use { second ->
                first.updateFromCaMetadata(multipleSessions)
                second.updateFromCaMetadata(multipleSessions)
                MediaCas.Faults.invalidateAt = MediaCas.Operation.EMM
                first.onEmmSection(TsPid(0x010), byteArrayOf(1))
                first.clearForResourceLoss()
                check(second.onEmmSection(TsPid(0x010), byteArrayOf(1)).isEmpty())
                check(second.lastDiagnostic().state == CasController.State.ACTIVE)
                check(MediaCas.Faults.creates == 2 && MediaCas.Faults.pluginCloses == 1)
            }
        }
    }

    private fun invokeOperation(
        plugin: CasController.MediaCasBridge,
        session: CasController.MediaCasSessionBridge,
        operation: MediaCas.Operation,
    ): Result<*> =
        when (operation) {
            MediaCas.Operation.PLUGIN_PRIVATE_DATA -> plugin.setPrivateData(byteArrayOf(1))
            MediaCas.Operation.OPEN -> plugin.openSession()
            MediaCas.Operation.SESSION_PRIVATE_DATA -> session.setPrivateData(byteArrayOf(1))
            MediaCas.Operation.ECM -> session.processEcm(byteArrayOf(1))
            MediaCas.Operation.EMM -> plugin.processEmm(byteArrayOf(1))
            MediaCas.Operation.SESSION_CLOSE -> runCatching { session.close() }
        }

    private class Descrambler : CasController.TunerDescramblerBridge {
        var rejectClose = false
        var closes = 0

        override fun setKeyToken(keyToken: TunerKeyToken) = Result.success(Unit)

        override fun addPid(elementaryPid: TsPid) = Result.success(Unit)

        override fun removePid(elementaryPid: TsPid) = Result.success(Unit)

        override fun close() {
            closes++
            check(!rejectClose) { "Descrambler閉鎖の失敗" }
        }
    }

    @Test fun underlyingSessionAndPluginCloseFailuresReachRealAdaptersAndOwner() {
        val f = MediaCas.Faults
        f.reset()
        val controller = CasController()
        try {
            controller.updateFromCaMetadata(metadata)
            f.sessionFailure = true
            f.pluginFailure = true
            check(runCatching { controller.close() }.isFailure)
            check(f.sessionCloses == 1 && f.pluginCloses == 0)
            f.sessionFailure = false
            check(runCatching { controller.close() }.isFailure)
            check(f.sessionCloses == 2 && f.pluginCloses == 1)
            f.pluginFailure = false
            controller.close()
            check(f.sessionCloses == 2 && f.pluginCloses == 2)
            controller.close()
            check(f.sessionCloses == 2 && f.pluginCloses == 2)
        } finally {
            f.sessionFailure = false
            f.pluginFailure = false
            controller.close()
        }
    }

    @Test fun failedOpenAndPluginRollbackRetainOwnershipBeforeReturningDiagnostic() {
        val f = MediaCas.Faults
        f.reset()
        val controller = CasController()
        try {
            f.openFailure = true
            f.pluginFailure = true
            val result = controller.updateFromCaMetadata(metadata)
            check(result.diagnostics.any { it.errorCode == CasController.ErrorCode.SESSION_OPEN_FAILED })
            check(result.ecmPids.isEmpty() && f.creates == 1 && f.pluginCloses == 1)
            check(runCatching { controller.updateFromCaMetadata(metadata) }.isFailure)
            check(f.creates == 1 && f.pluginCloses == 2 && f.sessionCloses == 0)
            f.pluginFailure = false
            f.openFailure = false
            controller.updateFromCaMetadata(metadata)
            check(f.pluginCloses == 3 && f.creates == 2)
            controller.close()
            check(f.pluginCloses == 4 && f.sessionCloses == 1)
        } finally {
            f.pluginFailure = false
            f.openFailure = false
            controller.close()
        }
    }
}

// 実値の入力・期待値を本体定数と独立に記述する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import android.media.MediaCas
import com.maleicacid.tvinput.aribsi.CaMetadata
import com.maleicacid.tvinput.aribsi.CaMetadataSource
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.TsPid
import com.maleicacid.tvinput.common.TunerKeyToken
import org.junit.Test

class CasPlaybackReadinessTest {
    private val key = ServiceKey(4, 1, 1)
    private val first =
        CaMetadata(
            key,
            5,
            TsPid(0x123),
            null,
            TsPid(0x101),
            source = CaMetadataSource.ELEMENTARY_STREAM,
        )

    private fun controller(typed: Boolean = true): CasController {
        MediaCas.Faults.reset()
        return CasController(
            mediaCasFactory =
                object : CasController.MediaCasBridgeFactory {
                    override fun create(caSystemId: Int) =
                        Result.success(
                            FrameworkMediaCasBridge({ MediaCas(caSystemId) }, typed),
                        )
                },
        )
    }

    @Test fun successfulEcmTransitionsNotifyOnceAndFailureCannotBeErasedByMetadataRefresh() {
        controller().use { cas ->
            val changes = mutableListOf<Pair<Long, CasController.ConnectionChange>>()
            cas.setOnConnectionChanged { generation, change -> changes += generation to change }
            val descrambler = Descrambler()
            cas.updateFromCaMetadata(listOf(first), 7L) { descrambler }
            check(!cas.isServiceDescramblingReady(key, 7L))
            check(cas.onEcmSection(TsPid(0x123), byteArrayOf(1)).isEmpty())
            check(cas.isServiceDescramblingReady(key, 7L))
            check(!cas.isServiceDescramblingReady(key, 6L))
            check(!cas.isServiceDescramblingReady(ServiceKey(4, 1, 2), 7L))
            cas.onEcmSection(TsPid(0x123), byteArrayOf(1))
            check(changes == listOf(7L to CasController.ConnectionChange.KEY_STATE_CHANGED))
            MediaCas.Faults.stateFailureAt = MediaCas.Operation.ECM
            check(cas.onEcmSection(TsPid(0x123), byteArrayOf(1)).isNotEmpty())
            check(!cas.isServiceDescramblingReady(key, 7L))
            cas.updateFromCaMetadata(listOf(first), 7L) { error("existing owner") }
            check(!cas.isServiceDescramblingReady(key, 7L))
            MediaCas.Faults.stateFailureAt = null
            cas.onEcmSection(TsPid(0x123), byteArrayOf(1))
            check(cas.isServiceDescramblingReady(key, 7L) && changes.size == 3)
            cas.clearForResourceLoss()
            check(!cas.isServiceDescramblingReady(key, 7L))
            check(descrambler.tokens.last().contentEquals(byteArrayOf(0)))
        }
    }

    @Test fun everyCurrentSessionAndPidMustBeLinkedBeforeServiceIsReady() {
        controller().use { cas ->
            val second = first.copy(ecmPid = TsPid(0x124), elementaryPid = TsPid(0x102))
            val descramblers = mutableListOf<Descrambler>()
            cas.updateFromCaMetadata(listOf(first, second), 7L) { Descrambler().also { descramblers += it } }
            cas.onEcmSection(TsPid(0x123), byteArrayOf(1))
            check(!cas.isServiceDescramblingReady(key, 7L))
            descramblers[1].rejectAdd = true
            check(cas.onEcmSection(TsPid(0x124), byteArrayOf(1)).isNotEmpty())
            check(!cas.isServiceDescramblingReady(key, 7L))
            descramblers[1].rejectAdd = false
            cas.onEcmSection(TsPid(0x124), byteArrayOf(1))
            check(cas.isServiceDescramblingReady(key, 7L))
            val additional = first.copy(elementaryPid = TsPid(0x103))
            cas.updateFromCaMetadata(listOf(first, second, additional), 7L) { error("existing owner") }
            check(!cas.isServiceDescramblingReady(key, 7L))
            cas.onEcmSection(TsPid(0x123), byteArrayOf(1))
            check(cas.isServiceDescramblingReady(key, 7L))
            cas.onTunerResourcesReclaimed()
            check(!cas.isServiceDescramblingReady(key, 7L))
        }
    }

    @Test fun diagnosticOnlyAndRejectedTokensNeverEnablePlayback() {
        controller(typed = false).use { cas ->
            cas.updateFromCaMetadata(listOf(first), 7L) { Descrambler() }
            cas.onEcmSection(TsPid(0x123), byteArrayOf(1))
            check(!cas.isServiceDescramblingReady(key, 7L))
        }
        controller().use { cas ->
            val descrambler = Descrambler().apply { rejectToken = true }
            cas.updateFromCaMetadata(listOf(first), 7L) { descrambler }
            cas.onEcmSection(TsPid(0x123), byteArrayOf(1))
            check(!cas.isServiceDescramblingReady(key, 7L))
            descrambler.rejectToken = false
            MediaCas.Faults.sessionId = byteArrayOf(0)
            cas.onEcmSection(TsPid(0x123), byteArrayOf(1))
            check(!cas.isServiceDescramblingReady(key, 7L))
        }
    }

    private class Descrambler : CasController.TunerDescramblerBridge {
        val tokens = mutableListOf<ByteArray>()
        var rejectToken = false
        var rejectAdd = false

        override fun setKeyToken(keyToken: TunerKeyToken): Result<Unit> =
            runCatching {
                check(!rejectToken)
                tokens += keyToken.toByteArray()
            }

        override fun addPid(elementaryPid: TsPid): Result<Unit> = runCatching { check(!rejectAdd) }

        override fun removePid(elementaryPid: TsPid) = Result.success(Unit)

        override fun close() = Unit
    }
}

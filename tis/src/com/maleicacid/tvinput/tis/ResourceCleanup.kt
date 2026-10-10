package com.maleicacid.tvinput.tis

import android.util.Log
import com.maleicacid.tvinput.common.LogTags

/** 呼出し元executorが所有し、解放成功まで対象資源と失敗を保持する。 */
internal class ResourceCleanup {
    private data class Pending(
        val name: String,
        val release: () -> Unit,
        val failure: Throwable,
    )

    // 拒否したFilter eventの即時解放失敗だけはcallbackから登録し、再試行は既存ownerが行う。
    // native解放はこのlist lock外で実行し、callbackとownerを相互待機させない。
    private val pending = mutableListOf<Pending>()
    val hasPending: Boolean get() = synchronized(pending) { pending.isNotEmpty() }

    // 境界呼出しの失敗を漏らさず扱い、既存の診断・解放・失敗伝播へ渡す。
    @Suppress("TooGenericExceptionCaught")
    fun release(
        name: String,
        action: () -> Unit,
    ) {
        try {
            action()
        } catch (error: Throwable) {
            Log.w(LogTags.TIS, "資源の解放失敗を再試行まで保持します resource=$name", error)
            synchronized(pending) { pending += Pending(name, action, error) }
        }
    }

    fun retry() {
        val previous = synchronized(pending) { pending.toList().also { pending.clear() } }
        previous.forEach { release(it.name, it.release) }
    }

    fun requireComplete() {
        val remaining = synchronized(pending) { pending.toList() }
        if (remaining.isEmpty()) return
        val error = IllegalStateException("資源の解放が未完了です: ${remaining.joinToString { it.name }}")
        remaining.forEach { if (it.failure !== error) error.addSuppressed(it.failure) }
        throw error
    }
}

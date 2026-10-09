package com.maleicacid.tvinput.tis

import android.util.Log
import com.maleicacid.tvinput.common.LogTags

/** 呼出し元executorが所有し、解放成功まで対象資源と失敗を保持する。 */
internal class ResourceCleanup {
    private data class Pending(
        val name: String,
        val release: () -> Unit,
        val failure: Throwable,
        val owner: Any?,
    )

    // 拒否したFilter eventの即時解放失敗だけはcallbackから登録し、再試行は既存ownerが行う。
    // native解放はこのlist lock外で実行し、callbackとownerを相互待機させない。
    private val pending = mutableListOf<Pending>()
    val hasPending: Boolean get() = synchronized(pending) { pending.isNotEmpty() }

    fun release(
        name: String,
        action: () -> Unit,
    ) = releaseOwned(name, null, action)

    // 境界呼出しの失敗を漏らさず扱い、既存の診断・解放・失敗伝播へ渡す。
    @Suppress("TooGenericExceptionCaught")
    fun releaseOwned(
        name: String,
        owner: Any?,
        action: () -> Unit,
    ): Boolean =
        try {
            action()
            true
        } catch (error: Throwable) {
            Log.w(LogTags.TIS, "資源の解放失敗を再試行まで保持します resource=$name", error)
            synchronized(pending) { pending += Pending(name, action, error, owner) }
            false
        }

    fun retry() {
        val previous = synchronized(pending) { pending.toList() }
        previous.forEach {
            if (synchronized(pending) { pending.remove(it) }) releaseOwned(it.name, it.owner, it.release)
        }
    }

    /** 親資源の解放成功によって失効した子の解放義務だけを完了させる。 */
    fun completeOwnedBy(owner: Any) {
        synchronized(pending) { pending.removeAll { it.owner === owner } }
    }

    fun requireComplete() {
        val remaining = synchronized(pending) { pending.toList() }
        if (remaining.isEmpty()) return
        val error = IllegalStateException("資源の解放が未完了です: ${remaining.joinToString { it.name }}")
        remaining.forEach { if (it.failure !== error) error.addSuppressed(it.failure) }
        throw error
    }
}

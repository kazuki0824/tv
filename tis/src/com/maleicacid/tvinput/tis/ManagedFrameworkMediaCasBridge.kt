package com.maleicacid.tvinput.tis

import android.content.Context
import android.media.MediaCas
import android.os.Handler
import android.os.Looper
import android.os.SystemClock

/** native instanceが遅れて成立しても、元のbridgeがclose完了まで所有する。 */
internal class ManagedFrameworkMediaCasBridge(
    private val context: Context,
    private val caSystemId: Int,
    private val sessionId: String?,
    private val priorityHint: Int,
) : CasController.InitializingMediaCasBridge {
    private val handler = Handler(Looper.getMainLooper())
    private val ownership = Any()
    private var native: MediaCas? = null
    private var constructing = false
    private var closing = false
    private var started = false
    private val adapter =
        FrameworkMediaCasBridge({ requireNotNull(native) { "MediaCas構築が未完了です" } }, typedSession = true)

    override val initializationBudgetMillis: Long = INITIALIZATION_BUDGET_MILLIS

    override fun elapsedRealtime(): Long = SystemClock.elapsedRealtime()

    override fun scheduleTimeout(
        delayMillis: Long,
        action: () -> Unit,
    ): () -> Unit {
        val timeout = Runnable(action)
        check(handler.postDelayed(timeout, delayMillis)) { "CAS初期化期限を登録できません" }
        return { handler.removeCallbacks(timeout) }
    }

    override fun initialize(listener: CasController.ConnectionListener) {
        synchronized(ownership) {
            check(!started && !closing) { "CAS接続を重複初期化できません" }
            started = true
        }
        check(handler.post { construct(listener) }) { "MediaCas構築をHandlerへ登録できません" }
    }

    private fun construct(listener: CasController.ConnectionListener) {
        synchronized(ownership) {
            if (closing) return
            constructing = true
        }
        val result =
            runCatching {
                check(context.getSystemService(Context.TV_TUNER_RESOURCE_MGR_SERVICE) != null) { "TRMを利用できません" }
                MediaCas(
                    context,
                    caSystemId,
                    sessionId,
                    priorityHint,
                    handler,
                    object : MediaCas.EventListener {
                        override fun onEvent(
                            mediaCas: MediaCas,
                            event: Int,
                            arg: Int,
                            data: ByteArray?,
                        ) {
                            // B25/B1では方式固有eventからTISの状態変更を導出しない。
                        }

                        override fun onPluginStatusUpdate(
                            mediaCas: MediaCas,
                            status: Int,
                            arg: Int,
                        ) {
                            if (isCurrent(mediaCas) && status == MediaCas.PLUGIN_STATUS_SESSION_NUMBER_CHANGED) {
                                listener.onCapacity(arg)
                            }
                        }

                        override fun onResourceLost(mediaCas: MediaCas) {
                            if (isCurrent(mediaCas)) listener.onResourceLost()
                        }
                    },
                )
            }
        synchronized(ownership) {
            native = result.getOrNull()
            constructing = false
        }
        // constructorと同じLooper上のstatus配送へ戻る前に、所有確定をcontrollerへpostする。
        listener.onConstructed()
        result.onFailure(listener::onFailure)
    }

    private fun isCurrent(mediaCas: MediaCas): Boolean = synchronized(ownership) { native === mediaCas }

    override fun setPrivateData(privateData: ByteArray): Result<Unit> = adapter.setPrivateData(privateData)

    override fun openSession(): Result<CasController.MediaCasSessionBridge> = adapter.openSession()

    override fun processEmm(section: ByteArray): Result<Unit> = adapter.processEmm(section)

    override fun close() {
        val owned =
            synchronized(ownership) {
                closing = true
                check(!constructing) { "構築中のMediaCasは所有を保持して解放を再試行します" }
                native
            }
        owned?.close()
        synchronized(ownership) { native = null }
    }

    companion object {
        private const val INITIALIZATION_BUDGET_MILLIS = 5_000L
    }
}

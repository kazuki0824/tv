// AOSPのCAS状態コードを本番定数と独立した試験入力として与える。
@file:Suppress("MagicNumber")

package android.media

/** host専用。Androidの資源解放実装ではなく、adapterの例外伝播境界を検査する入力。 */
@Suppress("UNUSED_PARAMETER")
class MediaCas(
    caSystemId: Int,
) : AutoCloseable {
    private var invalidated = false

    init {
        Faults.creates++
    }

    fun setPrivateData(data: ByteArray) = call(Operation.PLUGIN_PRIVATE_DATA)

    fun processEmm(
        data: ByteArray,
        offset: Int,
        length: Int,
    ) = call(Operation.EMM)

    fun openSession(): Session {
        call(Operation.OPEN)
        if (Faults.openFailure) throw UnsupportedOperationException("underlying session open failed")
        return Session()
    }

    fun openSession(
        intent: Int,
        mode: Int,
    ): Session {
        Faults.typedRequests += intent to mode
        return openSession()
    }

    override fun close() {
        Faults.pluginCloses++
        if (Faults.pluginFailure) error("underlying plugin close failed")
    }

    private fun call(operation: Operation) {
        Faults.calls += operation
        if (Faults.invalidateAt == operation) {
            Faults.invalidateAt = null
            invalidated = true
        }
        check(!invalidated) { "Framework接続は無効化済みです" }
        if (Faults.stateFailureAt == operation) MediaCasStateException.throwExceptionIfNeeded(3, "CAS固有状態エラー")
        require(Faults.argumentFailureAt != operation) { "CAS引数エラー" }
    }

    inner class Session : AutoCloseable {
        val sessionId: ByteArray get() = Faults.sessionId.copyOf()

        fun setPrivateData(data: ByteArray) = call(Operation.SESSION_PRIVATE_DATA)

        fun processEcm(
            data: ByteArray,
            offset: Int,
            length: Int,
        ) = call(Operation.ECM)

        override fun close() {
            Faults.sessionCloses++
            call(Operation.SESSION_CLOSE)
            if (Faults.sessionFailure) {
                MediaCasStateException.throwExceptionIfNeeded(3, "underlying session close failed")
            }
        }
    }

    enum class Operation { PLUGIN_PRIVATE_DATA, OPEN, SESSION_PRIVATE_DATA, ECM, EMM, SESSION_CLOSE }

    object Faults {
        var sessionId = byteArrayOf(4, 5, 6)
        val typedRequests = mutableListOf<Pair<Int, Int>>()
        val calls = mutableListOf<Operation>()
        var invalidateAt: Operation? = null
        var stateFailureAt: Operation? = null
        var argumentFailureAt: Operation? = null
        var creates = 0
        var pluginCloses = 0
        var sessionCloses = 0
        var openFailure = false
        var pluginFailure = false
        var sessionFailure = false

        fun reset() {
            sessionId = byteArrayOf(4, 5, 6)
            typedRequests.clear()
            calls.clear()
            invalidateAt = null
            stateFailureAt = null
            argumentFailureAt = null
            creates = 0
            pluginCloses = 0
            sessionCloses = 0
            openFailure = false
            pluginFailure = false
            sessionFailure = false
        }
    }
}

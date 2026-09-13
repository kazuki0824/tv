package android.media

/** host専用。Androidの資源解放実装ではなく、adapterの例外伝播境界を検査する入力。 */
@Suppress("UNUSED_PARAMETER")
class MediaCas(
    caSystemId: Int,
) : AutoCloseable {
    init {
        Faults.creates++
    }

    fun setPrivateData(data: ByteArray) = Unit

    fun processEmm(
        data: ByteArray,
        offset: Int,
        length: Int,
    ) = Unit

    fun openSession(): Session {
        if (Faults.openFailure) error("underlying session open failed")
        return Session()
    }

    override fun close() {
        Faults.pluginCloses++
        if (Faults.pluginFailure) error("underlying plugin close failed")
    }

    class Session : AutoCloseable {
        fun setPrivateData(data: ByteArray) = Unit

        fun processEcm(
            data: ByteArray,
            offset: Int,
            length: Int,
        ) = Unit

        override fun close() {
            Faults.sessionCloses++
            if (Faults.sessionFailure) error("underlying session close failed")
        }
    }

    object Faults {
        var creates = 0
        var pluginCloses = 0
        var sessionCloses = 0
        var openFailure = false
        var pluginFailure = false
        var sessionFailure = false

        fun reset() {
            creates = 0
            pluginCloses = 0
            sessionCloses = 0
            openFailure = false
            pluginFailure = false
            sessionFailure = false
        }
    }
}

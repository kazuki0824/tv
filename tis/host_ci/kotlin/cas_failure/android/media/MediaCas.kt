package android.media

/** host専用。公開例外と、親のclose後はSessionを操作できないAndroidの寿命を検査する入力。 */
@Suppress("UNUSED_PARAMETER")
class MediaCas(
    private val caSystemId: Int,
) : AutoCloseable {
    private var closed = false

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
        Faults.pluginCloseSystems += caSystemId
        if (Faults.pluginFailure) error("underlying plugin close failed")
        closed = true
    }

    inner class Session : AutoCloseable {
        private var sessionClosed = false

        fun setPrivateData(data: ByteArray) = Unit

        fun processEcm(
            data: ByteArray,
            offset: Int,
            length: Int,
        ) = Unit

        override fun close() {
            check(!closed && !sessionClosed) { "session is no longer usable" }
            Faults.sessionCloses++
            if (Faults.sessionFailure && (Faults.failingSessionSystemId == null || Faults.failingSessionSystemId == caSystemId)) {
                error("underlying session close failed")
            }
            sessionClosed = true
        }
    }

    object Faults {
        var creates = 0
        var pluginCloses = 0
        var sessionCloses = 0
        var openFailure = false
        var pluginFailure = false
        var sessionFailure = false
        var failingSessionSystemId: Int? = null
        val pluginCloseSystems = mutableListOf<Int>()

        fun reset() {
            creates = 0
            pluginCloses = 0
            sessionCloses = 0
            openFailure = false
            pluginFailure = false
            sessionFailure = false
            failingSessionSystemId = null
            pluginCloseSystems.clear()
        }
    }
}

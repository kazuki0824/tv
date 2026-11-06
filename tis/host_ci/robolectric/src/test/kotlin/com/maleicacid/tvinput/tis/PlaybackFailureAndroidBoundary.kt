// Android資源の失敗だけを注入し、TIS所有者の初期化は本番constructorを通す。
@file:Suppress("MagicNumber", "UNUSED_PARAMETER")

package com.maleicacid.tvinput.tis

import android.content.Context
import android.media.MediaCas
import android.media.MediaCasStateException
import android.media.MediaCodec
import android.media.tv.tuner.DemuxCapabilities
import android.media.tv.tuner.Tuner
import android.media.tv.tuner.filter.Filter
import android.media.tv.tuner.filter.FilterCallback
import android.media.tv.tuner.filter.FilterConfiguration
import android.media.tv.tuner.filter.FilterEvent
import android.media.tv.tuner.filter.MediaEvent
import android.media.tv.tuner.frontend.FrontendSettings
import org.robolectric.annotation.Implementation
import org.robolectric.annotation.Implements
import org.robolectric.annotation.RealObject
import org.robolectric.shadow.api.Shadow
import org.robolectric.shadows.ShadowMediaCodec
import org.robolectric.util.ReflectionHelpers
import java.util.concurrent.Executor

@Implements(Tuner::class)
class FailureTuner {
    @Implementation(methodName = "__constructor__")
    fun construct(
        context: Context,
        sessionId: String?,
        useCase: Int,
    ) = Unit

    @Implementation
    fun setResourceLostListener(
        executor: Executor,
        listener: Tuner.OnResourceLostListener,
    ) = Unit

    @Implementation
    fun tune(settings: FrontendSettings): Int = 1

    @Implementation
    fun clearOnTuneEventListener() = Unit

    @Implementation
    fun closeFrontend() = Unit

    @Implementation
    fun close() = Unit

    @Implementation
    fun getDemuxCapabilities(): DemuxCapabilities =
        DemuxCapabilities::class.java.declaredConstructors
            .single()
            .apply { isAccessible = true }
            .newInstance(1, 0, 0, 16, sectionFilterCount, 16, 16, 16, 16, 4096L, 0, intArrayOf(), intArrayOf(), false) as DemuxCapabilities

    @Implementation
    fun openFilter(
        type: Int,
        subtype: Int,
        bufferSize: Long,
        executor: Executor,
        callback: FilterCallback,
    ): Filter? {
        openFilterCalls++
        nextFilter?.let { Shadow.extract<FailureFilter>(it).callback = callback }
        return nextFilter
    }

    companion object {
        var nextFilter: Filter? = null
        var openFilterCalls = 0
        var sectionFilterCount = 16

        @JvmStatic
        @Implementation
        fun getTunerVersion(): Int = 0x00030000
    }
}

@Implements(Filter::class)
class FailureFilter {
    @RealObject private lateinit var filter: Filter

    @JvmField var configureFailure = 0

    @JvmField var startFailure = 0

    @JvmField var rejectClose = false

    @JvmField var configurations = 0

    @JvmField var starts = 0

    @JvmField var closes = 0

    @JvmField var sectionPayloads = emptyArray<ByteArray>()

    @JvmField var reads = 0
    var callback: FilterCallback? = null

    fun deliver(events: Array<FilterEvent>): Boolean {
        val target = callback ?: return false
        target.onFilterEvent(filter, events)
        return true
    }

    @Implementation
    fun read(
        buffer: ByteArray,
        offset: Long,
        size: Long,
    ): Int {
        val payload = sectionPayloads[reads++]
        payload.copyInto(buffer, offset.toInt())
        return payload.size
    }

    @Implementation
    fun configure(configuration: FilterConfiguration): Int {
        configurations++
        check(configureFailure != 2) { "injected configure" }
        return configureFailure
    }

    @Implementation
    fun start(): Int {
        starts++
        check(startFailure != 2) { "injected start" }
        return startFailure
    }

    @Implementation
    fun stop(): Int = 0

    @Implementation
    fun close() {
        callback = null
        closes++
        check(!rejectClose) { "injected filter close" }
    }
}

@Implements(MediaEvent::class)
class FailureMediaEvent {
    @JvmField var rejectRead = false

    @JvmField var rejectRelease = false

    @JvmField var releases = 0

    @Implementation
    fun getOffset(): Long {
        check(!rejectRead) { "injected MediaEvent read" }
        return -1L
    }

    @Implementation
    fun getDataLength(): Long = 1L

    @Implementation
    fun isSecureMemory(): Boolean = false

    @Implementation
    fun getLinearBlock(): MediaCodec.LinearBlock? = null

    @Implementation
    fun getPts(): Long = 0L

    @Implementation
    fun release() {
        releases++
        check(!rejectRelease) { "injected MediaEvent release" }
    }
}

@Implements(MediaCodec::class)
class FailureCodec : ShadowMediaCodec() {
    var rejectOutputRelease = false

    @Implementation
    protected override fun releaseOutputBuffer(
        index: Int,
        render: Boolean,
    ) {
        check(!rejectOutputRelease) { "injected codec output release" }
    }
}

@Suppress("UNUSED_PARAMETER")
@Implements(MediaCas::class)
class FailureMediaCas {
    private var invalidated = false

    @RealObject private lateinit var mediaCas: MediaCas

    @Implementation(methodName = "__constructor__")
    fun construct(caSystemId: Int) {
        Faults.creates++
    }

    @Implementation
    fun setPrivateData(data: ByteArray) = call(Operation.PLUGIN_PRIVATE_DATA)

    @Implementation
    fun processEmm(
        data: ByteArray,
        offset: Int,
        length: Int,
    ) = call(Operation.EMM)

    @Implementation
    fun openSession(): MediaCas.Session {
        call(Operation.OPEN)
        if (Faults.openFailure) throw UnsupportedOperationException("underlying session open failed")
        return ReflectionHelpers.callConstructor(
            MediaCas.Session::class.java,
            ReflectionHelpers.ClassParameter.from(MediaCas::class.java, mediaCas),
            ReflectionHelpers.ClassParameter.from(ByteArray::class.java, Faults.sessionId.copyOf()),
        )
    }

    @Implementation
    fun openSession(
        intent: Int,
        mode: Int,
    ): MediaCas.Session {
        Faults.typedRequests += intent to mode
        return openSession()
    }

    @Implementation
    fun close() {
        Faults.pluginCloses++
        if (Faults.pluginFailure) error("underlying plugin close failed")
    }

    fun call(operation: Operation) {
        Faults.calls += operation
        if (Faults.invalidateAt == operation) {
            Faults.invalidateAt = null
            invalidated = true
        }
        check(!invalidated) { "Framework接続は無効化済みです" }
        if (Faults.stateFailureAt == operation) MediaCasStateException.throwExceptionIfNeeded(3, "CAS固有状態エラー")
        require(Faults.argumentFailureAt != operation) { "CAS引数エラー" }
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

@Implements(MediaCas.Session::class)
class FailureCasSession {
    @RealObject private lateinit var session: MediaCas.Session

    private fun owner(): FailureMediaCas = Shadow.extract(ReflectionHelpers.getField<MediaCas>(session, "this$0"))

    @Implementation
    fun getSessionId(): ByteArray = FailureMediaCas.Faults.sessionId.copyOf()

    @Implementation
    fun setPrivateData(data: ByteArray) = owner().call(FailureMediaCas.Operation.SESSION_PRIVATE_DATA)

    @Implementation
    fun processEcm(
        data: ByteArray,
        offset: Int,
        length: Int,
    ) = owner().call(FailureMediaCas.Operation.ECM)

    @Implementation
    fun close() {
        FailureMediaCas.Faults.sessionCloses++
        owner().call(FailureMediaCas.Operation.SESSION_CLOSE)
        if (FailureMediaCas.Faults.sessionFailure) {
            MediaCasStateException.throwExceptionIfNeeded(3, "underlying session close failed")
        }
    }
}

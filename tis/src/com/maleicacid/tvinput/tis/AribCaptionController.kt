package com.maleicacid.tvinput.tis

import android.media.tv.TvTrackInfo
import android.os.Handler
import android.os.Looper
import android.util.Log
import com.maleicacid.tvinput.aribsi.NativeAribCaptionRenderer
import com.maleicacid.tvinput.common.CaptionTimestamp
import com.maleicacid.tvinput.common.LogTags
import java.util.PriorityQueue
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong

// 同じ状態・境界を扱う操作群を一つの所有者に保つ。

/** ARIB字幕/文字スーパーのnative generation、表示時刻、RGBA overlayを直列化する。 */
@Suppress("TooManyFunctions")
class AribCaptionController(
    private val overlayView: CaptionOverlayView,
    private val mediaClock: () -> PlaybackPipeline.MediaClockSnapshot?,
    private val overlayLayerId: String = "caption",
    private val allowNoPts: Boolean = false,
    private val broadcastDeadline: ((AribBroadcastClock.StatementTime, Long?) -> AribBroadcastClock.Deadline?)? = null,
    private val onDiagnostic: (CaptionDiagnostic) -> Unit = { diagnostic ->
        Log.w(LogTags.TIS, "ARIB字幕診断 $diagnostic")
    },
) : AutoCloseable {
    data class CaptionDiagnostic(
        val reason: Reason,
        val playbackGeneration: Long,
        val trackId: String?,
        val count: Int,
    ) {
        enum class Reason {
            NO_AUTHORITATIVE_PTS,
            PRESENTATION_QUEUE_OVERFLOW,
            PRESENTATION_HORIZON_EXCEEDED,
            BROADCAST_TIMED_PES_OVERFLOW,
            OWNER_SUBMISSION_FAILED,
        }
    }

    data class CaptionViewport(
        val overlayWidthPx: Int,
        val overlayHeightPx: Int,
        val contentLeftPx: Int,
        val contentTopPx: Int,
        val contentWidthPx: Int,
        val contentHeightPx: Int,
    )

    private sealed interface Boundary {
        val mediaTimeMillis: Long
        val frameToken: Long

        data class Clear(
            override val mediaTimeMillis: Long,
            override val frameToken: Long,
        ) : Boundary

        data class Display(
            override val mediaTimeMillis: Long,
            override val frameToken: Long,
            val frame: NativeAribCaptionRenderer.RenderedCaptionFrame,
            val viewport: CaptionViewport,
        ) : Boundary
    }

    private val executor =
        LifecycleSerialExecutor(
            "maleicacid-subtitle-$overlayLayerId",
            maxPendingDataTasks = CAPTION_MAX_PENDING_DATA_TASKS,
        )
    private val mainHandler = Handler(Looper.getMainLooper())
    private val released = AtomicBoolean(false)
    private val presentationEpoch = AtomicLong(0L)
    private val boundaries =
        PriorityQueue(
            compareBy<Boundary> { it.mediaTimeMillis }
                .thenBy { it.frameToken }
                .thenBy { if (it is Boundary.Display) 0 else 1 },
        )
    private var enabled = false
    private var selectedTrack: TunerController.TisTrack? = null
    private var playbackGeneration: Long = -1L
    private var overlayWidth: Int = 0
    private var overlayHeight: Int = 0
    private var videoWidth: Int = 0
    private var videoHeight: Int = 0
    private var videoDisplayAspectRatio: Double? = null
    private var videoPathExpected = false
    private var viewport: CaptionViewport? = null
    private var renderer: NativeAribCaptionRenderer? = null
    private var scheduledRunnable: Runnable? = null
    private var nextFrameToken: Long = 0L
    private var displayedFrameToken: Long? = null
    private var noPtsRejectedCount: Int = 0
    private var invalidViewportCount: Int = 0
    private var pendingOverflowCount: Int = 0
    private val broadcastTimedPesScheduler =
        BroadcastTimedPesScheduler(
            resolveDeadline = { statementTime, expectedGeneration ->
                broadcastDeadline?.invoke(statementTime, expectedGeneration)
            },
            currentPlaybackGeneration = { playbackGeneration },
            currentTrackId = { selectedTrack?.id },
            dispatch = { action -> enqueue(action) },
            postDelayed = { runnable, delayMillis ->
                mainHandler.postDelayed(runnable, delayMillis)
            },
            removeCallbacks = { runnable -> mainHandler.removeCallbacks(runnable) },
            onDue = { trackId, pesData ->
                decodePesOnExecutor(trackId, pesData, CaptionTimestamp.NoPts, forceImmediate = true)
            },
            onDrop = { recordPendingOverflow(CaptionDiagnostic.Reason.BROADCAST_TIMED_PES_OVERFLOW) },
        )

    init {
        overlayView.setOnOverlaySizeChangedListener(overlayLayerId) { width, height ->
            enqueue { updateOverlaySize(width, height) }
        }
    }

    private fun <T> runBlocking(
        cleanup: Boolean = false,
        action: () -> T,
    ): T =
        try {
            executor.callControl(CAPTION_CONTROL_WAIT_MS, cleanup = cleanup) {
                check(cleanup || !released.get()) { "解放中のownerへ通常controlを実行できません" }
                action()
            }
        } catch (error: ControlResultUnknownException) {
            released.set(true)
            executor.executeCleanupControl {
                runCatching { close() }.onFailure {
                    Log.w(LogTags.TIS, "結果未確定controlの後片付けを再試行まで保持します", it)
                }
            }
            throw error
        }

    private fun enqueue(action: () -> Unit) {
        executor.executeCallback(isReleased = released::get, onFailure = ::handleSubmissionFailure, action = action)
    }

    private fun enqueueControl(action: () -> Unit) {
        executor.executeCallback(
            control = true,
            isReleased = released::get,
            onFailure = ::handleSubmissionFailure,
            action = action,
        )
    }

    // 診断失敗より解放失敗を優先して記録し、未完解放は同じownerで再試行するためuseへ変換しない。
    @Suppress("ConvertTryFinallyToUseCall")
    private fun handleSubmissionFailure(error: RuntimeException) {
        if (!released.compareAndSet(false, true)) return
        Log.w(LogTags.TIS, "caption owner投入失敗: 同じownerで解放します", error)
        executor.executeTerminalCleanup {
            runCatching {
                try {
                    onDiagnostic(
                        CaptionDiagnostic(
                            CaptionDiagnostic.Reason.OWNER_SUBMISSION_FAILED,
                            playbackGeneration,
                            selectedTrack?.id,
                            1,
                        ),
                    )
                } finally {
                    close()
                }
            }.onFailure { Log.w(LogTags.TIS, "投入失敗の後片付けを再試行まで保持します", it) }
        }
    }

    fun setEnabled(value: Boolean) =
        enqueueControl {
            if (enabled == value) return@enqueueControl
            enabled = value
            restartPresentation()
        }

    fun selectTrack(track: TunerController.TisTrack?) =
        enqueueControl {
            val normalized = track?.takeIf { it.type == TvTrackInfo.TYPE_SUBTITLE }
            if (normalized?.id == selectedTrack?.id) return@enqueueControl
            selectedTrack = normalized
            restartPresentation()
        }

    fun beginPlaybackGeneration(
        generation: Long,
        hasVideo: Boolean,
    ) = enqueue {
        if (playbackGeneration == generation && videoPathExpected == hasVideo) return@enqueue
        playbackGeneration = generation
        noPtsRejectedCount = 0
        videoPathExpected = hasVideo
        videoWidth = 0
        videoHeight = 0
        videoDisplayAspectRatio = null
        viewport = null
        restartPresentation()
    }

    fun updateVideoGeometry(
        generation: Long,
        width: Int,
        height: Int,
        displayAspectRatio: Double? = null,
    ) = enqueue {
        val invalidVideoOutput =
            generation != playbackGeneration || !videoPathExpected || width <= 0 || height <= 0
        if (invalidVideoOutput) return@enqueue
        videoWidth = width
        videoHeight = height
        videoDisplayAspectRatio = displayAspectRatio?.takeIf { it.isFinite() && it > 0.0 }
        updateViewport()
    }

    fun onPlaybackClockChanged() = enqueue { armNextBoundary() }

    fun onBroadcastTimedPesData(
        trackId: String,
        pesData: ByteArray,
        statementTime: AribBroadcastClock.StatementTime,
    ) = enqueue {
        if (!allowNoPts || broadcastDeadline == null || selectedTrack?.id != trackId) return@enqueue
        broadcastTimedPesScheduler.submit(trackId, pesData, statementTime)
    }

    fun onBroadcastClockChanged() =
        enqueue {
            broadcastTimedPesScheduler.onClockChanged()
        }

    fun onPesData(
        trackId: String,
        pesData: ByteArray,
        timestamp: CaptionTimestamp,
    ) = enqueue {
        decodePesOnExecutor(trackId, pesData, timestamp, forceImmediate = false)
    }

    // 入力拒否・未準備・失敗を発生点で返し、成功経路を深い入れ子にしない。
    @Suppress("ReturnCount")
    private fun decodePesOnExecutor(
        trackId: String,
        pesData: ByteArray,
        timestamp: CaptionTimestamp,
        forceImmediate: Boolean,
    ) {
        val track = selectedTrack ?: return
        if (!enabled || track.id != trackId) return
        if (!allowNoPts && timestamp == CaptionTimestamp.NoPts) {
            recordNoPtsRejected(trackId)
            return
        }
        val currentViewport = viewport ?: return
        val currentRenderer = renderer ?: return
        when (
            val decoded =
                runCatching { currentRenderer.decodePes(pesData, timestamp) }
                    .onFailure { error -> Log.w(LogTags.TIS, "ARIB字幕PES処理に失敗しました trackId=$trackId", error) }
                    .getOrNull() ?: return
        ) {
            NativeAribCaptionRenderer.DecodeResult.NoPtsRejected -> {
                recordNoPtsRejected(trackId)
            }

            NativeAribCaptionRenderer.DecodeResult.NoOutput -> {
                // 描画する字幕がない。
            }

            is NativeAribCaptionRenderer.DecodeResult.Rendered -> {
                val frame = if (forceImmediate) decoded.frame.copy(ptsMillis = null) else decoded.frame
                enqueueFrame(frame, currentViewport)
            }
        }
    }

    fun flushForSubtitleContinuityLoss() = enqueueControl { restartPresentation() }

    private fun recordNoPtsRejected(trackId: String?) {
        noPtsRejectedCount++
        onDiagnostic(
            CaptionDiagnostic(
                reason = CaptionDiagnostic.Reason.NO_AUTHORITATIVE_PTS,
                playbackGeneration = playbackGeneration,
                trackId = trackId,
                count = noPtsRejectedCount,
            ),
        )
    }

    private fun updateOverlaySize(
        width: Int,
        height: Int,
    ) {
        if (width == overlayWidth && height == overlayHeight) return
        overlayWidth = width.coerceAtLeast(0)
        overlayHeight = height.coerceAtLeast(0)
        updateViewport()
    }

    // 入力拒否・未準備・失敗を発生点で返し、成功経路を深い入れ子にしない。
    @Suppress("ReturnCount")
    private fun updateViewport() {
        val next =
            calculateViewport(
                overlayWidth,
                overlayHeight,
                videoWidth,
                videoHeight,
                playbackGeneration,
                videoDisplayAspectRatio,
            )
        if (next == viewport) return
        cancelScheduledBoundary()
        displayedFrameToken = null
        postClear()
        viewport = next
        if (next == null) {
            invalidViewportCount++
            // 一時的な未確定viewportでdecoder continuityを破棄しない。
            return
        }
        val existing = renderer
        if (existing != null && !existing.setViewport(next.contentWidthPx, next.contentHeightPx)) {
            viewport = null
            invalidViewportCount++
            return
        }
        ensureRenderer()
        val currentFrame = renderCurrentFrame()
        if (currentFrame != null) {
            enqueueFrame(currentFrame, next)
        } else {
            armNextBoundary()
        }
    }

    // 入力拒否・未準備・失敗を発生点で返し、成功経路を深い入れ子にしない。
    @Suppress("ReturnCount")
    private fun renderCurrentFrame(): NativeAribCaptionRenderer.RenderedCaptionFrame? {
        if (!enabled || !videoPathExpected || viewport == null) return null
        val clock = mediaClock() ?: return null
        return runCatching { renderer?.renderAt(currentMediaMillis(clock)) }
            .onFailure { error -> Log.w(LogTags.TIS, "字幕の現在画像を再描画できません", error) }
            .getOrNull()
    }

    private fun restartPresentation() {
        cancelScheduledBoundary()
        broadcastTimedPesScheduler.cancelAll()
        boundaries.clear()
        displayedFrameToken = null
        renderer?.flush()
        renderer?.close()
        renderer = null
        postClear()
        ensureRenderer()
    }

    // 入力拒否・未準備・失敗を発生点で返し、成功経路を深い入れ子にしない。
    @Suppress("ReturnCount")
    private fun ensureRenderer() {
        if (!enabled || !videoPathExpected) return
        val track = selectedTrack ?: return
        val currentViewport = viewport ?: return
        if (renderer != null) return
        val created =
            NativeAribCaptionRenderer(
                dataComponentId = track.dataComponentId ?: ARIB_PROFILE_A_COMPONENT_ID,
                superimpose = track.captionServiceKind == "superimpose",
                languageId = track.captionLanguageId ?: 1,
            )
        if (!created.setViewport(currentViewport.contentWidthPx, currentViewport.contentHeightPx)) {
            created.close()
            invalidViewportCount++
            return
        }
        renderer = created
    }

    @Suppress("ReturnCount")
    private fun enqueueFrame(
        frame: NativeAribCaptionRenderer.RenderedCaptionFrame,
        currentViewport: CaptionViewport,
    ) {
        val pts = frame.ptsMillis
        if (pts == null) {
            if (!allowNoPts) {
                recordNoPtsRejected(selectedTrack?.id)
                return
            }
            displayImmediate(frame, currentViewport)
            return
        }
        val timing =
            captionTimingWithinHorizon(
                pts = pts,
                durationMillis = frame.durationMillis,
                nowMediaMillis = mediaClock()?.let(::currentMediaMillis),
            )
        if (timing == null) {
            recordPendingOverflow(CaptionDiagnostic.Reason.PRESENTATION_HORIZON_EXCEEDED)
            return
        }
        val clearAt = timing.clearAt
        val replaced = boundaries.filter { boundary -> boundary.mediaTimeMillis == pts && boundary is Boundary.Display }
        val remainingCount = boundaries.size - replaced.size
        val incomingCount = if (clearAt == null) 1 else 2
        val queuedBytes =
            boundaries
                .asSequence()
                .filterIsInstance<Boundary.Display>()
                .filterNot { it in replaced }
                .sumOf { boundary -> boundary.frame.images.sumOf { image -> image.rgba8888.size.toLong() } }
        val incomingBytes = frame.images.sumOf { image -> image.rgba8888.size.toLong() }
        if (
            incomingBytes > CAPTION_MAX_PENDING_BYTES ||
            remainingCount > CAPTION_MAX_BOUNDARIES - incomingCount ||
            queuedBytes > CAPTION_MAX_PENDING_BYTES - incomingBytes
        ) {
            recordPendingOverflow(CaptionDiagnostic.Reason.PRESENTATION_QUEUE_OVERFLOW)
            return
        }
        val token = ++nextFrameToken
        boundaries += Boundary.Display(pts, token, frame, currentViewport)
        clearAt?.let { boundaries += Boundary.Clear(it, token) }
        armNextBoundary()
    }

    private data class CaptionTiming(
        val clearAt: Long?,
    )

    private fun captionTimingWithinHorizon(
        pts: Long,
        durationMillis: Long?,
        nowMediaMillis: Long?,
    ): CaptionTiming? {
        val clearAt = durationMillis?.let { duration -> pts.checkedAdd(duration) }
        val clearBoundaryInvalid = durationMillis != null && clearAt == null
        val outsideHorizon =
            nowMediaMillis?.let { now ->
                listOfNotNull(pts, clearAt).any { boundary ->
                    val delta = runCatching { Math.subtractExact(boundary, now) }.getOrNull()
                    delta == null || delta > CAPTION_MAX_FUTURE_HORIZON_MS
                }
            } ?: false
        return if (clearBoundaryInvalid || outsideHorizon) null else CaptionTiming(clearAt)
    }

    private fun recordPendingOverflow(reason: CaptionDiagnostic.Reason) {
        pendingOverflowCount++
        onDiagnostic(
            CaptionDiagnostic(
                reason = reason,
                playbackGeneration = playbackGeneration,
                trackId = selectedTrack?.id,
                count = pendingOverflowCount,
            ),
        )
    }

    private fun displayImmediate(
        frame: NativeAribCaptionRenderer.RenderedCaptionFrame,
        currentViewport: CaptionViewport,
    ) {
        cancelScheduledBoundary()
        boundaries.clear()
        val token = ++nextFrameToken
        displayedFrameToken = token
        postFrame(frame, currentViewport)
        frame.durationMillis?.let { duration ->
            val epochAtArm = presentationEpoch.get()
            val runnable =
                Runnable {
                    enqueue {
                        if (epochAtArm != presentationEpoch.get() || displayedFrameToken != token) return@enqueue
                        scheduledRunnable = null
                        displayedFrameToken = null
                        postClear()
                    }
                }
            scheduledRunnable = runnable
            mainHandler.postDelayed(runnable, duration.coerceAtLeast(0L))
        }
    }

    // 入力拒否・未準備・失敗を発生点で返し、成功経路を深い入れ子にしない。
    @Suppress("ReturnCount")
    private fun armNextBoundary() {
        cancelScheduledBoundary()
        while (true) {
            val boundary = boundaries.peek() ?: return
            val snapshot = mediaClock() ?: return
            val nowMediaMillis = currentMediaMillis(snapshot)
            val remainingMediaMillis = boundary.mediaTimeMillis - nowMediaMillis
            if (remainingMediaMillis <= 0L) {
                boundaries.poll()
                applyBoundary(boundary)
                continue
            }
            if (snapshot.clockRate <= 0.0f) return
            val delayMillis =
                kotlin.math
                    .ceil(remainingMediaMillis / snapshot.clockRate.toDouble())
                    .toLong()
                    .coerceAtLeast(1L)
            val epochAtArm = presentationEpoch.get()
            val runnable =
                Runnable {
                    enqueue {
                        if (epochAtArm != presentationEpoch.get()) return@enqueue
                        scheduledRunnable = null
                        armNextBoundary()
                    }
                }
            scheduledRunnable = runnable
            mainHandler.postDelayed(runnable, delayMillis)
            return
        }
    }

    private fun applyBoundary(boundary: Boundary) {
        when (boundary) {
            is Boundary.Clear -> {
                if (displayedFrameToken == boundary.frameToken) {
                    displayedFrameToken = null
                    postClear()
                }
            }

            is Boundary.Display -> {
                val currentViewport = viewport ?: return
                if (boundary.viewport != currentViewport) {
                    renderCurrentFrame()?.let { enqueueFrame(it, currentViewport) }
                    return
                }
                displayedFrameToken = boundary.frameToken
                postFrame(boundary.frame, currentViewport)
            }
        }
    }

    private fun postFrame(
        frame: NativeAribCaptionRenderer.RenderedCaptionFrame,
        frameViewport: CaptionViewport,
    ) {
        val epoch = presentationEpoch.get()
        mainHandler.post {
            if (presentationEpoch.get() != epoch) return@post
            if (frame.images.isEmpty()) {
                overlayView.clearCaptionLayer(overlayLayerId)
            } else if (!overlayView.showCaptionFrame(
                    overlayLayerId,
                    frame.images,
                    frameViewport.contentLeftPx,
                    frameViewport.contentTopPx,
                )
            ) {
                overlayView.clearCaptionLayer(overlayLayerId)
            }
        }
    }

    private fun cancelScheduledBoundary() {
        scheduledRunnable?.let(mainHandler::removeCallbacks)
        scheduledRunnable = null
    }

    private fun postClear() {
        val epoch = presentationEpoch.incrementAndGet()
        mainHandler.post { if (presentationEpoch.get() == epoch) overlayView.clearCaptionLayer(overlayLayerId) }
    }

    override fun close() {
        if (executor.isShutdown) return
        released.set(true)
        runBlocking(cleanup = true) {
            SectionFilterPolicy.completeCleanup(
                { cancelScheduledBoundary() },
                { broadcastTimedPesScheduler.cancelAll() },
                { boundaries.clear() },
                { renderer?.flush() },
                {
                    renderer?.close()
                    renderer = null
                },
                { postClear() },
            )
        }
        executor.shutdownNow()
    }

    companion object {
        private const val ARIB_PROFILE_A_COMPONENT_ID = 0x0008
        private const val CAPTION_CONTROL_WAIT_MS = 2_000L
        private const val CAPTION_MAX_PENDING_DATA_TASKS = 64
        private const val CAPTION_MAX_BOUNDARIES = 64
        private const val CAPTION_MAX_PENDING_BYTES = 8L * 1024L * 1024L
        private const val CAPTION_MAX_FUTURE_HORIZON_MS = 60_000L

        fun shouldDrawCaptionForTest(
            enabled: Boolean,
            selectedTrackId: String?,
            incomingTrackId: String,
        ): Boolean = enabled && selectedTrackId != null && selectedTrackId == incomingTrackId

        // 独立した既存入力を明示し、引数数だけを理由に別の状態保持型を導入しない。
        @Suppress("LongParameterList")
        fun calculateViewport(
            overlayWidth: Int,
            overlayHeight: Int,
            videoWidth: Int,
            videoHeight: Int,
            generation: Long,
            displayAspectRatio: Double? = null,
        ): CaptionViewport? {
            val invalidViewportInput =
                overlayWidth <= 0 || overlayHeight <= 0 || videoWidth <= 0 || videoHeight <= 0 || generation < 0L
            if (invalidViewportInput) return null
            val overlayAspect = overlayWidth.toDouble() / overlayHeight.toDouble()
            val videoAspect =
                displayAspectRatio?.takeIf { it.isFinite() && it > 0.0 }
                    ?: (videoWidth.toDouble() / videoHeight.toDouble())
            return if (overlayAspect > videoAspect) {
                val contentWidth = (overlayHeight * videoAspect).toInt().coerceAtLeast(1)
                CaptionViewport(
                    overlayWidth,
                    overlayHeight,
                    (overlayWidth - contentWidth) / 2,
                    0,
                    contentWidth,
                    overlayHeight,
                )
            } else {
                val contentHeight = (overlayWidth / videoAspect).toInt().coerceAtLeast(1)
                CaptionViewport(
                    overlayWidth,
                    overlayHeight,
                    0,
                    (overlayHeight - contentHeight) / 2,
                    overlayWidth,
                    contentHeight,
                )
            }
        }

        // この処理の規格値・ビット幅・単位換算・固定上限をリテラルのまま照合できる形に保つ。
        @Suppress("MagicNumber")
        fun currentMediaMillis(snapshot: PlaybackPipeline.MediaClockSnapshot): Long {
            val elapsedNanos = (System.nanoTime() - snapshot.nanoTime).coerceAtLeast(0L)
            return snapshot.mediaTimeUs / 1_000L +
                ((elapsedNanos / 1_000_000.0) * snapshot.clockRate.toDouble()).toLong()
        }

        private fun Long.checkedAdd(other: Long): Long? = runCatching { Math.addExact(this, other) }.getOrNull()
    }
}

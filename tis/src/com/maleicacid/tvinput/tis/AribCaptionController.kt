package com.maleicacid.tvinput.tis

import android.media.tv.TvTrackInfo
import android.os.Handler
import android.os.Looper
import android.util.Log
import com.maleicacid.tvinput.aribsi.NativeAribCaptionRenderer
import com.maleicacid.tvinput.common.CaptionTimestamp
import com.maleicacid.tvinput.common.LogTags
import java.util.PriorityQueue
import java.util.concurrent.Callable
import java.util.concurrent.ExecutionException
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong

/** ARIB字幕/文字スーパーのnative generation、表示時刻、RGBA overlayを直列化する。 */
class AribCaptionController(
    private val overlayView: CaptionOverlayView,
    private val mediaClock: () -> PlaybackPipeline.MediaClockSnapshot?,
    private val overlayLayerId: String = "caption",
    private val allowNoPts: Boolean = false,
    private val broadcastDeadline: ((AribBroadcastClock.StatementTime, Long?) -> AribBroadcastClock.Deadline?)? = null,
) : AutoCloseable {
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

    @Volatile private var executorThread: Thread? = null
    private val executor: ExecutorService = Executors.newSingleThreadExecutor { runnable ->
        Thread(runnable, "maleicacid-subtitle-$overlayLayerId").also { thread ->
            thread.isDaemon = true
            executorThread = thread
        }
    }
    private val mainHandler = Handler(Looper.getMainLooper())
    private val released = AtomicBoolean(false)
    private val presentationEpoch = AtomicLong(0L)
    private val boundaries = PriorityQueue<Boundary>(
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
    private val broadcastTimedPesScheduler = BroadcastTimedPesScheduler(
        resolveDeadline = { statementTime, expectedGeneration ->
            broadcastDeadline?.invoke(statementTime, expectedGeneration)
        },
        currentPlaybackGeneration = { playbackGeneration },
        currentTrackId = { selectedTrack?.id },
        dispatch = { action -> enqueue(action) },
        postDelayed = { runnable, delayMillis ->
            mainHandler.postDelayed(runnable, delayMillis)
            Unit
        },
        removeCallbacks = { runnable -> mainHandler.removeCallbacks(runnable) },
        onDue = { trackId, pesData ->
            decodePesOnExecutor(trackId, pesData, CaptionTimestamp.NoPts, forceImmediate = true)
        },
    )

    init {
        overlayView.setOnOverlaySizeChangedListener(overlayLayerId) { width, height ->
            enqueue { updateOverlaySize(width, height) }
        }
    }

    private fun <T> runBlocking(action: () -> T): T {
        if (Thread.currentThread() == executorThread) return action()
        val future = executor.submit(Callable<T> { action() })
        return try {
            future.get()
        } catch (error: InterruptedException) {
            Thread.currentThread().interrupt()
            throw RuntimeException("subtitle executor interrupted", error)
        } catch (error: ExecutionException) {
            throw RuntimeException(error.cause ?: error)
        }
    }

    private fun enqueue(action: () -> Unit) {
        if (released.get()) return
        if (Thread.currentThread() == executorThread) {
            action()
        } else {
            runCatching { executor.execute { if (!released.get()) action() } }
        }
    }

    fun setEnabled(value: Boolean) = enqueue {
        if (enabled == value) return@enqueue
        enabled = value
        restartPresentation()
    }

    fun selectTrack(track: TunerController.TisTrack?) = enqueue {
        val normalized = track?.takeIf { it.type == TvTrackInfo.TYPE_SUBTITLE }
        if (normalized?.id == selectedTrack?.id) return@enqueue
        selectedTrack = normalized
        restartPresentation()
    }

    fun beginPlaybackGeneration(generation: Long, hasVideo: Boolean) = enqueue {
        if (playbackGeneration == generation && videoPathExpected == hasVideo) return@enqueue
        playbackGeneration = generation
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
        if (generation != playbackGeneration || !videoPathExpected || width <= 0 || height <= 0) return@enqueue
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

    fun onBroadcastClockChanged() = enqueue {
        broadcastTimedPesScheduler.onClockChanged()
    }

    fun onPesData(trackId: String, pesData: ByteArray, timestamp: CaptionTimestamp) = enqueue {
        decodePesOnExecutor(trackId, pesData, timestamp, forceImmediate = false)
    }

    private fun decodePesOnExecutor(
        trackId: String,
        pesData: ByteArray,
        timestamp: CaptionTimestamp,
        forceImmediate: Boolean,
    ) {
        val track = selectedTrack ?: return
        val currentViewport = viewport ?: return
        val currentRenderer = renderer ?: return
        if (!enabled || track.id != trackId) return
        when (val decoded = runCatching { currentRenderer.decodePes(pesData, timestamp) }
            .onFailure { error -> Log.w(LogTags.TIS, "ARIB字幕PES処理に失敗しました trackId=$trackId", error) }
            .getOrNull() ?: return) {
            NativeAribCaptionRenderer.DecodeResult.NoPtsRejected -> {
                noPtsRejectedCount++
                Log.w(LogTags.TIS, "この字幕serviceではauthoritative PTSなしPESを受理しません count=$noPtsRejectedCount")
            }
            NativeAribCaptionRenderer.DecodeResult.NoOutput -> Unit
            is NativeAribCaptionRenderer.DecodeResult.Rendered -> {
                val frame = if (forceImmediate) decoded.frame.copy(ptsMillis = null) else decoded.frame
                enqueueFrame(frame, currentViewport)
            }
        }
    }

    fun flushForSubtitleContinuityLoss() = enqueue { restartPresentation() }

    fun noPtsRejectedCountForDiagnostic(): Int = runBlocking { noPtsRejectedCount }
    fun invalidViewportCountForDiagnostic(): Int = runBlocking { invalidViewportCount }

    private fun updateOverlaySize(width: Int, height: Int) {
        if (width == overlayWidth && height == overlayHeight) return
        overlayWidth = width.coerceAtLeast(0)
        overlayHeight = height.coerceAtLeast(0)
        updateViewport()
    }

    private fun updateViewport() {
        val next = calculateViewport(
            overlayWidth,
            overlayHeight,
            videoWidth,
            videoHeight,
            playbackGeneration,
            videoDisplayAspectRatio,
        )
        if (next == viewport) return
        cancelScheduledBoundary()
        boundaries.clear()
        displayedFrameToken = null
        postClear()
        viewport = next
        if (next == null) {
            invalidViewportCount++
            renderer?.close()
            renderer = null
            return
        }
        val existing = renderer
        if (existing != null && !existing.setViewport(next.contentWidthPx, next.contentHeightPx)) {
            existing.close()
            renderer = null
        }
        ensureRenderer()
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

    private fun ensureRenderer() {
        if (!enabled || !videoPathExpected) return
        val track = selectedTrack ?: return
        val currentViewport = viewport ?: return
        if (renderer != null) return
        val created = NativeAribCaptionRenderer(
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

    private fun enqueueFrame(
        frame: NativeAribCaptionRenderer.RenderedCaptionFrame,
        currentViewport: CaptionViewport,
    ) {
        val pts = frame.ptsMillis
        if (pts == null) {
            if (!allowNoPts) {
                noPtsRejectedCount++
                return
            }
            displayImmediate(frame, currentViewport)
            return
        }
        val token = ++nextFrameToken
        boundaries.removeIf { boundary -> boundary.mediaTimeMillis == pts && boundary is Boundary.Display }
        boundaries += Boundary.Display(pts, token, frame, currentViewport)
        frame.durationMillis?.let { duration ->
            val clearAt = pts.checkedAdd(duration) ?: return@let
            boundaries += Boundary.Clear(clearAt, token)
        }
        armNextBoundary()
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
            val runnable = Runnable {
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

    private fun armNextBoundary() {
        cancelScheduledBoundary()
        while (true) {
            val boundary = boundaries.peek() ?: return
            val snapshot = mediaClock() ?: return
            if (snapshot.clockRate <= 0.0f) return
            val nowMediaMillis = currentMediaMillis(snapshot)
            val remainingMediaMillis = boundary.mediaTimeMillis - nowMediaMillis
            if (remainingMediaMillis <= 0L) {
                boundaries.poll()
                applyBoundary(boundary)
                continue
            }
            val delayMillis = kotlin.math.ceil(remainingMediaMillis / snapshot.clockRate.toDouble()).toLong()
                .coerceAtLeast(1L)
            val epochAtArm = presentationEpoch.get()
            val runnable = Runnable {
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
                if (boundary.viewport != viewport) return
                displayedFrameToken = boundary.frameToken
                postFrame(boundary.frame, boundary.viewport)
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
                )) {
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
        if (!released.compareAndSet(false, true)) return
        runBlocking {
            cancelScheduledBoundary()
            broadcastTimedPesScheduler.cancelAll()
            boundaries.clear()
            renderer?.flush()
            renderer?.close()
            renderer = null
            postClear()
        }
        executor.shutdownNow()
    }

    companion object {
        private const val ARIB_PROFILE_A_COMPONENT_ID = 0x0008

        fun shouldDrawCaptionForTest(enabled: Boolean, selectedTrackId: String?, incomingTrackId: String): Boolean =
            enabled && selectedTrackId != null && selectedTrackId == incomingTrackId

        fun calculateViewport(
            overlayWidth: Int,
            overlayHeight: Int,
            videoWidth: Int,
            videoHeight: Int,
            generation: Long,
            displayAspectRatio: Double? = null,
        ): CaptionViewport? {
            if (overlayWidth <= 0 || overlayHeight <= 0 || videoWidth <= 0 || videoHeight <= 0 || generation < 0L) return null
            val overlayAspect = overlayWidth.toDouble() / overlayHeight.toDouble()
            val videoAspect = displayAspectRatio?.takeIf { it.isFinite() && it > 0.0 }
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

        fun currentMediaMillis(snapshot: PlaybackPipeline.MediaClockSnapshot): Long {
            val elapsedNanos = (System.nanoTime() - snapshot.nanoTime).coerceAtLeast(0L)
            return snapshot.mediaTimeUs / 1_000L +
                ((elapsedNanos / 1_000_000.0) * snapshot.clockRate.toDouble()).toLong()
        }

        private fun Long.checkedAdd(other: Long): Long? =
            runCatching { Math.addExact(this, other) }.getOrNull()
    }
}

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

/** ARIB字幕のnative generation、MediaSync時刻、RGBA overlayを直列化する。 */
class AribCaptionController(
    private val overlayView: CaptionOverlayView,
    private val mediaClock: () -> PlaybackPipeline.MediaClockSnapshot?,
) : AutoCloseable {
    data class CaptionViewport(
        val overlayWidthPx: Int,
        val overlayHeightPx: Int,
        val contentLeftPx: Int,
        val contentTopPx: Int,
        val contentWidthPx: Int,
        val contentHeightPx: Int,
        val generationToken: Long,
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
        Thread(runnable, "maleicacid-subtitle").also { thread ->
            thread.isDaemon = true
            executorThread = thread
        }
    }
    private val mainHandler = Handler(Looper.getMainLooper())
    private val released = AtomicBoolean(false)
    private val uiEpoch = AtomicLong(0L)
    private val boundaries = PriorityQueue<Boundary>(
        compareBy<Boundary> { it.mediaTimeMillis }
            .thenBy { it.frameToken }
            .thenBy { if (it is Boundary.Display) 0 else 1 },
    )
    private var enabled = false
    private var selectedTrack: TunerController.TisTrack? = null
    private var playbackGeneration: Long = -1L
    private var subtitleGeneration: Long = 0L
    private var overlayWidth: Int = 0
    private var overlayHeight: Int = 0
    private var videoWidth: Int = 0
    private var videoHeight: Int = 0
    private var videoPathExpected = false
    private var viewport: CaptionViewport? = null
    private var renderer: NativeAribCaptionRenderer? = null
    private var scheduledRunnable: Runnable? = null
    private var nextFrameToken: Long = 0L
    private var displayedFrameToken: Long? = null
    private var noPtsRejectedCount: Int = 0
    private var invalidViewportCount: Int = 0

    init {
        overlayView.setOnOverlaySizeChangedListener { width, height ->
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
        restartSubtitleGeneration()
    }

    fun selectTrack(track: TunerController.TisTrack?) = enqueue {
        val normalized = track?.takeIf { it.type == TvTrackInfo.TYPE_SUBTITLE }
        if (normalized?.id == selectedTrack?.id) return@enqueue
        selectedTrack = normalized
        restartSubtitleGeneration()
    }

    fun beginPlaybackGeneration(generation: Long, hasVideo: Boolean) = enqueue {
        if (playbackGeneration == generation && videoPathExpected == hasVideo) return@enqueue
        playbackGeneration = generation
        videoPathExpected = hasVideo
        videoWidth = 0
        videoHeight = 0
        viewport = null
        restartSubtitleGeneration()
    }

    fun updateVideoGeometry(generation: Long, width: Int, height: Int) = enqueue {
        if (generation != playbackGeneration || !videoPathExpected || width <= 0 || height <= 0) return@enqueue
        videoWidth = width
        videoHeight = height
        updateViewport()
    }

    fun onPlaybackClockChanged() = enqueue { armNextBoundary() }

    fun onPesData(trackId: String, pesData: ByteArray, timestamp: CaptionTimestamp) = enqueue {
        val track = selectedTrack ?: return@enqueue
        val currentViewport = viewport ?: return@enqueue
        val currentRenderer = renderer ?: return@enqueue
        if (!enabled || track.id != trackId || currentViewport.generationToken != playbackGeneration) return@enqueue
        when (val decoded = runCatching { currentRenderer.decodePes(pesData, timestamp) }
            .onFailure { error -> Log.w(LogTags.TIS, "ARIB字幕PES処理に失敗しました trackId=$trackId", error) }
            .getOrNull() ?: return@enqueue) {
            NativeAribCaptionRenderer.DecodeResult.NoPtsRejected -> {
                noPtsRejectedCount++
                Log.w(LogTags.TIS, "authoritative PTSのない字幕をrenderer queueへ追加しません count=$noPtsRejectedCount")
            }
            NativeAribCaptionRenderer.DecodeResult.NoOutput -> Unit
            is NativeAribCaptionRenderer.DecodeResult.Rendered -> enqueueFrame(decoded.frame, currentViewport)
        }
    }

    fun flushForSubtitleContinuityLoss() = enqueue { restartSubtitleGeneration() }

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

    private fun restartSubtitleGeneration() {
        subtitleGeneration++
        cancelScheduledBoundary()
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
        if (currentViewport.generationToken != playbackGeneration) return
        if (renderer != null) return
        val created = NativeAribCaptionRenderer(
            dataComponentId = track.dataComponentId ?: ARIB_PROFILE_A_COMPONENT_ID,
            superimpose = track.captionServiceKind == "superimpose",
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
        val token = ++nextFrameToken
        boundaries.removeIf { boundary -> boundary.mediaTimeMillis == frame.ptsMillis && boundary is Boundary.Display }
        boundaries += Boundary.Display(frame.ptsMillis, token, frame, currentViewport)
        frame.durationMillis?.let { duration ->
            val clearAt = frame.ptsMillis.checkedAdd(duration) ?: return@let
            boundaries += Boundary.Clear(clearAt, token)
        }
        armNextBoundary()
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
            val generationAtArm = subtitleGeneration
            val runnable = Runnable {
                enqueue {
                    if (generationAtArm != subtitleGeneration) return@enqueue
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
                if (boundary.viewport.generationToken != playbackGeneration || boundary.viewport != viewport) return
                displayedFrameToken = boundary.frameToken
                val epoch = uiEpoch.get()
                mainHandler.post {
                    if (uiEpoch.get() != epoch) return@post
                    if (boundary.frame.images.isEmpty()) {
                        overlayView.clearCaption()
                    } else if (!overlayView.showCaptionFrame(
                            boundary.frame.images,
                            boundary.viewport.contentLeftPx,
                            boundary.viewport.contentTopPx,
                        )) {
                        overlayView.clearCaption()
                    }
                }
            }
        }
    }

    private fun cancelScheduledBoundary() {
        scheduledRunnable?.let(mainHandler::removeCallbacks)
        scheduledRunnable = null
    }

    private fun postClear() {
        val epoch = uiEpoch.incrementAndGet()
        mainHandler.post { if (uiEpoch.get() == epoch) overlayView.clearCaption() }
    }

    override fun close() {
        if (!released.compareAndSet(false, true)) return
        runBlocking {
            subtitleGeneration++
            cancelScheduledBoundary()
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
        ): CaptionViewport? {
            if (overlayWidth <= 0 || overlayHeight <= 0 || videoWidth <= 0 || videoHeight <= 0 || generation < 0L) return null
            val overlayAspect = overlayWidth.toDouble() / overlayHeight.toDouble()
            val videoAspect = videoWidth.toDouble() / videoHeight.toDouble()
            return if (overlayAspect > videoAspect) {
                val contentWidth = (overlayHeight * videoAspect).toInt().coerceAtLeast(1)
                CaptionViewport(
                    overlayWidth,
                    overlayHeight,
                    (overlayWidth - contentWidth) / 2,
                    0,
                    contentWidth,
                    overlayHeight,
                    generation,
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
                    generation,
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

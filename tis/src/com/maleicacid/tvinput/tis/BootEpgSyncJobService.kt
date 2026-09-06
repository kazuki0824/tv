package com.maleicacid.tvinput.tis

import android.app.job.JobParameters
import android.app.job.JobService
import android.os.Handler
import android.os.Looper
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference

class BootEpgSyncJobService : JobService() {
    private data class ScanCompletion(val generation: Int, val needsReschedule: Boolean)

    private data class RunContext(
        val params: JobParameters,
        val completionDelivered: AtomicBoolean = AtomicBoolean(false),
        val stopped: AtomicBoolean = AtomicBoolean(false),
        val scanGeneration: AtomicInteger = AtomicInteger(0),
        val earlyCompletion: AtomicReference<ScanCompletion?> = AtomicReference(null),
    )

    private val activeRun = AtomicReference<RunContext?>(null)
    private val mainHandler = Handler(Looper.getMainLooper())

    override fun onStartJob(params: JobParameters): Boolean {
        val run = RunContext(params)
        activeRun.set(run)
        when (DirectBootGuard.drainIfUserUnlocked(applicationContext, "JOB_START", System.currentTimeMillis())) {
            DirectBootGuard.DrainDecision.SKIP_NO_PENDING -> {
                activeRun.compareAndSet(run, null)
                return false
            }
            DirectBootGuard.DrainDecision.SKIP_LOCKED,
            DirectBootGuard.DrainDecision.SKIP_TV_PROVIDER_UNAVAILABLE,
            -> return finish(run, needsReschedule = true)
            DirectBootGuard.DrainDecision.START_BOOT_EPG_SYNC -> Unit
        }
        val inputId = TisInputIdResolver.resolveOwnInputId(applicationContext)
        if (inputId == null) {
            DirectBootGuard.deferPending(applicationContext, "TV_INPUT_ID_UNRESOLVED")
            return finish(run, needsReschedule = true)
        }
        val existingChannels = TvProviderWriter(applicationContext, inputId).existingChannelsResult()
        if (existingChannels.isFailure) {
            DirectBootGuard.deferPending(applicationContext, "TV_PROVIDER_CHANNEL_QUERY_FAILED")
            return finish(run, needsReschedule = true)
        }
        val targetChannels = existingChannels.getOrThrow()
        if (targetChannels.isEmpty()) {
            DirectBootGuard.clearPending(applicationContext)
            activeRun.compareAndSet(run, null)
            return false
        }
        val generation = ChannelScanManager.startBootEpgSyncIfIdle(applicationContext, inputId, targetChannels) { completedGeneration, needsReschedule ->
            if (activeRun.get() !== run || run.stopped.get()) return@startBootEpgSyncIfIdle
            val activeGeneration = run.scanGeneration.get()
            if (activeGeneration == 0) {
                run.earlyCompletion.compareAndSet(null, ScanCompletion(completedGeneration, needsReschedule))
                return@startBootEpgSyncIfIdle
            }
            if (!run.scanGeneration.compareAndSet(completedGeneration, 0)) return@startBootEpgSyncIfIdle
            finish(run, needsReschedule)
        } ?: return finish(run, needsReschedule = true)
        run.scanGeneration.set(generation)
        run.earlyCompletion.getAndSet(null)?.takeIf { it.generation == generation }?.let { completion ->
            if (run.scanGeneration.compareAndSet(generation, 0)) finish(run, completion.needsReschedule)
        }
        return true
    }

    override fun onStopJob(params: JobParameters): Boolean {
        val run = activeRun.get()
        if (run == null || run.params !== params) return true
        run.stopped.set(true)
        val generation = run.scanGeneration.getAndSet(0)
        if (generation != 0) {
            ChannelScanManager.cancelIfCurrent(generation, ScanPurpose.BOOT_EPG_SYNC)
        }
        DirectBootGuard.deferPending(applicationContext, "BOOT_EPG_JOB_STOPPED")
        run.completionDelivered.compareAndSet(false, true)
        activeRun.compareAndSet(run, null)
        return true
    }

    private fun finish(run: RunContext, needsReschedule: Boolean): Boolean {
        if (run.completionDelivered.compareAndSet(false, true)) {
            mainHandler.post {
                if (activeRun.get() === run && !run.stopped.get()) {
                    activeRun.compareAndSet(run, null)
                    jobFinished(run.params, needsReschedule)
                }
            }
        }
        return true
    }
}

package com.maleicacid.tvinput.tis

import android.app.job.JobParameters
import android.app.job.JobService
import android.os.Handler
import android.os.Looper
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger

class BootEpgSyncJobService : JobService() {
    private val completionDelivered = AtomicBoolean(false)
    private val stopped = AtomicBoolean(false)
    private val activeGeneration = AtomicInteger(0)
    private val mainHandler = Handler(Looper.getMainLooper())

    override fun onStartJob(params: JobParameters): Boolean {
        completionDelivered.set(false)
        stopped.set(false)
        activeGeneration.set(0)
        when (DirectBootGuard.drainIfUserUnlocked(applicationContext, "JOB_START", System.currentTimeMillis())) {
            DirectBootGuard.DrainDecision.SKIP_NO_PENDING -> return false
            DirectBootGuard.DrainDecision.SKIP_LOCKED,
            DirectBootGuard.DrainDecision.SKIP_TV_PROVIDER_UNAVAILABLE,
            -> return finish(params, needsReschedule = true)
            DirectBootGuard.DrainDecision.START_BOOT_EPG_SYNC -> Unit
        }
        val inputId = TisInputIdResolver.resolveOwnInputId(applicationContext)
        if (inputId == null) {
            DirectBootGuard.deferPending(applicationContext, "TV_INPUT_ID_UNRESOLVED")
            return finish(params, needsReschedule = true)
        }
        val existingChannels = TvProviderWriter(applicationContext, inputId).existingChannelsResult()
        if (existingChannels.isFailure) {
            DirectBootGuard.deferPending(applicationContext, "TV_PROVIDER_CHANNEL_QUERY_FAILED")
            return finish(params, needsReschedule = true)
        }
        val targetChannels = existingChannels.getOrThrow()
        if (targetChannels.isEmpty()) {
            DirectBootGuard.clearPending(applicationContext)
            return false
        }
        val generation = ChannelScanManager.startBootEpgSyncIfIdle(applicationContext, inputId, targetChannels) { completedGeneration, needsReschedule ->
            activeGeneration.compareAndSet(completedGeneration, 0)
            finish(params, needsReschedule)
        }
            ?: return finish(params, needsReschedule = true)
        activeGeneration.set(generation)
        if (completionDelivered.get()) activeGeneration.compareAndSet(generation, 0)
        return true
    }

    override fun onStopJob(params: JobParameters): Boolean {
        stopped.set(true)
        val generation = activeGeneration.getAndSet(0)
        if (generation != 0) {
            ChannelScanManager.cancelIfCurrent(generation, ScanPurpose.BOOT_EPG_SYNC)
        }
        DirectBootGuard.deferPending(applicationContext, "BOOT_EPG_JOB_STOPPED")
        completionDelivered.compareAndSet(false, true)
        return true
    }

    private fun finish(params: JobParameters, needsReschedule: Boolean): Boolean {
        if (completionDelivered.compareAndSet(false, true)) {
            mainHandler.post {
                if (!stopped.get()) jobFinished(params, needsReschedule)
            }
        }
        return true
    }
}

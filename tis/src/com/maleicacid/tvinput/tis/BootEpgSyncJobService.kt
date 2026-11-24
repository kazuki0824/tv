package com.maleicacid.tvinput.tis

import android.app.job.JobParameters
import android.app.job.JobService
import android.os.Handler
import android.os.Looper
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference

open class BootEpgSyncJobService : JobService() {
    protected open val periodic: Boolean = false

    private data class ScanCompletion(
        val generation: Int,
        val needsReschedule: Boolean,
    )

    private data class RunContext(
        val params: JobParameters,
        val completionDelivered: AtomicBoolean = AtomicBoolean(false),
        val stopped: AtomicBoolean = AtomicBoolean(false),
        val scanGeneration: AtomicInteger = AtomicInteger(0),
        val earlyCompletion: AtomicReference<ScanCompletion?> = AtomicReference(null),
    ) {
        fun acceptCompletion(completion: ScanCompletion): Boolean =
            synchronized(this) {
                if (scanGeneration.get() == 0) {
                    earlyCompletion.compareAndSet(null, completion)
                    false
                } else {
                    scanGeneration.compareAndSet(completion.generation, 0)
                }
            }

        fun bindGeneration(generation: Int): ScanCompletion? =
            synchronized(this) {
                scanGeneration.set(generation)
                earlyCompletion.getAndSet(null)
            }
    }

    private val activeRun = AtomicReference<RunContext?>(null)
    private val mainHandler = Handler(Looper.getMainLooper())

    override fun onStartJob(params: JobParameters): Boolean {
        val run = RunContext(params)
        activeRun.set(run)
        if (!periodic && !drainBootWork(run)) return activeRun.get() === run
        return startRefresh(run)
    }

    private fun drainBootWork(run: RunContext): Boolean =
        when (DirectBootGuard.drainIfUserUnlocked(applicationContext, "JOB_START", System.currentTimeMillis())) {
            DirectBootGuard.DrainDecision.SKIP_NO_PENDING -> {
                activeRun.compareAndSet(run, null)
                false
            }

            DirectBootGuard.DrainDecision.SKIP_LOCKED,
            DirectBootGuard.DrainDecision.SKIP_TV_PROVIDER_UNAVAILABLE,
            -> {
                finish(run, needsReschedule = true)
                false
            }

            DirectBootGuard.DrainDecision.START_BOOT_EPG_SYNC -> true
        }

    // periodic jobは次の定期枠で再試行し、Direct Bootの保留情報に触れない。
    @Suppress("ReturnCount")
    private fun startRefresh(run: RunContext): Boolean {
        if (!ProgramUpgradeCleanup.ensure(applicationContext) { success ->
                if (success && !periodic) {
                    BootEpgSyncScheduler.scheduleIfEligible(applicationContext, "PROGRAM_CLEANUP_COMPLETED")
                }
            }
        ) {
            deferBootWork("PROGRAM_UPGRADE_CLEANUP_PENDING")
            return finish(run, needsReschedule = true)
        }
        val inputId = TisInputIdResolver.resolveOwnInputId(applicationContext)
        if (inputId == null) {
            deferBootWork("TV_INPUT_ID_UNRESOLVED")
            return finish(run, needsReschedule = true)
        }
        val existingChannels = TvProviderWriter(applicationContext, inputId).existingChannelsResult()
        if (existingChannels.isFailure) {
            deferBootWork("TV_PROVIDER_CHANNEL_QUERY_FAILED")
            return finish(run, needsReschedule = true)
        }
        val targetChannels = existingChannels.getOrThrow()
        if (targetChannels.isEmpty()) {
            if (!periodic) DirectBootGuard.clearPending(applicationContext)
            activeRun.compareAndSet(run, null)
            return false
        }
        val generation =
            ChannelScanManager.startEpgRefreshIfIdle(
                applicationContext,
                inputId,
                targetChannels,
                periodic,
            ) { completedGeneration, needsReschedule ->
                if (activeRun.get() !== run || run.stopped.get()) return@startEpgRefreshIfIdle
                val shouldFinish = run.acceptCompletion(ScanCompletion(completedGeneration, needsReschedule))
                if (shouldFinish) finish(run, needsReschedule)
            } ?: return finish(run, needsReschedule = true)
        val earlyCompletion = run.bindGeneration(generation)
        earlyCompletion?.takeIf { it.generation == generation }?.let { completion ->
            if (run.scanGeneration.compareAndSet(generation, 0)) finish(run, completion.needsReschedule)
        }
        return true
    }

    override fun onStopJob(params: JobParameters): Boolean {
        val run = activeRun.get()
        if (run == null || run.params !== params) return !periodic
        run.stopped.set(true)
        val generation = run.scanGeneration.getAndSet(0)
        if (generation != 0) {
            val purpose = if (periodic) ScanPurpose.PERIODIC_EPG_REFRESH else ScanPurpose.BOOT_EPG_SYNC
            ChannelScanManager.cancelIfCurrent(generation, purpose)
        }
        deferBootWork("BOOT_EPG_JOB_STOPPED")
        run.completionDelivered.compareAndSet(false, true)
        activeRun.compareAndSet(run, null)
        return !periodic
    }

    private fun deferBootWork(reason: String) {
        if (!periodic) DirectBootGuard.deferPending(applicationContext, reason)
    }

    private fun finish(
        run: RunContext,
        needsReschedule: Boolean,
    ): Boolean {
        if (run.completionDelivered.compareAndSet(false, true)) {
            mainHandler.post {
                if (activeRun.get() === run && !run.stopped.get()) {
                    activeRun.compareAndSet(run, null)
                    jobFinished(run.params, !periodic && needsReschedule)
                }
            }
        }
        return true
    }
}

class PeriodicEpgRefreshJobService : BootEpgSyncJobService() {
    override val periodic: Boolean = true
}

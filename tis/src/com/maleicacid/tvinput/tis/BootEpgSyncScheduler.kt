package com.maleicacid.tvinput.tis

import android.app.job.JobInfo
import android.app.job.JobScheduler
import android.content.ComponentName
import android.content.Context
import android.os.UserManager
import android.util.Log
import com.maleicacid.tvinput.common.LogTags

object BootEpgSyncScheduler {
    const val JOB_ID = 0x4d455047

    enum class ScheduleResult { SCHEDULED, ALREADY_SCHEDULED, NO_PENDING, USER_LOCKED, UNAVAILABLE }

    fun scheduleIfEligible(context: Context, source: String): ScheduleResult {
        val appContext = context.applicationContext
        if (!DirectBootGuard.isPending(appContext)) return ScheduleResult.NO_PENDING
        val userManager = appContext.getSystemService(UserManager::class.java)
        if (userManager == null || !userManager.isUserUnlocked) return ScheduleResult.USER_LOCKED
        val scheduler = appContext.getSystemService(JobScheduler::class.java)
            ?: return ScheduleResult.UNAVAILABLE
        if (scheduler.getPendingJob(JOB_ID) != null) return ScheduleResult.ALREADY_SCHEDULED
        val job = JobInfo.Builder(JOB_ID, ComponentName(appContext, BootEpgSyncJobService::class.java))
            .setPersisted(false)
            .build()
        val result = if (scheduler.schedule(job) == JobScheduler.RESULT_SUCCESS) {
            ScheduleResult.SCHEDULED
        } else {
            ScheduleResult.UNAVAILABLE
        }
        Log.i(LogTags.TIS, "boot EPG job登録結果 source=$source result=$result")
        return result
    }
}

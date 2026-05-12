package com.maleicacid.tvinput.tis

import android.content.Context
import android.database.sqlite.SQLiteException
import android.media.tv.TvContract
import android.os.UserManager
import android.util.Log
import com.maleicacid.tvinput.common.LogTags

object DirectBootGuard {
    data class DirectBootPendingState(
        val pending: Boolean,
        val lastLockedBootReceivedAt: Long,
        val bootReason: String,
        val lastSkippedReason: String,
    )

    enum class DrainDecision {
        START_BOOT_EPG_SYNC,
        SKIP_NO_PENDING,
        SKIP_LOCKED,
        SKIP_TV_PROVIDER_UNAVAILABLE,
    }

    private const val PREFS = "maleicacid_direct_boot_epg"
    private const val KEY_PENDING = "pending"
    private const val KEY_LAST_LOCKED_BOOT_RECEIVED_AT = "lastLockedBootReceivedAt"
    private const val KEY_BOOT_REASON = "bootReason"
    private const val KEY_LAST_SKIPPED_REASON = "lastSkippedReason"

    fun onLockedBootCompleted(context: Context, nowMillis: Long, bootReason: String) {
        val prefs = prefs(context)
        prefs.edit()
            .putBoolean(KEY_PENDING, true)
            .putLong(KEY_LAST_LOCKED_BOOT_RECEIVED_AT, nowMillis)
            .putString(KEY_BOOT_REASON, bootReason)
            .putString(KEY_LAST_SKIPPED_REASON, "LOCKED_BOOT_DEFERRED")
            .apply()
        BootEpgSyncDiagnostics.lockedBootDeferredCount.incrementAndGet()
        BootEpgSyncDiagnostics.lastSkippedReason = "LOCKED_BOOT_DEFERRED"
        Log.i(LogTags.TIS, "Direct Boot 中の boot EPG 同期を延期しました reason=$bootReason")
    }

    fun drainIfUserUnlocked(context: Context, source: String, nowMillis: Long): DrainDecision {
        val prefs = prefs(context)
        if (!prefs.getBoolean(KEY_PENDING, false)) return DrainDecision.SKIP_NO_PENDING
        val userManager = context.getSystemService(UserManager::class.java)
        if (userManager == null || !userManager.isUserUnlocked) {
            markSkipped(context, "USER_LOCKED")
            BootEpgSyncDiagnostics.pendingDrainSkippedLockedCount.incrementAndGet()
            return DrainDecision.SKIP_LOCKED
        }
        if (!isTvProviderReady(context)) {
            markSkipped(context, "TV_PROVIDER_UNAVAILABLE")
            BootEpgSyncDiagnostics.pendingDrainSkippedTvProviderUnavailableCount.incrementAndGet()
            return DrainDecision.SKIP_TV_PROVIDER_UNAVAILABLE
        }
        BootEpgSyncDiagnostics.pendingDrainStartedCount.incrementAndGet()
        Log.i(LogTags.TIS, "Direct Boot pending boot EPG 同期を開始します source=$source now=$nowMillis")
        return DrainDecision.START_BOOT_EPG_SYNC
    }

    fun clearPending(context: Context) {
        prefs(context).edit().putBoolean(KEY_PENDING, false).apply()
    }

    fun deferPending(context: Context, reason: String) {
        prefs(context).edit()
            .putBoolean(KEY_PENDING, true)
            .putString(KEY_LAST_SKIPPED_REASON, reason)
            .apply()
        BootEpgSyncDiagnostics.lastSkippedReason = reason
        Log.i(LogTags.TIS, "boot EPG 同期を延期しました reason=$reason")
    }

    fun markTunerUnavailable(context: Context) {
        markSkipped(context, "TUNER_UNAVAILABLE")
        BootEpgSyncDiagnostics.pendingDrainSkippedNoTunerCount.incrementAndGet()
    }

    fun pendingStateForTest(context: Context): DirectBootPendingState {
        val prefs = prefs(context)
        return DirectBootPendingState(
            pending = prefs.getBoolean(KEY_PENDING, false),
            lastLockedBootReceivedAt = prefs.getLong(KEY_LAST_LOCKED_BOOT_RECEIVED_AT, 0L),
            bootReason = prefs.getString(KEY_BOOT_REASON, "").orEmpty(),
            lastSkippedReason = prefs.getString(KEY_LAST_SKIPPED_REASON, "").orEmpty(),
        )
    }

    private fun markSkipped(context: Context, reason: String) {
        prefs(context).edit().putString(KEY_LAST_SKIPPED_REASON, reason).apply()
        BootEpgSyncDiagnostics.lastSkippedReason = reason
        Log.w(LogTags.TIS, "boot EPG 同期 drain を延期します reason=$reason")
    }

    private fun isTvProviderReady(context: Context): Boolean = try {
        context.contentResolver.query(
            TvContract.Channels.CONTENT_URI,
            arrayOf(TvContract.Channels._ID),
            null,
            null,
            null,
        )?.close()
        true
    } catch (e: SecurityException) {
        false
    } catch (e: IllegalStateException) {
        false
    } catch (e: SQLiteException) {
        false
    } catch (e: RuntimeException) {
        false
    }

    private fun prefs(context: Context) = context.createDeviceProtectedStorageContext()
        .getSharedPreferences(PREFS, Context.MODE_PRIVATE)
}

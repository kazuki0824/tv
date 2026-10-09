package com.maleicacid.tvinput.tis

import android.content.ContentUris
import android.content.Context
import android.media.tv.TvContract
import android.os.Build
import android.util.Log
import com.maleicacid.tvinput.common.LogTags
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean

/**
 * Product/TIS更新時の番組表cleanup。
 * Programsは再生成可能なcacheとして扱い、旧release provider-dataをmigrationしない。
 */
object ProgramUpgradeCleanup {
    private const val PREFS_NAME = "program_upgrade_cleanup"
    private const val KEY_SOFTWARE_IDENTITY = "software_identity"
    private val running = AtomicBoolean(false)
    private val ready = AtomicBoolean(false)
    private val worker =
        Executors.newSingleThreadExecutor { runnable ->
            Thread(runnable, "maleicacid-program-upgrade-cleanup").apply { isDaemon = true }
        }

    private val completionLock = Any()
    private val completions = mutableListOf<(Boolean) -> Unit>()

    /** Provider I/Oを呼出元で行わず、保留した利用要求へ完了を一度通知する。 */
    fun ensure(
        context: Context,
        onComplete: ((Boolean) -> Unit)? = null,
    ): Boolean = ensure(cleanup = { cleanup(context.applicationContext) }, onComplete = onComplete)

    // ready/runningと登録を同じlockで扱い、完了と登録の競合で要求を失わない。
    @Suppress("TooGenericExceptionCaught")
    internal fun ensure(
        cleanup: () -> Boolean,
        onComplete: ((Boolean) -> Unit)? = null,
    ): Boolean {
        var alreadyReady = false
        val start =
            synchronized(completionLock) {
                if (ready.get()) {
                    alreadyReady = true
                    false
                } else {
                    onComplete?.let(completions::add)
                    running.compareAndSet(false, true)
                }
            }
        if (alreadyReady) {
            onComplete?.invoke(true)
            return true
        }
        if (start) {
            try {
                worker.execute {
                    val success =
                        try {
                            cleanup()
                        } catch (error: Exception) {
                            Log.w(LogTags.TIS, "Program upgrade cleanupに失敗しました", error)
                            false
                        }
                    complete(success)
                }
            } catch (error: java.util.concurrent.RejectedExecutionException) {
                Log.w(LogTags.TIS, "Program upgrade cleanupを受け付けられません", error)
                complete(false)
            }
        }
        return false
    }

    @Suppress("TooGenericExceptionCaught")
    private fun complete(success: Boolean) {
        val callbacks =
            synchronized(completionLock) {
                ready.set(success)
                running.set(false)
                completions.toList().also { completions.clear() }
            }
        callbacks.forEach { callback ->
            try {
                callback(success)
            } catch (error: Exception) {
                Log.w(LogTags.TIS, "Program cleanup完了通知に失敗しました", error)
            }
        }
    }

    // 未準備の原因を発生点で返す。I/Oは単一workerだけが実行する。
    @Suppress("ReturnCount")
    private fun cleanup(appContext: Context): Boolean {
        val storage = appContext.createDeviceProtectedStorageContext()
        val prefs = storage.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
        val currentIdentity = currentSoftwareIdentity(appContext) ?: return false
        if (prefs.getString(KEY_SOFTWARE_IDENTITY, null) == currentIdentity) return true

        val inputId = TisInputIdResolver.resolveOwnInputId(appContext) ?: return false
        val completed =
            deleteOwnedPrograms(appContext, inputId) {
                prefs.edit().putString(KEY_SOFTWARE_IDENTITY, currentIdentity).commit()
            }
        if (!completed) {
            Log.w(LogTags.TIS, "Program upgrade cleanupを完了できません inputId=$inputId")
            return false
        }
        Log.i(LogTags.TIS, "旧product buildのProgram行を破棄しました inputId=$inputId")
        return true
    }

    internal fun deleteOwnedPrograms(
        context: Context,
        inputId: String,
        commitIdentity: () -> Boolean,
    ): Boolean {
        val programIds = queryOwnedProgramIds(context, inputId) ?: return false
        return runCleanupTransaction(programIds, { deleteProgram(context, it) }, commitIdentity)
    }

    private fun currentSoftwareIdentity(context: Context): String? =
        runCatching {
            val packageInfo = context.packageManager.getPackageInfo(context.packageName, 0)
            "${Build.FINGERPRINT}|${packageInfo.longVersionCode}"
        }.onFailure {
            Log.w(LogTags.TIS, "Program upgrade cleanup software identityを取得できません", it)
        }.getOrNull()

    private fun queryOwnedProgramIds(
        context: Context,
        inputId: String,
    ): List<Long>? =
        runCatching {
            val channelCursor =
                context.contentResolver.query(
                    TvContract.buildChannelsUriForInput(inputId),
                    arrayOf(TvContract.Channels._ID),
                    null,
                    null,
                    null,
                ) ?: error("TvProviderのチャンネル問い合わせがnull cursorを返しました")
            val channelIds = mutableListOf<Long>()
            channelCursor.use { cursor ->
                while (cursor.moveToNext()) channelIds += cursor.getLong(0)
            }

            val rows = mutableListOf<Pair<Long, String?>>()
            channelIds.forEach { channelId ->
                val programCursor =
                    context.contentResolver.query(
                        TvContract.buildProgramsUriForChannel(channelId),
                        arrayOf(
                            TvContract.Programs._ID,
                            TvContract.Programs.COLUMN_PACKAGE_NAME,
                        ),
                        null,
                        null,
                        null,
                    ) ?: error("TvProviderの番組問い合わせがnull cursorを返しました channelId=$channelId")
                programCursor.use { cursor ->
                    while (cursor.moveToNext()) {
                        rows += cursor.getLong(0) to cursor.getString(1)
                    }
                }
            }
            ownedProgramIds(rows, context.packageName)
        }.onFailure {
            Log.w(LogTags.TIS, "旧Program行の列挙に失敗しました", it)
        }.getOrNull()

    private fun deleteProgram(
        context: Context,
        programId: Long,
    ): Boolean =
        runCatching {
            context.contentResolver.delete(
                ContentUris.withAppendedId(TvContract.Programs.CONTENT_URI, programId),
                null,
                null,
            ) == 1
        }.onFailure {
            Log.w(LogTags.TIS, "旧Program行の削除に失敗しました id=$programId", it)
        }.getOrDefault(false)

    internal fun ownedProgramIds(
        rows: List<Pair<Long, String?>>,
        ownPackage: String,
    ): List<Long> = rows.filter { it.second == ownPackage }.map { it.first }

    internal fun runCleanupTransaction(
        programIds: List<Long>,
        deleteProgram: (Long) -> Boolean,
        commitIdentity: () -> Boolean,
    ): Boolean {
        for (programId in programIds) {
            if (!deleteProgram(programId)) return false
        }
        return commitIdentity()
    }
}

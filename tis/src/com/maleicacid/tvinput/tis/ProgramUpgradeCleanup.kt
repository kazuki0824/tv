package com.maleicacid.tvinput.tis

import android.content.ContentUris
import android.content.Context
import android.media.tv.TvContract
import android.os.Build
import android.util.Log
import com.maleicacid.tvinput.common.LogTags

/**
 * Product/TIS更新時の番組表cleanup。
 * Programsは再生成可能なcacheとして扱い、旧release provider-dataをmigrationしない。
 */
object ProgramUpgradeCleanup {
    private const val PREFS_NAME = "program_upgrade_cleanup"
    private const val KEY_SOFTWARE_IDENTITY = "software_identity"
    private val lock = Any()

    fun ensure(context: Context): Boolean =
        synchronized(lock) {
            val appContext = context.applicationContext
            val storage = appContext.createDeviceProtectedStorageContext()
            val prefs = storage.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
            val currentIdentity = currentSoftwareIdentity(appContext) ?: return false
            if (prefs.getString(KEY_SOFTWARE_IDENTITY, null) == currentIdentity) return true

            val inputId = TisInputIdResolver.resolveOwnInputId(appContext) ?: return false
            val deleted = deleteOwnedPrograms(appContext, inputId)
            if (!deleted) return false

            if (!prefs.edit().putString(KEY_SOFTWARE_IDENTITY, currentIdentity).commit()) {
                Log.w(LogTags.TIS, "Program upgrade cleanup software identityを保存できません")
                return false
            }
            Log.i(LogTags.TIS, "旧product buildのProgram行を破棄しました inputId=$inputId")
            true
        }

    private fun currentSoftwareIdentity(context: Context): String? =
        runCatching {
            val packageInfo = context.packageManager.getPackageInfo(context.packageName, 0)
            "${Build.FINGERPRINT}|${packageInfo.longVersionCode}"
        }.onFailure {
            Log.w(LogTags.TIS, "Program upgrade cleanup software identityを取得できません", it)
        }.getOrNull()

    private fun deleteOwnedPrograms(
        context: Context,
        inputId: String,
    ): Boolean =
        runCatching {
            val channelCursor =
                context.contentResolver.query(
                    TvContract.buildChannelsUriForInput(inputId),
                    arrayOf(TvContract.Channels._ID),
                    null,
                    null,
                    null,
                ) ?: error("TvProvider channel query returned null cursor")
            val channelIds = mutableListOf<Long>()
            channelCursor.use { cursor ->
                while (cursor.moveToNext()) channelIds += cursor.getLong(0)
            }

            val programIds = mutableListOf<Long>()
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
                    ) ?: error("TvProvider program query returned null cursor channelId=$channelId")
                programCursor.use { cursor ->
                    while (cursor.moveToNext()) {
                        if (cursor.getString(1) == context.packageName) {
                            programIds += cursor.getLong(0)
                        }
                    }
                }
            }

            programIds.forEach { programId ->
                val deleted =
                    context.contentResolver.delete(
                        ContentUris.withAppendedId(TvContract.Programs.CONTENT_URI, programId),
                        null,
                        null,
                    )
                check(deleted == 1) { "旧Program行を削除できません id=$programId deleted=$deleted" }
            }
            true
        }.onFailure {
            Log.w(LogTags.TIS, "Program upgrade cleanupに失敗しました", it)
        }.getOrDefault(false)

    internal fun softwareIdentityForTest(
        fingerprint: String,
        versionCode: Long,
    ): String = "$fingerprint|$versionCode"
}

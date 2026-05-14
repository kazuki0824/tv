package com.maleicacid.tvinput.tis

import android.content.ComponentName
import android.content.Context
import android.media.tv.TvInputManager
import android.util.Log
import com.maleicacid.tvinput.common.LogTags

/**
 * 自TISの TvInputInfo.id を framework 登録情報から解決する。
 *
 * r51 では setup / boot / unlock drain のいずれも、固定文字列や package 名だけを
 * inputId として扱わず、TvInputManager が公開した実際の inputId を使う。
 */
object TisInputIdResolver {
    fun resolveOwnInputId(context: Context): String? {
        val ownComponent = ComponentName(context, MaleicacidTvInputService::class.java)
        val manager = context.getSystemService(TvInputManager::class.java)
        if (manager == null) {
            Log.w(LogTags.TIS, "TvInputManager が取得できないため自TIS inputIdを解決できません")
            return null
        }
        val matches = manager.tvInputList.filter { info ->
            isOwnInputInfoForTest(
                infoId = info.id,
                servicePackageName = info.serviceInfo.packageName,
                serviceName = info.serviceInfo.name,
                ownPackageName = ownComponent.packageName,
                ownClassName = ownComponent.className,
            )
        }
        if (matches.size != 1) {
            Log.w(LogTags.TIS, "自TIS inputId が一意に解決できません matches=${matches.map { it.id }}")
            return null
        }
        return matches.single().id
    }

    fun isOwnInputId(context: Context, candidate: String?): Boolean {
        val id = candidate?.takeIf { it.isNotBlank() } ?: return false
        val ownComponent = ComponentName(context, MaleicacidTvInputService::class.java)
        val manager = context.getSystemService(TvInputManager::class.java) ?: return false
        return manager.tvInputList.any { info ->
            isOwnInputInfoForTest(
                infoId = info.id,
                servicePackageName = info.serviceInfo.packageName,
                serviceName = info.serviceInfo.name,
                ownPackageName = ownComponent.packageName,
                ownClassName = ownComponent.className,
            ) && info.id == id
        }
    }

    fun isOwnInputInfoForTest(
        infoId: String?,
        servicePackageName: String?,
        serviceName: String?,
        ownPackageName: String,
        ownClassName: String,
    ): Boolean = !infoId.isNullOrBlank() &&
        servicePackageName == ownPackageName &&
        serviceName == ownClassName
}

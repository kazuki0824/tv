package com.maleicacid.tvinput.tis

import android.content.Context
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.util.ReflectionHelpers

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class ProgramUpgradeIdentityTest {
    @Test
    fun replacingApkAtSameVersionAndPathChangesCleanupIdentity() {
        val context = RuntimeEnvironment.getApplication()
        val apk = java.io.File.createTempFile("tis-identity", ".apk", context.cacheDir)
        val info = context.applicationInfo
        val previous = info.sourceDir
        try {
            info.sourceDir = apk.path

            fun identity(): String? =
                ReflectionHelpers.callInstanceMethod(
                    ProgramUpgradeCleanup,
                    "currentSoftwareIdentity",
                    ReflectionHelpers.ClassParameter.from(Context::class.java, context),
                )
            apk.writeText("old provider-data implementation")
            val old = checkNotNull(identity())
            check(identity() == old)
            apk.writeText("new provider-data implementation")
            check(checkNotNull(identity()) != old)
            check(apk.delete())
            check(identity() == null)
        } finally {
            info.sourceDir = previous
            apk.delete()
        }
    }
}

// TunerControllerの実際のlease切替境界を検査し、物理Frontend割当はTRMへ委譲する。
package com.maleicacid.tvinput.tis

import android.media.tv.tuner.frontend.FrontendSettings
import org.junit.Test
import sun.misc.Unsafe

class TunerFrontendLeaseTransitionTest {
    private val field =
        TunerController::class.java
            .getDeclaredField("frontendLeaseType")
            .apply { isAccessible = true }
    private val transition =
        TunerController::class.java
            .getDeclaredMethod(
                "releaseFrontendBeforeTypeChange",
                Int::class.javaPrimitiveType ?: error("Int primitive type unavailable"),
                Function0::class.java,
            ).apply { isAccessible = true }

    private fun controller(): TunerController {
        val unsafe =
            Unsafe::class.java
                .getDeclaredField("theUnsafe")
                .apply { isAccessible = true }
                .get(null) as Unsafe
        return unsafe.allocateInstance(TunerController::class.java) as TunerController
    }

    private fun switch(
        controller: TunerController,
        targetType: Int,
        close: () -> Unit,
    ) {
        transition.invoke(controller, targetType, close)
    }

    @Test
    fun sameTypeAndNoLeaseDoNotReleaseFrontend() {
        val controller = controller()
        var closes = 0
        val close = { closes++; Unit }
        switch(controller, FrontendSettings.TYPE_ISDBT, close)
        check(closes == 0 && field.get(controller) == null)
        field.set(controller, FrontendSettings.TYPE_ISDBT)
        switch(controller, FrontendSettings.TYPE_ISDBT, close)
        check(closes == 0 && field.get(controller) == FrontendSettings.TYPE_ISDBT)
    }

    @Test
    fun satelliteToTerrestrialAndBackReleaseOnlyPreviousLease() {
        val controller = controller()
        var closes = 0
        val close = { closes++; Unit }
        field.set(controller, FrontendSettings.TYPE_ISDBS)
        switch(controller, FrontendSettings.TYPE_ISDBT, close)
        check(closes == 1 && field.get(controller) == null)
        // 新しいTuner.tuneがFrontendを要求した状態に相当する。
        field.set(controller, FrontendSettings.TYPE_ISDBT)
        switch(controller, FrontendSettings.TYPE_ISDBS, close)
        check(closes == 2 && field.get(controller) == null)
    }

    @Test
    fun failedCloseRetainsLeaseIdentityForRetry() {
        val controller = controller()
        field.set(controller, FrontendSettings.TYPE_ISDBS)
        val failure =
            runCatching {
                switch(controller, FrontendSettings.TYPE_ISDBT) {
                    error("frontend close failed")
                }
            }.exceptionOrNull()
        check(failure is java.lang.reflect.InvocationTargetException)
        check(failure.cause?.message == "frontend close failed")
        check(field.get(controller) == FrontendSettings.TYPE_ISDBS)
        var retries = 0
        switch(controller, FrontendSettings.TYPE_ISDBT) { retries++ }
        check(retries == 1 && field.get(controller) == null)
    }
}

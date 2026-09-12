// テストの入力・期待値を本体の定数と独立した具体値で記述する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import android.content.Intent
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class DirectBootGuardR51FixTest {
    @Test fun lockedBootStoresOnlyPendingState() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        DirectBootGuard.onLockedBootCompleted(context, 1234L, Intent.ACTION_LOCKED_BOOT_COMPLETED)
        val state = DirectBootGuard.pendingStateForTest(context)
        check(state.pending)
        check(state.lastLockedBootReceivedAt == 1234L)
        check(state.bootReason == Intent.ACTION_LOCKED_BOOT_COMPLETED)
        check(state.lastSkippedReason == "LOCKED_BOOT_DEFERRED")
    }
}

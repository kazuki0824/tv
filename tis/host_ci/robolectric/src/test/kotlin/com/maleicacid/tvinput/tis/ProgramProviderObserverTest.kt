@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.util.ReflectionHelpers
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

@RunWith(RobolectricTestRunner::class)
@Config(
    sdk = [35],
    instrumentedPackages = ["com.maleicacid.tvinput.aribsi"],
    shadows = [TunerFrontendLeaseTransitionTest.SiEngineBoundary::class],
)
class ProgramProviderObserverTest {
    // 実Session observerとowner占有を通し、通知数がtask数へ増幅しないことを検査する。
    @Suppress("LongMethod")
    @Test
    fun providerBurstMarksOneDirtyStateWithoutReleasingSession() {
        val context = RuntimeEnvironment.getApplication()
        val session = MaleicacidLiveSession(context, context, "provider-burst-test", "provider-burst-test")
        val executor = ReflectionHelpers.getField<LifecycleSerialExecutor>(session, "sessionExecutor")
        val released = ReflectionHelpers.getField<AtomicBoolean>(session, "releaseOnce")
        val dirty = ReflectionHelpers.getField<AtomicBoolean>(session, "programProviderDirty")
        val coordinator = ReflectionHelpers.getField<ProgramPublishCoordinator>(session, "programPublishCoordinator")
        @Suppress("UNCHECKED_CAST")
        val signatures =
            ProgramPublishCoordinator::class.java
                .getDeclaredField("lastInputSignatureByMode")
                .apply { isAccessible = true }
                .get(coordinator) as MutableMap<ChannelScanController.PublishMode, String>
        signatures[ChannelScanController.PublishMode.LIVE_TUNE_REFRESH] = "old"
        val observer = ReflectionHelpers.getField<android.database.ContentObserver>(session, "programProviderObserver")
        val entered = CountDownLatch(1)
        val unblock = CountDownLatch(1)
        executor.executeControl {
            entered.countDown()
            check(unblock.await(5, TimeUnit.SECONDS))
        }
        try {
            check(entered.await(5, TimeUnit.SECONDS))
            repeat(128) { observer.onChange(false) }
            check(!released.get() && dirty.get() && signatures.isNotEmpty())
            unblock.countDown()
            executor.callControl(5_000L) {
                val snapshot =
                    com.maleicacid.tvinput.aribsi.ProgramPublishSnapshot(
                        discoveryProfile = 0,
                        ingestSequence = 0L,
                        events = emptyList(),
                        updateWindows = emptyList(),
                        semanticFactsByServiceKey = emptyMap(),
                        descriptorDiagnostics = emptyList(),
                        parserDiagnostics = emptyList(),
                    )
                MaleicacidLiveSession::class.java
                    .getDeclaredMethod(
                        "publishLivePrograms",
                        List::class.java,
                        snapshot.javaClass,
                    ).apply { isAccessible = true }
                    .invoke(session, emptyList<Any>(), snapshot)
                check(signatures.isEmpty() && !dirty.get() && !released.get())
            }
        } finally {
            unblock.countDown()
            session.onRelease()
            check(executor.awaitTermination(5, TimeUnit.SECONDS))
        }
    }
}

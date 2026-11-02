@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import com.maleicacid.tvinput.aribsi.AribSiEngine
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.util.ReflectionHelpers
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

@RunWith(RobolectricTestRunner::class)
@Config(
    sdk = [35],
    instrumentedPackages = ["com.maleicacid.tvinput.aribsi"],
    shadows = [TunerFrontendLeaseTransitionTest.SiEngineBoundary::class],
)
class ScanCancellationProviderTest {
    @Test
    fun uiCancellationReturnsWhileProviderPublicationOrFinalCommitIsBlocked() {
        for (finalizing in listOf(false, true)) {
            val context = RuntimeEnvironment.getApplication<android.content.Context>()
            val task = ChannelScanManager.ActiveScanTask(1, ScanPurpose.SETUP_SCAN, context)
            val engine = AribSiEngine(context)
            val controller =
                ChannelScanController(
                    context,
                    "scan-cancellation-test",
                    engine,
                    ScanPurpose.SETUP_SCAN,
                    task.cancelRequested,
                    task.publicationLock,
                )
            val fence = ReflectionHelpers.getField<ChannelScanController.ScanGenerationFence>(controller, "scanGenerationFence")
            task.controller = controller
            val entered = CountDownLatch(1)
            val release = CountDownLatch(1)
            val workers = Executors.newFixedThreadPool(2)
            try {
                val publish =
                    workers.submit {
                        val blockedProvider = {
                            entered.countDown()
                            check(release.await(5, TimeUnit.SECONDS))
                        }
                        if (finalizing) {
                            fence.finishScan(blockedProvider)
                        } else {
                            fence.publishIfCurrent(1L, blockedProvider)
                        }
                    }
                assertTrue(entered.await(1, TimeUnit.SECONDS))
                val cancel = workers.submit<Boolean> { task.requestCancel() }
                assertEquals(!finalizing, cancel.get(1, TimeUnit.SECONDS))
                assertEquals(!finalizing, task.cancelRequested.get())
                release.countDown()
                publish.get(1, TimeUnit.SECONDS)
                if (!finalizing) {
                    assertEquals(null, fence.publishIfCurrent(1L) { error("取消し後に公開しました") })
                }
                fence.finishScan { Unit }
                assertFalse(task.requestCancel())
            } finally {
                release.countDown()
                workers.shutdownNow()
                controller.close()
                engine.close()
            }
        }
    }
}

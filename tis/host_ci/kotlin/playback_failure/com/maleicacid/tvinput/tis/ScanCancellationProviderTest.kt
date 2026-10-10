@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class ScanCancellationProviderTest {
    @Test
    fun uiCancellationReturnsWhileProviderPublicationOrFinalCommitIsBlocked() {
        for (finalizing in listOf(false, true)) {
            val task =
                ChannelScanManager.ActiveScanTask(1, ScanPurpose.SETUP_SCAN, android.content.ContextWrapper(null))
            val fence = ChannelScanController.ScanGenerationFence(task.publicationLock, task.cancelRequested)
            // native資源を作らず、実Manager→Controller→fenceの取消し経路を検査する。
            val unsafeField = sun.misc.Unsafe::class.java.getDeclaredField("theUnsafe").apply { isAccessible = true }
            val unsafe = unsafeField.get(null) as sun.misc.Unsafe
            val controller = unsafe.allocateInstance(ChannelScanController::class.java) as ChannelScanController
            ChannelScanController::class.java.getDeclaredField("scanGenerationFence").apply {
                isAccessible = true
                set(controller, fence)
            }
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
            }
        }
    }
}

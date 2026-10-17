// 本体の待機予算と独立した試験値を使用する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import org.junit.Test
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicLong

class CaptionCleanupRetryTest {
    @Test
    fun failedRendererCleanupKeepsOwnerUntilRetrySucceeds() {
        val executor = LifecycleSerialExecutor("字幕解放試験")
        val epoch = AtomicLong(Long.MAX_VALUE)
        val cleanup = ResourceCleanup()
        var releases = 0
        var rendererOwned = true
        var clears = 0
        val close = {
            executor.callControl(1_000L, cleanup = true) {
                AribCaptionController.completeTerminalCleanup(
                    epoch,
                    cleanup = {
                        releases++
                        check(releases > 1) { "初回のrenderer解放失敗" }
                        rendererOwned = false
                    },
                    clear = { clears++ },
                    shutdown = { executor.shutdownNow() },
                )
            }
        }
        try {
            cleanup.release("字幕", close)
            check(cleanup.hasPending && rendererOwned && !executor.isShutdown)
            check(releases == 1 && clears == 1 && epoch.get() == -1L)
            cleanup.retry()
            cleanup.requireComplete()
            check(!rendererOwned && releases == 2 && clears == 2)
            check(executor.isShutdown && executor.awaitTermination(1, TimeUnit.SECONDS))
        } finally {
            executor.shutdownNow()
        }
    }

    @Test
    fun exhaustedPresentationDoesNotPreventTerminalCleanup() {
        listOf(Long.MAX_VALUE, -1L).forEach { initialEpoch ->
            val epoch = AtomicLong(initialEpoch)
            val order = mutableListOf<String>()
            AribCaptionController.completeTerminalCleanup(
                epoch,
                cleanup = { order += "解放" },
                clear = { order += "表示消去" },
                shutdown = { order += "停止" },
            )
            check(epoch.get() == -1L)
            check(order == listOf("解放", "表示消去", "停止"))
        }
    }
}

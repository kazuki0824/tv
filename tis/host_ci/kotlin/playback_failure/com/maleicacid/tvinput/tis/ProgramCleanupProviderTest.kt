@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import android.content.ContentInterface
import android.content.ContentResolver
import android.content.ContextWrapper
import android.database.MatrixCursor
import android.net.Uri
import org.junit.Test
import java.lang.reflect.Proxy
import java.util.concurrent.CountDownLatch
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

class ProgramCleanupProviderTest {
    // 実workerとContentResolverでProvider停止・部分削除・retryを連続して検証する。
    @Suppress("LongMethod", "CyclomaticComplexMethod")
    @Test
    fun blockedProviderAndPartialDeleteRemainClosedUntilSuccessfulRetry() {
        val entered = CountDownLatch(1)
        val release = CountDownLatch(1)
        val rows = (1L..10000L).toMutableSet()
        var rejectDelete = true
        var committed = false
        val provider =
            Proxy.newProxyInstance(
                ContentInterface::class.java.classLoader,
                arrayOf(ContentInterface::class.java),
            ) { _, method, args ->
                val uri = args[0] as Uri
                when (method.name) {
                    "query" -> {
                        if (uri.path == "/channel") {
                            entered.countDown()
                            check(release.await(5, TimeUnit.SECONDS))
                            MatrixCursor(arrayOf("_id")).apply { addRow(arrayOf(1L)) }
                        } else {
                            check(uri.path == "/program")
                            MatrixCursor(arrayOf("_id", "package_name")).apply {
                                rows.forEach { addRow(arrayOf(it, "own.package")) }
                                addRow(arrayOf(20000L, "other.package"))
                            }
                        }
                    }

                    "delete" -> {
                        check(uri.pathSegments.first() == "program")
                        val id = uri.lastPathSegment!!.toLong()
                        check(id != 20000L)
                        if (rejectDelete && id == 5000L) {
                            0
                        } else if (rows.remove(id)) {
                            1
                        } else {
                            0
                        }
                    }

                    else -> {
                        error("予期しないProvider操作 ${method.name}")
                    }
                }
            } as ContentInterface
        // Android固定jarのwrapperだけを構築し、Binder/native初期化をホスト境界で省く。
        val unsafe =
            sun.misc.Unsafe::class.java
                .getDeclaredField("theUnsafe")
                .apply { isAccessible = true }
                .get(null) as sun.misc.Unsafe
        val resolver = unsafe.allocateInstance(Class.forName("android.content.ContentResolver\$1")) as ContentResolver
        ContentResolver::class.java.getDeclaredField("mWrapped").apply {
            isAccessible = true
            set(resolver, provider)
        }
        val context =
            object : ContextWrapper(null) {
                override fun getContentResolver(): ContentResolver = resolver

                override fun getPackageName(): String = "own.package"

                override fun getApplicationContext() = this
            }
        val ready =
            ProgramUpgradeCleanup::class.java
                .getDeclaredField("ready")
                .apply { isAccessible = true }
                .get(null) as AtomicBoolean
        val worker =
            ProgramUpgradeCleanup::class.java
                .getDeclaredField("worker")
                .apply { isAccessible = true }
                .get(null) as ExecutorService
        val previous = ready.getAndSet(false)
        val caller = Executors.newSingleThreadExecutor()
        val cleanup = {
            ProgramUpgradeCleanup.deleteOwnedPrograms(context, "own/input") {
                committed = true
                true
            }
        }
        try {
            check(!caller.submit<Boolean> { ProgramUpgradeCleanup.ensure(cleanup) }.get(1, TimeUnit.SECONDS))
            check(entered.await(1, TimeUnit.SECONDS))
            check(!caller.submit<Boolean> { ProgramUpgradeCleanup.ensure(cleanup) }.get(1, TimeUnit.SECONDS))
            val firstCompletion =
                java.util.concurrent.atomic
                    .AtomicReference<Boolean>()
            check(!ProgramUpgradeCleanup.ensure(cleanup, firstCompletion::set))
            val setupGeneration = checkNotNull(ChannelScanManager.startIfIdle(context, "own/input"))
            check(ChannelScanManager.currentState() is ScanState.Running)
            check(firstCompletion.get() == null && !ready.get() && !committed)
            release.countDown()
            worker.submit {}.get(5, TimeUnit.SECONDS)
            check(!ready.get() && !committed && rows.size == 5001 && firstCompletion.get() == false)
            val scanWorker =
                ChannelScanManager::class.java
                    .getDeclaredField("executor")
                    .apply {
                        isAccessible = true
                    }.get(ChannelScanManager) as ExecutorService
            scanWorker.submit {}.get(5, TimeUnit.SECONDS)
            val failed = ChannelScanManager.currentState() as ScanState.Failed
            check(failed.generation == setupGeneration && failed.message == "Program cleanupに失敗しました")
            rejectDelete = false
            val resumed = CountDownLatch(1)
            check(
                !ProgramUpgradeCleanup.ensure(cleanup) { success ->
                    check(success && rows.isEmpty() && committed)
                    resumed.countDown()
                },
            )
            check(resumed.await(5, TimeUnit.SECONDS))
            worker.submit {}.get(5, TimeUnit.SECONDS)
            check(rows.isEmpty() && committed && ProgramUpgradeCleanup.ensure(cleanup))
        } finally {
            release.countDown()
            worker.submit {}.get(5, TimeUnit.SECONDS)
            ready.set(previous)
            caller.shutdownNow()
        }
    }
}

package com.maleicacid.tvinput.tis

import org.junit.Test
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

@Suppress("MagicNumber")
class ProgramUpgradeCleanupTest {
    // 同一workerの待機・失敗・retryを一つの寿命で検査する。
    @Suppress("LongMethod")
    @Test
    fun slowCleanupRejectsUseWithoutWaitingAndFailedDeleteRetriesBeforeReady() {
        val readyField = ProgramUpgradeCleanup::class.java.getDeclaredField("ready").apply { isAccessible = true }
        val ready = readyField.get(null) as AtomicBoolean
        val runningField = ProgramUpgradeCleanup::class.java.getDeclaredField("running").apply { isAccessible = true }
        val running = runningField.get(null) as AtomicBoolean
        val workerField = ProgramUpgradeCleanup::class.java.getDeclaredField("worker").apply { isAccessible = true }
        val worker = workerField.get(null) as java.util.concurrent.ExecutorService
        val previous = ready.getAndSet(false)
        val entered = CountDownLatch(1)
        val release = CountDownLatch(1)
        val caller = Executors.newSingleThreadExecutor()
        val rows = (1L..10000L).toMutableSet()
        var committed = false
        try {
            val request =
                caller.submit<Boolean> {
                    ProgramUpgradeCleanup.ensure {
                        entered.countDown()
                        check(release.await(5, TimeUnit.SECONDS))
                        ProgramUpgradeCleanup.runCleanupTransaction(
                            rows.toList(),
                            deleteProgram = { if (it == 5000L) false else rows.remove(it) },
                            commitIdentity = {
                                committed = true
                                true
                            },
                        )
                    }
                }
            check(!request.get(1, TimeUnit.SECONDS))
            check(entered.await(1, TimeUnit.SECONDS))
            repeat(10) {
                check(!ProgramUpgradeCleanup.ensure { error("cleanupを重複実行しました") })
            }
            check(!ready.get() && !committed)
            release.countDown()
            worker.submit {}.get(2, TimeUnit.SECONDS)
            check(!running.get() && !ready.get() && !committed && rows.size == 5001)
            check(
                !ProgramUpgradeCleanup.ensure {
                    ProgramUpgradeCleanup.runCleanupTransaction(
                        rows.toList(),
                        deleteProgram = { rows.remove(it) },
                        commitIdentity = {
                            committed = true
                            true
                        },
                    )
                },
            )
            worker.submit {}.get(2, TimeUnit.SECONDS)
            check(committed && rows.isEmpty())
            check(ProgramUpgradeCleanup.ensure { error("完了後に再削除しました") })
        } finally {
            release.countDown()
            worker.submit {}.get(2, TimeUnit.SECONDS)
            ready.set(previous)
            caller.shutdownNow()
        }
    }

    @Test
    fun contextIoIsConfinedToWorkerAndExceptionAllowsRetry() {
        val readyField = ProgramUpgradeCleanup::class.java.getDeclaredField("ready").apply { isAccessible = true }
        val ready = readyField.get(null) as AtomicBoolean
        val workerField = ProgramUpgradeCleanup::class.java.getDeclaredField("worker").apply { isAccessible = true }
        val worker = workerField.get(null) as java.util.concurrent.ExecutorService
        val previous = ready.getAndSet(false)
        val callerThread = Thread.currentThread()
        var attempts = 0
        val context =
            object : android.content.ContextWrapper(null) {
                override fun getApplicationContext(): android.content.Context = this

                override fun createDeviceProtectedStorageContext(): android.content.Context {
                    check(Thread.currentThread() !== callerThread)
                    attempts++
                    error("ストレージ利用不能")
                }
            }
        try {
            repeat(2) {
                check(!ProgramUpgradeCleanup.ensure(context))
                worker.submit {}.get(2, TimeUnit.SECONDS)
                check(!ready.get())
            }
            check(attempts == 2)
        } finally {
            ready.set(previous)
        }
    }

    @Test
    fun deletesOnlyOwnPrograms() {
        val rows =
            listOf(
                1L to "com.maleicacid.tvinput",
                2L to "other.package",
                3L to "com.maleicacid.tvinput",
                4L to null,
            )
        check(
            ProgramUpgradeCleanup.ownedProgramIds(rows, "com.maleicacid.tvinput") ==
                listOf(1L, 3L),
        )
    }

    @Test
    fun commitsIdentityOnlyAfterEveryDeleteSucceeds() {
        val deleted = mutableListOf<Long>()
        var committed = false
        val success =
            ProgramUpgradeCleanup.runCleanupTransaction(
                programIds = listOf(10L, 20L, 30L),
                deleteProgram = {
                    deleted += it
                    true
                },
                commitIdentity = {
                    committed = true
                    true
                },
            )
        check(success)
        check(deleted == listOf(10L, 20L, 30L))
        check(committed)
    }

    @Test
    fun failedDeleteStopsBeforeIdentityCommit() {
        val deleted = mutableListOf<Long>()
        var committed = false
        val success =
            ProgramUpgradeCleanup.runCleanupTransaction(
                programIds = listOf(10L, 20L, 30L),
                deleteProgram = {
                    deleted += it
                    it != 20L
                },
                commitIdentity = {
                    committed = true
                    true
                },
            )
        check(!success)
        check(deleted == listOf(10L, 20L))
        check(!committed)
    }
}

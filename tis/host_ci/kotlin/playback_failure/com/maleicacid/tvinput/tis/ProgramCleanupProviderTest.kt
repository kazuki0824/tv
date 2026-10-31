@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import android.content.ContentInterface
import android.content.ContentResolver
import android.content.ContextWrapper
import android.content.SharedPreferences
import android.database.MatrixCursor
import android.net.Uri
import org.junit.Test
import java.lang.reflect.Proxy
import java.util.concurrent.ConcurrentLinkedQueue
import java.util.concurrent.CountDownLatch
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger

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
        val pendingReads = AtomicInteger()
        val preferences =
            Proxy.newProxyInstance(
                SharedPreferences::class.java.classLoader,
                arrayOf(SharedPreferences::class.java),
            ) { _, method, args ->
                check(method.name == "getBoolean" && args[0] == "pending") {
                    "予期しないpreferences操作 ${method.name}"
                }
                pendingReads.incrementAndGet()
                false
            } as SharedPreferences
        val context =
            object : ContextWrapper(null) {
                override fun getContentResolver(): ContentResolver = resolver

                override fun getPackageName(): String = "own.package"

                override fun getApplicationContext() = this

                override fun createDeviceProtectedStorageContext() = this

                override fun getSharedPreferences(
                    name: String,
                    mode: Int,
                ): SharedPreferences = preferences
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
        val scanWorker =
            ChannelScanManager::class.java
                .getDeclaredField("executor")
                .apply { isAccessible = true }
                .get(ChannelScanManager) as ExecutorService
        val scanFailures = ConcurrentLinkedQueue<Throwable>()
        val activeScan =
            ChannelScanManager::class.java
                .getDeclaredField("activeTask")
                .apply { isAccessible = true }
                .get(ChannelScanManager) as java.util.concurrent.atomic.AtomicReference<*>
        val previousHandler =
            scanWorker
                .submit<Thread.UncaughtExceptionHandler> {
                    Thread.currentThread().uncaughtExceptionHandler.also {
                        Thread.currentThread().uncaughtExceptionHandler =
                            Thread.UncaughtExceptionHandler { _, error -> scanFailures.add(error) }
                    }
                }.get(5, TimeUnit.SECONDS)
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
            scanWorker.submit {}.get(5, TimeUnit.SECONDS)
            val failed = ChannelScanManager.currentState() as ScanState.Failed
            check(failed.generation == setupGeneration && failed.message == "Program cleanupに失敗しました")
            check(activeScan.get() == null && pendingReads.get() == 0)
            rejectDelete = false
            val resumed = CountDownLatch(1)
            val retryEntered = CountDownLatch(1)
            val continueRetry = CountDownLatch(1)
            check(
                !ProgramUpgradeCleanup.ensure(cleanup = {
                    retryEntered.countDown()
                    check(continueRetry.await(5, TimeUnit.SECONDS))
                    cleanup()
                }, onComplete = { success ->
                    check(success && rows.isEmpty() && committed)
                    resumed.countDown()
                }),
            )
            check(retryEntered.await(5, TimeUnit.SECONDS))
            val retriedSetup = checkNotNull(ChannelScanManager.startIfIdle(context, "own/input"))
            check(ChannelScanManager.currentState() is ScanState.Running)
            continueRetry.countDown()
            check(resumed.await(5, TimeUnit.SECONDS))
            worker.submit {}.get(5, TimeUnit.SECONDS)
            scanWorker.submit {}.get(5, TimeUnit.SECONDS)
            // cleanup成功後は再要求なしで実scanへ進む。hostのAndroid Tuner constructorは未提供。
            val attempted = ChannelScanManager.currentState() as ScanState.Failed
            check(attempted.generation == retriedSetup && attempted.message != "Program cleanupに失敗しました")
            check(rows.isEmpty() && committed && ProgramUpgradeCleanup.ensure(cleanup))
            // cleanup成功後の実scan終端はDirectBoot pending検査まで一度到達する。
            check(activeScan.get() == null && pendingReads.get() == 1)
        } finally {
            release.countDown()
            worker.submit {}.get(5, TimeUnit.SECONDS)
            ready.set(previous)
            caller.shutdownNow()
            val restored = scanWorker.submit { Thread.currentThread().uncaughtExceptionHandler = previousHandler }
            restored.get(5, TimeUnit.SECONDS)
        }
        check(scanFailures.isEmpty()) { "scan ownerの非同期例外: ${scanFailures.joinToString { it.stackTraceToString() }}" }
    }

    // 実Session observerとowner占有を通し、通知数がtask数へ増幅しないことを検査する。
    @Suppress("LongMethod")
    @Test
    fun providerBurstMarksOneDirtyStateWithoutReleasingSession() {
        val unsafe =
            sun.misc.Unsafe::class.java
                .getDeclaredField("theUnsafe")
                .apply { isAccessible = true }
                .get(null) as sun.misc.Unsafe
        val session = unsafe.allocateInstance(MaleicacidLiveSession::class.java) as MaleicacidLiveSession

        fun set(
            name: String,
            value: Any,
        ) {
            MaleicacidLiveSession::class.java
                .getDeclaredField(name)
                .apply { isAccessible = true }
                .set(session, value)
        }
        val executor = LifecycleSerialExecutor("provider-burst-test", 64)
        val released = AtomicBoolean(false)
        val dirty = AtomicBoolean(false)
        set("sessionExecutor", executor)
        set("releaseOnce", released)
        set("programProviderDirty", dirty)
        val context = object : ContextWrapper(null) {}
        val coordinator = ProgramPublishCoordinator(TvProviderWriter(context, "test"))
        set("programPublishCoordinator", coordinator)
        @Suppress("UNCHECKED_CAST")
        val signatures =
            ProgramPublishCoordinator::class.java
                .getDeclaredField("lastInputSignatureByMode")
                .apply { isAccessible = true }
                .get(coordinator) as MutableMap<ChannelScanController.PublishMode, String>
        signatures[ChannelScanController.PublishMode.LIVE_TUNE_REFRESH] = "old"
        val observer =
            Class
                .forName("com.maleicacid.tvinput.tis.MaleicacidLiveSession\$programProviderObserver\$1")
                .getDeclaredConstructor(MaleicacidLiveSession::class.java)
                .apply { isAccessible = true }
                .newInstance(session) as android.database.ContentObserver
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
                        0,
                        0L,
                        emptyList(),
                        emptyList(),
                        emptyMap(),
                        emptyList(),
                        emptyList(),
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
            executor.shutdownNow()
        }
    }

    // 同一keyの別package行を含む実ContentResolver索引からupdate/insertまで検査する。
    @Suppress("LongMethod")
    @Test
    fun foreignProgramWithSameKeyIsNeverUpdated() {
        val key =
            com.maleicacid.tvinput.common
                .ServiceKey(4, 16625, 101)
        val program =
            com.maleicacid.tvinput.db.ProgramRecord(
                key,
                10,
                """{"kind":"arib-event-v1","originalNetworkId":4,"transportStreamId":16625,"serviceId":101,"eventId":10}""",
                1_700_000_000_000L,
                1_800_000L,
                "News",
                "desc",
                casFactsCanonicalJson = testCasFacts(false),
            )
        val payload =
            (
                com.maleicacid.tvinput.aribsi.ProviderDataBridge
                    .buildProgramProviderData(program)
                    as com.maleicacid.tvinput.aribsi.ProviderDataBridge.Success
            ).bytes
        var includeOwn = true
        val operations = mutableListOf<TvProviderWriter.ProgramUpsertRequest>()
        val provider =
            Proxy.newProxyInstance(
                ContentInterface::class.java.classLoader,
                arrayOf(ContentInterface::class.java),
            ) { _, method, args ->
                when (method.name) {
                    "query" -> {
                        @Suppress("UNCHECKED_CAST")
                        val projection = args[1] as Array<String>
                        MatrixCursor(projection).apply {
                            val ids = if (includeOwn) listOf(1L, 100L) else listOf(100L)
                            ids.forEach { id ->
                                addRow(
                                    projection
                                        .map { column ->
                                            when (column) {
                                                "_id" -> id
                                                "package_name" -> if (id == 1L) "own.package" else "other.package"
                                                "internal_provider_data" -> payload
                                                "start_time_utc_millis" -> program.startTimeMillis
                                                "end_time_utc_millis" -> program.startTimeMillis + program.durationMillis
                                                else -> null
                                            }
                                        }.toTypedArray(),
                                )
                            }
                        }
                    }

                    else -> {
                        error("Unexpected provider call ${method.name}")
                    }
                }
            } as ContentInterface
        val unsafe =
            sun.misc.Unsafe::class.java
                .getDeclaredField("theUnsafe")
                .apply { isAccessible = true }
                .get(null) as sun.misc.Unsafe
        val resolver = unsafe.allocateInstance(Class.forName("android.content.ContentResolver\$1")) as ContentResolver
        ContentResolver::class.java
            .getDeclaredField("mWrapped")
            .apply { isAccessible = true }
            .set(resolver, provider)
        val context =
            object : ContextWrapper(null) {
                override fun getContentResolver() = resolver

                override fun getPackageName() = "own.package"
            }
        val androidWriter = TvProviderWriter(context, "test")
        val store =
            TvProviderWriter::class.java
                .getDeclaredField("channelStore")
                .apply { isAccessible = true }
                .get(androidWriter) as TvProviderWriter.ChannelStore
        val writer =
            TvProviderWriter(
                "test",
                object : TvProviderWriter.ChannelStore by store {
                    override fun upsertProgramsBatch(
                        requests: List<TvProviderWriter.ProgramUpsertRequest>,
                    ): Result<List<TvProviderWriter.ProgramUpsertOutcome>> {
                        operations += requests
                        return Result.success(requests.map { TvProviderWriter.ProgramUpsertOutcome(it.existingProgramId ?: 2L) })
                    }

                    override fun readCanonicalGenres(
                        channelId: Long,
                        programIds: Set<Long>,
                    ): Result<Map<Long, String?>> = Result.success(emptyMap())
                },
                testOnly = true,
            )
        val values = writer.programValues(1L, program, true, payload)
        val publication =
            TvProviderWriter.PreparedProgramPublication(
                listOf(TvProviderWriter.PreparedServicePrograms(key, 1L, listOf(program to values), emptyList())),
                emptyList(),
            )
        val first = writer.upsertPreparedPrograms(publication)
        check(first.failures.isEmpty() && first.updated == 1)
        check(operations.single().existingProgramId == 1L)
        operations.clear()
        includeOwn = false
        val second = writer.upsertPreparedPrograms(publication)
        check(second.failures.isEmpty() && second.inserted == 1)
        check(operations.single().existingProgramId == null)
    }
}

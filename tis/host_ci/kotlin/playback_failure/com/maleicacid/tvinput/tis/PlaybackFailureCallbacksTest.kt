// テストの入力・期待値を本体の定数と独立した具体値で記述する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import android.content.ContentValues
import android.media.MediaCas
import android.media.MediaSync
import android.media.tv.tuner.Tuner
import android.media.tv.tuner.filter.Filter
import com.maleicacid.tvinput.aribsi.AribElementaryStream
import com.maleicacid.tvinput.aribsi.PmtCatCaMetadataMapper
import com.maleicacid.tvinput.aribsi.ServicePolicyDecision
import com.maleicacid.tvinput.common.FrequencyHz
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.StreamSelector
import com.maleicacid.tvinput.common.TsPid
import com.maleicacid.tvinput.common.TunerKeyToken
import org.junit.Test
import sun.misc.Unsafe
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

// 実controllerの停止通知と解放再試行を同じfixtureで検証し、試験数だけを理由にfixtureを複製しない。
@Suppress("TooManyFunctions", "LargeClass")
class PlaybackFailureCallbacksTest {
    @Suppress("LongMethod")
    @Test
    fun casFilterRejectRollbackKeepsProductionRetryMarker() {
        val executor = ControllerSerialExecutor("maleicacid-tis-controller-test")
        try {
            val fixture =
                executor
                    .submitControl<Fixture> { Fixture(false, false, failCleanup = false) }
                    .get(5, TimeUnit.SECONDS)
            val controller = fixture.allocate(TunerController::class.java)
            val cas = CasController()
            val emmPid = TsPid(0x120)

            fun set(
                name: String,
                value: Any,
            ) {
                TunerController::class.java
                    .getDeclaredField(name)
                    .apply { isAccessible = true }
                    .set(controller, value)
            }

            set("inputId", "test")
            set("sectionExecutor", executor)
            set("tuneAccepted", true)
            set("tuneGeneration", 7L)
            set("tuner", fixture.tuner)
            set("playbackPipeline", fixture.pipeline)
            set("dynamicPmtPids", linkedSetOf<TsPid>())
            set("dynamicEcmPids", linkedSetOf<TsPid>())
            set("dynamicEmmPids", linkedSetOf<TsPid>())
            set("failedDynamicPmtPids", linkedSetOf<TsPid>())
            set("failedDynamicEcmPids", linkedSetOf<TsPid>())
            set("failedDynamicEmmPids", linkedSetOf<TsPid>())
            set("sectionFilterHandles", linkedMapOf<TsPid, TunerController.SectionFilterHandle>())
            set("sectionFilters", linkedMapOf<TsPid, List<Filter>>())
            controller.setCasController(cas)

            val failingFilter = fixture.allocate(Filter::class.java)
            Filter::class.java.getField("configureFailure").setInt(failingFilter, 1)
            Tuner::class.java.getField("nextFilter").set(null, failingFilter)
            Tuner::class.java.getField("openFilterCalls").setInt(null, 0)
            Tuner::class.java.getField("sectionFilterCount").setInt(null, 16)

            val metadata =
                listOf(
                    com.maleicacid.tvinput.aribsi.CaMetadata(
                        null,
                        CasController.SupportedCasSystemIds.ARIB_STD_B25,
                        null,
                        emmPid,
                        null,
                        source = com.maleicacid.tvinput.aribsi.CaMetadataSource.CAT,
                    ),
                )

            check(runCatching { controller.updateCasMetadataAndFilters(metadata, emptySet(), 7L, true) }.isFailure)
            check(Tuner::class.java.getField("openFilterCalls").getInt(null) == 1)

            check(runCatching { controller.updateCasMetadataAndFilters(metadata, emptySet(), 7L, true) }.isFailure)
            check(Tuner::class.java.getField("openFilterCalls").getInt(null) == 1)

            Tuner::class.java.getField("nextFilter").set(null, null)
            cas.close()
        } finally {
            executor.shutdownNow()
        }
    }

    @Suppress("LongMethod")
    @Test
    fun removedFailedPmtRetriesRetainedCleanupBeforeSingleReopen() {
        val executor = ControllerSerialExecutor("maleicacid-tis-controller-test")
        try {
            val fixture =
                executor
                    .submitControl<Fixture> { Fixture(false, false, failCleanup = false) }
                    .get(5, TimeUnit.SECONDS)
            val controller = fixture.allocate(TunerController::class.java)
            val pid = TsPid(0x1004)
            val retainedHandle = TestSectionHandle(pid, rejectClose = true)

            fun set(
                name: String,
                value: Any,
            ) {
                TunerController::class.java
                    .getDeclaredField(name)
                    .apply { isAccessible = true }
                    .set(controller, value)
            }

            val failedPmt = linkedSetOf(pid)
            set("inputId", "test")
            set("sectionExecutor", executor)
            set("tuneAccepted", true)
            set("tuneGeneration", 7L)
            set("tuner", fixture.tuner)
            set("playbackPipeline", fixture.pipeline)
            set("dynamicPmtPids", linkedSetOf<TsPid>())
            set("dynamicEcmPids", linkedSetOf<TsPid>())
            set("dynamicEmmPids", linkedSetOf<TsPid>())
            set("failedDynamicPmtPids", failedPmt)
            set("failedDynamicEcmPids", linkedSetOf<TsPid>())
            set("failedDynamicEmmPids", linkedSetOf<TsPid>())
            set("sectionFilterHandles", linkedMapOf<TsPid, TunerController.SectionFilterHandle>(pid to retainedHandle))
            set("sectionFilters", linkedMapOf<TsPid, List<Filter>>())

            check(runCatching { controller.updatePmtFilters(emptySet(), 7L) }.isFailure)
            check(retainedHandle.closes == 1)
            check(failedPmt.isEmpty())

            retainedHandle.rejectClose = false
            val replacement = fixture.allocate(Filter::class.java)
            Tuner::class.java.getField("nextFilter").set(null, replacement)
            Tuner::class.java.getField("openFilterCalls").setInt(null, 0)
            Tuner::class.java.getField("sectionFilterCount").setInt(null, 16)

            controller.updatePmtFilters(setOf(pid), 7L)

            check(retainedHandle.closes == 2)
            check(Tuner::class.java.getField("openFilterCalls").getInt(null) == 1)
            check(failedPmt.isEmpty())

            controller.closeSectionFilters()
            Tuner::class.java.getField("nextFilter").set(null, null)
        } finally {
            executor.shutdownNow()
        }
    }

    @Test fun invalidatedEcmAndEmmStopFiltersAndPlaybackAndNotifyOriginalGeneration() {
        checkCasInvalidation(failCleanup = false)
    }

    @Test fun casAndFilterCleanupFailuresStillStopPlaybackAndNotifyOriginalGeneration() {
        checkCasInvalidation(failCleanup = true)
    }

    // 実controllerの通知から失効・独立cleanup・再試行までを同じ受信contextで確認する。
    @Test fun casFailureAndReclaimPreserveSiWithoutReportingTunerLoss() {
        for (initializing in listOf(false, true)) checkCasConnectionFailure(initializing)
    }

    @Suppress("LongMethod")
    private fun checkCasConnectionFailure(initializing: Boolean) {
        val executor = ControllerSerialExecutor("maleicacid-tis-controller-test")
        val faults = MediaCas.Faults
        faults.reset()
        val delegate = FrameworkMediaCasBridgeFactory().create(5).getOrThrow()
        lateinit var connectionListener: CasController.ConnectionListener
        val bridge =
            object : CasController.InitializingMediaCasBridge, CasController.MediaCasBridge by delegate {
                override val initializationBudgetMillis = 10L

                override fun elapsedRealtime() = 100L

                override fun initialize(listener: CasController.ConnectionListener) {
                    connectionListener = listener
                }

                override fun scheduleTimeout(
                    delayMillis: Long,
                    action: () -> Unit,
                ): () -> Unit = {}
            }
        val factory =
            object : CasController.MediaCasBridgeFactory {
                override fun create(caSystemId: Int) = Result.success(bridge)
            }
        try {
            CasController(mediaCasFactory = factory).use { cas ->
                val metadata = CasControllerStateTestVectors.pluginSelectionSuccessMetadata()
                cas.updateFromCaMetadata(metadata, 7L)
                if (!initializing) {
                    connectionListener.onCapacity(2)
                    check(cas.updateFromCaMetadata(metadata, 7L).diagnostics.isEmpty())
                }
                val fixture =
                    executor
                        .submitControl<Fixture> { Fixture(false, false, failCleanup = true) }
                        .get(5, TimeUnit.SECONDS)
                val controller = fixture.allocate(TunerController::class.java)
                val ecm = TestSectionHandle(TsPid(0x123), rejectClose = true)
                val emm = TestSectionHandle(TsPid(0x120))
                val pmt = TestSectionHandle(TsPid(0x100))
                val eit = TestSectionHandle(TsPid(0x12))
                var lostGeneration: Long? = null

                fun set(
                    name: String,
                    value: Any,
                ) {
                    TunerController::class.java
                        .getDeclaredField(name)
                        .apply { isAccessible = true }
                        .set(controller, value)
                }
                set("inputId", "test")
                set("sectionExecutor", executor)
                set("tuneAccepted", true)
                set("tuneGeneration", 7L)
                set("playbackPipeline", fixture.pipeline)
                set("captionLanguagesByPid", java.util.concurrent.ConcurrentHashMap<TsPid, String>())
                set("superimposeTimingByPid", java.util.concurrent.ConcurrentHashMap<TsPid, String>())
                set("dynamicPmtPids", linkedSetOf(pmt.pid))
                set("dynamicEcmPids", linkedSetOf(ecm.pid))
                set("dynamicEmmPids", linkedSetOf(emm.pid))
                set("failedDynamicPmtPids", linkedSetOf<TsPid>())
                set("failedDynamicEcmPids", linkedSetOf<TsPid>())
                set("failedDynamicEmmPids", linkedSetOf<TsPid>())
                set("sectionFilterHandles", linkedMapOf(ecm.pid to ecm, emm.pid to emm, pmt.pid to pmt, eit.pid to eit))
                set("sectionFilters", linkedMapOf<TsPid, List<Filter>>())
                controller.setCasController(cas)
                controller.setOnTunerResourceLostCallback { lostGeneration = it }
                faults.pluginFailure = true
                if (initializing) connectionListener.onCapacity(0) else connectionListener.onResourceLost()
                executor.submitControl { cas.onEcmSection(ecm.pid, byteArrayOf(1)) }.get(5, TimeUnit.SECONDS)
                check(lostGeneration == null)
                check(pmt.isOpen)
                check(pmt.closes == 0)
                check(eit.isOpen)
                check(eit.closes == 0)
                check(
                    TunerController::class.java
                        .getDeclaredField("tuneAccepted")
                        .apply { isAccessible = true }
                        .getBoolean(controller),
                )
                check(fixture.notifications == 1)
                check(fixture.failures.single().generation == 7L)
                check(ecm.closes == 1 && emm.closes == 1)
                val attempts = if (initializing) 2 else 1
                check(faults.sessionCloses == 0 && faults.pluginCloses == attempts)
                val expectedError =
                    if (initializing) {
                        CasController.ErrorCode.PLUGIN_UNAVAILABLE
                    } else {
                        CasController.ErrorCode.MEDIA_CAS_RESOURCE_LOST
                    }
                check(cas.lastDiagnostic().errorCode == expectedError)
                check(cas.updateFromCaMetadata(metadata, 7L).ecmPids.isEmpty())
                if (initializing) connectionListener.onCapacity(0) else connectionListener.onResourceLost()
                executor.submitControl { cas.onEcmSection(ecm.pid, byteArrayOf(1)) }.get(5, TimeUnit.SECONDS)
                check(fixture.notifications == 1 && faults.pluginCloses == attempts)
                faults.pluginFailure = false
                cas.clearForResourceLoss()
                ecm.rejectClose = false
                controller.closeSectionFilters()
                fixture.rejectRelease = false
                executor.submitControl { fixture.pipeline.stop() }.get(5, TimeUnit.SECONDS)
                check(faults.pluginCloses == attempts + 1 && faults.sessionCloses == 0)
                check(ecm.closes == 2)
            }
        } finally {
            executor.shutdownNow()
            faults.reset()
        }
    }

    // 同じ配送から所有解放・再生通知までの因果関係を一続きに確認する。
    @Suppress("LongMethod", "CyclomaticComplexMethod")
    private fun checkCasInvalidation(failCleanup: Boolean) {
        val executor = ControllerSerialExecutor("maleicacid-tis-controller-test")
        try {
            executor
                .submit {
                    for (operation in listOf(MediaCas.Operation.ECM, MediaCas.Operation.EMM)) {
                        val faults = MediaCas.Faults
                        faults.reset()
                        CasController().use { cas ->
                            val metadata =
                                CasControllerStateTestVectors.pluginSelectionSuccessMetadata().map {
                                    if (it.emmPid != null) it.copy(emmPid = TsPid(0x120)) else it
                                }
                            cas.updateFromCaMetadata(metadata)
                            val fixture = Fixture(false, false, failCleanup)
                            val controller = fixture.allocate(TunerController::class.java)
                            val pmt = TestSectionHandle(TsPid(0x100))
                            val ecm = TestSectionHandle(TsPid(0x123), failCleanup)
                            val emm = TestSectionHandle(TsPid(0x120))
                            val pmtPids = linkedSetOf(pmt.pid)
                            val ecmPids = linkedSetOf(ecm.pid)
                            val emmPids = linkedSetOf(emm.pid)

                            fun set(
                                name: String,
                                value: Any,
                            ) {
                                TunerController::class.java
                                    .getDeclaredField(name)
                                    .apply { isAccessible = true }
                                    .set(controller, value)
                            }
                            set("inputId", "test")
                            set("sectionExecutor", executor)
                            set("tuneAccepted", true)
                            set("tuneGeneration", 7L)
                            set("casController", cas)
                            set("playbackPipeline", fixture.pipeline)
                            set("dynamicPmtPids", pmtPids)
                            set("dynamicEcmPids", ecmPids)
                            set("dynamicEmmPids", emmPids)
                            set("failedDynamicPmtPids", linkedSetOf<TsPid>())
                            set("failedDynamicEcmPids", linkedSetOf<TsPid>())
                            set("failedDynamicEmmPids", linkedSetOf<TsPid>())
                            set("sectionFilterHandles", linkedMapOf(pmt.pid to pmt, ecm.pid to ecm, emm.pid to emm))
                            set("sectionFilters", linkedMapOf<TsPid, List<Filter>>())
                            faults.invalidateAt = operation
                            faults.pluginFailure = failCleanup
                            val pid = if (operation == MediaCas.Operation.ECM) ecm.pid else emm.pid
                            val result = runCatching { controller.onSection(pid, byteArrayOf(1)) }
                            check(result.isFailure == failCleanup)
                            if (failCleanup) check(requireNotNull(result.exceptionOrNull()).suppressed.size == 2)
                            check(faults.pluginCloses == 1 && faults.sessionCloses == 0)
                            check(ecm.closes == 1 && emm.closes == 1 && pmt.closes == 0)
                            check(pmtPids == setOf(pmt.pid) && emmPids.isEmpty())
                            check(ecmPids.isEmpty() == !failCleanup)
                            check(fixture.pipeline.currentPlaybackGenerationForTest() == 8L)
                            check(fixture.notifications == 1)
                            check(fixture.state == PlaybackStartState.Failed(fixture.signature, 7L))
                            val failure = fixture.failures.single()
                            check(failure.generation == 7L)
                            check(failure.reason == PlaybackPipeline.PlaybackUnavailableReason.CAS_NO_KEY)
                            check(cas.lastDiagnostic().errorCode == CasController.ErrorCode.MEDIA_CAS_INVALIDATED)
                            val calls = faults.calls.toList()
                            controller.onSection(pid, byteArrayOf(1))
                            check(faults.calls == calls && fixture.notifications == 1)
                            faults.pluginFailure = false
                            cas.clearForResourceLoss()
                            ecm.rejectClose = false
                            controller.closeSectionFilters()
                            fixture.rejectRelease = false
                            fixture.pipeline.stop()
                            check(!fixture.cleanup.hasPending)
                            check(cas.updateFromCaMetadata(metadata).diagnostics.isEmpty() && faults.creates == 2)
                        }
                    }
                }.get(10, TimeUnit.SECONDS)
        } finally {
            executor.shutdownNow()
        }
    }

    private class TestSectionHandle(
        override val pid: TsPid,
        var rejectClose: Boolean = false,
    ) : TunerController.SectionFilterHandle {
        override var isOpen = true
        var closes = 0

        override fun close() {
            closes++
            isOpen = false
            check(!rejectClose) { "CAS filter閉鎖の失敗" }
        }
    }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    // 候補の処理と入れ子の資源寿命を同じ手順内で確認できる構造を保つ。
    @Suppress("MaxLineLength", "NestedBlockDepth")
    @Test
    fun avAndCaptionFilterFactoriesRetainFailedConfigurationAndStartCleanup() {
        for (kind in listOf("video", "audio", "subtitle", "superimpose")) {
            for (phase in listOf("configure", "start")) {
                for (failure in listOf(1, 2)) {
                    val fixture = Fixture(false, false, failCleanup = false)
                    val filter = fixture.allocate(Filter::class.java)

                    fun set(
                        name: String,
                        value: Any,
                    ) {
                        Filter::class.java.getField(name).set(filter, value)
                    }

                    fun count(name: String) = Filter::class.java.getField(name).getInt(filter)
                    Tuner::class.java.getField("nextFilter").set(null, filter)
                    set("${phase}Failure", failure)
                    set("rejectClose", true)
                    val result =
                        if (kind == "video" || kind == "audio") {
                            fixture.invoke(
                                "createAndStartAvFilter",
                                fixture.tuner,
                                requireNotNull(if (kind == "audio") fixture.selection.audio else fixture.selection.video),
                                kind == "audio",
                            )
                        } else {
                            val stream =
                                AribElementaryStream(
                                    TsPid(0x103),
                                    0x06,
                                    null,
                                    null,
                                    null,
                                    isCaption = kind == "subtitle",
                                    isSuperimpose = kind == "superimpose",
                                )
                            fixture.invoke("createAndStartCaptionPesFilter", fixture.tuner, stream, "test", kind == "superimpose")
                        }
                    check(result !is Filter)
                    check(count("configurations") == 1 && count("starts") == if (phase == "start") 1 else 0)
                    check(count("closes") == 1 && fixture.cleanup.hasPending)
                    check(
                        PlaybackPipeline::class.java
                            .getDeclaredField("${kind}Filter")
                            .apply { isAccessible = true }
                            .get(fixture.pipeline) ==
                            null,
                    )
                    set("rejectClose", false)
                    fixture.pipeline.stop()
                    check(count("closes") == 2 && !fixture.cleanup.hasPending)
                    Tuner::class.java.getField("nextFilter").set(null, null)
                }
            }
        }
    }

    @Test fun mediaSyncAudioFailureRetainsResourcesAndNotifiesOriginalSession() {
        for (waiting in listOf(false, true)) {
            for (audioOnly in listOf(false, true)) {
                val fixture = Fixture(waiting, audioOnly)
                val sync = MediaSync()
                fixture.set("mediaSync", sync)
                fixture.invoke("handleMediaSyncError", sync, 7L, MediaSync.MEDIASYNC_ERROR_AUDIOTRACK_FAIL, 0)
                fixture.checkTerminalCleanupFailure()
            }
        }
    }

    @Test fun audioFailureAndRouteOrFormatRestartShareTerminalCleanupHandling() {
        for (waiting in listOf(false, true)) {
            for (reason in listOf(
                PlaybackPipeline.PlaybackUnavailableReason.AUDIO_UNAVAILABLE,
                PlaybackPipeline.PlaybackUnavailableReason.UNSUPPORTED_AUDIO_STREAM,
            )) {
                val fixture = Fixture(waiting, false)
                fixture.invoke("handleAudioFailure", reason, "audio output/configuration/deadline failure", false)
                fixture.checkTerminalCleanupFailure()
            }
            val fixture = Fixture(waiting, false)
            fixture.invoke("restartCurrentPlaybackGeneration", 7L)
            fixture.checkTerminalCleanupFailure()
        }
    }

    @Test fun requestedStartAndAudioSwitchReturnCleanupFailureToTheirCaller() {
        for (switchAudio in listOf(false, true)) {
            val fixture = Fixture(false, false)
            val generation: Long
            val diagnostics: List<String>
            if (switchAudio) {
                val result = fixture.pipeline.switchAudio(fixture.tuner, fixture.selection)
                check(!result.switchedAudio && !result.firstFramePending)
                generation = result.generation
                diagnostics = result.diagnostics
            } else {
                val result = fixture.pipeline.start(fixture.tuner, fixture.channel, fixture.selection)
                check(!result.startedVideo && !result.startedAudio && !result.firstFramePending)
                generation = result.generation
                diagnostics = result.diagnostics
            }
            check(generation == 8L && diagnostics.any { it.contains("injected filter") })
            check(fixture.failures.isEmpty() && fixture.notifications == 0)
            val next =
                PlaybackStartTransitions.afterRestartResult(
                    fixture.state,
                    fixture.signature,
                    generation,
                    firstOutputPending = false,
                    started = false,
                )
            MaleicacidLiveSession.commitPlaybackStartResult(next, { fixture.state = it }) {
                check(fixture.state == PlaybackStartState.Failed(fixture.signature, generation))
                fixture.notifications++
            }
            check(fixture.notifications == 1)
            fixture.checkRetainedThenReleased()
        }
    }

    @Test fun realCasLinkageReevaluatesLiveAndReachesGenericPlaybackStart() {
        MediaCas.Faults.reset()
        val executor = ControllerSerialExecutor("maleicacid-tis-controller-test")
        val factory =
            object : CasController.MediaCasBridgeFactory {
                override fun create(caSystemId: Int) =
                    Result.success(
                        FrameworkMediaCasBridge({ MediaCas(caSystemId) }, true),
                    )
            }
        try {
            CasController(mediaCasFactory = factory).use { cas ->
                val fixture = executor.submitControl<Fixture> { Fixture(false, false, false) }.get(5, TimeUnit.SECONDS)
                val controller = fixture.allocate(TunerController::class.java)

                fun set(
                    name: String,
                    value: Any,
                ) {
                    TunerController::class.java
                        .getDeclaredField(name)
                        .apply { isAccessible = true }
                        .set(controller, value)
                }
                set("inputId", "test")
                set("sectionExecutor", executor)
                set("tuneAccepted", true)
                set("tuneGeneration", 7L)
                set("currentTune", fixture.channel)
                set("tuner", fixture.tuner)
                set("playbackPipeline", fixture.pipeline)
                set("superimposeTimingByPid", java.util.concurrent.ConcurrentHashMap<TsPid, String>())
                controller.setCasController(cas)
                val decisions = mutableListOf<Boolean>()
                val policy = ServicePolicyDecision(fixture.channel.serviceKey, true, true, true, emptyList())
                controller.setOnSectionIngestedCallback {
                    val ready = cas.isServiceDescramblingReady(fixture.channel.serviceKey, 7L)
                    decisions += policy.livePlaybackEligible(ready)
                }
                val metadata =
                    listOf(
                        com.maleicacid.tvinput.aribsi.CaMetadata(
                            fixture.channel.serviceKey,
                            5,
                            TsPid(0x123),
                            null,
                            TsPid(0x101),
                        ),
                    )
                val descrambler =
                    object : CasController.TunerDescramblerBridge {
                        override fun setKeyToken(keyToken: TunerKeyToken) = Result.success(Unit)

                        override fun addPid(elementaryPid: TsPid) = Result.success(Unit)

                        override fun removePid(elementaryPid: TsPid) = Result.success(Unit)

                        override fun close() = Unit
                    }
                cas.updateFromCaMetadata(metadata, 7L) { descrambler }
                check(controller.startPlayback(fixture.selection, requiresCas = true, generation = 7L) == null)
                check(fixture.pipeline.currentPlaybackGenerationForTest() == 7L)
                cas.onEcmSection(TsPid(0x123), byteArrayOf(1))
                executor.submitControl {}.get(5, TimeUnit.SECONDS)
                check(decisions == listOf(true))
                val result = controller.startPlayback(fixture.selection, requiresCas = true, generation = 7L)
                check(result != null && result.generation > 7L)
                check(fixture.failures.single().reason == PlaybackPipeline.PlaybackUnavailableReason.SURFACE_NOT_SET)
                // CAS gate通過後は既存pipelineがSurface不足を判定する。再生成功の捏造はしない。
                check(!result.startedVideo && !result.firstFramePending)
                check(controller.startPlayback(fixture.selection, requiresCas = true, generation = 6L) == null)
                cas.clearForResourceLoss()
                check(controller.startPlayback(fixture.selection, requiresCas = true, generation = 7L) == null)
            }
        } finally {
            executor.shutdownNow()
        }
    }

    @Test fun audioOnlyStopsBeforeNotifyingAndFailedVideoOnlyResultReachesSession() {
        for (audioOnly in listOf(false, true)) {
            val fixture = Fixture(false, audioOnly, failCleanup = false)
            fixture.invoke(
                "handleAudioFailure",
                PlaybackPipeline.PlaybackUnavailableReason.AUDIO_UNAVAILABLE,
                "audio output failure",
                audioOnly,
            )
            check(fixture.notifications == 1 && fixture.state is PlaybackStartState.Failed)
            if (audioOnly) {
                check(fixture.restarts.isEmpty())
                check(fixture.failures.single().generation == 7L)
                check(fixture.failures.single().reason == PlaybackPipeline.PlaybackUnavailableReason.AUDIO_UNAVAILABLE)
                check(fixture.pipeline.currentPlaybackGenerationForTest() == 8L)
            } else {
                val restart = fixture.restarts.single()
                check(restart.videoOnly && restart.originGeneration == 7L && restart.result.generation == 9L)
                check(!restart.result.startedVideo && !restart.result.firstFramePending)
                check(PlaybackStartTransitions.signature(fixture.state)?.audioPid == null)
                check(PlaybackStartTransitions.pipelineGeneration(fixture.state) == 9L)
            }
        }
    }

    @Test fun staleMediaSyncAndRestartCallbacksDoNotTouchCurrentGeneration() {
        val fixture = Fixture(false, false)
        val sync = MediaSync()
        fixture.set("mediaSync", sync)
        fixture.invoke("handleMediaSyncError", sync, 6L, MediaSync.MEDIASYNC_ERROR_AUDIOTRACK_FAIL, 0)
        fixture.invoke("handleMediaSyncError", MediaSync(), 7L, MediaSync.MEDIASYNC_ERROR_AUDIOTRACK_FAIL, 0)
        fixture.invoke("restartCurrentPlaybackGeneration", 6L)
        check(fixture.notifications == 0 && fixture.failures.isEmpty() && fixture.restarts.isEmpty())
        check(fixture.pipeline.currentPlaybackGenerationForTest() == 7L && fixture.releaseAttempts == 1)
        fixture.rejectRelease = false
        fixture.cleanup.retry()
        fixture.cleanup.requireComplete()
    }

    // 実JNIのPAT/SDTから本番session refreshとcontrollerのFilter開始までを通す。
    @Test
    fun pendingLiveSiBootstrapsPmtFilterAndBecomesReadyAfterPmtReception() {
        LiveSiFixture().use { live ->
            live.ingestInitialTables()
            check(live.decision().state == com.maleicacid.tvinput.aribsi.ServicePolicyState.PENDING)
            live.refresh()
            check(Filter::class.java.getField("starts").getInt(live.filter) == 1)
            live.refresh()
            check(Filter::class.java.getField("starts").getInt(live.filter) == 1)
            live.ingestPmt()
            val ready = live.decision()
            check(ready.state == com.maleicacid.tvinput.aribsi.ServicePolicyState.READY && ready.casDecisionReady)
            check(live.pendingNotifications.isEmpty())
        }
    }

    // 同じ実JNI失効からCAS・再生停止・通知までを一つの回帰で観測する。
    @Suppress("LongMethod")
    @Test
    fun conflictingPmtStopsEstablishedPlaybackAndRetiresCasMetadata() {
        for (playing in listOf(true, false)) {
            LiveSiFixture().use { live ->
                live.ingestInitialTables()
                live.refresh()
                live.ingestPmt()
                check(live.decision().state == com.maleicacid.tvinput.aribsi.ServicePolicyState.READY)
                live.set(live.session, "latestLiveSnapshot", live.engine.livePlaybackSnapshot())
                val signature =
                    AvPlaybackSignature(live.key, TsPid(0x101), TsPid(0x101), 0x1b, TsPid(0x102), 0x0f, true, false)
                val previousState = if (playing) PlaybackStartState.Started(signature, 7L) else PlaybackStartState.Idle
                live.set(live.session, "playbackState", previousState)
                val emm = TsPid(0x123)
                val metadata =
                    listOf(
                        com.maleicacid.tvinput.aribsi.CaMetadata(
                            null,
                            5,
                            null,
                            emm,
                            null,
                            source = com.maleicacid.tvinput.aribsi.CaMetadataSource.CAT,
                        ),
                    )
                val emmFilter = live.playback.allocate(Filter::class.java)
                Tuner::class.java.getField("nextFilter").set(null, emmFilter)
                val result = live.controller.updateCasMetadataAndFilters(metadata, setOf(TsPid(0x100)), 7L, true)
                check(result?.emmPids == setOf(emm))
                check(MediaCas.Faults.creates == 1 && MediaCas.Faults.pluginCloses == 0)
                // 同一version・同一番号のPCR PIDだけが矛盾するCRC付きPMT。
                live.ingest(0x100, "02b0170001c10000e102f0001be101f0000fe102f000a3052165")
                check(live.decision().state == com.maleicacid.tvinput.aribsi.ServicePolicyState.PENDING)
                live.refresh()
                check(live.state() == PlaybackStartState.Stopped)
                check(live.playback.pipeline.currentPlaybackGenerationForTest() == 8L)
                check(MediaCas.Faults.pluginCloses == 1)
                check(live.dynamicPids("dynamicEcmPids").isEmpty() && live.dynamicPids("dynamicEmmPids").isEmpty())
                check(live.dynamicPids("dynamicPmtPids") == setOf(TsPid(0x100)))
                check(Filter::class.java.getField("closes").getInt(emmFilter) == 1)
                check(Filter::class.java.getField("closes").getInt(live.filter) == 0)
                check(live.pendingNotifications.size == 1)
                live.ingestPmt()
                check(live.decision().state == com.maleicacid.tvinput.aribsi.ServicePolicyState.PENDING)
                check(live.state() == PlaybackStartState.Stopped)
            }
        }
    }

    // 既存の初回SI試験と失効試験で同じ実JNI・controller fixtureを共用する。
    // Androidの通知は未初期化Sessionの保留listで観測し、字幕native/UI資源は既に閉じた境界とする。
    private class LiveSiFixture : AutoCloseable {
        private val executor = ControllerSerialExecutor("live SI失効試験")
        val playback = executor.submitControl { Fixture(false, false, failCleanup = false) }.get(5, TimeUnit.SECONDS)
        val controller = playback.allocate(TunerController::class.java)
        val session = playback.allocate(MaleicacidLiveSession::class.java)
        val engine =
            com.maleicacid.tvinput.aribsi
                .AribSiEngine(android.content.ContextWrapper(null))
        val key = ServiceKey(0x22, 0x11, 1)
        val filter = playback.allocate(Filter::class.java)
        val pendingNotifications = mutableListOf<Runnable>()
        private val cas = CasController()
        private val captionExecutors = mutableListOf<LifecycleSerialExecutor>()

        init {
            MediaCas.Faults.reset()
            set(controller, "inputId", "test")
            set(controller, "sectionExecutor", executor)
            set(controller, "tuneAccepted", true)
            set(controller, "tuneGeneration", 7L)
            set(controller, "tuner", playback.tuner)
            set(controller, "playbackPipeline", playback.pipeline)
            for (name in listOf(
                "dynamicPmtPids",
                "dynamicEcmPids",
                "dynamicEmmPids",
                "failedDynamicPmtPids",
                "failedDynamicEcmPids",
                "failedDynamicEmmPids",
            )) {
                set(controller, name, linkedSetOf<TsPid>())
            }
            set(controller, "sectionFilterHandles", linkedMapOf<TsPid, TunerController.SectionFilterHandle>())
            set(controller, "sectionFilters", linkedMapOf<TsPid, List<Filter>>())
            controller.setCasController(cas)
            set(session, "currentService", key)
            set(session, "currentGeneration", 7L)
            set(session, "playbackState", PlaybackStartState.Idle)
            set(session, "tunerController", controller)
            set(session, "aribSiEngine", engine)
            set(session, "caMapper", PmtCatCaMetadataMapper())
            for (name in listOf("captionController", "superimposeController")) {
                val caption = playback.allocate(AribCaptionController::class.java)
                set(caption, "released", AtomicBoolean(true))
                val captionExecutor = LifecycleSerialExecutor("closed caption fixture")
                captionExecutors += captionExecutor
                set(caption, "executor", captionExecutor)
                set(session, name, caption)
            }
            val sessionType = android.media.tv.TvInputService.Session::class.java
            sessionType.getDeclaredField("mLock").apply { isAccessible = true }.set(session, Any())
            sessionType
                .getDeclaredField("mPendingActions")
                .apply { isAccessible = true }
                .set(session, pendingNotifications)
            Tuner::class.java.getField("nextFilter").set(null, filter)
            Tuner::class.java.getField("sectionFilterCount").setInt(null, 16)
            engine.setDiscoveryProfile(com.maleicacid.tvinput.aribsi.SiDiscoveryProfile.ISDB_T)
        }

        fun ingestInitialTables() {
            ingest(0, "00b00d0011c100000001e1004521f9b6")
            ingest(0x11, "42f0180011c100000022000001fc80074805010002543128d78c81")
            ingest(0x10, "40b01c0022c10000f004fe020300f00b00110022f0054103000101ab293465")
        }

        fun ingestPmt() = ingest(0x100, "02b0170001c10000e101f0001be101f0000fe102f0009e28c6dd")

        fun ingest(
            pid: Int,
            hex: String,
        ) {
            val bytes = hex.chunked(2).map { it.toInt(16).toByte() }.toByteArray()
            check(engine.ingestSection(TsPid(pid), bytes).status == com.maleicacid.tvinput.aribsi.SiStatus.OK)
        }

        fun decision() =
            com.maleicacid.tvinput.aribsi.ServicePolicyEvaluator
                .evaluateLive(engine.livePlaybackSnapshot(), key)

        fun refresh() {
            MaleicacidLiveSession::class.java
                .getDeclaredMethod("refreshDynamicSiAndCasFilters")
                .apply { isAccessible = true }
                .invoke(session)
        }

        fun state() =
            MaleicacidLiveSession::class.java
                .getDeclaredField("playbackState")
                .apply { isAccessible = true }
                .get(session)

        fun dynamicPids(name: String) =
            TunerController::class.java
                .getDeclaredField(name)
                .apply { isAccessible = true }
                .get(controller) as Set<*>

        fun set(
            target: Any,
            name: String,
            value: Any,
        ) {
            target.javaClass
                .getDeclaredField(name)
                .apply { isAccessible = true }
                .set(target, value)
        }

        override fun close() {
            try {
                controller.closeSectionFilter(TsPid(0x100))
                executor.submitControl { cas.close() }.get(5, TimeUnit.SECONDS)
            } finally {
                Tuner::class.java.getField("nextFilter").set(null, null)
                engine.close()
                executor.shutdownNow()
                PlaybackPipeline::class.java
                    .getDeclaredField("executor")
                    .apply { isAccessible = true }
                    .get(playback.pipeline)
                    .let { (it as? java.util.concurrent.ExecutorService)?.shutdownNow() }
                captionExecutors.forEach { it.shutdownNow() }
                MediaCas.Faults.reset()
            }
        }
    }

    // 恒等写像ではなく、本番候補loop・実tune拒否・typed scan終端を通す。
    @Suppress("LongMethod")
    @Test
    fun synchronousTuneFailureStopsRemainingInitialScanCandidates() {
        val executor = ControllerSerialExecutor("scan同期選局拒否試験")
        val fixture = executor.submitControl { Fixture(false, false, failCleanup = false) }.get(5, TimeUnit.SECONDS)
        val controller = fixture.allocate(TunerController::class.java)
        val scan = fixture.allocate(ChannelScanController::class.java)
        val engine =
            com.maleicacid.tvinput.aribsi
                .AribSiEngine(android.content.ContextWrapper(null))

        fun set(
            target: Any,
            name: String,
            value: Any,
        ) {
            target.javaClass
                .getDeclaredField(name)
                .apply { isAccessible = true }
                .set(target, value)
        }
        set(controller, "sectionExecutor", executor)
        set(controller, "inputId", "input.test")
        set(controller, "playbackPipeline", fixture.pipeline)
        for (name in listOf(
            "dynamicPmtPids",
            "dynamicEcmPids",
            "dynamicEmmPids",
            "failedDynamicPmtPids",
            "failedDynamicEcmPids",
            "failedDynamicEmmPids",
        )) {
            set(controller, name, linkedSetOf<TsPid>())
        }
        for (name in listOf("captionLanguagesByPid", "captionFactParsers", "superimposeTimingByPid")) {
            set(controller, name, java.util.concurrent.ConcurrentHashMap<TsPid, Any>())
        }
        set(controller, "sectionFilterHandles", linkedMapOf<TsPid, TunerController.SectionFilterHandle>())
        set(controller, "sectionFilters", linkedMapOf<TsPid, List<Filter>>())
        // 未初期化のTunerは利用不可を同期返却する。実tuneForScan経路を差し替えない。
        set(scan, "engine", engine)
        set(scan, "tunerController", controller)
        set(scan, "cancelled", AtomicBoolean(false))
        set(scan, "scanGenerationFence", ChannelScanController.ScanGenerationFence())
        set(
            scan,
            "tvProviderWriter",
            TvProviderWriter(
                "input.test",
                object : TvProviderWriter.ChannelStore {
                    // 標準整形後に残る型付きstore契約の宣言だけ行長を許容する。
                    @Suppress("MaxLineLength")
                    override fun indexExistingChannelIds(keys: Set<ServiceKey>): Result<Map<ServiceKey, Long>> = Result.success(emptyMap())

                    override fun insertChannel(values: ContentValues): Result<Long?> = Result.success(null)

                    override fun updateChannel(
                        channelId: Long,
                        values: ContentValues,
                    ): Result<Int> = Result.success(0)
                },
                testOnly = true,
            ),
        )
        val candidates = JapanIsdbScanPlan.defaultInitialScan().take(3)
        try {
            val result = scan.startInitialScan(candidates)
            val terminal = result.terminal.outcome
            check(terminal == ChannelScanController.ScanTerminalOutcome.TUNE_REJECTED) { result.toString() }
            check(result.scanned == 1 && result.successfulCandidates == 0 && result.published == 0)
            check(result.diagnostics.single().candidate == candidates.first())
        } finally {
            engine.close()
            executor.shutdownNow()
        }
    }

    // 同一ownerの初期化・実資源停止・失敗保持を一続きの反例で検査する。
    @Suppress("LongMethod")
    @Test
    fun acceptedTuneFailureFencesOldPlaybackAndRetainsCleanupDespiteNotificationFailure() {
        val controllerExecutor = ControllerSerialExecutor("accepted-tune-controller-test")
        val sessionExecutor = LifecycleSerialExecutor("accepted-tune-session-test")
        val fixture = controllerExecutor.submit<Fixture> { Fixture(false, false) }.get(5, TimeUnit.SECONDS)
        try {
            val controller = fixture.allocate(TunerController::class.java)
            val session = fixture.allocate(MaleicacidLiveSession::class.java)

            fun set(
                target: Any,
                name: String,
                value: Any,
            ) {
                target.javaClass
                    .getDeclaredField(name)
                    .apply { isAccessible = true }
                    .set(target, value)
            }
            set(controller, "sectionExecutor", controllerExecutor)
            set(controller, "playbackPipeline", fixture.pipeline)
            set(controller, "tuneAccepted", true)
            set(controller, "currentTune", fixture.channel)
            for (name in listOf("failedDynamicPmtPids", "failedDynamicEcmPids", "failedDynamicEmmPids")) {
                set(controller, name, linkedSetOf<TsPid>())
            }
            for (name in listOf("captionLanguagesByPid", "captionFactParsers", "superimposeTimingByPid")) {
                set(controller, name, java.util.concurrent.ConcurrentHashMap<TsPid, Any>())
            }
            set(controller, "sectionFilters", linkedMapOf<TsPid, Any>())
            set(controller, "sectionFilterHandles", linkedMapOf<TsPid, Any>())
            val released = AtomicBoolean(false)
            set(session, "releaseOnce", released)
            set(session, "sessionExecutor", sessionExecutor)
            set(session, "tuneRequestLock", Any())
            set(session, "tunerController", controller)
            set(session, "playbackState", fixture.state)
            // 字幕ownerとFramework通知を未初期化にし、両方の失敗後も実playback停止を検査する。
            val engine =
                com.maleicacid.tvinput.aribsi
                    .AribSiEngine(android.content.ContextWrapper(null))
            val parser =
                engine.javaClass
                    .getDeclaredField("nativeParser")
                    .apply { isAccessible = true }
                    .get(engine) as com.maleicacid.tvinput.aribsi.NativeAribSiParser
            parser.close()
            set(parser, "handle", Long.MAX_VALUE)
            set(session, "aribSiEngine", engine)
            val primary = checkNotNull(runCatching { engine.reset() }.exceptionOrNull())
            check(primary is com.maleicacid.tvinput.aribsi.NativeParserCleanupException)
            val handler =
                MaleicacidLiveSession::class.java
                    .getDeclaredMethod(
                        "handleAcceptedTuneFailure",
                        android.net.Uri::class.java,
                        Throwable::class.java,
                    ).apply { isAccessible = true }
            sessionExecutor.callControl(5_000L) {
                handler.invoke(session, android.net.Uri.parse("content://android.media.tv/channel/2"), primary)
            }
            check(released.get())
            check(
                !controller.javaClass
                    .getDeclaredField("tuneAccepted")
                    .apply { isAccessible = true }
                    .getBoolean(controller),
            )
            check(fixture.pipeline.currentPlaybackGenerationForTest() != 7L)
            check(fixture.cleanup.hasPending)
            check(primary.suppressed.size >= 2)
            val pending =
                MaleicacidLiveSession::class.java
                    .getDeclaredField("releaseCleanup")
                    .apply { isAccessible = true }
                    .get(session) as ResourceCleanup
            check(pending.hasPending)
            check(!sessionExecutor.isShutdown)
            check(!session.onTune(android.net.Uri.parse("content://android.media.tv/channel/3")))
        } finally {
            fixture.rejectRelease = false
            runCatching { fixture.pipeline.release() }
            check(!fixture.cleanup.hasPending)
            controllerExecutor.shutdownNow()
            sessionExecutor.shutdownNow()
        }
    }

    @Test
    fun releaseRetainsOwnerWhenDiscardedInputReleaseFailsThenCompletesOnRetry() {
        val fixture = Fixture(false, false, false)
        val executor = LifecycleSerialExecutor("未実行event解放試験")
        fixture.set("executor", executor)
        val started = java.util.concurrent.CountDownLatch(1)
        val unblock = java.util.concurrent.CountDownLatch(1)
        val firstDone = java.util.concurrent.CountDownLatch(1)
        val retryDone = java.util.concurrent.CountDownLatch(1)
        var attempts = 0
        val activePipelines =
            ChannelScanManager::class.java
                .getDeclaredField("activePlaybackPipelines")
                .apply { isAccessible = true }
                .get(ChannelScanManager) as java.util.concurrent.atomic.AtomicInteger
        val baseline = activePipelines.get()
        val registration =
            PlaybackPipeline::class.java
                .getDeclaredField("resourceActivityReported")
                .apply { isAccessible = true }
        ChannelScanManager.registerPlaybackPipeline()
        fixture.set("resourceActivityReported", true)
        try {
            executor.executeControl {
                started.countDown()
                unblock.await()
            }
            check(started.await(5, TimeUnit.SECONDS))
            executor.executeCallback(
                isReleased = { false },
                onFailure = { throw it },
                onDiscard = {
                    fixture.cleanup.release("未実行MediaEvent") {
                        attempts++
                        check(attempts > 1) { "MediaEvent解放失敗" }
                    }
                },
            ) { error("release後に未実行入力を処理しました") }
            executor.executeControl {
                runCatching { fixture.pipeline.release() }
                firstDone.countDown()
            }
            unblock.countDown()
            check(firstDone.await(5, TimeUnit.SECONDS))
            check(attempts == 1 && fixture.cleanup.hasPending && !executor.isShutdown)
            check(activePipelines.get() == baseline + 1 && registration.getBoolean(fixture.pipeline))
            executor.executeControl {
                // Unsafe fixtureのcallback threadは未初期化。停止後のNPEは試験対象外。
                runCatching { fixture.pipeline.release() }
                retryDone.countDown()
            }
            check(retryDone.await(5, TimeUnit.SECONDS))
            check(attempts == 2 && !fixture.cleanup.hasPending && executor.isShutdown)
            check(activePipelines.get() == baseline && !registration.getBoolean(fixture.pipeline))
        } finally {
            if (registration.getBoolean(fixture.pipeline)) ChannelScanManager.unregisterPlaybackPipeline(null)
            unblock.countDown()
            executor.shutdownNow()
        }
    }

    // 本番callback/readから実JNIまで、正常集合・有限飽和・retune失効を同じfixtureで検査する。
    @Suppress("LongMethod")
    @Test
    fun sectionCallbackBurstCompletesSiAndReportsFiniteAdmissionLoss() {
        val executor = ControllerSerialExecutor("section burst試験", maxPendingDataTasks = 16)
        val fixture = executor.submitControl { Fixture(false, false, failCleanup = false) }.get(5, TimeUnit.SECONDS)
        val controller = fixture.allocate(TunerController::class.java)
        val engine =
            com.maleicacid.tvinput.aribsi
                .AribSiEngine(android.content.ContextWrapper(null))
        val ingest =
            com.maleicacid.tvinput.aribsi
                .SectionIngestController(engine)
        var releaseOwner = java.util.concurrent.CountDownLatch(1)

        fun set(
            name: String,
            value: Any,
        ) {
            TunerController::class.java
                .getDeclaredField(name)
                .apply { isAccessible = true }
                .set(controller, value)
        }

        fun holdOwner(): java.util.concurrent.CountDownLatch {
            val started = java.util.concurrent.CountDownLatch(1)
            releaseOwner = java.util.concurrent.CountDownLatch(1)
            executor.executeControl {
                started.countDown()
                check(releaseOwner.await(5, TimeUnit.SECONDS))
            }
            check(started.await(5, TimeUnit.SECONDS))
            return releaseOwner
        }
        val eventConstructor =
            android.media.tv.tuner.filter.SectionEvent::class.java
                .getDeclaredConstructor(
                    Int::class.javaPrimitiveType,
                    Int::class.javaPrimitiveType,
                    Int::class.javaPrimitiveType,
                    Long::class.javaPrimitiveType,
                ).apply { isAccessible = true }

        fun deliver(
            filter: Filter,
            hex: Array<String>,
        ) {
            val payloads = hex.map { h -> h.chunked(2).map { it.toInt(16).toByte() }.toByteArray() }
            Filter::class.java.getField("sectionPayloads").set(filter, payloads.toTypedArray())
            Filter::class.java.getField("reads").setInt(filter, 0)
            val events = payloads.map { bytes -> eventConstructor.newInstance(0, 0, 0, bytes.size.toLong()) }
            val before = System.nanoTime()
            check(
                Filter::class.java
                    .getMethod("deliver", Array<android.media.tv.tuner.filter.FilterEvent>::class.java)
                    .invoke(filter, events.toTypedArray()) == true,
            )
            check(TimeUnit.NANOSECONDS.toMillis(System.nanoTime() - before) < 1_000L)
        }
        val pat0 = "00b00d0011c100010001e1000c2c9e3b"
        val pat1 = "00b00d0011c101010000e01088d42568"
        val filters = linkedMapOf<Int, Filter>()
        try {
            set("inputId", "test")
            set("sectionExecutor", executor)
            set("tuneAccepted", true)
            set("tuneGeneration", 7L)
            set("tuner", fixture.tuner)
            set("sectionIngestController", ingest)
            set("sectionFilterHandles", linkedMapOf<TsPid, TunerController.SectionFilterHandle>())
            set("sectionFilters", linkedMapOf<TsPid, List<Filter>>())
            set("dynamicPmtPids", linkedSetOf(TsPid(0x100)))
            set("dynamicEcmPids", linkedSetOf<TsPid>())
            set("dynamicEmmPids", linkedSetOf<TsPid>())
            Tuner::class.java.getField("sectionFilterCount").setInt(null, 16)
            for (pid in listOf(0, 0x10, 0x11, 0x100)) {
                val filter = fixture.allocate(Filter::class.java)
                Tuner::class.java.getField("nextFilter").set(null, filter)
                check(controller.openSectionFilter(TsPid(pid), 7L).isOpen)
                filters[pid] = filter
            }
            val received = java.util.concurrent.CountDownLatch(5)
            controller.setOnSectionIngestedCallback { received.countDown() }
            val owner = holdOwner()
            deliver(filters.getValue(0), arrayOf(pat0, pat1))
            deliver(filters.getValue(0x10), arrayOf("40b01c0022c10000f004fe020300f00b00110022f0054103000101ab293465"))
            deliver(filters.getValue(0x11), arrayOf("42f0180011c100000022000001fc80074805010002543128d78c81"))
            deliver(filters.getValue(0x100), arrayOf("02b0170001c10000e101f0001be101f0000fe102f0009e28c6dd"))
            check(ingest.inputDeliveryLossCount == 0)
            owner.countDown()
            check(received.await(5, TimeUnit.SECONDS))
            check(ingest.diagnostics().sumOf { it.acceptedCount } == 5)
            check(engine.pmtPidsForSectionFilters() == setOf(TsPid(0x100)))
            check(
                com.maleicacid.tvinput.aribsi.ServicePolicyEvaluator
                    .evaluateLive(engine.livePlaybackSnapshot(), ServiceKey(0x22, 0x11, 1))
                    .registrationReady,
            )
            val saturated = java.util.concurrent.CountDownLatch(16)
            controller.setOnSectionIngestedCallback { saturated.countDown() }
            val stalled = holdOwner()
            deliver(filters.getValue(0), Array(17) { pat0 })
            check(ingest.inputDeliveryLossCount == 1 && ingest.diagnosticSummary().contains("inputDeliveryLoss=1"))
            stalled.countDown()
            check(saturated.await(5, TimeUnit.SECONDS))
            val beforeStale = ingest.diagnostics().sumOf { it.acceptedCount }
            val staleOwner = holdOwner()
            deliver(filters.getValue(0), arrayOf(pat1))
            val retune = executor.submitControl { set("tuneGeneration", 8L) }
            staleOwner.countDown()
            retune.get(5, TimeUnit.SECONDS)
            val dataFinished = java.util.concurrent.CountDownLatch(1)
            executor.executeData { dataFinished.countDown() }
            check(dataFinished.await(5, TimeUnit.SECONDS))
            check(ingest.diagnostics().sumOf { it.acceptedCount } == beforeStale)
        } finally {
            releaseOwner.countDown()
            filters.keys.forEach { controller.closeSectionFilter(TsPid(it)) }
            Tuner::class.java.getField("nextFilter").set(null, null)
            engine.close()
            executor.shutdownNow()
        }
    }

    private class Fixture(
        waiting: Boolean,
        audioOnly: Boolean,
        failCleanup: Boolean = true,
    ) {
        // Androidのthread/native初期化だけを省く。本番の停止・開始・エラー処理を直接実行する。
        private val unsafe =
            Unsafe::class.java.getDeclaredField("theUnsafe").run {
                isAccessible = true
                get(null) as Unsafe
            }
        val pipeline = unsafe.allocateInstance(PlaybackPipeline::class.java) as PlaybackPipeline
        val cleanup = ResourceCleanup()
        val tuner = unsafe.allocateInstance(Tuner::class.java) as Tuner
        private val key = ServiceKey(4, 0x4010, 101)
        val channel =
            TunerController.ResolvedChannel(
                null,
                "test",
                key,
                if (audioOnly) 2 else 1,
                "test",
                "1",
                "ISDB_T",
                FrequencyHz(473_000_000L),
                StreamSelector.NONE,
                null,
                null,
            )
        private val video = AribElementaryStream(TsPid(0x101), 0x1b, null, null, null)
        private val audio = AribElementaryStream(TsPid(0x102), 0x0f, null, null, null, codec = "AAC")
        val selection = TunerController.AvStreamSelection(key, TsPid(0x100), if (audioOnly) null else video, audio)
        val signature =
            AvPlaybackSignature(
                key,
                TsPid(0x100),
                selection.video?.elementaryPid,
                selection.video?.streamType,
                audio.elementaryPid,
                audio.streamType,
                true,
                false,
            )
        var state: PlaybackStartState =
            if (waiting) {
                PlaybackStartState.WaitingFirstOutput(signature, 7L)
            } else {
                PlaybackStartState.Started(signature, 7L)
            }
        val failures = mutableListOf<PlaybackPipeline.PlaybackUnavailable>()
        val restarts = mutableListOf<PlaybackPipeline.PlaybackGenerationRestart>()
        var notifications = 0
        var releaseAttempts = 0
        var rejectRelease = failCleanup
        private var resourceOwned = failCleanup

        init {
            set("inputId", "test")
            set("executor", LifecycleSerialExecutor("playback-fixture"))
            set("playbackGeneration", 7L)
            set("released", AtomicBoolean(false))
            set("videoAvailableNotified", AtomicBoolean(!waiting))
            set("resourceCleanup", cleanup)
            set("outstandingAudioOutputs", linkedMapOf<Int, Any>())
            val ptsType = field("ptsEpochCoordinator").type
            set("ptsEpochCoordinator", ptsType.getDeclaredConstructor().apply { isAccessible = true }.newInstance())
            set("activeTuner", tuner)
            set("activeChannel", channel)
            set("activeSelection", selection)
            set("audioPathExpected", true)
            set("onVideoUnavailable", { failure: PlaybackPipeline.PlaybackUnavailable ->
                failures += failure
                if (PlaybackStartTransitions.acceptsUnavailable(state, failure.generation)) {
                    state = PlaybackStartTransitions.failCurrentGeneration(state, failure.generation)
                    notifications++
                }
            })
            set("onPlaybackGenerationRestarted", { restart: PlaybackPipeline.PlaybackGenerationRestart ->
                restarts += restart
                MaleicacidLiveSession.acceptPlaybackGenerationRestart(state, restart, { state = it }) {
                    check(state is PlaybackStartState.Failed)
                    notifications++
                }
            })
            if (failCleanup) {
                cleanup.release("injected filter") {
                    releaseAttempts++
                    if (rejectRelease) error("filter close rejected")
                    resourceOwned = false
                }
            }
        }

        fun checkTerminalCleanupFailure() {
            val failure = failures.single()
            check(failure.reason == PlaybackPipeline.PlaybackUnavailableReason.PLAYBACK_RECOVERY_FAILED)
            check(failure.generation == 7L && failure.detail.contains("injected filter"))
            check(pipeline.currentPlaybackGenerationForTest() == 8L)
            check(notifications == 1 && state == PlaybackStartState.Failed(signature, 7L))
            check(restarts.isEmpty())
            checkRetainedThenReleased()
        }

        fun checkRetainedThenReleased() {
            check(cleanup.hasPending && resourceOwned && releaseAttempts == 2)
            check(field("activeTuner").get(pipeline) == null && field("activeChannel").get(pipeline) == null)
            rejectRelease = false
            pipeline.stop()
            check(!cleanup.hasPending && !resourceOwned && releaseAttempts == 3)
        }

        fun set(
            name: String,
            value: Any?,
        ) {
            field(name).set(pipeline, value)
        }

        // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
        @Suppress("MaxLineLength")
        private fun field(name: String) = PlaybackPipeline::class.java.getDeclaredField(name).apply { isAccessible = true }

        fun <T> allocate(type: Class<T>): T = type.cast(unsafe.allocateInstance(type))

        // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
        @Suppress("MaxLineLength")
        fun invoke(
            name: String,
            vararg args: Any,
        ): Any? {
            val method = PlaybackPipeline::class.java.declaredMethods.single { it.name == name || it.name.startsWith("$name-") }
            method.isAccessible = true
            return method.invoke(pipeline, *args)
        }
    }
}

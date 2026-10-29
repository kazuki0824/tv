// TunerController公開選局入口を通す。Frameworkのnative/TRM接続だけをShadowで代替する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import android.content.Context
import android.media.tv.tuner.DemuxCapabilities
import android.media.tv.tuner.Tuner
import android.media.tv.tuner.TunerUtils
import android.media.tv.tuner.filter.Filter
import android.media.tv.tuner.filter.FilterCallback
import android.media.tv.tuner.filter.FilterConfiguration
import android.media.tv.tuner.filter.FilterEvent
import android.media.tv.tuner.filter.SectionEvent
import android.media.tv.tuner.frontend.FrontendInfo
import android.media.tv.tuner.frontend.FrontendSettings
import android.media.tv.tunerresourcemanager.ITunerResourceManager
import android.media.tv.tunerresourcemanager.TunerResourceManager
import android.net.Uri
import android.widget.TextView
import com.maleicacid.tvinput.aribsi.AribSiEngine
import com.maleicacid.tvinput.aribsi.SectionIngestController
import com.maleicacid.tvinput.aribsi.SectionTransportLossSnapshot
import com.maleicacid.tvinput.aribsi.SiIngestResult
import com.maleicacid.tvinput.aribsi.SiStatus
import com.maleicacid.tvinput.aribsi.WellKnownSectionPid
import com.maleicacid.tvinput.common.FrequencyHz
import com.maleicacid.tvinput.common.TsPid
import com.maleicacid.tvinput.db.ChannelRecord
import com.maleicacid.tvinput.tis.TunerFrontendLeaseTransitionTest.Companion.controller
import com.maleicacid.tvinput.tis.TunerFrontendLeaseTransitionTest.Companion.satellite
import com.maleicacid.tvinput.tis.TunerFrontendLeaseTransitionTest.Companion.terrestrial
import com.maleicacid.tvinput.tis.TunerFrontendLeaseTransitionTest.NativeFilter
import com.maleicacid.tvinput.tis.TunerFrontendLeaseTransitionTest.NativeTuner
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.annotation.Implementation
import org.robolectric.annotation.Implements
import org.robolectric.annotation.RealObject
import org.robolectric.shadow.api.Shadow
import org.robolectric.util.ReflectionHelpers
import java.util.concurrent.Executor
import java.util.concurrent.ExecutorService
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference
import java.util.concurrent.locks.ReentrantLock

@RunWith(RobolectricTestRunner::class)
@Config(
    sdk = [35],
    instrumentedPackages = ["com.maleicacid.tvinput.aribsi"],
    shadows = [
        TunerFrontendLeaseTransitionTest.NativeTuner::class,
        TunerFrontendLeaseTransitionTest.NativeFilter::class,
        TunerFrontendLeaseTransitionTest.NativeTrm::class,
        TunerFrontendLeaseTransitionTest.SiEngineBoundary::class,
    ],
)
class TunerScanCleanupTest {
    @Test
    fun successfulScanCloseReleasesOwnerAndAllowsNextScan() {
        val controller = controller()
        NativeTuner.successfulTunes = true
        val task = ChannelScanManager.ActiveScanTask(17, ScanPurpose.SETUP_SCAN, RuntimeEnvironment.getApplication())
        val owner = AtomicReference<ChannelScanManager.ActiveScanTask?>(task)
        task.controller = controller
        try {
            check(controller.tuneForScan(terrestrial()).success)
            val sdk = checkNotNull(NativeTuner.current)
            check(ChannelScanManager.finishScanRelease(task, owner) { throw it })
            check(owner.get() == null && sdk.frontendHandle() == null)
            check(sdk.trm.frontendReleases == 1 && sdk.fullClosed == 1)
            val next = TunerController(RuntimeEnvironment.getApplication(), "next-scan")
            try {
                check(next.tuneForScan(terrestrial()).success)
                check(NativeTuner.current !== sdk)
            } finally {
                next.release()
            }
        } finally {
            controller.release()
        }
    }

    @Test
    fun frontendCloseFailureIsIrreversibleEvenAfterHalCleanupCompletes() {
        val controller = controller()
        try {
            check(!controller.tuneForScan(satellite()).success)
            val sdk = checkNotNull(NativeTuner.current)
            sdk.frontendCloseResult = Tuner.RESULT_UNKNOWN_ERROR
            check(!controller.tuneForScan(terrestrial()).success)
            val calls = sdk.closed
            // HALの後片付けが済んでもFrameworkが破棄した参照は戻らない。
            sdk.frontendCloseResult = Tuner.RESULT_SUCCESS
            check(!controller.tuneForScan(terrestrial()).success)
            check(sdk.closed == calls && sdk.closeCalls == 0)
            check(sdk.frontendHandle() == 17 && sdk.trm.frontendReleases == 0)
            check(NativeTuner.instances.size == 1)
            check(runCatching { controller.release() }.exceptionOrNull() is TunerOneShotCloseFailure)
        } finally {
            runCatching { controller.release() }
        }
    }

    @Test
    fun failedOneShotTunerCloseCannotBeMisreadAsSuccessfulRetry() {
        val controller = controller()
        val sdk = checkNotNull(NativeTuner.current)
        try {
            check(!controller.tuneForScan(satellite()).success)
            sdk.failNextFullClose = true
            val first = controller.tuneForScan(terrestrial())
            check(!first.success && first.resultCode == Tuner.RESULT_UNKNOWN_ERROR)
            check(sdk.oneShotClosed && sdk.fullClosed == 0 && sdk.closeCalls == 1)
            check(sdk.trm.frontendReleases == 1 && sdk.frontendHandle() == null)
            val again = controller.tuneForScan(terrestrial())
            check(!again.success && again.resultCode == Tuner.RESULT_UNKNOWN_ERROR)
            check(again.message.contains("資源解放未確定"))
            check(sdk.closeCalls == 1 && sdk.requested == listOf(FrontendSettings.TYPE_ISDBS))
            check(NativeTuner.instances.size == 1)
            val bs = controller.prepareBsCandidateSource()
            check(!bs.success && bs.resultCode == Tuner.RESULT_UNKNOWN_ERROR)
            check(sdk.closeCalls == 1 && sdk.applied.isEmpty())
            // terminal releaseもAOSPのclose再呼出しno-opを成功判定せず、実行器のみ閉じる。
            check(runCatching { controller.release() }.exceptionOrNull() is TunerOneShotCloseFailure)
            check(runCatching { controller.release() }.exceptionOrNull() is TunerOneShotCloseFailure)
            check(sdk.closeCalls == 1 && sdk.fullClosed == 0)
        } finally {
            runCatching { controller.release() }
        }
    }

    @Test
    fun oneShotCloseRetainsControllerFilterCleanupUntilRetryCompletes() {
        val controller = controller()
        NativeTuner.successfulTunes = true
        val task = ChannelScanManager.ActiveScanTask(6, ScanPurpose.SETUP_SCAN, RuntimeEnvironment.getApplication())
        val owner = AtomicReference<ChannelScanManager.ActiveScanTask?>(task)
        task.controller = controller
        var engineCloses = 0
        task.engine = AutoCloseable { engineCloses++ }
        var reported: Exception? = null
        try {
            check(controller.tuneForScan(terrestrial()).success)
            val sdk = checkNotNull(NativeTuner.current)
            val filter = Shadow.extract<NativeFilter>(sdk.filters.first())
            filter.rejectClose = true
            sdk.failNextFullClose = true
            repeat(2) {
                check(!ChannelScanManager.finishScanRelease(task, owner) { reported = it })
                check(reported != null && reported !is TunerOneShotCloseFailure)
                check(owner.get() === task && task.controller === controller && task.engine == null)
                check(!ReflectionHelpers.getField<Boolean>(controller, "released"))
                check(sdk.closeCalls == 0 && engineCloses == 1)
            }
            check(filter.closes == 2)
            filter.rejectClose = false
            check(!ChannelScanManager.finishScanRelease(task, owner) { reported = it })
            check(reported is TunerOneShotCloseFailure)
            check(owner.get() === task && task.controller === controller)
            check(!ReflectionHelpers.getField<Boolean>(controller, "released"))
            check(sdk.closed >= 1)
            // 未パッチSDKではclient登録/native後始末が残る。単なる受付開放を成功にしない。
            var rejected = 0
            val managerOwner =
                ReflectionHelpers.getField<AtomicReference<ChannelScanManager.ActiveScanTask?>>(
                    ChannelScanManager,
                    "activeTask",
                )
            check(managerOwner.compareAndSet(null, task))
            try {
                val next = ChannelScanManager.startIfIdle(RuntimeEnvironment.getApplication(), "test") { rejected++ }
                check(next == null)
                check(rejected == 1)
            } finally {
                managerOwner.compareAndSet(task, null)
            }
            check(filter.closes == 3 && sdk.closeCalls == 1 && engineCloses == 1)
            check(NativeTuner.instances.size == 1)
        } finally {
            NativeTuner.instances.flatMap { it.filters }.forEach {
                Shadow.extract<NativeFilter>(it).rejectClose = false
            }
            ReflectionHelpers.setField(controller, "tunerCloseFailure", null)
            runCatching { controller.release() }
        }
    }

    @Test
    fun setupStartRejectionDisplaysGenerationExhaustionInsteadOfWaitingForOldScan() {
        val owner =
            ReflectionHelpers.getField<AtomicReference<ChannelScanManager.ActiveScanTask?>>(
                ChannelScanManager,
                "activeTask",
            )
        val generation = ReflectionHelpers.getField<AtomicInteger>(ChannelScanManager, "nextGeneration")
        val oldGeneration = generation.get()
        val oldState = ChannelScanManager.currentState()
        check(owner.get() == null)
        val activityController = Robolectric.buildActivity(SetupActivity::class.java).create()
        val activity = activityController.get()
        val status = ReflectionHelpers.getField<TextView>(activity, "statusView")
        ReflectionHelpers.setField(activity, "setupGeneration", 7)
        try {
            val task = ChannelScanManager.ActiveScanTask(7, ScanPurpose.SETUP_SCAN, activity.applicationContext)
            owner.set(task)
            ReflectionHelpers.setField(ChannelScanManager, "state", ScanState.Running(0L, 7, ScanPurpose.SETUP_SCAN))
            check(startSetupScan(activity) == null)
            check(status.text.toString().contains("前のスキャンを終了中"))
            owner.set(null)
            generation.set(Int.MAX_VALUE)
            check(startSetupScan(activity) == null)
            val failure = ChannelScanManager.currentState() as ScanState.Failed
            check(failure.message == "SCAN_GENERATION_EXHAUSTED")
            check(status.text.toString() == "設定スキャン 失敗: SCAN_GENERATION_EXHAUSTED")
            check(!status.text.toString().contains("少し待って"))
            check(owner.get() == null && generation.get() == Int.MAX_VALUE)
            check(ReflectionHelpers.getField<Int?>(activity, "setupGeneration") == 7)
        } finally {
            owner.set(null)
            generation.set(oldGeneration)
            ReflectionHelpers.setField(ChannelScanManager, "state", oldState)
            activityController.destroy()
        }
    }

    @Test
    fun failedScanWithPendingCleanupReportsBusyInsteadOfOldTerminalFailure() {
        val owner =
            ReflectionHelpers.getField<AtomicReference<ChannelScanManager.ActiveScanTask?>>(
                ChannelScanManager,
                "activeTask",
            )
        val managerExecutor = ReflectionHelpers.getField<ExecutorService>(ChannelScanManager, "executor")
        val oldState = ChannelScanManager.currentState()
        check(owner.get() == null)
        val activityController = Robolectric.buildActivity(SetupActivity::class.java).create()
        val activity = activityController.get()
        val controller = controller()
        val task = ChannelScanManager.ActiveScanTask(7, ScanPurpose.SETUP_SCAN, activity.applicationContext)
        task.controller = controller
        owner.set(task)
        val previousFailure = ScanState.Failed("PREVIOUS_SCAN_TUNE_FAILED", 7, ScanPurpose.SETUP_SCAN)
        val sdk = checkNotNull(NativeTuner.current)
        try {
            NativeTuner.successfulTunes = true
            check(controller.tuneForScan(terrestrial()).success)
            sdk.frontendCloseResult = Tuner.RESULT_UNKNOWN_ERROR
            ReflectionHelpers.callInstanceMethod<Unit>(
                ChannelScanManager,
                "setTerminalStateIfCurrent",
                ReflectionHelpers.ClassParameter.from(Int::class.javaPrimitiveType, 7),
                ReflectionHelpers.ClassParameter.from(ScanState::class.java, previousFailure),
            )
            check(!finishSetupScan(7))
            check(task.closing && owner.get() === task && task.controller === controller)
            val status = ReflectionHelpers.getField<TextView>(activity, "statusView")
            check(status.text.toString().contains(previousFailure.message))
            check(startSetupScan(activity) == null)
            managerExecutor.submit {}.get(5, TimeUnit.SECONDS)
            check(status.text.toString().contains("前のスキャンを終了中"))
            check(!status.text.toString().contains(previousFailure.message))
            check(ChannelScanManager.currentState() === previousFailure)
            check(task.closing && owner.get() === task && task.controller === controller)
            check(sdk.closeCalls == 0 && !sdk.oneShotClosed)
            sdk.frontendCloseResult = Tuner.RESULT_SUCCESS
            check(!finishSetupScan(7))
            check(owner.get() === task)
        } finally {
            sdk.frontendCloseResult = Tuner.RESULT_SUCCESS
            managerExecutor.submit {}.get(5, TimeUnit.SECONDS)
            runCatching { controller.release() }
            owner.compareAndSet(task, null)
            ReflectionHelpers.setField(ChannelScanManager, "state", oldState)
            activityController.destroy()
        }
    }

    private fun startSetupScan(activity: SetupActivity): Int? =
        ChannelScanManager.startIfIdle(activity, "test") { failure ->
            ReflectionHelpers.callInstanceMethod<Unit>(
                activity,
                "onScanStartRejected",
                ReflectionHelpers.ClassParameter.from(ScanState.Failed::class.java, failure),
            )
        }

    private fun finishSetupScan(generation: Int): Boolean =
        ReflectionHelpers.callInstanceMethod(
            ChannelScanManager,
            "finishScanIfCurrent",
            ReflectionHelpers.ClassParameter.from(Int::class.javaPrimitiveType, generation),
        )
}

// SDKとSIの既存Shadowを共有し、旧Filterが現行診断へ混入しないことを検査する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import android.media.tv.tuner.filter.Filter
import android.media.tv.tuner.filter.SectionEvent
import com.maleicacid.tvinput.aribsi.AribSiEngine
import com.maleicacid.tvinput.aribsi.SectionIngestController
import com.maleicacid.tvinput.aribsi.WellKnownSectionPid
import com.maleicacid.tvinput.common.FrequencyHz
import com.maleicacid.tvinput.common.TsPid
import com.maleicacid.tvinput.db.ChannelRecord
import com.maleicacid.tvinput.tis.TunerController.SectionFilterHandle
import com.maleicacid.tvinput.tis.TunerFrontendLeaseTransitionTest.NativeTuner
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.util.ReflectionHelpers
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

@RunWith(RobolectricTestRunner::class)
@Config(
    sdk = [35],
    instrumentedPackages = ["com.maleicacid.tvinput.aribsi"],
    shadows = [
        NativeTuner::class,
        TunerFrontendLeaseTransitionTest.NativeFilter::class,
        TunerFrontendLeaseTransitionTest.SiEngineBoundary::class,
    ],
)
class TunerSectionSourceDiagnosticTest {
    @Test
    fun replacedFilterCannotIncrementMalformedOrOversizedDiagnostics() = checkEvents(retune = false)

    @Test
    fun oldGenerationCannotIncrementMalformedOrOversizedDiagnostics() = checkEvents(retune = true)

    @Test
    fun replacedFilterCannotIncrementInputDeliveryLoss() = checkDeliveryLoss(retune = false)

    @Test
    fun oldGenerationCannotIncrementInputDeliveryLoss() = checkDeliveryLoss(retune = true)

    private fun checkEvents(retune: Boolean) {
        Fixture(retune).use { fixture ->
            fixture.event(fixture.stale, 0L)
            fixture.event(fixture.stale, 4097L)
            fixture.awaitData()
            fixture.checkCounters(0)
            fixture.event(fixture.current, 0L)
            fixture.event(fixture.current, 4097L)
            fixture.awaitData()
            // payloadなしの過大eventはdrain不足も一度記録する。
            fixture.checkCounters(1)
        }
    }

    private fun checkDeliveryLoss(retune: Boolean) {
        Fixture(retune).use { fixture ->
            val entered = CountDownLatch(1)
            val resume = CountDownLatch(1)
            fixture.executor.executeControl {
                entered.countDown()
                check(resume.await(5, TimeUnit.SECONDS))
            }
            try {
                check(entered.await(5, TimeUnit.SECONDS))
                repeat(16) { fixture.executor.executeData {} }
                // close前に読み出し済みで、配送だけが後着するcallbackを再現する。
                fixture.deliver(fixture.stale)
                check(fixture.ingest.inputDeliveryLossCount == 0)
                fixture.deliver(fixture.current)
                check(fixture.ingest.inputDeliveryLossCount == 1)
            } finally {
                resume.countDown()
            }
        }
    }

    private data class Source(
        val filter: Filter,
        val handle: SectionFilterHandle,
        val generation: Long,
    )

    private class Fixture(
        retune: Boolean,
    ) : AutoCloseable {
        private val pid = WellKnownSectionPid.SDT_BAT
        private val engine = AribSiEngine(RuntimeEnvironment.getApplication())
        val ingest = SectionIngestController(engine)
        private val controller: TunerController
        val executor: ControllerSerialExecutor
        val stale: Source
        val current: Source

        init {
            NativeTuner.instances.clear()
            NativeTuner.current = null
            NativeTuner.successfulTunes = true
            NativeTuner.failNextConstruction = false
            controller = TunerController(RuntimeEnvironment.getApplication(), "section-source-test")
            executor = ReflectionHelpers.getField(controller, "sectionExecutor")
            controller.setSectionIngestController(ingest)
            val candidate =
                ScanCandidate(ChannelRecord.DELIVERY_SYSTEM_ISDB_T, FrequencyHz(473_142_857L), displayChannel = "13")
            check(controller.tuneForScan(candidate).success)
            stale = source()
            if (retune) {
                check(controller.tuneForScan(candidate).success)
            } else {
                controller.closeSectionFilter(pid)
                check(controller.openSectionFilter(pid).isOpen)
            }
            current = source()
            check(stale.filter !== current.filter)
            check((stale.generation != current.generation) == retune)
        }

        private fun source(): Source {
            val filters = ReflectionHelpers.getField<Map<TsPid, List<Filter>>>(controller, "sectionFilters")
            val handles =
                ReflectionHelpers.getField<Map<TsPid, SectionFilterHandle>>(controller, "sectionFilterHandles")
            return Source(
                filters.getValue(pid).single(),
                handles.getValue(pid),
                ReflectionHelpers.getField(controller, "tuneGeneration"),
            )
        }

        fun event(
            source: Source,
            length: Long,
        ) {
            val event =
                ReflectionHelpers.callConstructor(
                    SectionEvent::class.java,
                    ReflectionHelpers.ClassParameter.from(Int::class.javaPrimitiveType, 0),
                    ReflectionHelpers.ClassParameter.from(Int::class.javaPrimitiveType, 0),
                    ReflectionHelpers.ClassParameter.from(Int::class.javaPrimitiveType, 0),
                    ReflectionHelpers.ClassParameter.from(Long::class.javaPrimitiveType, length),
                )
            invoke("handleSectionFilterEvent", source.filter, event, source.handle)
        }

        fun deliver(source: Source) {
            invoke("deliverSectionRead", source.filter, byteArrayOf(0x42), 1, source.handle)
        }

        private fun invoke(
            name: String,
            vararg args: Any,
        ) {
            val method = TunerController::class.java.declaredMethods.single { it.name.startsWith(name) }
            method.isAccessible = true
            method.invoke(controller, *args)
        }

        fun awaitData() {
            val finished = CountDownLatch(1)
            executor.executeData { finished.countDown() }
            check(finished.await(5, TimeUnit.SECONDS))
        }

        fun checkCounters(expected: Int) {
            val names = listOf("sectionMalformedCounters", "sectionOversizedCounters", "sectionReadErrorCounters")
            for (name in names) {
                val counters = ReflectionHelpers.getField<Map<TsPid, Int>>(controller, name)
                check((counters[pid] ?: 0) == expected) { "$name=$counters expected=$expected" }
            }
        }

        override fun close() {
            controller.release()
            engine.close()
        }
    }
}

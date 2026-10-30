// テストの入力・期待値を本体の定数と独立した具体値で記述する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import android.content.ContextWrapper
import com.maleicacid.tvinput.aribsi.AribSiEngine
import com.maleicacid.tvinput.aribsi.SectionIngestController
import com.maleicacid.tvinput.aribsi.SiStatus
import com.maleicacid.tvinput.aribsi.WellKnownSectionPid
import com.maleicacid.tvinput.common.TsPid
import org.junit.Test
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

class TunerControllerSectionBoundsTest {
    @Test fun clockFiltersSelectDisjointTablesWithTheRequiredCrcPolicy() {
        val settings = TunerController.sectionSettingsForPid(WellKnownSectionPid.TDT)
        check(settings.size == 2)
        for (tableId in 0..255) {
            val selected =
                settings.filter { setting ->
                    val mask = setting.mask.single().toInt() and 0xff
                    check(setting.mode.contentEquals(byteArrayOf(0)))
                    (tableId and mask) == (setting.filterBytes.single().toInt() and mask)
                }
            when (tableId) {
                0x70 -> check(!selected.single().isCrcEnabled)
                0x73 -> check(selected.single().isCrcEnabled)
                else -> check(selected.isEmpty())
            }
        }
    }

    @Test fun liveAndSetupEitAcceptOnlyActualPfWithoutChangingEpgMaintenance() {
        val eit = WellKnownSectionPid.EIT
        val live = TunerController.sectionSettingsForPid(eit, eitPfActualOnly = true).single()
        val scan = TunerController.sectionSettingsForPid(eit).single()
        check(live.isRepeat && live.isCrcEnabled)
        check(scan.isRepeat && scan.isCrcEnabled)
        check(live.mask.size == 1 && live.mask.single().toInt() and 0xff == 0xff)
        val tableFilter = live.filterBytes.single().toInt() and 0xff
        val mask = live.mask.single().toInt() and 0xff
        for (tableId in 0x00..0xff) {
            val permitted = (tableId and mask) == (tableFilter and mask)
            check(permitted == (tableId == 0x4e))
        }
        // boot EPG同期・background maintenanceは従来どおりschedule等も収集する。
        check(scan.filterBytes.isEmpty() && scan.mask.isEmpty())
        val pat =
            TunerController
                .sectionSettingsForPid(WellKnownSectionPid.PAT, eitPfActualOnly = true)
                .single()
        check(pat.filterBytes.isEmpty())
    }

    @Test
    fun scheduleBurstIsRejectedBeforeBoundedAdmissionAndPatSdtAreIngested() {
        val engine = AribSiEngine(ContextWrapper(null))
        val ingest = SectionIngestController(engine)
        val executor = ControllerSerialExecutor("live-eit-admission-test", maxPendingDataTasks = 2)
        val ownerEntered = CountDownLatch(1)
        val allowOwner = CountDownLatch(1)
        val accepted = CountDownLatch(2)
        val ingested = CopyOnWriteArrayList<Pair<TsPid, Int>>()
        try {
            executor.executeControl {
                ownerEntered.countDown()
                check(allowOwner.await(5, TimeUnit.SECONDS))
            }
            check(ownerEntered.await(5, TimeUnit.SECONDS))

            // 実際のFilter条件を用い、callbackがdrainしたsectionを既存data ownerへ有限投入する。
            fun deliver(
                pid: TsPid,
                hex: String,
            ) {
                val bytes =
                    hex
                        .chunked(2)
                        .map { it.toInt(16).toByte() }
                        .toByteArray()
                val tableId = bytes[0].toUByte().toInt()
                val setting = TunerController.sectionSettingsForPid(pid, eitPfActualOnly = true).single()
                val mask =
                    setting.mask
                        .firstOrNull()
                        ?.toUByte()
                        ?.toInt()
                if (mask != null) {
                    val selectedTableId =
                        setting.filterBytes
                            .single()
                            .toUByte()
                            .toInt()
                    if ((tableId and mask) != (selectedTableId and mask)) return
                }
                if (pid == WellKnownSectionPid.SDT_BAT) ingest.recordSdtRead(pid, tableId)
                try {
                    executor.executeData {
                        ingested += pid to ingest.onSection(pid, bytes).status
                        accepted.countDown()
                    }
                } catch (_: DataCapacityExceededException) {
                    ingest.recordInputDeliveryLoss(pid, tableId)
                }
            }

            // schedule (0x50) とp/f other (0x4f)が大半を占めてもqueueに蓄積しない。
            repeat(92) { deliver(WellKnownSectionPid.EIT, "50") }
            repeat(4) { deliver(WellKnownSectionPid.EIT, "4f") }
            check(executor.queue.isEmpty())
            val pat = "00b01d7fe3db00000000e0100418e1010419e1020598ffc8049fe7f029c30f6e"
            val sdt = "42f00c7fe3c100007fe3ffed225f15"
            deliver(WellKnownSectionPid.PAT, pat)
            deliver(WellKnownSectionPid.SDT_BAT, sdt)
            check(executor.queue.size == 2)
            deliver(WellKnownSectionPid.SDT_BAT, sdt) // 真の容量飽和だけは従来どおり損失計上
            check(ingest.inputDeliveryLossCount == 1)
            check("sdtActualDropped=1" in ingest.sdtDeliveryDiagnostic())
            allowOwner.countDown()
            check(accepted.await(5, TimeUnit.SECONDS))
            check(ingested.map { it.first }.toSet() == setOf(WellKnownSectionPid.PAT, WellKnownSectionPid.SDT_BAT))
            check(ingested.all { it.second == SiStatus.OK })
            check("sdtActualAccepted=1" in ingest.sdtDeliveryDiagnostic())
            check("sdtActualRead=2" in ingest.sdtDeliveryDiagnostic())
            // 本試験はproducerに対するFilter maskとowner受理を検証する。実HALの損失解消は別途実測する。
        } finally {
            allowOwner.countDown()
            executor.shutdownNow()
            engine.close()
        }
    }

    @Test fun ordinarySectionFiltersKeepCrcEnabled() {
        val setting =
            TunerController
                .sectionSettingsForPid(
                    TsPid(0),
                ).single()
        check(setting.isCrcEnabled)
        check(setting.isRepeat)
        check(!setting.isRaw)
        check(setting.lengthFieldBitWidth == 12)
    }

    @Test fun sectionEventDataLengthDecisionIsFixedAt4096Bytes() {
        check(SectionFilterPolicy.dataLengthDecision(0) == SectionFilterPolicy.DataLengthDecision.MALFORMED)
        check(SectionFilterPolicy.dataLengthDecision(-1) == SectionFilterPolicy.DataLengthDecision.MALFORMED)
        check(SectionFilterPolicy.dataLengthDecision(1) == SectionFilterPolicy.DataLengthDecision.ACCEPT)
        check(SectionFilterPolicy.dataLengthDecision(4096) == SectionFilterPolicy.DataLengthDecision.ACCEPT)
        check(SectionFilterPolicy.dataLengthDecision(4097) == SectionFilterPolicy.DataLengthDecision.OVERSIZED)
    }
}

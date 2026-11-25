@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import android.content.ContentValues
import android.content.Context
import android.media.tv.TvContract
import com.maleicacid.tvinput.aribsi.AribSiEngine
import com.maleicacid.tvinput.aribsi.SectionIngestController
import com.maleicacid.tvinput.aribsi.SiDiscoveryProfile
import com.maleicacid.tvinput.aribsi.SiStatus
import com.maleicacid.tvinput.common.FrequencyHz
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.StreamSelector
import com.maleicacid.tvinput.common.TsPid
import com.maleicacid.tvinput.db.ChannelRecord
import org.robolectric.util.ReflectionHelpers
import java.nio.ByteBuffer

internal fun verifyBootPartialOther(
    context: Context,
    direct: ServiceKey,
    rows: List<ContentValues>,
    pf: ByteArray,
    ingest: (AribSiEngine) -> Unit,
) {
    val other = ServiceKey(direct.originalNetworkId, 0x7777, 1049)
    val targets = setOf(direct, other)
    val engine = AribSiEngine(context)
    val scan = ChannelScanController(context, "real-ts-test-input", engine, ScanPurpose.BOOT_EPG_SYNC)
    val native = TunerFrontendLeaseTransitionTest.NativeTuner
    val previous = native.successfulTunes
    try {
        engine.setDiscoveryProfile(SiDiscoveryProfile.ISDB_T)
        ingest(engine)
        val data = ByteBuffer.wrap(pf)
        for (offset in listOf(0, 13)) data.put(offset, 0x4f)
        for (offset in listOf(6, 7, 12)) data.put(offset, 0)
        data.putShort(14, 0xba02.toShort())
        writeSectionCrc(pf)
        check(SectionIngestController(engine).onSection(TsPid.EIT, pf).status == SiStatus.OK)
        val published =
            ReflectionHelpers.callInstanceMethod<ChannelScanController.PublishSnapshotResult>(
                scan,
                "publishProgramsForRegisteredServices",
                ReflectionHelpers.ClassParameter.from(
                    ChannelScanController.PublishMode::class.java,
                    ChannelScanController.PublishMode.BOOT_EPG_SYNC,
                ),
                ReflectionHelpers.ClassParameter.from(Set::class.java, targets),
            )
        check(published.committedServiceKeys.containsAll(targets))
        check(published.coveredOtherServices.isEmpty())
        val saved = rows.filter { it.getAsLong(TvContract.Programs.COLUMN_CHANNEL_ID) == 9L }
        check(saved.any { it.getAsInteger(TvContract.Programs.COLUMN_EVENT_ID) == 0xba02 })
        native.successfulTunes = false
        val b =
            ChannelRecord(
                other,
                1,
                "B",
                "B",
                FrequencyHz(1_049_480_000L),
                deliverySystem = "ISDB_S",
                streamSelector = StreamSelector.tsid(0x7777),
            )
        val failed = scan.startBootEpgSync(listOf(b))
        check(failed.diagnostics.any { "選局に失敗" in it.message })
        val committed = published.completedOnTune(setOf(direct), satellite = true) + failed.committedServiceKeys
        DirectBootGuard.markBootEpgSyncRequested(context, "test")
        val result = failed.copy(committedServiceKeys = committed)
        check(!ChannelScanManager.finishBootTargets(context, targets, result))
        check(DirectBootGuard.pendingState(context).lastSkippedReason == "BOOT_EPG_INCOMPLETE_TARGET_SET")
        check(DirectBootGuard.isPending(context))
    } finally {
        native.successfulTunes = previous
        scan.close()
        engine.close()
    }
}

internal fun writeSectionCrc(bytes: ByteArray) {
    var crc = -1
    for (i in 0 until bytes.size - 4) {
        crc = crc xor ((bytes[i].toInt() and 255) shl 24)
        repeat(8) { crc = if (crc < 0) (crc shl 1) xor 0x04c11db7 else crc shl 1 }
    }
    ByteBuffer.wrap(bytes).putInt(bytes.size - 4, crc)
}

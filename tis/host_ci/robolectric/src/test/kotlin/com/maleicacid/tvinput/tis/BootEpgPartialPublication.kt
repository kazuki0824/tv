@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import android.content.ContentValues
import android.content.Context
import android.media.tv.TvContract
import com.maleicacid.tvinput.aribsi.AribSiEngine
import com.maleicacid.tvinput.aribsi.SectionIngestController
import com.maleicacid.tvinput.aribsi.SiStatus
import com.maleicacid.tvinput.common.FrequencyHz
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.StreamSelector
import com.maleicacid.tvinput.common.TsPid
import com.maleicacid.tvinput.db.ChannelRecord
import org.robolectric.shadows.ShadowSystemClock
import java.nio.ByteBuffer
import java.time.Duration
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

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
    val injected = AtomicBoolean()
    val worker = Executors.newSingleThreadExecutor()
    val filter = TunerFrontendLeaseTransitionTest.NativeFilter
    val previousStart = filter.onStart
    try {
        native.successfulTunes = true
        filter.onStart = {
            if (injected.compareAndSet(false, true)) {
                ingest(engine)
                prepareOtherPf(pf)
                check(SectionIngestController(engine).onSection(TsPid.EIT, pf).status == SiStatus.OK)
                native.successfulTunes = false
            }
        }
        val channels =
            listOf(direct, other).map { key ->
                ChannelRecord(
                    key,
                    1,
                    key.serviceId.toString(),
                    "BS",
                    FrequencyHz(1_049_480_000L),
                    deliverySystem = "ISDB_S",
                    streamSelector = StreamSelector.tsid(key.transportStreamId),
                )
            }
        DirectBootGuard.markBootEpgSyncRequested(context, "test")
        val future = worker.submit<ChannelScanController.ScanResult> { scan.startBootEpgSync(channels) }
        val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(15)
        while (!future.isDone && System.nanoTime() < deadline) {
            Thread.sleep(100)
            ShadowSystemClock.advanceBy(Duration.ofSeconds(1))
        }
        val result = future.get(1, TimeUnit.SECONDS)
        val rejected = result.diagnostics.filter { "選局に失敗" in it.message }
        check(result.scanned == 2 && rejected.single().candidate.displayChannel == other.serviceId.toString())
        check(result.committedServiceKeys == setOf(direct)) { result.toString() }
        val saved = rows.filter { it.getAsLong(TvContract.Programs.COLUMN_CHANNEL_ID) == 9L }
        check(saved.any { it.getAsInteger(TvContract.Programs.COLUMN_EVENT_ID) == 0xba02 })
        check(!ChannelScanManager.finishBootTargets(context, targets, result))
        check(DirectBootGuard.pendingState(context).lastSkippedReason == "BOOT_EPG_INCOMPLETE_TARGET_SET")
        check(DirectBootGuard.isPending(context))
    } finally {
        filter.onStart = previousStart
        native.successfulTunes = previous
        scan.cancelScan()
        worker.shutdownNow()
        scan.close()
        engine.close()
    }
}

private fun prepareOtherPf(pf: ByteArray) {
    val data = ByteBuffer.wrap(pf)
    for (offset in listOf(0, 13)) data.put(offset, 0x4f)
    for (offset in listOf(6, 7, 12)) data.put(offset, 0)
    data.putShort(14, 0xba02.toShort())
    writeSectionCrc(pf)
}

// 地上波fixtureのNIT network descriptor loopだけをBSのsystem_management_descriptorに置き換える。
internal fun asBsNit(section: ByteArray): ByteArray {
    val bytes = section.copyOf()
    val end = 10 + ((bytes[8].toInt() and 15) shl 8) + (bytes[9].toInt() and 255)
    var pos = 10
    while (pos < end) {
        if ((bytes[pos].toInt() and 255) == 0xfe) bytes[pos + 2] = 2
        pos += 2 + (bytes[pos + 1].toInt() and 255)
    }
    writeSectionCrc(bytes)
    return bytes
}

internal fun writeSectionCrc(bytes: ByteArray) {
    var crc = -1
    for (i in 0 until bytes.size - 4) {
        crc = crc xor ((bytes[i].toInt() and 255) shl 24)
        repeat(8) { crc = if (crc < 0) (crc shl 1) xor 0x04c11db7 else crc shl 1 }
    }
    ByteBuffer.wrap(bytes).putInt(bytes.size - 4, crc)
}

package com.maleicacid.tvinput.tis

import com.maleicacid.tvinput.aribsi.AribEvent
import com.maleicacid.tvinput.aribsi.EventModelMapper
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.db.ProgramRecord

/** decoderで観測したvideo metadataをProgramへ反映する純粋なmerge規則。 */
object ProgramVideoMetadataPolicy {
    fun currentProgramsWithMetadata(
        events: List<AribEvent>,
        serviceKey: ServiceKey,
        nowMillis: Long,
        info: PlaybackPipeline.VideoFormatInfo,
    ): List<ProgramRecord> {
        val records = EventModelMapper().toProgramRecords(
            events.filter { event -> eventContainsTime(event, serviceKey, nowMillis) },
        )
        return merge(records, records.associate { key(it) to info })
    }

    fun key(record: ProgramRecord): String {
        val serviceKey = record.serviceKey
        return listOf(
            serviceKey.originalNetworkId,
            serviceKey.transportStreamId,
            serviceKey.serviceId,
            record.eventId,
            record.stableIdentity,
        ).joinToString(":")
    }

    fun merge(
        records: List<ProgramRecord>,
        latestByProgramKey: Map<String, PlaybackPipeline.VideoFormatInfo>,
    ): List<ProgramRecord> = records.map { record ->
        val info = latestByProgramKey[key(record)]
        if (info == null || record.videoWidth != null || record.videoHeight != null || record.videoFormat != null) {
            record
        } else {
            record.copy(videoWidth = info.width, videoHeight = info.height, videoFormat = info.mime)
        }
    }

    fun eventContainsTime(event: AribEvent, serviceKey: ServiceKey, nowMillis: Long): Boolean {
        val end = runCatching { Math.addExact(event.startTimeMillis, event.durationMillis) }.getOrNull()
            ?: return false
        return event.serviceKey == serviceKey && nowMillis >= event.startTimeMillis && nowMillis < end
    }
}

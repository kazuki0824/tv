package com.maleicacid.tvinput.aribsi

import com.maleicacid.tvinput.db.ProgramRecord

class EventModelMapper {
    fun toProgramRecords(events: List<AribEvent>): List<ProgramRecord> = events.mapNotNull { event ->
        val end = event.startTimeMillis + event.durationMillis
        if (event.startTimeMillis <= 0L || end <= event.startTimeMillis) {
            null
        } else {
            ProgramRecord(
                serviceKey = event.serviceKey,
                eventId = event.eventId,
                stableIdentity = event.stableIdentity,
                startTimeMillis = event.startTimeMillis,
                durationMillis = event.durationMillis,
                title = event.title.ifBlank { "event-${event.eventId}" },
                description = providerDescription(event),
                shortDescription = shortDescription(event),
                extendedItemsJson = extendedItemsJson(event.extendedItems),
                componentText = event.componentText,
                audioComponentText = event.audioComponentText,
                audioLanguage = event.audioLanguage,
                canonicalGenre = event.canonicalGenre,
                genreSupplementText = event.genreSupplementText,
                eventGroupText = event.eventGroupText,
                freeCaText = event.freeCaText,
                seriesName = event.seriesName,
                diagnosticText = event.diagnosticText,
                diagnosticDescriptorJson = event.diagnosticDescriptorJson,
            )
        }
    }

    private fun shortDescription(event: AribEvent): String = event.description.take(256)

    private fun providerDescription(event: AribEvent): String {
        val extended = event.extendedItems.joinToString("\n") { item ->
            if (item.itemDescription.isBlank()) item.itemText else "【${item.itemDescription}】${item.itemText}"
        }
        val uiSupplements = listOfNotNull(
            event.componentText?.takeIf { it.isNotBlank() }?.let { "映像: $it" },
            event.audioComponentText?.takeIf { it.isNotBlank() }?.let { "音声: $it" },
            event.genreSupplementText?.takeIf { it.isNotBlank() }?.let { "ジャンル: $it" },
            event.eventGroupText?.takeIf { it.isNotBlank() }?.let { "関連番組: $it" },
            event.freeCaText?.takeIf { it.isNotBlank() }?.let { "放送種別: $it" },
        )
        return listOf(event.description, event.extendedDescription, extended)
            .plus(uiSupplements)
            .filter { it.isNotBlank() }
            .joinToString("\n")
    }

    private fun extendedItemsJson(items: List<AribExtendedItem>): String =
        items.joinToString(prefix = "[", postfix = "]") { item ->
            "{\"description\":\"${escapeJson(item.itemDescription)}\",\"text\":\"${escapeJson(item.itemText)}\"}"
        }

    private fun escapeJson(value: String): String = value
        .replace("\\", "\\\\")
        .replace("\"", "\\\"")
        .replace("\n", "\\n")
}

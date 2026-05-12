package com.maleicacid.tvinput.tis

import android.content.ContentUris
import android.content.ContentValues
import android.content.Context
import android.media.tv.TvContentRating
import android.media.tv.TvContract
import android.net.Uri
import com.maleicacid.tvinput.aribsi.AribEvent
import com.maleicacid.tvinput.aribsi.AribRatingMapper
import com.maleicacid.tvinput.common.ServiceKey

class CurrentProgramRatingResolver(private val context: Context) {
    enum class Source { TV_PROVIDER_CURRENT_PROGRAM, LATEST_EIT_CACHE, UNRATED_FALLBACK }

    data class CurrentProgramRatingSet(
        val ratings: List<TvContentRating>,
        val source: Source,
        val channelUriString: String,
        val serviceKey: ServiceKey?,
        val eventId: Int?,
        val startTimeMillis: Long?,
        val endTimeMillis: Long?,
    ) {
        fun ratingsForBlocking(): List<TvContentRating> = ratings.ifEmpty { listOf(AribRatingMapper.unrated()) }

        fun unblockKeyFor(rating: TvContentRating): String = unblockKey(
            serviceKey = serviceKey,
            eventId = eventId,
            ratingString = rating.flattenToString(),
        )

        fun programIdentityKey(): String? {
            if (serviceKey == null || eventId == null) return null
            return stableProgramKey(serviceKey, eventId)
        }

        fun currentRowSelectionKey(): String? {
            if (eventId == null || startTimeMillis == null || endTimeMillis == null) return null
            return listOf(
                channelUriString,
                serviceKey?.originalNetworkId?.toString().orEmpty(),
                serviceKey?.transportStreamId?.toString().orEmpty(),
                serviceKey?.serviceId?.toString().orEmpty(),
                eventId.toString(),
                startTimeMillis.toString(),
                endTimeMillis.toString(),
            ).joinToString("|")
        }

        /**
         * Returns the unblock key only when the framework-provided rating matches the
         * current program and the current program identity is complete. UNRATED fallback
         * without event/start/end identity is intentionally not unblockable.
         */
        fun exactUnblockKeyFor(rating: TvContentRating): String? {
            if (programIdentityKey() == null) return null
            val requested = rating.flattenToString()
            val currentRatings = ratingsForBlocking().map { it.flattenToString() }.toSet()
            if (requested !in currentRatings) return null
            return unblockKeyFor(rating)
        }
    }

    fun resolve(
        channelUri: Uri?,
        serviceKey: ServiceKey?,
        latestEvents: List<AribEvent>,
        nowMillis: Long = System.currentTimeMillis(),
    ): CurrentProgramRatingSet {
        fromTvProvider(channelUri, serviceKey, nowMillis)?.let { return it }
        fromLatestEit(channelUri, serviceKey, latestEvents, nowMillis)?.let { return it }
        return CurrentProgramRatingSet(
            ratings = listOf(AribRatingMapper.unrated()),
            source = Source.UNRATED_FALLBACK,
            channelUriString = channelUri?.toString().orEmpty(),
            serviceKey = serviceKey,
            eventId = null,
            startTimeMillis = null,
            endTimeMillis = null,
        )
    }

    private fun fromTvProvider(channelUri: Uri?, serviceKey: ServiceKey?, nowMillis: Long): CurrentProgramRatingSet? {
        if (channelUri == null) return null
        val projection = arrayOf(
            TvContract.Programs._ID,
            TvContract.Programs.COLUMN_EVENT_ID,
            TvContract.Programs.COLUMN_START_TIME_UTC_MILLIS,
            TvContract.Programs.COLUMN_END_TIME_UTC_MILLIS,
            TvContract.Programs.COLUMN_CONTENT_RATING,
            TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA,
        )
        val selection = "${TvContract.Programs.COLUMN_START_TIME_UTC_MILLIS} <= ? AND ${TvContract.Programs.COLUMN_END_TIME_UTC_MILLIS} > ?"
        val selectionArgs = arrayOf(nowMillis.toString(), nowMillis.toString())
        val sortOrder = "${TvContract.Programs.COLUMN_START_TIME_UTC_MILLIS} DESC, ${TvContract.Programs.COLUMN_END_TIME_UTC_MILLIS} ASC, ${TvContract.Programs._ID} DESC"
        return runCatching {
            data class Candidate(
                val rowId: Long,
                val eventId: Int,
                val start: Long,
                val end: Long,
                val flattenedRatings: String?,
                val providerData: String?,
            )
            val candidates = mutableListOf<Candidate>()
            context.contentResolver.query(TvContract.buildProgramsUriForChannel(channelUri), projection, selection, selectionArgs, sortOrder)?.use { cursor ->
                while (cursor.moveToNext()) {
                    val providerData = runCatching { cursor.getBlob(5)?.let { String(it, Charsets.UTF_8) } }.getOrNull()
                        ?: runCatching { cursor.getString(5) }.getOrNull()
                    if (TvProviderWriter.providerDataMatchesService(providerData, serviceKey)) {
                        candidates += Candidate(
                            rowId = cursor.getLong(0),
                            eventId = cursor.getInt(1),
                            start = cursor.getLong(2),
                            end = cursor.getLong(3),
                            flattenedRatings = cursor.getString(4),
                            providerData = providerData,
                        )
                    }
                }
            }
            val selected = candidates.firstOrNull() ?: return@runCatching null
            val ratingSet = CurrentProgramRatingSet(
                ratings = AribRatingMapper.parseFlattenedList(selected.flattenedRatings),
                source = Source.TV_PROVIDER_CURRENT_PROGRAM,
                channelUriString = channelUri.toString(),
                serviceKey = serviceKey,
                eventId = selected.eventId,
                startTimeMillis = selected.start,
                endTimeMillis = selected.end,
            )
            val selectionRule = "START_DESC_END_ASC_ID_DESC"
            runCatching {
                val updatedProviderData = TvProviderWriter.providerDataWithCurrentProgramDiagnostics(
                    providerData = selected.providerData,
                    overlapCount = candidates.size,
                    selectedProgramId = selected.rowId,
                    selectionRule = selectionRule,
                )
                val updateValues = ContentValues().apply {
                    put(TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA, updatedProviderData.toByteArray(Charsets.UTF_8))
                }
                context.contentResolver.update(ContentUris.withAppendedId(TvContract.Programs.CONTENT_URI, selected.rowId), updateValues, null, null)
            }
            ratingSet
        }.getOrNull()
    }

    private fun fromLatestEit(
        channelUri: Uri?,
        serviceKey: ServiceKey?,
        latestEvents: List<AribEvent>,
        nowMillis: Long,
    ): CurrentProgramRatingSet? {
        val key = serviceKey ?: return null
        val event = latestEvents
            .filter { event -> event.serviceKey == key && nowMillis >= event.startTimeMillis && nowMillis < event.startTimeMillis + event.durationMillis }
            .sortedWith(compareByDescending<com.maleicacid.tvinput.aribsi.AribEvent> { it.startTimeMillis }
                .thenBy { it.startTimeMillis + it.durationMillis }
                .thenByDescending { it.eventId })
            .firstOrNull() ?: return null
        return CurrentProgramRatingSet(
            ratings = event.parentalRatings.mapNotNull { AribRatingMapper.toTvContentRating(it) },
            source = Source.LATEST_EIT_CACHE,
            channelUriString = channelUri?.toString().orEmpty(),
            serviceKey = key,
            eventId = event.eventId,
            startTimeMillis = event.startTimeMillis,
            endTimeMillis = event.startTimeMillis + event.durationMillis,
        )
    }

    companion object {
        fun stableProgramKey(serviceKey: ServiceKey, eventId: Int): String =
            "onid=${serviceKey.originalNetworkId};tsid=${serviceKey.transportStreamId};sid=${serviceKey.serviceId};event=$eventId"

        fun unblockKey(
            serviceKey: ServiceKey?,
            eventId: Int?,
            ratingString: String,
        ): String = listOf(
            serviceKey?.let { stableProgramKey(it, eventId ?: -1) }.orEmpty(),
            ratingString,
        ).joinToString("|")
    }
}

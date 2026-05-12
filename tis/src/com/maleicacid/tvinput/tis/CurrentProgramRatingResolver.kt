package com.maleicacid.tvinput.tis

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
            channelUriString = channelUriString,
            serviceKey = serviceKey,
            eventId = eventId,
            startTimeMillis = startTimeMillis,
            endTimeMillis = endTimeMillis,
            ratingString = rating.flattenToString(),
        )

        fun programIdentityKey(): String? {
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
        )
        val selection = "${TvContract.Programs.COLUMN_START_TIME_UTC_MILLIS} <= ? AND ${TvContract.Programs.COLUMN_END_TIME_UTC_MILLIS} > ?"
        val selectionArgs = arrayOf(nowMillis.toString(), nowMillis.toString())
        val sortOrder = "${TvContract.Programs.COLUMN_START_TIME_UTC_MILLIS} DESC, ${TvContract.Programs.COLUMN_END_TIME_UTC_MILLIS} ASC, ${TvContract.Programs._ID} DESC"
        return runCatching {
            var found: CurrentProgramRatingSet? = null
            context.contentResolver.query(TvContract.buildProgramsUriForChannel(channelUri), projection, selection, selectionArgs, sortOrder)?.use { cursor ->
                if (cursor.moveToFirst()) {
                    val start = cursor.getLong(2)
                    val end = cursor.getLong(3)
                    val flattened = cursor.getString(4)
                    found = CurrentProgramRatingSet(
                        ratings = AribRatingMapper.parseFlattenedList(flattened),
                        source = Source.TV_PROVIDER_CURRENT_PROGRAM,
                        channelUriString = channelUri.toString(),
                        serviceKey = serviceKey,
                        eventId = cursor.getInt(1),
                        startTimeMillis = start,
                        endTimeMillis = end,
                    )
                }
            }
            found
        }.getOrNull()
    }

    private fun fromLatestEit(
        channelUri: Uri?,
        serviceKey: ServiceKey?,
        latestEvents: List<AribEvent>,
        nowMillis: Long,
    ): CurrentProgramRatingSet? {
        val key = serviceKey ?: return null
        val event = latestEvents.firstOrNull { event ->
            event.serviceKey == key && nowMillis >= event.startTimeMillis && nowMillis < event.startTimeMillis + event.durationMillis
        } ?: return null
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
        fun unblockKey(
            channelUriString: String,
            serviceKey: ServiceKey?,
            eventId: Int?,
            startTimeMillis: Long?,
            endTimeMillis: Long?,
            ratingString: String,
        ): String = listOf(
            channelUriString,
            serviceKey?.originalNetworkId?.toString().orEmpty(),
            serviceKey?.transportStreamId?.toString().orEmpty(),
            serviceKey?.serviceId?.toString().orEmpty(),
            eventId?.toString().orEmpty(),
            startTimeMillis?.toString().orEmpty(),
            endTimeMillis?.toString().orEmpty(),
            ratingString,
        ).joinToString("|")
    }
}

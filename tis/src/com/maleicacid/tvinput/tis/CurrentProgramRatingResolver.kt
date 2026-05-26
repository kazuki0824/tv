package com.maleicacid.tvinput.tis

import android.content.ContentUris
import android.content.ContentValues
import android.content.Context
import android.media.tv.TvContentRating
import android.media.tv.TvContract
import android.net.Uri
import com.maleicacid.tvinput.aribsi.AribEvent
import com.maleicacid.tvinput.aribsi.ProviderDataBridge
import com.maleicacid.tvinput.aribsi.AribRatingMapper
import com.maleicacid.tvinput.common.ServiceKey

class CurrentProgramRatingResolver(private val context: Context) {
    enum class Source { TV_PROVIDER_CURRENT_PROGRAM, LATEST_EIT_CACHE, UNRATED_FALLBACK }

    sealed class ResolveResult {
        data class Ratings(val ratingSet: CurrentProgramRatingSet) : ResolveResult()
        data class ProviderQueryFailed(
            val channelUriString: String,
            val serviceKey: ServiceKey?,
            val reason: String,
        ) : ResolveResult()
    }

    private sealed class TvProviderLookupResult {
        data class Success(val ratingSet: CurrentProgramRatingSet?) : TvProviderLookupResult()
        data class QueryFailed(val reason: String) : TvProviderLookupResult()
    }

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
         * framework 提供 レーティングが現在 Program と一致し、現在 Program identity が完全な場合だけ
         * unblock key を返す。event / start / end identity を持たない UNRATED 代替処理 は、
         * 意図的に unblock 不可とする。
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
    ): CurrentProgramRatingSet = when (val result = resolveDetailed(channelUri, serviceKey, latestEvents, nowMillis)) {
        is ResolveResult.Ratings -> result.ratingSet
        is ResolveResult.ProviderQueryFailed -> unresolvedRatingFallback(channelUri, serviceKey)
    }

    fun resolveDetailed(
        channelUri: Uri?,
        serviceKey: ServiceKey?,
        latestEvents: List<AribEvent>,
        nowMillis: Long = System.currentTimeMillis(),
    ): ResolveResult {
        return when (val tvProvider = fromTvProvider(channelUri, serviceKey, nowMillis)) {
            is TvProviderLookupResult.Success -> {
                tvProvider.ratingSet?.let { ResolveResult.Ratings(it) }
                    ?: fromLatestEit(channelUri, serviceKey, latestEvents, nowMillis)?.let { ResolveResult.Ratings(it) }
                    ?: ResolveResult.Ratings(unresolvedRatingFallback(channelUri, serviceKey))
            }
            is TvProviderLookupResult.QueryFailed -> {
                fromLatestEit(channelUri, serviceKey, latestEvents, nowMillis)?.let { ResolveResult.Ratings(it) }
                    ?: ResolveResult.ProviderQueryFailed(
                        channelUriString = channelUri?.toString().orEmpty(),
                        serviceKey = serviceKey,
                        reason = tvProvider.reason,
                    )
            }
        }
    }

    private fun unresolvedRatingFallback(channelUri: Uri?, serviceKey: ServiceKey?): CurrentProgramRatingSet = CurrentProgramRatingSet(
        ratings = listOf(AribRatingMapper.unrated()),
        source = Source.UNRATED_FALLBACK,
        channelUriString = channelUri?.toString().orEmpty(),
        serviceKey = serviceKey,
        eventId = null,
        startTimeMillis = null,
        endTimeMillis = null,
    )

    private fun fromTvProvider(channelUri: Uri?, serviceKey: ServiceKey?, nowMillis: Long): TvProviderLookupResult {
        if (channelUri == null) return TvProviderLookupResult.Success(null)
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
        data class Candidate(
            val rowId: Long,
            val eventId: Int,
            val start: Long,
            val end: Long,
            val flattenedRatings: String?,
            val providerData: ByteArray?,
        )
        val candidates = mutableListOf<Candidate>()
        val cursor = try {
            context.contentResolver.query(TvContract.buildProgramsUriForChannel(channelUri), projection, selection, selectionArgs, sortOrder)
        } catch (e: RuntimeException) {
            return TvProviderLookupResult.QueryFailed(e.message ?: e.javaClass.name)
        } ?: return TvProviderLookupResult.QueryFailed("QUERY_RETURNED_NULL_CURSOR")
        try {
            cursor.use { current ->
                while (current.moveToNext()) {
                    val providerData = providerDataBytes(current, 5)
                    if (TvProviderWriter.providerDataMatchesService(providerData, serviceKey)) {
                        candidates += Candidate(
                            rowId = current.getLong(0),
                            eventId = current.getInt(1),
                            start = current.getLong(2),
                            end = current.getLong(3),
                            flattenedRatings = current.getString(4),
                            providerData = providerData,
                        )
                    }
                }
            }
        } catch (e: RuntimeException) {
            return TvProviderLookupResult.QueryFailed(e.message ?: e.javaClass.name)
        }
        val selected = candidates.firstOrNull() ?: return TvProviderLookupResult.Success(null)
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
                put(TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA, updatedProviderData)
            }
            context.contentResolver.update(ContentUris.withAppendedId(TvContract.Programs.CONTENT_URI, selected.rowId), updateValues, null, null)
        }
        return TvProviderLookupResult.Success(ratingSet)
    }

    private fun providerDataBytes(cursor: android.database.Cursor, index: Int): ByteArray? =
        runCatching { cursor.getBlob(index) }.getOrNull()
            ?: runCatching { cursor.getString(index)?.toByteArray(Charsets.UTF_8) }.getOrNull()

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
        fun stableProgramKey(serviceKey: ServiceKey, eventId: Int): String = ProviderDataBridge.buildProgramKey(serviceKey, eventId)

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

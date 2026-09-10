package com.maleicacid.tvinput.tis

import android.content.Context
import android.media.tv.TvContentRating
import android.media.tv.TvContract
import android.net.Uri
import com.maleicacid.tvinput.aribsi.AribEvent
import com.maleicacid.tvinput.aribsi.ProviderDataBridge
import com.maleicacid.tvinput.aribsi.AribRatingMapper
import com.maleicacid.tvinput.common.ServiceKey

class CurrentProgramRatingResolver internal constructor(
    private val queryPrograms: (Uri, Array<String>, String, Array<String>, String) -> android.database.Cursor?,
) {
    constructor(context: Context) : this({ uri, projection, selection, args, order ->
        context.contentResolver.query(uri, projection, selection, args, order)
    })
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
            channelUriString = channelUriString,
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
            if (programIdentityKey() == null || currentRowSelectionKey() == null) return null
            val requested = rating.flattenToString()
            val currentRatings = ratingsForBlocking().map { it.flattenToString() }.toSet()
            if (requested !in currentRatings) return null
            return unblockKeyFor(rating)
        }
    }

    data class CurrentProgramResolutionDiagnostic(
        val selectionRule: String,
        val overlapCount: Int,
        val selectedProgramId: Long?,
        val ratingFreshnessRule: String = "",
    )

    @Volatile
    private var currentProgramResolutionDiagnostic = CurrentProgramResolutionDiagnostic("", 0, null)

    fun currentProgramResolutionDiagnosticForTest(): CurrentProgramResolutionDiagnostic =
        currentProgramResolutionDiagnostic

    fun resolve(
        channelUri: Uri?,
        serviceKey: ServiceKey?,
        latestEvents: List<AribEvent>,
        ratingProfile: AribRatingMapper.BroadcastProfile,
        nowMillis: Long = System.currentTimeMillis(),
    ): CurrentProgramRatingSet = when (val result = resolveDetailed(channelUri, serviceKey, latestEvents, ratingProfile, nowMillis)) {
        is ResolveResult.Ratings -> result.ratingSet
        is ResolveResult.ProviderQueryFailed -> unresolvedRatingFallback(channelUri, serviceKey)
    }

    sealed class EitAuthority {
        object UNCONFIRMED : EitAuthority()
        object AUTHORITATIVE_EMPTY : EitAuthority()
        data class PresentObserved(val event: AribEvent?) : EitAuthority()
    }

    fun eitAuthority(snapshot: com.maleicacid.tvinput.aribsi.ProgramPublishSnapshot?, key: ServiceKey?): EitAuthority {
        if (snapshot == null || key == null) return EitAuthority.UNCONFIRMED
        val present = snapshot.eitInstances.singleOrNull {
            it.serviceKey == key && it.tableId == 0x4e && it.currentNextIndicator
        } ?: return EitAuthority.UNCONFIRMED
        if (present.inconsistent || 0 !in present.receivedSections || 0 !in present.safeSections || 0 in present.missingSections) {
            return EitAuthority.UNCONFIRMED
        }
        fun isPresent(source: com.maleicacid.tvinput.aribsi.AribProgramSource): Boolean =
            source.tableId == 0x4e && source.version == present.version && source.sectionNumber == 0
        val events = snapshot.events.filter { it.serviceKey == key && isPresent(it.source) }
        val excluded = snapshot.excludedEventDescriptorFacts.any { it.serviceKey == key && isPresent(it.source) }
        if (events.isEmpty() && !excluded) return EitAuthority.AUTHORITATIVE_EMPTY
        // 診断専用eventや複数presentから現在番組を推測しないが、欠測にも戻さない。
        return EitAuthority.PresentObserved(events.singleOrNull()?.takeIf {
            !excluded && (it.timingState == "DEFINED" || it.timingState == "UNDEFINED_TIME")
        })
    }

    fun resolveDetailed(
        channelUri: Uri?,
        serviceKey: ServiceKey?,
        latestEvents: List<AribEvent>,
        ratingProfile: AribRatingMapper.BroadcastProfile,
        nowMillis: Long = System.currentTimeMillis(),
        eitAuthority: EitAuthority = EitAuthority.UNCONFIRMED,
    ): ResolveResult {
        if (eitAuthority == EitAuthority.AUTHORITATIVE_EMPTY) {
            currentProgramResolutionDiagnostic = CurrentProgramResolutionDiagnostic(
                "CURRENT_GENERATION_EMPTY_EIT", 0, null, "AUTHORITATIVE_EMPTY_BEFORE_PROVIDER_QUERY",
            )
            return ResolveResult.Ratings(unresolvedRatingFallback(channelUri, serviceKey))
        }
        if (eitAuthority is EitAuthority.PresentObserved) {
            val event = eitAuthority.event?.takeIf { it.serviceKey == serviceKey }
            currentProgramResolutionDiagnostic = CurrentProgramResolutionDiagnostic(
                "CURRENT_GENERATION_PRESENT_EIT", 0, null, "PRESENT_OBSERVED_BEFORE_PROVIDER_QUERY",
            )
            return ResolveResult.Ratings(event?.let { ratingFromEvent(channelUri, it, ratingProfile) }
                ?: unresolvedRatingFallback(channelUri, serviceKey))
        }
        val latestEit = fromLatestEit(channelUri, serviceKey, latestEvents, ratingProfile, nowMillis)
        if (latestEit != null) {
            currentProgramResolutionDiagnostic = CurrentProgramResolutionDiagnostic(
                "CURRENT_GENERATION_EIT", 0, null, "LATEST_EIT_BEFORE_PROVIDER_QUERY",
            )
            return ResolveResult.Ratings(latestEit)
        }
        return when (val tvProvider = fromTvProvider(channelUri, serviceKey, nowMillis)) {
            is TvProviderLookupResult.Success -> {
                val selection = selectCurrentRating(tvProvider.ratingSet, latestEit)
                currentProgramResolutionDiagnostic = currentProgramResolutionDiagnostic.copy(
                    ratingFreshnessRule = selection.second,
                )
                ResolveResult.Ratings(selection.first ?: unresolvedRatingFallback(channelUri, serviceKey))
            }
            is TvProviderLookupResult.QueryFailed -> ResolveResult.ProviderQueryFailed(
                channelUriString = channelUri?.toString().orEmpty(),
                serviceKey = serviceKey,
                reason = tvProvider.reason,
            )
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
        )
        val candidates = mutableListOf<Candidate>()
        val cursor = try {
            queryPrograms(TvContract.buildProgramsUriForChannel(channelUri), projection, selection, selectionArgs, sortOrder)
        } catch (e: RuntimeException) {
            return TvProviderLookupResult.QueryFailed(e.message ?: e.javaClass.name)
        } ?: return TvProviderLookupResult.QueryFailed("QUERY_RETURNED_NULL_CURSOR")
        try {
            cursor.use { current ->
                while (current.moveToNext()) {
                    val providerData = current.getBlob(5)
                    if (TvProviderWriter.providerDataMatchesService(providerData, serviceKey)) {
                        candidates += Candidate(
                            rowId = current.getLong(0),
                            eventId = current.getInt(1),
                            start = current.getLong(2),
                            end = current.getLong(3),
                            flattenedRatings = current.getString(4),
                        )
                    }
                }
            }
        } catch (e: RuntimeException) {
            return TvProviderLookupResult.QueryFailed(e.message ?: e.javaClass.name)
        }
        val selected = candidates.firstOrNull()
        if (selected == null) {
            currentProgramResolutionDiagnostic = CurrentProgramResolutionDiagnostic("", 0, null)
            return TvProviderLookupResult.Success(null)
        }
        currentProgramResolutionDiagnostic = CurrentProgramResolutionDiagnostic(
            selectionRule = "START_DESC_END_ASC_ID_DESC",
            overlapCount = candidates.size,
            selectedProgramId = selected.rowId,
        )
        val ratingSet = CurrentProgramRatingSet(
            ratings = AribRatingMapper.parseFlattenedList(selected.flattenedRatings),
            source = Source.TV_PROVIDER_CURRENT_PROGRAM,
            channelUriString = channelUri.toString(),
            serviceKey = serviceKey,
            eventId = selected.eventId,
            startTimeMillis = selected.start,
            endTimeMillis = selected.end,
        )
        return TvProviderLookupResult.Success(ratingSet)
    }


    private fun fromLatestEit(
        channelUri: Uri?,
        serviceKey: ServiceKey?,
        latestEvents: List<AribEvent>,
        ratingProfile: AribRatingMapper.BroadcastProfile,
        nowMillis: Long,
    ): CurrentProgramRatingSet? {
        val key = serviceKey ?: return null
        val selected = latestEvents
            .mapNotNull { event ->
                if (event.timingState != "DEFINED") return@mapNotNull null
                val end = runCatching { Math.addExact(event.startTimeMillis, event.durationMillis) }.getOrNull()
                    ?: return@mapNotNull null
                (event to end).takeIf { event.serviceKey == key && nowMillis >= event.startTimeMillis && nowMillis < end }
            }
            .sortedWith(compareByDescending<Pair<com.maleicacid.tvinput.aribsi.AribEvent, Long>> { it.first.startTimeMillis }
                .thenBy { it.second }
                .thenByDescending { it.first.eventId })
            .firstOrNull() ?: return null
        return ratingFromEvent(channelUri, selected.first, ratingProfile)
    }

    private fun ratingFromEvent(
        channelUri: Uri?,
        event: AribEvent,
        ratingProfile: AribRatingMapper.BroadcastProfile,
    ): CurrentProgramRatingSet {
        val end = if (event.timingState == "DEFINED" && event.durationMillis > 0L) {
            runCatching { Math.addExact(event.startTimeMillis, event.durationMillis) }.getOrNull()
        } else null
        return CurrentProgramRatingSet(
            ratings = event.descriptors.parentalRatings.mapNotNull { AribRatingMapper.toTvContentRating(it, ratingProfile) },
            source = Source.LATEST_EIT_CACHE,
            channelUriString = channelUri?.toString().orEmpty(),
            serviceKey = event.serviceKey,
            eventId = event.eventId,
            startTimeMillis = event.startTimeMillis.takeIf { end != null },
            endTimeMillis = end,
        )
    }

    companion object {
        /**
         * `latestEit` は現在のtune generationへboundされているため、永続Provider rowより
         * 後の受信観測である。同一event/同一時刻ならrating refresh、同一eventの時刻変更
         * またはevent変更なら旧row失効として、いずれもEITを優先する。
         */
        private fun selectCurrentRating(
            provider: CurrentProgramRatingSet?,
            latestEit: CurrentProgramRatingSet?,
        ): Pair<CurrentProgramRatingSet?, String> = when {
            latestEit == null && provider == null -> null to "NO_CURRENT_PROGRAM"
            latestEit == null -> provider to "TV_PROVIDER_FALLBACK_NO_CURRENT_GENERATION_EIT"
            provider == null -> latestEit to "LATEST_EIT_ONLY"
            sameProgramOccurrence(provider, latestEit) -> latestEit to "LATEST_EIT_REFRESHED_SAME_OCCURRENCE"
            sameStableProgram(provider, latestEit) -> latestEit to "LATEST_EIT_SUPERSEDES_RETIMED_EVENT"
            else -> latestEit to "LATEST_EIT_SUPERSEDES_PROVIDER_EVENT"
        }

        private fun sameStableProgram(left: CurrentProgramRatingSet, right: CurrentProgramRatingSet): Boolean =
            left.serviceKey != null && left.serviceKey == right.serviceKey &&
                left.eventId != null && left.eventId == right.eventId

        private fun sameProgramOccurrence(left: CurrentProgramRatingSet, right: CurrentProgramRatingSet): Boolean =
            sameStableProgram(left, right) &&
                left.startTimeMillis != null && left.startTimeMillis == right.startTimeMillis &&
                left.endTimeMillis != null && left.endTimeMillis == right.endTimeMillis

        internal fun selectCurrentRatingForTest(
            provider: CurrentProgramRatingSet?,
            latestEit: CurrentProgramRatingSet?,
        ): CurrentProgramRatingSet? = selectCurrentRating(provider, latestEit).first

        fun stableProgramKey(serviceKey: ServiceKey, eventId: Int): String = ProviderDataBridge.buildProgramKey(serviceKey, eventId)

        fun unblockKey(
            channelUriString: String,
            serviceKey: ServiceKey?,
            eventId: Int?,
            ratingString: String,
        ): String = listOf(
            channelUriString,
            serviceKey?.let { stableProgramKey(it, eventId ?: -1) }.orEmpty(),
            ratingString,
        ).joinToString("|")
    }
}

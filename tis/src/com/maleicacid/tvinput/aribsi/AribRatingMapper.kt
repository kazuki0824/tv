package com.maleicacid.tvinput.aribsi

import android.media.tv.TvContentRating

/**
 * Maps ARIB parental_rating_descriptor values to Android TvContentRating.
 *
 * Rust owns only the ARIB descriptor parsing result. The Android rating domain is
 * fixed at the TIS boundary so Programs projection and live-session enforcement
 * use the same AOSP system-defined ISDB rating strings.
 */
object AribRatingMapper {
    const val DOMAIN = "com.android.tv"
    const val RATING_SYSTEM = "ISDB"
    const val RATING_PREFIX = "ISDB_"

    fun toTvContentRatingString(rating: AribParentalRating): String? = toTvContentRating(rating)?.flattenToString()

    fun toTvContentRating(rating: AribParentalRating): TvContentRating? {
        if (!rating.supported || rating.countryCode != "JPN") return null
        val age = rating.rating.takeIf { it in 4..20 } ?: return null
        return TvContentRating.createRating(DOMAIN, RATING_SYSTEM, "$RATING_PREFIX$age")
    }

    fun unrated(): TvContentRating = TvContentRating.UNRATED

    fun parseFlattened(value: String): TvContentRating? = runCatching { TvContentRating.unflattenFromString(value) }.getOrNull()

    fun parseFlattenedList(value: String?): List<TvContentRating> = value
        ?.split(',')
        .orEmpty()
        .map { it.trim() }
        .filter { it.isNotBlank() }
        .mapNotNull { parseFlattened(it) }
}

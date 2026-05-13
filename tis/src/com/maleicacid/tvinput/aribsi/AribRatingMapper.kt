package com.maleicacid.tvinput.aribsi

import android.media.tv.TvContentRating

/**
 * ARIB parental_rating_descriptor の値を Android TvContentRating へ写像する。
 *
 * Rust は ARIB descriptor の解析結果だけを持つ。Android rating領域は
 * TIS 境界で固定し、Programs 投影とlive sessionの制御で同じAOSP system定義
 * ISDB rating 文字列を使う。
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

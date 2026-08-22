package com.maleicacid.tvinput.aribsi

import android.media.tv.TvContentRating

/**
 * ARIB parental_rating_descriptor の値を Android TvContentRating へ写像する。
 *
 * raw ARIB 値の意味を Android 境界で潰さない。年齢レーティングは AOSP system-defined
 * ISDB domain へ、年齢値ではない放送事業者指定値は product の rating-provider domain
 * へ写像する。TvContentRating.UNRATED は rating 情報そのものが得られない場合だけに使う。
 */
object AribRatingMapper {
    const val DOMAIN = "com.android.tv"
    const val RATING_SYSTEM = "ISDB"
    const val RATING_PREFIX = "ISDB_"

    const val EXCEPTIONAL_DOMAIN = "com.maleicacid.tv.ratings"
    const val EXCEPTIONAL_RATING_SYSTEM = "ARIB_EXCEPTIONAL"
    const val EXCEPTIONAL_RATING = "BROADCASTER_DEFINED"

    fun toTvContentRatingString(rating: AribParentalRating): String? = toTvContentRating(rating)?.flattenToString()

    fun toTvContentRating(rating: AribParentalRating): TvContentRating? {
        if (rating.countryCode != "JPN") return null
        return when (val raw = rating.rawRatingByte) {
            0x00 -> null
            in 0x01..0x11 -> {
                if (!rating.supported) return null
                val age = raw + 3
                TvContentRating.createRating(DOMAIN, RATING_SYSTEM, "$RATING_PREFIX$age")
            }
            in 0x12..0xff -> TvContentRating.createRating(
                EXCEPTIONAL_DOMAIN,
                EXCEPTIONAL_RATING_SYSTEM,
                EXCEPTIONAL_RATING,
            )
            else -> null
        }
    }

    fun isProductSupported(rating: AribParentalRating): Boolean =
        rating.countryCode == "JPN" && when (rating.rawRatingByte) {
            in 0x01..0x11 -> rating.supported
            in 0x12..0xff -> true
            else -> false
        }

    fun isExceptional(rating: AribParentalRating): Boolean =
        rating.countryCode == "JPN" && rating.rawRatingByte in 0x12..0xff

    fun unrated(): TvContentRating = TvContentRating.UNRATED

    fun parseFlattened(value: String): TvContentRating? = runCatching { TvContentRating.unflattenFromString(value) }.getOrNull()

    fun parseFlattenedList(value: String?): List<TvContentRating> = value
        ?.split(',')
        .orEmpty()
        .map { it.trim() }
        .filter { it.isNotBlank() }
        .mapNotNull { parseFlattened(it) }
}

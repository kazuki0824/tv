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
    enum class BroadcastProfile { TERRESTRIAL, BS_CS, UNRESOLVED }

    const val DOMAIN = "com.android.tv"
    const val RATING_SYSTEM = "ISDB"
    const val RATING_PREFIX = "ISDB_"

    const val EXCEPTIONAL_DOMAIN = "com.maleicacid.tv.ratings"
    const val EXCEPTIONAL_RATING_SYSTEM = "ARIB_EXCEPTIONAL"
    const val EXCEPTIONAL_RATING = "BROADCASTER_DEFINED"

    fun profileForDeliverySystem(deliverySystem: String?): BroadcastProfile = when (deliverySystem) {
        "ISDB_T" -> BroadcastProfile.TERRESTRIAL
        "ISDB_S" -> BroadcastProfile.BS_CS
        else -> BroadcastProfile.UNRESOLVED
    }

    fun toTvContentRatingString(
        rating: AribParentalRating,
        profile: BroadcastProfile,
    ): String? = toTvContentRating(rating, profile)?.flattenToString()

    fun toTvContentRating(
        rating: AribParentalRating,
        profile: BroadcastProfile,
    ): TvContentRating? {
        if (rating.countryCode != "JPN") return null
        if (rating.parseStatus != "OK") return null
        if (profile != BroadcastProfile.BS_CS) return null
        return when (val raw = rating.rawRatingByte) {
            0x00 -> null
            in 0x01..0x11 -> TvContentRating.createRating(DOMAIN, RATING_SYSTEM, "$RATING_PREFIX${raw + 3}")
            in 0x12..0xff -> TvContentRating.createRating(
                EXCEPTIONAL_DOMAIN,
                EXCEPTIONAL_RATING_SYSTEM,
                EXCEPTIONAL_RATING,
            )
            else -> null
        }
    }

    fun isExceptional(rating: AribParentalRating, profile: BroadcastProfile): Boolean =
        profile == BroadcastProfile.BS_CS && rating.countryCode == "JPN" && rating.rawRatingByte in 0x12..0xff

    fun unrated(): TvContentRating = TvContentRating.UNRATED

    fun parseFlattened(value: String): TvContentRating? = runCatching { TvContentRating.unflattenFromString(value) }.getOrNull()

    fun parseFlattenedList(value: String?): List<TvContentRating> = value
        ?.split(',')
        .orEmpty()
        .map { it.trim() }
        .filter { it.isNotBlank() }
        .mapNotNull { parseFlattened(it) }
}

package com.maleicacid.tvinput.tis

import android.media.tv.TvContentRating
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.maleicacid.tvinput.aribsi.AribParentalRating
import com.maleicacid.tvinput.aribsi.AribRatingMapper
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertNotEquals
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class AribRatingMapperTest {
    @Test
    fun aribRawAgeValuesMapToAospIsdbAges() {
        assertEquals(
            TvContentRating.createRating("com.android.tv", "ISDB", "ISDB_4"),
            AribRatingMapper.toTvContentRating(rating(0x01)),
        )
        assertEquals(
            TvContentRating.createRating("com.android.tv", "ISDB", "ISDB_20"),
            AribRatingMapper.toTvContentRating(rating(0x11)),
        )
    }

    @Test
    fun explicitExceptionalValuesNeverCollapseToUnrated() {
        val expected = TvContentRating.createRating(
            AribRatingMapper.EXCEPTIONAL_DOMAIN,
            AribRatingMapper.EXCEPTIONAL_RATING_SYSTEM,
            AribRatingMapper.EXCEPTIONAL_RATING,
        )
        assertEquals(expected, AribRatingMapper.toTvContentRating(rating(0x12)))
        assertEquals(expected, AribRatingMapper.toTvContentRating(rating(0xff)))
        assertNotEquals(TvContentRating.UNRATED, expected)
    }

    @Test
    fun undefinedOrForeignRatingsDoNotInventAndroidRatings() {
        assertNull(AribRatingMapper.toTvContentRating(rating(0x00)))
        assertNull(AribRatingMapper.toTvContentRating(rating(0x12, country = "USA")))
    }

    private fun rating(raw: Int, country: String = "JPN") = AribParentalRating(
        countryCode = country,
        ratingValue = raw,
        rawRatingByte = raw,
        supported = country == "JPN" && raw in 0x01..0x11,
    )
}

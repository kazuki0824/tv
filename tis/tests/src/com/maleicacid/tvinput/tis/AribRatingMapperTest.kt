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
            AribRatingMapper.toTvContentRating(rating(0x01), AribRatingMapper.BroadcastProfile.BS_CS),
        )
        assertEquals(
            TvContentRating.createRating("com.android.tv", "ISDB", "ISDB_20"),
            AribRatingMapper.toTvContentRating(rating(0x11), AribRatingMapper.BroadcastProfile.BS_CS),
        )
    }

    @Test
    fun explicitExceptionalValuesNeverCollapseToUnrated() {
        val expected = TvContentRating.createRating(
            AribRatingMapper.EXCEPTIONAL_DOMAIN,
            AribRatingMapper.EXCEPTIONAL_RATING_SYSTEM,
            AribRatingMapper.EXCEPTIONAL_RATING,
        )
        assertEquals(expected, AribRatingMapper.toTvContentRating(rating(0x12), AribRatingMapper.BroadcastProfile.BS_CS))
        assertEquals(expected, AribRatingMapper.toTvContentRating(rating(0xff), AribRatingMapper.BroadcastProfile.BS_CS))
        assertNotEquals(TvContentRating.UNRATED, expected)
    }

    @Test
    fun undefinedOrForeignRatingsDoNotInventAndroidRatings() {
        assertNull(AribRatingMapper.toTvContentRating(rating(0x00), AribRatingMapper.BroadcastProfile.BS_CS))
        assertNull(AribRatingMapper.toTvContentRating(rating(0x12, country = "USA"), AribRatingMapper.BroadcastProfile.BS_CS))
        assertNull(AribRatingMapper.toTvContentRating(rating(0x0f), AribRatingMapper.BroadcastProfile.TERRESTRIAL))
    }

    private fun rating(raw: Int, country: String = "JPN") = AribParentalRating(
        countryCode = country,
        rawRatingByte = raw,
    )
}

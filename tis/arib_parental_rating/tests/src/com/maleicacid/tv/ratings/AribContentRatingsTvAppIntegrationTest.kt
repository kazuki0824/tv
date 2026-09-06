package com.maleicacid.tv.ratings

import android.media.tv.TvContentRating
import android.media.tv.TvInputManager
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import kotlin.test.assertFalse
import kotlin.test.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class AribContentRatingsTvAppIntegrationTest {
    @Test
    fun exceptionalRatingCanBeBlockedAndUnblockedThroughTifAuthority() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val manager = context.getSystemService(TvInputManager::class.java)
            ?: error("TvInputManagerを取得できません")
        val rating = TvContentRating.createRating(
            "com.maleicacid.tv.ratings",
            "ARIB_EXCEPTIONAL",
            "BROADCASTER_DEFINED",
        )
        val initiallyBlocked = manager.isRatingBlocked(rating)

        try {
            manager.addBlockedRating(rating)
            assertTrue(manager.isRatingBlocked(rating))

            manager.removeBlockedRating(rating)
            assertFalse(manager.isRatingBlocked(rating))
        } finally {
            if (initiallyBlocked) {
                manager.addBlockedRating(rating)
            } else {
                manager.removeBlockedRating(rating)
            }
        }
    }
}

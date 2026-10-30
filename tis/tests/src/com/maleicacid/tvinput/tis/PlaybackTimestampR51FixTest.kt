// テストのPTS値は本体の定数と独立した具体値で記述する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import org.junit.Test

class PlaybackTimestampR51FixTest {
    @Test fun authoritativePtsIsRequiredWithoutConsultingProvenanceBit() {
        check(PlaybackPipeline.isAuthoritativePtsValid(90_000L))
        check(PlaybackPipeline.isAuthoritativePtsValid((1L shl 33) - 1))
        check(!PlaybackPipeline.isAuthoritativePtsValid(1L shl 33))
        check(!PlaybackPipeline.isAuthoritativePtsValid(null))
        check(!PlaybackPipeline.isAuthoritativePtsValid(-1L))
    }
}

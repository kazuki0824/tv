package com.maleicacid.tvinput.aribsi.generated


sealed class EitTimingStateDto {

    object DEFINED : EitTimingStateDto()

    object UNDEFINED_TIME : EitTimingStateDto()

    object BOTH_TIMING_UNDEFINED : EitTimingStateDto()

    object MALFORMED_TIMING : EitTimingStateDto()
}


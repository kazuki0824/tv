package com.maleicacid.tvinput.aribsi.generated


sealed class EitTimingStateDto {

    object Defined : EitTimingStateDto()

    object UndefinedTime : EitTimingStateDto()

    object BothTimingUndefined : EitTimingStateDto()

    object MalformedTiming : EitTimingStateDto()
}

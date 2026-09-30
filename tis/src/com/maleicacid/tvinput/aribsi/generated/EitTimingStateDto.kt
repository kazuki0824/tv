// serde の wire enum 名を維持する生成ファイル。
@file:Suppress("ClassName")

package com.maleicacid.tvinput.aribsi.generated


sealed class EitTimingStateDto {

    object DEFINED : EitTimingStateDto()

    object UNDEFINED_TIME : EitTimingStateDto()

    object BOTH_TIMING_UNDEFINED : EitTimingStateDto()

    object MALFORMED_TIMING : EitTimingStateDto()
}


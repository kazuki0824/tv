// serde の wire enum 名を維持する生成ファイル。
@file:Suppress("ClassName")

package com.maleicacid.tvinput.aribsi.generated


sealed class BroadcastSystemDto {

    object ISDB_T : BroadcastSystemDto()

    object ISDB_S_BS : BroadcastSystemDto()

    object ISDB_S_110CS : BroadcastSystemDto()
}


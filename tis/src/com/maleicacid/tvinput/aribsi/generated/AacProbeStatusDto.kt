package com.maleicacid.tvinput.aribsi.generated


sealed class AacProbeStatusDto {

    object Pending : AacProbeStatusDto()

    object Invalid : AacProbeStatusDto()

    object Ready : AacProbeStatusDto()
}

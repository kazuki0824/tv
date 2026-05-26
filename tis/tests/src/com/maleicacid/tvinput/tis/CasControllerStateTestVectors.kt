package com.maleicacid.tvinput.tis

import com.maleicacid.tvinput.aribsi.CaMetadata
import com.maleicacid.tvinput.aribsi.CaMetadataSource
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.TsPid

/** CasController の状態遷移確認用ベクトル。 */
object CasControllerStateTestVectors {
    val serviceKey = ServiceKey(originalNetworkId = 4, transportStreamId = 16625, serviceId = 101)

    fun pluginSelectionSuccessMetadata(): List<CaMetadata> = listOf(
        CaMetadata(serviceKey, CasController.SupportedCasSystemIds.ARIB_STD_B25, ecmPid = TsPid(0x123), emmPid = null, elementaryPid = null, privateData = byteArrayOf(0x01), source = CaMetadataSource.PROGRAM),
        CaMetadata(serviceKey, CasController.SupportedCasSystemIds.ARIB_STD_B25, ecmPid = TsPid(0x123), emmPid = null, elementaryPid = TsPid(0x101), privateData = byteArrayOf(0x02), source = CaMetadataSource.ELEMENTARY_STREAM),
        CaMetadata(null, CasController.SupportedCasSystemIds.ARIB_STD_B25, ecmPid = null, emmPid = TsPid(0x010), elementaryPid = null, privateData = byteArrayOf(0x03), source = CaMetadataSource.CAT),
    )

    fun unsupportedSystemIdMetadata(): List<CaMetadata> = listOf(
        CaMetadata(serviceKey, 0x7fff, ecmPid = TsPid(0x123), emmPid = null, elementaryPid = TsPid(0x101), source = CaMetadataSource.ELEMENTARY_STREAM),
    )

    fun pmtUpdateRemovesOldPidMetadata(): Pair<List<CaMetadata>, List<CaMetadata>> = Pair(
        listOf(CaMetadata(serviceKey, CasController.SupportedCasSystemIds.ARIB_STD_B25, ecmPid = TsPid(0x123), emmPid = null, elementaryPid = TsPid(0x101), source = CaMetadataSource.ELEMENTARY_STREAM)),
        listOf(CaMetadata(serviceKey, CasController.SupportedCasSystemIds.ARIB_STD_B25, ecmPid = TsPid(0x124), emmPid = null, elementaryPid = TsPid(0x102), source = CaMetadataSource.ELEMENTARY_STREAM)),
    )
}

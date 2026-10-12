package com.maleicacid.tvinput.aribsi

import com.maleicacid.tvinput.common.TsPid

class PmtCatCaMetadataMapper {
    fun programLevel(metadata: List<CaMetadata>): List<CaMetadata> =
        metadata.filter {
            it.source == CaMetadataSource.PROGRAM && it.ecmPid != null && it.elementaryPid == null &&
                it.serviceKey != null
        }

    /**
     * PMT の番組単位 CA_descriptor を ES PID 単位の束縛へ展開する。
     * 同じ サービス、CA_system_id、ECM PID の ES-level CA_descriptor がない場合に使う。
     * PMT/CAT の意味解釈は TIS 側 SI 解析器と制御部に留める。
     */
    @Suppress("MaxLineLength")
    fun expandProgramLevelToElementaryStreams(
        metadata: List<CaMetadata>,
        services: List<AribService>,
    ): List<CaMetadata> {
        val serviceByKey = services.associateBy { it.serviceKey }
        // 第2ループでESのCA方式が明示されていれば、第1ループの同方式のECMを
        // そのESへ重複展開しない。ECM_PID=0x1FFFも明示的な非スクランブル指定。
        val explicitEsKeys =
            metadata
                .filter {
                    it.source == CaMetadataSource.ELEMENTARY_STREAM &&
                        it.serviceKey != null && it.elementaryPid != null
                }.map { Triple(it.serviceKey, it.caSystemId, it.elementaryPid) }
                .toSet()
        // MediaCasへ配送するmetadataはECMを実際に送出するものだけにする。
        val activeMetadata = metadata.filter { it.ecmPid != TsPid.NULL }
        val existingEsKeys =
            activeMetadata
                .filter {
                    it.source == CaMetadataSource.ELEMENTARY_STREAM && it.serviceKey != null && it.ecmPid != null &&
                        it.elementaryPid != null
                }.map { EsBindingKey(it.serviceKey.toString(), it.caSystemId, it.ecmPid!!, it.elementaryPid!!) }
                .toMutableSet()
        val expanded = mutableListOf<CaMetadata>()
        programLevel(activeMetadata).forEach { programCa ->
            val service = serviceByKey[programCa.serviceKey] ?: return@forEach
            val ecmPid = programCa.ecmPid ?: return@forEach
            service.streams.forEach stream@{ stream ->
                if (Triple(programCa.serviceKey, programCa.caSystemId, stream.elementaryPid) in explicitEsKeys) return@stream
                val key = EsBindingKey(programCa.serviceKey.toString(), programCa.caSystemId, ecmPid, stream.elementaryPid)
                if (existingEsKeys.add(key)) {
                    expanded +=
                        programCa.copy(
                            elementaryPid = stream.elementaryPid,
                            source = CaMetadataSource.ELEMENTARY_STREAM,
                        )
                }
            }
        }
        return activeMetadata + expanded
    }

    private data class EsBindingKey(
        val serviceKey: String,
        val caSystemId: Int,
        val ecmPid: TsPid,
        val elementaryPid: TsPid,
    )
}

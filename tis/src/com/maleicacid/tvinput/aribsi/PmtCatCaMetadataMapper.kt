package com.maleicacid.tvinput.aribsi

class PmtCatCaMetadataMapper {
    fun programLevel(metadata: List<CaMetadata>): List<CaMetadata> {
        return metadata.filter { it.source == CaMetadataSource.PROGRAM && it.ecmPid != null && it.elementaryPid == null && it.serviceKey != null }
    }

    fun elementaryStreamLevel(metadata: List<CaMetadata>): List<CaMetadata> {
        return metadata.filter { it.source == CaMetadataSource.ELEMENTARY_STREAM && it.ecmPid != null && it.elementaryPid != null }
    }

    fun emm(metadata: List<CaMetadata>): List<CaMetadata> {
        return metadata.filter { it.source == CaMetadataSource.CAT && it.emmPid != null }
    }

    fun unsupportedForB25B1(metadata: List<CaMetadata>, supportedSystemIds: Set<Int>): List<CaMetadata> {
        return metadata.filterNot { it.caSystemId in supportedSystemIds }
    }

    /**
     * PMT の番組単位 CA_descriptor を ES PID 単位の束縛へ展開する。
     * 同じ サービス、CA_system_id、ECM PID の ES-level CA_descriptor がない場合に使う。
     * PMT/CAT の意味解釈は TIS 側 SI 解析器と制御部に留める。
     */
    fun expandProgramLevelToElementaryStreams(
        metadata: List<CaMetadata>,
        services: List<AribService>,
    ): List<CaMetadata> {
        val serviceByKey = services.associateBy { it.serviceKey }
        val existingEsKeys = metadata
            .filter { it.source == CaMetadataSource.ELEMENTARY_STREAM && it.serviceKey != null && it.ecmPid != null && it.elementaryPid != null }
            .map { EsBindingKey(it.serviceKey.toString(), it.caSystemId, it.ecmPid!!, it.elementaryPid!!) }
            .toMutableSet()
        val expanded = mutableListOf<CaMetadata>()
        programLevel(metadata).forEach { programCa ->
            val service = serviceByKey[programCa.serviceKey] ?: return@forEach
            val ecmPid = programCa.ecmPid ?: return@forEach
            service.streams.forEach { stream ->
                val key = EsBindingKey(programCa.serviceKey.toString(), programCa.caSystemId, ecmPid, stream.elementaryPid)
                if (existingEsKeys.add(key)) {
                    expanded += programCa.copy(
                        elementaryPid = stream.elementaryPid,
                        source = CaMetadataSource.ELEMENTARY_STREAM,
                    )
                }
            }
        }
        return metadata + expanded
    }

    private data class EsBindingKey(
        val serviceKey: String,
        val caSystemId: Int,
        val ecmPid: Int,
        val elementaryPid: Int,
    )
}

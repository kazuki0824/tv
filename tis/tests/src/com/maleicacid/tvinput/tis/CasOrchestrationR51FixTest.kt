package com.maleicacid.tvinput.tis

import com.maleicacid.tvinput.aribsi.CaMetadata
import com.maleicacid.tvinput.aribsi.CaMetadataSource
import com.maleicacid.tvinput.aribsi.PmtCatCaMetadataMapper
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.TsPid
import org.junit.Test

class CasOrchestrationR51FixTest {
    @Test fun catOnlyDoesNotRequireCasButIsRetainedForEmmFilter() {
        val catOnly = listOf(CaMetadata(null, 0x0005, ecmPid = null, emmPid = TsPid(0x0010), elementaryPid = null, source = CaMetadataSource.CAT))
        val mapped = PmtCatCaMetadataMapper().expandProgramLevelToElementaryStreams(catOnly, services = emptyList())
        check(mapped.single().emmPid == TsPid(0x0010))
        check(mapped.none { it.source != CaMetadataSource.CAT && it.serviceKey != null })
    }

    @Test fun pmtOrEsCaRequiresCas() {
        val key = ServiceKey(4, 16625, 101)
        val program = CaMetadata(key, 0x0005, ecmPid = TsPid(0x1fff), emmPid = null, elementaryPid = null, source = CaMetadataSource.PROGRAM)
        val es = CaMetadata(key, 0x0005, ecmPid = TsPid(0x1ffe), emmPid = null, elementaryPid = TsPid(0x0100), source = CaMetadataSource.ELEMENTARY_STREAM)
        check(listOf(program, es).any { it.source != CaMetadataSource.CAT && it.serviceKey != null })
    }
}

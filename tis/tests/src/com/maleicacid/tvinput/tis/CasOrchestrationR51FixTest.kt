// テストの入力・期待値を本体の定数と独立した具体値で記述する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import com.maleicacid.tvinput.aribsi.AribElementaryStream
import com.maleicacid.tvinput.aribsi.AribService
import com.maleicacid.tvinput.aribsi.CaMetadata
import com.maleicacid.tvinput.aribsi.CaMetadataSource
import com.maleicacid.tvinput.aribsi.PmtCatCaMetadataMapper
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.TsPid
import org.junit.Test

class CasOrchestrationR51FixTest {
    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    @Test
    fun catOnlyDoesNotRequireCasButIsRetainedForEmmFilter() {
        val catOnly =
            listOf(CaMetadata(null, 0x0005, ecmPid = null, emmPid = TsPid(0x0010), elementaryPid = null, source = CaMetadataSource.CAT))
        val mapped = PmtCatCaMetadataMapper().expandProgramLevelToElementaryStreams(catOnly, services = emptyList())
        check(mapped.single().emmPid == TsPid(0x0010))
        check(mapped.none { it.source != CaMetadataSource.CAT && it.serviceKey != null })
    }

    // 実TSの第一ループECM=0x0031、ES側非スクランブル=0x1FFFを最小fixtureで固定する。
    @Suppress("MaxLineLength")
    @Test
    fun nullEcmPidMarksClearEsWithoutCompetingSessions() {
        val key = ServiceKey(32739, 32739, 1048)
        val videoPid = TsPid(273)
        val audioPid = TsPid(274)
        val dataPid = TsPid(2142)
        val service =
            AribService(
                key,
                "fixture",
                streams =
                    listOf(videoPid to 0x02, audioPid to 0x0f, dataPid to 0x0d).map { (pid, type) ->
                        AribElementaryStream(pid, type, null, null, null)
                    },
            )
        val program = CaMetadata(key, 0x0005, TsPid(0x0031), null, null, source = CaMetadataSource.PROGRAM)
        val clearData = CaMetadata(key, 0x0005, TsPid(0x1fff), null, dataPid, source = CaMetadataSource.ELEMENTARY_STREAM)
        val mapper = PmtCatCaMetadataMapper()
        val result = mapper.expandProgramLevelToElementaryStreams(listOf(program, clearData), listOf(service))
        check(result.none { it.ecmPid == TsPid(0x1fff) })
        check(result.any { it.ecmPid == TsPid(0x0031) && it.elementaryPid == videoPid })
        check(result.any { it.ecmPid == TsPid(0x0031) && it.elementaryPid == audioPid })
        check(result.none { it.elementaryPid == dataPid })
        // 第2ループの実ECMが第1ループのECMと異なる場合も、ESの明示指定を優先する。
        val esSpecific = clearData.copy(ecmPid = TsPid(0x0032))
        val mappedSpecific = mapper.expandProgramLevelToElementaryStreams(listOf(program, esSpecific), listOf(service))
        check(mappedSpecific.count { it.elementaryPid == dataPid } == 1)
        check(mappedSpecific.single { it.elementaryPid == dataPid }.ecmPid == TsPid(0x0032))
        check(mapper.expandProgramLevelToElementaryStreams(listOf(program.copy(ecmPid = TsPid(0x1fff))), listOf(service)).isEmpty())

        val allClear = listOf(videoPid, audioPid, dataPid).map { clearData.copy(elementaryPid = it) }
        val allClearResult = mapper.expandProgramLevelToElementaryStreams(listOf(program) + allClear, listOf(service))
        check(allClearResult.isEmpty()) // Rustのraw factとは別にruntimeの不要PROGRAM sessionを除外

        val allSpecific =
            listOf(videoPid, audioPid, dataPid).mapIndexed { i, pid ->
                clearData.copy(elementaryPid = pid, ecmPid = TsPid(0x0032 + i))
            }
        val allSpecificResult = mapper.expandProgramLevelToElementaryStreams(listOf(program) + allSpecific, listOf(service))
        check(allSpecificResult.size == 3)
        check(allSpecificResult.none { it.source == CaMetadataSource.PROGRAM })
        check(allSpecificResult.mapNotNull { it.ecmPid }.toSet().size == 3)
    }

    @Test
    fun clearServiceWithCatDoesNotInvokeUnavailablePluginOrBlockPlayback() {
        val key = ServiceKey(4, 16625, 101)
        val cat = CaMetadata(null, 0x0005, null, TsPid(0x0010), null, source = CaMetadataSource.CAT)
        val decision = ServicePolicyDecision(key, true, false, true, emptyList())
        check(decision.livePlaybackEligible(casLinkageReady = false))
        val runtime = SectionFilterPolicy.metadataForLiveService(decision.requiresCas, key, listOf(cat))
        check(runtime.isEmpty())
        val unavailableFactory =
            object : CasController.MediaCasBridgeFactory {
                override fun create(caSystemId: Int): Result<CasController.MediaCasBridge> =
                    Result.failure(IllegalStateException("B25 plugin unavailable"))
            }
        CasController(mediaCasFactory = unavailableFactory).use { controller ->
            val update = controller.updateFromCaMetadata(runtime, generation = 1)
            check(update.diagnostics.isEmpty() && update.ecmPids.isEmpty() && update.emmPids.isEmpty())
            check(controller.lastDiagnostic().state == CasController.State.IDLE)
            // CATのみを直接渡した場合はplugin失敗となることも同じfactoryで確認。
            val invalid = controller.updateFromCaMetadata(listOf(cat), generation = 1)
            check(invalid.diagnostics.any { it.errorCode == CasController.ErrorCode.PLUGIN_UNAVAILABLE })
        }
    }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    @Test
    fun pmtOrEsCaRequiresCas() {
        val key = ServiceKey(4, 16625, 101)
        val program =
            CaMetadata(key, 0x0005, ecmPid = TsPid(0x1fff), emmPid = null, elementaryPid = null, source = CaMetadataSource.PROGRAM)
        val es =
            CaMetadata(
                key,
                0x0005,
                ecmPid = TsPid(0x1ffe),
                emmPid = null,
                elementaryPid = TsPid(0x0100),
                source = CaMetadataSource.ELEMENTARY_STREAM,
            )
        check(listOf(program, es).any { it.source != CaMetadataSource.CAT && it.serviceKey != null })
    }
}

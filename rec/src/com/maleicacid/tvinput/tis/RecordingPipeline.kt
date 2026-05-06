package com.maleicacid.tvinput.tis

import android.content.Context
import android.media.tv.TvInputService
import android.net.Uri
import android.util.Log
import com.maleicacid.tvinput.aribsi.AribSiEngine
import com.maleicacid.tvinput.aribsi.SectionIngestController
import com.maleicacid.tvinput.common.LogTags

class RecordingPipeline(
    private val context: Context,
    private val inputId: String,
) {
    private val aribSiEngine = AribSiEngine(context)
    private val sectionIngestController = SectionIngestController(aribSiEngine)
    private val tunerController = TunerController(context, inputId, TvInputService.PRIORITY_HINT_USE_CASE_TYPE_RECORD)
    private val casController = CasController()
    private val caMapper = com.maleicacid.tvinput.aribsi.PmtCatCaMetadataMapper()

    init {
        tunerController.setSectionIngestController(sectionIngestController)
        tunerController.setCasController(casController)
        tunerController.setOnSectionIngestedCallback { refreshDynamicSiAndCasFilters() }
    }

    fun tune(channelUri: Uri) {
        Log.i(LogTags.TIS, "録画用 tune inputId=$inputId channelUri=$channelUri")
        tunerController.tuneForLive(channelUri)
        refreshDynamicSiAndCasFilters()
    }

    fun startRecording(programUri: Uri?) {
        refreshDynamicSiAndCasFilters()
        Log.i(LogTags.TIS, "録画開始 inputId=$inputId programUri=$programUri")
    }

    fun stopRecording(): Uri? {
        Log.i(LogTags.TIS, "録画停止 inputId=$inputId")
        return null
    }

    private fun refreshDynamicSiAndCasFilters() {
        val caMetadata = caMapper.expandProgramLevelToElementaryStreams(aribSiEngine.snapshotCaMetadata(), aribSiEngine.snapshotServices())
        val pmtPids = aribSiEngine.snapshotPmtPids().map { it.pmtPid }.filter { it in 0..0x1fff }.toSet()
        val ecmPids = caMetadata.mapNotNull { it.ecmPid }.filter { it in 0..0x1fff }.toSet()
        val emmPids = caMetadata.mapNotNull { it.emmPid }.filter { it in 0..0x1fff }.toSet()
        tunerController.openDynamicFiltersFromCurrentSi(pmtPids, ecmPids, emmPids)
        casController.updateFromCaMetadata(caMetadata, tunerController.createDescramblerBridge())
    }

    fun release() {
        casController.close()
        tunerController.release()
        aribSiEngine.close()
        Log.i(LogTags.TIS, "録画経路を解放します inputId=$inputId")
    }
}

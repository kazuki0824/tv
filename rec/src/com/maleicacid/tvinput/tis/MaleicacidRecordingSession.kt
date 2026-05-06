package com.maleicacid.tvinput.tis

import android.content.Context
import android.media.tv.TvInputService
import android.net.Uri

class MaleicacidRecordingSession(
    context: Context,
    private val inputId: String,
) : TvInputService.RecordingSession(context) {
    private val recordingPipeline = RecordingPipeline(context, inputId)
    private var tunedChannelUri: Uri? = null

    override fun onTune(channelUri: Uri?) {
        tunedChannelUri = channelUri
        if (channelUri != null) {
            recordingPipeline.tune(channelUri)
            notifyTuned(channelUri)
        }
    }

    override fun onStartRecording(programUri: Uri?) {
        recordingPipeline.startRecording(programUri)
    }

    override fun onStopRecording() {
        val recordedProgramUri = recordingPipeline.stopRecording()
        notifyRecordingStopped(recordedProgramUri)
    }

    override fun onRelease() {
        recordingPipeline.release()
    }
}

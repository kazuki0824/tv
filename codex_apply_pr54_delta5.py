from pathlib import Path
import re

ROOT = Path('.')

def replace_once(path, old, new, label):
    p = ROOT / path
    text = p.read_text(encoding='utf-8')
    count = text.count(old)
    if count != 1:
        raise SystemExit(f'{label}: expected 1 occurrence, found {count}')
    p.write_text(text.replace(old, new, 1), encoding='utf-8')

# #10: do not invent a TIF session id for the legacy one-argument entry point.
replace_once(
    'tis/src/com/maleicacid/tvinput/tis/MaleicacidTvInputService.kt',
    '''    override fun onCreateSession(inputId: String): Session {\n        val fallbackSessionId = fallbackSessionId(inputId)\n        Log.i(LogTags.TIS, "旧1引数 onCreateSession 経路でライブセッションを作成します inputId=$inputId fallbackSessionId=$fallbackSessionId")\n        return createLiveSession(inputId, fallbackSessionId, this)\n    }\n''',
    '''    override fun onCreateSession(inputId: String): Session? {\n        Log.w(LogTags.TIS, "TIF sessionId がない旧1引数 onCreateSession 経路では TRM/Tuner ライブセッションを作成しません inputId=$inputId")\n        return null\n    }\n''',
    'legacy session creation',
)
replace_once(
    'tis/src/com/maleicacid/tvinput/tis/MaleicacidTvInputService.kt',
    '''\n    private fun fallbackSessionId(inputId: String): String = legacyFallbackSessionIdForTest(inputId)\n''',
    '',
    'fallback session helper',
)
replace_once(
    'tis/src/com/maleicacid/tvinput/tis/MaleicacidTvInputService.kt',
    '''\n        fun legacyFallbackSessionIdForTest(inputId: String): String = "maleicacid-$inputId-${System.nanoTime()}"\n''',
    '',
    'legacy fallback test helper',
)

# #24 + #10 + #7 plumbing in TunerController.
path = 'tis/src/com/maleicacid/tvinput/tis/TunerController.kt'
replace_once(path,
    'import android.media.tv.tuner.frontend.FrontendSettings\n',
    'import android.media.tv.tuner.frontend.FrontendSettings\nimport android.media.tv.tuner.frontend.OnTuneEventListener\n',
    'OnTuneEventListener import')
replace_once(path,
    '''        val subtitleLanguageId: Int? = null,\n        val superimpose: AribElementaryStream? = null,\n    )\n''',
    '''        val subtitleLanguageId: Int? = null,\n        val superimpose: AribElementaryStream? = null,\n        val dualMonoPresentation: PlaybackPipeline.DualMonoPresentation = PlaybackPipeline.DualMonoPresentation.MAIN,\n    )\n''',
    'AvStreamSelection dual mono')
replace_once(path,
    '''    private var onSectionIngestedCallback: (() -> Unit)? = null\n    private var onTunerResourceLostCallback: ((Long) -> Unit)? = null\n    private val tvInputSessionId: String = sessionId?.takeIf { it.isNotBlank() } ?: "maleicacid-$inputId-${System.nanoTime()}"\n''',
    '''    private var onSectionIngestedCallback: (() -> Unit)? = null\n    private var onTunerResourceLostCallback: ((Long) -> Unit)? = null\n    private var onTuneEventCallback: ((Long, Int) -> Unit)? = null\n    private val tvInputSessionId: String? = normalizedTvInputSessionId(sessionId)\n''',
    'callbacks and session id')
replace_once(path,
    '''    fun setOnTunerResourceLostCallback(callback: ((Long) -> Unit)?) = callOnController { onTunerResourceLostCallback = callback }\n\n    fun setPlaybackCallbacks''',
    '''    fun setOnTunerResourceLostCallback(callback: ((Long) -> Unit)?) = callOnController { onTunerResourceLostCallback = callback }\n\n    fun setOnTuneEventCallback(callback: ((Long, Int) -> Unit)?) = callOnController { onTuneEventCallback = callback }\n\n    fun setPlaybackCallbacks''',
    'tune event setter')
replace_once(path,
    '''    private fun handleTunerResourceLostOnController() {\n        val lostGeneration = tuneGeneration\n        playbackPipeline.stop()\n        closeSectionFiltersOnController()\n        currentTune = null\n        tuneAccepted = false\n        descramblerBridge = null\n        onTunerResourceLostCallback?.invoke(lostGeneration)\n    }\n''',
    '''    private fun handleTunerResourceLostOnController() {\n        val lostGeneration = tuneGeneration\n        playbackPipeline.stop()\n        closeSectionFiltersOnController()\n        currentTune = null\n        tuneAccepted = false\n        descramblerBridge = null\n        onTunerResourceLostCallback?.invoke(lostGeneration)\n    }\n\n    private fun armTuneEventListener(tunerInstance: Tuner, generation: Long): Boolean {\n        if (onTuneEventCallback == null) return true\n        return runCatching {\n            tunerInstance.setOnTuneEventListener(sectionExecutor) { event ->\n                if (tunerInstance === tuner && !released) handleTuneEventOnController(generation, event)\n            }\n        }.onFailure { error ->\n            Log.w(LogTags.TIS, "frontend tune event listener 登録に失敗しました inputId=$inputId generation=$generation", error)\n        }.isSuccess\n    }\n\n    private fun handleTuneEventOnController(generation: Long, event: Int) {\n        if (!tuneAccepted || generation != tuneGeneration || currentTune == null) return\n        when (event) {\n            OnTuneEventListener.SIGNAL_NO_SIGNAL, OnTuneEventListener.SIGNAL_LOST_LOCK -> playbackPipeline.stop()\n            OnTuneEventListener.SIGNAL_LOCKED -> Unit\n            else -> return\n        }\n        onTuneEventCallback?.invoke(generation, event)\n    }\n''',
    'tune event handler')
replace_once(path,
    '''        val result = tunerInstance.tune(settings)\n        if (result == Tuner.RESULT_SUCCESS) {\n            tuneAccepted = true\n            tuneGeneration++\n            beginSiIngestAfterTune()\n        }\n''',
    '''        val result = tunerInstance.tune(settings)\n        if (result == Tuner.RESULT_SUCCESS) {\n            tuneAccepted = true\n            tuneGeneration++\n            beginSiIngestAfterTune()\n        }\n''',
    'scan tune block unchanged guard')
# Live tune: arm listener with the generation that will be committed by this tune.
replace_once(path,
    '''        val result = runCatching { tunerInstance.tune(settings) }.getOrElse { e ->\n            Log.w(LogTags.TIS, "Tuner.tune が例外を返しました inputId=$inputId channel=$channel", e)\n            return TuneOutcome(false, Tuner.RESULT_UNAVAILABLE, channel, tuneGeneration, e.message.orEmpty())\n        }\n        return if (result == Tuner.RESULT_SUCCESS) {\n            currentTune = channel\n            tuneAccepted = true\n            tuneGeneration++\n            beginSiIngestAfterTune()\n            TuneOutcome(true, result, channel, tuneGeneration)\n        } else {\n            currentTune = null\n            tuneAccepted = false\n            playbackPipeline.stop()\n            TuneOutcome(false, result, channel, tuneGeneration, "Tuner.tune に失敗しました result=$result")\n        }\n''',
    '''        val nextGeneration = tuneGeneration + 1L\n        if (!armTuneEventListener(tunerInstance, nextGeneration)) {\n            return TuneOutcome(false, Tuner.RESULT_UNAVAILABLE, channel, tuneGeneration, "frontend tune event listenerを登録できません")\n        }\n        val result = runCatching { tunerInstance.tune(settings) }.getOrElse { e ->\n            runCatching { tunerInstance.clearOnTuneEventListener() }\n            Log.w(LogTags.TIS, "Tuner.tune が例外を返しました inputId=$inputId channel=$channel", e)\n            return TuneOutcome(false, Tuner.RESULT_UNAVAILABLE, channel, tuneGeneration, e.message.orEmpty())\n        }\n        return if (result == Tuner.RESULT_SUCCESS) {\n            currentTune = channel\n            tuneAccepted = true\n            tuneGeneration = nextGeneration\n            beginSiIngestAfterTune()\n            TuneOutcome(true, result, channel, tuneGeneration)\n        } else {\n            runCatching { tunerInstance.clearOnTuneEventListener() }\n            currentTune = null\n            tuneAccepted = false\n            playbackPipeline.stop()\n            TuneOutcome(false, result, channel, tuneGeneration, "Tuner.tune に失敗しました result=$result")\n        }\n''',
    'live tune generation listener')
replace_once(path,
    '''    private fun resetBeforeTune() {\n        playbackPipeline.stop()\n        closeSectionFilters()\n''',
    '''    private fun resetBeforeTune() {\n        playbackPipeline.stop()\n        runCatching { tuner?.clearOnTuneEventListener() }\n        closeSectionFilters()\n''',
    'clear old tune listener')
replace_once(path,
    '''        subtitleExplicitlyDisabled: Boolean = false,\n        defaultComponentGroupTags: Set<Int>? = null,\n    ): AvStreamSelection {\n''',
    '''        subtitleExplicitlyDisabled: Boolean = false,\n        defaultComponentGroupTags: Set<Int>? = null,\n        dualMonoPresentation: PlaybackPipeline.DualMonoPresentation = PlaybackPipeline.DualMonoPresentation.MAIN,\n    ): AvStreamSelection {\n''',
    'select args dual mono')
replace_once(path,
    '''        return AvStreamSelection(serviceKey, pcrPid, video, audio, subtitle, selectedCaptionTrack?.captionLanguageId, superimpose)\n''',
    '''        return AvStreamSelection(serviceKey, pcrPid, video, audio, subtitle, selectedCaptionTrack?.captionLanguageId, superimpose, dualMonoPresentation)\n''',
    'selection return dual mono')
replace_once(path,
    '''    fun stopPlayback() {\n        playbackPipeline.stop()\n    }\n''',
    '''    fun setDualMonoPresentation(presentation: PlaybackPipeline.DualMonoPresentation): Boolean =\n        playbackPipeline.setDualMonoPresentation(presentation)\n\n    fun stopPlayback() {\n        playbackPipeline.stop()\n    }\n''',
    'dual mono delegate')
replace_once(path,
    '''        onSectionIngestedCallback = null\n        currentTune = null\n''',
    '''        onSectionIngestedCallback = null\n        onTuneEventCallback = null\n        runCatching { tuner?.clearOnTuneEventListener() }\n        currentTune = null\n''',
    'release tune listener')
replace_once(path,
    '''    companion object {\n        private const val SECTION_FILTER_BUFFER_BYTES = 64 * 1024L\n    }\n''',
    '''    companion object {\n        private const val SECTION_FILTER_BUFFER_BYTES = 64 * 1024L\n\n        internal fun normalizedTvInputSessionId(sessionId: String?): String? =\n            sessionId?.takeIf { it.isNotBlank() }\n\n        internal fun isSignalUnavailableTuneEventForTest(event: Int): Boolean =\n            event == OnTuneEventListener.SIGNAL_NO_SIGNAL || event == OnTuneEventListener.SIGNAL_LOST_LOCK\n    }\n''',
    'TunerController companion helpers')

# #7 + #24 + #10 live-session state and private command.
path = 'tis/src/com/maleicacid/tvinput/tis/MaleicacidLiveSession.kt'
replace_once(path,
    'import android.net.Uri\nimport android.os.Build\n',
    'import android.net.Uri\nimport android.os.Build\nimport android.os.Bundle\nimport android.media.tv.tuner.frontend.OnTuneEventListener\n',
    'LiveSession imports')
replace_once(path,
    '''    private val inputId: String,\n    private val sessionId: String? = null,\n) : TvInputService.Session(sessionContext) {\n''',
    '''    private val inputId: String,\n    private val sessionId: String,\n) : TvInputService.Session(sessionContext) {\n''',
    'non-null session id')
replace_once(path,
    '''        Thread(runnable, "maleicacid-live-session-${sessionId ?: "legacy"}").also { thread ->\n''',
    '''        Thread(runnable, "maleicacid-live-session-$sessionId").also { thread ->\n''',
    'session thread name')
replace_once(path,
    '''    private var audioFallbackDisabled: Boolean = false\n    private var selectedSubtitleTrackId: String? = null\n''',
    '''    private var audioFallbackDisabled: Boolean = false\n    private var dualMonoPresentation: PlaybackPipeline.DualMonoPresentation = PlaybackPipeline.DualMonoPresentation.MAIN\n    private var frontendSignalUnavailable: Boolean = false\n    private var selectedSubtitleTrackId: String? = null\n''',
    'dual mono and frontend state')
replace_once(path,
    '''        tunerController.setOnTunerResourceLostCallback { tuneGeneration ->\n            enqueueSessionAction { handleTunerResourceLost(tuneGeneration) }\n        }\n''',
    '''        tunerController.setOnTunerResourceLostCallback { tuneGeneration ->\n            enqueueSessionAction { handleTunerResourceLost(tuneGeneration) }\n        }\n        tunerController.setOnTuneEventCallback { tuneGeneration, event ->\n            enqueueSessionAction { handleFrontendTuneEvent(tuneGeneration, event) }\n        }\n''',
    'live tune event callback')
replace_once(path,
    '''        preferredAudioTrackId = null\n        audioFallbackDisabled = false\n        selectedSubtitleTrackId = null\n''',
    '''        preferredAudioTrackId = null\n        audioFallbackDisabled = false\n        dualMonoPresentation = PlaybackPipeline.DualMonoPresentation.MAIN\n        frontendSignalUnavailable = false\n        selectedSubtitleTrackId = null\n''',
    'tune reset presentation')
# Both playback selections should carry the current presentation without changing ES identity.
p = ROOT / path
text = p.read_text(encoding='utf-8')
needle = '''                    subtitleExplicitlyDisabled = subtitleExplicitlyDisabled,\n                    defaultComponentGroupTags = defaultComponentGroupTags,\n                )'''
if text.count(needle) != 1:
    raise SystemExit(f'audio switch select args: expected 1, found {text.count(needle)}')
text = text.replace(needle, '''                    subtitleExplicitlyDisabled = subtitleExplicitlyDisabled,\n                    defaultComponentGroupTags = defaultComponentGroupTags,\n                    dualMonoPresentation = dualMonoPresentation,\n                )''', 1)
needle2 = '''            subtitleExplicitlyDisabled = subtitleExplicitlyDisabled,\n            defaultComponentGroupTags = currentDefaultComponentGroupTags(service.serviceKey),\n        )'''
if text.count(needle2) != 1:
    raise SystemExit(f'playback select args: expected 1, found {text.count(needle2)}')
text = text.replace(needle2, '''            subtitleExplicitlyDisabled = subtitleExplicitlyDisabled,\n            defaultComponentGroupTags = currentDefaultComponentGroupTags(service.serviceKey),\n            dualMonoPresentation = dualMonoPresentation,\n        )''', 1)
p.write_text(text, encoding='utf-8')
replace_once(path,
    '''                val previousAudioTrackId = preferredAudioTrackId\n                val previousAudioFallbackDisabled = audioFallbackDisabled\n                if (playbackState !is PlaybackStartState.Started) return false\n                preferredAudioTrackId = trackId\n                audioFallbackDisabled = false\n''',
    '''                val previousAudioTrackId = preferredAudioTrackId\n                val previousAudioFallbackDisabled = audioFallbackDisabled\n                val previousDualMonoPresentation = dualMonoPresentation\n                if (playbackState !is PlaybackStartState.Started) return false\n                preferredAudioTrackId = trackId\n                audioFallbackDisabled = false\n                if (trackId != previousAudioTrackId) dualMonoPresentation = PlaybackPipeline.DualMonoPresentation.MAIN\n''',
    'audio switch presentation reset')
# Restore presentation on either switch failure path.
p = ROOT / path
text = p.read_text(encoding='utf-8')
old = '''                    preferredAudioTrackId = previousAudioTrackId\n                    audioFallbackDisabled = previousAudioFallbackDisabled\n                    return false\n'''
if text.count(old) != 1:
    raise SystemExit(f'audio signature rollback: expected 1, found {text.count(old)}')
text = text.replace(old, '''                    preferredAudioTrackId = previousAudioTrackId\n                    audioFallbackDisabled = previousAudioFallbackDisabled\n                    dualMonoPresentation = previousDualMonoPresentation\n                    return false\n''', 1)
old2 = '''                    preferredAudioTrackId = previousAudioTrackId\n                    audioFallbackDisabled = previousAudioFallbackDisabled\n                    false\n'''
if text.count(old2) != 1:
    raise SystemExit(f'audio switch rollback: expected 1, found {text.count(old2)}')
text = text.replace(old2, '''                    preferredAudioTrackId = previousAudioTrackId\n                    audioFallbackDisabled = previousAudioFallbackDisabled\n                    dualMonoPresentation = previousDualMonoPresentation\n                    false\n''', 1)
p.write_text(text, encoding='utf-8')
replace_once(path,
    '''    override fun onSelectTrack(type: Int, trackId: String?): Boolean = runOnSessionExecutorBlocking {\n        onSelectTrackOnSessionExecutor(type, trackId)\n    }\n''',
    '''    override fun onAppPrivateCommand(action: String, data: Bundle?) {\n        enqueueSessionAction {\n            if (action != ACTION_SET_DUAL_MONO_PRESENTATION) return@enqueueSessionAction\n            val presentation = when (data?.getString(EXTRA_DUAL_MONO_PRESENTATION)) {\n                DUAL_MONO_MAIN -> PlaybackPipeline.DualMonoPresentation.MAIN\n                DUAL_MONO_SUB -> PlaybackPipeline.DualMonoPresentation.SUB\n                DUAL_MONO_MAIN_SUB -> PlaybackPipeline.DualMonoPresentation.MAIN_SUB\n                else -> return@enqueueSessionAction\n            }\n            if (tunerController.setDualMonoPresentation(presentation)) {\n                dualMonoPresentation = presentation\n            }\n        }\n    }\n\n    override fun onSelectTrack(type: Int, trackId: String?): Boolean = runOnSessionExecutorBlocking {\n        onSelectTrackOnSessionExecutor(type, trackId)\n    }\n''',
    'dual mono private command')
replace_once(path,
    '''    private fun handleTunerResourceLost(lostTuneGeneration: Long) {\n        if (lostTuneGeneration != currentGeneration) return\n        playbackState = PlaybackStartState.Stopped\n        beginCaptionPresentationGeneration(-1L, false)\n        notifyVideoUnavailable(TvInputManager.VIDEO_UNAVAILABLE_REASON_UNKNOWN)\n    }\n''',
    '''    private fun handleTunerResourceLost(lostTuneGeneration: Long) {\n        if (lostTuneGeneration != currentGeneration) return\n        frontendSignalUnavailable = false\n        playbackState = PlaybackStartState.Stopped\n        beginCaptionPresentationGeneration(-1L, false)\n        notifyVideoUnavailable(TvInputManager.VIDEO_UNAVAILABLE_REASON_UNKNOWN)\n    }\n\n    private fun handleFrontendTuneEvent(tuneGeneration: Long, event: Int) {\n        if (tuneGeneration != currentGeneration) return\n        when (event) {\n            OnTuneEventListener.SIGNAL_NO_SIGNAL, OnTuneEventListener.SIGNAL_LOST_LOCK -> {\n                frontendSignalUnavailable = true\n                playbackState = PlaybackStartState.Stopped\n                tunerController.stopPlayback()\n                beginCaptionPresentationGeneration(-1L, false)\n                notifyVideoUnavailable(TvInputManager.VIDEO_UNAVAILABLE_REASON_WEAK_SIGNAL)\n            }\n            OnTuneEventListener.SIGNAL_LOCKED -> {\n                if (!frontendSignalUnavailable) return\n                frontendSignalUnavailable = false\n                playbackState = PlaybackStartState.Idle\n                refreshDynamicSiAndCasFilters()\n            }\n        }\n    }\n''',
    'frontend tune event state')
replace_once(path,
    '''    companion object {\n        private const val ENABLE_CAS_ORCHESTRATION = true\n    }\n''',
    '''    companion object {\n        private const val ENABLE_CAS_ORCHESTRATION = true\n        const val ACTION_SET_DUAL_MONO_PRESENTATION = "com.maleicacid.tvinput.tis.action.SET_DUAL_MONO_PRESENTATION"\n        const val EXTRA_DUAL_MONO_PRESENTATION = "presentation"\n        const val DUAL_MONO_MAIN = "main"\n        const val DUAL_MONO_SUB = "sub"\n        const val DUAL_MONO_MAIN_SUB = "main_sub"\n    }\n''',
    'private command constants')

# #7 AudioTrack presentation and nullable non-fabricated session id.
path = 'tis/src/com/maleicacid/tvinput/tis/PlaybackPipeline.kt'
replace_once(path,
    '''    private val inputId: String,\n    private val sessionId: String,\n''',
    '''    private val inputId: String,\n    private val sessionId: String?,\n''',
    'nullable playback session id')
replace_once(path,
    '''    data class VideoFormatInfo(\n''',
    '''    enum class DualMonoPresentation { MAIN, SUB, MAIN_SUB }\n\n    data class VideoFormatInfo(\n''',
    'dual mono enum')
replace_once(path,
    '''    fun setVolume(volume: Float) {\n        enqueuePlaybackAction { setVolumeOnPlaybackExecutor(volume) }\n    }\n''',
    '''    fun setVolume(volume: Float) {\n        enqueuePlaybackAction { setVolumeOnPlaybackExecutor(volume) }\n    }\n\n    fun setDualMonoPresentation(presentation: DualMonoPresentation): Boolean = runOnPlaybackExecutorBlocking {\n        audioDecoder?.setDualMonoPresentation(presentation) ?: false\n    }\n''',
    'pipeline dual mono setter')
replace_once(path,
    '''            audioDecoder = AudioDecoderPipeline(audioKind!!, streamVolume, startGeneration) { reason, detail ->\n                if (startGeneration == playbackGeneration) handleAudioFailure(reason, detail, audioOnly)\n            }\n''',
    '''            audioDecoder = AudioDecoderPipeline(audioKind!!, requireNotNull(audio), selection.dualMonoPresentation, streamVolume, startGeneration) { reason, detail ->\n                if (startGeneration == playbackGeneration) handleAudioFailure(reason, detail, audioOnly)\n            }\n''',
    'audio decoder construction')
replace_once(path,
    '''    private inner class AudioDecoderPipeline(\n        private val kind: AudioCodecKind,\n        initialVolume: Float,\n        override val generation: Long,\n''',
    '''    private inner class AudioDecoderPipeline(\n        private val kind: AudioCodecKind,\n        private val stream: AribElementaryStream,\n        initialDualMonoPresentation: DualMonoPresentation,\n        initialVolume: Float,\n        override val generation: Long,\n''',
    'audio decoder signature')
replace_once(path,
    '''        private var volume: Float = initialVolume\n        private var outputSampleRate: Int = DEFAULT_AUDIO_SAMPLE_RATE\n''',
    '''        private var volume: Float = initialVolume\n        private var dualMonoPresentation: DualMonoPresentation = initialDualMonoPresentation\n        private val isDualMonoStream: Boolean = isAribDualMonoComponentType(stream.componentType)\n        private var outputSampleRate: Int = DEFAULT_AUDIO_SAMPLE_RATE\n''',
    'audio decoder state')
replace_once(path,
    '''        fun setVolume(value: Float) {\n            volume = value\n            audioTrack?.let { track -> runCatching { track.setVolume(value) } }\n        }\n''',
    '''        fun setVolume(value: Float) {\n            volume = value\n            audioTrack?.let { track -> runCatching { track.setVolume(value) } }\n        }\n\n        fun setDualMonoPresentation(presentation: DualMonoPresentation): Boolean {\n            if (!isDualMonoStream) return false\n            dualMonoPresentation = presentation\n            val track = audioTrack ?: return true\n            return track.setDualMonoMode(audioTrackDualMonoMode(presentation))\n        }\n''',
    'audio decoder dual mono setter')
replace_once(path,
    '''            val created = builder.build()\n            created.setVolume(volume)\n            requireNotNull(mediaSync).setAudioTrack(created)\n''',
    '''            val created = builder.build()\n            created.setVolume(volume)\n            if (isDualMonoStream && !created.setDualMonoMode(audioTrackDualMonoMode(dualMonoPresentation))) {\n                created.release()\n                throw IllegalStateException("ARIB dual-mono presentationをAudioTrackへ設定できません componentType=${stream.componentType}")\n            }\n            requireNotNull(mediaSync).setAudioTrack(created)\n''',
    'apply dual mono to AudioTrack')
replace_once(path,
    '''    companion object {\n        fun h264DimensionsForTest''',
    '''    companion object {\n        private fun isAribDualMonoComponentType(componentType: Int?): Boolean =\n            componentType != null && (componentType and 0x1f) == 0x02\n\n        private fun audioTrackDualMonoMode(presentation: DualMonoPresentation): Int = when (presentation) {\n            DualMonoPresentation.MAIN -> AudioTrack.DUAL_MONO_MODE_LL\n            DualMonoPresentation.SUB -> AudioTrack.DUAL_MONO_MODE_RR\n            DualMonoPresentation.MAIN_SUB -> AudioTrack.DUAL_MONO_MODE_LR\n        }\n\n        fun isAribDualMonoComponentTypeForTest(componentType: Int?): Boolean =\n            isAribDualMonoComponentType(componentType)\n\n        fun dualMonoModeForTest(presentation: DualMonoPresentation): Int =\n            audioTrackDualMonoMode(presentation)\n\n        fun h264DimensionsForTest''',
    'dual mono test helpers')

# #30 standard TvProvider video dimensions + #22 owned legacy/corrupt cleanup.
path = 'tis/src/com/maleicacid/tvinput/tis/TvProviderWriter.kt'
replace_once(path,
    '''        if (program.description.isBlank()) putNull(TvContract.Programs.COLUMN_LONG_DESCRIPTION) else put(TvContract.Programs.COLUMN_LONG_DESCRIPTION, program.description)\n        val audioLanguages = program.descriptors.components.audio\n''',
    '''        if (program.description.isBlank()) putNull(TvContract.Programs.COLUMN_LONG_DESCRIPTION) else put(TvContract.Programs.COLUMN_LONG_DESCRIPTION, program.description)\n        val videoWidth = program.videoWidth?.takeIf { it > 0 }\n        val videoHeight = program.videoHeight?.takeIf { it > 0 }\n        if (videoWidth != null && videoHeight != null) {\n            put(TvContract.Programs.COLUMN_VIDEO_WIDTH, videoWidth)\n            put(TvContract.Programs.COLUMN_VIDEO_HEIGHT, videoHeight)\n        } else {\n            putNull(TvContract.Programs.COLUMN_VIDEO_WIDTH)\n            putNull(TvContract.Programs.COLUMN_VIDEO_HEIGHT)\n        }\n        val audioLanguages = program.descriptors.components.audio\n''',
    'video dimensions projection')
replace_once(path,
    '''            TvContract.Programs.COLUMN_AUDIO_LANGUAGE,\n            TvContract.Programs.COLUMN_BROADCAST_GENRE,\n''',
    '''            TvContract.Programs.COLUMN_AUDIO_LANGUAGE,\n            TvContract.Programs.COLUMN_VIDEO_WIDTH,\n            TvContract.Programs.COLUMN_VIDEO_HEIGHT,\n            TvContract.Programs.COLUMN_BROADCAST_GENRE,\n''',
    'video dimensions signature')
replace_once(path,
    '''        fun providerDataMatchesService(providerData: ByteArray?, serviceKey: ServiceKey?): Boolean {\n            if (serviceKey == null) return true\n            val key = ProviderDataBridge.extractProgramKeyResult(providerData) ?: return false\n            return key.serviceKey == serviceKey\n        }\n''',
    '''        fun providerDataMatchesService(providerData: ByteArray?, serviceKey: ServiceKey?): Boolean {\n            if (serviceKey == null) return true\n            val key = ProviderDataBridge.extractProgramKeyResult(providerData) ?: return false\n            return key.serviceKey == serviceKey\n        }\n\n        internal fun shouldDeleteOwnedObsoleteProgramRow(\n            ownerPackage: String?,\n            ownPackage: String,\n            programKey: String?,\n            validProgramKeys: Set<String>,\n        ): Boolean = ownerPackage == ownPackage && (programKey == null || programKey !in validProgramKeys)\n''',
    'owned obsolete helper')
replace_once(path,
    '''            val projection = arrayOf(TvContract.Programs._ID, TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA)\n            val selection = "${TvContract.Programs.COLUMN_CHANNEL_ID}=? AND ${TvContract.Programs.COLUMN_END_TIME_UTC_MILLIS}>? AND ${TvContract.Programs.COLUMN_START_TIME_UTC_MILLIS}<?"\n''',
    '''            val projection = arrayOf(TvContract.Programs._ID, TvContract.Programs.COLUMN_PACKAGE_NAME, TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA)\n            val selection = "${TvContract.Programs.COLUMN_CHANNEL_ID}=? AND ${TvContract.Programs.COLUMN_END_TIME_UTC_MILLIS}>? AND ${TvContract.Programs.COLUMN_START_TIME_UTC_MILLIS}<?"\n''',
    'obsolete projection ownership')
replace_once(path,
    '''                    val id = cursor.getLong(0)\n                    val key = TvProviderWriter.parseProgramKey(providerDataBytes(cursor, 1))\n                    if (key == null) {\n                        Log.w(LogTags.TIS, "Program provider-data から安定キーを抽出できないため削除を保留します id=$id")\n                    } else if (key !in validProgramKeys) {\n                        deleted += context.contentResolver.delete(ContentUris.withAppendedId(TvContract.Programs.CONTENT_URI, id), null, null)\n                    }\n''',
    '''                    val id = cursor.getLong(0)\n                    val ownerPackage = cursor.getString(1)\n                    val key = TvProviderWriter.parseProgramKey(providerDataBytes(cursor, 2))\n                    if (TvProviderWriter.shouldDeleteOwnedObsoleteProgramRow(ownerPackage, context.packageName, key, validProgramKeys)) {\n                        deleted += context.contentResolver.delete(ContentUris.withAppendedId(TvContract.Programs.CONTENT_URI, id), null, null)\n                    } else if (key == null) {\n                        Log.w(LogTags.TIS, "所有元を確認できないProgram provider-data破損行は保持します id=$id owner=$ownerPackage")\n                    }\n''',
    'owned corrupt deletion')

# Keep host test count unchanged: extend existing tests instead of adding methods.
path = 'tis/tests/src/com/maleicacid/tvinput/tis/TisR51FixedPlanAcceptanceTest.kt'
replace_once(path,
    'import android.media.tv.TvTrackInfo\n',
    'import android.media.tv.TvTrackInfo\nimport android.media.tv.tuner.frontend.OnTuneEventListener\n',
    'test tune event import')
replace_once(path,
    '''    @Test fun api30SessionIdIsPropagatedWithoutFallback() {\n        val sessionId = "framework-session-123"\n        check(MaleicacidTvInputService.api30SessionIdForTest("input.test", sessionId) == sessionId)\n        check(!MaleicacidTvInputService.legacyFallbackSessionIdForTest("input.test").contains(sessionId))\n    }\n''',
    '''    @Test fun api30SessionIdIsPropagatedWithoutFallback() {\n        val sessionId = "framework-session-123"\n        check(MaleicacidTvInputService.api30SessionIdForTest("input.test", sessionId) == sessionId)\n        check(TunerController.normalizedTvInputSessionId(sessionId) == sessionId)\n        check(TunerController.normalizedTvInputSessionId(null) == null)\n        check(TunerController.normalizedTvInputSessionId("") == null)\n        check(TunerController.isSignalUnavailableTuneEventForTest(OnTuneEventListener.SIGNAL_NO_SIGNAL))\n        check(TunerController.isSignalUnavailableTuneEventForTest(OnTuneEventListener.SIGNAL_LOST_LOCK))\n        check(!TunerController.isSignalUnavailableTuneEventForTest(OnTuneEventListener.SIGNAL_LOCKED))\n    }\n''',
    'session and tune event test')

path = 'tis/tests/src/com/maleicacid/tvinput/tis/PlaybackAudioSinkR51FixTest.kt'
replace_once(path,
    'import android.media.tv.tuner.filter.AvSettings\n',
    'import android.media.AudioTrack\nimport android.media.tv.tuner.filter.AvSettings\n',
    'audio test import')
replace_once(path,
    '''        check(PlaybackPipeline.normalizedAudioStreamTypeForTest(0x11) == AvSettings.AUDIO_STREAM_TYPE_UNDEFINED)\n''',
    '''        check(PlaybackPipeline.normalizedAudioStreamTypeForTest(0x11) == AvSettings.AUDIO_STREAM_TYPE_UNDEFINED)\n        check(PlaybackPipeline.isAribDualMonoComponentTypeForTest(0x02))\n        check(PlaybackPipeline.isAribDualMonoComponentTypeForTest(0x22))\n        check(!PlaybackPipeline.isAribDualMonoComponentTypeForTest(0x03))\n        check(PlaybackPipeline.dualMonoModeForTest(PlaybackPipeline.DualMonoPresentation.MAIN) == AudioTrack.DUAL_MONO_MODE_LL)\n        check(PlaybackPipeline.dualMonoModeForTest(PlaybackPipeline.DualMonoPresentation.SUB) == AudioTrack.DUAL_MONO_MODE_RR)\n        check(PlaybackPipeline.dualMonoModeForTest(PlaybackPipeline.DualMonoPresentation.MAIN_SUB) == AudioTrack.DUAL_MONO_MODE_LR)\n''',
    'dual mono assertions')

path = 'tis/tests/src/com/maleicacid/tvinput/tis/TvProviderWriterProgramsTest.kt'
replace_once(path,
    '''        check(store.programs.values.single().getAsString(TvContract.Programs.COLUMN_CONTENT_RATING) == rating18)\n''',
    '''        check(store.programs.values.single().getAsString(TvContract.Programs.COLUMN_CONTENT_RATING) == rating18)\n        check(store.programs.values.single().getAsInteger(TvContract.Programs.COLUMN_VIDEO_WIDTH) == 1920)\n        check(store.programs.values.single().getAsInteger(TvContract.Programs.COLUMN_VIDEO_HEIGHT) == 1080)\n''',
    'video dimension assertions')
replace_once(path,
    '''        check(authoritative.deleted == 1)\n        check(store.programs.size == 2)\n''',
    '''        check(authoritative.deleted == 1)\n        check(store.programs.size == 2)\n        val validKey = TvProviderWriter.programKeyForTest(p1)\n        check(TvProviderWriter.shouldDeleteOwnedObsoleteProgramRow("com.maleicacid.tv", "com.maleicacid.tv", null, setOf(validKey)))\n        check(TvProviderWriter.shouldDeleteOwnedObsoleteProgramRow("com.maleicacid.tv", "com.maleicacid.tv", "obsolete", setOf(validKey)))\n        check(!TvProviderWriter.shouldDeleteOwnedObsoleteProgramRow("other.package", "com.maleicacid.tv", null, setOf(validKey)))\n        check(!TvProviderWriter.shouldDeleteOwnedObsoleteProgramRow(null, "com.maleicacid.tv", null, setOf(validKey)))\n        check(!TvProviderWriter.shouldDeleteOwnedObsoleteProgramRow("com.maleicacid.tv", "com.maleicacid.tv", validKey, setOf(validKey)))\n''',
    'owned corrupt cleanup assertions')

# Postconditions: #21/#31 remain untouched by this delta, and no fabricated session ID remains in production TIS.
for forbidden in [
    'legacyFallbackSessionIdForTest',
    '"maleicacid-$inputId-${System.nanoTime()}"',
]:
    for file in [
        ROOT / 'tis/src/com/maleicacid/tvinput/tis/MaleicacidTvInputService.kt',
        ROOT / 'tis/src/com/maleicacid/tvinput/tis/TunerController.kt',
    ]:
        if forbidden in file.read_text(encoding='utf-8'):
            raise SystemExit(f'forbidden fallback remains: {forbidden} in {file}')

print('applied #7 #10 #22 #24 #30')

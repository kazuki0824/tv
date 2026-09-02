from pathlib import Path

ROOT = Path('.')

def replace_once(path, old, new, label):
    p = ROOT / path
    text = p.read_text(encoding='utf-8')
    count = text.count(old)
    if count != 1:
        raise SystemExit(f'{label}: expected 1 occurrence, found {count}')
    p.write_text(text.replace(old, new, 1), encoding='utf-8')

# Carry the current EIT Audio Component Descriptor component_type separately from PMT ES identity.
replace_once(
    'tis/src/com/maleicacid/tvinput/tis/TunerController.kt',
    '''        val superimpose: AribElementaryStream? = null,\n        val dualMonoPresentation: PlaybackPipeline.DualMonoPresentation = PlaybackPipeline.DualMonoPresentation.MAIN,\n''',
    '''        val superimpose: AribElementaryStream? = null,\n        val audioComponentType: Int? = null,\n        val dualMonoPresentation: PlaybackPipeline.DualMonoPresentation = PlaybackPipeline.DualMonoPresentation.MAIN,\n''',
    'selection audio component type',
)
replace_once(
    'tis/src/com/maleicacid/tvinput/tis/TunerController.kt',
    '''        return AvStreamSelection(serviceKey, pcrPid, video, audio, subtitle, selectedCaptionTrack?.captionLanguageId, superimpose, dualMonoPresentation)\n''',
    '''        return AvStreamSelection(serviceKey, pcrPid, video, audio, subtitle, selectedCaptionTrack?.captionLanguageId, superimpose, audio?.componentType, dualMonoPresentation)\n''',
    'selection default component type',
)

# Resolve ARIB dual-mono semantics from the current EIT audio_component_descriptor by component_tag.
path = 'tis/src/com/maleicacid/tvinput/tis/MaleicacidLiveSession.kt'
replace_once(path,
    '''        val selection = tunerController.selectAvStreams(\n            service.serviceKey,\n            service.pcrPid,\n            service.streams,\n            preferredAudioTrackId,\n            selectedSubtitleTrackId,\n            audioExplicitlyDisabled = audioFallbackDisabled,\n            subtitleExplicitlyDisabled = subtitleExplicitlyDisabled,\n            defaultComponentGroupTags = currentDefaultComponentGroupTags(service.serviceKey),\n            dualMonoPresentation = dualMonoPresentation,\n        )\n''',
    '''        val initialSelection = tunerController.selectAvStreams(\n            service.serviceKey,\n            service.pcrPid,\n            service.streams,\n            preferredAudioTrackId,\n            selectedSubtitleTrackId,\n            audioExplicitlyDisabled = audioFallbackDisabled,\n            subtitleExplicitlyDisabled = subtitleExplicitlyDisabled,\n            defaultComponentGroupTags = currentDefaultComponentGroupTags(service.serviceKey),\n            dualMonoPresentation = dualMonoPresentation,\n        )\n        val selection = initialSelection.copy(\n            audioComponentType = currentAudioComponentType(service.serviceKey, initialSelection.audio),\n        )\n''',
    'initial playback EIT audio component type')
replace_once(path,
    '''                val selection = tunerController.selectAvStreams(\n                    service.serviceKey,\n                    service.pcrPid,\n                    service.streams,\n                    preferredAudioTrackId,\n                    selectedSubtitleTrackId,\n                    audioExplicitlyDisabled = audioFallbackDisabled,\n                    subtitleExplicitlyDisabled = subtitleExplicitlyDisabled,\n                    defaultComponentGroupTags = defaultComponentGroupTags,\n                    dualMonoPresentation = dualMonoPresentation,\n                )\n''',
    '''                val initialSelection = tunerController.selectAvStreams(\n                    service.serviceKey,\n                    service.pcrPid,\n                    service.streams,\n                    preferredAudioTrackId,\n                    selectedSubtitleTrackId,\n                    audioExplicitlyDisabled = audioFallbackDisabled,\n                    subtitleExplicitlyDisabled = subtitleExplicitlyDisabled,\n                    defaultComponentGroupTags = defaultComponentGroupTags,\n                    dualMonoPresentation = dualMonoPresentation,\n                )\n                val selection = initialSelection.copy(\n                    audioComponentType = currentAudioComponentType(service.serviceKey, initialSelection.audio),\n                )\n''',
    'audio switch EIT audio component type')
replace_once(path,
    '''    private fun currentDefaultComponentGroupTags(serviceKey: ServiceKey, nowMillis: Long = System.currentTimeMillis()): Set<Int>? {\n''',
    '''    private fun currentAudioComponentType(\n        serviceKey: ServiceKey,\n        audio: com.maleicacid.tvinput.aribsi.AribElementaryStream?,\n        nowMillis: Long = System.currentTimeMillis(),\n    ): Int? {\n        audio ?: return null\n        val componentTag = audio.componentTag ?: return audio.componentType\n        val currentEvent = aribSiEngine.programStateSnapshot().events\n            .asSequence()\n            .filter { event -> event.serviceKey == serviceKey && event.durationMillis > 0L }\n            .filter { event -> nowMillis >= event.startTimeMillis && nowMillis < event.startTimeMillis + event.durationMillis }\n            .minByOrNull { it.startTimeMillis }\n            ?: return audio.componentType\n        return currentEvent.descriptors.components.audio\n            .firstOrNull { component -> component.parseStatus == "OK" && component.componentTag == componentTag }\n            ?.componentType\n            ?: audio.componentType\n    }\n\n    private fun currentDefaultComponentGroupTags(serviceKey: ServiceKey, nowMillis: Long = System.currentTimeMillis()): Set<Int>? {\n''',
    'current EIT audio helper')

# Playback consumes the canonical current-event component_type, not only PMT-side optional metadata.
replace_once(
    'tis/src/com/maleicacid/tvinput/tis/PlaybackPipeline.kt',
    '''            audioDecoder = AudioDecoderPipeline(audioKind!!, requireNotNull(audio), selection.dualMonoPresentation, streamVolume, startGeneration) { reason, detail ->\n''',
    '''            audioDecoder = AudioDecoderPipeline(audioKind!!, selection.audioComponentType ?: requireNotNull(audio).componentType, selection.dualMonoPresentation, streamVolume, startGeneration) { reason, detail ->\n''',
    'audio decoder canonical component type')
replace_once(
    'tis/src/com/maleicacid/tvinput/tis/PlaybackPipeline.kt',
    '''        private val kind: AudioCodecKind,\n        private val stream: AribElementaryStream,\n        initialDualMonoPresentation: DualMonoPresentation,\n''',
    '''        private val kind: AudioCodecKind,\n        private val componentType: Int?,\n        initialDualMonoPresentation: DualMonoPresentation,\n''',
    'audio decoder component type parameter')
replace_once(
    'tis/src/com/maleicacid/tvinput/tis/PlaybackPipeline.kt',
    '''        private var dualMonoPresentation: DualMonoPresentation = initialDualMonoPresentation\n        private val isDualMonoStream: Boolean = isAribDualMonoComponentType(stream.componentType)\n''',
    '''        private var dualMonoPresentation: DualMonoPresentation = initialDualMonoPresentation\n        private val isDualMonoStream: Boolean = isAribDualMonoComponentType(componentType)\n''',
    'audio decoder canonical dual mono detection')
replace_once(
    'tis/src/com/maleicacid/tvinput/tis/PlaybackPipeline.kt',
    '''                throw IllegalStateException("ARIB dual-mono presentationをAudioTrackへ設定できません componentType=${stream.componentType}")\n''',
    '''                throw IllegalStateException("ARIB dual-mono presentationをAudioTrackへ設定できません componentType=$componentType")\n''',
    'audio decoder diagnostic')

print('applied EIT-aware dual-mono component_type correlation')

from pathlib import Path
ROOT=Path('.')
def read(p): return (ROOT/p).read_text(encoding='utf-8')
def write(p,s): (ROOT/p).write_text(s,encoding='utf-8')
def rep(p,old,new,label,count=1):
 s=read(p); n=s.count(old)
 if n!=count: raise SystemExit(f'{label}: expected {count}, found {n}')
 write(p,s.replace(old,new,count))

# Direct Boot service-key ledger instead of candidate count.
p='tis/src/com/maleicacid/tvinput/tis/ProgramPublishCoordinator.kt'
rep(p,
'''        val eligibleTargetCount: Int = 0,\n        val committedServiceCount: Int = 0,\n    ) {\n        val changed: Int get() = inserted + updated + deleted\n        val hasCommittedTarget: Boolean\n            get() = eligibleTargetCount > 0 && committedServiceCount > 0 && failures.isEmpty()\n''',
'''        val eligibleTargetCount: Int = 0,\n        val committedServiceCount: Int = 0,\n        val committedServiceKeys: Set<ServiceKey> = emptySet(),\n    ) {\n        val changed: Int get() = inserted + updated + deleted\n        val hasCommittedTarget: Boolean\n            get() = eligibleTargetCount > 0 && committedServiceKeys.isNotEmpty() && failures.isEmpty()\n''','publish service ledger')
rep(p,
'''                committedServiceCount = eligibleTargetServiceKeys.size,\n            )\n''',
'''                committedServiceCount = eligibleTargetServiceKeys.size,\n                committedServiceKeys = eligibleTargetServiceKeys,\n            )\n''','unchanged ledger')
rep(p,
'''            committedServiceCount = result.succeededServiceKeys.count { it in eligibleTargetServiceKeys },\n        )\n''',
'''            committedServiceCount = result.succeededServiceKeys.count { it in eligibleTargetServiceKeys },\n            committedServiceKeys = result.succeededServiceKeys.filterTo(linkedSetOf()) { it in eligibleTargetServiceKeys && it !in failedServiceKeys },\n        )\n''','result ledger')

p='tis/src/com/maleicacid/tvinput/tis/ChannelScanController.kt'
rep(p,
'''    data class ScanResult(val scanned: Int, val published: Int, val diagnostics: List<ScanDiagnostic>, val successfulCandidates: Int = 0, val terminalCancelObserved: Boolean = false)\n''',
'''    data class ScanResult(\n        val scanned: Int,\n        val published: Int,\n        val diagnostics: List<ScanDiagnostic>,\n        val successfulCandidates: Int = 0,\n        val terminalCancelObserved: Boolean = false,\n        val committedServiceKeys: Set<ServiceKey> = emptySet(),\n    )\n''','scan result ledger')
rep(p,
'''        val hasCommittedProgramTarget: Boolean = false,\n    ) {\n''',
'''        val hasCommittedProgramTarget: Boolean = false,\n        val committedServiceKeys: Set<ServiceKey> = emptySet(),\n    ) {\n''','snapshot ledger')
rep(p,
'''        var updated = 0\n        var successfulCandidates = 0\n        candidates.forEach { candidate ->\n''',
'''        var updated = 0\n        var successfulCandidates = 0\n        val committedServiceKeys = linkedSetOf<ServiceKey>()\n        candidates.forEach { candidate ->\n''','maintenance ledger init')
rep(p,
'''            if (collection.outcome == SiCollectionOutcome.COMPLETE && collection.registrationReadyServices > 0 && publishResult.success && publishResult.hasCommittedProgramTarget) successfulCandidates++\n            updated += publishResult.changed\n''',
'''            if (collection.outcome == SiCollectionOutcome.COMPLETE && collection.registrationReadyServices > 0 && publishResult.success && publishResult.hasCommittedProgramTarget) successfulCandidates++\n            committedServiceKeys += publishResult.committedServiceKeys\n            updated += publishResult.changed\n''','maintenance ledger union')
rep(p,
'''        return ScanResult(candidates.size, updated, diagnostics, successfulCandidates = successfulCandidates, terminalCancelObserved = terminalCancelObserved)\n''',
'''        return ScanResult(\n            candidates.size,\n            updated,\n            diagnostics,\n            successfulCandidates = successfulCandidates,\n            terminalCancelObserved = terminalCancelObserved,\n            committedServiceKeys = committedServiceKeys,\n        )\n''','maintenance ledger result')
rep(p,
'''                hasCommittedProgramTarget = result.hasCommittedTarget,\n            )\n''',
'''                hasCommittedProgramTarget = result.hasCommittedTarget,\n                committedServiceKeys = result.committedServiceKeys,\n            )\n''','snapshot ledger propagation')

p='tis/src/com/maleicacid/tvinput/tis/ChannelScanManager.kt'
rep(p,
'''            val scanTargets = targets.getOrElse { emptyList() }\n            val candidates = controller.maintenanceCandidates(scanTargets)\n            val scanResult = controller.startBootEpgSync(scanTargets)\n            val terminalCancel = token.get() || scanResult.terminalCancelObserved\n            val allRequiredTargetsCommitted = scanResult.scanned > 0 &&\n                scanResult.successfulCandidates == scanResult.scanned &&\n                !terminalCancel\n''',
'''            val scanTargets = targets.getOrElse { emptyList() }\n            val requiredTargetKeys = scanTargets.mapTo(linkedSetOf()) { it.serviceKey }\n            val scanResult = controller.startBootEpgSync(scanTargets)\n            val terminalCancel = token.get() || scanResult.terminalCancelObserved\n            val allRequiredTargetsCommitted = requiredTargetKeys.isNotEmpty() &&\n                scanResult.committedServiceKeys.containsAll(requiredTargetKeys) &&\n                !terminalCancel\n''','boot ledger completion')

# BS23 fallback: add current TS3 18803 without deleting 18802 absent authoritative proof.
p='tis/src/com/maleicacid/tvinput/tis/ScanPlan.kt'
rep(p,
'''BsTsidEntry(FrequencyHz(1_471_440_000L), TransportStreamId16(18288), "BS23-18288", 23), BsTsidEntry(FrequencyHz(1_471_440_000L), TransportStreamId16(18801), "BS23-18801", 23), BsTsidEntry(FrequencyHz(1_471_440_000L), TransportStreamId16(18802), "BS23-18802", 23),''',
'''BsTsidEntry(FrequencyHz(1_471_440_000L), TransportStreamId16(18288), "BS23-18288", 23), BsTsidEntry(FrequencyHz(1_471_440_000L), TransportStreamId16(18801), "BS23-18801", 23), BsTsidEntry(FrequencyHz(1_471_440_000L), TransportStreamId16(18802), "BS23-18802", 23), BsTsidEntry(FrequencyHz(1_471_440_000L), TransportStreamId16(18803), "BS23-18803", 23),''','BS23 TS3 fallback')

# Caption language tracks: use EIT data_contents selector keyed by component_tag when available.
p='tis/src/com/maleicacid/tvinput/tis/TunerController.kt'
rep(p,
'''        defaultComponentGroupTags: Set<Int>? = null,\n        dualMonoPresentation: PlaybackPipeline.DualMonoPresentation = PlaybackPipeline.DualMonoPresentation.MAIN,\n    ): AvStreamSelection {\n''',
'''        defaultComponentGroupTags: Set<Int>? = null,\n        captionLanguagesByComponentTag: Map<Int, List<Pair<Int, String>>> = emptyMap(),\n        dualMonoPresentation: PlaybackPipeline.DualMonoPresentation = PlaybackPipeline.DualMonoPresentation.MAIN,\n    ): AvStreamSelection {\n''','selection caption language map')
rep(p,
'''        val captionTracks = captionTracksFor(streams, defaultComponentGroupTags)\n''',
'''        val captionTracks = captionTracksFor(streams, defaultComponentGroupTags, captionLanguagesByComponentTag)\n''','selection caption tracks map')
rep(p,
'''    fun tracksFor(streams: List<AribElementaryStream>, defaultComponentGroupTags: Set<Int>? = null): List<TisTrack> = buildList {\n''',
'''    fun tracksFor(\n        streams: List<AribElementaryStream>,\n        defaultComponentGroupTags: Set<Int>? = null,\n        captionLanguagesByComponentTag: Map<Int, List<Pair<Int, String>>> = emptyMap(),\n    ): List<TisTrack> = buildList {\n''','tracksFor signature')
rep(p,
'''        addAll(captionTracksFor(streams, defaultComponentGroupTags))\n''',
'''        addAll(captionTracksFor(streams, defaultComponentGroupTags, captionLanguagesByComponentTag))\n''','tracksFor caption map')
rep(p,
'''    private fun captionTracksFor(streams: List<AribElementaryStream>, defaultComponentGroupTags: Set<Int>? = null): List<TisTrack> = buildList {\n        TunerSelectionPolicy.orderedCaptionStreams(streams, defaultComponentGroupTags).forEach { stream ->\n            val languages = stream.languageCodes.take(2)\n            if (languages.isEmpty()) {\n                add(TisTrack(TunerSelectionPolicy.trackIdForSubtitle(stream, 1), android.media.tv.TvTrackInfo.TYPE_SUBTITLE, stream.elementaryPid, stream.streamType, stream.componentTag, stream.componentType, null, stream.dataComponentId, TunerSelectionPolicy.captionKind(stream), 1))\n            } else {\n                languages.forEachIndexed { index, language ->\n                    val languageId = index + 1\n                    add(TisTrack(TunerSelectionPolicy.trackIdForSubtitle(stream, languageId), android.media.tv.TvTrackInfo.TYPE_SUBTITLE, stream.elementaryPid, stream.streamType, stream.componentTag, stream.componentType, language, stream.dataComponentId, TunerSelectionPolicy.captionKind(stream), languageId))\n                }\n            }\n        }\n    }\n''',
'''    private fun captionTracksFor(\n        streams: List<AribElementaryStream>,\n        defaultComponentGroupTags: Set<Int>? = null,\n        captionLanguagesByComponentTag: Map<Int, List<Pair<Int, String>>> = emptyMap(),\n    ): List<TisTrack> = buildList {\n        TunerSelectionPolicy.orderedCaptionStreams(streams, defaultComponentGroupTags).forEach { stream ->\n            val selectorLanguages = stream.componentTag?.let(captionLanguagesByComponentTag::get).orEmpty().take(2)\n            if (selectorLanguages.isNotEmpty()) {\n                selectorLanguages.forEach { (languageId, language) ->\n                    add(TisTrack(TunerSelectionPolicy.trackIdForSubtitle(stream, languageId), android.media.tv.TvTrackInfo.TYPE_SUBTITLE, stream.elementaryPid, stream.streamType, stream.componentTag, stream.componentType, language, stream.dataComponentId, TunerSelectionPolicy.captionKind(stream), languageId))\n                }\n            } else {\n                // PMT generic ISO639 is not a caption-language SSOT. Until management/EIT selector\n                // facts arrive, expose only the default language id without inventing a language code.\n                add(TisTrack(TunerSelectionPolicy.trackIdForSubtitle(stream, 1), android.media.tv.TvTrackInfo.TYPE_SUBTITLE, stream.elementaryPid, stream.streamType, stream.componentTag, stream.componentType, null, stream.dataComponentId, TunerSelectionPolicy.captionKind(stream), 1))\n            }\n        }\n    }\n''','caption tracks selector source')

# Session helpers for caption selector and video component projection.
p='tis/src/com/maleicacid/tvinput/tis/MaleicacidLiveSession.kt'
rep(p,
'''    private fun currentAudioComponent(\n''',
'''    private fun currentEventForService(\n        serviceKey: ServiceKey,\n        nowMillis: Long = System.currentTimeMillis(),\n    ) = aribSiEngine.programStateSnapshot().events\n        .asSequence()\n        .filter { event -> event.serviceKey == serviceKey && event.durationMillis > 0L }\n        .filter { event -> nowMillis >= event.startTimeMillis && nowMillis < event.startTimeMillis + event.durationMillis }\n        .minByOrNull { it.startTimeMillis }\n\n    private fun currentCaptionLanguagesByComponentTag(serviceKey: ServiceKey): Map<Int, List<Pair<Int, String>>> =\n        currentEventForService(serviceKey)?.descriptors?.captionSelectors.orEmpty()\n            .filter { it.dataComponentId == 0x0008 && it.parseStatus.equals("OK", ignoreCase = true) }\n            .associate { selector ->\n                selector.componentTag to selector.languages\n                    .filter { it.parseStatus.equals("OK", ignoreCase = true) }\n                    .take(2)\n                    .map { language -> (language.languageTag + 1) to language.languageCode }\n            }\n\n    private fun currentVideoComponent(\n        serviceKey: ServiceKey,\n        componentTag: Int?,\n    ): com.maleicacid.tvinput.aribsi.AribComponentEntry? {\n        componentTag ?: return null\n        return currentEventForService(serviceKey)?.descriptors?.components?.video\n            ?.firstOrNull { component -> component.parseStatus.equals("OK", ignoreCase = true) && component.componentTag == componentTag }\n    }\n\n    private fun currentAudioComponent(\n''','session current event helpers')
# simplify currentAudio/currentDefault to reuse helper
rep(p,
'''        val currentEvent = aribSiEngine.programStateSnapshot().events\n            .asSequence()\n            .filter { event -> event.serviceKey == serviceKey && event.durationMillis > 0L }\n            .filter { event -> nowMillis >= event.startTimeMillis && nowMillis < event.startTimeMillis + event.durationMillis }\n            .minByOrNull { it.startTimeMillis }\n            ?: return null\n        return currentEvent.descriptors.components.audio\n''',
'''        val currentEvent = currentEventForService(serviceKey, nowMillis) ?: return null\n        return currentEvent.descriptors.components.audio\n''','reuse current event audio')
rep(p,
'''        val currentEvent = aribSiEngine.programStateSnapshot().events\n            .asSequence()\n            .filter { event -> event.serviceKey == serviceKey && event.durationMillis > 0L }\n            .filter { event -> nowMillis >= event.startTimeMillis && nowMillis < event.startTimeMillis + event.durationMillis }\n            .minByOrNull { it.startTimeMillis }\n            ?: return null\n''',
'''        val currentEvent = currentEventForService(serviceKey, nowMillis) ?: return null\n''','reuse current event group')
rep(p,
'''        val defaultComponentGroupTags = currentDefaultComponentGroupTags(service.serviceKey)\n        val tracks = tunerController.tracksFor(service.streams, defaultComponentGroupTags).filterNot { track ->\n''',
'''        val defaultComponentGroupTags = currentDefaultComponentGroupTags(service.serviceKey)\n        val captionLanguages = currentCaptionLanguagesByComponentTag(service.serviceKey)\n        val tracks = tunerController.tracksFor(service.streams, defaultComponentGroupTags, captionLanguages).filterNot { track ->\n''','tracks caption selectors')
rep(p,
'''        val audioMetadataByTrackId = tracks\n''',
'''        val videoComponentByTrackId = tracks\n            .filter { it.type == TvTrackInfo.TYPE_VIDEO }\n            .associate { track -> track.id to currentVideoComponent(service.serviceKey, track.componentTag) }\n        val audioMetadataByTrackId = tracks\n''','video metadata map')
rep(p,
'''            val videoComponentType = if (track.type == TvTrackInfo.TYPE_VIDEO) track.componentType ?: -1 else -1\n''',
'''            val videoComponent = videoComponentByTrackId[track.id]\n            val videoComponentType = if (track.type == TvTrackInfo.TYPE_VIDEO) track.componentType ?: -1 else -1\n''','video component signature local')
rep(p,
'''                videoComponentType.toString(),\n                subtitleDataComponentId.toString(),\n''',
'''                videoComponentType.toString(),\n                videoComponent?.text.orEmpty(),\n                videoEncodingForStreamType(track.streamType).orEmpty(),\n                subtitleDataComponentId.toString(),\n''','video signature facts')
rep(p,
'''                if (track.type == TvTrackInfo.TYPE_AUDIO && audioMetadata != null) {\n''',
'''                if (track.type == TvTrackInfo.TYPE_VIDEO) {\n                    videoEncodingForStreamType(track.streamType)?.let(builder::setEncoding)\n                    videoComponentByTrackId[track.id]?.text?.takeIf { it.isNotBlank() }?.let(builder::setDescription)\n                }\n                if (track.type == TvTrackInfo.TYPE_AUDIO && audioMetadata != null) {\n''','video track projection')
# Add encoding helper before currentAudio component helper.
rep(p,
'''    private fun currentEventForService(\n''',
'''    private fun videoEncodingForStreamType(streamType: Int): String? = when (streamType) {\n        0x02 -> android.media.MediaFormat.MIMETYPE_VIDEO_MPEG2\n        0x1b -> android.media.MediaFormat.MIMETYPE_VIDEO_AVC\n        0x24 -> android.media.MediaFormat.MIMETYPE_VIDEO_HEVC\n        else -> null\n    }\n\n    private fun currentEventForService(\n''','video encoding helper')
# Pass caption map into selections. Replace all 2 known call forms.
s=read(p)
s=s.replace('''tunerController.selectAvStreams(service.serviceKey, service.pcrPid, service.streams, preferredAudioTrackId, selectedSubtitleTrackId)''', '''tunerController.selectAvStreams(\n                    service.serviceKey, service.pcrPid, service.streams, preferredAudioTrackId, selectedSubtitleTrackId,\n                    defaultComponentGroupTags = currentDefaultComponentGroupTags(service.serviceKey),\n                    captionLanguagesByComponentTag = currentCaptionLanguagesByComponentTag(service.serviceKey),\n                )''')
s=s.replace('''tunerController.tracksFor(it.streams, currentDefaultComponentGroupTags(it.serviceKey))''', '''tunerController.tracksFor(it.streams, currentDefaultComponentGroupTags(it.serviceKey), currentCaptionLanguagesByComponentTag(it.serviceKey))''')
write(p,s)

# 3/4/5ch + decoder provided channel mask preference.
p='tis/src/com/maleicacid/tvinput/tis/PlaybackPipeline.kt'
rep(p,
'''        private var outputSampleRate: Int = DEFAULT_AUDIO_SAMPLE_RATE\n        private var outputChannels: Int = DEFAULT_AUDIO_CHANNEL_COUNT\n''',
'''        private var outputSampleRate: Int = DEFAULT_AUDIO_SAMPLE_RATE\n        private var outputChannels: Int = DEFAULT_AUDIO_CHANNEL_COUNT\n        private var outputChannelMask: Int? = null\n''','audio mask field')
rep(p,
'''            outputChannels = getIntegerOrDefault(format, MediaFormat.KEY_CHANNEL_COUNT, DEFAULT_AUDIO_CHANNEL_COUNT)\n            ensureAudioTrack()\n''',
'''            outputChannels = getIntegerOrDefault(format, MediaFormat.KEY_CHANNEL_COUNT, DEFAULT_AUDIO_CHANNEL_COUNT)\n            outputChannelMask = validDecoderChannelMask(format, outputChannels)\n            ensureAudioTrack()\n''','configured mask')
rep(p,
'''            outputChannels = getIntegerOrDefault(format, MediaFormat.KEY_CHANNEL_COUNT, outputChannels)\n            if (audioTrack == null) ensureAudioTrack()\n''',
'''            outputChannels = getIntegerOrDefault(format, MediaFormat.KEY_CHANNEL_COUNT, outputChannels)\n            outputChannelMask = validDecoderChannelMask(format, outputChannels) ?: outputChannelMask\n            if (audioTrack == null) ensureAudioTrack()\n''','output format mask')
rep(p,
'''            val channelMask = channelMaskForPcmOutput(outputChannels)\n''',
'''            val channelMask = outputChannelMask ?: channelMaskForPcmOutput(outputChannels)\n''','prefer decoder mask')
# Both production and test helper mappings.
old='''        1 -> AudioFormat.CHANNEL_OUT_MONO\n        2 -> AudioFormat.CHANNEL_OUT_STEREO\n        6 -> AudioFormat.CHANNEL_OUT_5POINT1\n        8 -> AudioFormat.CHANNEL_OUT_7POINT1_SURROUND\n        else -> null'''
new='''        1 -> AudioFormat.CHANNEL_OUT_MONO\n        2 -> AudioFormat.CHANNEL_OUT_STEREO\n        3 -> AudioFormat.CHANNEL_OUT_STEREO or AudioFormat.CHANNEL_OUT_FRONT_CENTER\n        4 -> AudioFormat.CHANNEL_OUT_QUAD\n        5 -> AudioFormat.CHANNEL_OUT_QUAD or AudioFormat.CHANNEL_OUT_FRONT_CENTER\n        6 -> AudioFormat.CHANNEL_OUT_5POINT1\n        8 -> AudioFormat.CHANNEL_OUT_7POINT1_SURROUND\n        else -> null'''
rep(p,old,new,'channel mask mappings',count=2)
# Add mask validator before channel-count mapping.
rep(p,
'''    private fun channelMaskForPcmOutput(channelCount: Int): Int? = when (channelCount) {\n''',
'''    private fun validDecoderChannelMask(format: MediaFormat, channelCount: Int): Int? {\n        val mask = runCatching { format.getInteger(MediaFormat.KEY_CHANNEL_MASK) }.getOrNull() ?: return null\n        return mask.takeIf { it > 0 && Integer.bitCount(it) == channelCount }\n    }\n\n    private fun channelMaskForPcmOutput(channelCount: Int): Int? = when (channelCount) {\n''','decoder mask validator')

print('applied remaining PR54 review fixes')

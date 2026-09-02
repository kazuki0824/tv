from pathlib import Path

source = Path(__file__).with_name("codex_apply_pr54_31.py").read_text(encoding="utf-8")
old = '''anchor = \'\'\'        check(!providerAudio.has(\\"liveViewableClaim\\"))\\
        check(TunerSelectionPolicy.selectVideo(service.streams) == null)\\
\'\'\'
if anchor not in text:
    raise SystemExit(\'metadata test anchor mismatch\')
replacement = \'\'\'        check(!providerAudio.has(\\"liveViewableClaim\\"))\\
        check(AudioTrackMetadataPolicy.encodingForPmtStreamType(0x0f) == android.media.MediaFormat.MIMETYPE_AUDIO_AAC)\\
        check(AudioTrackMetadataPolicy.channelCountForComponentType(0x02) == 2)\\
        check(AudioTrackMetadataPolicy.channelCountForComponentType(0x29) == 6)\\
        check(AudioTrackMetadataPolicy.sampleRateHz(0x07) == 48_000)\\
        check(AudioTrackMetadataPolicy.sampleRateHz(0x04) == null)\\
        check(AudioTrackMetadataPolicy.isAudioDescription(0x20))\\
        check(AudioTrackMetadataPolicy.isHardOfHearing(0x40))\\
        check(TunerSelectionPolicy.selectVideo(service.streams) == null)\\
\'\'\'
text = text.replace(anchor, replacement, 1)'''
new = '''anchor = \'\'\'        check(!providerAudio.has(\\"r51PlaybackSupported\\"))\\
        check(TunerSelectionPolicy.selectVideo(service.streams)?.streamType == 0x1b)\\
\'\'\'
if anchor not in text:
    raise SystemExit(\'metadata test anchor mismatch\')
replacement = \'\'\'        check(!providerAudio.has(\\"r51PlaybackSupported\\"))\\
        check(AudioTrackMetadataPolicy.encodingForPmtStreamType(0x0f) == android.media.MediaFormat.MIMETYPE_AUDIO_AAC)\\
        check(AudioTrackMetadataPolicy.channelCountForComponentType(0x02) == 2)\\
        check(AudioTrackMetadataPolicy.channelCountForComponentType(0x29) == 6)\\
        check(AudioTrackMetadataPolicy.sampleRateHz(0x07) == 48_000)\\
        check(AudioTrackMetadataPolicy.sampleRateHz(0x04) == null)\\
        check(AudioTrackMetadataPolicy.isAudioDescription(0x20))\\
        check(AudioTrackMetadataPolicy.isHardOfHearing(0x40))\\
        check(TunerSelectionPolicy.selectVideo(service.streams)?.streamType == 0x1b)\\
\'\'\'
text = text.replace(anchor, replacement, 1)'''
if old not in source:
    raise SystemExit("staging script guard block not found")
source = source.replace(old, new, 1)
exec(compile(source, "codex_apply_pr54_31.py", "exec"), {"__name__": "__main__", "__file__": str(Path(__file__).with_name("codex_apply_pr54_31.py"))})

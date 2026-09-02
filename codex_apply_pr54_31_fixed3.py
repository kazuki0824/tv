from pathlib import Path

script_path = Path(__file__).with_name("codex_apply_pr54_31.py")
namespace = {"__name__": "__main__", "__file__": str(script_path)}
try:
    exec(compile(script_path.read_text(encoding="utf-8"), str(script_path), "exec"), namespace)
except SystemExit as error:
    if str(error) != "metadata test anchor mismatch":
        raise
else:
    raise SystemExit("original staging script unexpectedly completed; fixed3 is no longer needed")

# The original script has already applied all production/schema/doc changes before this late test guard.
test_path = Path("tis/tests/src/com/maleicacid/tvinput/tis/TisR51FixedPlanAcceptanceTest.kt")
text = test_path.read_text(encoding="utf-8")
anchor = '''        check(!providerAudio.has("r51PlaybackSupported"))
        check(TunerSelectionPolicy.selectVideo(service.streams)?.streamType == 0x1b)
'''
if text.count(anchor) != 1:
    raise SystemExit(f"current audio metadata test anchor count={text.count(anchor)}")
replacement = '''        check(!providerAudio.has("r51PlaybackSupported"))
        check(AudioTrackMetadataPolicy.encodingForPmtStreamType(0x0f) == android.media.MediaFormat.MIMETYPE_AUDIO_AAC)
        check(AudioTrackMetadataPolicy.channelCountForComponentType(0x02) == 2)
        check(AudioTrackMetadataPolicy.channelCountForComponentType(0x29) == 6)
        check(AudioTrackMetadataPolicy.sampleRateHz(0x07) == 48_000)
        check(AudioTrackMetadataPolicy.sampleRateHz(0x04) == null)
        check(AudioTrackMetadataPolicy.isAudioDescription(0x20))
        check(AudioTrackMetadataPolicy.isHardOfHearing(0x40))
        check(TunerSelectionPolicy.selectVideo(service.streams)?.streamType == 0x1b)
'''
test_path.write_text(text.replace(anchor, replacement, 1), encoding="utf-8")

provider_test_path = Path("tis/tests/src/com/maleicacid/tvinput/tis/TvProviderWriterProgramsTest.kt")
text = provider_test_path.read_text(encoding="utf-8")
anchor = '''        check(providerData.utf8Contains("secondLanguage"))
        check(providerData.utf8Contains("genres"))
'''
if text.count(anchor) != 1:
    raise SystemExit(f"provider-data audio test anchor count={text.count(anchor)}")
replacement = '''        check(providerData.utf8Contains("secondLanguage"))
        check(providerData.utf8Contains("streamContent"))
        check(providerData.utf8Contains("simulcastGroupTag"))
        check(providerData.utf8Contains("samplingRate"))
        check(providerData.utf8Contains("qualityIndicator"))
        check(providerData.utf8Contains("multiLingual"))
        check(providerData.utf8Contains("genres"))
'''
provider_test_path.write_text(text.replace(anchor, replacement, 1), encoding="utf-8")

projection = Path("ARIB_SI_EPG_TvProvider投影方針.md").read_text(encoding="utf-8")
for forbidden in ["TvTrackInfo.Builder", "setAudioSampleRate(", "setHardOfHearing(", "Tuner.scan("]:
    if forbidden in projection:
        raise SystemExit(f"projection MD scope expansion detected: {forbidden}")

print("applied PR54 #31 and provider-data projection consistency changes")

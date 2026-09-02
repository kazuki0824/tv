from pathlib import Path

script_path = Path(__file__).with_name("codex_apply_pr54_31.py")
source = script_path.read_text(encoding="utf-8")
old_one = 'check(!providerAudio.has(\\"liveViewableClaim\\"))'
new_one = 'check(!providerAudio.has(\\"r51PlaybackSupported\\"))'
old_two = 'check(TunerSelectionPolicy.selectVideo(service.streams) == null)'
new_two = 'check(TunerSelectionPolicy.selectVideo(service.streams)?.streamType == 0x1b)'
if source.count(old_one) != 2:
    raise SystemExit(f"unexpected providerAudio guard count={source.count(old_one)}")
if source.count(old_two) != 2:
    raise SystemExit(f"unexpected audio selection guard count={source.count(old_two)}")
source = source.replace(old_one, new_one)
source = source.replace(old_two, new_two)
exec(compile(source, str(script_path), "exec"), {"__name__": "__main__", "__file__": str(script_path)})

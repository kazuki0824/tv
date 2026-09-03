from pathlib import Path
import re

root = Path('.')
staging = Path(__file__).parent
base = (staging / 'codex_apply_pr54_review_remaining.py').read_text(encoding='utf-8')
start = base.find("p='tis/src/com/maleicacid/tvinput/tis/ChannelScanManager.kt'")
end = base.find("# BS23 fallback", start)
if start < 0 or end < 0:
    raise SystemExit('manager patch block not found')
base = base[:start] + base[end:]
base = base.replace("rep(p,old,new,'channel mask mappings',count=2)", "rep(p,old,new,'channel mask mapping',count=1)")
exec(compile(base, str(Path(__file__)), 'exec'))

# Direct Boot completion: semantic regex anchors tolerate formatting but remain single-match guarded.
p = root / 'tis/src/com/maleicacid/tvinput/tis/ChannelScanManager.kt'
s = p.read_text(encoding='utf-8')
pat = re.compile(r'(?m)^(\s*)val scanTargets = targets\.getOrElse \{ emptyList\(\) \}\s*\n\1val scanResult = controller\.startBootEpgSync\(scanTargets\)')
matches = list(pat.finditer(s))
if len(matches) != 1:
    idx = s.find('startBootEpgSync(targetSnapshot)')
    print('manager diagnostic:', s[max(0, idx-700):idx+1200])
    raise SystemExit(f'boot scan invocation matches={len(matches)}')
def insert_keys(m):
    i=m.group(1)
    return f'{i}val scanTargets = targets.getOrElse {{ emptyList() }}\n{i}val requiredTargetKeys = scanTargets.mapTo(linkedSetOf()) {{ it.serviceKey }}\n{i}val scanResult = controller.startBootEpgSync(scanTargets)'
s = pat.sub(insert_keys, s, count=1)
pat = re.compile(r'(?m)^(\s*)val allRequiredTargetsCommitted = scanResult\.scanned > 0 &&\s*\n\1\s*scanResult\.successfulCandidates == scanResult\.scanned &&\s*\n\1\s*!terminalCancel')
matches = list(pat.finditer(s))
if len(matches) != 1:
    idx = s.find('allRequiredTargetsCommitted')
    print('completion diagnostic:', s[max(0, idx-300):idx+600])
    raise SystemExit(f'boot completion matches={len(matches)}')
def replace_completion(m):
    i=m.group(1)
    return f'{i}val allRequiredTargetsCommitted = requiredTargetKeys.isNotEmpty() &&\n{i}    scanResult.committedServiceKeys.containsAll(requiredTargetKeys) &&\n{i}    !terminalCancel'
s = pat.sub(replace_completion, s, count=1)
p.write_text(s, encoding='utf-8')

# Test-only PCM helper; production mapping was handled by the base script.
p = root / 'tis/src/com/maleicacid/tvinput/tis/PlaybackPipeline.kt'
s = p.read_text(encoding='utf-8')
old_test = '''            1 -> AudioFormat.CHANNEL_OUT_MONO\n            2 -> AudioFormat.CHANNEL_OUT_STEREO\n            6 -> AudioFormat.CHANNEL_OUT_5POINT1\n            8 -> AudioFormat.CHANNEL_OUT_7POINT1_SURROUND\n            else -> null\n'''
new_test = '''            1 -> AudioFormat.CHANNEL_OUT_MONO\n            2 -> AudioFormat.CHANNEL_OUT_STEREO\n            3 -> AudioFormat.CHANNEL_OUT_STEREO or AudioFormat.CHANNEL_OUT_FRONT_CENTER\n            4 -> AudioFormat.CHANNEL_OUT_QUAD\n            5 -> AudioFormat.CHANNEL_OUT_QUAD or AudioFormat.CHANNEL_OUT_FRONT_CENTER\n            6 -> AudioFormat.CHANNEL_OUT_5POINT1\n            8 -> AudioFormat.CHANNEL_OUT_7POINT1_SURROUND\n            else -> null\n'''
if s.count(old_test) != 1:
    raise SystemExit(f'test PCM mapping count={s.count(old_test)}')
p.write_text(s.replace(old_test, new_test, 1), encoding='utf-8')
print('applied semantic Direct Boot ledger and PCM mappings')

from pathlib import Path
root = Path('.')
staging = Path(__file__).parent
base = (staging / 'codex_apply_pr54_review_remaining.py').read_text(encoding='utf-8')
# Remove fragile ChannelScanManager block; apply it directly below.
start = base.find("p='tis/src/com/maleicacid/tvinput/tis/ChannelScanManager.kt'")
end = base.find("# BS23 fallback", start)
if start < 0 or end < 0: raise SystemExit('manager patch block not found')
base = base[:start] + base[end:]
# Production and test-helper mappings differ in indentation/context on current HEAD.
base = base.replace("rep(p,old,new,'channel mask mappings',count=2)", "rep(p,old,new,'channel mask mapping',count=1)")
exec(compile(base, str(Path(__file__)), 'exec'))

# Direct Boot exact source patch.
p = root / 'tis/src/com/maleicacid/tvinput/tis/ChannelScanManager.kt'
s = p.read_text(encoding='utf-8')
old = '''                val scanTargets = targets.getOrElse { emptyList() }\n                val scanResult = controller.startBootEpgSync(scanTargets)\n                val terminalCancel = scanResult.terminalCancelObserved || isCancelledGeneration(generation)\n                val allRequiredTargetsCommitted = scanResult.scanned > 0 &&\n                    scanResult.successfulCandidates == scanResult.scanned &&\n                    !terminalCancel\n'''
new = '''                val scanTargets = targets.getOrElse { emptyList() }\n                val requiredTargetKeys = scanTargets.mapTo(linkedSetOf()) { it.serviceKey }\n                val scanResult = controller.startBootEpgSync(scanTargets)\n                val terminalCancel = scanResult.terminalCancelObserved || isCancelledGeneration(generation)\n                val allRequiredTargetsCommitted = requiredTargetKeys.isNotEmpty() &&\n                    scanResult.committedServiceKeys.containsAll(requiredTargetKeys) &&\n                    !terminalCancel\n'''
if s.count(old) != 1: raise SystemExit(f'exact manager block count={s.count(old)}')
p.write_text(s.replace(old,new,1), encoding='utf-8')

# Test-only mask helper has a different indentation context; align it explicitly.
p = root / 'tis/src/com/maleicacid/tvinput/tis/PlaybackPipeline.kt'
s = p.read_text(encoding='utf-8')
old_test = '''            1 -> AudioFormat.CHANNEL_OUT_MONO\n            2 -> AudioFormat.CHANNEL_OUT_STEREO\n            6 -> AudioFormat.CHANNEL_OUT_5POINT1\n            8 -> AudioFormat.CHANNEL_OUT_7POINT1_SURROUND\n            else -> null\n'''
new_test = '''            1 -> AudioFormat.CHANNEL_OUT_MONO\n            2 -> AudioFormat.CHANNEL_OUT_STEREO\n            3 -> AudioFormat.CHANNEL_OUT_STEREO or AudioFormat.CHANNEL_OUT_FRONT_CENTER\n            4 -> AudioFormat.CHANNEL_OUT_QUAD\n            5 -> AudioFormat.CHANNEL_OUT_QUAD or AudioFormat.CHANNEL_OUT_FRONT_CENTER\n            6 -> AudioFormat.CHANNEL_OUT_5POINT1\n            8 -> AudioFormat.CHANNEL_OUT_7POINT1_SURROUND\n            else -> null\n'''
if s.count(old_test) != 1: raise SystemExit(f'test PCM mapping count={s.count(old_test)}')
p.write_text(s.replace(old_test,new_test,1), encoding='utf-8')
print('applied exact boot ledger and PCM mappings')

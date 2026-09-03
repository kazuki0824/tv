from pathlib import Path

root = Path('.')
staging = Path(__file__).parent
base = (staging / 'codex_apply_pr54_review_remaining.py').read_text(encoding='utf-8')
start = base.find("p='tis/src/com/maleicacid/tvinput/tis/ChannelScanManager.kt'")
end = base.find("# BS23 fallback", start)
if start < 0 or end < 0:
    raise SystemExit('ChannelScanManager patch block not found in staging script')
# Execute all remaining changes except the fragile manager patch.
without_manager = base[:start] + base[end:]
exec(compile(without_manager, str(Path(__file__)), 'exec'))

path = root / 'tis/src/com/maleicacid/tvinput/tis/ChannelScanManager.kt'
text = path.read_text(encoding='utf-8')
old = '''                val scanTargets = targets.getOrElse { emptyList() }\n                val scanResult = controller.startBootEpgSync(scanTargets)\n                val terminalCancel = scanResult.terminalCancelObserved || isCancelledGeneration(generation)\n                val allRequiredTargetsCommitted = scanResult.scanned > 0 &&\n                    scanResult.successfulCandidates == scanResult.scanned &&\n                    !terminalCancel\n'''
new = '''                val scanTargets = targets.getOrElse { emptyList() }\n                val requiredTargetKeys = scanTargets.mapTo(linkedSetOf()) { it.serviceKey }\n                val scanResult = controller.startBootEpgSync(scanTargets)\n                val terminalCancel = scanResult.terminalCancelObserved || isCancelledGeneration(generation)\n                val allRequiredTargetsCommitted = requiredTargetKeys.isNotEmpty() &&\n                    scanResult.committedServiceKeys.containsAll(requiredTargetKeys) &&\n                    !terminalCancel\n'''
if text.count(old) != 1:
    raise SystemExit(f'exact ChannelScanManager target block count={text.count(old)}')
path.write_text(text.replace(old, new, 1), encoding='utf-8')
print('applied exact Direct Boot service-key completion ledger')

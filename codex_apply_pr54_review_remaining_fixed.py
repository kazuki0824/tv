from pathlib import Path

base = Path(__file__).with_name('codex_apply_pr54_review_remaining.py').read_text(encoding='utf-8')
old = '''            val scanTargets = targets.getOrElse { emptyList() }\\n            val candidates = controller.maintenanceCandidates(scanTargets)\\n            val scanResult = controller.startBootEpgSync(scanTargets)\\n            val terminalCancel = token.get() || scanResult.terminalCancelObserved\\n            val allRequiredTargetsCommitted = scanResult.scanned > 0 &&\\n                scanResult.successfulCandidates == scanResult.scanned &&\\n                !terminalCancel\\n'''
new = '''                val scanTargets = targets.getOrElse { emptyList() }\\n                val requiredTargetKeys = scanTargets.mapTo(linkedSetOf()) { it.serviceKey }\\n                val scanResult = controller.startBootEpgSync(scanTargets)\\n                val terminalCancel = scanResult.terminalCancelObserved || isCancelledGeneration(generation)\\n                val allRequiredTargetsCommitted = requiredTargetKeys.isNotEmpty() &&\\n                    scanResult.committedServiceKeys.containsAll(requiredTargetKeys) &&\\n                    !terminalCancel\\n'''
# Rewrite the patch-script anchor/replacement pair, not target source directly.
start = base.find("p='tis/src/com/maleicacid/tvinput/tis/ChannelScanManager.kt'")
end = base.find("# BS23 fallback", start)
if start < 0 or end < 0:
    raise SystemExit('ChannelScanManager patch block not found')
block = base[start:end]
first = block.find("'''", block.find('rep(p,'))
second = block.find("'''", first + 3)
third = block.find("'''", second + 3)
fourth = block.find("'''", third + 3)
if min(first, second, third, fourth) < 0:
    raise SystemExit('ChannelScanManager patch literals not found')
replacement_block = block[:first] + "'''" + '''                val scanTargets = targets.getOrElse { emptyList() }\n                val scanResult = controller.startBootEpgSync(scanTargets)\n                val terminalCancel = scanResult.terminalCancelObserved || isCancelledGeneration(generation)\n                val allRequiredTargetsCommitted = scanResult.scanned > 0 &&\n                    scanResult.successfulCandidates == scanResult.scanned &&\n                    !terminalCancel\n''' + "'''" + block[second+3:third] + "'''" + '''                val scanTargets = targets.getOrElse { emptyList() }\n                val requiredTargetKeys = scanTargets.mapTo(linkedSetOf()) { it.serviceKey }\n                val scanResult = controller.startBootEpgSync(scanTargets)\n                val terminalCancel = scanResult.terminalCancelObserved || isCancelledGeneration(generation)\n                val allRequiredTargetsCommitted = requiredTargetKeys.isNotEmpty() &&\n                    scanResult.committedServiceKeys.containsAll(requiredTargetKeys) &&\n                    !terminalCancel\n''' + "'''" + block[fourth+3:]
fixed = base[:start] + replacement_block + base[end:]
exec(compile(fixed, str(Path(__file__)), 'exec'))

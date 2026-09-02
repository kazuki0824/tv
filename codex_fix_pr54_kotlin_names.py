from pathlib import Path

scan = Path("tis/src/com/maleicacid/tvinput/tis/ScanPlan.kt")
text = scan.read_text(encoding="utf-8")
old = 'backendHint == BS_DISCOVERY_BACKEND_HINT && streamSelector.type == StreamSelectorType.NONE'
new = 'backendHint == JapanIsdbScanPlan.BS_DISCOVERY_BACKEND_HINT && streamSelector.type == StreamSelectorType.NONE'
if text.count(old) != 1:
    raise SystemExit(f"BS discovery constant reference count={text.count(old)}")
scan.write_text(text.replace(old, new, 1), encoding="utf-8")

policy = Path("tis/src/com/maleicacid/tvinput/tis/TunerSelectionPolicy.kt")
text = policy.read_text(encoding="utf-8")
old = 'import com.maleicacid.tvinput.aribsi.AribElementaryStream\n'
new = 'import com.maleicacid.tvinput.aribsi.AribComponentEntry\nimport com.maleicacid.tvinput.aribsi.AribElementaryStream\n'
if text.count(old) != 1:
    raise SystemExit(f"AribElementaryStream import anchor count={text.count(old)}")
policy.write_text(text.replace(old, new, 1), encoding="utf-8")
print("fixed PR54 Kotlin symbol resolution")

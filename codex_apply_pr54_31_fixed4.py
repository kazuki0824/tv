from pathlib import Path
import runpy

runpy.run_path(str(Path(__file__).with_name("codex_apply_pr54_31_fixed3.py")), run_name="__main__")
path = Path("tis/src/com/maleicacid/tvinput/tis/TunerSelectionPolicy.kt")
text = path.read_text(encoding="utf-8")
path.write_text(text.rstrip() + "\n", encoding="utf-8")

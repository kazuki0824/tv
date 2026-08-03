from __future__ import annotations

import sys
from pathlib import Path

source_path = Path(__file__).with_name("pr2-review7-patch-20260803.py")
source = source_path.read_text(encoding="utf-8")
needle = "required_hal = ["
injected = """hal = hal.replace(\"旧要求を正確に1回再投入する\", \"旧要求を自動再投入しない\")
hal = hal.replace(\"旧要求を正確に1回再投入\", \"旧要求を自動再投入しない\")

required_hal = ["""
if source.count(needle) != 1:
    raise RuntimeError("required_hal injection point mismatch")
source = source.replace(needle, injected, 1)
exec(compile(source, str(source_path), "exec"), {"__name__": "__main__", "__file__": str(source_path)})

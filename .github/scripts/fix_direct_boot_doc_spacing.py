from pathlib import Path

for path in ("tis/DESIGN_JA.md", "tis/INTEGRATION.md"):
    target = Path(path)
    text = target.read_text()
    text = text.replace("ライブ視聴の選局を優先する。\n## ", "ライブ視聴の選局を優先する。\n\n## ", 1)
    target.write_text(text)

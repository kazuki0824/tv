from pathlib import Path

p = Path('.github/scripts/pr54_review_round2_once.py')
text = p.read_text()
start_marker = 'p = Path("tis/src/com/maleicacid/tvinput/tis/PlaybackPipeline.kt")\ntext = p.read_text()\n# AvStreamSelectionにcaptionDiscoveryを追加する。\n'
start = text.index(start_marker)
next_marker = "old = '''        val subtitle = selection.subtitle"
next_pos = text.index(next_marker, start)
block = text[start:next_pos]
fixed = block.replace(
    'p = Path("tis/src/com/maleicacid/tvinput/tis/PlaybackPipeline.kt")',
    'p = Path("tis/src/com/maleicacid/tvinput/tis/TunerController.kt")',
    1,
) + 'p.write_text(text)\n\np = Path("tis/src/com/maleicacid/tvinput/tis/PlaybackPipeline.kt")\ntext = p.read_text()\n'
text = text[:start] + fixed + text[next_pos:]
text = text.replace('!obsolete_section_keys.contains(key)', '!obsolete_section_keys.contains(*key)')
p.write_text(text)

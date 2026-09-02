from pathlib import Path
p = Path('tis/tests/src/com/maleicacid/tvinput/aribsi/NativeAribSiParserCasDiscoveryTest.kt')
text = p.read_text(encoding='utf-8')
old = 'event.descriptors.linkage.single().privateDataHex == "aabb"'
new = 'event.descriptors.linkage.single().privateDataPrefixHex == "aabb"'
if text.count(old) != 1:
    raise SystemExit(f'expected linkage test reference once, found {text.count(old)}')
p.write_text(text.replace(old, new, 1), encoding='utf-8')
print('aligned linkage test field name')

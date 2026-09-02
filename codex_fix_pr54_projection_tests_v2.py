from pathlib import Path

# Multilingual preservation intentionally makes extendedItems multi-valued.
p = Path('tis/tests/src/com/maleicacid/tvinput/tis/EventModelMapperDescriptorTest.kt')
text = p.read_text(encoding='utf-8')
old = '        check(record.descriptors.extendedItems.single().itemDescription == "出演")\n'
new = '''        check(record.descriptors.extendedItems.size == 2)\n        check(record.descriptors.extendedItems.first { it.languageCode == "jpn" }.itemDescription == "出演")\n        check(record.descriptors.extendedItems.first { it.languageCode == "eng" }.itemDescription == "Cast")\n'''
if text.count(old) != 1:
    raise SystemExit(f'extendedItems assertion expected once, found {text.count(old)}')
p.write_text(text.replace(old, new, 1), encoding='utf-8')

# Test fixture conversion must preserve JSON null as Kotlin null. JSONObject.optString(NULL)
# can yield the literal "null", which is correctly rejected by production ISO-639 validation.
p = Path('tis/tests/src/com/maleicacid/tvinput/tis/TisR51FixedPlanAcceptanceTest.kt')
text = p.read_text(encoding='utf-8')
old = '''            codec = obj.optString("codec").takeIf { it.isNotBlank() },\n            language = obj.optString("language").takeIf { it.isNotBlank() },\n            dataComponentId = obj.optInt("dataComponentId").takeIf { obj.has("dataComponentId") },\n            captionServiceKind = obj.optString("captionServiceKind").takeIf { it.isNotBlank() },\n'''
new = '''            codec = obj.optString("codec").takeIf { !obj.isNull("codec") && it.isNotBlank() },\n            language = obj.optString("language").takeIf { !obj.isNull("language") && it.isNotBlank() },\n            dataComponentId = obj.optInt("dataComponentId").takeIf { obj.has("dataComponentId") && !obj.isNull("dataComponentId") },\n            captionServiceKind = obj.optString("captionServiceKind").takeIf { !obj.isNull("captionServiceKind") && it.isNotBlank() },\n'''
if text.count(old) != 1:
    raise SystemExit(f'component fixture anchor expected once, found {text.count(old)}')
p.write_text(text.replace(old, new, 1), encoding='utf-8')
print('aligned multilingual assertions and nullable metadata fixtures')

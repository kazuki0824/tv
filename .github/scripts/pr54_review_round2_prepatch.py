from pathlib import Path

p = Path('.github/scripts/pr54_review_round2_once.py')
text = p.read_text()
old = '''# manual known-field helperはnested validationから不要。
for fn_name in ("field_object_has_only", "object_has_only"):
    marker = f"fn {fn_name}("
    if marker in text:
        s = text.index(marker)
        # 次の関数境界まで削除。ただしobject_has_onlyが他用途なら後段compileで検出する。
        m = re.search(r"\\nfn [A-Za-z0-9_]+\\(", text[s + 1:])
        if m:
            e = s + 1 + m.start() + 1
            candidate = text[:s] + text[e:]
            if fn_name not in candidate:
                text = candidate
'''
new = '''# manual known-field helperはnested validationから不要。
manual_helpers = ''' + '"""' + '''fn field_object_has_only(parent: &serde_json::Value, field: &str, known_keys: &[&str]) -> bool {
    parent
        .get(field)
        .map(|value| object_has_only(value, known_keys))
        .unwrap_or(true)
}

fn object_has_only(value: &serde_json::Value, known_keys: &[&str]) -> bool {
    value
        .as_object()
        .map(|object| {
            object
                .keys()
                .all(|key| known_keys.iter().any(|known| *known == key))
        })
        .unwrap_or(false)
}

''' + '"""' + '''
if manual_helpers not in text:
    raise SystemExit("F02 manual helper blockが見つかりません")
text = text.replace(manual_helpers, "", 1)
'''
if old not in text:
    raise SystemExit('main scriptのhelper削除blockが見つかりません')
text = text.replace(old, new, 1)
p.write_text(text)

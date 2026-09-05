from pathlib import Path
import json
import re
import urllib.request


def replace_once(path: str, old: str, new: str) -> None:
    p = Path(path)
    text = p.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"置換対象が一意ではありません: {path} count={count}")
    p.write_text(text.replace(old, new, 1))


# provider-dataのdescriptor diagnostic nested objectをclosed schemaへ統一する。
p = Path("arib_si_engine_rs/src/core/provider_data.rs")
text = p.read_text()
for struct_name in ("SectionScopeV1", "DescriptorScopeV1", "DescriptorDiagnosticV1"):
    old = f'#[serde(rename_all = "camelCase")]\npub(crate) struct {struct_name}'
    new = f'#[serde(rename_all = "camelCase", deny_unknown_fields)]\npub(crate) struct {struct_name}'
    if text.count(old) != 1:
        raise SystemExit(f"deny_unknown_fields対象が一意ではありません: {struct_name}")
    text = text.replace(old, new, 1)
call_start = text.index("        collect_descriptor_diagnostic_unknown(")
call_end = text.index("        collect_array_unknown(", call_start)
text = text[:call_start] + text[call_end:]
fn_start = text.index("fn collect_descriptor_diagnostic_unknown(")
fn_end = text.index("\nfn collect_array_unknown(", fn_start)
text = text[:fn_start] + text[fn_end + 1 :]
p.write_text(text)

# descriptor diagnostic JSON Schemaも同じclosed contractへ揃える。
p = Path("arib_si_engine_rs/schema/descriptor_diagnostic_v1.schema.json")
data = json.loads(p.read_text())
data["additionalProperties"] = False
data["$defs"]["sectionScope"]["additionalProperties"] = False
data["$defs"]["descriptorScope"]["additionalProperties"] = False
p.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n")

# EIT version更新は同じsection_numberだけを置換し、未到着sectionを消さない。
p = Path("arib_si_engine_rs/src/core/eit.rs")
text = p.read_text()
old = '''        let scope_section_keys: Vec<EitSectionKey> = self
            .section_events
            .keys()
            .filter(|key| {
                key.table_id == header.table_id
                    && key.service_id == service_id
                    && key.transport_stream_id == transport_stream_id
                    && key.original_network_id == original_network_id
            })
            .cloned()
            .collect();
        let scope_version_changed = scope_section_keys.iter().any(|key| {
            self.section_events
                .get(key)
                .map(|old| old.version != version)
                .unwrap_or(false)
        });
        let mut previous_keys: BTreeSet<EitEventKey> = BTreeSet::new();
        if scope_version_changed {
            for key in scope_section_keys {
                if let Some(old) = self.section_events.remove(&key) {
                    previous_keys.extend(old.event_keys);
                }
                self.diagnostic_section_events.remove(&key);
            }
        } else {
            previous_keys = self
                .section_events
                .get(&section_key)
                .map(|old| old.event_keys.clone())
                .unwrap_or_default();
        }
'''
new = '''        let previous_keys: BTreeSet<EitEventKey> = self
            .section_events
            .get(&section_key)
            .map(|old| old.event_keys.clone())
            .unwrap_or_default();
'''
if text.count(old) != 1:
    raise SystemExit("EIT version置換対象が一意ではありません")
text = text.replace(old, new, 1)
marker = '''    #[test]
    fn authoritative_valid_update_window_marks_obsolete_delete_allowed() {'''
test = '''    #[test]
    fn version_update_replaces_only_matching_section_number() {
        let mut store = EitStore::default();
        let start1 = [0xee, 0x00, 0x12, 0x00, 0x00];
        let start2 = [0xee, 0x01, 0x13, 0x00, 0x00];
        let mut section0 = eit_body(1, &[(1, start1)]);
        section0[6] = 0;
        section0[7] = 1;
        let mut section1 = eit_body(1, &[(2, start2)]);
        section1[6] = 1;
        section1[7] = 1;
        store.upsert_section(&section_with_crc(section0));
        store.upsert_section(&section_with_crc(section1));
        assert_eq!(store.snapshot_present_following_actual().len(), 2);

        let mut new_section0 = eit_body(2, &[(1, start1)]);
        new_section0[6] = 0;
        new_section0[7] = 1;
        store.upsert_section(&section_with_crc(new_section0));

        let events = store.snapshot_present_following_actual();
        assert_eq!(events.len(), 2);
        assert!(events.iter().any(|event| event.event_id == 1 && event.version == 2));
        assert!(events.iter().any(|event| event.event_id == 2 && event.version == 1));
    }

'''
if text.count(marker) != 1:
    raise SystemExit("EIT test挿入位置が一意ではありません")
p.write_text(text.replace(marker, test + marker, 1))

# JIS X 0213面1/2をPython標準codecから静的Unicode表へ生成する。
def codepoint_literal(s: str) -> str:
    if len(s) != 1:
        raise ValueError(s)
    return f"\\u{{{ord(s):x}}}"

plane1 = []
plane2 = []
for first in range(0x21, 0x7F):
    for second in range(0x21, 0x7F):
        try:
            s = bytes([first | 0x80, second | 0x80]).decode("euc_jis_2004")
            if len(s) == 1:
                plane1.append((first, second, s))
        except UnicodeDecodeError:
            pass
        try:
            s = bytes([0x8F, first | 0x80, second | 0x80]).decode("euc_jis_2004")
            if len(s) == 1:
                plane2.append((first, second, s))
        except UnicodeDecodeError:
            pass

# Additional Symbolsは製品依存libaribcaptionの固定commitのUnicode表を入力にする。
url = "https://raw.githubusercontent.com/xqq/libaribcaption/c64c23b8905ba514b87c9789269e9f66f949ffe0/src/decoder/b24_gaiji_table.hpp"
source = urllib.request.urlopen(url, timeout=20).read().decode()
body = source.split("kAdditionalSymbolsTable_Unicode[] = {", 1)[1].split("};", 1)[0]
codepoints = [int(value, 16) for value in re.findall(r"0x([0-9a-fA-F]+)", body)]
if len(codepoints) < 940:
    raise SystemExit(f"追加記号表が短すぎます: {len(codepoints)}")

out = [
    "// 自動生成。ARIB SI/EPGで用いるJIS互換漢字面1/2と追加記号のUnicode対応。",
    "// JIS X 0213はPython標準euc_jis_2004 codec、追加記号はlibaribcaption固定commit c64c23b...を入力とする。",
    "",
]
for name, rows in (("map_jis_x0213_plane1", plane1), ("map_jis_x0213_plane2", plane2)):
    out.append(f"fn {name}(first: u8, second: u8) -> &'static str {{")
    out.append("    match (first, second) {")
    for first, second, value in rows:
        out.append(f'        (0x{first:02x}, 0x{second:02x}) => "{codepoint_literal(value)}",')
    out.extend(["        _ => \"�\",", "    }", "}", ""])
out.append("fn map_arib_additional_symbol(first: u8, second: u8) -> &'static str {")
out.append('    if !(0x75..=0x7e).contains(&first) || !(0x21..=0x7e).contains(&second) { return "�"; }')
out.append("    match ((first - 0x75) as usize) * 94 + (second - 0x21) as usize {")
for index, cp in enumerate(codepoints[:940]):
    if cp != 0xFFFD and cp <= 0x10FFFF:
        out.append(f'        {index} => "\\u{{{cp:x}}}",')
out.extend(["        _ => \"�\",", "    }", "}"])
Path("arib_si_engine_rs/src/core/arib_extended_graphic_table.rs").write_text("\n".join(out) + "\n")

p = Path("arib_si_engine_rs/src/core/arib_string.rs")
text = p.read_text()
text = text.replace('include!("arib_jis_x0208_table.rs");\n', 'include!("arib_jis_x0208_table.rs");\ninclude!("arib_extended_graphic_table.rs");\n', 1)
text = text.replace(
    '''enum GraphicSet {
    Alnum,
    Hiragana,
    Katakana,
    Kanji,
}''',
    '''enum GraphicSet {
    Alnum,
    Hiragana,
    Katakana,
    Kanji,
    JisPlane1,
    JisPlane2,
    AdditionalSymbols,
}''',
    1,
)
text = text.replace(
    '''        GraphicSet::Kanji => {
            let second = *bytes
                .get(index + 2)
                .ok_or(AribStringDecodeError::TruncatedGraphic)?;
            map_kanji(first, second).to_string()
        }
    };
    let consumed_after_control = if matches!(set, GraphicSet::Kanji) {
        2
    } else {
        1
    };''',
    '''        GraphicSet::Kanji | GraphicSet::JisPlane1 | GraphicSet::JisPlane2 | GraphicSet::AdditionalSymbols => {
            let second = *bytes
                .get(index + 2)
                .ok_or(AribStringDecodeError::TruncatedGraphic)?;
            map_two_byte_graphic(set, first, second).to_string()
        }
    };
    let consumed_after_control = if is_two_byte_graphic(set) { 2 } else { 1 };''',
    1,
)
text = text.replace("if matches!(set, GraphicSet::Kanji) {\n                        3", "if is_two_byte_graphic(set) {\n                        3", 1)
text = text.replace("if matches!(state.gl, GraphicSet::Kanji) && bytes.get(index + 1).is_some()", "if is_two_byte_graphic(state.gl) && bytes.get(index + 1).is_some()", 1)
text = text.replace("if matches!(state.gr, GraphicSet::Kanji) && bytes.get(index + 1).is_some()", "if is_two_byte_graphic(state.gr) && bytes.get(index + 1).is_some()", 1)
text = text.replace(
    '''                GraphicSet::Kanji => {
                    let Some(next) = bytes.get(index + 1).copied() else {''',
    '''                GraphicSet::Kanji | GraphicSet::JisPlane1 | GraphicSet::JisPlane2 | GraphicSet::AdditionalSymbols => {
                    let Some(next) = bytes.get(index + 1).copied() else {''',
    1,
)
text = text.replace("out.push_str(map_kanji(byte, next));", "out.push_str(map_two_byte_graphic(state.gl, byte, next));", 1)
text = text.replace(
    '''                    GraphicSet::Kanji => {
                        let Some(next) = bytes.get(index + 1).copied() else {''',
    '''                    GraphicSet::Kanji | GraphicSet::JisPlane1 | GraphicSet::JisPlane2 | GraphicSet::AdditionalSymbols => {
                        let Some(next) = bytes.get(index + 1).copied() else {''',
    1,
)
text = text.replace("out.push_str(map_kanji(normalized, next & 0x7f));", "out.push_str(map_two_byte_graphic(state.gr, normalized, next & 0x7f));", 1)
old = '''                (b'$', b'B') => {
                    state.g0 = GraphicSet::Kanji;
                    state.gl = state.g0;
                    3
                }
                (b'$', b'(') if bytes.len() >= 4 && matches!(bytes[3], b'B' | b'@') => {
                    state.g0 = GraphicSet::Kanji;
                    state.gl = state.g0;
                    4
                }
                (b'$', b')') if bytes.len() >= 4 && matches!(bytes[3], b'B' | b'@') => {
                    state.g1 = GraphicSet::Kanji;
                    4
                }
                (b'$', b'*') if bytes.len() >= 4 && matches!(bytes[3], b'B' | b'@') => {
                    state.g2 = GraphicSet::Kanji;
                    4
                }
                (b'$', b'+') if bytes.len() >= 4 && matches!(bytes[3], b'B' | b'@') => {
                    state.g3 = GraphicSet::Kanji;
                    4
                }'''
new = '''                (b'$', final_byte @ (b'B' | b'@' | b'9' | b':' | b';')) => {
                    state.g0 = two_byte_graphic_set(final_byte)?;
                    state.gl = state.g0;
                    3
                }
                (b'$', b'(') if bytes.len() >= 4 && matches!(bytes[3], b'B' | b'@' | b'9' | b':' | b';') => {
                    state.g0 = two_byte_graphic_set(bytes[3])?;
                    state.gl = state.g0;
                    4
                }
                (b'$', b')') if bytes.len() >= 4 && matches!(bytes[3], b'B' | b'@' | b'9' | b':' | b';') => {
                    state.g1 = two_byte_graphic_set(bytes[3])?;
                    4
                }
                (b'$', b'*') if bytes.len() >= 4 && matches!(bytes[3], b'B' | b'@' | b'9' | b':' | b';') => {
                    state.g2 = two_byte_graphic_set(bytes[3])?;
                    4
                }
                (b'$', b'+') if bytes.len() >= 4 && matches!(bytes[3], b'B' | b'@' | b'9' | b':' | b';') => {
                    state.g3 = two_byte_graphic_set(bytes[3])?;
                    4
                }'''
if text.count(old) != 1:
    raise SystemExit("ARIB ESC置換対象が一意ではありません")
text = text.replace(old, new, 1)
marker = "fn map_hiragana(byte: u8) -> &'static str {"
helpers = '''fn is_two_byte_graphic(set: GraphicSet) -> bool {
    matches!(set, GraphicSet::Kanji | GraphicSet::JisPlane1 | GraphicSet::JisPlane2 | GraphicSet::AdditionalSymbols)
}

fn two_byte_graphic_set(final_byte: u8) -> Result<GraphicSet, AribStringDecodeError> {
    match final_byte {
        b'B' | b'@' => Ok(GraphicSet::Kanji),
        b'9' => Ok(GraphicSet::JisPlane1),
        b':' => Ok(GraphicSet::JisPlane2),
        b';' => Ok(GraphicSet::AdditionalSymbols),
        _ => Err(AribStringDecodeError::UnsupportedEscape),
    }
}

fn map_two_byte_graphic(set: GraphicSet, first: u8, second: u8) -> &'static str {
    match set {
        GraphicSet::Kanji => map_kanji(first, second),
        GraphicSet::JisPlane1 => map_jis_x0213_plane1(first, second),
        GraphicSet::JisPlane2 => map_jis_x0213_plane2(first, second),
        GraphicSet::AdditionalSymbols => map_arib_additional_symbol(first, second),
        _ => "�",
    }
}

'''
if text.count(marker) != 1:
    raise SystemExit("ARIB helper挿入位置が一意ではありません")
text = text.replace(marker, helpers + marker, 1)
test_marker = '''    #[test]
    fn arib_string_decodes_basic_kanji() {'''
tests = '''    #[test]
    fn arib_string_decodes_jis_compatible_plane1_without_replacement() {
        let bytes = [0x1b, b'$', b'(', b'9', 0x21, 0x21];
        let (decoded, diagnostic) = decode_arib_string_lossy(&bytes);
        assert_ne!(decoded, "�");
        assert_eq!(diagnostic.replacement_count, 0);
    }

    #[test]
    fn arib_string_decodes_jis_compatible_plane2_without_replacement() {
        let bytes = [0x1b, b'$', b'(', b':', 0x21, 0x21];
        let (decoded, diagnostic) = decode_arib_string_lossy(&bytes);
        assert_ne!(decoded, "�");
        assert_eq!(diagnostic.replacement_count, 0);
    }

    #[test]
    fn arib_string_decodes_additional_symbol_without_replacement() {
        let bytes = [0x1b, b'$', b'(', b';', 0x75, 0x21];
        let (decoded, diagnostic) = decode_arib_string_lossy(&bytes);
        assert_eq!(decoded, "\\u{3402}");
        assert_eq!(diagnostic.replacement_count, 0);
    }

'''
if text.count(test_marker) != 1:
    raise SystemExit("ARIB test挿入位置が一意ではありません")
p.write_text(text.replace(test_marker, tests + test_marker, 1))

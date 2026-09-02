from pathlib import Path
import runpy

src = Path(__file__).with_name('codex_apply_pr54_projection_gaps.py').read_text(encoding='utf-8')
src = src.replace('language_fragments', 'fragments')
# The stored and request provider structs intentionally share the same field sequence.
# Allow only the first guarded replacement to consume the first occurrence; the request
# replacement then sees exactly one remaining occurrence.
old_helper = """    count = text.count(old)\n    if count != 1:\n        raise SystemExit(f'{label}: expected 1 occurrence, found {count}')\n    write(path, text.replace(old, new, 1))\n"""
new_helper = """    count = text.count(old)\n    if label == 'provider stored candidates' and count == 2:\n        write(path, text.replace(old, new, 1))\n        return\n    if count != 1:\n        raise SystemExit(f'{label}: expected 1 occurrence, found {count}')\n    write(path, text.replace(old, new, 1))\n"""
if old_helper not in src:
    raise SystemExit('replace_once helper anchor mismatch')
src = src.replace(old_helper, new_helper, 1)

start = src.index("path = 'arib_si_engine_rs/src/descriptors.rs'\nreplace_once(path,")
end = src.index("old_short_tail =", start)
q = "'''"
replacement = """path = 'arib_si_engine_rs/src/descriptors.rs'\nreplace_once(path,\n""" + q + """#[derive(Clone, Debug, Default, Eq, PartialEq)]\npub struct EventDescriptors {\n    pub diagnostics: Vec<DescriptorDiagnostic>,\n    pub title: String,\n    /// short_event_descriptor.text。TvProvider の SHORT_DESCRIPTION に対応する。\n    pub description: String,\n    /// extended_event_descriptor.text。TvProvider の LONG_DESCRIPTION の詳細本文に対応する。\n    pub extended_description: String,\n""" + q + ",\n" + q + """#[derive(Clone, Debug, Default, Eq, PartialEq)]\npub struct EventDescriptors {\n    pub diagnostics: Vec<DescriptorDiagnostic>,\n    pub title: String,\n    /// short_event_descriptor.text。TvProvider の SHORT_DESCRIPTION に対応する。\n    pub description: String,\n    /// extended_event_descriptor.text。TvProvider の LONG_DESCRIPTION の詳細本文に対応する。\n    pub extended_description: String,\n    pub short_events: Vec<ShortEventText>,\n    pub extended_texts: Vec<ExtendedEventText>,\n""" + q + ", 'multilingual descriptor fields')\n\nreplace_once(path,\n" + q + """#[derive(Clone, Debug, Eq, PartialEq)]\npub struct ExtendedEventItem {\n    pub language_code: String,\n    pub item_description: String,\n    pub item_text: String,\n}\n""" + q + ",\n" + q + """#[derive(Clone, Debug, Eq, PartialEq)]\npub struct ExtendedEventItem {\n    pub language_code: String,\n    pub item_description: String,\n    pub item_text: String,\n}\n\n#[derive(Clone, Debug, Eq, PartialEq)]\npub struct ShortEventText {\n    pub language_code: String,\n    pub title: String,\n    pub text: String,\n}\n\n#[derive(Clone, Debug, Eq, PartialEq)]\npub struct ExtendedEventText {\n    pub language_code: String,\n    pub text: String,\n}\n""" + q + ", 'multilingual descriptor models')\n\n"
runtime = Path('/tmp/codex_apply_pr54_projection_gaps_runtime.py')
runtime.write_text(src[:start] + replacement + src[end:], encoding='utf-8')
runpy.run_path(str(runtime), run_name='__main__')

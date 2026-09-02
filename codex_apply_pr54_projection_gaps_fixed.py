from pathlib import Path
import runpy

src = Path(__file__).with_name('codex_apply_pr54_projection_gaps.py').read_text(encoding='utf-8')
start = src.index("path = 'arib_si_engine_rs/src/descriptors.rs'\nreplace_once(path,")
end = src.index("old_short_tail =", start)
replacement = r'''path = 'arib_si_engine_rs/src/descriptors.rs'
replace_once(path,
''' + "'''" + r'''#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EventDescriptors {
    pub diagnostics: Vec<DescriptorDiagnostic>,
    pub title: String,
    /// short_event_descriptor.text。TvProvider の SHORT_DESCRIPTION に対応する。
    pub description: String,
    /// extended_event_descriptor.text。TvProvider の LONG_DESCRIPTION の詳細本文に対応する。
    pub extended_description: String,
''' + "'''" + r''',
''' + "'''" + r'''#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EventDescriptors {
    pub diagnostics: Vec<DescriptorDiagnostic>,
    pub title: String,
    /// short_event_descriptor.text。TvProvider の SHORT_DESCRIPTION に対応する。
    pub description: String,
    /// extended_event_descriptor.text。TvProvider の LONG_DESCRIPTION の詳細本文に対応する。
    pub extended_description: String,
    pub short_events: Vec<ShortEventText>,
    pub extended_texts: Vec<ExtendedEventText>,
''' + "'''" + r''', 'multilingual descriptor fields')

replace_once(path,
''' + "'''" + r'''#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ExtendedEventItem {
    pub language_code: String,
    pub item_description: String,
    pub item_text: String,
}
''' + "'''" + r''',
''' + "'''" + r'''#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ExtendedEventItem {
    pub language_code: String,
    pub item_description: String,
    pub item_text: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ShortEventText {
    pub language_code: String,
    pub title: String,
    pub text: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ExtendedEventText {
    pub language_code: String,
    pub text: String,
}
''' + "'''" + r''', 'multilingual descriptor models')

'''
runtime = Path('/tmp/codex_apply_pr54_projection_gaps_runtime.py')
runtime.write_text(src[:start] + replacement + src[end:], encoding='utf-8')
runpy.run_path(str(runtime), run_name='__main__')

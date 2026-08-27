from pathlib import Path
import re
import sys


def replace_once(text: str, old: str, new: str, label: str) -> str:
    if old not in text:
        raise SystemExit(f"{label} not found")
    return text.replace(old, new, 1)


def fix_tuner() -> None:
    p = Path("tuner_hal2/common/src/lib.rs")
    s = p.read_text()
    old = '''#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct TransportStreamPid(u16);

impl TransportStreamPid {
    pub fn validate_u16(pid: u16) -> Result<Self, ()> {
        if pid <= 0x1fff {
            Ok(Self(pid))
        } else {
            Err(())
        }
    }

    pub fn validate_i32(pid: i32) -> Result<Self, ()> {
        if (0..=0x1fff).contains(&pid) {
            Ok(Self(pid as u16))
        } else {
            Err(())
        }
    }
'''
    new = '''#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct TransportStreamPid(u16);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransportStreamPidValidationError {
    OutOfRange,
}

impl TransportStreamPid {
    pub fn validate_u16(pid: u16) -> Result<Self, TransportStreamPidValidationError> {
        if pid <= 0x1fff {
            Ok(Self(pid))
        } else {
            Err(TransportStreamPidValidationError::OutOfRange)
        }
    }

    pub fn validate_i32(pid: i32) -> Result<Self, TransportStreamPidValidationError> {
        if (0..=0x1fff).contains(&pid) {
            Ok(Self(pid as u16))
        } else {
            Err(TransportStreamPidValidationError::OutOfRange)
        }
    }
'''
    s = replace_once(s, old, new, "TransportStreamPid block")
    p.write_text(s)

    p = Path("tuner_hal2/resource_ledger/src/lib.rs")
    s = p.read_text()
    impl_start = s.index("impl<K: LedgerResourceKind> TypedResourceLedger<K>")
    macro_start = s.index("macro_rules! define_ledger_wrapper", impl_start)
    before = s[:macro_start]
    if "pub fn is_empty(&self) -> bool" not in before[impl_start:]:
        old = "    pub fn len(&self) -> usize {\n        self.inner.len()\n    }\n}\n\n"
        new = "    pub fn len(&self) -> usize {\n        self.inner.len()\n    }\n\n    pub fn is_empty(&self) -> bool {\n        self.inner.is_empty()\n    }\n}\n\n"
        before = replace_once(before, old, new, "typed ledger len")
        s = before + s[macro_start:]
    macro_start = s.index("macro_rules! define_ledger_wrapper")
    macro_end = s.index("define_ledger_wrapper!(FrontendLedger", macro_start)
    macro = s[macro_start:macro_end]
    if "pub fn is_empty(&self) -> bool" not in macro:
        old = "            pub fn len(&self) -> usize {\n                self.inner.len()\n            }\n"
        new = old + "            pub fn is_empty(&self) -> bool {\n                self.inner.is_empty()\n            }\n"
        macro = replace_once(macro, old, new, "wrapper ledger len")
        s = s[:macro_start] + macro + s[macro_end:]
    p.write_text(s)

    p = Path("tuner_hal2/device/src/runtime/frontend_worker.rs")
    s = p.read_text()
    old = '''        let result: Arc<Mutex<Option<Result<(Result<(), HalError>, WorkerExit), HalError>>>> =
            Arc::new(Mutex::new(None));
'''
    new = '''        type WorkerThreadResult =
            Arc<Mutex<Option<Result<(Result<(), HalError>, WorkerExit), HalError>>>>;
        let result: WorkerThreadResult = Arc::new(Mutex::new(None));
'''
    s = replace_once(s, old, new, "frontend worker result type")
    p.write_text(s)

    p = Path("tuner_hal2/descrambler/src/core/packet.rs")
    s = p.read_text()
    old = '''        for i in 4..TS_PACKET_SIZE {
            p[i] = (i as u8).wrapping_mul(3).wrapping_add(1);
        }
'''
    new = '''        for (i, byte) in p.iter_mut().enumerate().skip(4) {
            *byte = (i as u8).wrapping_mul(3).wrapping_add(1);
        }
'''
    s = replace_once(s, old, new, "packet test initialization")
    p.write_text(s)


def fix_arib() -> None:
    p = Path("arib_si_engine_rs/src/lib.rs")
    s = p.read_text().replace("mod arib_jis_x0208_table;\n", "", 1)
    s = replace_once(
        s,
        "    fn services(&self) -> Vec<DiscoveredService> {\n        self.snapshot().services\n    }\n",
        "    #[cfg(test)]\n    fn services(&self) -> Vec<DiscoveredService> {\n        self.snapshot().services\n    }\n",
        "ParserState services",
    )
    s = s.replace("\n    fn transports(&self) -> Vec<DiscoveredTransport> {\n        self.snapshot().transports\n    }\n", "", 1)
    s = s.replace("\n    fn clear_epg_update_windows(&mut self) {\n        self.collector.clear_epg_update_windows()\n    }\n", "", 1)
    p.write_text(s)

    p = Path("arib_si_engine_rs/src/arib_string.rs")
    s = p.read_text()
    if "use std::fmt::Write as _;" not in s:
        s = "use std::fmt::Write as _;\n\n" + s
    marker = 'pub const ARIB_STRING_DECODER_SCOPE: &str = "mirakc_scope_non_caption_si_epg_only";\n'
    helper = '''\nfn bytes_hex_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut out, "{byte:02x}").expect("writing to String cannot fail");
    }
    out
}
'''
    s = replace_once(s, marker, marker + helper, "ARIB hex helper marker")
    old = '''            input_prefix_hex: bytes
                .iter()
                .skip(offset)
                .take(8)
                .map(|byte| format!("{:02x}", byte))
                .collect(),
'''
    new = '''            input_prefix_hex: {
                let remaining = bytes.get(offset..).unwrap_or_default();
                bytes_hex_lower(&remaining[..remaining.len().min(8)])
            },
'''
    s = replace_once(s, old, new, "ARIB diagnostic hex")
    s = s.replace("    Macro,\n", "", 1)
    s = s.replace('        GraphicSet::Macro => "�".to_string(),\n', "", 1)
    s = s.replace("        GraphicSet::Macro => Err(AribStringDecodeError::UnsupportedEscape),\n", "", 1)
    s = s.replace("                    GraphicSet::Macro => return Err(AribStringDecodeError::UnsupportedEscape),\n", "", 1)
    s = re.sub(r'''\n\s*if matches!\(set, GraphicSet::Macro\) \{\n.*?"SS3/Macro".*?\n\s*\}\n''', "\n", s, count=1, flags=re.S)
    macro_branch = re.compile(r'''\n\s*GraphicSet::Macro => \{\n.*?"unsupported_macro".*?out\.push\('�'\);\n\s*\},?''', re.S)
    s, count = macro_branch.subn("", s)
    if count != 2:
        raise SystemExit(f"expected 2 lossy Macro branches, removed {count}")
    s = replace_once(
        s,
        "pub fn decode_arib_string(bytes: &[u8]) -> Result<String, AribStringDecodeError> {\n",
        "#[cfg(test)]\npub fn decode_arib_string(bytes: &[u8]) -> Result<String, AribStringDecodeError> {\n",
        "strict decoder cfg(test)",
    )
    p.write_text(s)

    p = Path("arib_si_engine_rs/src/ca_descriptor.rs")
    s = p.read_text()
    s = replace_once(
        s,
        "pub fn parse_ca_descriptors(descriptors: &[u8]) -> Vec<CaDescriptor> {\n",
        "#[cfg(test)]\npub fn parse_ca_descriptors(descriptors: &[u8]) -> Vec<CaDescriptor> {\n",
        "CA parser cfg(test)",
    )
    p.write_text(s)

    p = Path("arib_si_engine_rs/src/eit.rs")
    s = p.read_text()
    s = replace_once(
        s,
        "    pub fn snapshot_present_following_actual(&self) -> Vec<EitEvent> {\n",
        "    #[cfg(test)]\n    pub fn snapshot_present_following_actual(&self) -> Vec<EitEvent> {\n",
        "EIT p/f cfg(test)",
    )
    s = s.replace("\n    pub fn clear_update_windows(&mut self) {\n        self.last_update_windows.clear();\n    }\n", "", 1)
    s = replace_once(
        s,
        "    pub fn section_count_for_diagnostic(&self) -> usize {\n",
        "    #[cfg(test)]\n    pub fn section_count_for_diagnostic(&self) -> usize {\n",
        "EIT count cfg(test)",
    )
    p.write_text(s)

    p = Path("arib_si_engine_rs/src/descriptors.rs")
    s = p.read_text()
    s = s.replace("        let Some(desc_end) = desc_start.checked_add(desc_len) else {\n            return None;\n        };\n", "        let desc_end = desc_start.checked_add(desc_len)?;\n", 1)
    s = s.replace("        let Some(item_end) = item_start.checked_add(item_len) else {\n            return None;\n        };\n", "        let item_end = item_start.checked_add(item_len)?;\n", 1)
    s = s.replace("    let Some(text_end) = text_start.checked_add(text_len) else {\n        return None;\n    };\n", "    let text_end = text_start.checked_add(text_len)?;\n", 1)
    old = '''fn decode_descriptor_text_lossy(
    bytes: &[u8],
    out: &mut EventDescriptors,
    tag: u8,
    descriptor_offset: usize,
    declared_length: usize,
    descriptor_body: &[u8],
    field_kind: &str,
    field_offset: usize,
) -> String {
'''
    new = '''type DescriptorTextField<'a> = (&'a str, usize);

fn decode_descriptor_text_lossy(
    bytes: &[u8],
    out: &mut EventDescriptors,
    tag: u8,
    descriptor_offset: usize,
    declared_length: usize,
    descriptor_body: &[u8],
    field: DescriptorTextField<'_>,
) -> String {
    let (field_kind, field_offset) = field;
'''
    s = replace_once(s, old, new, "descriptor text signature")
    pairs = [
        ('        body,\n        "eventName",\n        offset.saturating_add(2).saturating_add(name_start),\n', '        body,\n        ("eventName", offset.saturating_add(2).saturating_add(name_start)),\n'),
        ('        body,\n        "text",\n        offset.saturating_add(2).saturating_add(text_start),\n', '        body,\n        ("text", offset.saturating_add(2).saturating_add(text_start)),\n'),
        ('            body,\n            "text",\n            offset.saturating_add(8),\n', '            body,\n            ("text", offset.saturating_add(8)),\n'),
        ('            body,\n            "text",\n            offset.saturating_add(2).saturating_add(cursor),\n', '            body,\n            ("text", offset.saturating_add(2).saturating_add(cursor)),\n'),
        ('                body,\n                "seriesName",\n                offset.saturating_add(11),\n', '                body,\n                ("seriesName", offset.saturating_add(11)),\n'),
    ]
    for i, (old_call, new_call) in enumerate(pairs):
        s = replace_once(s, old_call, new_call, f"descriptor call {i}")
    s = re.sub(r'''\n#\[cfg\(test\)\]\npub fn event_descriptor_diagnostics_array_json\(desc: &EventDescriptors\) -> String \{\n    event_descriptor_diagnostics_array_json_scoped\(desc, None\)\n\}\n''', "\n", s, count=1)
    for name in [
        "descriptor_diagnostic_to_json_scoped",
        "event_group_to_json",
        "event_group_reference_to_json",
        "other_network_event_group_reference_to_json",
        "additive_checksum",
    ]:
        marker = f"fn {name}("
        s = replace_once(s, marker, f"#[cfg(test)]\n{marker}", f"cfg(test) {name}")
    p.write_text(s)

    p = Path("arib_si_engine_rs/src/provider_data.rs")
    s = p.read_text()
    s = replace_once(
        s,
        "pub fn extract_program_key(raw_bytes: &[u8]) -> Option<String> {\n",
        "#[cfg(test)]\npub fn extract_program_key(raw_bytes: &[u8]) -> Option<String> {\n",
        "program key cfg(test)",
    )
    old = '''    let hex = |value: &str| {
        value
            .as_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };
    [
        canonical
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>(),
'''
    new = '''    let hex = |value: &[u8]| {
        use std::fmt::Write as _;
        let mut out = String::with_capacity(value.len() * 2);
        for byte in value {
            write!(&mut out, "{byte:02x}").expect("writing to String cannot fail");
        }
        out
    };
    [
        hex(&canonical),
'''
    s = replace_once(s, old, new, "provider hex")
    s = s.replace("        hex(&data.tune.delivery_system),\n", "        hex(data.tune.delivery_system.as_bytes()),\n", 1)
    s = s.replace("            .map(hex)\n", "            .map(|value| hex(value.as_bytes()))\n", 1)
    a = s.find("#[cfg(test)]\nfn sha256_hex(data: &[u8]) -> String {")
    b = s.find("#[cfg(test)]\nmod provider_data_tests", a)
    if a < 0 or b < 0:
        raise SystemExit("unused SHA block markers missing")
    s = s[:a] + s[b:]
    p.write_text(s)

    p = Path("arib_si_engine_rs/src/service_discovery.rs")
    s = p.read_text()
    s = replace_once(
        s,
        "impl ServiceSemanticFacts {\n    pub fn cas_signaling_facts(&self) -> CasSignalingFacts {\n",
        "impl ServiceSemanticFacts {\n    #[cfg(test)]\n    pub fn cas_signaling_facts(&self) -> CasSignalingFacts {\n",
        "CAS facts cfg(test)",
    )
    for name in ["partial_snapshot", "best_available_snapshot", "complete_snapshot"]:
        marker = f"    pub fn {name}"
        s = replace_once(s, marker, f"    #[cfg(test)]\n{marker}", f"cfg(test) {name}")
    s = s.replace("\n    pub fn clear_epg_update_windows(&mut self) {\n        self.eit_store.clear_update_windows();\n    }\n", "", 1)
    s = s.replace("\n    fn note_eit_diagnostic(&mut self, section: &[u8]) {\n        let _ = section;\n    }\n", "", 1)
    s = s.replace("\n    pub fn clear_epg_update_windows(&mut self) {\n        self.engine.clear_epg_update_windows()\n    }\n", "", 1)
    marker = "    pub fn is_complete(&self) -> bool {\n        self.state().is_complete()\n    }\n"
    s = replace_once(s, marker, "    #[cfg(test)]\n" + marker, "collector is_complete cfg(test)")
    p.write_text(s)


if __name__ == "__main__":
    if len(sys.argv) != 2 or sys.argv[1] not in {"tuner", "arib"}:
        raise SystemExit("usage: fix_clippy_pr53_pr54.py tuner|arib")
    if sys.argv[1] == "tuner":
        fix_tuner()
    else:
        fix_arib()

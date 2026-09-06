include!("arib_jis_x0208_table.rs");
include!("arib_extended_graphic_table.rs");
include!("arib_jis_x0213_multiscalar.rs");

pub const ARIB_STRING_DECODER_SCOPE: &str = "mirakc_scope_non_caption_si_epg_only";

fn bytes_hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AribStringDecodeError {
    TruncatedEscape,
    TruncatedGraphic,
    UnsupportedEscape,
    UnsupportedControl,
    MalformedCsi,
    MiddleSizeGraphic,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ErrorPolicy {
    Strict,
    Replace,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AribStringDecodeDiagnostic {
    pub replacement_count: usize,
    pub unsupported_escape_count: usize,
    pub truncated_escape_count: usize,
    pub truncated_graphic_count: usize,
    pub entries: Vec<AribStringDecodeDiagnosticEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AribStringDecodeDiagnosticEntry {
    pub offset: usize,
    pub input_prefix_hex: String,
    pub code_set_or_control: String,
    pub reason: String,
    pub replacement_emitted: bool,
}

impl AribStringDecodeDiagnostic {
    fn record_entry(
        &mut self,
        bytes: &[u8],
        offset: usize,
        code_set_or_control: &str,
        reason: &str,
        replacement_emitted: bool,
    ) {
        self.entries.push(AribStringDecodeDiagnosticEntry {
            offset,
            input_prefix_hex: {
                let remaining = bytes.get(offset..).unwrap_or_default();
                bytes_hex_lower(&remaining[..remaining.len().min(8)])
            },
            code_set_or_control: code_set_or_control.to_string(),
            reason: reason.to_string(),
            replacement_emitted,
        });
    }

    pub fn summary(&self) -> String {
        let entries = self
            .entries
            .iter()
            .map(|entry| {
                format!(
                    "{{offset:{} input_prefix_hex:{} code:{} reason:{} replacement:{}}}",
                    entry.offset,
                    entry.input_prefix_hex,
                    entry.code_set_or_control,
                    entry.reason,
                    entry.replacement_emitted
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "scope={} replacement_count={} unsupported_escape_count={} truncated_escape_count={} truncated_graphic_count={} entries=[{}]",
            ARIB_STRING_DECODER_SCOPE,
            self.replacement_count,
            self.unsupported_escape_count,
            self.truncated_escape_count,
            self.truncated_graphic_count,
            entries
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GraphicSet {
    Alnum,
    Hiragana,
    Katakana,
    Kanji,
    JisPlane1,
    JisPlane2,
    AdditionalSymbols,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct InvocationState {
    g0: GraphicSet,
    g1: GraphicSet,
    g2: GraphicSet,
    g3: GraphicSet,
    gl: GraphicSet,
    gr: GraphicSet,
    middle_size: bool,
}

impl Default for InvocationState {
    fn default() -> Self {
        // ARIB TR-B15の衛星SI運用profileに合わせ、G0/GLはJIS互換漢字Plane 1を正本とする。
        // G1は英数字、G2はひらがな、G3はカタカナ、GRはLS2R(G2)。
        Self {
            g0: GraphicSet::JisPlane1,
            g1: GraphicSet::Alnum,
            g2: GraphicSet::Hiragana,
            g3: GraphicSet::Katakana,
            gl: GraphicSet::JisPlane1,
            gr: GraphicSet::Hiragana,
            middle_size: false,
        }
    }
}

fn decode_single_shift(
    set: GraphicSet,
    bytes: &[u8],
    index: usize,
) -> Result<(String, usize), AribStringDecodeError> {
    let first = *bytes
        .get(index + 1)
        .ok_or(AribStringDecodeError::TruncatedGraphic)?;
    let value = match set {
        GraphicSet::Alnum => (first as char).to_string(),
        GraphicSet::Hiragana => map_hiragana(first).to_string(),
        GraphicSet::Katakana => map_katakana(first).to_string(),
        GraphicSet::Kanji
        | GraphicSet::JisPlane1
        | GraphicSet::JisPlane2
        | GraphicSet::AdditionalSymbols => {
            let second = *bytes
                .get(index + 2)
                .ok_or(AribStringDecodeError::TruncatedGraphic)?;
            map_two_byte_graphic(set, first, second).to_string()
        }
    };
    let consumed_after_control = if is_two_byte_graphic(set) { 2 } else { 1 };
    Ok((value, consumed_after_control))
}

fn consume_csi(bytes: &[u8]) -> Result<usize, AribStringDecodeError> {
    if bytes.first() != Some(&0x9b) {
        return Err(AribStringDecodeError::MalformedCsi);
    }
    for (index, byte) in bytes.iter().copied().enumerate().skip(1) {
        if (0x40..=0x7e).contains(&byte) {
            return Ok(index + 1);
        }
        if !(0x20..=0x3f).contains(&byte) {
            return Err(AribStringDecodeError::MalformedCsi);
        }
    }
    Err(AribStringDecodeError::MalformedCsi)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum XcsMarker {
    Start,
    End,
}

fn csi_xcs_marker(bytes: &[u8]) -> Result<(usize, Option<XcsMarker>), AribStringDecodeError> {
    let consumed = consume_csi(bytes)?;
    let marker = if consumed == 4 && bytes.get(2) == Some(&0x20) && bytes.get(3) == Some(&b'f') {
        match bytes.get(1) {
            Some(b'0') => Some(XcsMarker::Start),
            Some(b'1') => Some(XcsMarker::End),
            _ => None,
        }
    } else {
        None
    };
    Ok((consumed, marker))
}

fn consume_xcs_block(bytes: &[u8]) -> Result<Option<(usize, usize, usize)>, AribStringDecodeError> {
    let (start_len, marker) = csi_xcs_marker(bytes)?;
    if marker != Some(XcsMarker::Start) {
        return Ok(None);
    }
    let mut cursor = start_len;
    while cursor < bytes.len() {
        if bytes[cursor] != 0x9b {
            cursor += 1;
            continue;
        }
        let (consumed, nested_marker) = csi_xcs_marker(&bytes[cursor..])?;
        match nested_marker {
            Some(XcsMarker::End) => return Ok(Some((start_len, cursor, cursor + consumed))),
            Some(XcsMarker::Start) => return Err(AribStringDecodeError::MalformedCsi),
            None => cursor += consumed,
        }
    }
    Err(AribStringDecodeError::MalformedCsi)
}

#[derive(Default)]
pub(crate) struct AribStringDecoder {
    state: InvocationState,
}

impl AribStringDecoder {
    pub(crate) fn decode(&mut self, bytes: &[u8]) -> Result<String, AribStringDecodeError> {
        let mut next_state = self.state;
        let (decoded, _) =
            decode_arib_string_with_policy(bytes, &mut next_state, ErrorPolicy::Strict)?;
        self.state = next_state;
        Ok(decoded)
    }

    pub(crate) fn lossy_diagnostic(&self, bytes: &[u8]) -> AribStringDecodeDiagnostic {
        let mut state = self.state;
        decode_arib_string_replacing(bytes, &mut state).1
    }
}

/// 字幕以外の ARIB SI/EPG 文字列を復号する。
/// サービス名、番組名、短形式イベント、長形式イベント用であり、字幕描画器ではない。
#[cfg(test)]
pub fn decode_arib_string(bytes: &[u8]) -> Result<String, AribStringDecodeError> {
    AribStringDecoder::default().decode(bytes)
}

/// 字幕以外の SI/EPG 文字列向けの劣化許容復号を行う。
/// 不正な放送データで設定処理や EPG スキャンを停止させないため、未対応バイトは置換文字にする。
pub fn decode_arib_string_lossy(bytes: &[u8]) -> (String, AribStringDecodeDiagnostic) {
    let mut state = InvocationState::default();
    decode_arib_string_replacing(bytes, &mut state)
}

fn decode_arib_string_replacing(
    bytes: &[u8],
    state: &mut InvocationState,
) -> (String, AribStringDecodeDiagnostic) {
    match decode_arib_string_with_policy(bytes, state, ErrorPolicy::Replace) {
        Ok(result) => result,
        Err(error) => {
            let mut diagnostic = AribStringDecodeDiagnostic {
                replacement_count: 1,
                ..Default::default()
            };
            diagnostic.record_entry(
                bytes,
                0,
                "decoder",
                &format!("replacement_policy_failure:{error:?}"),
                true,
            );
            ("�".to_string(), diagnostic)
        }
    }
}

fn decode_arib_string_with_policy(
    bytes: &[u8],
    state: &mut InvocationState,
    error_policy: ErrorPolicy,
) -> Result<(String, AribStringDecodeDiagnostic), AribStringDecodeError> {
    let mut out = String::new();
    let mut diagnostic = AribStringDecodeDiagnostic::default();
    let mut index = 0usize;
    let mut pending_xcs_at: Option<usize> = None;
    while index < bytes.len() {
        if pending_xcs_at.is_some() && pending_xcs_at != Some(index) {
            pending_xcs_at = None;
        }
        let byte = bytes[index];
        match byte {
            0x00 => {}
            0x0d => out.push('\n'),      // APR
            0x0e => state.gl = state.g1, // LS1
            0x0f => state.gl = state.g0, // LS0
            0x19 | 0x1d => {
                let set = if byte == 0x19 { state.g2 } else { state.g3 };
                if state.middle_size && set != GraphicSet::Alnum {
                    if error_policy == ErrorPolicy::Strict {
                        return Err(AribStringDecodeError::MiddleSizeGraphic);
                    }
                    diagnostic.replacement_count = diagnostic.replacement_count.saturating_add(1);
                    diagnostic.record_entry(
                        bytes,
                        index,
                        "MSZ/single_shift",
                        "middle_size_non_alphanumeric",
                        true,
                    );
                    out.push('�');
                    index += if is_two_byte_graphic(set) { 3 } else { 2 };
                    continue;
                }
                match decode_single_shift(set, bytes, index) {
                    Ok((value, consumed)) => {
                        out.push_str(&value);
                        pending_xcs_at = (value == "�").then_some(index + consumed + 1);
                        index += consumed;
                    }
                    Err(AribStringDecodeError::TruncatedGraphic) => {
                        if error_policy == ErrorPolicy::Strict {
                            return Err(AribStringDecodeError::TruncatedGraphic);
                        }
                        diagnostic.truncated_graphic_count =
                            diagnostic.truncated_graphic_count.saturating_add(1);
                        diagnostic.replacement_count =
                            diagnostic.replacement_count.saturating_add(1);
                        diagnostic.record_entry(bytes, index, "graphic", "truncated_graphic", true);
                        out.push('�');
                        break;
                    }
                    Err(error) => {
                        if error_policy == ErrorPolicy::Strict {
                            return Err(error);
                        }
                        diagnostic.replacement_count =
                            diagnostic.replacement_count.saturating_add(1);
                        diagnostic.record_entry(
                            bytes,
                            index,
                            "single_shift",
                            "unsupported_or_truncated_single_shift",
                            true,
                        );
                        out.push('�');
                        break;
                    }
                }
            }
            0x1b => match apply_escape(state, &bytes[index..]) {
                Ok(consumed) => index += consumed.saturating_sub(1),
                Err(AribStringDecodeError::TruncatedEscape) => {
                    if error_policy == ErrorPolicy::Strict {
                        return Err(AribStringDecodeError::TruncatedEscape);
                    }
                    diagnostic.truncated_escape_count =
                        diagnostic.truncated_escape_count.saturating_add(1);
                    diagnostic.replacement_count = diagnostic.replacement_count.saturating_add(1);
                    diagnostic.record_entry(bytes, index, "ESC", "truncated_escape", true);
                    out.push('�');
                    break;
                }
                Err(AribStringDecodeError::TruncatedGraphic) => {
                    if error_policy == ErrorPolicy::Strict {
                        return Err(AribStringDecodeError::TruncatedGraphic);
                    }
                    diagnostic.truncated_graphic_count =
                        diagnostic.truncated_graphic_count.saturating_add(1);
                    diagnostic.replacement_count = diagnostic.replacement_count.saturating_add(1);
                    diagnostic.record_entry(bytes, index, "ESC/graphic", "truncated_graphic", true);
                    out.push('�');
                    break;
                }
                Err(AribStringDecodeError::UnsupportedEscape) => {
                    if error_policy == ErrorPolicy::Strict {
                        return Err(AribStringDecodeError::UnsupportedEscape);
                    }
                    diagnostic.unsupported_escape_count =
                        diagnostic.unsupported_escape_count.saturating_add(1);
                    diagnostic.replacement_count = diagnostic.replacement_count.saturating_add(1);
                    diagnostic.record_entry(bytes, index, "ESC", "unsupported_escape", true);
                    out.push('�');
                    index += unsupported_escape_sequence_length(&bytes[index..]).saturating_sub(1);
                }
                Err(error) => {
                    if error_policy == ErrorPolicy::Strict {
                        return Err(error);
                    }
                    diagnostic.unsupported_escape_count =
                        diagnostic.unsupported_escape_count.saturating_add(1);
                    diagnostic.replacement_count = diagnostic.replacement_count.saturating_add(1);
                    diagnostic.record_entry(bytes, index, "ESC", "unsupported_escape", true);
                    out.push('�');
                    index += unsupported_escape_sequence_length(&bytes[index..]).saturating_sub(1);
                }
            },
            0x20 => out.push(' '),
            0x89 => state.middle_size = true,
            0x8a => state.middle_size = false,
            0x9b => match consume_xcs_block(&bytes[index..]) {
                Ok(Some((content_start, content_end, consumed))) => {
                    if pending_xcs_at == Some(index) {
                        if out.ends_with('�') {
                            out.pop();
                        }
                        let mut fallback_state = *state;
                        let (fallback, fallback_diagnostic) = decode_arib_string_with_policy(
                            &bytes[index + content_start..index + content_end],
                            &mut fallback_state,
                            error_policy,
                        )?;
                        *state = fallback_state;
                        out.push_str(&fallback);
                        diagnostic.replacement_count = diagnostic
                            .replacement_count
                            .saturating_add(fallback_diagnostic.replacement_count);
                        diagnostic.unsupported_escape_count = diagnostic
                            .unsupported_escape_count
                            .saturating_add(fallback_diagnostic.unsupported_escape_count);
                        diagnostic.truncated_escape_count = diagnostic
                            .truncated_escape_count
                            .saturating_add(fallback_diagnostic.truncated_escape_count);
                        diagnostic.truncated_graphic_count = diagnostic
                            .truncated_graphic_count
                            .saturating_add(fallback_diagnostic.truncated_graphic_count);
                        diagnostic.entries.extend(fallback_diagnostic.entries);
                    }
                    pending_xcs_at = None;
                    index += consumed.saturating_sub(1);
                }
                Ok(None) => match consume_csi(&bytes[index..]) {
                    Ok(consumed) => {
                        pending_xcs_at = None;
                        index += consumed.saturating_sub(1);
                    }
                    Err(error) => {
                        if error_policy == ErrorPolicy::Strict {
                            return Err(error);
                        }
                        diagnostic.replacement_count =
                            diagnostic.replacement_count.saturating_add(1);
                        diagnostic.record_entry(
                            bytes,
                            index,
                            "CSI/XCS",
                            "malformed_or_truncated_csi",
                            true,
                        );
                        out.push('�');
                        break;
                    }
                },
                Err(error) => {
                    if error_policy == ErrorPolicy::Strict {
                        return Err(error);
                    }
                    diagnostic.replacement_count = diagnostic.replacement_count.saturating_add(1);
                    diagnostic.record_entry(
                        bytes,
                        index,
                        "CSI/XCS",
                        "malformed_or_truncated_csi",
                        true,
                    );
                    out.push('�');
                    break;
                }
            },
            0x21..=0x7e if state.middle_size && state.gl != GraphicSet::Alnum => {
                if error_policy == ErrorPolicy::Strict {
                    return Err(AribStringDecodeError::MiddleSizeGraphic);
                }
                diagnostic.replacement_count = diagnostic.replacement_count.saturating_add(1);
                diagnostic.record_entry(
                    bytes,
                    index,
                    "MSZ/GL",
                    "middle_size_non_alphanumeric",
                    true,
                );
                out.push('�');
                if is_two_byte_graphic(state.gl) && bytes.get(index + 1).is_some() {
                    index += 1;
                }
            }
            0x21..=0x7e => match state.gl {
                GraphicSet::Alnum => out.push(byte as char),
                GraphicSet::Hiragana => out.push_str(map_hiragana(byte)),
                GraphicSet::Katakana => out.push_str(map_katakana(byte)),
                GraphicSet::Kanji
                | GraphicSet::JisPlane1
                | GraphicSet::JisPlane2
                | GraphicSet::AdditionalSymbols => {
                    let Some(next) = bytes.get(index + 1).copied() else {
                        if error_policy == ErrorPolicy::Strict {
                            return Err(AribStringDecodeError::TruncatedGraphic);
                        }
                        diagnostic.truncated_graphic_count =
                            diagnostic.truncated_graphic_count.saturating_add(1);
                        diagnostic.replacement_count =
                            diagnostic.replacement_count.saturating_add(1);
                        diagnostic.record_entry(
                            bytes,
                            index,
                            "GL/Kanji",
                            "truncated_graphic",
                            true,
                        );
                        out.push('�');
                        break;
                    };
                    if !(0x21..=0x7e).contains(&next) {
                        if error_policy == ErrorPolicy::Strict {
                            return Err(AribStringDecodeError::UnsupportedControl);
                        }
                        diagnostic.replacement_count =
                            diagnostic.replacement_count.saturating_add(1);
                        diagnostic.record_entry(bytes, index, "GL/Kanji", "不正な2バイト目", true);
                        out.push('�');
                    } else {
                        let mapped = map_two_byte_graphic(state.gl, byte, next);
                        out.push_str(mapped);
                        pending_xcs_at = (mapped == "�").then_some(index + 2);
                        index += 1;
                    }
                }
            },
            0xa1..=0xfe if state.middle_size && state.gr != GraphicSet::Alnum => {
                if error_policy == ErrorPolicy::Strict {
                    return Err(AribStringDecodeError::MiddleSizeGraphic);
                }
                diagnostic.replacement_count = diagnostic.replacement_count.saturating_add(1);
                diagnostic.record_entry(
                    bytes,
                    index,
                    "MSZ/GR",
                    "middle_size_non_alphanumeric",
                    true,
                );
                out.push('�');
                if is_two_byte_graphic(state.gr) && bytes.get(index + 1).is_some() {
                    index += 1;
                }
            }
            0xa1..=0xfe => {
                let normalized = byte & 0x7f;
                match state.gr {
                    GraphicSet::Alnum => out.push(normalized as char),
                    GraphicSet::Hiragana => out.push_str(map_hiragana(normalized)),
                    GraphicSet::Katakana => out.push_str(map_katakana(normalized)),
                    GraphicSet::Kanji
                    | GraphicSet::JisPlane1
                    | GraphicSet::JisPlane2
                    | GraphicSet::AdditionalSymbols => {
                        let Some(next) = bytes.get(index + 1).copied() else {
                            if error_policy == ErrorPolicy::Strict {
                                return Err(AribStringDecodeError::TruncatedGraphic);
                            }
                            diagnostic.truncated_graphic_count =
                                diagnostic.truncated_graphic_count.saturating_add(1);
                            diagnostic.replacement_count =
                                diagnostic.replacement_count.saturating_add(1);
                            diagnostic.record_entry(
                                bytes,
                                index,
                                "GR/Kanji",
                                "truncated_graphic",
                                true,
                            );
                            out.push('�');
                            break;
                        };
                        if !(0xa1..=0xfe).contains(&next) && !(0x21..=0x7e).contains(&next) {
                            if error_policy == ErrorPolicy::Strict {
                                return Err(AribStringDecodeError::UnsupportedControl);
                            }
                            diagnostic.replacement_count =
                                diagnostic.replacement_count.saturating_add(1);
                            diagnostic.record_entry(
                                bytes,
                                index,
                                "GR/Kanji",
                                "不正な2バイト目",
                                true,
                            );
                            out.push('�');
                        } else {
                            let mapped = map_two_byte_graphic(state.gr, normalized, next & 0x7f);
                            out.push_str(mapped);
                            pending_xcs_at = (mapped == "�").then_some(index + 2);
                            index += 1;
                        }
                    }
                }
            }
            _ => {
                if error_policy == ErrorPolicy::Strict {
                    return Err(AribStringDecodeError::UnsupportedControl);
                }
                diagnostic.replacement_count = diagnostic.replacement_count.saturating_add(1);
                diagnostic.record_entry(
                    bytes,
                    index,
                    "control_or_private",
                    "unsupported_byte",
                    true,
                );
                out.push('�');
            }
        }
        index += 1;
    }
    Ok((out.trim_matches('\0').to_string(), diagnostic))
}

fn unsupported_escape_sequence_length(bytes: &[u8]) -> usize {
    match bytes.get(1).copied() {
        Some(b'$') if matches!(bytes.get(2), Some(b'(' | b')' | b'*' | b'+')) => bytes.len().min(4),
        Some(b'$' | b'(' | b')' | b'*' | b'+') => bytes.len().min(3),
        Some(_) => bytes.len().min(2),
        None => 1,
    }
}

fn apply_escape(state: &mut InvocationState, bytes: &[u8]) -> Result<usize, AribStringDecodeError> {
    if bytes.len() < 2 {
        return Err(AribStringDecodeError::TruncatedEscape);
    }
    let consumed = match bytes[1] {
        b'n' => {
            state.gl = state.g2;
            2
        } // LS2
        b'o' => {
            state.gl = state.g3;
            2
        } // LS3
        b'~' => {
            state.gr = state.g1;
            2
        } // LS1R
        b'}' => {
            state.gr = state.g2;
            2
        } // LS2R
        b'|' => {
            state.gr = state.g3;
            2
        } // LS3R
        _ => {
            if bytes.len() < 3 {
                return Err(AribStringDecodeError::TruncatedEscape);
            }
            if bytes[1] == b'$' && matches!(bytes[2], b'(' | b')' | b'*' | b'+') && bytes.len() < 4
            {
                return Err(AribStringDecodeError::TruncatedEscape);
            }
            match (bytes[1], bytes[2]) {
                (b'(', b'B' | b'J') => {
                    state.g0 = GraphicSet::Alnum;
                    state.gl = state.g0;
                    3
                }
                (b'(', b'I') => {
                    state.g0 = GraphicSet::Katakana;
                    state.gl = state.g0;
                    3
                }
                (b'(', b'0') => {
                    state.g0 = GraphicSet::Hiragana;
                    state.gl = state.g0;
                    3
                }
                (b')', b'B' | b'J') => {
                    state.g1 = GraphicSet::Alnum;
                    3
                }
                (b')', b'I') => {
                    state.g1 = GraphicSet::Katakana;
                    3
                }
                (b')', b'0') => {
                    state.g1 = GraphicSet::Hiragana;
                    3
                }
                (b'*', b'B' | b'J') => {
                    state.g2 = GraphicSet::Alnum;
                    3
                }
                (b'*', b'I') => {
                    state.g2 = GraphicSet::Katakana;
                    3
                }
                (b'*', b'0') => {
                    state.g2 = GraphicSet::Hiragana;
                    3
                }
                (b'+', b'B' | b'J') => {
                    state.g3 = GraphicSet::Alnum;
                    3
                }
                (b'+', b'I') => {
                    state.g3 = GraphicSet::Katakana;
                    3
                }
                (b'+', b'0') => {
                    state.g3 = GraphicSet::Hiragana;
                    3
                }
                (b'$', final_byte @ (b'B' | b'@' | b'9' | b':' | b';')) => {
                    state.g0 = two_byte_graphic_set(final_byte)?;
                    state.gl = state.g0;
                    3
                }
                (b'$', b'(')
                    if bytes.len() >= 4 && matches!(bytes[3], b'B' | b'@' | b'9' | b':' | b';') =>
                {
                    state.g0 = two_byte_graphic_set(bytes[3])?;
                    state.gl = state.g0;
                    4
                }
                (b'$', b')')
                    if bytes.len() >= 4 && matches!(bytes[3], b'B' | b'@' | b'9' | b':' | b';') =>
                {
                    state.g1 = two_byte_graphic_set(bytes[3])?;
                    4
                }
                (b'$', b'*')
                    if bytes.len() >= 4 && matches!(bytes[3], b'B' | b'@' | b'9' | b':' | b';') =>
                {
                    state.g2 = two_byte_graphic_set(bytes[3])?;
                    4
                }
                (b'$', b'+')
                    if bytes.len() >= 4 && matches!(bytes[3], b'B' | b'@' | b'9' | b':' | b';') =>
                {
                    state.g3 = two_byte_graphic_set(bytes[3])?;
                    4
                }
                _ => return Err(AribStringDecodeError::UnsupportedEscape),
            }
        }
    };
    Ok(consumed)
}

fn is_two_byte_graphic(set: GraphicSet) -> bool {
    matches!(
        set,
        GraphicSet::Kanji
            | GraphicSet::JisPlane1
            | GraphicSet::JisPlane2
            | GraphicSet::AdditionalSymbols
    )
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
        GraphicSet::JisPlane1 => map_jis_x0213_plane1_multiscalar(first, second)
            .unwrap_or_else(|| map_jis_x0213_plane1(first, second)),
        GraphicSet::JisPlane2 => map_jis_x0213_plane2_multiscalar(first, second)
            .unwrap_or_else(|| map_jis_x0213_plane2(first, second)),
        GraphicSet::AdditionalSymbols => map_arib_additional_symbol(first, second),
        _ => "�",
    }
}

fn map_hiragana(byte: u8) -> &'static str {
    const TABLE: &[&str] = &[
        "ぁ", "あ", "ぃ", "い", "ぅ", "う", "ぇ", "え", "ぉ", "お", "か", "が", "き", "ぎ", "く",
        "ぐ", "け", "げ", "こ", "ご", "さ", "ざ", "し", "じ", "す", "ず", "せ", "ぜ", "そ", "ぞ",
        "た", "だ", "ち", "ぢ", "っ", "つ", "づ", "て", "で", "と", "ど", "な", "に", "ぬ", "ね",
        "の", "は", "ば", "ぱ", "ひ", "び", "ぴ", "ふ", "ぶ", "ぷ", "へ", "べ", "ぺ", "ほ", "ぼ",
        "ぽ", "ま", "み", "む", "め", "も", "ゃ", "や", "ゅ", "ゆ", "ょ", "よ", "ら", "り", "る",
        "れ", "ろ", "ゎ", "わ", "ゐ", "ゑ", "を", "ん", "ゔ", "ゕ", "ゖ", "。", "「", "」", "、",
        "・", "ー", "ゝ", "ゞ",
    ];
    TABLE
        .get((byte.saturating_sub(0x21)) as usize)
        .copied()
        .unwrap_or("�")
}

fn map_katakana(byte: u8) -> &'static str {
    const TABLE: &[&str] = &[
        "ァ", "ア", "ィ", "イ", "ゥ", "ウ", "ェ", "エ", "ォ", "オ", "カ", "ガ", "キ", "ギ", "ク",
        "グ", "ケ", "ゲ", "コ", "ゴ", "サ", "ザ", "シ", "ジ", "ス", "ズ", "セ", "ゼ", "ソ", "ゾ",
        "タ", "ダ", "チ", "ヂ", "ッ", "ツ", "ヅ", "テ", "デ", "ト", "ド", "ナ", "ニ", "ヌ", "ネ",
        "ノ", "ハ", "バ", "パ", "ヒ", "ビ", "ピ", "フ", "ブ", "プ", "ヘ", "ベ", "ペ", "ホ", "ボ",
        "ポ", "マ", "ミ", "ム", "メ", "モ", "ャ", "ヤ", "ュ", "ユ", "ョ", "ヨ", "ラ", "リ", "ル",
        "レ", "ロ", "ヮ", "ワ", "ヰ", "ヱ", "ヲ", "ン", "ヴ", "ヵ", "ヶ", "。", "「", "」", "、",
        "・", "ー", "ヽ", "ヾ",
    ];
    TABLE
        .get((byte.saturating_sub(0x21)) as usize)
        .copied()
        .unwrap_or("�")
}

fn map_kanji(first: u8, second: u8) -> &'static str {
    map_jis_x0208(first, second)
}

#[cfg(test)]
mod tests {
    use super::{
        decode_arib_string, decode_arib_string_lossy, AribStringDecodeError,
        ARIB_STRING_DECODER_SCOPE,
    };

    #[test]
    fn arib_string_decodes_basic_katakana() {
        let bytes = [0x1b, b'(', b'I', 0x22, 0x24, 0x26];
        assert_eq!(decode_arib_string_lossy(&bytes).0, "アイウ");
    }

    #[test]
    fn arib_string_decodes_service_name_descriptor_payload() {
        let descriptor_body = [0x00, 0x05, 0x1b, b'(', b'I', 0x22, 0x24];
        let service_name = &descriptor_body[2..];
        assert_eq!(decode_arib_string_lossy(service_name).0, "アイ");
    }

    #[test]
    fn arib_string_accepts_jis_x0208_esc_dollar_paren_b() {
        let bytes = [0x1b, b'$', b'(', b'B', 0x46, 0x7c];
        let out = decode_arib_string_lossy(&bytes).0;
        assert!(!out.is_empty());
    }

    #[test]
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
        assert_eq!(decoded, "\u{3402}");
        assert_eq!(diagnostic.replacement_count, 0);
    }

    #[test]
    fn arib_string_decodes_basic_kanji() {
        let bytes = [0x1b, b'$', b'B', b'E', b'l', b'5', b'~', 0x1b, b'(', b'B'];
        assert_eq!(decode_arib_string_lossy(&bytes).0, "東京");
    }

    #[test]
    fn arib_string_decodes_mixed_katakana_kanji_service_name() {
        let bytes = [
            0x1b, b'$', b'B', b'#', b'N', b'#', b'H', b'#', b'K', b'A', b'm', b'9', b'g', 0x1b,
            b'(', b'B',
        ];
        assert_eq!(decode_arib_string_lossy(&bytes).0, "ＮＨＫ総合");
    }

    #[test]
    fn arib_string_does_not_fallback_to_utf8_lossy() {
        assert_eq!(decode_arib_string_lossy(&[0xff, 0xff]).0, "��");
    }

    #[test]
    fn arib_string_decoder_scope_excludes_caption_renderer_claim() {
        assert_eq!(
            ARIB_STRING_DECODER_SCOPE,
            "mirakc_scope_non_caption_si_epg_only"
        );
        assert_eq!(
            decode_arib_string_lossy(&[0x1b, b'(', b'B', b'E', b'P', b'G']).0,
            "EPG"
        );
    }

    #[test]
    fn strict_and_lossy_decoders_match_for_valid_si_inputs() {
        let valid_inputs: &[&[u8]] = &[
            b"El5~",
            &[0x1b, b'(', b'I', 0x22, 0x24, 0x26],
            &[0x1b, b'(', b'B', b'A', 0x19, 0x22, b'B'],
            &[0x1b, b'(', b'B', b'A', 0x9b, b'1', b';', b'2', b'X', b'B'],
        ];
        for bytes in valid_inputs {
            let strict = decode_arib_string(bytes).unwrap();
            let (lossy, diagnostic) = decode_arib_string_lossy(bytes);
            assert_eq!(lossy, strict);
            assert_eq!(diagnostic, Default::default());
        }
    }

    #[test]
    fn arib_string_lossy_preserves_prefix_on_truncated_escape() {
        assert_eq!(
            decode_arib_string_lossy(&[0x1b, b'(', b'B', b'A', 0x1b, b'(']).0,
            "A�"
        );
    }

    #[test]
    fn arib_string_lossy_preserves_prefix_on_truncated_kanji() {
        let bytes = [0x1b, b'(', b'B', b'A', 0x1b, b'$', b'B', b'E'];
        assert_eq!(decode_arib_string_lossy(&bytes).0, "A�");
    }
    #[test]
    fn arib_string_reports_non_caption_diagnostic_counts() {
        let (_decoded, diagnostic) = decode_arib_string_lossy(&[0x1b, b'$', b'X', b'A']);
        assert_eq!(diagnostic.unsupported_escape_count, 1);
        assert!(diagnostic.replacement_count >= 1);
        assert!(diagnostic.summary().contains(ARIB_STRING_DECODER_SCOPE));
    }

    #[test]
    fn arib_string_initial_state_decodes_kanji_without_escape() {
        let bytes = [b'E', b'l', b'5', b'~'];
        assert_eq!(decode_arib_string_lossy(&bytes).0, "東京");
    }

    #[test]
    fn arib_string_initial_gr_decodes_hiragana() {
        let bytes = [0xa2, 0xa4];
        assert_eq!(decode_arib_string_lossy(&bytes).0, "あい");
    }

    #[test]
    fn arib_string_ss2_decodes_one_hiragana_character() {
        let bytes = [0x1b, b'(', b'B', b'A', 0x19, 0x22, b'B'];
        assert_eq!(decode_arib_string_lossy(&bytes).0, "AあB");
    }

    #[test]
    fn initial_g3_single_shift_decodes_katakana_not_macro() {
        assert_eq!(decode_arib_string(&[0x1d, 0x22]).unwrap(), "ア");
    }

    #[test]
    fn apr_space_and_middle_size_transitions_follow_si_text_contract() {
        let normal = [
            0x1b, b'(', b'B', b'A', 0x20, b'B', 0x0d, 0x89, b'C', 0x8a, b'D',
        ];
        assert_eq!(decode_arib_string(&normal).unwrap(), "A B\nCD");

        let unsupported_middle_size = [0x1b, b'(', b'0', 0x89, 0x22, 0x8a, 0x24];
        assert_eq!(
            decode_arib_string(&unsupported_middle_size),
            Err(AribStringDecodeError::MiddleSizeGraphic)
        );
        let (lossy, diagnostic) = decode_arib_string_lossy(&unsupported_middle_size);
        assert_eq!(lossy, "�い");
        assert!(diagnostic
            .entries
            .iter()
            .any(|entry| entry.reason == "middle_size_non_alphanumeric"));
    }

    #[test]
    fn csi_xcs_is_consumed_without_output_or_invocation_state_change() {
        let bytes = [0x1b, b'(', b'B', b'A', 0x9b, b'1', b';', b'2', b'X', b'B'];
        assert_eq!(decode_arib_string(&bytes).unwrap(), "AB");

        let malformed = [0x1b, b'(', b'B', b'A', 0x9b, b'1'];
        assert_eq!(
            decode_arib_string(&malformed),
            Err(AribStringDecodeError::MalformedCsi)
        );
        let (lossy, diagnostic) = decode_arib_string_lossy(&malformed);
        assert_eq!(lossy, "A�");
        assert!(diagnostic.entries.iter().any(|entry| {
            entry.code_set_or_control == "CSI/XCS" && entry.reason == "malformed_or_truncated_csi"
        }));
    }

    #[test]
    fn arib_string_diagnostic_entries_include_offset_reason_and_replacement_flag() {
        let (_decoded, diagnostic) = decode_arib_string_lossy(&[0x1b, b'$', b'X']);
        assert_eq!(diagnostic.entries.len(), 1);
        assert_eq!(diagnostic.entries[0].offset, 0);
        assert_eq!(diagnostic.entries[0].input_prefix_hex, "1b2458");
        assert_eq!(diagnostic.entries[0].code_set_or_control, "ESC");
        assert_eq!(diagnostic.entries[0].reason, "unsupported_escape");
        assert!(diagnostic.entries[0].replacement_emitted);
    }

    #[test]
    fn initial_si_graphic_set_is_jis_x0213_plane1_and_preserves_multiscalar() {
        assert_eq!(decode_arib_string(&[0x24, 0x77]), Ok("か゚".to_string()));
    }

    #[test]
    fn designated_jis_x0213_plane2_decodes_non_bmp_scalar() {
        let bytes = [0x1b, b'$', b'(', b':', 0x21, 0x21];
        assert_eq!(decode_arib_string(&bytes), Ok("𠂉".to_string()));
    }

    #[test]
    fn xcs_selects_alternative_only_for_unrenderable_source_graphic() {
        let unsupported_with_fallback = [
            0x24, 0x7c, 0x9b, b'0', 0x20, b'f', 0x19, 0x22, 0x9b, b'1', 0x20, b'f',
        ];
        assert_eq!(
            decode_arib_string(&unsupported_with_fallback),
            Ok("あ".to_string())
        );

        let supported_with_fallback = [
            0x24, 0x22, 0x9b, b'0', 0x20, b'f', 0x19, 0x24, 0x9b, b'1', 0x20, b'f',
        ];
        assert_eq!(
            decode_arib_string(&supported_with_fallback),
            Ok("あ".to_string())
        );
    }
}

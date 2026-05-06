include!("arib_jis_x0208_table.rs");

pub const ARIB_STRING_DECODER_SCOPE: &str = "mirakc_scope_non_caption_si_epg_only";

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AribStringDecodeError {
    TruncatedEscape,
    TruncatedGraphic,
    UnsupportedEscape,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AribStringDecodeDiagnostic {
    pub replacement_count: usize,
    pub unsupported_escape_count: usize,
    pub truncated_escape_count: usize,
    pub truncated_graphic_count: usize,
}

impl AribStringDecodeDiagnostic {
    pub fn summary(self) -> String {
        format!(
            "scope={} replacement_count={} unsupported_escape_count={} truncated_escape_count={} truncated_graphic_count={}",
            ARIB_STRING_DECODER_SCOPE,
            self.replacement_count,
            self.unsupported_escape_count,
            self.truncated_escape_count,
            self.truncated_graphic_count
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GraphicSet {
    Alnum,
    Hiragana,
    Katakana,
    Kanji,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct InvocationState {
    g0: GraphicSet,
    g1: GraphicSet,
    gl: GraphicSet,
}

impl Default for InvocationState {
    fn default() -> Self {
        Self {
            g0: GraphicSet::Alnum,
            g1: GraphicSet::Katakana,
            gl: GraphicSet::Alnum,
        }
    }
}

/// 字幕以外の ARIB SI/EPG 文字列を復号する。
/// サービス名、番組名、短形式イベント、長形式イベント用であり、字幕描画器ではない。
pub fn decode_arib_string(bytes: &[u8]) -> Result<String, AribStringDecodeError> {
    let mut out = String::new();
    let mut state = InvocationState::default();
    let mut index = 0usize;
    while index < bytes.len() {
        let byte = bytes[index];
        match byte {
            0x00 => {}
            0x0e => state.gl = state.g1,
            0x0f => state.gl = state.g0,
            0x1b => {
                let consumed = apply_escape(&mut state, &bytes[index..])?;
                index += consumed.saturating_sub(1);
            }
            0x20 => out.push(' '),
            0x21..=0x7e => {
                match state.gl {
                    GraphicSet::Alnum => out.push(byte as char),
                    GraphicSet::Hiragana => out.push_str(map_hiragana(byte)),
                    GraphicSet::Katakana => out.push_str(map_katakana(byte)),
                    GraphicSet::Kanji => {
                        let next = *bytes.get(index + 1).ok_or(AribStringDecodeError::TruncatedGraphic)?;
                        out.push_str(map_kanji(byte, next));
                        index += 1;
                    }
                }
            }
            0xa1..=0xfe => {
                let normalized = byte & 0x7f;
                match state.gl {
                    GraphicSet::Alnum => out.push(normalized as char),
                    GraphicSet::Hiragana => out.push_str(map_hiragana(normalized)),
                    GraphicSet::Katakana => out.push_str(map_katakana(normalized)),
                    GraphicSet::Kanji => {
                        let next = *bytes.get(index + 1).ok_or(AribStringDecodeError::TruncatedGraphic)? & 0x7f;
                        out.push_str(map_kanji(normalized, next));
                        index += 1;
                    }
                }
            }
            _ => out.push('�'),
        }
        index += 1;
    }
    Ok(out.trim_matches('\0').to_string())
}

/// 字幕以外の SI/EPG 文字列向けの劣化許容復号を行う。
/// 不正な放送データで設定処理や EPG スキャンを停止させないため、未対応バイトは置換文字にする。
pub fn decode_arib_string_lossy(bytes: &[u8]) -> String {
    decode_arib_string_lossy_with_diagnostic(bytes).0
}

pub fn decode_arib_string_lossy_with_diagnostic(bytes: &[u8]) -> (String, AribStringDecodeDiagnostic) {
    let mut out = String::new();
    let mut diagnostic = AribStringDecodeDiagnostic::default();
    let mut state = InvocationState::default();
    let mut index = 0usize;
    while index < bytes.len() {
        let byte = bytes[index];
        match byte {
            0x00 => {}
            0x0e => state.gl = state.g1,
            0x0f => state.gl = state.g0,
            0x1b => {
                match apply_escape(&mut state, &bytes[index..]) {
                    Ok(consumed) => index += consumed.saturating_sub(1),
                    Err(AribStringDecodeError::TruncatedEscape) => {
                        diagnostic.truncated_escape_count = diagnostic.truncated_escape_count.saturating_add(1);
                        diagnostic.replacement_count = diagnostic.replacement_count.saturating_add(1);
                        out.push('�');
                        break;
                    }
                    Err(AribStringDecodeError::TruncatedGraphic) => {
                        diagnostic.truncated_graphic_count = diagnostic.truncated_graphic_count.saturating_add(1);
                        diagnostic.replacement_count = diagnostic.replacement_count.saturating_add(1);
                        out.push('�');
                        break;
                    }
                    Err(AribStringDecodeError::UnsupportedEscape) => {
                        diagnostic.unsupported_escape_count = diagnostic.unsupported_escape_count.saturating_add(1);
                        diagnostic.replacement_count = diagnostic.replacement_count.saturating_add(1);
                        out.push('�');
                    }
                }
            }
            0x20 => out.push(' '),
            0x21..=0x7e => match state.gl {
                GraphicSet::Alnum => out.push(byte as char),
                GraphicSet::Hiragana => out.push_str(map_hiragana(byte)),
                GraphicSet::Katakana => out.push_str(map_katakana(byte)),
                GraphicSet::Kanji => {
                    let Some(next) = bytes.get(index + 1).copied() else {
                        diagnostic.truncated_graphic_count = diagnostic.truncated_graphic_count.saturating_add(1);
                        diagnostic.replacement_count = diagnostic.replacement_count.saturating_add(1);
                        out.push('�');
                        break;
                    };
                    if !(0x21..=0x7e).contains(&next) {
                        diagnostic.replacement_count = diagnostic.replacement_count.saturating_add(1);
                        out.push('�');
                    } else {
                        out.push_str(map_kanji(byte, next));
                        index += 1;
                    }
                }
            },
            0xa1..=0xfe => {
                let normalized = byte & 0x7f;
                match state.gl {
                    GraphicSet::Alnum | GraphicSet::Hiragana | GraphicSet::Katakana => {
                        diagnostic.replacement_count = diagnostic.replacement_count.saturating_add(1);
                        out.push('�');
                    }
                    GraphicSet::Kanji => {
                        let Some(next) = bytes.get(index + 1).copied() else {
                            diagnostic.truncated_graphic_count = diagnostic.truncated_graphic_count.saturating_add(1);
                            diagnostic.replacement_count = diagnostic.replacement_count.saturating_add(1);
                            out.push('�');
                            break;
                        };
                        if !(0xa1..=0xfe).contains(&next) && !(0x21..=0x7e).contains(&next) {
                            diagnostic.replacement_count = diagnostic.replacement_count.saturating_add(1);
                            out.push('�');
                        } else {
                            out.push_str(map_kanji(normalized, next & 0x7f));
                            index += 1;
                        }
                    }
                }
            }
            _ => {
                diagnostic.replacement_count = diagnostic.replacement_count.saturating_add(1);
                out.push('�');
            }
        }
        index += 1;
    }
    (out.trim_matches('\0').to_string(), diagnostic)
}

fn apply_escape(state: &mut InvocationState, bytes: &[u8]) -> Result<usize, AribStringDecodeError> {
    if bytes.len() < 3 {
        return Err(AribStringDecodeError::TruncatedEscape);
    }
    let consumed = match (bytes[1], bytes[2]) {
        (b'(', b'B' | b'J') => { state.g0 = GraphicSet::Alnum; 3 }
        (b'(', b'I') => { state.g0 = GraphicSet::Katakana; 3 }
        (b'(', b'0') => { state.g0 = GraphicSet::Hiragana; 3 }
        (b')', b'B' | b'J') => { state.g1 = GraphicSet::Alnum; 3 }
        (b')', b'I') => { state.g1 = GraphicSet::Katakana; 3 }
        (b')', b'0') => { state.g1 = GraphicSet::Hiragana; 3 }
        (b'$', b'B') => { state.g0 = GraphicSet::Kanji; 3 }
        (b'$', b'(') if bytes.len() >= 4 && matches!(bytes[3], b'B' | b'@') => { state.g0 = GraphicSet::Kanji; 4 }
        (b'$', b')') if bytes.len() >= 4 && matches!(bytes[3], b'B' | b'@') => { state.g1 = GraphicSet::Kanji; 4 }
        (b'$', b'*') if bytes.len() >= 4 && matches!(bytes[3], b'B' | b'@') => { state.g0 = GraphicSet::Kanji; 4 }
        (b'$', b'+') if bytes.len() >= 4 && matches!(bytes[3], b'B' | b'@') => { state.g1 = GraphicSet::Kanji; 4 }
        _ => return Err(AribStringDecodeError::UnsupportedEscape),
    };
    Ok(consumed)
}

fn map_hiragana(byte: u8) -> &'static str {
    const TABLE: &[&str] = &[
        "ぁ","あ","ぃ","い","ぅ","う","ぇ","え","ぉ","お","か","が","き","ぎ","く","ぐ","け","げ","こ","ご",
        "さ","ざ","し","じ","す","ず","せ","ぜ","そ","ぞ","た","だ","ち","ぢ","っ","つ","づ","て","で","と",
        "ど","な","に","ぬ","ね","の","は","ば","ぱ","ひ","び","ぴ","ふ","ぶ","ぷ","へ","べ","ぺ","ほ","ぼ",
        "ぽ","ま","み","む","め","も","ゃ","や","ゅ","ゆ","ょ","よ","ら","り","る","れ","ろ","ゎ","わ","ゐ",
        "ゑ","を","ん","ゔ","ゕ","ゖ","。","「","」","、","・","ー","ゝ","ゞ"
    ];
    TABLE.get((byte.saturating_sub(0x21)) as usize).copied().unwrap_or("�")
}

fn map_katakana(byte: u8) -> &'static str {
    const TABLE: &[&str] = &[
        "ァ","ア","ィ","イ","ゥ","ウ","ェ","エ","ォ","オ","カ","ガ","キ","ギ","ク","グ","ケ","ゲ","コ","ゴ",
        "サ","ザ","シ","ジ","ス","ズ","セ","ゼ","ソ","ゾ","タ","ダ","チ","ヂ","ッ","ツ","ヅ","テ","デ","ト",
        "ド","ナ","ニ","ヌ","ネ","ノ","ハ","バ","パ","ヒ","ビ","ピ","フ","ブ","プ","ヘ","ベ","ペ","ホ","ボ",
        "ポ","マ","ミ","ム","メ","モ","ャ","ヤ","ュ","ユ","ョ","ヨ","ラ","リ","ル","レ","ロ","ヮ","ワ","ヰ",
        "ヱ","ヲ","ン","ヴ","ヵ","ヶ","。","「","」","、","・","ー","ヽ","ヾ"
    ];
    TABLE.get((byte.saturating_sub(0x21)) as usize).copied().unwrap_or("�")
}

fn map_kanji(first: u8, second: u8) -> &'static str {
    map_jis_x0208(first, second)
}

#[cfg(test)]
mod tests {
    use super::{decode_arib_string_lossy, ARIB_STRING_DECODER_SCOPE};

    #[test]
    fn arib_string_decodes_basic_katakana() {
        let bytes = [0x1b, b'(', b'I', 0x22, 0x24, 0x26];
        assert_eq!(decode_arib_string_lossy(&bytes), "アイウ");
    }

    #[test]
    fn arib_string_decodes_service_name_descriptor_payload() {
        let descriptor_body = [0x00, 0x05, 0x1b, b'(', b'I', 0x22, 0x24];
        let service_name = &descriptor_body[2..];
        assert_eq!(decode_arib_string_lossy(service_name), "アイ");
    }

    #[test]
    fn arib_string_accepts_jis_x0208_esc_dollar_paren_b() {
        let bytes = [0x1b, b'$', b'(', b'B', 0x46, 0x7c];
        let out = decode_arib_string_lossy(&bytes);
        assert!(!out.is_empty());
    }

    #[test]
    fn arib_string_decodes_basic_kanji() {
        let bytes = [0x1b, b'$', b'B', b'E', b'l', b'5', b'~', 0x1b, b'(', b'B'];
        assert_eq!(decode_arib_string_lossy(&bytes), "東京");
    }

    #[test]
    fn arib_string_decodes_mixed_katakana_kanji_service_name() {
        let bytes = [0x1b, b'$', b'B', b'#', b'N', b'#', b'H', b'#', b'K', b'A', b'm', b'9', b'g', 0x1b, b'(', b'B'];
        assert_eq!(decode_arib_string_lossy(&bytes), "ＮＨＫ総合");
    }

    #[test]
    fn arib_string_does_not_fallback_to_utf8_lossy() {
        assert_eq!(decode_arib_string_lossy(&[0xff, 0xfe]), "��");
    }



    #[test]
    fn arib_string_decoder_scope_excludes_caption_renderer_claim() {
        assert_eq!(ARIB_STRING_DECODER_SCOPE, "mirakc_scope_non_caption_si_epg_only");
        assert_eq!(decode_arib_string_lossy(b"EPG"), "EPG");
    }
    #[test]
    fn arib_string_lossy_preserves_prefix_on_truncated_escape() {
        assert_eq!(decode_arib_string_lossy(&[b'A', 0x1b, b'(']), "A�");
    }

    #[test]
    fn arib_string_lossy_preserves_prefix_on_truncated_kanji() {
        let bytes = [b'A', 0x1b, b'$', b'B', b'E'];
        assert_eq!(decode_arib_string_lossy(&bytes), "A�");
    }
    #[test]
    fn arib_string_reports_non_caption_diagnostic_counts() {
        let (_decoded, diagnostic) = super::decode_arib_string_lossy_with_diagnostic(&[0x1b, b'$', b'X', b'A']);
        assert_eq!(diagnostic.unsupported_escape_count, 1);
        assert!(diagnostic.replacement_count >= 1);
        assert!(diagnostic.summary().contains(ARIB_STRING_DECODER_SCOPE));
    }

}

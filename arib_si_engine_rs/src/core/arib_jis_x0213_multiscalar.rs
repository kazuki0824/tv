// JIS X 0213:2004 のUnicode対応で、1セルが複数Unicode scalarへ対応する項目。
// Python標準 `euc_jis_2004` codecでPlane 1/2の94x94セルを全走査して生成した。
// Plane 1には25件、Plane 2にはmulti-scalar対応は0件である。

fn map_jis_x0213_plane1_multiscalar(first: u8, second: u8) -> Option<&'static str> {
    match (first, second) {
        (0x24, 0x77) => Some("か゚"),
        (0x24, 0x78) => Some("き゚"),
        (0x24, 0x79) => Some("く゚"),
        (0x24, 0x7a) => Some("け゚"),
        (0x24, 0x7b) => Some("こ゚"),
        (0x25, 0x77) => Some("カ゚"),
        (0x25, 0x78) => Some("キ゚"),
        (0x25, 0x79) => Some("ク゚"),
        (0x25, 0x7a) => Some("ケ゚"),
        (0x25, 0x7b) => Some("コ゚"),
        (0x25, 0x7c) => Some("セ゚"),
        (0x25, 0x7d) => Some("ツ゚"),
        (0x25, 0x7e) => Some("ト゚"),
        (0x26, 0x78) => Some("ㇷ゚"),
        (0x2b, 0x44) => Some("æ̀"),
        (0x2b, 0x48) => Some("ɔ̀"),
        (0x2b, 0x49) => Some("ɔ́"),
        (0x2b, 0x4a) => Some("ʌ̀"),
        (0x2b, 0x4b) => Some("ʌ́"),
        (0x2b, 0x4c) => Some("ə̀"),
        (0x2b, 0x4d) => Some("ə́"),
        (0x2b, 0x4e) => Some("ɚ̀"),
        (0x2b, 0x4f) => Some("ɚ́"),
        (0x2b, 0x65) => Some("˩˥"),
        (0x2b, 0x66) => Some("˥˩"),
        _ => None,
    }
}

fn map_jis_x0213_plane2_multiscalar(_first: u8, _second: u8) -> Option<&'static str> {
    None
}

#[cfg(test)]
mod tests {
    use super::{map_jis_x0213_plane1_multiscalar, map_jis_x0213_plane2_multiscalar};

    #[test]
    fn plane1_multiscalar_table_has_expected_boundaries() {
        assert_eq!(map_jis_x0213_plane1_multiscalar(0x24, 0x77), Some("か゚"));
        assert_eq!(map_jis_x0213_plane1_multiscalar(0x2b, 0x66), Some("˥˩"));
        assert_eq!(map_jis_x0213_plane1_multiscalar(0x21, 0x21), None);
    }

    #[test]
    fn plane2_has_no_multiscalar_cells_in_euc_jis_2004_mapping() {
        assert_eq!(map_jis_x0213_plane2_multiscalar(0x21, 0x21), None);
        assert_eq!(map_jis_x0213_plane2_multiscalar(0x7e, 0x7e), None);
    }
}

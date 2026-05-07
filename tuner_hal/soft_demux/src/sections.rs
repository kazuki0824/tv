use maleicacid_tuner_hal_common::MAX_SECTION_PAYLOAD_BYTES;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SectionHeader {
    pub table_id: u8,
    pub syntax: bool,
    pub section_length: usize,
    pub total_length: usize,
    pub table_id_extension: Option<u16>,
    pub version: Option<u8>,
    pub current_next_indicator: Option<bool>,
    pub section_number: Option<u8>,
    pub last_section_number: Option<u8>,
}

pub fn normalize_length_field_bits(bits: i32) -> Option<i32> {
    match bits {
        0 | 12 => Some(12),
        _ => None,
    }
}

pub fn parse_section_header(section: &[u8], length_field_bits: i32) -> Option<SectionHeader> {
    let normalized = normalize_length_field_bits(length_field_bits)?;
    if normalized != 12 || section.len() < 3 {
        return None;
    }
    let section_length = (((section[1] & 0x0f) as usize) << 8) | section[2] as usize;
    let total_length = 3 + section_length;
    if section.len() < total_length {
        return None;
    }
    let syntax = (section[1] & 0x80) != 0;
    let (table_id_extension, version, current_next_indicator, section_number, last_section_number) =
        if syntax {
            if total_length < 8 {
                return None;
            }
            (
                Some(u16::from_be_bytes([section[3], section[4]])),
                Some((section[5] >> 1) & 0x1f),
                Some((section[5] & 0x01) != 0),
                Some(section[6]),
                Some(section[7]),
            )
        } else {
            (None, None, None, None, None)
        };
    Some(SectionHeader {
        table_id: section[0],
        syntax,
        section_length,
        total_length,
        table_id_extension,
        version,
        current_next_indicator,
        section_number,
        last_section_number,
    })
}

pub fn section_crc_valid(section: &[u8], length_field_bits: i32) -> bool {
    let Some(header) = parse_section_header(section, length_field_bits) else {
        return false;
    };
    if header.section_length < 4 {
        return false;
    }
    crc32_mpeg(&section[..header.total_length]) == 0
}

pub fn crc32_mpeg(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in bytes {
        crc ^= (*byte as u32) << 24;
        for _ in 0..8 {
            if (crc & 0x8000_0000) != 0 {
                crc = (crc << 1) ^ 0x04c1_1db7;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SectionPushOutcome {
    pub sections: Vec<Vec<u8>>,
    pub oversized_section_drop_delta: u64,
    pub stale_partial_discard_delta: u64,
}

impl SectionPushOutcome {
    pub fn has_drop_or_discard(&self) -> bool {
        self.oversized_section_drop_delta > 0 || self.stale_partial_discard_delta > 0
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SectionAssembler {
    expected_len: Option<usize>,
    buf: Vec<u8>,
    oversized_section_drops: u64,
    stale_partial_section_discards: u64,
}

impl SectionAssembler {
    pub fn reset(&mut self) {
        self.expected_len = None;
        self.buf.clear();
    }

    pub fn oversized_section_drops(&self) -> u64 {
        self.oversized_section_drops
    }

    pub fn stale_partial_section_discards(&self) -> u64 {
        self.stale_partial_section_discards
    }

    pub(crate) fn set_expected_len_or_drop(&mut self, expected_len: usize) -> bool {
        if expected_len > MAX_SECTION_PAYLOAD_BYTES {
            self.oversized_section_drops = self.oversized_section_drops.saturating_add(1);
            self.reset();
            return false;
        }
        self.expected_len = Some(expected_len);
        true
    }

    pub fn push_payload_with_outcome(
        &mut self,
        payload_unit_start: bool,
        payload: &[u8],
    ) -> SectionPushOutcome {
        let oversized_before = self.oversized_section_drops;
        let stale_before = self.stale_partial_section_discards;
        let sections = self.push_payload(payload_unit_start, payload);
        SectionPushOutcome {
            sections,
            oversized_section_drop_delta: self
                .oversized_section_drops
                .saturating_sub(oversized_before),
            stale_partial_discard_delta: self
                .stale_partial_section_discards
                .saturating_sub(stale_before),
        }
    }

    pub fn push_payload(&mut self, payload_unit_start: bool, payload: &[u8]) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        if payload.is_empty() {
            return out;
        }

        let mut cursor = 0usize;
        if payload_unit_start {
            let pointer = payload[0] as usize;
            if 1 + pointer > payload.len() {
                self.reset();
                return out;
            }
            if !self.buf.is_empty() || self.expected_len.is_some() {
                if pointer > 0 {
                    self.buf.extend_from_slice(&payload[1..1 + pointer]);
                    self.try_take_pending(&mut out);
                }
                // PUSI marks a new section boundary.  Pointer bytes are the only
                // legal continuation of the previous section; if they do not
                // complete it (including pointer == 0), the stale partial section
                // must not be concatenated with the new section body.
                if !self.buf.is_empty() || self.expected_len.is_some() {
                    self.stale_partial_section_discards = self
                        .stale_partial_section_discards
                        .saturating_add(1);
                    self.reset();
                }
            }
            cursor = 1 + pointer;
        } else if self.buf.is_empty() && self.expected_len.is_none() {
            return out;
        }

        if cursor >= payload.len() {
            return out;
        }

        if !self.buf.is_empty() || self.expected_len.is_some() {
            self.buf.extend_from_slice(&payload[cursor..]);
            self.try_take_pending(&mut out);
            return out;
        }

        while cursor < payload.len() {
            if payload[cursor] == 0xff {
                break;
            }
            let remaining = &payload[cursor..];
            if remaining.len() < 3 {
                self.buf.extend_from_slice(remaining);
                self.expected_len = None;
                break;
            }
            let Some(header) = parse_section_header(remaining, 12).or_else(|| {
                let partial_len =
                    3 + ((((remaining[1] & 0x0f) as usize) << 8) | remaining[2] as usize);
                Some(SectionHeader {
                    table_id: remaining[0],
                    syntax: (remaining[1] & 0x80) != 0,
                    section_length: partial_len.saturating_sub(3),
                    total_length: partial_len,
                    table_id_extension: None,
                    version: None,
                    current_next_indicator: None,
                    section_number: None,
                    last_section_number: None,
                })
            }) else {
                break;
            };
            if header.total_length > MAX_SECTION_PAYLOAD_BYTES {
                self.oversized_section_drops = self.oversized_section_drops.saturating_add(1);
                break;
            }
            if remaining.len() >= header.total_length {
                out.push(remaining[..header.total_length].to_vec());
                cursor += header.total_length;
                continue;
            }
            self.buf.extend_from_slice(remaining);
            if !self.set_expected_len_or_drop(header.total_length) {
                break;
            }
            break;
        }
        out
    }

    fn try_take_pending(&mut self, out: &mut Vec<Vec<u8>>) {
        loop {
            if self.expected_len.is_none() && self.buf.len() >= 3 {
                let expected_len =
                    3 + ((((self.buf[1] & 0x0f) as usize) << 8) | self.buf[2] as usize);
                if !self.set_expected_len_or_drop(expected_len) {
                    return;
                }
            }
            let Some(expected_len) = self.expected_len else {
                return;
            };
            if self.buf.len() < expected_len {
                return;
            }
            let remaining = self.buf.split_off(expected_len);
            let section = std::mem::replace(&mut self.buf, remaining);
            self.expected_len = None;
            out.push(section);
            if self.buf.is_empty() {
                return;
            }
            if self.buf[0] == 0xff {
                self.reset();
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{crc32_mpeg, parse_section_header, section_crc_valid, SectionAssembler};
    use maleicacid_tuner_hal_common::MAX_SECTION_PAYLOAD_BYTES;

    fn section_with_crc(mut bytes: Vec<u8>) -> Vec<u8> {
        let crc = crc32_mpeg(&bytes);
        bytes.extend_from_slice(&crc.to_be_bytes());
        bytes
    }

    #[test]
    fn header_parser_reads_syntax_fields() {
        let section = section_with_crc(vec![
            0x42, 0xf0, 0x0d, 0x00, 0x01, 0xc7, 0x02, 0x03, 0x00, 0x00,
        ]);
        let header = parse_section_header(&section, 12).unwrap();
        assert_eq!(header.table_id, 0x42);
        assert_eq!(header.table_id_extension, Some(1));
        assert_eq!(header.version, Some(3));
        assert_eq!(header.section_number, Some(2));
        assert_eq!(header.last_section_number, Some(3));
        assert!(section_crc_valid(&section, 12));
    }

    #[test]
    fn assembler_carries_pointer_tail_into_previous_section() {
        let mut assembler = SectionAssembler::default();
        let section = section_with_crc(vec![
            0x00, 0xb0, 0x0d, 0x00, 0x01, 0xc1, 0x00, 0x00, 0x00, 0x01, 0xe1, 0x00,
        ]);
        let first = vec![
            0x00, section[0], section[1], section[2], section[3], section[4], section[5],
        ];
        assert!(assembler.push_payload(true, &first).is_empty());
        let mut second = vec![section.len() as u8 - 6];
        second.extend_from_slice(&section[6..]);
        let out = assembler.push_payload(true, &second);
        assert_eq!(out, vec![section]);
    }

    #[test]
    fn assembler_has_8192_byte_product_payload_cap() {
        assert_eq!(MAX_SECTION_PAYLOAD_BYTES, 8192);
    }

    #[test]
    fn assembler_accepts_8192_cap_and_rejects_8193_for_product_guard() {
        let mut assembler = SectionAssembler::default();
        assert!(assembler.set_expected_len_or_drop(MAX_SECTION_PAYLOAD_BYTES));
        assert_eq!(assembler.oversized_section_drops(), 0);
        assembler.reset();

        assert!(!assembler.set_expected_len_or_drop(MAX_SECTION_PAYLOAD_BYTES + 1));
        assert_eq!(assembler.oversized_section_drops(), 1);
    }

    #[test]
    fn assembler_delivers_largest_mpeg_section_under_8192_cap() {
        // MPEG/ARIB の section_length は 12 bit なので、表現可能な最大 section は 3 + 4095 bytes。
        // 8192 bytes の製品 guard が有効な大きめ EIT 系 section を拒否しないことを確認する。
        let mut assembler = SectionAssembler::default();
        let total_len = 3 + 4095;
        let mut section = vec![0x4e, 0xbf, 0xff];
        section.resize(total_len, 0x00);
        let mut payload = vec![0x00];
        payload.extend_from_slice(&section);
        let out = assembler.push_payload(true, &payload);
        assert_eq!(out, vec![section]);
        assert_eq!(assembler.oversized_section_drops(), 0);
    }

    #[test]
    fn assembler_emits_multiple_sections_from_single_pusi_payload() {
        let mut assembler = SectionAssembler::default();
        let s1 = section_with_crc(vec![
            0x00, 0xb0, 0x0d, 0x00, 0x01, 0xc1, 0x00, 0x00, 0x00, 0x01, 0xe1, 0x00,
        ]);
        let s2 = section_with_crc(vec![
            0x42, 0xf0, 0x0d, 0x00, 0x01, 0xc1, 0x00, 0x00, 0x48, 0x00,
        ]);
        let mut payload = vec![0x00];
        payload.extend_from_slice(&s1);
        payload.extend_from_slice(&s2);
        let out = assembler.push_payload(true, &payload);
        assert_eq!(out, vec![s1, s2]);
    }

    #[test]
    fn assembler_finishes_pending_then_emits_following_section() {
        let mut assembler = SectionAssembler::default();
        let s1 = section_with_crc(vec![
            0x00, 0xb0, 0x0d, 0x00, 0x01, 0xc1, 0x00, 0x00, 0x00, 0x01, 0xe1, 0x00,
        ]);
        let s2 = section_with_crc(vec![
            0x42, 0xf0, 0x0d, 0x00, 0x01, 0xc1, 0x00, 0x00, 0x48, 0x00,
        ]);
        let mut first = vec![0x00];
        first.extend_from_slice(&s1[..6]);
        assert!(assembler.push_payload(true, &first).is_empty());
        let mut second = vec![(s1.len() - 6) as u8];
        second.extend_from_slice(&s1[6..]);
        second.extend_from_slice(&s2);
        let out = assembler.push_payload(true, &second);
        assert_eq!(out, vec![s1, s2]);
    }

    #[test]
    fn assembler_does_not_concatenate_pointer_zero_new_section_to_stale_partial() {
        let mut assembler = SectionAssembler::default();
        let stale = section_with_crc(vec![
            0x00, 0xb0, 0x0d, 0x00, 0x01, 0xc1, 0x00, 0x00, 0x00, 0x01, 0xe1, 0x00,
        ]);
        let replacement = section_with_crc(vec![
            0x42, 0xf0, 0x0d, 0x00, 0x01, 0xc1, 0x00, 0x00, 0x48, 0x00,
        ]);
        let mut first = vec![0x00];
        first.extend_from_slice(&stale[..6]);
        assert!(assembler.push_payload(true, &first).is_empty());

        let mut second = vec![0x00];
        second.extend_from_slice(&replacement);
        let out = assembler.push_payload(true, &second);
        assert_eq!(out, vec![replacement]);
        assert_eq!(assembler.stale_partial_section_discards(), 1);
    }

    #[test]
    fn assembler_counts_pointer_tail_that_does_not_finish_stale_partial() {
        let mut assembler = SectionAssembler::default();
        let stale = section_with_crc(vec![
            0x00, 0xb0, 0x0d, 0x00, 0x01, 0xc1, 0x00, 0x00, 0x00, 0x01, 0xe1, 0x00,
        ]);
        let replacement = section_with_crc(vec![
            0x42, 0xf0, 0x0d, 0x00, 0x01, 0xc1, 0x00, 0x00, 0x48, 0x00,
        ]);
        let mut first = vec![0x00];
        first.extend_from_slice(&stale[..6]);
        assert!(assembler.push_payload(true, &first).is_empty());

        // Pointer bytes are too short to complete the pending section.  They
        // must be treated as the legal tail attempt, then the old partial must
        // be discarded before the new section body starts.
        let mut second = vec![0x02];
        second.extend_from_slice(&stale[6..8]);
        second.extend_from_slice(&replacement);
        let out = assembler.push_payload(true, &second);
        assert_eq!(out, vec![replacement]);
        assert_eq!(assembler.stale_partial_section_discards(), 1);
    }

    #[test]
    fn assembler_resets_on_invalid_pointer_field() {
        let mut assembler = SectionAssembler::default();
        assert!(assembler.push_payload(true, &[0x05, 0x00, 0x01]).is_empty());
        let section = section_with_crc(vec![
            0x00, 0xb0, 0x0d, 0x00, 0x01, 0xc1, 0x00, 0x00, 0x00, 0x01, 0xe1, 0x00,
        ]);
        let mut payload = vec![0x00];
        payload.extend_from_slice(&section);
        let out = assembler.push_payload(true, &payload);
        assert_eq!(out, vec![section]);
    }
}

#[cfg(test)]
mod section_header_contract_tests {
    use super::parse_section_header;

    #[test]
    fn parses_current_next_indicator() {
        let section = [
            0x00, 0xb0, 0x0d, 0x00, 0x01, 0xc0, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
            0xff, 0xff,
        ];
        let header = parse_section_header(&section, 12).unwrap();
        assert_eq!(header.version, Some(0));
        assert_eq!(header.current_next_indicator, Some(false));
    }

    #[test]
    fn rejects_non_12bit_length_field_contract() {
        let section = [0x00, 0xb0, 0x0d];
        assert!(parse_section_header(&section, 10).is_none());
    }
}

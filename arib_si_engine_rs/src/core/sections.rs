use std::collections::BTreeSet;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct SectionTracker {
    pub(crate) version: Option<u8>,
    pub(crate) last_section_number: Option<u8>,
    pub(crate) seen_sections: BTreeSet<u8>,
    pub(crate) inconsistent: bool,
    payloads: std::collections::BTreeMap<u8, Vec<u8>>,
}

impl SectionTracker {
    /// 5bit版番号の半周未満の前進だけを採用し、逆行・半周差は収集resetまで保留する。
    pub(crate) fn accepts_version(&self, version: u8) -> bool {
        self.version.map_or(true, |old| {
            (u16::from(version) + 32 - u16::from(old)) % 32 < 16
        })
    }

    pub(crate) fn observe(&mut self, version: u8, section: u8, last: u8, bytes: &[u8]) -> bool {
        if !self.accepts_version(version) {
            return false;
        }
        self.mark_seen(version, section, last);
        if let Some(previous) = self.payloads.get(&section) {
            if previous != bytes {
                self.inconsistent = true;
            }
            return false;
        }
        if self.inconsistent {
            return false;
        }
        self.payloads.insert(section, bytes.to_vec());
        true
    }

    pub(crate) fn mark_seen(&mut self, version: u8, section_number: u8, last_section_number: u8) {
        if self.version != Some(version) {
            self.version = Some(version);
            self.last_section_number = None;
            self.seen_sections.clear();
            self.payloads.clear();
            self.inconsistent = false;
        }
        if section_number > last_section_number {
            self.inconsistent = true;
            return;
        }
        match self.last_section_number {
            Some(expected) if expected != last_section_number => {
                self.inconsistent = true;
                return;
            }
            None => self.last_section_number = Some(last_section_number),
            Some(_) => {}
        }
        self.seen_sections.insert(section_number);
    }

    pub(crate) fn is_complete(&self) -> bool {
        if self.inconsistent {
            return false;
        }
        let Some(last) = self.last_section_number else {
            return false;
        };
        (0..=last).all(|section_number| self.seen_sections.contains(&section_number))
    }
}

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

pub fn parse_section_header(section: &[u8]) -> Option<SectionHeader> {
    if section.len() < 3 {
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

pub fn section_crc_valid(section: &[u8]) -> bool {
    let Some(header) = parse_section_header(section) else {
        return false;
    };
    section_crc_valid_with_header(section, &header)
}

pub fn section_crc_valid_with_header(section: &[u8], header: &SectionHeader) -> bool {
    if header.section_length < 4 || header.total_length > section.len() {
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

#[cfg(test)]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SectionAssembler {
    expected_len: Option<usize>,
    buf: Vec<u8>,
}

#[cfg(test)]
impl SectionAssembler {
    pub fn reset(&mut self) {
        self.expected_len = None;
        self.buf.clear();
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
            if pointer > 0 && (!self.buf.is_empty() || self.expected_len.is_some()) {
                self.buf.extend_from_slice(&payload[1..1 + pointer]);
                self.try_take_pending(&mut out);
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
            let Some(header) = parse_section_header(remaining).or_else(|| {
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
            if remaining.len() >= header.total_length {
                out.push(remaining[..header.total_length].to_vec());
                cursor += header.total_length;
                continue;
            }
            self.buf.extend_from_slice(remaining);
            self.expected_len = Some(header.total_length);
            break;
        }
        out
    }

    fn try_take_pending(&mut self, out: &mut Vec<Vec<u8>>) {
        loop {
            if self.expected_len.is_none() && self.buf.len() >= 3 {
                self.expected_len =
                    Some(3 + ((((self.buf[1] & 0x0f) as usize) << 8) | self.buf[2] as usize));
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

    fn section_with_crc(mut bytes: Vec<u8>) -> Vec<u8> {
        let crc = crc32_mpeg(&bytes);
        bytes.extend_from_slice(&crc.to_be_bytes());
        bytes
    }

    #[test]
    fn header_parser_reads_syntax_fields() {
        let section = section_with_crc(vec![
            0x42, 0xf0, 0x0b, 0x00, 0x01, 0xc7, 0x02, 0x03, 0x00, 0x00,
        ]);
        let header = parse_section_header(&section).unwrap();
        assert_eq!(header.table_id, 0x42);
        assert_eq!(header.table_id_extension, Some(1));
        assert_eq!(header.version, Some(3));
        assert_eq!(header.section_number, Some(2));
        assert_eq!(header.last_section_number, Some(3));
        assert!(section_crc_valid(&section));
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
    fn assembler_emits_multiple_sections_from_single_pusi_payload() {
        let mut assembler = SectionAssembler::default();
        let s1 = section_with_crc(vec![
            0x00, 0xb0, 0x0d, 0x00, 0x01, 0xc1, 0x00, 0x00, 0x00, 0x01, 0xe1, 0x00,
        ]);
        let s2 = section_with_crc(vec![
            0x42, 0xf0, 0x0b, 0x00, 0x01, 0xc1, 0x00, 0x00, 0x48, 0x00,
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
            0x42, 0xf0, 0x0b, 0x00, 0x01, 0xc1, 0x00, 0x00, 0x48, 0x00,
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
        let header = parse_section_header(&section).unwrap();
        assert_eq!(header.version, Some(0));
        assert_eq!(header.current_next_indicator, Some(false));
    }
}

fn section_body_end(section: &[u8]) -> Option<usize> {
    let header = parse_section_header(section)?;
    if header.section_length < 4 || header.total_length > section.len() {
        return None;
    }
    Some(3 + header.section_length - 4)
}

pub fn descriptor_loop_well_formed(bytes: &[u8]) -> bool {
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        if cursor + 2 > bytes.len() {
            return false;
        }
        let len = bytes[cursor + 1] as usize;
        let Some(next) = cursor.checked_add(2).and_then(|v| v.checked_add(len)) else {
            return false;
        };
        if next > bytes.len() {
            return false;
        }
        cursor = next;
    }
    true
}

pub fn section_has_malformed_descriptor_loop(
    pid: u16,
    table_id: u8,
    section: &[u8],
    known_pmt_pid: bool,
) -> bool {
    let Some(body_end) = section_body_end(section) else {
        return true;
    };
    match (pid, table_id) {
        (0x0001, 0x01) => {
            section.len() < 8 || body_end < 8 || !descriptor_loop_well_formed(&section[8..body_end])
        }
        (_, 0x02) if known_pmt_pid => {
            if section.len() < 12 || body_end < 12 || body_end > section.len() {
                return true;
            }
            let program_info_length = (((section[10] & 0x0f) as usize) << 8) | section[11] as usize;
            let Some(program_info_end) = 12usize.checked_add(program_info_length) else {
                return true;
            };
            if program_info_end > body_end
                || !descriptor_loop_well_formed(&section[12..program_info_end])
            {
                return true;
            }
            let mut cursor = program_info_end;
            while cursor < body_end {
                if cursor + 5 > body_end {
                    return true;
                }
                let es_info_length =
                    (((section[cursor + 3] & 0x0f) as usize) << 8) | section[cursor + 4] as usize;
                let Some(desc_start) = cursor.checked_add(5) else {
                    return true;
                };
                let Some(desc_end) = desc_start.checked_add(es_info_length) else {
                    return true;
                };
                if desc_end > body_end
                    || !descriptor_loop_well_formed(&section[desc_start..desc_end])
                {
                    return true;
                }
                cursor = desc_end;
            }
            false
        }
        (0x0010, 0x40) | (0x0010, 0x41) => {
            if section.len() < 10 || body_end < 10 {
                return true;
            }
            let descriptors_length = (((section[8] & 0x0f) as usize) << 8) | section[9] as usize;
            let Some(network_desc_end) = 10usize.checked_add(descriptors_length) else {
                return true;
            };
            if network_desc_end > body_end
                || !descriptor_loop_well_formed(&section[10..network_desc_end])
            {
                return true;
            }
            if network_desc_end + 2 > body_end {
                return true;
            }
            let transport_loop_length = (((section[network_desc_end] & 0x0f) as usize) << 8)
                | section[network_desc_end + 1] as usize;
            let mut cursor = network_desc_end + 2;
            let Some(transport_end) = cursor.checked_add(transport_loop_length) else {
                return true;
            };
            if transport_end > body_end {
                return true;
            }
            while cursor < transport_end {
                if cursor + 6 > transport_end {
                    return true;
                }
                let desc_len =
                    (((section[cursor + 4] & 0x0f) as usize) << 8) | section[cursor + 5] as usize;
                let desc_start = cursor + 6;
                let Some(desc_end) = desc_start.checked_add(desc_len) else {
                    return true;
                };
                if desc_end > transport_end
                    || !descriptor_loop_well_formed(&section[desc_start..desc_end])
                {
                    return true;
                }
                cursor = desc_end;
            }
            false
        }
        (0x0011, 0x42) | (0x0011, 0x46) => {
            if section.len() < 11 || body_end < 11 {
                return true;
            }
            let mut cursor = 11usize;
            while cursor < body_end {
                if cursor + 5 > body_end {
                    return true;
                }
                let desc_len =
                    (((section[cursor + 3] & 0x0f) as usize) << 8) | section[cursor + 4] as usize;
                let desc_start = cursor + 5;
                let Some(desc_end) = desc_start.checked_add(desc_len) else {
                    return true;
                };
                if desc_end > body_end
                    || !descriptor_loop_well_formed(&section[desc_start..desc_end])
                {
                    return true;
                }
                cursor = desc_end;
            }
            false
        }
        (0x0011, 0x4a) => {
            if section.len() < 10 || body_end < 10 {
                return true;
            }
            let bouquet_desc_len = (((section[8] & 0x0f) as usize) << 8) | section[9] as usize;
            let Some(bouquet_desc_end) = 10usize.checked_add(bouquet_desc_len) else {
                return true;
            };
            if bouquet_desc_end > body_end
                || !descriptor_loop_well_formed(&section[10..bouquet_desc_end])
            {
                return true;
            }
            if bouquet_desc_end + 2 > body_end {
                return true;
            }
            let transport_loop_length = (((section[bouquet_desc_end] & 0x0f) as usize) << 8)
                | section[bouquet_desc_end + 1] as usize;
            let mut cursor = bouquet_desc_end + 2;
            let Some(transport_end) = cursor.checked_add(transport_loop_length) else {
                return true;
            };
            if transport_end > body_end {
                return true;
            }
            while cursor < transport_end {
                if cursor + 6 > transport_end {
                    return true;
                }
                let desc_len =
                    (((section[cursor + 4] & 0x0f) as usize) << 8) | section[cursor + 5] as usize;
                let desc_start = cursor + 6;
                let Some(desc_end) = desc_start.checked_add(desc_len) else {
                    return true;
                };
                if desc_end > transport_end
                    || !descriptor_loop_well_formed(&section[desc_start..desc_end])
                {
                    return true;
                }
                cursor = desc_end;
            }
            false
        }
        _ => false,
    }
}

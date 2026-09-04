use std::collections::BTreeMap;

const DATA_IDENTIFIER_CAPTION: u8 = 0x80;
const DATA_IDENTIFIER_SUPERIMPOSE: u8 = 0x81;
const PRIVATE_STREAM_ID: u8 = 0xff;
const DATA_GROUP_HEADER_BYTES: usize = 5;
const CRC_BYTES: usize = 2;
const MAX_ASSEMBLED_GROUP_BYTES: usize = 256 * 1024;
const MAX_LANGUAGES: usize = 8;
const TMD_OFFSET_TIME: u8 = 0x02;
const TMD_REAL_TIME: u8 = 0x01;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptionLanguageFact {
    pub language_tag: u8,
    pub iso639: [u8; 3],
    pub dmf: u8,
    pub automatic_presentation_on_reception: bool,
    pub dc: Option<u8>,
    pub format: u8,
    pub tcs: u8,
    pub rollup_mode: u8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptionManagementFact {
    pub tmd: u8,
    pub languages: Vec<CaptionLanguageFact>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaptionStatementTimeFact {
    pub tmd: u8,
    pub millis_of_day: u32,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub enum CaptionFactDisposition {
    #[default]
    None = 0,
    FragmentPending = 1,
    Management = 2,
    StatementTimed = 3,
    StatementInvalid = 4,
    Invalid = 5,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CaptionFactBatch {
    pub disposition: CaptionFactDisposition,
    pub management: Option<CaptionManagementFact>,
    pub statement_time: Option<CaptionStatementTimeFact>,
}

impl CaptionFactBatch {
    fn note(&mut self, disposition: CaptionFactDisposition) {
        if disposition > self.disposition {
            self.disposition = disposition;
        }
    }
}

#[derive(Clone, Debug)]
enum FragmentAcceptance {
    Complete(Vec<u8>),
    Pending,
    Invalid,
}

#[derive(Clone, Debug)]
struct LinkedAssembly {
    last_link_number: u8,
    next_link_number: u8,
    body: Vec<u8>,
}

#[derive(Default)]
pub struct CaptionFactParser {
    superimpose: bool,
    assemblies: BTreeMap<(u8, u8), LinkedAssembly>,
}

impl CaptionFactParser {
    pub fn new(superimpose: bool) -> Self {
        Self {
            superimpose,
            assemblies: BTreeMap::new(),
        }
    }

    pub fn reset(&mut self) {
        self.assemblies.clear();
    }

    pub fn ingest(&mut self, pes_payload: &[u8]) -> CaptionFactBatch {
        let mut out = CaptionFactBatch::default();
        let expected_identifier = if self.superimpose {
            DATA_IDENTIFIER_SUPERIMPOSE
        } else {
            DATA_IDENTIFIER_CAPTION
        };
        if pes_payload.len() < 3
            || pes_payload[0] != expected_identifier
            || pes_payload[1] != PRIVATE_STREAM_ID
        {
            return out;
        }
        let private_header_length = (pes_payload[2] & 0x0f) as usize;
        let Some(mut cursor) = 3usize.checked_add(private_header_length) else {
            return out;
        };
        if cursor > pes_payload.len() {
            return out;
        }

        while cursor + DATA_GROUP_HEADER_BYTES + CRC_BYTES <= pes_payload.len() {
            let id_and_version = pes_payload[cursor];
            let data_group_id = id_and_version >> 2;
            let version = id_and_version & 0x03;
            let link_number = pes_payload[cursor + 1];
            let last_link_number = pes_payload[cursor + 2];
            let data_group_size =
                u16::from_be_bytes([pes_payload[cursor + 3], pes_payload[cursor + 4]]) as usize;
            let data_start = cursor + DATA_GROUP_HEADER_BYTES;
            let Some(data_end) = data_start.checked_add(data_group_size) else {
                break;
            };
            let Some(group_end) = data_end.checked_add(CRC_BYTES) else {
                break;
            };
            if group_end > pes_payload.len() || link_number > last_link_number {
                break;
            }
            let expected_crc =
                u16::from_be_bytes([pes_payload[data_end], pes_payload[data_end + 1]]);
            if crc16_arib(&pes_payload[cursor..data_end]) != expected_crc {
                self.assemblies.remove(&(data_group_id, version));
                out.note(CaptionFactDisposition::Invalid);
                cursor = group_end;
                continue;
            }

            match self.accept_fragment(
                data_group_id,
                version,
                link_number,
                last_link_number,
                &pes_payload[data_start..data_end],
            ) {
                FragmentAcceptance::Pending => out.note(CaptionFactDisposition::FragmentPending),
                FragmentAcceptance::Invalid => out.note(CaptionFactDisposition::Invalid),
                FragmentAcceptance::Complete(assembled) => {
                    let low_id = data_group_id & 0x1f;
                    if low_id == 0 {
                        if let Some(management) = parse_management(&assembled) {
                            out.management = Some(management);
                            out.note(CaptionFactDisposition::Management);
                        } else {
                            out.note(CaptionFactDisposition::Invalid);
                        }
                    } else if (1..=8).contains(&low_id) && self.superimpose {
                        if let Some(statement_time) = parse_statement_time(&assembled) {
                            out.statement_time = Some(statement_time);
                            out.note(CaptionFactDisposition::StatementTimed);
                        } else {
                            out.note(CaptionFactDisposition::StatementInvalid);
                        }
                    }
                }
            }
            cursor = group_end;
        }
        out
    }

    fn accept_fragment(
        &mut self,
        data_group_id: u8,
        version: u8,
        link_number: u8,
        last_link_number: u8,
        body: &[u8],
    ) -> FragmentAcceptance {
        let key = (data_group_id, version);
        if link_number == 0 {
            self.assemblies.remove(&key);
            if body.len() > MAX_ASSEMBLED_GROUP_BYTES {
                return FragmentAcceptance::Invalid;
            }
            if last_link_number == 0 {
                return FragmentAcceptance::Complete(body.to_vec());
            }
            self.assemblies.insert(
                key,
                LinkedAssembly {
                    last_link_number,
                    next_link_number: 1,
                    body: body.to_vec(),
                },
            );
            return FragmentAcceptance::Pending;
        }

        let Some(assembly) = self.assemblies.get_mut(&key) else {
            return FragmentAcceptance::Invalid;
        };
        if assembly.last_link_number != last_link_number || assembly.next_link_number != link_number
        {
            self.assemblies.remove(&key);
            return FragmentAcceptance::Invalid;
        }
        let Some(next_len) = assembly.body.len().checked_add(body.len()) else {
            self.assemblies.remove(&key);
            return FragmentAcceptance::Invalid;
        };
        if next_len > MAX_ASSEMBLED_GROUP_BYTES {
            self.assemblies.remove(&key);
            return FragmentAcceptance::Invalid;
        }
        assembly.body.extend_from_slice(body);
        if link_number == last_link_number {
            return match self.assemblies.remove(&key) {
                Some(completed) => FragmentAcceptance::Complete(completed.body),
                None => FragmentAcceptance::Invalid,
            };
        }
        assembly.next_link_number = assembly.next_link_number.saturating_add(1);
        FragmentAcceptance::Pending
    }
}

fn crc16_arib(data: &[u8]) -> u16 {
    let mut crc = 0u16;
    for &byte in data {
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

fn parse_management(body: &[u8]) -> Option<CaptionManagementFact> {
    let mut cursor = 0usize;
    let tmd = (body.get(cursor)? >> 6) & 0x03;
    cursor += 1;
    if tmd == TMD_OFFSET_TIME {
        cursor = cursor.checked_add(5)?;
        if cursor > body.len() {
            return None;
        }
    }
    let language_count = *body.get(cursor)? as usize;
    cursor += 1;
    if language_count > MAX_LANGUAGES {
        return None;
    }
    let mut languages = Vec::with_capacity(language_count);
    for _ in 0..language_count {
        let tag_and_dmf = *body.get(cursor)?;
        cursor += 1;
        let language_tag = (tag_and_dmf >> 5) & 0x07;
        let dmf = tag_and_dmf & 0x0f;
        let dc = if matches!(dmf, 0x0c..=0x0e) {
            let value = *body.get(cursor)?;
            cursor += 1;
            Some(value)
        } else {
            None
        };
        let iso639 = [
            *body.get(cursor)?,
            *body.get(cursor + 1)?,
            *body.get(cursor + 2)?,
        ];
        if !iso639.iter().all(u8::is_ascii_alphabetic) {
            return None;
        }
        cursor += 3;
        let format_tcs_rollup = *body.get(cursor)?;
        cursor += 1;
        languages.push(CaptionLanguageFact {
            language_tag,
            iso639,
            dmf,
            automatic_presentation_on_reception: ((dmf >> 2) & 0x03) == 0,
            dc,
            format: format_tcs_rollup >> 4,
            tcs: (format_tcs_rollup >> 2) & 0x03,
            rollup_mode: format_tcs_rollup & 0x03,
        });
    }
    languages.sort_by_key(|language| language.language_tag);
    Some(CaptionManagementFact { tmd, languages })
}

fn parse_statement_time(body: &[u8]) -> Option<CaptionStatementTimeFact> {
    if body.len() < 6 {
        return None;
    }
    let tmd = (body[0] >> 6) & 0x03;
    if tmd != TMD_REAL_TIME {
        return None;
    }
    let mut digits = [0u8; 9];
    let mut digit_index = 0usize;
    for value in &body[1..=5] {
        if digit_index < digits.len() {
            digits[digit_index] = value >> 4;
            digit_index += 1;
        }
        if digit_index < digits.len() {
            digits[digit_index] = value & 0x0f;
            digit_index += 1;
        }
    }
    if digits.iter().any(|digit| *digit > 9) {
        return None;
    }
    let hour = (digits[0] as u32) * 10 + digits[1] as u32;
    let minute = (digits[2] as u32) * 10 + digits[3] as u32;
    let second = (digits[4] as u32) * 10 + digits[5] as u32;
    let millis = (digits[6] as u32) * 100 + (digits[7] as u32) * 10 + digits[8] as u32;
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    Some(CaptionStatementTimeFact {
        tmd,
        millis_of_day: (hour * 3_600 + minute * 60 + second) * 1_000 + millis,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(id: u8, version: u8, link: u8, last: u8, body: &[u8]) -> Vec<u8> {
        let mut out = vec![(id << 2) | (version & 0x03), link, last];
        out.extend_from_slice(&(body.len() as u16).to_be_bytes());
        out.extend_from_slice(body);
        let crc = crc16_arib(&out);
        out.extend_from_slice(&crc.to_be_bytes());
        out
    }

    fn payload(superimpose: bool, groups: &[Vec<u8>]) -> Vec<u8> {
        let mut out = vec![if superimpose { 0x81 } else { 0x80 }, 0xff, 0x00];
        for group in groups {
            out.extend_from_slice(group);
        }
        out
    }

    #[test]
    fn crc16_matches_ccitt_zero_known_vector() {
        assert_eq!(crc16_arib(b"123456789"), 0x31c3);
    }

    #[test]
    fn rejects_corrupt_data_group_crc() {
        let body = [0x00, 0x01, 0x00, b'j', b'p', b'n', 0x10];
        let mut corrupt = group(0x00, 0, 0, 0, &body);
        let last = corrupt.len() - 1;
        corrupt[last] ^= 0x01;
        let mut parser = CaptionFactParser::new(false);
        let facts = parser.ingest(&payload(false, &[corrupt]));
        assert_eq!(facts.management, None);
        assert_eq!(facts.disposition, CaptionFactDisposition::Invalid);
    }

    #[test]
    fn corrupt_link_fragment_discards_in_progress_assembly() {
        let body = [0x00, 0x01, 0x00, b'j', b'p', b'n', 0x10];
        let first = group(0x00, 0, 0, 1, &body[..3]);
        let mut second = group(0x00, 0, 1, 1, &body[3..]);
        let last = second.len() - 1;
        second[last] ^= 0x01;
        let mut parser = CaptionFactParser::new(false);
        let pending = parser.ingest(&payload(false, &[first]));
        assert_eq!(pending.management, None);
        assert_eq!(pending.disposition, CaptionFactDisposition::FragmentPending);
        let invalid = parser.ingest(&payload(false, &[second]));
        assert_eq!(invalid.management, None);
        assert_eq!(invalid.disposition, CaptionFactDisposition::Invalid);
        assert!(parser.assemblies.is_empty());
    }

    #[test]
    fn reconstructs_linked_management_before_parsing() {
        let body = [0x00, 0x01, 0x00, b'j', b'p', b'n', 0x10];
        let first = group(0x00, 1, 0, 1, &body[..3]);
        let second = group(0x00, 1, 1, 1, &body[3..]);
        let mut parser = CaptionFactParser::new(false);
        let pending = parser.ingest(&payload(false, &[first]));
        assert_eq!(pending.management, None);
        assert_eq!(pending.disposition, CaptionFactDisposition::FragmentPending);
        let facts = parser.ingest(&payload(false, &[second]));
        assert_eq!(facts.disposition, CaptionFactDisposition::Management);
        let management = facts.management.expect("linked management");
        assert_eq!(management.languages.len(), 1);
        assert_eq!(management.languages[0].language_tag, 0);
        assert_eq!(&management.languages[0].iso639, b"jpn");
    }

    #[test]
    fn rejects_out_of_order_link_and_recovers_on_new_link_zero() {
        let body = [0x00, 0x01, 0x00, b'j', b'p', b'n', 0x10];
        let mut parser = CaptionFactParser::new(false);
        parser.ingest(&payload(false, &[group(0x00, 0, 0, 2, &body[..2])]));
        assert_eq!(
            parser
                .ingest(&payload(false, &[group(0x00, 0, 2, 2, &body[2..])]))
                .management,
            None
        );
        let facts = parser.ingest(&payload(false, &[group(0x00, 0, 0, 0, &body)]));
        assert!(facts.management.is_some());
    }

    #[test]
    fn complete_superimpose_statement_with_reserved_tmd_is_invalid() {
        let body = [0x80, 0x12, 0x00, 0x05, 0x12, 0x30];
        let mut parser = CaptionFactParser::new(true);
        let facts = parser.ingest(&payload(true, &[group(0x01, 0, 0, 0, &body)]));
        assert_eq!(facts.disposition, CaptionFactDisposition::StatementInvalid);
        assert_eq!(facts.statement_time, None);
    }

    #[test]
    fn complete_superimpose_statement_with_malformed_stm_is_invalid() {
        let body = [0x40, 0x1a, 0x00, 0x05, 0x12, 0x30];
        let mut parser = CaptionFactParser::new(true);
        let facts = parser.ingest(&payload(true, &[group(0x01, 0, 0, 0, &body)]));
        assert_eq!(facts.disposition, CaptionFactDisposition::StatementInvalid);
        assert_eq!(facts.statement_time, None);
    }

    #[test]
    fn parses_superimpose_real_time_statement_stm() {
        let body = [0x40, 0x12, 0x00, 0x05, 0x12, 0x30];
        let mut parser = CaptionFactParser::new(true);
        let facts = parser.ingest(&payload(true, &[group(0x01, 0, 0, 0, &body)]));
        assert_eq!(facts.disposition, CaptionFactDisposition::StatementTimed);
        assert_eq!(
            facts.statement_time,
            Some(CaptionStatementTimeFact {
                tmd: 1,
                millis_of_day: (12 * 3_600 + 5) * 1_000 + 123,
            })
        );
    }
}

use crate::sections::{descriptor_loop_well_formed, section_crc_valid_with_header, SectionHeader};

pub const TABLE_ID_TDT: u8 = 0x70;
pub const TABLE_ID_TOT: u8 = 0x73;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BroadcastClockFact {
    pub table_id: u8,
    pub mjd: u16,
    pub millis_of_day: u32,
}

pub fn parse_broadcast_clock(section: &[u8], header: &SectionHeader) -> Option<BroadcastClockFact> {
    if section.len() < 8 || (section[1] & 0xf0) != 0x70 || (section[1] & 0x0c) != 0 {
        return None;
    }
    let table_id = header.table_id;
    let section_length = header.section_length;
    if header.syntax || section.len() != header.total_length {
        return None;
    }
    match table_id {
        TABLE_ID_TDT => {
            if section_length != 5 || section.len() != 8 {
                return None;
            }
        }
        TABLE_ID_TOT => {
            if section_length < 11 || section.len() < 14 || (section[8] & 0xf0) != 0xf0 {
                return None;
            }
            let descriptors_loop_length =
                (((section[8] & 0x0f) as usize) << 8) | section[9] as usize;
            if section_length != 11usize.checked_add(descriptors_loop_length)?
                || !section_crc_valid_with_header(section, header)
                || !descriptor_loop_well_formed(&section[10..header.total_length - 4])
            {
                return None;
            }
        }
        _ => return None,
    }

    let mjd = u16::from_be_bytes([section[3], section[4]]);
    let hour = decode_bcd(section[5])?;
    let minute = decode_bcd(section[6])?;
    let second = decode_bcd(section[7])?;
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    Some(BroadcastClockFact {
        table_id,
        mjd,
        millis_of_day: (hour * 3_600 + minute * 60 + second) * 1_000,
    })
}

fn decode_bcd(value: u8) -> Option<u32> {
    let high = value >> 4;
    let low = value & 0x0f;
    (high <= 9 && low <= 9).then_some((high as u32) * 10 + low as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sections::crc32_mpeg;

    fn clock(section: &[u8]) -> Option<BroadcastClockFact> {
        let header = crate::sections::parse_section_header(section)?;
        parse_broadcast_clock(section, &header)
    }

    fn tot_with_crc(mut prefix: Vec<u8>) -> Vec<u8> {
        let crc = crc32_mpeg(&prefix);
        prefix.extend_from_slice(&crc.to_be_bytes());
        prefix
    }

    #[test]
    fn parses_tdt_jst_clock() {
        let section = [0x70, 0x70, 0x05, 0xea, 0x60, 0x12, 0x34, 0x56];
        assert_eq!(
            clock(&section),
            Some(BroadcastClockFact {
                table_id: TABLE_ID_TDT,
                mjd: 0xea60,
                millis_of_day: (12 * 3_600 + 34 * 60 + 56) * 1_000,
            })
        );
    }

    #[test]
    fn parses_tot_jst_clock_and_validates_crc() {
        let section = tot_with_crc(vec![
            0x73, 0x70, 0x0b, 0xea, 0x60, 0x12, 0x34, 0x56, 0xf0, 0x00,
        ]);
        assert_eq!(
            clock(&section),
            Some(BroadcastClockFact {
                table_id: TABLE_ID_TOT,
                mjd: 0xea60,
                millis_of_day: (12 * 3_600 + 34 * 60 + 56) * 1_000,
            })
        );
        let mut corrupt = section;
        let last = corrupt.len() - 1;
        corrupt[last] ^= 0x01;
        assert_eq!(clock(&corrupt), None);
    }

    #[test]
    fn rejects_bad_bcd_and_invalid_tot_descriptor_length() {
        assert_eq!(
            clock(&[0x70, 0x70, 0x05, 0xea, 0x60, 0x12, 0x6a, 0x56]),
            None
        );
        let invalid_tot = tot_with_crc(vec![
            0x73, 0x70, 0x0b, 0xea, 0x60, 0x12, 0x34, 0x56, 0xf0, 0x01,
        ]);
        assert_eq!(clock(&invalid_tot), None);
    }
    #[test]
    fn rejects_clock_structure_before_publishing_a_fact() {
        let tdt = vec![0x70, 0x70, 0x05, 0xea, 0x60, 0x12, 0x34, 0x56];
        for (index, value) in [
            (0, 0x71),
            (1, 0xf0),
            (2, 6),
            (5, 0x24),
            (6, 0x60),
            (7, 0x60),
        ] {
            let mut bad = tdt.clone();
            bad[index] = value;
            assert_eq!(clock(&bad), None);
        }
        let mut trailing = tdt;
        trailing.push(0);
        assert_eq!(clock(&trailing), None);
        let malformed_tot = tot_with_crc(vec![
            0x73, 0x70, 0x0e, 0xea, 0x60, 0x12, 0x34, 0x56, 0xf0, 3, 0x58, 2, 0,
        ]);
        assert_eq!(clock(&malformed_tot), None);
    }
}

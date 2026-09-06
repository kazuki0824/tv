use crate::discovery_requirements::DiscoveryProfile;
use crate::sections::parse_section_header;

/// Program publishへ渡してよいEIT sectionかを媒体profile込みで判定する。
/// BS/110CSのEIT[p/f] actual (table_id 0x4e) はsection 0/1だけが現在/次番組の
/// publish事実であり、section 2以降をProgram行へ投影しない。
pub(crate) fn is_program_publish_eit_section(
    profile: DiscoveryProfile,
    pid: u16,
    section: &[u8],
) -> bool {
    if pid != 0x0012 {
        return true;
    }
    let Some(header) = parse_section_header(section) else {
        return true;
    };
    if header.table_id != 0x4e {
        return true;
    }
    match profile {
        DiscoveryProfile::Bs | DiscoveryProfile::Cs110 => {
            matches!(header.section_number, Some(0 | 1))
        }
        DiscoveryProfile::IsdbT => true,
    }
}

#[cfg(test)]
mod tests {
    use super::is_program_publish_eit_section;
    use crate::discovery_requirements::DiscoveryProfile;

    fn syntax_section(table_id: u8, section_number: u8) -> Vec<u8> {
        // policy判定に必要なsyntax headerだけを持つ最小section。CRC妥当性は上位のingestで検証する。
        vec![table_id, 0xb0, 0x05, 0x00, 0x01, 0xc1, section_number, section_number]
    }

    #[test]
    fn satellite_pf_actual_allows_only_sections_zero_and_one() {
        for profile in [DiscoveryProfile::Bs, DiscoveryProfile::Cs110] {
            assert!(is_program_publish_eit_section(
                profile,
                0x0012,
                &syntax_section(0x4e, 0),
            ));
            assert!(is_program_publish_eit_section(
                profile,
                0x0012,
                &syntax_section(0x4e, 1),
            ));
            assert!(!is_program_publish_eit_section(
                profile,
                0x0012,
                &syntax_section(0x4e, 2),
            ));
            assert!(!is_program_publish_eit_section(
                profile,
                0x0012,
                &syntax_section(0x4e, 7),
            ));
        }
    }

    #[test]
    fn terrestrial_and_non_pf_actual_sections_are_unchanged() {
        assert!(is_program_publish_eit_section(
            DiscoveryProfile::IsdbT,
            0x0012,
            &syntax_section(0x4e, 2),
        ));
        assert!(is_program_publish_eit_section(
            DiscoveryProfile::Bs,
            0x0012,
            &syntax_section(0x50, 2),
        ));
        assert!(is_program_publish_eit_section(
            DiscoveryProfile::Bs,
            0x0011,
            &syntax_section(0x4e, 2),
        ));
    }
}

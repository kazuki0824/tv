from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    p = Path(path)
    text = p.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one anchor, found {count}: {old[:100]!r}")
    p.write_text(text.replace(old, new, 1))


# #8: table_id-specific section_length transport closure.
replace_once(
    "tuner_hal2/common/src/lib.rs",
    """/// ARIB STD-B10 の table_id 別 section_length 上限を返す。
/// EIT p/f と EIT schedule は 0x4e..=0x6f、それ以外は短い section として扱う。
pub fn max_arib_section_length_for_table_id(table_id: u8) -> usize {
    match table_id {
        0x4e..=0x6f => MAX_ARIB_EIT_SECTION_LENGTH,
        0x70 => ARIB_TDT_SECTION_LENGTH,
        _ => MAX_ARIB_SHORT_SECTION_LENGTH,
    }
}
""",
    """/// ARIB STD-B10 の table_id 別 section_length 上限を返す。
///
/// HALは表の意味解析を行わずtransport外形だけを検証する。STD-B10で
/// 1021-byte区分として固定される既知tableだけをshort classへ置き、
/// EIT/ST/INT/PCAT/BIT/NBIT/LDT/LIT/ERT/ITT/AMTおよび予約/private/未割当は
/// 4093-byte transport classとして扱う。TDTだけはsection_length=5固定。
pub fn max_arib_section_length_for_table_id(table_id: u8) -> usize {
    match table_id {
        0x70 => ARIB_TDT_SECTION_LENGTH,
        0x00..=0x03 | 0x40 | 0x41 | 0x42 | 0x46 | 0x4a | 0x71 | 0x73 => {
            MAX_ARIB_SHORT_SECTION_LENGTH
        }
        _ => MAX_ARIB_EIT_SECTION_LENGTH,
    }
}
""",
)

common = Path("tuner_hal2/common/src/lib.rs")
text = common.read_text()
if "section_length_contract_distinguishes_short_extended_and_tdt_classes" not in text:
    text += r'''

#[cfg(test)]
mod section_length_contract_tests {
    use super::*;

    #[test]
    fn section_length_contract_distinguishes_short_extended_and_tdt_classes() {
        for table_id in [0x00, 0x01, 0x02, 0x03, 0x40, 0x41, 0x42, 0x46, 0x4a, 0x71, 0x73] {
            assert!(is_valid_arib_section_length(table_id, 1021));
            assert!(!is_valid_arib_section_length(table_id, 1022));
        }
        for table_id in [0x04, 0x4c, 0x4e, 0x6f, 0x72, 0xc2, 0xc4, 0xc7, 0xd0, 0xd2, 0xfe, 0xff] {
            assert!(is_valid_arib_section_length(table_id, 4093));
            assert!(!is_valid_arib_section_length(table_id, 4094));
        }
        assert!(is_valid_arib_section_length(0x70, 5));
        assert!(!is_valid_arib_section_length(0x70, 4));
        assert!(!is_valid_arib_section_length(0x70, 6));
    }
}
'''
    common.write_text(text)

# #10: product profile promises authoritative presentation timestamps for successful video events.
replace_once(
    "tuner_hal2/demux/src/runtime/filter.rs",
    "        if self.open_type != FilterOpenType::TsAudio {\n",
    "        if self.open_type == FilterOpenType::TsVideo && packet.pts_90khz.is_none() {\n"
    "            return Err(AudioTimestampAssociationFailure::MissingAnchor);\n"
    "        }\n"
    "        if self.open_type != FilterOpenType::TsAudio {\n",
)

filter_rs = Path("tuner_hal2/demux/src/runtime/filter.rs")
text = filter_rs.read_text()
if "video_media_payload_requires_authoritative_pes_pts" not in text:
    text += r'''

#[cfg(test)]
mod video_pts_contract_tests {
    use super::*;
    use crate::config::ConfigInputPid;

    fn video_packet(pts_90khz: Option<u64>) -> PesPacket {
        PesPacket {
            pid: PacketPid::from_config_pid(ConfigInputPid::validate_tpid(0x0100).unwrap()),
            stream_id: 0xe0,
            pts_90khz,
            dts_90khz: None,
            is_pes_private_data: false,
            data_alignment_indicator: true,
            raw_bytes: vec![0, 0, 1, 0xe0],
            payload: vec![1, 2, 3, 4],
        }
    }

    #[test]
    fn video_media_payload_requires_authoritative_pes_pts() {
        let mut filter = FilterRuntime::new_typed(1, 1, FilterOpenType::TsVideo);
        assert_eq!(
            filter.prepare_av_media_payloads(video_packet(None), TsInputOrigin::frontend(1)),
            Err(AudioTimestampAssociationFailure::MissingAnchor)
        );
        let prepared = filter
            .prepare_av_media_payloads(video_packet(Some(90_000)), TsInputOrigin::frontend(1))
            .expect("explicit video PTS is authoritative for this product profile");
        assert_eq!(prepared.len(), 1);
        assert!(prepared[0].metadata.is_pts_present);
        assert_eq!(prepared[0].metadata.pts_90khz, Some(90_000));
    }
}
'''
    filter_rs.write_text(text)

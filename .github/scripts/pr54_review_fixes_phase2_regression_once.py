from pathlib import Path

p = Path("arib_si_engine_rs/src/core/provider_data.rs")
text = p.read_text()
old = '''fn valid_subtitle_component(v: &SubtitleComponentV1) -> bool {
    valid_optional_es_pid(v.es_pid)
        && valid_optional_u8(v.component_tag)
        && (v.es_pid.is_some() || v.component_tag.is_some())
        && valid_optional_u16(v.data_component_id)
        && valid_optional_iso639(&v.language)
        && nonempty(&v.caption_service_kind)
        && nonempty(&v.parse_status)
}'''
new = '''fn valid_subtitle_component(v: &SubtitleComponentV1) -> bool {
    valid_optional_es_pid(v.es_pid)
        && valid_optional_u8(v.component_tag)
        && (v.es_pid.is_some() || v.component_tag.is_some())
        && valid_optional_u16(v.data_component_id)
        && v.caption_dmf
            .map(|value| (0..=15).contains(&value))
            .unwrap_or(true)
        && v.caption_timing
            .map(|value| (0..=3).contains(&value))
            .unwrap_or(true)
        && valid_optional_iso639(&v.language)
        && nonempty(&v.caption_service_kind)
        && nonempty(&v.parse_status)
}'''
if text.count(old) != 1:
    raise SystemExit(f"subtitle validator置換対象が一意ではありません: {text.count(old)}")
text = text.replace(old, new, 1)

marker = '''    #[test]
    fn series_expire_date_requires_matching_validity_flag() {'''
test = '''    #[test]
    fn subtitle_component_preserves_data_component_presentation_facts() {
        let mut request = minimal_program_request_value();
        request["components"]["subtitle"] = serde_json::json!([{
            "esPid": 0x123,
            "componentTag": 0x30,
            "dataComponentId": 0x0008,
            "captionDmf": 0x0c,
            "captionTiming": 0x02,
            "automaticPresentationOnReception": true,
            "language": "jpn",
            "captionServiceKind": "superimpose",
            "parseStatus": "OK"
        }]);

        let result = build_program_provider_data(&request.to_string());
        assert!(result.success, "{}", result.error_message);
        let canonical: serde_json::Value = serde_json::from_str(&result.json).unwrap();
        let subtitle = &canonical["components"]["subtitle"][0];
        assert_eq!(subtitle["captionDmf"], 0x0c);
        assert_eq!(subtitle["captionTiming"], 0x02);
        assert_eq!(subtitle["automaticPresentationOnReception"], true);
    }

'''
if text.count(marker) != 1:
    raise SystemExit(f"subtitle regression挿入位置が一意ではありません: {text.count(marker)}")
p.write_text(text.replace(marker, test + marker, 1))

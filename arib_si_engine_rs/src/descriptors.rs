use crate::arib_string::decode_arib_string_lossy;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EventDescriptors {
    pub title: String,
    /// short_event_descriptor.text。TvProvider の SHORT_DESCRIPTION に対応する。
    pub description: String,
    /// extended_event_descriptor.text。TvProvider の LONG_DESCRIPTION の詳細本文に対応する。
    pub extended_description: String,
    pub contents: Vec<ContentDescriptorItem>,
    pub components: Vec<ComponentDescriptor>,
    pub audio_components: Vec<AudioComponentDescriptor>,
    pub parental_ratings: Vec<ParentalRating>,
    pub series: Vec<SeriesDescriptor>,
    pub event_groups: Vec<EventGroupDescriptor>,
    pub linkages: Vec<LinkageDescriptor>,
    pub extended_items: Vec<ExtendedEventItem>,
    pub unknown: Vec<(u8, Vec<u8>)>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EventProviderFields {
    pub title: String,
    pub description: String,
    pub extended_description: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EventDescriptorDiagnostic {
    pub descriptor_json: String,
    pub content_count: usize,
    pub component_count: usize,
    pub audio_component_count: usize,
    pub parental_rating_count: usize,
    pub series_count: usize,
    pub event_group_count: usize,
    pub linkage_count: usize,
    pub unknown_count: usize,
}

pub fn event_provider_fields(desc: &EventDescriptors) -> EventProviderFields {
    EventProviderFields {
        title: desc.title.clone(),
        description: desc.description.clone(),
        extended_description: desc.extended_description.clone(),
    }
}

pub fn event_descriptor_diagnostic(desc: &EventDescriptors) -> EventDescriptorDiagnostic {
    EventDescriptorDiagnostic {
        descriptor_json: event_descriptors_to_json(desc),
        content_count: desc.contents.len(),
        component_count: desc.components.len(),
        audio_component_count: desc.audio_components.len(),
        parental_rating_count: desc.parental_ratings.len(),
        series_count: desc.series.len(),
        event_group_count: desc.event_groups.len(),
        linkage_count: desc.linkages.len(),
        unknown_count: desc.unknown.len(),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtendedEventItem {
    pub item_description: String,
    pub item_text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentDescriptorItem {
    pub content_nibble_level_1: u8,
    pub content_nibble_level_2: u8,
    pub user_nibble_1: u8,
    pub user_nibble_2: u8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComponentDescriptor {
    pub stream_content: u8,
    pub component_type: u8,
    pub component_tag: u8,
    pub language_code: String,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AudioComponentDescriptor {
    pub stream_content: u8,
    pub component_type: u8,
    pub component_tag: u8,
    pub stream_type: u8,
    pub simulcast_group_tag: u8,
    pub es_multi_lingual_flag: bool,
    pub main_component_flag: bool,
    pub quality_indicator: u8,
    pub sampling_rate: u8,
    pub language_code: String,
    pub language_code_2: Option<String>,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParentalRating {
    pub country_code: String,
    pub rating: u8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SeriesDescriptor {
    pub series_id: u16,
    pub repeat_label: u8,
    pub program_pattern: u8,
    pub expire_date: u16,
    pub episode_number: u16,
    pub last_episode_number: u16,
    pub series_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventGroupDescriptor {
    pub group_type: u8,
    pub events: Vec<EventGroupReference>,
    pub other_network_events: Vec<EventGroupReference>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventGroupReference {
    pub service_id: u16,
    pub event_id: u16,
    pub original_network_id: Option<u16>,
    pub transport_stream_id: Option<u16>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LinkageDescriptor {
    pub transport_stream_id: u16,
    pub original_network_id: u16,
    pub service_id: u16,
    pub linkage_type: u8,
    pub private_data: Vec<u8>,
}

pub fn parse_event_descriptors(bytes: &[u8]) -> EventDescriptors {
    let mut out = EventDescriptors::default();
    let mut extended_event_bodies: Vec<Vec<u8>> = Vec::new();
    let mut cursor = 0usize;
    while cursor + 2 <= bytes.len() {
        let tag = bytes[cursor];
        let len = bytes[cursor + 1] as usize;
        let body_start = cursor + 2;
        let Some(body_end) = body_start.checked_add(len) else { break; };
        if body_end > bytes.len() { break; }
        let body = &bytes[body_start..body_end];
        match tag {
            0x4d => parse_short_event(body, &mut out),
            0x4e => extended_event_bodies.push(body.to_vec()),
            0x54 => out.contents.extend(parse_content_descriptor(body)),
            0x50 => if let Some(v) = parse_component_descriptor(body) { out.components.push(v); },
            0xc4 => if let Some(v) = parse_audio_component_descriptor(body) { out.audio_components.push(v); },
            0x55 => out.parental_ratings.extend(parse_parental_rating_descriptor(body)),
            0xd5 => if let Some(v) = parse_series_descriptor(body) { out.series.push(v); },
            0xd6 => if let Some(v) = parse_event_group_descriptor(body) { out.event_groups.push(v); },
            0x4a => if let Some(v) = parse_linkage_descriptor(body) { out.linkages.push(v); },
            _ => out.unknown.push((tag, body.to_vec())),
        }
        cursor = body_end;
    }
    parse_extended_event_fragments(&extended_event_bodies, &mut out);
    out
}

fn parse_short_event(body: &[u8], out: &mut EventDescriptors) {
    if body.len() < 4 { return; }
    let name_len = body[3] as usize;
    let name_start = 4usize;
    let name_end = name_start.saturating_add(name_len).min(body.len());
    if out.title.is_empty() && name_start <= name_end {
        out.title = decode_arib_string_lossy(&body[name_start..name_end]).trim().to_string();
    }
    if name_end < body.len() {
        let text_len = body[name_end] as usize;
        let text_start = name_end + 1;
        let text_end = text_start.saturating_add(text_len).min(body.len());
        let text = decode_arib_string_lossy(&body[text_start..text_end]).trim().to_string();
        if !text.is_empty() { out.description = join_description(&out.description, &text); }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct ExtendedEventFragment {
    descriptor_number: u8,
    items: Vec<(Vec<u8>, Vec<u8>)>,
    text: Vec<u8>,
}

fn parse_extended_event_fragments(bodies: &[Vec<u8>], out: &mut EventDescriptors) {
    let mut fragments = bodies.iter().filter_map(|body| parse_extended_event_fragment(body)).collect::<Vec<_>>();
    fragments.sort_by_key(|fragment| fragment.descriptor_number);

    let mut text_bytes = Vec::new();
    let mut current_item_description = Vec::new();
    let mut current_item_text = Vec::new();
    let mut flush_item = |out: &mut EventDescriptors, description: &mut Vec<u8>, item_text: &mut Vec<u8>| {
        if description.is_empty() && item_text.is_empty() {
            return;
        }
        out.extended_items.push(ExtendedEventItem {
            item_description: decode_arib_string_lossy(description).trim().to_string(),
            item_text: decode_arib_string_lossy(item_text).trim().to_string(),
        });
        description.clear();
        item_text.clear();
    };
    for fragment in &fragments {
        text_bytes.extend_from_slice(&fragment.text);
        for (description, item_text) in &fragment.items {
            if !description.is_empty() {
                flush_item(out, &mut current_item_description, &mut current_item_text);
                current_item_description.extend_from_slice(description);
            }
            current_item_text.extend_from_slice(item_text);
        }
    }
    flush_item(out, &mut current_item_description, &mut current_item_text);
    let text = decode_arib_string_lossy(&text_bytes).trim().to_string();
    if !text.is_empty() {
        out.extended_description = join_description(&out.extended_description, &text);
    }
}

fn parse_extended_event_fragment(body: &[u8]) -> Option<ExtendedEventFragment> {
    if body.len() < 6 { return None; }
    let descriptor_number = (body[0] >> 4) & 0x0f;
    let items_len = body[4] as usize;
    let mut cursor = 5usize;
    let items_end = cursor.checked_add(items_len)?.min(body.len());
    let mut items = Vec::new();
    while cursor + 2 <= items_end {
        let desc_len = body[cursor] as usize;
        let desc_start = cursor + 1;
        let desc_end = desc_start.checked_add(desc_len)?.min(items_end);
        if desc_end > items_end { break; }
        let item_len_index = desc_end;
        if item_len_index >= items_end { break; }
        let item_len = body[item_len_index] as usize;
        let item_start = item_len_index + 1;
        let item_end = item_start.checked_add(item_len)?.min(items_end);
        if item_end > items_end { break; }
        items.push((body[desc_start..desc_end].to_vec(), body[item_start..item_end].to_vec()));
        cursor = item_end;
    }
    let text_len_index = 5usize.checked_add(items_len)?;
    if text_len_index >= body.len() {
        return Some(ExtendedEventFragment { descriptor_number, items, text: Vec::new() });
    }
    let text_len = body[text_len_index] as usize;
    let text_start = text_len_index + 1;
    let text_end = text_start.checked_add(text_len)?.min(body.len());
    Some(ExtendedEventFragment {
        descriptor_number,
        items,
        text: body[text_start..text_end].to_vec(),
    })
}

fn parse_content_descriptor(body: &[u8]) -> Vec<ContentDescriptorItem> {
    body.chunks_exact(2).map(|chunk| ContentDescriptorItem {
        content_nibble_level_1: chunk[0] >> 4,
        content_nibble_level_2: chunk[0] & 0x0f,
        user_nibble_1: chunk[1] >> 4,
        user_nibble_2: chunk[1] & 0x0f,
    }).collect()
}

fn parse_component_descriptor(body: &[u8]) -> Option<ComponentDescriptor> {
    if body.len() < 6 { return None; }
    Some(ComponentDescriptor {
        stream_content: body[0] & 0x0f,
        component_type: body[1],
        component_tag: body[2],
        language_code: language(&body[3..6]),
        text: decode_arib_string_lossy(&body[6..]).trim().to_string(),
    })
}

fn parse_audio_component_descriptor(body: &[u8]) -> Option<AudioComponentDescriptor> {
    if body.len() < 9 { return None; }
    let flags = body[5];
    let second_language = (flags & 0x80) != 0;
    let mut cursor = 9usize;
    let lang2 = if second_language && body.len() >= 12 {
        cursor = 12;
        Some(language(&body[9..12]))
    } else { None };
    Some(AudioComponentDescriptor {
        stream_content: body[0] & 0x0f,
        component_type: body[1],
        component_tag: body[2],
        stream_type: body[3],
        simulcast_group_tag: body[4],
        es_multi_lingual_flag: second_language,
        main_component_flag: (flags & 0x40) != 0,
        quality_indicator: (flags >> 4) & 0x03,
        sampling_rate: flags & 0x07,
        language_code: language(&body[6..9]),
        language_code_2: lang2,
        text: decode_arib_string_lossy(body.get(cursor..).unwrap_or(&[])).trim().to_string(),
    })
}

fn parse_parental_rating_descriptor(body: &[u8]) -> Vec<ParentalRating> {
    body.chunks_exact(4).map(|chunk| ParentalRating { country_code: language(&chunk[0..3]), rating: chunk[3] & 0x0f }).collect()
}

fn parse_series_descriptor(body: &[u8]) -> Option<SeriesDescriptor> {
    if body.len() < 8 { return None; }
    Some(SeriesDescriptor {
        series_id: u16_at(body, 0),
        repeat_label: (body[2] >> 4) & 0x0f,
        program_pattern: body[2] & 0x07,
        expire_date: u16::from_be_bytes([body[3] & 0x0f, body[4]]),
        episode_number: (((body[5] & 0x0f) as u16) << 8) | body[6] as u16,
        last_episode_number: (((body[7] & 0x0f) as u16) << 8) | *body.get(8).unwrap_or(&0) as u16,
        series_name: if body.len() > 9 { decode_arib_string_lossy(&body[9..]).trim().to_string() } else { String::new() },
    })
}

fn parse_event_group_descriptor(body: &[u8]) -> Option<EventGroupDescriptor> {
    if body.is_empty() { return None; }
    let group_type = (body[0] >> 4) & 0x0f;
    let event_count = (body[0] & 0x0f) as usize;
    let mut cursor = 1usize;
    let mut events = Vec::new();
    for _ in 0..event_count {
        if cursor + 4 > body.len() { return None; }
        events.push(EventGroupReference { service_id: u16_at(body, cursor), event_id: u16_at(body, cursor + 2), original_network_id: None, transport_stream_id: None });
        cursor += 4;
    }
    let mut other_network_events = Vec::new();
    while cursor + 8 <= body.len() {
        other_network_events.push(EventGroupReference {
            service_id: u16_at(body, cursor + 4),
            event_id: u16_at(body, cursor + 6),
            original_network_id: Some(u16_at(body, cursor)),
            transport_stream_id: Some(u16_at(body, cursor + 2)),
        });
        cursor += 8;
    }
    Some(EventGroupDescriptor { group_type, events, other_network_events })
}

fn parse_linkage_descriptor(body: &[u8]) -> Option<LinkageDescriptor> {
    if body.len() < 7 { return None; }
    Some(LinkageDescriptor {
        transport_stream_id: u16_at(body, 0),
        original_network_id: u16_at(body, 2),
        service_id: u16_at(body, 4),
        linkage_type: body[6],
        private_data: body[7..].to_vec(),
    })
}

fn language(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).trim().to_string()
}

fn u16_at(bytes: &[u8], offset: usize) -> u16 { u16::from_be_bytes([bytes[offset], bytes[offset + 1]]) }
fn join_description(current: &str, next: &str) -> String { if current.is_empty() { next.to_string() } else { format!("{}\n{}", current, next) } }

/// TvProvider の安定キーに自然に入らない記述子向けの診断専用 JSON。
/// TvProvider 向けのタイトルと説明は event_provider_fields() を使う。
pub fn event_descriptors_to_json(desc: &EventDescriptors) -> String {
    let mut fields = Vec::new();
    fields.push(format!("\"title\":\"{}\"", json_escape(&desc.title)));
    fields.push(format!("\"description\":\"{}\"", json_escape(&desc.description)));
    fields.push(format!("\"contents\":[{}]", desc.contents.iter().map(|c| format!("{{\"level1\":{},\"level2\":{},\"user1\":{},\"user2\":{}}}", c.content_nibble_level_1, c.content_nibble_level_2, c.user_nibble_1, c.user_nibble_2)).collect::<Vec<_>>().join(",")));
    fields.push(format!("\"parentalRatings\":[{}]", desc.parental_ratings.iter().map(|r| format!("{{\"country\":\"{}\",\"rating\":{}}}", json_escape(&r.country_code), r.rating)).collect::<Vec<_>>().join(",")));
    fields.push(format!("\"components\":[{}]", desc.components.iter().map(|c| format!("{{\"streamContent\":{},\"componentType\":{},\"componentTag\":{},\"language\":\"{}\",\"text\":\"{}\"}}", c.stream_content, c.component_type, c.component_tag, json_escape(&c.language_code), json_escape(&c.text))).collect::<Vec<_>>().join(",")));
    fields.push(format!("\"audioComponents\":[{}]", desc.audio_components.iter().map(|a| format!("{{\"streamContent\":{},\"componentType\":{},\"componentTag\":{},\"streamType\":{},\"simulcastGroupTag\":{},\"multiLingual\":{},\"main\":{},\"quality\":{},\"samplingRate\":{},\"language\":\"{}\",\"secondLanguage\":\"{}\",\"text\":\"{}\"}}", a.stream_content, a.component_type, a.component_tag, a.stream_type, a.simulcast_group_tag, a.es_multi_lingual_flag, a.main_component_flag, a.quality_indicator, a.sampling_rate, json_escape(&a.language_code), json_escape(a.language_code_2.as_deref().unwrap_or("")), json_escape(&a.text))).collect::<Vec<_>>().join(",")));
    fields.push(format!("\"series\":[{}]", desc.series.iter().map(|v| format!("{{\"seriesId\":{},\"repeatLabel\":{},\"programPattern\":{},\"expireDate\":{},\"episodeNumber\":{},\"lastEpisodeNumber\":{},\"seriesName\":\"{}\"}}", v.series_id, v.repeat_label, v.program_pattern, v.expire_date, v.episode_number, v.last_episode_number, json_escape(&v.series_name))).collect::<Vec<_>>().join(",")));
    fields.push(format!("\"eventGroups\":[{}]", desc.event_groups.iter().map(event_group_to_json).collect::<Vec<_>>().join(",")));
    fields.push(format!("\"linkages\":[{}]", desc.linkages.iter().map(|l| format!("{{\"transportStreamId\":{},\"originalNetworkId\":{},\"serviceId\":{},\"linkageType\":{},\"privateDataHex\":\"{}\"}}", l.transport_stream_id, l.original_network_id, l.service_id, l.linkage_type, hex_prefix(&l.private_data, 32))).collect::<Vec<_>>().join(",")));
    fields.push(format!("\"extendedItems\":[{}]", desc.extended_items.iter().map(|i| format!("{{\"description\":\"{}\",\"text\":\"{}\"}}", json_escape(&i.item_description), json_escape(&i.item_text))).collect::<Vec<_>>().join(",")));
    fields.push(format!("\"unknownDescriptors\":[{}]", desc.unknown.iter().map(|(tag, body)| format!("{{\"tag\":{},\"length\":{},\"hexPrefix\":\"{}\",\"checksum\":{}}}", tag, body.len(), hex_prefix(body, 16), additive_checksum(body))).collect::<Vec<_>>().join(",")));
    fields.push(format!("\"unknownDescriptorCount\":{}", desc.unknown.len()));
    format!("{{{}}}", fields.join(","))
}

fn event_group_to_json(group: &EventGroupDescriptor) -> String {
    let events = group.events.iter().map(event_group_reference_to_json).collect::<Vec<_>>().join(",");
    let other = group.other_network_events.iter().map(event_group_reference_to_json).collect::<Vec<_>>().join(",");
    format!("{{\"groupType\":{},\"events\":[{}],\"otherNetworkEvents\":[{}]}}", group.group_type, events, other)
}

fn event_group_reference_to_json(reference: &EventGroupReference) -> String {
    format!(
        "{{\"serviceId\":{},\"eventId\":{},\"originalNetworkId\":{},\"transportStreamId\":{}}}",
        reference.service_id,
        reference.event_id,
        reference.original_network_id.map(|v| v.to_string()).unwrap_or_else(|| "null".to_string()),
        reference.transport_stream_id.map(|v| v.to_string()).unwrap_or_else(|| "null".to_string())
    )
}

fn hex_prefix(bytes: &[u8], max_len: usize) -> String {
    bytes.iter().take(max_len).map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join("")
}

fn additive_checksum(bytes: &[u8]) -> u32 { bytes.iter().fold(0u32, |acc, b| acc.wrapping_add(u32::from(*b))) }

pub(crate) fn json_escape(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod diagnostic_json_tests {
    use super::*;

    #[test]
    fn descriptor_json_contains_major_values_and_unknown_digest() {
        let descriptors = EventDescriptors {
            contents: vec![ContentDescriptorItem { content_nibble_level_1: 1, content_nibble_level_2: 2, user_nibble_1: 3, user_nibble_2: 4 }],
            components: vec![ComponentDescriptor { stream_content: 1, component_type: 0xb3, component_tag: 7, language_code: "jpn".to_string(), text: "映像".to_string() }],
            audio_components: vec![AudioComponentDescriptor {
                stream_content: 2,
                component_type: 0x03,
                component_tag: 8,
                stream_type: 0x0f,
                simulcast_group_tag: 0,
                es_multi_lingual_flag: true,
                main_component_flag: true,
                quality_indicator: 1,
                sampling_rate: 7,
                language_code: "jpn".to_string(),
                language_code_2: Some("eng".to_string()),
                text: "音声".to_string(),
            }],
            parental_ratings: vec![ParentalRating { country_code: "JPN".to_string(), rating: 15 }],
            series: vec![SeriesDescriptor { series_id: 0x1234, repeat_label: 1, program_pattern: 2, expire_date: 0x1fff, episode_number: 3, last_episode_number: 12, series_name: "シリーズ".to_string() }],
            linkages: vec![LinkageDescriptor { transport_stream_id: 1, original_network_id: 4, service_id: 101, linkage_type: 0x0d, private_data: vec![0xaa, 0xbb] }],
            event_groups: vec![EventGroupDescriptor { group_type: 1, events: vec![EventGroupReference { service_id: 101, event_id: 202, original_network_id: None, transport_stream_id: None }], other_network_events: vec![] }],
            unknown: vec![(0xfe, vec![0x12, 0x34, 0x56])],
            ..EventDescriptors::default()
        };
        let json = event_descriptors_to_json(&descriptors);
        assert!(json.contains("\"level1\":1"));
        assert!(json.contains("\"componentType\":179"));
        assert!(json.contains("\"linkageType\":13"));
        assert!(json.contains("\"groupType\":1"));
        assert!(json.contains("\"language\":\"jpn\""));
        assert!(json.contains("\"country\":\"JPN\""));
        assert!(json.contains("\"seriesId\":4660"));
        assert!(json.contains("\"tag\":254"));
        assert!(json.contains("\"hexPrefix\":\"123456\""));
    }

    #[test]
    fn provider_fields_are_separate_from_descriptor_diagnostic_json() {
        let descriptors = EventDescriptors {
            title: "番組".to_string(),
            description: "説明".to_string(),
            contents: vec![ContentDescriptorItem { content_nibble_level_1: 1, content_nibble_level_2: 2, user_nibble_1: 0, user_nibble_2: 0 }],
            ..EventDescriptors::default()
        };
        let provider = event_provider_fields(&descriptors);
        let diagnostic = event_descriptor_diagnostic(&descriptors);
        assert_eq!(provider.title, "番組");
        assert_eq!(provider.description, "説明");
        assert_eq!(diagnostic.content_count, 1);
        assert!(diagnostic.descriptor_json.contains("\"contents\""));
    }
}


#[cfg(test)]
mod mirakc_scope_extended_event_tests {
    use super::*;

    fn descriptor(tag: u8, body: &[u8]) -> Vec<u8> {
        let mut out = vec![tag, body.len() as u8];
        out.extend_from_slice(body);
        out
    }

    #[test]
    fn extended_event_text_fragments_are_decoded_after_descriptor_order_concatenation() {
        let mut first = vec![0x10, b'j', b'p', b'n', 0x00, 0x02, b'A', b'B'];
        let mut second = vec![0x00, b'j', b'p', b'n', 0x00, 0x02, b'C', b'D'];
        let mut bytes = descriptor(0x4e, &first);
        bytes.extend_from_slice(&descriptor(0x4e, &second));
        let parsed = parse_event_descriptors(&bytes);
        assert_eq!(parsed.description, "");
        assert_eq!(parsed.extended_description, "CDAB");
    }

    #[test]
    fn extended_event_item_fragments_continue_until_next_description() {
        let first = vec![0x00, b'j', b'p', b'n', 0x06, 0x03, b'A', b'B', b'C', 0x01, b'D', 0x00];
        let second = vec![0x10, b'j', b'p', b'n', 0x03, 0x00, 0x02, b'E', b'F', 0x00];
        let mut bytes = descriptor(0x4e, &first);
        bytes.extend_from_slice(&descriptor(0x4e, &second));
        let parsed = parse_event_descriptors(&bytes);
        assert_eq!(parsed.extended_items.len(), 1);
        assert_eq!(parsed.extended_items[0].item_description, "ABC");
        assert_eq!(parsed.extended_items[0].item_text, "DEF");
    }
}

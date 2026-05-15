use crate::arib_string::decode_arib_string_lossy;
use crate::provider_data::{DescriptorDiagnosticV1, DescriptorScopeV1, SectionScopeV1};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EventDescriptors {
    pub diagnostics: Vec<DescriptorDiagnostic>,
    pub title: String,
    /// short_event_descriptor.text。TvProvider の SHORT_DESCRIPTION に対応する。
    pub description: String,
    /// extended_event_descriptor.text。TvProvider の LONG_DESCRIPTION の詳細本文に対応する。
    pub extended_description: String,
    pub contents: Vec<ContentDescriptorItem>,
    pub components: Vec<ComponentDescriptor>,
    pub audio_components: Vec<AudioComponentDescriptor>,
    pub parental_ratings: Vec<ParentalRating>,
    pub parental_rating_descriptors: Vec<ParentalRatingDescriptor>,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DescriptorParseStatus {
    Ok,
    MalformedLength,
    TruncatedDescriptor,
    UnsupportedValue,
    InvalidSequence,
}

impl DescriptorParseStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "Ok",
            Self::MalformedLength => "MalformedLength",
            Self::TruncatedDescriptor => "TruncatedDescriptor",
            Self::UnsupportedValue => "UnsupportedValue",
            Self::InvalidSequence => "InvalidSequence",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DescriptorDiagnostic {
    pub parse_status: DescriptorParseStatus,
    pub descriptor_tag: u8,
    pub offset: usize,
    pub declared_length: usize,
    pub remaining_length: usize,
    pub raw_prefix: Vec<u8>,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RawExtendedEventDescriptor {
    body: Vec<u8>,
    tag: u8,
    offset: usize,
    declared_length: usize,
}

fn descriptor_diagnostic(status: DescriptorParseStatus, tag: u8, offset: usize, declared_length: usize, remaining_length: usize, raw_prefix: &[u8], message: &str) -> DescriptorDiagnostic {
    DescriptorDiagnostic {
        parse_status: status,
        descriptor_tag: tag,
        offset,
        declared_length,
        remaining_length,
        raw_prefix: raw_prefix.iter().take(16).copied().collect(),
        message: message.to_string(),
    }
}

pub fn event_descriptor_loop_truncated_diagnostic(offset: usize, declared_length: usize, remaining_length: usize, raw_prefix: &[u8]) -> DescriptorDiagnostic {
    descriptor_diagnostic(
        DescriptorParseStatus::TruncatedDescriptor,
        0xff,
        offset,
        declared_length,
        remaining_length,
        raw_prefix,
        "EIT event descriptors_loop_length exceeds section body",
    )
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
    pub arib_display_name: String,
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
    pub rating_value: u8,
    pub raw_rating_byte: u8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParentalRatingDescriptor {
    pub entries: Vec<ParentalRating>,
    pub raw_descriptor_bytes: Vec<u8>,
    pub parse_status: DescriptorParseStatus,
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
    let mut extended_event_bodies: Vec<RawExtendedEventDescriptor> = Vec::new();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        if cursor + 2 > bytes.len() {
            out.diagnostics.push(descriptor_diagnostic(
                DescriptorParseStatus::TruncatedDescriptor,
                0xff,
                cursor,
                0,
                bytes.len().saturating_sub(cursor),
                &bytes[cursor..],
                "descriptor header is truncated",
            ));
            break;
        }
        let tag = bytes[cursor];
        let len = bytes[cursor + 1] as usize;
        let body_start = cursor + 2;
        let Some(body_end) = body_start.checked_add(len) else {
            out.diagnostics.push(descriptor_diagnostic(DescriptorParseStatus::MalformedLength, tag, cursor, len, bytes.len().saturating_sub(body_start), &bytes[cursor..], "descriptor length overflows usize"));
            break;
        };
        if body_end > bytes.len() {
            out.diagnostics.push(descriptor_diagnostic(DescriptorParseStatus::TruncatedDescriptor, tag, cursor, len, bytes.len().saturating_sub(body_start), &bytes[cursor..], "descriptor body exceeds event descriptor loop"));
            break;
        }
        let body = &bytes[body_start..body_end];
        match tag {
            0x4d => parse_short_event(body, &mut out, tag, cursor, len),
            0x4e => extended_event_bodies.push(RawExtendedEventDescriptor { body: body.to_vec(), tag, offset: cursor, declared_length: len }),
            0x54 => parse_content_descriptor(body, &mut out, tag, cursor, len),
            0x50 => match parse_component_descriptor(body) { Some(v) => out.components.push(v), None => out.diagnostics.push(descriptor_diagnostic(DescriptorParseStatus::MalformedLength, tag, cursor, len, body.len(), body, "component_descriptor is shorter than its fixed fields")), },
            0xc4 => match parse_audio_component_descriptor(body) { Some(v) => out.audio_components.push(v), None => out.diagnostics.push(descriptor_diagnostic(DescriptorParseStatus::MalformedLength, tag, cursor, len, body.len(), body, "audio_component_descriptor is shorter than its fixed fields or second language is truncated")), },
            0x55 => {
                let descriptor = parse_parental_rating_descriptor(body, &mut out, tag, cursor, len);
                out.parental_ratings.extend(descriptor.entries.clone());
                out.parental_rating_descriptors.push(descriptor);
            },
            0xd5 => match parse_series_descriptor(body) { Some(v) => out.series.push(v), None => out.diagnostics.push(descriptor_diagnostic(DescriptorParseStatus::MalformedLength, tag, cursor, len, body.len(), body, "series_descriptor is shorter than 9-byte fixed fields")), },
            0xd6 => if let Some(v) = parse_event_group_descriptor(body) { out.event_groups.push(v); } else { out.diagnostics.push(descriptor_diagnostic(DescriptorParseStatus::MalformedLength, tag, cursor, len, body.len(), body, "event_group_descriptor is malformed")); },
            0x4a => if let Some(v) = parse_linkage_descriptor(body) { out.linkages.push(v); } else { out.diagnostics.push(descriptor_diagnostic(DescriptorParseStatus::MalformedLength, tag, cursor, len, body.len(), body, "linkage_descriptor is shorter than fixed fields")); },
            _ => {
                out.unknown.push((tag, body.to_vec()));
                out.diagnostics.push(descriptor_diagnostic(
                    DescriptorParseStatus::UnsupportedValue,
                    tag,
                    cursor,
                    len,
                    body.len(),
                    body,
                    "unknown descriptor is preserved for diagnostics only",
                ));
            },
        }
        cursor = body_end;
    }
    parse_extended_event_fragments(&extended_event_bodies, &mut out);
    out
}

fn parse_short_event(body: &[u8], out: &mut EventDescriptors, tag: u8, offset: usize, declared_length: usize) {
    if body.len() < 4 {
        out.diagnostics.push(descriptor_diagnostic(DescriptorParseStatus::MalformedLength, tag, offset, declared_length, body.len(), body, "short_event_descriptor is shorter than fixed fields"));
        return;
    }
    let name_len = body[3] as usize;
    let name_start = 4usize;
    let Some(name_end) = name_start.checked_add(name_len) else {
        out.diagnostics.push(descriptor_diagnostic(DescriptorParseStatus::MalformedLength, tag, offset, declared_length, body.len(), body, "short_event_descriptor event_name length overflows"));
        return;
    };
    if name_end > body.len() {
        out.diagnostics.push(descriptor_diagnostic(DescriptorParseStatus::MalformedLength, tag, offset, declared_length, body.len().saturating_sub(name_start), body, "short_event_descriptor event_name length exceeds body"));
        return;
    }
    if name_end >= body.len() {
        out.diagnostics.push(descriptor_diagnostic(DescriptorParseStatus::MalformedLength, tag, offset, declared_length, 0, body, "short_event_descriptor text length byte is missing"));
        return;
    }
    let text_len = body[name_end] as usize;
    let text_start = name_end + 1;
    let Some(text_end) = text_start.checked_add(text_len) else {
        out.diagnostics.push(descriptor_diagnostic(DescriptorParseStatus::MalformedLength, tag, offset, declared_length, body.len(), body, "short_event_descriptor text length overflows"));
        return;
    };
    if text_end != body.len() {
        out.diagnostics.push(descriptor_diagnostic(DescriptorParseStatus::MalformedLength, tag, offset, declared_length, body.len().saturating_sub(text_start), body, "short_event_descriptor text length does not match body"));
        return;
    }
    if out.title.is_empty() {
        out.title = decode_arib_string_lossy(&body[name_start..name_end]).trim().to_string();
    }
    let text = decode_arib_string_lossy(&body[text_start..text_end]).trim().to_string();
    if !text.is_empty() { out.description = join_description(&out.description, &text); }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct ExtendedEventFragment {
    descriptor_number: u8,
    last_descriptor_number: u8,
    items: Vec<(Vec<u8>, Vec<u8>)>,
    text: Vec<u8>,
}

fn parse_extended_event_fragments(bodies: &[RawExtendedEventDescriptor], out: &mut EventDescriptors) {
    let mut fragments = Vec::new();
    for raw in bodies {
        if let Some(fragment) = parse_extended_event_fragment(&raw.body, out, raw.tag, raw.offset, raw.declared_length) {
            fragments.push(fragment);
        }
    }
    if fragments.is_empty() { return; }
    fragments.sort_by_key(|fragment| fragment.descriptor_number);
    let expected_last = fragments[0].last_descriptor_number;
    let mut seen = std::collections::BTreeSet::new();
    let sequence_ok = fragments.iter().all(|fragment| fragment.last_descriptor_number == expected_last)
        && expected_last as usize + 1 == fragments.len()
        && fragments.iter().all(|fragment| seen.insert(fragment.descriptor_number))
        && (0..=expected_last).all(|number| seen.contains(&number));
    if !sequence_ok {
        out.diagnostics.push(descriptor_diagnostic(DescriptorParseStatus::InvalidSequence, 0x4e, 0, bodies.len(), fragments.len(), bodies.first().map(|b| b.body.as_slice()).unwrap_or(&[]), "extended_event_descriptor fragment sequence has duplicate, missing, or inconsistent last_descriptor_number"));
        return;
    }

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

fn parse_extended_event_fragment(body: &[u8], out: &mut EventDescriptors, tag: u8, offset: usize, declared_length: usize) -> Option<ExtendedEventFragment> {
    if body.len() < 6 {
        out.diagnostics.push(descriptor_diagnostic(DescriptorParseStatus::MalformedLength, tag, offset, declared_length, body.len(), body, "extended_event_descriptor is shorter than fixed fields"));
        return None;
    }
    let descriptor_number = (body[0] >> 4) & 0x0f;
    let last_descriptor_number = body[0] & 0x0f;
    let items_len = body[4] as usize;
    let mut cursor = 5usize;
    let Some(items_end) = cursor.checked_add(items_len) else {
        out.diagnostics.push(descriptor_diagnostic(DescriptorParseStatus::MalformedLength, tag, offset, declared_length, body.len(), body, "extended_event_descriptor items length overflows"));
        return None;
    };
    if items_end >= body.len() {
        out.diagnostics.push(descriptor_diagnostic(DescriptorParseStatus::MalformedLength, tag, offset, declared_length, body.len().saturating_sub(cursor), body, "extended_event_descriptor items length exceeds body or text length byte is missing"));
        return None;
    }
    let mut items = Vec::new();
    while cursor < items_end {
        if cursor + 1 > items_end {
            out.diagnostics.push(descriptor_diagnostic(DescriptorParseStatus::MalformedLength, tag, offset, declared_length, items_end.saturating_sub(cursor), body, "extended_event_descriptor item description length is truncated"));
            return None;
        }
        let desc_len = body[cursor] as usize;
        let desc_start = cursor + 1;
        let Some(desc_end) = desc_start.checked_add(desc_len) else { return None; };
        if desc_end >= items_end {
            out.diagnostics.push(descriptor_diagnostic(DescriptorParseStatus::MalformedLength, tag, offset, declared_length, items_end.saturating_sub(desc_start), body, "extended_event_descriptor item description length exceeds items area"));
            return None;
        }
        let item_len = body[desc_end] as usize;
        let item_start = desc_end + 1;
        let Some(item_end) = item_start.checked_add(item_len) else { return None; };
        if item_end > items_end {
            out.diagnostics.push(descriptor_diagnostic(DescriptorParseStatus::MalformedLength, tag, offset, declared_length, items_end.saturating_sub(item_start), body, "extended_event_descriptor item text length exceeds items area"));
            return None;
        }
        items.push((body[desc_start..desc_end].to_vec(), body[item_start..item_end].to_vec()));
        cursor = item_end;
    }
    let text_len_index = items_end;
    let text_len = body[text_len_index] as usize;
    let text_start = text_len_index + 1;
    let Some(text_end) = text_start.checked_add(text_len) else { return None; };
    if text_end != body.len() {
        out.diagnostics.push(descriptor_diagnostic(DescriptorParseStatus::MalformedLength, tag, offset, declared_length, body.len().saturating_sub(text_start), body, "extended_event_descriptor text length does not match body"));
        return None;
    }
    Some(ExtendedEventFragment { descriptor_number, last_descriptor_number, items, text: body[text_start..text_end].to_vec() })
}

fn parse_content_descriptor(body: &[u8], out: &mut EventDescriptors, tag: u8, offset: usize, declared_length: usize) {
    if body.len() % 2 != 0 {
        out.diagnostics.push(descriptor_diagnostic(DescriptorParseStatus::MalformedLength, tag, offset, declared_length, body.len(), body, "content_descriptor has trailing byte"));
        return;
    }
    out.contents.extend(body.chunks_exact(2).map(|chunk| {
        let level1 = chunk[0] >> 4;
        let level2 = chunk[0] & 0x0f;
        ContentDescriptorItem {
            content_nibble_level_1: level1,
            content_nibble_level_2: level2,
            user_nibble_1: chunk[1] >> 4,
            user_nibble_2: chunk[1] & 0x0f,
            arib_display_name: arib_content_to_display_name(level1, level2),
        }
    }));
}

fn arib_content_major_name(level1: u8) -> &'static str {
    match level1 {
        0x0 => "ニュース/報道",
        0x1 => "スポーツ",
        0x2 => "情報/ワイドショー",
        0x3 => "ドラマ",
        0x4 => "音楽",
        0x5 => "バラエティ",
        0x6 => "映画",
        0x7 => "アニメ/特撮",
        0x8 => "ドキュメンタリー/教養",
        0x9 => "劇場/公演",
        0xa => "趣味/教育",
        0xb => "福祉",
        _ => "その他",
    }
}

fn arib_content_minor_name(level1: u8, level2: u8) -> &'static str {
    match (level1, level2) {
        (0x0, 0x0) => "定時・総合",
        (0x0, 0x1) => "天気",
        (0x0, 0x2) => "特集・ドキュメント",
        (0x0, 0x3) => "政治・国会",
        (0x0, 0x4) => "経済・市況",
        (0x0, 0x5) => "海外・国際",
        (0x0, 0x6) => "解説",
        (0x0, 0x7) => "討論・会談",
        (0x0, 0x8) => "報道特番",
        (0x0, 0x9) => "ローカル・地域",
        (0x0, 0xa) => "交通",
        (0x1, 0x0) => "スポーツニュース",
        (0x1, 0x1) => "野球",
        (0x1, 0x2) => "サッカー",
        (0x1, 0x3) => "ゴルフ",
        (0x1, 0x4) => "その他の球技",
        (0x1, 0x5) => "相撲・格闘技",
        (0x1, 0x6) => "オリンピック・国際大会",
        (0x1, 0x7) => "マラソン・陸上・水泳",
        (0x1, 0x8) => "モータースポーツ",
        (0x1, 0x9) => "マリン・ウィンタースポーツ",
        (0x1, 0xa) => "競馬・公営競技",
        (0x2, 0x0) => "芸能・ワイドショー",
        (0x2, 0x1) => "ファッション",
        (0x2, 0x2) => "暮らし・住まい",
        (0x2, 0x3) => "健康・医療",
        (0x2, 0x4) => "ショッピング・通販",
        (0x2, 0x5) => "グルメ・料理",
        (0x2, 0x6) => "イベント",
        (0x2, 0x7) => "番組紹介・お知らせ",
        (0x3, 0x0) => "国内ドラマ",
        (0x3, 0x1) => "海外ドラマ",
        (0x3, 0x2) => "時代劇",
        (0x4, 0x0) => "国内ロック・ポップス",
        (0x4, 0x1) => "海外ロック・ポップス",
        (0x4, 0x2) => "クラシック・オペラ",
        (0x4, 0x3) => "ジャズ・フュージョン",
        (0x4, 0x4) => "歌謡曲・演歌",
        (0x4, 0x5) => "ライブ・コンサート",
        (0x4, 0x6) => "ランキング・リクエスト",
        (0x4, 0x7) => "カラオケ・のど自慢",
        (0x4, 0x8) => "民謡・邦楽",
        (0x4, 0x9) => "童謡・キッズ",
        (0x4, 0xa) => "民族音楽・ワールドミュージック",
        (0x5, 0x0) => "クイズ",
        (0x5, 0x1) => "ゲーム",
        (0x5, 0x2) => "トークバラエティ",
        (0x5, 0x3) => "お笑い・コメディ",
        (0x5, 0x4) => "音楽バラエティ",
        (0x5, 0x5) => "旅バラエティ",
        (0x5, 0x6) => "料理バラエティ",
        (0x6, 0x0) => "洋画",
        (0x6, 0x1) => "邦画",
        (0x6, 0x2) => "アニメ",
        (0x7, 0x0) => "国内アニメ",
        (0x7, 0x1) => "海外アニメ",
        (0x7, 0x2) => "特撮",
        (0x8, 0x0) => "社会・時事",
        (0x8, 0x1) => "歴史・紀行",
        (0x8, 0x2) => "自然・動物・環境",
        (0x8, 0x3) => "宇宙・科学・医学",
        (0x8, 0x4) => "カルチャー・伝統文化",
        (0x8, 0x5) => "文学・文芸",
        (0x8, 0x6) => "スポーツ",
        (0x8, 0x7) => "ドキュメンタリー全般",
        (0x8, 0x8) => "インタビュー・討論",
        (0x9, 0x0) => "現代劇・新劇",
        (0x9, 0x1) => "ミュージカル",
        (0x9, 0x2) => "ダンス・バレエ",
        (0x9, 0x3) => "落語・演芸",
        (0x9, 0x4) => "歌舞伎・古典",
        (0xa, 0x0) => "旅・釣り・アウトドア",
        (0xa, 0x1) => "園芸・ペット・手芸",
        (0xa, 0x2) => "音楽・美術・工芸",
        (0xa, 0x3) => "囲碁・将棋",
        (0xa, 0x4) => "麻雀・パチンコ",
        (0xa, 0x5) => "車・オートバイ",
        (0xa, 0x6) => "コンピュータ・TVゲーム",
        (0xa, 0x7) => "会話・語学",
        (0xa, 0x8) => "幼児・小学生",
        (0xa, 0x9) => "中学生・高校生",
        (0xa, 0xa) => "大学生・受験",
        (0xa, 0xb) => "生涯教育・資格",
        (0xa, 0xc) => "教育問題",
        (0xb, 0x0) => "高齢者",
        (0xb, 0x1) => "障害者",
        (0xb, 0x2) => "社会福祉",
        (0xb, 0x3) => "ボランティア",
        (0xb, 0x4) => "手話",
        (0xb, 0x5) => "文字（字幕）",
        (0xb, 0x6) => "音声解説",
        (_, 0xf) => "その他",
        _ => "未定義",
    }
}

fn arib_content_to_display_name(level1: u8, level2: u8) -> String {
    format!("{}/{}", arib_content_major_name(level1), arib_content_minor_name(level1, level2))
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
    if second_language && body.len() < 12 { return None; }
    let lang2 = if second_language {
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
        sampling_rate: (flags >> 1) & 0x07,
        language_code: language(&body[6..9]),
        language_code_2: lang2,
        text: decode_arib_string_lossy(body.get(cursor..).unwrap_or(&[])).trim().to_string(),
    })
}

fn parse_parental_rating_descriptor(body: &[u8], out: &mut EventDescriptors, tag: u8, offset: usize, declared_length: usize) -> ParentalRatingDescriptor {
    let parse_status = if body.len() % 4 != 0 {
        out.diagnostics.push(descriptor_diagnostic(DescriptorParseStatus::MalformedLength, tag, offset, declared_length, body.len(), body, "parental_rating_descriptor body length is not a multiple of 4"));
        DescriptorParseStatus::MalformedLength
    } else {
        DescriptorParseStatus::Ok
    };
    let entries = body.chunks_exact(4).map(|chunk| ParentalRating {
        country_code: language(&chunk[0..3]),
        rating_value: chunk[3],
        raw_rating_byte: chunk[3],
    }).collect();
    let mut raw_descriptor_bytes = Vec::with_capacity(body.len() + 2);
    raw_descriptor_bytes.push(tag);
    raw_descriptor_bytes.push(declared_length as u8);
    raw_descriptor_bytes.extend_from_slice(body);
    ParentalRatingDescriptor { entries, raw_descriptor_bytes, parse_status }
}

fn parse_series_descriptor(body: &[u8]) -> Option<SeriesDescriptor> {
    if body.len() < 9 { return None; }
    Some(SeriesDescriptor {
        series_id: u16_at(body, 0),
        repeat_label: (body[2] >> 4) & 0x0f,
        program_pattern: body[2] & 0x07,
        expire_date: u16::from_be_bytes([body[3] & 0x0f, body[4]]),
        episode_number: (((body[5] & 0x0f) as u16) << 8) | body[6] as u16,
        last_episode_number: (((body[7] & 0x0f) as u16) << 8) | body[8] as u16,
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
    if cursor != body.len() { return None; }
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

#[cfg(test)]
pub fn event_descriptor_diagnostics_array_json(desc: &EventDescriptors) -> String {
    event_descriptor_diagnostics_array_json_scoped(desc, None)
}

pub fn event_descriptor_diagnostics_array_json_scoped(desc: &EventDescriptors, scope: Option<DescriptorSectionScope>) -> String {
    let models = desc.diagnostics.iter().map(|d| descriptor_diagnostic_model(d, scope)).collect::<Vec<_>>();
    serde_json::to_string(&models).unwrap_or_else(|_| "[]".to_string())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DescriptorSectionScope {
    pub pid: Option<u16>,
    pub table_id: Option<u8>,
    pub table_id_extension: Option<u16>,
    pub version: Option<u8>,
    pub section_number: Option<u8>,
    pub original_network_id: Option<u16>,
    pub transport_stream_id: Option<u16>,
    pub service_id: Option<u16>,
    pub event_id: Option<u16>,
}

#[cfg(test)]
pub fn event_descriptors_to_json(desc: &EventDescriptors) -> String {
    let mut fields = Vec::new();
    fields.push("\"schemaVersion\":1".to_string());
    fields.push(format!("\"title\":\"{}\"", json_escape(&desc.title)));
    fields.push(format!("\"description\":\"{}\"", json_escape(&desc.description)));
    fields.push(format!("\"contents\":[{}]", desc.contents.iter().map(|c| format!("{{\"level1\":{},\"level2\":{},\"user1\":{},\"user2\":{},\"aribDisplayName\":\"{}\"}}", c.content_nibble_level_1, c.content_nibble_level_2, c.user_nibble_1, c.user_nibble_2, json_escape(&c.arib_display_name))).collect::<Vec<_>>().join(",")));
    fields.push(format!("\"parentalRatings\":[{}]", desc.parental_ratings.iter().map(|r| format!("{{\"country\":\"{}\",\"ratingValue\":{},\"rawRatingByte\":{}}}", json_escape(&r.country_code), r.rating_value, r.raw_rating_byte)).collect::<Vec<_>>().join(",")));
    fields.push(format!("\"parentalRatingDescriptors\":[{}]", desc.parental_rating_descriptors.iter().map(|p| format!("{{\"parseStatus\":\"{}\",\"rawDescriptorHex\":\"{}\",\"entryCount\":{}}}", p.parse_status.as_str(), hex(&p.raw_descriptor_bytes), p.entries.len())).collect::<Vec<_>>().join(",")));
    fields.push(format!("\"components\":[{}]", desc.components.iter().map(|c| format!("{{\"streamContent\":{},\"componentType\":{},\"componentTag\":{},\"language\":\"{}\",\"text\":\"{}\"}}", c.stream_content, c.component_type, c.component_tag, json_escape(&c.language_code), json_escape(&c.text))).collect::<Vec<_>>().join(",")));
    fields.push(format!("\"audioComponents\":[{}]", desc.audio_components.iter().map(|a| format!("{{\"streamContent\":{},\"componentType\":{},\"componentTag\":{},\"streamType\":{},\"simulcastGroupTag\":{},\"multiLingual\":{},\"main\":{},\"quality\":{},\"samplingRate\":{},\"language\":\"{}\",\"secondLanguage\":\"{}\",\"text\":\"{}\"}}", a.stream_content, a.component_type, a.component_tag, a.stream_type, a.simulcast_group_tag, a.es_multi_lingual_flag, a.main_component_flag, a.quality_indicator, a.sampling_rate, json_escape(&a.language_code), json_escape(a.language_code_2.as_deref().unwrap_or("")), json_escape(&a.text))).collect::<Vec<_>>().join(",")));
    fields.push(format!("\"series\":[{}]", desc.series.iter().map(|v| format!("{{\"seriesId\":{},\"repeatLabel\":{},\"programPattern\":{},\"expireDate\":{},\"episodeNumber\":{},\"lastEpisodeNumber\":{},\"name\":\"{}\"}}", v.series_id, v.repeat_label, v.program_pattern, v.expire_date, v.episode_number, v.last_episode_number, json_escape(&v.series_name))).collect::<Vec<_>>().join(",")));
    fields.push(format!("\"eventGroups\":[{}]", desc.event_groups.iter().map(event_group_to_json).collect::<Vec<_>>().join(",")));
    fields.push(format!("\"linkages\":[{}]", desc.linkages.iter().map(|l| format!("{{\"transportStreamId\":{},\"originalNetworkId\":{},\"serviceId\":{},\"linkageType\":{},\"privateDataHex\":\"{}\"}}", l.transport_stream_id, l.original_network_id, l.service_id, l.linkage_type, hex_prefix(&l.private_data, 32))).collect::<Vec<_>>().join(",")));
    fields.push(format!("\"extendedItems\":[{}]", desc.extended_items.iter().map(|i| format!("{{\"description\":\"{}\",\"text\":\"{}\"}}", json_escape(&i.item_description), json_escape(&i.item_text))).collect::<Vec<_>>().join(",")));
    fields.push(format!("\"unknownDescriptors\":[{}]", desc.unknown.iter().map(|(tag, body)| format!("{{\"tag\":{},\"length\":{},\"hexPrefix\":\"{}\",\"checksum\":{}}}", tag, body.len(), hex_prefix(body, 16), additive_checksum(body))).collect::<Vec<_>>().join(",")));
    fields.push(format!("\"diagnostics\":[{}]", desc.diagnostics.iter().map(descriptor_diagnostic_to_json).collect::<Vec<_>>().join(",")));
    fields.push(format!("\"malformedDescriptorCount\":{}", desc.diagnostics.len()));
    fields.push(format!("\"unknownDescriptorCount\":{}", desc.unknown.len()));
    format!("{{{}}}", fields.join(","))
}

#[cfg(test)]
fn descriptor_diagnostic_to_json(d: &DescriptorDiagnostic) -> String {
    descriptor_diagnostic_to_json_scoped(d, None)
}

fn descriptor_diagnostic_model(d: &DescriptorDiagnostic, scope: Option<DescriptorSectionScope>) -> DescriptorDiagnosticV1 {
    let s = scope.unwrap_or(DescriptorSectionScope {
        pid: None,
        table_id: None,
        table_id_extension: None,
        version: None,
        section_number: None,
        original_network_id: None,
        transport_stream_id: None,
        service_id: None,
        event_id: None,
    });
    DescriptorDiagnosticV1 {
        schema: "maleicacid.tv.descriptorDiagnostic".to_string(),
        schema_version: 1,
        severity: descriptor_diagnostic_severity(d.parse_status).to_string(),
        code: descriptor_diagnostic_code(d.parse_status).to_string(),
        scope: SectionScopeV1 {
            pid: s.pid.map(i64::from),
            table_id: s.table_id.map(i64::from),
            table_id_extension: s.table_id_extension.map(i64::from),
            version: s.version.map(i64::from),
            section_number: s.section_number.map(i64::from),
            original_network_id: s.original_network_id.map(i64::from),
            transport_stream_id: s.transport_stream_id.map(i64::from),
            service_id: s.service_id.map(i64::from),
            event_id: s.event_id.map(i64::from),
        },
        descriptor: DescriptorScopeV1 {
            tag: i64::from(d.descriptor_tag),
            name: None,
            offset: d.offset as i64,
            declared_length: d.declared_length as i64,
            actual_remaining_length: d.remaining_length as i64,
            parse_status: d.parse_status.as_str().to_string(),
            raw_prefix_hex: hex_prefix(&d.raw_prefix, 16),
        },
        message: d.message.clone(),
    }
}

fn descriptor_diagnostic_to_json_scoped(d: &DescriptorDiagnostic, scope: Option<DescriptorSectionScope>) -> String {
    let model = descriptor_diagnostic_model(d, scope);
    serde_json::to_string(&model).unwrap_or_else(|_| "{}".to_string())
}

fn descriptor_diagnostic_severity(status: DescriptorParseStatus) -> &'static str {
    match status {
        DescriptorParseStatus::Ok | DescriptorParseStatus::UnsupportedValue => "info",
        DescriptorParseStatus::MalformedLength | DescriptorParseStatus::TruncatedDescriptor | DescriptorParseStatus::InvalidSequence => "warning",
    }
}

fn descriptor_diagnostic_code(status: DescriptorParseStatus) -> &'static str {
    match status {
        DescriptorParseStatus::Ok => "OK",
        DescriptorParseStatus::MalformedLength => "MALFORMED_LENGTH",
        DescriptorParseStatus::TruncatedDescriptor => "TRUNCATED_DESCRIPTOR",
        DescriptorParseStatus::UnsupportedValue => "UNKNOWN_DESCRIPTOR",
        DescriptorParseStatus::InvalidSequence => "INVALID_SEQUENCE",
    }
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
            contents: vec![ContentDescriptorItem { content_nibble_level_1: 1, content_nibble_level_2: 2, user_nibble_1: 3, user_nibble_2: 4, arib_display_name: "スポーツ/サッカー".to_string() }],
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
            parental_ratings: vec![ParentalRating { country_code: "JPN".to_string(), rating_value: 15, raw_rating_byte: 15 }],
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
            contents: vec![ContentDescriptorItem { content_nibble_level_1: 1, content_nibble_level_2: 2, user_nibble_1: 0, user_nibble_2: 0, arib_display_name: "スポーツ/サッカー".to_string() }],
            ..EventDescriptors::default()
        };
        let provider = event_provider_fields(&descriptors);
        let diagnostic = event_descriptor_diagnostic(&descriptors);
        assert_eq!(provider.title, "番組");
        assert_eq!(provider.description, "説明");
        assert_eq!(diagnostic.content_count, 1);
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
        let first_text = [0x1b, b'(', b'B', b'A', b'B'];
        let second_text = [0x1b, b'(', b'B', b'C', b'D'];
        let mut first = vec![0x11, b'j', b'p', b'n', 0x00, first_text.len() as u8];
        first.extend_from_slice(&first_text);
        let mut second = vec![0x01, b'j', b'p', b'n', 0x00, second_text.len() as u8];
        second.extend_from_slice(&second_text);
        let mut bytes = descriptor(0x4e, &first);
        bytes.extend_from_slice(&descriptor(0x4e, &second));
        let parsed = parse_event_descriptors(&bytes);
        assert_eq!(parsed.description, "");
        assert_eq!(parsed.extended_description, "CDAB");
    }

    #[test]
    fn extended_event_item_fragments_continue_until_next_description() {
        let desc = [0x1b, b'(', b'B', b'A', b'B', b'C'];
        let first_text = [0x1b, b'(', b'B', b'D'];
        let second_text = [0x1b, b'(', b'B', b'E', b'F'];
        let mut first = vec![0x01, b'j', b'p', b'n', (1 + desc.len() + 1 + first_text.len()) as u8, desc.len() as u8];
        first.extend_from_slice(&desc);
        first.push(first_text.len() as u8);
        first.extend_from_slice(&first_text);
        first.push(0x00);
        let mut second = vec![0x11, b'j', b'p', b'n', (1 + 0 + 1 + second_text.len()) as u8, 0x00, second_text.len() as u8];
        second.extend_from_slice(&second_text);
        second.push(0x00);
        let mut bytes = descriptor(0x4e, &first);
        bytes.extend_from_slice(&descriptor(0x4e, &second));
        let parsed = parse_event_descriptors(&bytes);
        assert_eq!(parsed.extended_items.len(), 1);
        assert_eq!(parsed.extended_items[0].item_description, "ABC");
        assert_eq!(parsed.extended_items[0].item_text, "DEF");
    }
    #[test]
    fn audio_component_sampling_rate_ignores_reserved_lsb() {
        let body = [0x20, 0x03, 0x08, 0x0f, 0x00, 0b1101_1111, b'j', b'p', b'n'];
        let parsed = parse_audio_component_descriptor(&body).unwrap();
        assert_eq!(parsed.quality_indicator, 1);
        assert_eq!(parsed.sampling_rate, 7);
    }

    #[test]
    fn parental_rating_keeps_full_rating_byte_and_reports_bad_length() {
        let desc = parse_event_descriptors(&[0x55, 0x05, b'J', b'P', b'N', 0x8f, 0xaa]);
        assert_eq!(desc.parental_ratings[0].rating_value, 0x8f);
        assert_eq!(desc.parental_ratings[0].raw_rating_byte, 0x8f);
        assert!(desc.diagnostics.iter().any(|d| d.parse_status == DescriptorParseStatus::MalformedLength));
    }

    #[test]
    fn series_descriptor_requires_complete_nine_byte_fixed_part() {
        let desc = parse_event_descriptors(&[0xd5, 0x08, 0x12, 0x34, 0x10, 0x0f, 0xff, 0x01, 0x02, 0x03]);
        assert!(desc.series.is_empty());
        assert!(desc.diagnostics.iter().any(|d| d.descriptor_tag == 0xd5));
    }

    #[test]
    fn malformed_descriptor_loop_is_recorded_in_diagnostics() {
        let desc = parse_event_descriptors(&[0x4d, 0x10, b'j', b'p']);
        assert!(desc.diagnostics.iter().any(|d| d.parse_status == DescriptorParseStatus::TruncatedDescriptor));
    }

}


#[cfg(test)]
mod r51_descriptor_coverage_tests {
    use super::*;

    fn descriptor(tag: u8, body: &[u8]) -> Vec<u8> {
        let mut out = vec![tag, body.len() as u8];
        out.extend_from_slice(body);
        out
    }

    fn arib_alnum(text: &[u8]) -> Vec<u8> {
        let mut out = vec![0x1b, b'(', b'B'];
        out.extend_from_slice(text);
        out
    }

    #[test]
    fn short_event_descriptor_sets_title_and_description() {
        let title = arib_alnum(b'AB');
        let text = arib_alnum(b'CD');
        let mut body = b"jpn".to_vec();
        body.push(title.len() as u8);
        body.extend_from_slice(&title);
        body.push(text.len() as u8);
        body.extend_from_slice(&text);
        let parsed = parse_event_descriptors(&descriptor(0x4d, &body));
        assert_eq!(parsed.title, "AB");
        assert_eq!(parsed.description, "CD");
    }

    #[test]
    fn content_descriptor_keeps_arib_classification_and_display_name() {
        let parsed = parse_event_descriptors(&descriptor(0x54, &[0x12, 0x34]));
        assert_eq!(parsed.contents.len(), 1);
        assert_eq!(parsed.contents[0].content_nibble_level_1, 1);
        assert_eq!(parsed.contents[0].content_nibble_level_2, 2);
        assert_eq!(parsed.contents[0].user_nibble_1, 3);
        assert_eq!(parsed.contents[0].user_nibble_2, 4);
        assert!(parsed.contents[0].arib_display_name.contains("スポーツ"));
    }

    #[test]
    fn component_descriptor_keeps_language_and_text() {
        let mut body = vec![0x11, 0xb3, 0x07, b'j', b'p', b'n'];
        body.extend_from_slice(&arib_alnum(b'V'));
        let parsed = parse_event_descriptors(&descriptor(0x50, &body));
        assert_eq!(parsed.components.len(), 1);
        assert_eq!(parsed.components[0].stream_content, 1);
        assert_eq!(parsed.components[0].component_type, 0xb3);
        assert_eq!(parsed.components[0].component_tag, 7);
        assert_eq!(parsed.components[0].language_code, "jpn");
        assert_eq!(parsed.components[0].text, "V");
    }

    #[test]
    fn event_group_descriptor_keeps_same_network_reference() {
        let body = [0x11, 0x00, 0x65, 0x01, 0x23];
        let parsed = parse_event_descriptors(&descriptor(0xd6, &body));
        assert_eq!(parsed.event_groups.len(), 1);
        assert_eq!(parsed.event_groups[0].group_type, 1);
        assert_eq!(parsed.event_groups[0].events[0].service_id, 101);
        assert_eq!(parsed.event_groups[0].events[0].event_id, 0x0123);
    }

    #[test]
    fn linkage_descriptor_keeps_private_data() {
        let body = [0x00, 0x11, 0x00, 0x22, 0x00, 0x65, 0x0d, 0xaa, 0xbb];
        let parsed = parse_event_descriptors(&descriptor(0x4a, &body));
        assert_eq!(parsed.linkages.len(), 1);
        assert_eq!(parsed.linkages[0].transport_stream_id, 0x0011);
        assert_eq!(parsed.linkages[0].original_network_id, 0x0022);
        assert_eq!(parsed.linkages[0].service_id, 101);
        assert_eq!(parsed.linkages[0].linkage_type, 0x0d);
        assert_eq!(parsed.linkages[0].private_data, vec![0xaa, 0xbb]);
    }

    #[test]
    fn unknown_descriptor_is_preserved_for_diagnostics() {
        let parsed = parse_event_descriptors(&descriptor(0xfe, &[0x12, 0x34, 0x56]));
        assert_eq!(parsed.unknown, vec![(0xfe, vec![0x12, 0x34, 0x56])]);
        assert!(parsed.diagnostics.iter().any(|d| d.descriptor_tag == 0xfe && d.parse_status == DescriptorParseStatus::UnsupportedValue));
        let json = event_descriptors_to_json(&parsed);
        assert!(json.contains("unknownDescriptors"));
        assert!(json.contains("\"schema\":\"maleicacid.tv.descriptorDiagnostic\""));
        assert!(json.contains("\"code\":\"UNKNOWN_DESCRIPTOR\""));
    }


    #[test]
    fn parental_rating_descriptor_diagnostics_keep_android_rating_domain_out_of_rust_output() {
        let parsed = parse_event_descriptors(&descriptor(0x55, &[b'J', b'P', b'N', 12]));
        assert_eq!(parsed.parental_ratings.len(), 1);
        let json = event_descriptors_to_json(&parsed);
        assert!(json.contains("\"country\":\"JPN\""));
        assert!(json.contains("\"ratingValue\":12"));
        let android_domain = format!("{}.{}", "com.android", "tv");
        let isdb_rating = format!("{}{}", "IS", "DB_12");
        let old_rating_system = format!("{}{}", "AR", "IB_JP");
        let old_rating = format!("{}{}", "AG", "E_12");
        assert!(!json.contains(android_domain.as_str()));
        assert!(!json.contains(isdb_rating.as_str()));
        assert!(!json.contains(old_rating_system.as_str()));
        assert!(!json.contains(old_rating.as_str()));
    }

    #[test]
    fn malformed_and_truncated_parental_rating_descriptors_are_diagnostics_not_android_projection() {
        let malformed = parse_event_descriptors(&descriptor(0x55, &[b'J', b'P', b'N', 12, 0xaa]));
        assert!(malformed.diagnostics.iter().any(|d| d.parse_status == DescriptorParseStatus::MalformedLength));
        let malformed_json = event_descriptors_to_json(&malformed);
        assert!(malformed_json.contains("MalformedLength"));
        let android_domain = format!("{}.{}", "com.android", "tv");
        let isdb_rating = format!("{}{}", "IS", "DB_12");
        assert!(!malformed_json.contains(android_domain.as_str()));
        assert!(!malformed_json.contains(isdb_rating.as_str()));

        let truncated = parse_event_descriptors(&[0x55, 0x04, b'J', b'P']);
        assert!(truncated.diagnostics.iter().any(|d| d.parse_status == DescriptorParseStatus::TruncatedDescriptor));
        let truncated_json = event_descriptors_to_json(&truncated);
        assert!(truncated_json.contains("TruncatedDescriptor"));
        let isdb_prefix = format!("{}{}", "IS", "DB_");
        assert!(!truncated_json.contains(android_domain.as_str()));
        assert!(!truncated_json.contains(isdb_prefix.as_str()));
    }


    #[test]
    fn malformed_short_event_does_not_project_title_or_description() {
        let mut body = b"jpn".to_vec();
        body.push(10);
        body.extend_from_slice(&arib_alnum(b"A"));
        let parsed = parse_event_descriptors(&descriptor(0x4d, &body));
        assert!(parsed.title.is_empty());
        assert!(parsed.description.is_empty());
        assert!(parsed.diagnostics.iter().any(|d| d.parse_status == DescriptorParseStatus::MalformedLength && d.descriptor_tag == 0x4d));
    }

    #[test]
    fn malformed_extended_event_sequence_does_not_project_text_or_items() {
        let text = arib_alnum(b"AB");
        let mut first = vec![0x00, b'j', b'p', b'n', 0x00, text.len() as u8];
        first.extend_from_slice(&text);
        let mut second = vec![0x20, b'j', b'p', b'n', 0x00, text.len() as u8];
        second.extend_from_slice(&text);
        let mut bytes = descriptor(0x4e, &first);
        bytes.extend_from_slice(&descriptor(0x4e, &second));
        let parsed = parse_event_descriptors(&bytes);
        assert!(parsed.extended_description.is_empty());
        assert!(parsed.extended_items.is_empty());
        assert!(parsed.diagnostics.iter().any(|d| d.parse_status == DescriptorParseStatus::InvalidSequence));
    }

    #[test]
    fn malformed_content_audio_and_event_group_are_not_partially_projected() {
        let parsed = parse_event_descriptors(&descriptor(0x54, &[0x12]));
        assert!(parsed.contents.is_empty());
        assert!(parsed.diagnostics.iter().any(|d| d.descriptor_tag == 0x54 && d.parse_status == DescriptorParseStatus::MalformedLength));

        let parsed = parse_event_descriptors(&descriptor(0xc4, &[0x20, 0x03, 0x08, 0x0f, 0x00, 0x80, b'j', b'p', b'n']));
        assert!(parsed.audio_components.is_empty());
        assert!(parsed.diagnostics.iter().any(|d| d.descriptor_tag == 0xc4 && d.parse_status == DescriptorParseStatus::MalformedLength));

        let parsed = parse_event_descriptors(&descriptor(0xd6, &[0x10, 0x00]));
        assert!(parsed.event_groups.is_empty());
        assert!(parsed.diagnostics.iter().any(|d| d.descriptor_tag == 0xd6 && d.parse_status == DescriptorParseStatus::MalformedLength));
    }

    #[test]
    fn parental_rating_raw_descriptor_hex_includes_tag_and_declared_length() {
        let parsed = parse_event_descriptors(&descriptor(0x55, &[b'J', b'P', b'N', 12]));
        let json = event_descriptors_to_json(&parsed);
        assert!(json.contains("\"rawDescriptorHex\":\"55044a504e0c\""));
    }

    #[test]
    fn descriptor_diagnostic_json_uses_v1_schema_shape() {
        let mut body = b"jpn".to_vec();
        body.push(10);
        body.extend_from_slice(&arib_alnum(b"A"));
        let parsed = parse_event_descriptors(&descriptor(0x4d, &body));
        let json = event_descriptors_to_json(&parsed);
        assert!(json.contains("\"schemaVersion\":1"));
        assert!(json.contains("\"diagnostics\":["));
        assert!(json.contains("\"schema\":\"maleicacid.tv.descriptorDiagnostic\""));
        assert!(json.contains("\"schemaVersion\":1"));
        assert!(json.contains("\"severity\":\"warning\""));
        assert!(json.contains("\"code\":\"MALFORMED_LENGTH\""));
        assert!(json.contains("\"descriptor\":{"));
        assert!(json.contains("\"parseStatus\":\"MalformedLength\""));
        assert!(json.contains("\"tag\":77"));
        assert!(json.contains("\"rawPrefixHex\":\"4d06"));
        assert!(json.contains("\"actualRemainingLength\":"));
        assert!(!json.contains("diagnosticCode"));
        assert!(!json.contains("descriptorOffset"));
    }

}

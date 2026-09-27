use crate::ca_descriptor::{CaDescriptor, MalformedCaDescriptorDiagnostic};
use crate::descriptors::{
    event_descriptor_diagnostics_scoped, event_provider_fields, ComponentGroupDescriptor,
    DescriptorParseStatus, DescriptorSectionScope, EventDescriptors, EventGroupDescriptor,
    LinkageDescriptor, ParentalRatingDescriptor, SeriesDescriptor,
};
use crate::eit::{EitEvent, EitTimingState};
use crate::eit_instances::EitInstanceState;
use crate::provider_data::{
    CasFactsV1, DescriptorDiagnosticV1, DescriptorFactsV1, ParentalRatingDescriptorV1, RatingV1,
    RawDescriptorV1,
};
use crate::runtime_snapshot_dto::*;
use crate::service_discovery::{
    BroadcastSystem, DiscoveredElementaryStream, DiscoveredTransport, ServiceSemanticFacts,
    SmdSemanticState, TableRequirementStatus,
};

fn hex(bytes: &[u8]) -> String {
    crate::ca_descriptor::hex_prefix(bytes, bytes.len())
}

fn hex_prefix(bytes: &[u8], maximum: usize) -> String {
    crate::ca_descriptor::hex_prefix(bytes, maximum)
}

fn parse_status(status: DescriptorParseStatus) -> SiParseStatusDto {
    match status {
        DescriptorParseStatus::Ok => SiParseStatusDto::Ok,
        DescriptorParseStatus::MalformedLength => SiParseStatusDto::MalformedLength,
        DescriptorParseStatus::TruncatedDescriptor => SiParseStatusDto::TruncatedDescriptor,
        DescriptorParseStatus::UnsupportedValue => SiParseStatusDto::UnsupportedValue,
        DescriptorParseStatus::InvalidSequence => SiParseStatusDto::InvalidSequence,
    }
}

fn timing_state(state: EitTimingState) -> EitTimingStateDto {
    match state {
        EitTimingState::Defined => EitTimingStateDto::Defined,
        EitTimingState::UndefinedTime => EitTimingStateDto::UndefinedTime,
        EitTimingState::BothTimingUndefined => EitTimingStateDto::BothTimingUndefined,
        EitTimingState::MalformedTiming => EitTimingStateDto::MalformedTiming,
    }
}

fn broadcast_system(system: BroadcastSystem) -> BroadcastSystemDto {
    match system {
        BroadcastSystem::IsdbT => BroadcastSystemDto::IsdbT,
        BroadcastSystem::IsdbSBs => BroadcastSystemDto::IsdbSBs,
        BroadcastSystem::IsdbS110Cs => BroadcastSystemDto::IsdbS110Cs,
    }
}

fn smd_state(state: SmdSemanticState) -> SmdSemanticStateDto {
    match state {
        SmdSemanticState::SupportedBroadcast => SmdSemanticStateDto::SupportedBroadcast,
        SmdSemanticState::NonBroadcast => SmdSemanticStateDto::NonBroadcast,
        SmdSemanticState::UndefinedBroadcastClass => SmdSemanticStateDto::UndefinedBroadcastClass,
        SmdSemanticState::UnsupportedBroadcastSystem => {
            SmdSemanticStateDto::UnsupportedBroadcastSystem
        }
        SmdSemanticState::UndeterminedSmd => SmdSemanticStateDto::UndeterminedSmd,
    }
}

fn ca_scope(scope: &str) -> CaDescriptorScopeDto {
    if scope == "ES" {
        CaDescriptorScopeDto::Es
    } else {
        CaDescriptorScopeDto::Program
    }
}

fn ca_source(source: &str) -> CaMetadataSourceDto {
    match source {
        "CAT" => CaMetadataSourceDto::Cat,
        "ELEMENTARY_STREAM" => CaMetadataSourceDto::ElementaryStream,
        _ => CaMetadataSourceDto::Program,
    }
}

pub fn service_key(
    original_network_id: u16,
    transport_stream_id: u16,
    service_id: u16,
) -> ServiceKeyDto {
    ServiceKeyDto {
        original_network_id: i32::from(original_network_id),
        transport_stream_id: i32::from(transport_stream_id),
        service_id: i32::from(service_id),
    }
}

pub fn table_requirement(value: &TableRequirementStatus) -> TableRequirementDto {
    TableRequirementDto {
        component: value.component.to_string(),
        original_network_id: value.original_network_id.map(i32::from),
        transport_stream_id: value.transport_stream_id.map(i32::from),
        service_id: value.service_id.map(i32::from),
        required: value.required,
        complete: value.complete,
    }
}

pub fn ca_metadata(
    key: Option<ServiceKeyDto>,
    ca: &CaDescriptor,
    ecm_pid: Option<u16>,
    emm_pid: Option<u16>,
    elementary_pid: Option<u16>,
    source: &str,
) -> CaMetadataDto {
    CaMetadataDto {
        service_key: key,
        ca_system_id: i32::from(ca.ca_system_id),
        ecm_pid: ecm_pid.map(i32::from),
        emm_pid: emm_pid.map(i32::from),
        elementary_pid: elementary_pid.map(i32::from),
        private_data_hex: hex(&ca.private_data),
        source: ca_source(source),
    }
}

pub fn malformed_ca(value: &MalformedCaDescriptorDiagnostic) -> MalformedCaDescriptorDiagnosticDto {
    MalformedCaDescriptorDiagnosticDto {
        pid: i32::from(value.pid),
        table_id: i32::from(value.table_id),
        table_id_extension: value.table_id_extension.map(i32::from),
        service_id: value.service_id.map(i32::from),
        elementary_pid: value.elementary_pid.map(i32::from),
        scope: value.scope.to_string(),
        offset: i32::try_from(value.offset).unwrap_or(i32::MAX),
        declared_length: i32::try_from(value.declared_length).unwrap_or(i32::MAX),
        actual_remaining_length: i32::try_from(value.actual_remaining_length).unwrap_or(i32::MAX),
        reason: value.reason.to_string(),
        raw_prefix_hex: value.raw_prefix_hex.clone(),
    }
}

pub fn transport(value: &DiscoveredTransport, sdt_actual: bool) -> TransportSemanticFactsDto {
    TransportSemanticFactsDto {
        original_network_id: i32::from(value.original_network_id),
        transport_stream_id: i32::from(value.transport_stream_id),
        network_name: value.network_name.clone(),
        transport_stream_name: value.ts_name.clone(),
        remote_control_key_id: value.remote_control_key_id.map(i32::from),
        sdt_actual,
    }
}

fn codec_facts(stream: &DiscoveredElementaryStream) -> CodecFactsDto {
    let avc = stream.codec_facts.avc.map(|value| AvcSignalingDto {
        profile_idc: i32::from(value.profile_idc),
        constraint_flags: i32::from(value.constraint_flags),
        level_idc: i32::from(value.level_idc),
    });
    let (audio_config_hex, audio_config_header) = stream
        .codec_facts
        .audio_extension
        .as_ref()
        .map(|extension| {
            let header = extension.header.map(|header| AudioConfigHeaderDto {
                audio_object_type: i32::from(header.audio_object_type),
                sampling_frequency: i32::try_from(header.sampling_frequency).unwrap_or(i32::MAX),
                channel_configuration: i32::from(header.channel_configuration),
                extension_sampling_frequency: header
                    .extension_sampling_frequency
                    .and_then(|value| i32::try_from(value).ok()),
                core_audio_object_type: header.core_audio_object_type.map(i32::from),
                channel_count: header.channel_count.map(i32::from),
            });
            (extension.audio_specific_config_hex.clone(), header)
        })
        .unwrap_or((None, None));
    CodecFactsDto {
        avc,
        audio_config_hex,
        audio_config_header,
        raw_descriptors_hex: (!stream.codec_facts.raw_descriptors_hex.is_empty())
            .then(|| stream.codec_facts.raw_descriptors_hex.clone()),
        profile_level: stream.codec_facts.profile_level(),
        resolved: stream.codec_facts.is_resolved(),
    }
}

fn elementary_stream(value: &DiscoveredElementaryStream) -> ElementaryStreamDto {
    let (codec_kind, codec) = value
        .codec_signaling()
        .map(|(kind, codec)| {
            let kind = if kind == "VIDEO" {
                ElementaryStreamKindDto::Video
            } else {
                ElementaryStreamKindDto::Audio
            };
            (Some(kind), Some(codec.to_string()))
        })
        .unwrap_or((None, None));
    ElementaryStreamDto {
        elementary_pid: i32::from(value.elementary_pid),
        stream_type: i32::from(value.stream_type),
        component_tag: value.component_tag.map(i32::from),
        component_type: value.component_type.map(i32::from),
        stream_content: value.stream_content.map(i32::from),
        language_codes: value.language_codes.clone(),
        data_component_id: value.data_component_id.map(i32::from),
        caption_dmf: value.caption_dmf.map(i32::from),
        caption_timing: value.caption_timing.map(i32::from),
        automatic_presentation_on_reception: value.automatic_presentation_on_reception,
        is_caption: value.is_caption,
        is_superimpose: value.is_superimpose,
        codec,
        codec_kind,
        codec_facts: codec_facts(value),
    }
}

fn service_ca_descriptor(
    ca: &CaDescriptor,
    scope: &str,
    es_pid: Option<u16>,
) -> ServiceCaDescriptorDto {
    ServiceCaDescriptorDto {
        ca_system_id: i32::from(ca.ca_system_id),
        ca_pid: i32::from(ca.ca_pid),
        scope: ca_scope(scope),
        es_pid: es_pid.map(i32::from),
        raw_descriptor_hex: hex(&ca.raw_descriptor),
        private_data_hex: hex(&ca.private_data),
    }
}

pub fn service_semantic_facts(value: &ServiceSemanticFacts) -> ServiceSemanticFactsDto {
    let mut ca = value
        .program_ca_descriptors
        .iter()
        .map(|descriptor| service_ca_descriptor(descriptor, "PROGRAM", None))
        .collect::<Vec<_>>();
    for group in &value.es_ca_descriptors {
        ca.extend(
            group.descriptors.iter().map(|descriptor| {
                service_ca_descriptor(descriptor, "ES", Some(group.elementary_pid))
            }),
        );
    }
    ServiceSemanticFactsDto {
        original_network_id: i32::from(value.original_network_id),
        transport_stream_id: i32::from(value.transport_stream_id),
        service_id: i32::from(value.service_id),
        service_type: value.service_type.map(i32::from),
        pmt_pid_resolved: value.pmt_pid_resolved,
        pmt_parsed: value.pmt_parsed,
        pcr_pid_resolved: value.pcr_pid_resolved,
        elementary_streams: value
            .elementary_streams
            .iter()
            .map(elementary_stream)
            .collect(),
        requires_cas: value.requires_cas,
        cas_facts_canonical_json: serde_json::to_string(&CasFactsV1::from(value)).ok(),
        ca_descriptors_resolved: value.ca_descriptors_resolved,
        free_ca_mode: value.free_ca_mode,
        smd: SmdSemanticFactsDto {
            descriptor_present: value.system_management.descriptor_present,
            syntax_valid: value.system_management.syntax_valid,
            system_management_id: value.system_management.system_management_id.map(i32::from),
            broadcasting_flag: value.system_management.broadcasting_flag.map(i32::from),
            broadcasting_identifier: value
                .system_management
                .broadcasting_identifier
                .map(i32::from),
            broadcast_system: value
                .system_management
                .broadcast_system
                .map(broadcast_system),
            additional_broadcasting_identification: value
                .system_management
                .additional_broadcasting_identification
                .map(i32::from),
            additional_identification_info_hex: hex(&value
                .system_management
                .additional_identification_info),
            semantic_state: smd_state(value.system_management.semantic_state),
            diagnostic: value.system_management.diagnostic.map(str::to_string),
        },
        missing_components: value
            .missing_components
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
        semantic_diagnostics: value
            .semantic_diagnostics
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
        name: value.name.clone(),
        provider_name: value.provider_name.clone(),
        pmt_pid: value.pmt_pid.map(i32::from),
        pcr_pid: value.pcr_pid.map(i32::from),
        service_scoped_ca_descriptors: ca,
    }
}

pub fn eit_instance(value: &EitInstanceState) -> EitInstanceDto {
    EitInstanceDto {
        original_network_id: i32::from(value.original_network_id),
        transport_stream_id: i32::from(value.transport_stream_id),
        service_id: i32::from(value.service_id),
        table_id: i32::from(value.table_id),
        version: i32::from(value.version),
        current_next_indicator: value.current_next_indicator,
        last_section_number: i32::from(value.last_section_number),
        received_sections: value
            .received_sections
            .iter()
            .copied()
            .map(i32::from)
            .collect(),
        missing_sections: value
            .missing_sections
            .iter()
            .copied()
            .map(i32::from)
            .collect(),
        safe_sections: value.safe_sections.iter().copied().map(i32::from).collect(),
        complete: value.complete,
        inconsistent: value.inconsistent,
    }
}

fn descriptor_diagnostic(value: DescriptorDiagnosticV1) -> DescriptorDiagnosticDto {
    DescriptorDiagnosticDto {
        schema: value.schema,
        schema_version: i32::try_from(value.schema_version).unwrap_or(i32::MAX),
        severity: value.severity,
        code: value.code,
        scope: DescriptorDiagnosticScopeDto {
            pid: value.scope.pid.and_then(|value| i32::try_from(value).ok()),
            table_id: value
                .scope
                .table_id
                .and_then(|value| i32::try_from(value).ok()),
            table_id_extension: value
                .scope
                .table_id_extension
                .and_then(|value| i32::try_from(value).ok()),
            version: value
                .scope
                .version
                .and_then(|value| i32::try_from(value).ok()),
            section_number: value
                .scope
                .section_number
                .and_then(|value| i32::try_from(value).ok()),
            original_network_id: value
                .scope
                .original_network_id
                .and_then(|value| i32::try_from(value).ok()),
            transport_stream_id: value
                .scope
                .transport_stream_id
                .and_then(|value| i32::try_from(value).ok()),
            service_id: value
                .scope
                .service_id
                .and_then(|value| i32::try_from(value).ok()),
            event_id: value
                .scope
                .event_id
                .and_then(|value| i32::try_from(value).ok()),
        },
        descriptor: DescriptorDiagnosticDescriptorDto {
            tag: i32::try_from(value.descriptor.tag).unwrap_or(i32::MAX),
            name: value.descriptor.name,
            offset: i32::try_from(value.descriptor.offset).unwrap_or(i32::MAX),
            declared_length: i32::try_from(value.descriptor.declared_length).unwrap_or(i32::MAX),
            actual_remaining_length: i32::try_from(value.descriptor.actual_remaining_length)
                .unwrap_or(i32::MAX),
            parse_status: value.descriptor.parse_status,
            raw_prefix_hex: value.descriptor.raw_prefix_hex,
        },
        message: value.message,
    }
}

fn rating_entries(descriptor: &ParentalRatingDescriptor) -> Vec<RatingV1> {
    descriptor
        .entries
        .iter()
        .map(|rating| RatingV1 {
            country_code: rating.country_code.clone(),
            raw_rating_byte: i64::from(rating.raw_rating_byte),
            parse_status: descriptor.parse_status.as_str().to_string(),
        })
        .collect()
}

fn descriptor_facts(event: &EitEvent) -> DescriptorFactsV1 {
    DescriptorFactsV1 {
        parental_rating_descriptors: event
            .descriptors
            .parental_rating_descriptors
            .iter()
            .map(|descriptor| ParentalRatingDescriptorV1 {
                entries: rating_entries(descriptor),
                raw_descriptor_hex: hex(&descriptor.raw_descriptor_bytes),
                parse_status: descriptor.parse_status.as_str().to_string(),
            })
            .collect(),
        unknown_descriptors: event
            .descriptors
            .unknown
            .iter()
            .map(|(tag, body)| {
                let mut raw = vec![*tag, body.len() as u8];
                raw.extend_from_slice(body);
                RawDescriptorV1 {
                    tag: i64::from(*tag),
                    raw_descriptor_hex: hex(&raw),
                }
            })
            .collect(),
    }
}

fn short_events(desc: &EventDescriptors) -> Vec<ShortEventDto> {
    desc.short_events
        .iter()
        .map(|value| ShortEventDto {
            language_code: value.language_code.clone(),
            title: value.title.clone(),
            text: value.text.clone(),
            parse_status: SiParseStatusDto::Ok,
        })
        .collect()
}

fn extended_texts(desc: &EventDescriptors) -> Vec<ExtendedTextDto> {
    desc.extended_texts
        .iter()
        .map(|value| ExtendedTextDto {
            language_code: value.language_code.clone(),
            text: value.text.clone(),
            parse_status: SiParseStatusDto::Ok,
        })
        .collect()
}

fn extended_items(desc: &EventDescriptors) -> Vec<ExtendedItemDto> {
    desc.extended_items
        .iter()
        .map(|value| ExtendedItemDto {
            language_code: value.language_code.clone(),
            description: value.item_description.clone(),
            text: value.item_text.clone(),
        })
        .collect()
}

fn series(value: &SeriesDescriptor) -> SeriesDto {
    SeriesDto {
        series_id: Some(i32::from(value.series_id)),
        repeat_label: i32::from(value.repeat_label),
        program_pattern: i32::from(value.program_pattern),
        expire_date_valid: value.expire_date_valid,
        expire_date: value
            .expire_date_valid
            .then(|| i32::from(value.expire_date)),
        episode_number: Some(i32::from(value.episode_number)),
        last_episode_number: Some(i32::from(value.last_episode_number)),
        name: (!value.series_name.is_empty()).then(|| value.series_name.clone()),
        parse_status: SiParseStatusDto::Ok,
    }
}

fn event_group(value: &EventGroupDescriptor) -> EventGroupDto {
    EventGroupDto {
        group_type: i32::from(value.group_type),
        events: value
            .events
            .iter()
            .map(|reference| EventGroupReferenceDto {
                service_id: i32::from(reference.service_id),
                event_id: i32::from(reference.event_id),
            })
            .collect(),
        other_network_events: value
            .other_network_events
            .iter()
            .map(|reference| OtherNetworkEventGroupReferenceDto {
                original_network_id: i32::from(reference.original_network_id),
                transport_stream_id: i32::from(reference.transport_stream_id),
                service_id: i32::from(reference.service_id),
                event_id: i32::from(reference.event_id),
            })
            .collect(),
        private_data_hex: hex(&value.private_data),
        parse_status: SiParseStatusDto::Ok,
    }
}

fn component_group(value: &ComponentGroupDescriptor) -> ComponentGroupDescriptorDto {
    ComponentGroupDescriptorDto {
        component_group_type: i32::from(value.component_group_type),
        groups: value
            .groups
            .iter()
            .map(|group| ComponentGroupDto {
                component_group_id: i32::from(group.component_group_id),
                component_tags: group
                    .component_tags
                    .iter()
                    .copied()
                    .map(i32::from)
                    .collect(),
            })
            .collect(),
        parse_status: SiParseStatusDto::Ok,
    }
}

fn linkage(value: &LinkageDescriptor) -> LinkageDto {
    LinkageDto {
        linkage_type: i32::from(value.linkage_type),
        original_network_id: i32::from(value.original_network_id),
        transport_stream_id: i32::from(value.transport_stream_id),
        service_id: i32::from(value.service_id),
        private_data_prefix_hex: hex_prefix(&value.private_data, 16),
        parse_status: SiParseStatusDto::Ok,
    }
}

fn video_component(value: &crate::descriptors::ComponentDescriptor) -> VideoComponentDto {
    let (resolution, scan) = match (value.stream_content, value.component_type) {
        (0x01, 0x01..=0x04) => (Some("480"), Some("interlaced")),
        (0x01, 0xa1..=0xa4) => (Some("480"), Some("progressive")),
        (0x01, 0xb1..=0xb4) => (Some("1080"), Some("interlaced")),
        (0x01, 0xc1..=0xc4) => (Some("720"), Some("progressive")),
        (0x01, 0xd1..=0xd4) => (Some("240"), Some("progressive")),
        _ => (None, None),
    };
    let aspect = resolution.and(match value.component_type & 0x0f {
        0x01 => Some("4:3"),
        0x02 | 0x03 => Some("16:9"),
        0x04 => Some(">16:9"),
        _ => None,
    });
    VideoComponentDto {
        stream_content: Some(i32::from(value.stream_content)),
        component_tag: Some(i32::from(value.component_tag)),
        component_type: Some(i32::from(value.component_type)),
        language: Some(value.language_code.clone()),
        text: Some(value.text.clone()),
        source_descriptor: Some("component_descriptor".to_string()),
        resolution: resolution.map(str::to_string),
        scan: scan.map(str::to_string),
        aspect: aspect.map(str::to_string),
        profile_level: None,
        parse_status: SiParseStatusDto::Ok,
    }
}

fn audio_channel_configuration(stream_content: u8, component_type: u8) -> Option<&'static str> {
    if stream_content != 0x02 {
        return None;
    }
    match component_type & 0x1f {
        0x01 => Some("1/0"),
        0x02 => Some("1/0+1/0"),
        0x03 => Some("2/0"),
        0x04 => Some("2/1"),
        0x05 => Some("3/0"),
        0x06 => Some("2/2"),
        0x07 => Some("3/1"),
        0x08 => Some("3/2"),
        0x09 => Some("3/2+LFE"),
        _ => None,
    }
}

fn audio_channel_count(stream_content: u8, component_type: u8) -> Option<i32> {
    if stream_content != 0x02 {
        return None;
    }
    match component_type & 0x1f {
        0x01 => Some(1),
        0x02 | 0x03 => Some(2),
        0x04 | 0x05 => Some(3),
        0x06 | 0x07 => Some(4),
        0x08 => Some(5),
        0x09 => Some(6),
        _ => None,
    }
}

fn audio_sample_rate_hz(value: u8) -> Option<i32> {
    match value {
        0x01 => Some(16_000),
        0x02 => Some(22_050),
        0x03 => Some(24_000),
        0x05 => Some(32_000),
        0x06 => Some(44_100),
        0x07 => Some(48_000),
        _ => None,
    }
}

fn audio_sampling_info(value: u8) -> Option<&'static str> {
    match value {
        0x01 => Some("16kHz"),
        0x02 => Some("22.05kHz"),
        0x03 => Some("24kHz"),
        0x05 => Some("32kHz"),
        0x06 => Some("44.1kHz"),
        0x07 => Some("48kHz"),
        _ => None,
    }
}

fn audio_component(value: &crate::descriptors::AudioComponentDescriptor) -> AudioComponentDto {
    let accessibility = (value.component_type >> 5) & 0x03;
    AudioComponentDto {
        stream_type: Some(i32::from(value.stream_type)),
        stream_content: Some(i32::from(value.stream_content)),
        component_tag: Some(i32::from(value.component_tag)),
        component_type: Some(i32::from(value.component_type)),
        language: Some(value.language_code.clone()),
        second_language: value.language_code_2.clone(),
        channel_configuration: audio_channel_configuration(
            value.stream_content,
            value.component_type,
        )
        .map(str::to_string),
        simulcast_group_tag: Some(i32::from(value.simulcast_group_tag)),
        sampling_rate: Some(i32::from(value.sampling_rate)),
        sampling_info: audio_sampling_info(value.sampling_rate).map(str::to_string),
        text: Some(value.text.clone()),
        source_descriptor: Some("audio_component_descriptor".to_string()),
        main: Some(value.main_component_flag),
        multi_lingual: Some(value.es_multi_lingual_flag),
        quality_indicator: Some(i32::from(value.quality_indicator)),
        parse_status: SiParseStatusDto::Ok,
        channel_count: audio_channel_count(value.stream_content, value.component_type),
        sample_rate_hz: audio_sample_rate_hz(value.sampling_rate),
        audio_description: Some(accessibility == 0x01),
        hard_of_hearing: Some(accessibility == 0x02),
        dual_mono: Some(value.stream_content == 0x02 && value.component_type & 0x1f == 0x02),
    }
}

fn descriptor_text_diagnostics(summary: &str) -> Vec<String> {
    summary
        .split([' ', '\n'])
        .filter(|value| {
            value.contains("unknownCount=")
                || value.contains("component=")
                || value.contains("audio=")
        })
        .map(str::to_string)
        .collect()
}

fn event_diagnostic_text(event: &EitEvent) -> String {
    let d = &event.descriptors;
    format!(
        "contentCount={} componentCount={} audioCount={} parentalCount={} seriesCount={} eventGroupCount={} componentGroupCount={} linkageCount={} unknownCount={} textDiagnostics={}",
        d.contents.len(),
        d.components.len(),
        d.audio_components.len(),
        d.parental_rating_descriptors.len(),
        d.series.len(),
        d.event_groups.len(),
        d.component_groups.len(),
        d.linkages.len(),
        d.unknown.len(),
        d.diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_str())
            .collect::<Vec<_>>()
            .join("; "),
    )
}

pub fn event(value: &EitEvent, stable_identity: Option<String>) -> EventDto {
    let provider = event_provider_fields(&value.descriptors);
    let scope = Some(DescriptorSectionScope {
        pid: Some(18),
        table_id: Some(value.table_id),
        table_id_extension: Some(value.service_id),
        version: Some(value.version),
        section_number: Some(value.section_number),
        original_network_id: Some(value.original_network_id),
        transport_stream_id: Some(value.transport_stream_id),
        service_id: Some(value.service_id),
        event_id: Some(value.event_id),
    });
    let descriptor_diagnostics = event_descriptor_diagnostics_scoped(&value.descriptors, scope);
    let descriptor_diagnostics_canonical_json =
        serde_json::to_string(&descriptor_diagnostics).unwrap_or_else(|_| "[]".to_string());
    let descriptor_facts_canonical_json = serde_json::to_string(&descriptor_facts(value)).ok();
    let summary = event_diagnostic_text(value);
    let parental_ratings = value
        .descriptors
        .parental_rating_descriptors
        .iter()
        .filter(|descriptor| descriptor.parse_status == DescriptorParseStatus::Ok)
        .flat_map(|descriptor| {
            descriptor
                .entries
                .iter()
                .map(move |rating| ParentalRatingDto {
                    country_code: rating.country_code.clone(),
                    raw_rating_byte: i32::from(rating.raw_rating_byte),
                    parse_status: parse_status(descriptor.parse_status),
                })
        })
        .collect();
    let series_candidates = value
        .descriptors
        .series
        .iter()
        .map(series)
        .collect::<Vec<_>>();
    let series_candidates_canonical_json = (series_candidates.len() > 1)
        .then(|| serde_json::to_string(&series_candidates).ok())
        .flatten();
    EventDto {
        service_key: service_key(
            value.original_network_id,
            value.transport_stream_id,
            value.service_id,
        ),
        stable_identity,
        event_id: i32::from(value.event_id),
        timing_state: timing_state(value.timing_state),
        raw_start_time_hex: hex(&value.raw_start_time),
        raw_duration_hex: hex(&value.raw_duration),
        start_time_millis: value.start_time_millis,
        duration_millis: value.duration_millis,
        title: provider.title,
        description: provider.description,
        extended_description: provider.extended_description,
        event_scope: value.scope.as_str().to_string(),
        source: ProgramSourceDto {
            pid: 18,
            table_id: i32::from(value.table_id),
            version: i32::from(value.version),
            section_number: i32::from(value.section_number),
            last_section_number: i32::from(value.last_section_number),
        },
        descriptors: EventDescriptorsDto {
            short_events: short_events(&value.descriptors),
            extended_texts: extended_texts(&value.descriptors),
            extended_items: extended_items(&value.descriptors),
            component_text: Some(
                value
                    .descriptors
                    .components
                    .iter()
                    .map(|component| component.text.clone())
                    .filter(|text| !text.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            audio_component_text: Some(
                value
                    .descriptors
                    .audio_components
                    .iter()
                    .map(|component| component.text.clone())
                    .filter(|text| !text.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            content_genres: value
                .descriptors
                .contents
                .iter()
                .map(|content| ContentGenreDto {
                    level1: i32::from(content.content_nibble_level_1),
                    level2: i32::from(content.content_nibble_level_2),
                    user_nibble: i32::from(
                        ((u16::from(content.user_nibble_1)) << 4)
                            | u16::from(content.user_nibble_2),
                    ),
                    arib_name: content.arib_display_name.clone(),
                    parse_status: SiParseStatusDto::Ok,
                })
                .collect(),
            genre_supplement_text: Some(
                value
                    .descriptors
                    .contents
                    .iter()
                    .map(|content| content.arib_display_name.clone())
                    .collect::<Vec<_>>()
                    .join("、"),
            ),
            event_groups: value
                .descriptors
                .event_groups
                .iter()
                .map(event_group)
                .collect(),
            component_groups: value
                .descriptors
                .component_groups
                .iter()
                .map(component_group)
                .collect(),
            linkage: value.descriptors.linkages.iter().map(linkage).collect(),
            free_ca_mode: Some(FreeCaModeDto {
                raw: Some(if value.free_ca_mode { 1 } else { 0 }),
                scrambled: Some(value.free_ca_mode),
                parse_status: SiParseStatusDto::Ok,
            }),
            series: match value.descriptors.series.as_slice() {
                [single] => Some(series(single)),
                _ => None,
            },
            series_candidates,
            series_candidates_canonical_json,
            parental_ratings,
            components: ComponentsDto {
                video: value
                    .descriptors
                    .components
                    .iter()
                    .map(video_component)
                    .collect(),
                audio: value
                    .descriptors
                    .audio_components
                    .iter()
                    .map(audio_component)
                    .collect(),
            },
            diagnostics: EventDiagnosticsDto {
                summary: summary.clone(),
                descriptor_diagnostics: descriptor_diagnostics
                    .into_iter()
                    .map(descriptor_diagnostic)
                    .collect(),
                descriptor_diagnostics_canonical_json,
                descriptor_facts_canonical_json,
                text_diagnostics: descriptor_text_diagnostics(&summary),
                truncated_descriptor_loop: value.descriptors.truncated_loop.as_ref().map(|facts| {
                    TruncatedDescriptorLoopDto {
                        declared_length: i32::try_from(facts.declared_length).unwrap_or(i32::MAX),
                        raw_bytes_hex: hex(&facts.raw_bytes),
                        parse_status: SiParseStatusDto::TruncatedDescriptor,
                    }
                }),
            },
        },
    }
}

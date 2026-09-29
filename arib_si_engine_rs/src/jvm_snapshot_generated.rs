use super::{SiJniFailure, SiJniFailureReason};
use jni::objects::{JObject, JValue};
use jni::JNIEnv;
use maleicacid_arib_si_engine_core::codec_probe_dto::*;
use maleicacid_arib_si_engine_core::runtime_snapshot_dto::*;

const PREFIX: &str = "com/maleicacid/tvinput/aribsi/generated/";

fn output_failure(detail: impl ToString) -> SiJniFailure {
    SiJniFailureReason::JniOutput.failure(detail)
}

fn jni_result<T>(result: jni::errors::Result<T>) -> Result<T, SiJniFailure> {
    result.map_err(output_failure)
}

fn new_generated<'local>(
    env: &mut JNIEnv<'local>,
    name: &str,
    signature: &str,
    args: &[JValue<'_, '_>],
) -> Result<JObject<'local>, SiJniFailure> {
    jni_result(env.new_object(format!("{PREFIX}{name}"), signature, args))
}

fn string_object<'local>(
    env: &mut JNIEnv<'local>,
    value: &str,
) -> Result<JObject<'local>, SiJniFailure> {
    env.new_string(value)
        .map(JObject::from)
        .map_err(output_failure)
}

fn optional_string_object<'local>(
    env: &mut JNIEnv<'local>,
    value: Option<&str>,
) -> Result<JObject<'local>, SiJniFailure> {
    match value {
        Some(value) => string_object(env, value),
        None => Ok(JObject::null()),
    }
}

fn boxed_int<'local>(
    env: &mut JNIEnv<'local>,
    value: Option<i32>,
) -> Result<JObject<'local>, SiJniFailure> {
    match value {
        Some(value) => jni_result(env.call_static_method(
            "java/lang/Integer",
            "valueOf",
            "(I)Ljava/lang/Integer;",
            &[JValue::Int(value)],
        ))?
        .l()
        .map_err(output_failure),
        None => Ok(JObject::null()),
    }
}

fn boxed_bool<'local>(
    env: &mut JNIEnv<'local>,
    value: Option<bool>,
) -> Result<JObject<'local>, SiJniFailure> {
    match value {
        Some(value) => jni_result(env.call_static_method(
            "java/lang/Boolean",
            "valueOf",
            "(Z)Ljava/lang/Boolean;",
            &[JValue::Bool(u8::from(value))],
        ))?
        .l()
        .map_err(output_failure),
        None => Ok(JObject::null()),
    }
}

fn generated_enum<'local>(
    env: &mut JNIEnv<'local>,
    enum_name: &str,
    variant: &str,
) -> Result<JObject<'local>, SiJniFailure> {
    let class = format!("{PREFIX}{enum_name}${variant}");
    let signature = format!("L{class};");
    jni_result(env.get_static_field(&class, "INSTANCE", &signature))?
        .l()
        .map_err(output_failure)
}

fn new_list<'local>(env: &mut JNIEnv<'local>) -> Result<JObject<'local>, SiJniFailure> {
    jni_result(env.new_object("java/util/ArrayList", "()V", &[]))
}

fn add_to_list(
    env: &mut JNIEnv<'_>,
    list: &JObject<'_>,
    value: &JObject<'_>,
) -> Result<(), SiJniFailure> {
    jni_result(env.call_method(
        list,
        "add",
        "(Ljava/lang/Object;)Z",
        &[JValue::Object(value)],
    ))?;
    Ok(())
}

fn object_list<'local, T>(
    env: &mut JNIEnv<'local>,
    values: &[T],
    mut build: impl FnMut(&mut JNIEnv<'local>, &T) -> Result<JObject<'local>, SiJniFailure>,
) -> Result<JObject<'local>, SiJniFailure> {
    let list = new_list(env)?;
    for value in values {
        let child = build(env, value)?;
        add_to_list(env, &list, &child)?;
        jni_result(env.delete_local_ref(child))?;
    }
    Ok(list)
}

fn string_list<'local>(
    env: &mut JNIEnv<'local>,
    values: &[String],
) -> Result<JObject<'local>, SiJniFailure> {
    object_list(env, values, |env, value| string_object(env, value))
}

fn int_list<'local>(
    env: &mut JNIEnv<'local>,
    values: &[i32],
) -> Result<JObject<'local>, SiJniFailure> {
    let list = new_list(env)?;
    for value in values {
        let child = boxed_int(env, Some(*value))?;
        add_to_list(env, &list, &child)?;
        jni_result(env.delete_local_ref(child))?;
    }
    Ok(list)
}

fn parse_status_variant(value: &SiParseStatusDto) -> &'static str {
    match value {
        SiParseStatusDto::Ok => "OK",
        SiParseStatusDto::MalformedLength => "MalformedLength",
        SiParseStatusDto::TruncatedDescriptor => "TruncatedDescriptor",
        SiParseStatusDto::UnsupportedValue => "UnsupportedValue",
        SiParseStatusDto::InvalidSequence => "InvalidSequence",
        SiParseStatusDto::Unresolved => "UNRESOLVED",
    }
}

fn timing_variant(value: &EitTimingStateDto) -> &'static str {
    match value {
        EitTimingStateDto::Defined => "Defined",
        EitTimingStateDto::UndefinedTime => "UndefinedTime",
        EitTimingStateDto::BothTimingUndefined => "BothTimingUndefined",
        EitTimingStateDto::MalformedTiming => "MalformedTiming",
    }
}

fn stream_kind_variant(value: &ElementaryStreamKindDto) -> &'static str {
    match value {
        ElementaryStreamKindDto::Video => "VIDEO",
        ElementaryStreamKindDto::Audio => "AUDIO",
    }
}

fn broadcast_system_variant(value: &BroadcastSystemDto) -> &'static str {
    match value {
        BroadcastSystemDto::IsdbT => "IsdbT",
        BroadcastSystemDto::IsdbSBs => "IsdbSBs",
        BroadcastSystemDto::IsdbS110Cs => "IsdbS110Cs",
    }
}

fn smd_state_variant(value: &SmdSemanticStateDto) -> &'static str {
    match value {
        SmdSemanticStateDto::SupportedBroadcast => "SupportedBroadcast",
        SmdSemanticStateDto::NonBroadcast => "NonBroadcast",
        SmdSemanticStateDto::UndefinedBroadcastClass => "UndefinedBroadcastClass",
        SmdSemanticStateDto::UnsupportedBroadcastSystem => "UnsupportedBroadcastSystem",
        SmdSemanticStateDto::UndeterminedSmd => "UndeterminedSmd",
    }
}

fn ca_scope_variant(value: &CaDescriptorScopeDto) -> &'static str {
    match value {
        CaDescriptorScopeDto::Program => "PROGRAM",
        CaDescriptorScopeDto::Es => "ES",
    }
}

fn ca_source_variant(value: &CaMetadataSourceDto) -> &'static str {
    match value {
        CaMetadataSourceDto::Program => "Program",
        CaMetadataSourceDto::ElementaryStream => "ElementaryStream",
        CaMetadataSourceDto::Cat => "Cat",
    }
}

fn aac_probe_status_variant(value: &AacProbeStatusDto) -> &'static str {
    match value {
        AacProbeStatusDto::Pending => "PENDING",
        AacProbeStatusDto::Invalid => "INVALID",
        AacProbeStatusDto::Ready => "READY",
    }
}

fn build_aac_adts_configuration<'local>(
    env: &mut JNIEnv<'local>,
    value: &AacAdtsConfigurationDto,
) -> Result<JObject<'local>, SiJniFailure> {
    let extension_sampling_frequency = boxed_int(env, value.extension_sampling_frequency)?;
    let audio_specific_config_hex = string_object(env, &value.audio_specific_config_hex)?;
    let result = new_generated(
        env,
        "AacAdtsConfigurationDto",
        "(IILjava/lang/Integer;IILjava/lang/String;)V",
        &[
            JValue::Int(value.audio_object_type),
            JValue::Int(value.sampling_frequency),
            JValue::Object(&extension_sampling_frequency),
            JValue::Int(value.channel_configuration),
            JValue::Int(value.channel_count),
            JValue::Object(&audio_specific_config_hex),
        ],
    );
    for object in [extension_sampling_frequency, audio_specific_config_hex] {
        if !object.is_null() {
            jni_result(env.delete_local_ref(object))?;
        }
    }
    result
}

pub(super) fn aac_probe_to_java<'local>(
    env: &mut JNIEnv<'local>,
    value: &AacConfigurationProbeDto,
) -> Result<JObject<'local>, SiJniFailure> {
    let status = generated_enum(
        env,
        "AacProbeStatusDto",
        aac_probe_status_variant(&value.status),
    )?;
    let reason = optional_string_object(env, value.reason.as_deref())?;
    let configuration = match value.configuration.as_ref() {
        Some(configuration) => build_aac_adts_configuration(env, configuration)?,
        None => JObject::null(),
    };
    let result = new_generated(
        env,
        "AacConfigurationProbeDto",
        "(Lcom/maleicacid/tvinput/aribsi/generated/AacProbeStatusDto;Ljava/lang/String;Lcom/maleicacid/tvinput/aribsi/generated/AacAdtsConfigurationDto;)V",
        &[
            JValue::Object(&status),
            JValue::Object(&reason),
            JValue::Object(&configuration),
        ],
    );
    for object in [status, reason, configuration] {
        if !object.is_null() {
            jni_result(env.delete_local_ref(object))?;
        }
    }
    result
}

fn build_service_key<'local>(
    env: &mut JNIEnv<'local>,
    value: &ServiceKeyDto,
) -> Result<JObject<'local>, SiJniFailure> {
    new_generated(
        env,
        "ServiceKeyDto",
        "(III)V",
        &[
            JValue::Int(value.original_network_id),
            JValue::Int(value.transport_stream_id),
            JValue::Int(value.service_id),
        ],
    )
}

fn build_broadcast_clock<'local>(
    env: &mut JNIEnv<'local>,
    value: &BroadcastClockDto,
) -> Result<JObject<'local>, SiJniFailure> {
    new_generated(
        env,
        "BroadcastClockDto",
        "(IIJ)V",
        &[
            JValue::Int(value.table_id),
            JValue::Int(value.mjd),
            JValue::Long(value.millis_of_day),
        ],
    )
}

fn build_table_requirement<'local>(
    env: &mut JNIEnv<'local>,
    value: &TableRequirementDto,
) -> Result<JObject<'local>, SiJniFailure> {
    let component = string_object(env, &value.component)?;
    let onid = boxed_int(env, value.original_network_id)?;
    let tsid = boxed_int(env, value.transport_stream_id)?;
    let sid = boxed_int(env, value.service_id)?;
    let result = new_generated(
        env,
        "TableRequirementDto",
        "(Ljava/lang/String;Ljava/lang/Integer;Ljava/lang/Integer;Ljava/lang/Integer;ZZ)V",
        &[
            JValue::Object(&component),
            JValue::Object(&onid),
            JValue::Object(&tsid),
            JValue::Object(&sid),
            JValue::Bool(u8::from(value.required)),
            JValue::Bool(u8::from(value.complete)),
        ],
    );
    for object in [component, onid, tsid, sid] {
        if !object.is_null() {
            jni_result(env.delete_local_ref(object))?;
        }
    }
    result
}

fn build_ca_metadata<'local>(
    env: &mut JNIEnv<'local>,
    value: &CaMetadataDto,
) -> Result<JObject<'local>, SiJniFailure> {
    let service_key = match value.service_key.as_ref() {
        Some(key) => build_service_key(env, key)?,
        None => JObject::null(),
    };
    let ecm_pid = boxed_int(env, value.ecm_pid)?;
    let emm_pid = boxed_int(env, value.emm_pid)?;
    let elementary_pid = boxed_int(env, value.elementary_pid)?;
    let private_data_hex = string_object(env, &value.private_data_hex)?;
    let source = generated_enum(env, "CaMetadataSourceDto", ca_source_variant(&value.source))?;
    let result = new_generated(
        env,
        "CaMetadataDto",
        "(Lcom/maleicacid/tvinput/aribsi/generated/ServiceKeyDto;ILjava/lang/Integer;Ljava/lang/Integer;Ljava/lang/Integer;Ljava/lang/String;Lcom/maleicacid/tvinput/aribsi/generated/CaMetadataSourceDto;)V",
        &[
            JValue::Object(&service_key),
            JValue::Int(value.ca_system_id),
            JValue::Object(&ecm_pid),
            JValue::Object(&emm_pid),
            JValue::Object(&elementary_pid),
            JValue::Object(&private_data_hex),
            JValue::Object(&source),
        ],
    );
    for object in [
        service_key,
        ecm_pid,
        emm_pid,
        elementary_pid,
        private_data_hex,
        source,
    ] {
        if !object.is_null() {
            jni_result(env.delete_local_ref(object))?;
        }
    }
    result
}

fn build_malformed_ca<'local>(
    env: &mut JNIEnv<'local>,
    value: &MalformedCaDescriptorDiagnosticDto,
) -> Result<JObject<'local>, SiJniFailure> {
    let table_id_extension = boxed_int(env, value.table_id_extension)?;
    let service_id = boxed_int(env, value.service_id)?;
    let elementary_pid = boxed_int(env, value.elementary_pid)?;
    let scope = string_object(env, &value.scope)?;
    let reason = string_object(env, &value.reason)?;
    let raw_prefix_hex = string_object(env, &value.raw_prefix_hex)?;
    let result = new_generated(
        env,
        "MalformedCaDescriptorDiagnosticDto",
        "(IILjava/lang/Integer;Ljava/lang/Integer;Ljava/lang/Integer;Ljava/lang/String;IIILjava/lang/String;Ljava/lang/String;)V",
        &[
            JValue::Int(value.pid),
            JValue::Int(value.table_id),
            JValue::Object(&table_id_extension),
            JValue::Object(&service_id),
            JValue::Object(&elementary_pid),
            JValue::Object(&scope),
            JValue::Int(value.offset),
            JValue::Int(value.declared_length),
            JValue::Int(value.actual_remaining_length),
            JValue::Object(&reason),
            JValue::Object(&raw_prefix_hex),
        ],
    );
    for object in [
        table_id_extension,
        service_id,
        elementary_pid,
        scope,
        reason,
        raw_prefix_hex,
    ] {
        if !object.is_null() {
            jni_result(env.delete_local_ref(object))?;
        }
    }
    result
}

fn build_malformed_count<'local>(
    env: &mut JNIEnv<'local>,
    value: &MalformedCaDescriptorCountDto,
) -> Result<JObject<'local>, SiJniFailure> {
    new_generated(
        env,
        "MalformedCaDescriptorCountDto",
        "(II)V",
        &[JValue::Int(value.service_id), JValue::Int(value.count)],
    )
}

fn build_transport<'local>(
    env: &mut JNIEnv<'local>,
    value: &TransportSemanticFactsDto,
) -> Result<JObject<'local>, SiJniFailure> {
    let network_name = optional_string_object(env, value.network_name.as_deref())?;
    let transport_stream_name =
        optional_string_object(env, value.transport_stream_name.as_deref())?;
    let remote_control_key_id = boxed_int(env, value.remote_control_key_id)?;
    let result = new_generated(
        env,
        "TransportSemanticFactsDto",
        "(IILjava/lang/String;Ljava/lang/String;Ljava/lang/Integer;Z)V",
        &[
            JValue::Int(value.original_network_id),
            JValue::Int(value.transport_stream_id),
            JValue::Object(&network_name),
            JValue::Object(&transport_stream_name),
            JValue::Object(&remote_control_key_id),
            JValue::Bool(u8::from(value.sdt_actual)),
        ],
    );
    for object in [network_name, transport_stream_name, remote_control_key_id] {
        if !object.is_null() {
            jni_result(env.delete_local_ref(object))?;
        }
    }
    result
}

fn build_avc<'local>(
    env: &mut JNIEnv<'local>,
    value: &AvcSignalingDto,
) -> Result<JObject<'local>, SiJniFailure> {
    new_generated(
        env,
        "AvcSignalingDto",
        "(III)V",
        &[
            JValue::Int(value.profile_idc),
            JValue::Int(value.constraint_flags),
            JValue::Int(value.level_idc),
        ],
    )
}

fn build_audio_header<'local>(
    env: &mut JNIEnv<'local>,
    value: &AudioConfigHeaderDto,
) -> Result<JObject<'local>, SiJniFailure> {
    let extension_sampling_frequency = boxed_int(env, value.extension_sampling_frequency)?;
    let core_audio_object_type = boxed_int(env, value.core_audio_object_type)?;
    let channel_count = boxed_int(env, value.channel_count)?;
    let result = new_generated(
        env,
        "AudioConfigHeaderDto",
        "(IIILjava/lang/Integer;Ljava/lang/Integer;Ljava/lang/Integer;)V",
        &[
            JValue::Int(value.audio_object_type),
            JValue::Int(value.sampling_frequency),
            JValue::Int(value.channel_configuration),
            JValue::Object(&extension_sampling_frequency),
            JValue::Object(&core_audio_object_type),
            JValue::Object(&channel_count),
        ],
    );
    for object in [
        extension_sampling_frequency,
        core_audio_object_type,
        channel_count,
    ] {
        if !object.is_null() {
            jni_result(env.delete_local_ref(object))?;
        }
    }
    result
}

fn build_codec_facts<'local>(
    env: &mut JNIEnv<'local>,
    value: &CodecFactsDto,
) -> Result<JObject<'local>, SiJniFailure> {
    let avc = match value.avc.as_ref() {
        Some(value) => build_avc(env, value)?,
        None => JObject::null(),
    };
    let audio_config_hex = optional_string_object(env, value.audio_config_hex.as_deref())?;
    let audio_config_header = match value.audio_config_header.as_ref() {
        Some(value) => build_audio_header(env, value)?,
        None => JObject::null(),
    };
    let raw_descriptors_hex = optional_string_object(env, value.raw_descriptors_hex.as_deref())?;
    let profile_level = optional_string_object(env, value.profile_level.as_deref())?;
    let result = new_generated(
        env,
        "CodecFactsDto",
        "(Lcom/maleicacid/tvinput/aribsi/generated/AvcSignalingDto;Ljava/lang/String;Lcom/maleicacid/tvinput/aribsi/generated/AudioConfigHeaderDto;Ljava/lang/String;Ljava/lang/String;Z)V",
        &[
            JValue::Object(&avc),
            JValue::Object(&audio_config_hex),
            JValue::Object(&audio_config_header),
            JValue::Object(&raw_descriptors_hex),
            JValue::Object(&profile_level),
            JValue::Bool(u8::from(value.resolved)),
        ],
    );
    for object in [
        avc,
        audio_config_hex,
        audio_config_header,
        raw_descriptors_hex,
        profile_level,
    ] {
        if !object.is_null() {
            jni_result(env.delete_local_ref(object))?;
        }
    }
    result
}

fn build_elementary_stream<'local>(
    env: &mut JNIEnv<'local>,
    value: &ElementaryStreamDto,
) -> Result<JObject<'local>, SiJniFailure> {
    let component_tag = boxed_int(env, value.component_tag)?;
    let component_type = boxed_int(env, value.component_type)?;
    let stream_content = boxed_int(env, value.stream_content)?;
    let language_codes = string_list(env, &value.language_codes)?;
    let data_component_id = boxed_int(env, value.data_component_id)?;
    let caption_dmf = boxed_int(env, value.caption_dmf)?;
    let caption_timing = boxed_int(env, value.caption_timing)?;
    let automatic_presentation = boxed_bool(env, value.automatic_presentation_on_reception)?;
    let codec = optional_string_object(env, value.codec.as_deref())?;
    let codec_kind = match value.codec_kind.as_ref() {
        Some(value) => generated_enum(env, "ElementaryStreamKindDto", stream_kind_variant(value))?,
        None => JObject::null(),
    };
    let codec_facts = build_codec_facts(env, &value.codec_facts)?;
    let result = new_generated(
        env,
        "ElementaryStreamDto",
        "(IILjava/lang/Integer;Ljava/lang/Integer;Ljava/lang/Integer;Ljava/util/List;Ljava/lang/Integer;Ljava/lang/Integer;Ljava/lang/Integer;Ljava/lang/Boolean;ZZLjava/lang/String;Lcom/maleicacid/tvinput/aribsi/generated/ElementaryStreamKindDto;Lcom/maleicacid/tvinput/aribsi/generated/CodecFactsDto;)V",
        &[
            JValue::Int(value.elementary_pid),
            JValue::Int(value.stream_type),
            JValue::Object(&component_tag),
            JValue::Object(&component_type),
            JValue::Object(&stream_content),
            JValue::Object(&language_codes),
            JValue::Object(&data_component_id),
            JValue::Object(&caption_dmf),
            JValue::Object(&caption_timing),
            JValue::Object(&automatic_presentation),
            JValue::Bool(u8::from(value.is_caption)),
            JValue::Bool(u8::from(value.is_superimpose)),
            JValue::Object(&codec),
            JValue::Object(&codec_kind),
            JValue::Object(&codec_facts),
        ],
    );
    for object in [
        component_tag,
        component_type,
        stream_content,
        language_codes,
        data_component_id,
        caption_dmf,
        caption_timing,
        automatic_presentation,
        codec,
        codec_kind,
        codec_facts,
    ] {
        if !object.is_null() {
            jni_result(env.delete_local_ref(object))?;
        }
    }
    result
}

fn build_ca_descriptor<'local>(
    env: &mut JNIEnv<'local>,
    value: &ServiceCaDescriptorDto,
) -> Result<JObject<'local>, SiJniFailure> {
    let scope = generated_enum(env, "CaDescriptorScopeDto", ca_scope_variant(&value.scope))?;
    let es_pid = boxed_int(env, value.es_pid)?;
    let raw_descriptor_hex = string_object(env, &value.raw_descriptor_hex)?;
    let private_data_hex = string_object(env, &value.private_data_hex)?;
    let result = new_generated(
        env,
        "ServiceCaDescriptorDto",
        "(IILcom/maleicacid/tvinput/aribsi/generated/CaDescriptorScopeDto;Ljava/lang/Integer;Ljava/lang/String;Ljava/lang/String;)V",
        &[
            JValue::Int(value.ca_system_id),
            JValue::Int(value.ca_pid),
            JValue::Object(&scope),
            JValue::Object(&es_pid),
            JValue::Object(&raw_descriptor_hex),
            JValue::Object(&private_data_hex),
        ],
    );
    for object in [scope, es_pid, raw_descriptor_hex, private_data_hex] {
        if !object.is_null() {
            jni_result(env.delete_local_ref(object))?;
        }
    }
    result
}

fn build_smd<'local>(
    env: &mut JNIEnv<'local>,
    value: &SmdSemanticFactsDto,
) -> Result<JObject<'local>, SiJniFailure> {
    let system_management_id = boxed_int(env, value.system_management_id)?;
    let broadcasting_flag = boxed_int(env, value.broadcasting_flag)?;
    let broadcasting_identifier = boxed_int(env, value.broadcasting_identifier)?;
    let broadcast_system = match value.broadcast_system.as_ref() {
        Some(value) => generated_enum(env, "BroadcastSystemDto", broadcast_system_variant(value))?,
        None => JObject::null(),
    };
    let additional_id = boxed_int(env, value.additional_broadcasting_identification)?;
    let additional_hex = string_object(env, &value.additional_identification_info_hex)?;
    let semantic_state = generated_enum(
        env,
        "SmdSemanticStateDto",
        smd_state_variant(&value.semantic_state),
    )?;
    let diagnostic = optional_string_object(env, value.diagnostic.as_deref())?;
    let result = new_generated(
        env,
        "SmdSemanticFactsDto",
        "(ZZLjava/lang/Integer;Ljava/lang/Integer;Ljava/lang/Integer;Lcom/maleicacid/tvinput/aribsi/generated/BroadcastSystemDto;Ljava/lang/Integer;Ljava/lang/String;Lcom/maleicacid/tvinput/aribsi/generated/SmdSemanticStateDto;Ljava/lang/String;)V",
        &[
            JValue::Bool(u8::from(value.descriptor_present)),
            JValue::Bool(u8::from(value.syntax_valid)),
            JValue::Object(&system_management_id),
            JValue::Object(&broadcasting_flag),
            JValue::Object(&broadcasting_identifier),
            JValue::Object(&broadcast_system),
            JValue::Object(&additional_id),
            JValue::Object(&additional_hex),
            JValue::Object(&semantic_state),
            JValue::Object(&diagnostic),
        ],
    );
    for object in [
        system_management_id,
        broadcasting_flag,
        broadcasting_identifier,
        broadcast_system,
        additional_id,
        additional_hex,
        semantic_state,
        diagnostic,
    ] {
        if !object.is_null() {
            jni_result(env.delete_local_ref(object))?;
        }
    }
    result
}

fn build_service_semantic_facts<'local>(
    env: &mut JNIEnv<'local>,
    value: &ServiceSemanticFactsDto,
) -> Result<JObject<'local>, SiJniFailure> {
    let service_type = boxed_int(env, value.service_type)?;
    let streams = object_list(env, &value.elementary_streams, build_elementary_stream)?;
    let cas_json = optional_string_object(env, value.cas_facts_canonical_json.as_deref())?;
    let free_ca_mode = boxed_bool(env, value.free_ca_mode)?;
    let smd = build_smd(env, &value.smd)?;
    let missing = string_list(env, &value.missing_components)?;
    let semantic = string_list(env, &value.semantic_diagnostics)?;
    let name = optional_string_object(env, value.name.as_deref())?;
    let provider_name = optional_string_object(env, value.provider_name.as_deref())?;
    let pmt_pid = boxed_int(env, value.pmt_pid)?;
    let pcr_pid = boxed_int(env, value.pcr_pid)?;
    let descriptors = object_list(
        env,
        &value.service_scoped_ca_descriptors,
        build_ca_descriptor,
    )?;
    let result = new_generated(
        env,
        "ServiceSemanticFactsDto",
        "(IIILjava/lang/Integer;ZZZLjava/util/List;ZLjava/lang/String;ZLjava/lang/Boolean;Lcom/maleicacid/tvinput/aribsi/generated/SmdSemanticFactsDto;Ljava/util/List;Ljava/util/List;Ljava/lang/String;Ljava/lang/String;Ljava/lang/Integer;Ljava/lang/Integer;Ljava/util/List;)V",
        &[
            JValue::Int(value.original_network_id),
            JValue::Int(value.transport_stream_id),
            JValue::Int(value.service_id),
            JValue::Object(&service_type),
            JValue::Bool(u8::from(value.pmt_pid_resolved)),
            JValue::Bool(u8::from(value.pmt_parsed)),
            JValue::Bool(u8::from(value.pcr_pid_resolved)),
            JValue::Object(&streams),
            JValue::Bool(u8::from(value.requires_cas)),
            JValue::Object(&cas_json),
            JValue::Bool(u8::from(value.ca_descriptors_resolved)),
            JValue::Object(&free_ca_mode),
            JValue::Object(&smd),
            JValue::Object(&missing),
            JValue::Object(&semantic),
            JValue::Object(&name),
            JValue::Object(&provider_name),
            JValue::Object(&pmt_pid),
            JValue::Object(&pcr_pid),
            JValue::Object(&descriptors),
        ],
    );
    for object in [
        service_type,
        streams,
        cas_json,
        free_ca_mode,
        smd,
        missing,
        semantic,
        name,
        provider_name,
        pmt_pid,
        pcr_pid,
        descriptors,
    ] {
        if !object.is_null() {
            jni_result(env.delete_local_ref(object))?;
        }
    }
    result
}

fn build_eit_instance<'local>(
    env: &mut JNIEnv<'local>,
    value: &EitInstanceDto,
) -> Result<JObject<'local>, SiJniFailure> {
    let received = int_list(env, &value.received_sections)?;
    let missing = int_list(env, &value.missing_sections)?;
    let safe = int_list(env, &value.safe_sections)?;
    let result = new_generated(
        env,
        "EitInstanceDto",
        "(IIIIIZILjava/util/List;Ljava/util/List;Ljava/util/List;ZZ)V",
        &[
            JValue::Int(value.original_network_id),
            JValue::Int(value.transport_stream_id),
            JValue::Int(value.service_id),
            JValue::Int(value.table_id),
            JValue::Int(value.version),
            JValue::Bool(u8::from(value.current_next_indicator)),
            JValue::Int(value.last_section_number),
            JValue::Object(&received),
            JValue::Object(&missing),
            JValue::Object(&safe),
            JValue::Bool(u8::from(value.complete)),
            JValue::Bool(u8::from(value.inconsistent)),
        ],
    );
    for object in [received, missing, safe] {
        jni_result(env.delete_local_ref(object))?;
    }
    result
}

fn build_parser_diagnostic<'local>(
    env: &mut JNIEnv<'local>,
    value: &ParserDiagnosticDto,
) -> Result<JObject<'local>, SiJniFailure> {
    let code = string_object(env, &value.code)?;
    let message = string_object(env, &value.message)?;
    let severity = optional_string_object(env, value.severity.as_deref())?;
    let result = new_generated(
        env,
        "ParserDiagnosticDto",
        "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V",
        &[
            JValue::Object(&code),
            JValue::Object(&message),
            JValue::Object(&severity),
        ],
    );
    for object in [code, message, severity] {
        if !object.is_null() {
            jni_result(env.delete_local_ref(object))?;
        }
    }
    result
}

fn build_short_event<'local>(
    env: &mut JNIEnv<'local>,
    value: &ShortEventDto,
) -> Result<JObject<'local>, SiJniFailure> {
    let language = string_object(env, &value.language_code)?;
    let title = string_object(env, &value.title)?;
    let text = string_object(env, &value.text)?;
    let status = generated_enum(
        env,
        "SiParseStatusDto",
        parse_status_variant(&value.parse_status),
    )?;
    let result = new_generated(
        env,
        "ShortEventDto",
        "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Lcom/maleicacid/tvinput/aribsi/generated/SiParseStatusDto;)V",
        &[
            JValue::Object(&language),
            JValue::Object(&title),
            JValue::Object(&text),
            JValue::Object(&status),
        ],
    );
    for object in [language, title, text, status] {
        jni_result(env.delete_local_ref(object))?;
    }
    result
}

fn build_extended_text<'local>(
    env: &mut JNIEnv<'local>,
    value: &ExtendedTextDto,
) -> Result<JObject<'local>, SiJniFailure> {
    let language = string_object(env, &value.language_code)?;
    let text = string_object(env, &value.text)?;
    let status = generated_enum(
        env,
        "SiParseStatusDto",
        parse_status_variant(&value.parse_status),
    )?;
    let result = new_generated(
        env,
        "ExtendedTextDto",
        "(Ljava/lang/String;Ljava/lang/String;Lcom/maleicacid/tvinput/aribsi/generated/SiParseStatusDto;)V",
        &[
            JValue::Object(&language),
            JValue::Object(&text),
            JValue::Object(&status),
        ],
    );
    for object in [language, text, status] {
        jni_result(env.delete_local_ref(object))?;
    }
    result
}

fn build_extended_item<'local>(
    env: &mut JNIEnv<'local>,
    value: &ExtendedItemDto,
) -> Result<JObject<'local>, SiJniFailure> {
    let language = string_object(env, &value.language_code)?;
    let description = string_object(env, &value.description)?;
    let text = string_object(env, &value.text)?;
    let result = new_generated(
        env,
        "ExtendedItemDto",
        "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V",
        &[
            JValue::Object(&language),
            JValue::Object(&description),
            JValue::Object(&text),
        ],
    );
    for object in [language, description, text] {
        jni_result(env.delete_local_ref(object))?;
    }
    result
}

fn build_content_genre<'local>(
    env: &mut JNIEnv<'local>,
    value: &ContentGenreDto,
) -> Result<JObject<'local>, SiJniFailure> {
    let arib_name = string_object(env, &value.arib_name)?;
    let status = generated_enum(
        env,
        "SiParseStatusDto",
        parse_status_variant(&value.parse_status),
    )?;
    let result = new_generated(
        env,
        "ContentGenreDto",
        "(IIILjava/lang/String;Lcom/maleicacid/tvinput/aribsi/generated/SiParseStatusDto;)V",
        &[
            JValue::Int(value.level1),
            JValue::Int(value.level2),
            JValue::Int(value.user_nibble),
            JValue::Object(&arib_name),
            JValue::Object(&status),
        ],
    );
    for object in [arib_name, status] {
        jni_result(env.delete_local_ref(object))?;
    }
    result
}

fn build_event_group_reference<'local>(
    env: &mut JNIEnv<'local>,
    value: &EventGroupReferenceDto,
) -> Result<JObject<'local>, SiJniFailure> {
    new_generated(
        env,
        "EventGroupReferenceDto",
        "(II)V",
        &[JValue::Int(value.service_id), JValue::Int(value.event_id)],
    )
}

fn build_other_event_group_reference<'local>(
    env: &mut JNIEnv<'local>,
    value: &OtherNetworkEventGroupReferenceDto,
) -> Result<JObject<'local>, SiJniFailure> {
    new_generated(
        env,
        "OtherNetworkEventGroupReferenceDto",
        "(IIII)V",
        &[
            JValue::Int(value.original_network_id),
            JValue::Int(value.transport_stream_id),
            JValue::Int(value.service_id),
            JValue::Int(value.event_id),
        ],
    )
}

fn build_event_group<'local>(
    env: &mut JNIEnv<'local>,
    value: &EventGroupDto,
) -> Result<JObject<'local>, SiJniFailure> {
    let events = object_list(env, &value.events, build_event_group_reference)?;
    let other_network_events = object_list(
        env,
        &value.other_network_events,
        build_other_event_group_reference,
    )?;
    let private_data_hex = string_object(env, &value.private_data_hex)?;
    let parse_status = generated_enum(
        env,
        "SiParseStatusDto",
        parse_status_variant(&value.parse_status),
    )?;
    let result = new_generated(
        env,
        "EventGroupDto",
        "(ILjava/util/List;Ljava/util/List;Ljava/lang/String;Lcom/maleicacid/tvinput/aribsi/generated/SiParseStatusDto;)V",
        &[
            JValue::Int(value.group_type),
            JValue::Object(&events),
            JValue::Object(&other_network_events),
            JValue::Object(&private_data_hex),
            JValue::Object(&parse_status),
        ],
    );
    for object in [events, other_network_events, private_data_hex, parse_status] {
        jni_result(env.delete_local_ref(object))?;
    }
    result
}

fn build_component_group<'local>(
    env: &mut JNIEnv<'local>,
    value: &ComponentGroupDto,
) -> Result<JObject<'local>, SiJniFailure> {
    let tags = int_list(env, &value.component_tags)?;
    let result = new_generated(
        env,
        "ComponentGroupDto",
        "(ILjava/util/List;)V",
        &[JValue::Int(value.component_group_id), JValue::Object(&tags)],
    );
    jni_result(env.delete_local_ref(tags))?;
    result
}

fn build_component_group_descriptor<'local>(
    env: &mut JNIEnv<'local>,
    value: &ComponentGroupDescriptorDto,
) -> Result<JObject<'local>, SiJniFailure> {
    let groups = object_list(env, &value.groups, build_component_group)?;
    let parse_status = generated_enum(
        env,
        "SiParseStatusDto",
        parse_status_variant(&value.parse_status),
    )?;
    let result = new_generated(
        env,
        "ComponentGroupDescriptorDto",
        "(ILjava/util/List;Lcom/maleicacid/tvinput/aribsi/generated/SiParseStatusDto;)V",
        &[
            JValue::Int(value.component_group_type),
            JValue::Object(&groups),
            JValue::Object(&parse_status),
        ],
    );
    for object in [groups, parse_status] {
        jni_result(env.delete_local_ref(object))?;
    }
    result
}

fn build_linkage<'local>(
    env: &mut JNIEnv<'local>,
    value: &LinkageDto,
) -> Result<JObject<'local>, SiJniFailure> {
    let private_data_prefix_hex = string_object(env, &value.private_data_prefix_hex)?;
    let parse_status = generated_enum(
        env,
        "SiParseStatusDto",
        parse_status_variant(&value.parse_status),
    )?;
    let result = new_generated(
        env,
        "LinkageDto",
        "(IIIILjava/lang/String;Lcom/maleicacid/tvinput/aribsi/generated/SiParseStatusDto;)V",
        &[
            JValue::Int(value.linkage_type),
            JValue::Int(value.original_network_id),
            JValue::Int(value.transport_stream_id),
            JValue::Int(value.service_id),
            JValue::Object(&private_data_prefix_hex),
            JValue::Object(&parse_status),
        ],
    );
    for object in [private_data_prefix_hex, parse_status] {
        jni_result(env.delete_local_ref(object))?;
    }
    result
}

fn build_free_ca_mode<'local>(
    env: &mut JNIEnv<'local>,
    value: &FreeCaModeDto,
) -> Result<JObject<'local>, SiJniFailure> {
    let raw = boxed_int(env, value.raw)?;
    let scrambled = boxed_bool(env, value.scrambled)?;
    let parse_status = generated_enum(
        env,
        "SiParseStatusDto",
        parse_status_variant(&value.parse_status),
    )?;
    let result = new_generated(
        env,
        "FreeCaModeDto",
        "(Ljava/lang/Integer;Ljava/lang/Boolean;Lcom/maleicacid/tvinput/aribsi/generated/SiParseStatusDto;)V",
        &[
            JValue::Object(&raw),
            JValue::Object(&scrambled),
            JValue::Object(&parse_status),
        ],
    );
    for object in [raw, scrambled, parse_status] {
        if !object.is_null() {
            jni_result(env.delete_local_ref(object))?;
        }
    }
    result
}

fn build_series<'local>(
    env: &mut JNIEnv<'local>,
    value: &SeriesDto,
) -> Result<JObject<'local>, SiJniFailure> {
    let series_id = boxed_int(env, value.series_id)?;
    let expire_date = boxed_int(env, value.expire_date)?;
    let episode_number = boxed_int(env, value.episode_number)?;
    let last_episode_number = boxed_int(env, value.last_episode_number)?;
    let name = optional_string_object(env, value.name.as_deref())?;
    let parse_status = generated_enum(
        env,
        "SiParseStatusDto",
        parse_status_variant(&value.parse_status),
    )?;
    let result = new_generated(
        env,
        "SeriesDto",
        "(Ljava/lang/Integer;IIZLjava/lang/Integer;Ljava/lang/Integer;Ljava/lang/Integer;Ljava/lang/String;Lcom/maleicacid/tvinput/aribsi/generated/SiParseStatusDto;)V",
        &[
            JValue::Object(&series_id),
            JValue::Int(value.repeat_label),
            JValue::Int(value.program_pattern),
            JValue::Bool(u8::from(value.expire_date_valid)),
            JValue::Object(&expire_date),
            JValue::Object(&episode_number),
            JValue::Object(&last_episode_number),
            JValue::Object(&name),
            JValue::Object(&parse_status),
        ],
    );
    for object in [
        series_id,
        expire_date,
        episode_number,
        last_episode_number,
        name,
        parse_status,
    ] {
        if !object.is_null() {
            jni_result(env.delete_local_ref(object))?;
        }
    }
    result
}

fn build_video_component<'local>(
    env: &mut JNIEnv<'local>,
    value: &VideoComponentDto,
) -> Result<JObject<'local>, SiJniFailure> {
    let stream_content = boxed_int(env, value.stream_content)?;
    let component_tag = boxed_int(env, value.component_tag)?;
    let component_type = boxed_int(env, value.component_type)?;
    let language = optional_string_object(env, value.language.as_deref())?;
    let text = optional_string_object(env, value.text.as_deref())?;
    let source_descriptor = optional_string_object(env, value.source_descriptor.as_deref())?;
    let resolution = optional_string_object(env, value.resolution.as_deref())?;
    let scan = optional_string_object(env, value.scan.as_deref())?;
    let aspect = optional_string_object(env, value.aspect.as_deref())?;
    let profile_level = optional_string_object(env, value.profile_level.as_deref())?;
    let parse_status = generated_enum(
        env,
        "SiParseStatusDto",
        parse_status_variant(&value.parse_status),
    )?;
    let result = new_generated(
        env,
        "VideoComponentDto",
        "(Ljava/lang/Integer;Ljava/lang/Integer;Ljava/lang/Integer;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Lcom/maleicacid/tvinput/aribsi/generated/SiParseStatusDto;)V",
        &[
            JValue::Object(&stream_content),
            JValue::Object(&component_tag),
            JValue::Object(&component_type),
            JValue::Object(&language),
            JValue::Object(&text),
            JValue::Object(&source_descriptor),
            JValue::Object(&resolution),
            JValue::Object(&scan),
            JValue::Object(&aspect),
            JValue::Object(&profile_level),
            JValue::Object(&parse_status),
        ],
    );
    for object in [
        stream_content,
        component_tag,
        component_type,
        language,
        text,
        source_descriptor,
        resolution,
        scan,
        aspect,
        profile_level,
        parse_status,
    ] {
        if !object.is_null() {
            jni_result(env.delete_local_ref(object))?;
        }
    }
    result
}

fn build_audio_component<'local>(
    env: &mut JNIEnv<'local>,
    value: &AudioComponentDto,
) -> Result<JObject<'local>, SiJniFailure> {
    let stream_type = boxed_int(env, value.stream_type)?;
    let stream_content = boxed_int(env, value.stream_content)?;
    let component_tag = boxed_int(env, value.component_tag)?;
    let component_type = boxed_int(env, value.component_type)?;
    let language = optional_string_object(env, value.language.as_deref())?;
    let second_language = optional_string_object(env, value.second_language.as_deref())?;
    let channel_configuration =
        optional_string_object(env, value.channel_configuration.as_deref())?;
    let simulcast_group_tag = boxed_int(env, value.simulcast_group_tag)?;
    let sampling_rate = boxed_int(env, value.sampling_rate)?;
    let sampling_info = optional_string_object(env, value.sampling_info.as_deref())?;
    let text = optional_string_object(env, value.text.as_deref())?;
    let source_descriptor = optional_string_object(env, value.source_descriptor.as_deref())?;
    let main = boxed_bool(env, value.main)?;
    let multi_lingual = boxed_bool(env, value.multi_lingual)?;
    let quality_indicator = boxed_int(env, value.quality_indicator)?;
    let parse_status = generated_enum(
        env,
        "SiParseStatusDto",
        parse_status_variant(&value.parse_status),
    )?;
    let channel_count = boxed_int(env, value.channel_count)?;
    let sample_rate_hz = boxed_int(env, value.sample_rate_hz)?;
    let audio_description = boxed_bool(env, value.audio_description)?;
    let hard_of_hearing = boxed_bool(env, value.hard_of_hearing)?;
    let dual_mono = boxed_bool(env, value.dual_mono)?;
    let result = new_generated(
        env,
        "AudioComponentDto",
        "(Ljava/lang/Integer;Ljava/lang/Integer;Ljava/lang/Integer;Ljava/lang/Integer;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/Integer;Ljava/lang/Integer;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/Boolean;Ljava/lang/Boolean;Ljava/lang/Integer;Lcom/maleicacid/tvinput/aribsi/generated/SiParseStatusDto;Ljava/lang/Integer;Ljava/lang/Integer;Ljava/lang/Boolean;Ljava/lang/Boolean;Ljava/lang/Boolean;)V",
        &[
            JValue::Object(&stream_type),
            JValue::Object(&stream_content),
            JValue::Object(&component_tag),
            JValue::Object(&component_type),
            JValue::Object(&language),
            JValue::Object(&second_language),
            JValue::Object(&channel_configuration),
            JValue::Object(&simulcast_group_tag),
            JValue::Object(&sampling_rate),
            JValue::Object(&sampling_info),
            JValue::Object(&text),
            JValue::Object(&source_descriptor),
            JValue::Object(&main),
            JValue::Object(&multi_lingual),
            JValue::Object(&quality_indicator),
            JValue::Object(&parse_status),
            JValue::Object(&channel_count),
            JValue::Object(&sample_rate_hz),
            JValue::Object(&audio_description),
            JValue::Object(&hard_of_hearing),
            JValue::Object(&dual_mono),
        ],
    );
    for object in [
        stream_type,
        stream_content,
        component_tag,
        component_type,
        language,
        second_language,
        channel_configuration,
        simulcast_group_tag,
        sampling_rate,
        sampling_info,
        text,
        source_descriptor,
        main,
        multi_lingual,
        quality_indicator,
        parse_status,
        channel_count,
        sample_rate_hz,
        audio_description,
        hard_of_hearing,
        dual_mono,
    ] {
        if !object.is_null() {
            jni_result(env.delete_local_ref(object))?;
        }
    }
    result
}

fn build_components<'local>(
    env: &mut JNIEnv<'local>,
    value: &ComponentsDto,
) -> Result<JObject<'local>, SiJniFailure> {
    let video = object_list(env, &value.video, build_video_component)?;
    let audio = object_list(env, &value.audio, build_audio_component)?;
    let result = new_generated(
        env,
        "ComponentsDto",
        "(Ljava/util/List;Ljava/util/List;)V",
        &[JValue::Object(&video), JValue::Object(&audio)],
    );
    for object in [video, audio] {
        jni_result(env.delete_local_ref(object))?;
    }
    result
}

fn build_parental_rating<'local>(
    env: &mut JNIEnv<'local>,
    value: &ParentalRatingDto,
) -> Result<JObject<'local>, SiJniFailure> {
    let country_code = string_object(env, &value.country_code)?;
    let parse_status = generated_enum(
        env,
        "SiParseStatusDto",
        parse_status_variant(&value.parse_status),
    )?;
    let result = new_generated(
        env,
        "ParentalRatingDto",
        "(Ljava/lang/String;ILcom/maleicacid/tvinput/aribsi/generated/SiParseStatusDto;)V",
        &[
            JValue::Object(&country_code),
            JValue::Int(value.raw_rating_byte),
            JValue::Object(&parse_status),
        ],
    );
    for object in [country_code, parse_status] {
        jni_result(env.delete_local_ref(object))?;
    }
    result
}

fn build_truncated_loop<'local>(
    env: &mut JNIEnv<'local>,
    value: &TruncatedDescriptorLoopDto,
) -> Result<JObject<'local>, SiJniFailure> {
    let raw_bytes_hex = string_object(env, &value.raw_bytes_hex)?;
    let parse_status = generated_enum(
        env,
        "SiParseStatusDto",
        parse_status_variant(&value.parse_status),
    )?;
    let result = new_generated(
        env,
        "TruncatedDescriptorLoopDto",
        "(ILjava/lang/String;Lcom/maleicacid/tvinput/aribsi/generated/SiParseStatusDto;)V",
        &[
            JValue::Int(value.declared_length),
            JValue::Object(&raw_bytes_hex),
            JValue::Object(&parse_status),
        ],
    );
    for object in [raw_bytes_hex, parse_status] {
        jni_result(env.delete_local_ref(object))?;
    }
    result
}

fn build_descriptor_scope<'local>(
    env: &mut JNIEnv<'local>,
    value: &DescriptorDiagnosticScopeDto,
) -> Result<JObject<'local>, SiJniFailure> {
    let pid = boxed_int(env, value.pid)?;
    let table_id = boxed_int(env, value.table_id)?;
    let table_id_extension = boxed_int(env, value.table_id_extension)?;
    let version = boxed_int(env, value.version)?;
    let section_number = boxed_int(env, value.section_number)?;
    let original_network_id = boxed_int(env, value.original_network_id)?;
    let transport_stream_id = boxed_int(env, value.transport_stream_id)?;
    let service_id = boxed_int(env, value.service_id)?;
    let event_id = boxed_int(env, value.event_id)?;
    let result = new_generated(
        env,
        "DescriptorDiagnosticScopeDto",
        "(Ljava/lang/Integer;Ljava/lang/Integer;Ljava/lang/Integer;Ljava/lang/Integer;Ljava/lang/Integer;Ljava/lang/Integer;Ljava/lang/Integer;Ljava/lang/Integer;Ljava/lang/Integer;)V",
        &[
            JValue::Object(&pid),
            JValue::Object(&table_id),
            JValue::Object(&table_id_extension),
            JValue::Object(&version),
            JValue::Object(&section_number),
            JValue::Object(&original_network_id),
            JValue::Object(&transport_stream_id),
            JValue::Object(&service_id),
            JValue::Object(&event_id),
        ],
    );
    for object in [
        pid,
        table_id,
        table_id_extension,
        version,
        section_number,
        original_network_id,
        transport_stream_id,
        service_id,
        event_id,
    ] {
        if !object.is_null() {
            jni_result(env.delete_local_ref(object))?;
        }
    }
    result
}

fn build_descriptor_descriptor<'local>(
    env: &mut JNIEnv<'local>,
    value: &DescriptorDiagnosticDescriptorDto,
) -> Result<JObject<'local>, SiJniFailure> {
    let name = optional_string_object(env, value.name.as_deref())?;
    let parse_status = string_object(env, &value.parse_status)?;
    let raw_prefix_hex = string_object(env, &value.raw_prefix_hex)?;
    let result = new_generated(
        env,
        "DescriptorDiagnosticDescriptorDto",
        "(ILjava/lang/String;IIILjava/lang/String;Ljava/lang/String;)V",
        &[
            JValue::Int(value.tag),
            JValue::Object(&name),
            JValue::Int(value.offset),
            JValue::Int(value.declared_length),
            JValue::Int(value.actual_remaining_length),
            JValue::Object(&parse_status),
            JValue::Object(&raw_prefix_hex),
        ],
    );
    for object in [name, parse_status, raw_prefix_hex] {
        if !object.is_null() {
            jni_result(env.delete_local_ref(object))?;
        }
    }
    result
}

fn build_descriptor_diagnostic<'local>(
    env: &mut JNIEnv<'local>,
    value: &DescriptorDiagnosticDto,
) -> Result<JObject<'local>, SiJniFailure> {
    let schema = string_object(env, &value.schema)?;
    let severity = string_object(env, &value.severity)?;
    let code = string_object(env, &value.code)?;
    let scope = build_descriptor_scope(env, &value.scope)?;
    let descriptor = build_descriptor_descriptor(env, &value.descriptor)?;
    let message = string_object(env, &value.message)?;
    let result = new_generated(
        env,
        "DescriptorDiagnosticDto",
        "(Ljava/lang/String;ILjava/lang/String;Ljava/lang/String;Lcom/maleicacid/tvinput/aribsi/generated/DescriptorDiagnosticScopeDto;Lcom/maleicacid/tvinput/aribsi/generated/DescriptorDiagnosticDescriptorDto;Ljava/lang/String;)V",
        &[
            JValue::Object(&schema),
            JValue::Int(value.schema_version),
            JValue::Object(&severity),
            JValue::Object(&code),
            JValue::Object(&scope),
            JValue::Object(&descriptor),
            JValue::Object(&message),
        ],
    );
    for object in [schema, severity, code, scope, descriptor, message] {
        jni_result(env.delete_local_ref(object))?;
    }
    result
}

fn build_event_diagnostics<'local>(
    env: &mut JNIEnv<'local>,
    value: &EventDiagnosticsDto,
) -> Result<JObject<'local>, SiJniFailure> {
    let summary = string_object(env, &value.summary)?;
    let descriptor_diagnostics = object_list(
        env,
        &value.descriptor_diagnostics,
        build_descriptor_diagnostic,
    )?;
    let descriptor_diagnostics_canonical_json =
        string_object(env, &value.descriptor_diagnostics_canonical_json)?;
    let descriptor_facts_canonical_json =
        optional_string_object(env, value.descriptor_facts_canonical_json.as_deref())?;
    let text_diagnostics = string_list(env, &value.text_diagnostics)?;
    let truncated_descriptor_loop = match value.truncated_descriptor_loop.as_ref() {
        Some(value) => build_truncated_loop(env, value)?,
        None => JObject::null(),
    };
    let result = new_generated(
        env,
        "EventDiagnosticsDto",
        "(Ljava/lang/String;Ljava/util/List;Ljava/lang/String;Ljava/lang/String;Ljava/util/List;Lcom/maleicacid/tvinput/aribsi/generated/TruncatedDescriptorLoopDto;)V",
        &[
            JValue::Object(&summary),
            JValue::Object(&descriptor_diagnostics),
            JValue::Object(&descriptor_diagnostics_canonical_json),
            JValue::Object(&descriptor_facts_canonical_json),
            JValue::Object(&text_diagnostics),
            JValue::Object(&truncated_descriptor_loop),
        ],
    );
    for object in [
        summary,
        descriptor_diagnostics,
        descriptor_diagnostics_canonical_json,
        descriptor_facts_canonical_json,
        text_diagnostics,
        truncated_descriptor_loop,
    ] {
        if !object.is_null() {
            jni_result(env.delete_local_ref(object))?;
        }
    }
    result
}

fn build_program_source<'local>(
    env: &mut JNIEnv<'local>,
    value: &ProgramSourceDto,
) -> Result<JObject<'local>, SiJniFailure> {
    new_generated(
        env,
        "ProgramSourceDto",
        "(IIIII)V",
        &[
            JValue::Int(value.pid),
            JValue::Int(value.table_id),
            JValue::Int(value.version),
            JValue::Int(value.section_number),
            JValue::Int(value.last_section_number),
        ],
    )
}

fn build_event_descriptors<'local>(
    env: &mut JNIEnv<'local>,
    value: &EventDescriptorsDto,
) -> Result<JObject<'local>, SiJniFailure> {
    let short_events = object_list(env, &value.short_events, build_short_event)?;
    let extended_texts = object_list(env, &value.extended_texts, build_extended_text)?;
    let extended_items = object_list(env, &value.extended_items, build_extended_item)?;
    let component_text = optional_string_object(env, value.component_text.as_deref())?;
    let audio_component_text = optional_string_object(env, value.audio_component_text.as_deref())?;
    let content_genres = object_list(env, &value.content_genres, build_content_genre)?;
    let genre_supplement_text =
        optional_string_object(env, value.genre_supplement_text.as_deref())?;
    let event_groups = object_list(env, &value.event_groups, build_event_group)?;
    let component_groups = object_list(
        env,
        &value.component_groups,
        build_component_group_descriptor,
    )?;
    let linkage = object_list(env, &value.linkage, build_linkage)?;
    let free_ca_mode = match value.free_ca_mode.as_ref() {
        Some(value) => build_free_ca_mode(env, value)?,
        None => JObject::null(),
    };
    let series = match value.series.as_ref() {
        Some(value) => build_series(env, value)?,
        None => JObject::null(),
    };
    let series_candidates = object_list(env, &value.series_candidates, build_series)?;
    let series_candidates_canonical_json =
        optional_string_object(env, value.series_candidates_canonical_json.as_deref())?;
    let parental_ratings = object_list(env, &value.parental_ratings, build_parental_rating)?;
    let components = build_components(env, &value.components)?;
    let diagnostics = build_event_diagnostics(env, &value.diagnostics)?;
    let result = new_generated(
        env,
        "EventDescriptorsDto",
        "(Ljava/util/List;Ljava/util/List;Ljava/util/List;Ljava/lang/String;Ljava/lang/String;Ljava/util/List;Ljava/lang/String;Ljava/util/List;Ljava/util/List;Ljava/util/List;Lcom/maleicacid/tvinput/aribsi/generated/FreeCaModeDto;Lcom/maleicacid/tvinput/aribsi/generated/SeriesDto;Ljava/util/List;Ljava/lang/String;Ljava/util/List;Lcom/maleicacid/tvinput/aribsi/generated/ComponentsDto;Lcom/maleicacid/tvinput/aribsi/generated/EventDiagnosticsDto;)V",
        &[
            JValue::Object(&short_events),
            JValue::Object(&extended_texts),
            JValue::Object(&extended_items),
            JValue::Object(&component_text),
            JValue::Object(&audio_component_text),
            JValue::Object(&content_genres),
            JValue::Object(&genre_supplement_text),
            JValue::Object(&event_groups),
            JValue::Object(&component_groups),
            JValue::Object(&linkage),
            JValue::Object(&free_ca_mode),
            JValue::Object(&series),
            JValue::Object(&series_candidates),
            JValue::Object(&series_candidates_canonical_json),
            JValue::Object(&parental_ratings),
            JValue::Object(&components),
            JValue::Object(&diagnostics),
        ],
    );
    for object in [
        short_events,
        extended_texts,
        extended_items,
        component_text,
        audio_component_text,
        content_genres,
        genre_supplement_text,
        event_groups,
        component_groups,
        linkage,
        free_ca_mode,
        series,
        series_candidates,
        series_candidates_canonical_json,
        parental_ratings,
        components,
        diagnostics,
    ] {
        if !object.is_null() {
            jni_result(env.delete_local_ref(object))?;
        }
    }
    result
}

fn build_event<'local>(
    env: &mut JNIEnv<'local>,
    value: &EventDto,
) -> Result<JObject<'local>, SiJniFailure> {
    let service_key = build_service_key(env, &value.service_key)?;
    let stable_identity = optional_string_object(env, value.stable_identity.as_deref())?;
    let timing_state = generated_enum(
        env,
        "EitTimingStateDto",
        timing_variant(&value.timing_state),
    )?;
    let raw_start_time_hex = string_object(env, &value.raw_start_time_hex)?;
    let raw_duration_hex = string_object(env, &value.raw_duration_hex)?;
    let title = string_object(env, &value.title)?;
    let description = string_object(env, &value.description)?;
    let extended_description = string_object(env, &value.extended_description)?;
    let event_scope = string_object(env, &value.event_scope)?;
    let source = build_program_source(env, &value.source)?;
    let descriptors = build_event_descriptors(env, &value.descriptors)?;
    let result = new_generated(
        env,
        "EventDto",
        "(Lcom/maleicacid/tvinput/aribsi/generated/ServiceKeyDto;Ljava/lang/String;ILcom/maleicacid/tvinput/aribsi/generated/EitTimingStateDto;Ljava/lang/String;Ljava/lang/String;JJLjava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Lcom/maleicacid/tvinput/aribsi/generated/ProgramSourceDto;Lcom/maleicacid/tvinput/aribsi/generated/EventDescriptorsDto;)V",
        &[
            JValue::Object(&service_key),
            JValue::Object(&stable_identity),
            JValue::Int(value.event_id),
            JValue::Object(&timing_state),
            JValue::Object(&raw_start_time_hex),
            JValue::Object(&raw_duration_hex),
            JValue::Long(value.start_time_millis),
            JValue::Long(value.duration_millis),
            JValue::Object(&title),
            JValue::Object(&description),
            JValue::Object(&extended_description),
            JValue::Object(&event_scope),
            JValue::Object(&source),
            JValue::Object(&descriptors),
        ],
    );
    for object in [
        service_key,
        stable_identity,
        timing_state,
        raw_start_time_hex,
        raw_duration_hex,
        title,
        description,
        extended_description,
        event_scope,
        source,
        descriptors,
    ] {
        if !object.is_null() {
            jni_result(env.delete_local_ref(object))?;
        }
    }
    result
}

pub(super) fn snapshot_to_java<'local>(
    env: &mut JNIEnv<'local>,
    snapshot: BulkSnapshotDto,
) -> Result<JObject<'local>, SiJniFailure> {
    let broadcast_clock = match snapshot.broadcast_clock.as_ref() {
        Some(value) => build_broadcast_clock(env, value)?,
        None => JObject::null(),
    };
    let table_requirements =
        object_list(env, &snapshot.table_requirements, build_table_requirement)?;
    let cat_ca_metadata = object_list(env, &snapshot.cat_ca_metadata, build_ca_metadata)?;
    let malformed_ca_descriptor_diagnostics = object_list(
        env,
        &snapshot.malformed_ca_descriptor_diagnostics,
        build_malformed_ca,
    )?;
    let malformed_ca_descriptor_counts = object_list(
        env,
        &snapshot.malformed_ca_descriptor_counts,
        build_malformed_count,
    )?;
    let transport_semantic_facts =
        object_list(env, &snapshot.transport_semantic_facts, build_transport)?;
    let events = object_list(env, &snapshot.events, build_event)?;
    let eit_instances = object_list(env, &snapshot.eit_instances, build_eit_instance)?;
    let service_semantic_facts = object_list(
        env,
        &snapshot.service_semantic_facts,
        build_service_semantic_facts,
    )?;
    let parser_diagnostics =
        object_list(env, &snapshot.parser_diagnostics, build_parser_diagnostic)?;
    let result = new_generated(
        env,
        "BulkSnapshotDto",
        "(JJILcom/maleicacid/tvinput/aribsi/generated/BroadcastClockDto;Ljava/util/List;Ljava/util/List;Ljava/util/List;Ljava/util/List;Ljava/util/List;Ljava/util/List;Ljava/util/List;Ljava/util/List;Ljava/util/List;)V",
        &[
            JValue::Long(snapshot.collection_generation),
            JValue::Long(snapshot.ingest_sequence),
            JValue::Int(snapshot.discovery_stage),
            JValue::Object(&broadcast_clock),
            JValue::Object(&table_requirements),
            JValue::Object(&cat_ca_metadata),
            JValue::Object(&malformed_ca_descriptor_diagnostics),
            JValue::Object(&malformed_ca_descriptor_counts),
            JValue::Object(&transport_semantic_facts),
            JValue::Object(&events),
            JValue::Object(&eit_instances),
            JValue::Object(&service_semantic_facts),
            JValue::Object(&parser_diagnostics),
        ],
    );
    for object in [
        broadcast_clock,
        table_requirements,
        cat_ca_metadata,
        malformed_ca_descriptor_diagnostics,
        malformed_ca_descriptor_counts,
        transport_semantic_facts,
        events,
        eit_instances,
        service_semantic_facts,
        parser_diagnostics,
    ] {
        if !object.is_null() {
            jni_result(env.delete_local_ref(object))?;
        }
    }
    result
}

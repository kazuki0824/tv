use super::{
    BulkSnapshot, CaMetadataDto, ElementaryStreamDto, MalformedCaDescriptorDiagnosticDto,
    ParserDiagnosticDto, ServiceCaDescriptorDto, ServiceSemanticFactsDto, SiJniFailure,
    SiJniFailureReason, TableRequirementStatusDto, TransportSemanticFactsDto,
};
use jni::objects::{JObject, JValue};
use jni::JNIEnv;
use maleicacid_arib_si_engine_core::eit_instances::EitInstanceState;
use serde_json::{Map, Value};
use std::collections::BTreeSet;

const FACTORY: &str = "com/maleicacid/tvinput/aribsi/NativeSiJvmFactory";
const ARIB_PREFIX: &str = "com/maleicacid/tvinput/aribsi/";

fn output_failure(detail: impl ToString) -> SiJniFailure {
    SiJniFailureReason::JniOutput.failure(detail)
}

fn jni_result<T>(result: jni::errors::Result<T>) -> Result<T, SiJniFailure> {
    result.map_err(output_failure)
}

fn call_factory<'local>(
    env: &mut JNIEnv<'local>,
    name: &str,
    signature: &str,
    args: &[JValue<'_, '_>],
) -> Result<JObject<'local>, SiJniFailure> {
    jni_result(env.call_static_method(FACTORY, name, signature, args))?
        .l()
        .map_err(output_failure)
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

fn enum_object<'local>(
    env: &mut JNIEnv<'local>,
    class: &str,
    constant: &str,
) -> Result<JObject<'local>, SiJniFailure> {
    let signature = format!("L{class};");
    jni_result(env.get_static_field(class, constant, &signature))?
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

fn static_string_list<'local>(
    env: &mut JNIEnv<'local>,
    values: &[&'static str],
) -> Result<JObject<'local>, SiJniFailure> {
    object_list(env, values, |env, value| string_object(env, value))
}

fn int_list<'local>(
    env: &mut JNIEnv<'local>,
    values: impl IntoIterator<Item = i32>,
) -> Result<JObject<'local>, SiJniFailure> {
    let list = new_list(env)?;
    for value in values {
        let child = boxed_int(env, Some(value))?;
        add_to_list(env, &list, &child)?;
        jni_result(env.delete_local_ref(child))?;
    }
    Ok(list)
}

fn byte_array<'local>(
    env: &mut JNIEnv<'local>,
    bytes: &[u8],
) -> Result<JObject<'local>, SiJniFailure> {
    env.byte_array_from_slice(bytes)
        .map(JObject::from)
        .map_err(output_failure)
}

fn hex_bytes(value: &str) -> Result<Vec<u8>, SiJniFailure> {
    if value.len() % 2 != 0 {
        return Err(output_failure("SI snapshot内部hex長が奇数です"));
    }
    let mut out = Vec::with_capacity(value.len() / 2);
    for pair in value.as_bytes().chunks_exact(2) {
        let pair = std::str::from_utf8(pair).map_err(output_failure)?;
        let byte = u8::from_str_radix(pair, 16).map_err(output_failure)?;
        out.push(byte);
    }
    Ok(out)
}

fn json_object<'a>(
    value: &'a Value,
    context: &str,
) -> Result<&'a Map<String, Value>, SiJniFailure> {
    value
        .as_object()
        .ok_or_else(|| output_failure(format!("{context} がobjectではありません")))
}

fn json_field<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    context: &str,
) -> Result<&'a Value, SiJniFailure> {
    object
        .get(key)
        .ok_or_else(|| output_failure(format!("{context}.{key} がありません")))
}

fn json_array<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    context: &str,
) -> Result<&'a Vec<Value>, SiJniFailure> {
    json_field(object, key, context)?
        .as_array()
        .ok_or_else(|| output_failure(format!("{context}.{key} がarrayではありません")))
}

fn json_object_field<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    context: &str,
) -> Result<&'a Map<String, Value>, SiJniFailure> {
    json_field(object, key, context)?
        .as_object()
        .ok_or_else(|| output_failure(format!("{context}.{key} がobjectではありません")))
}

fn json_optional_object_field<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    context: &str,
) -> Result<Option<&'a Map<String, Value>>, SiJniFailure> {
    let value = json_field(object, key, context)?;
    if value.is_null() {
        Ok(None)
    } else {
        value
            .as_object()
            .map(Some)
            .ok_or_else(|| output_failure(format!("{context}.{key} がobject/nullではありません")))
    }
}

fn json_string<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    context: &str,
) -> Result<&'a str, SiJniFailure> {
    json_field(object, key, context)?
        .as_str()
        .ok_or_else(|| output_failure(format!("{context}.{key} がstringではありません")))
}

fn json_optional_string<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    context: &str,
) -> Result<Option<&'a str>, SiJniFailure> {
    let value = json_field(object, key, context)?;
    if value.is_null() {
        Ok(None)
    } else {
        value
            .as_str()
            .map(Some)
            .ok_or_else(|| output_failure(format!("{context}.{key} がstring/nullではありません")))
    }
}

fn json_i64(
    object: &Map<String, Value>,
    key: &str,
    context: &str,
) -> Result<i64, SiJniFailure> {
    json_field(object, key, context)?
        .as_i64()
        .ok_or_else(|| output_failure(format!("{context}.{key} がintegerではありません")))
}

fn json_i32(
    object: &Map<String, Value>,
    key: &str,
    context: &str,
) -> Result<i32, SiJniFailure> {
    i32::try_from(json_i64(object, key, context)?).map_err(output_failure)
}

fn json_optional_i32(
    object: &Map<String, Value>,
    key: &str,
    context: &str,
) -> Result<Option<i32>, SiJniFailure> {
    let value = json_field(object, key, context)?;
    if value.is_null() {
        Ok(None)
    } else {
        let raw = value
            .as_i64()
            .ok_or_else(|| output_failure(format!("{context}.{key} がinteger/nullではありません")))?;
        i32::try_from(raw).map(Some).map_err(output_failure)
    }
}

fn json_bool(
    object: &Map<String, Value>,
    key: &str,
    context: &str,
) -> Result<bool, SiJniFailure> {
    json_field(object, key, context)?
        .as_bool()
        .ok_or_else(|| output_failure(format!("{context}.{key} がbooleanではありません")))
}

fn json_optional_bool(
    object: &Map<String, Value>,
    key: &str,
    context: &str,
) -> Result<Option<bool>, SiJniFailure> {
    let value = json_field(object, key, context)?;
    if value.is_null() {
        Ok(None)
    } else {
        value
            .as_bool()
            .map(Some)
            .ok_or_else(|| output_failure(format!("{context}.{key} がboolean/nullではありません")))
    }
}

fn parse_status_constant(value: &str) -> Result<&'static str, SiJniFailure> {
    match value {
        "OK" => Ok("OK"),
        "MalformedLength" => Ok("MALFORMED_LENGTH"),
        "TruncatedDescriptor" => Ok("TRUNCATED_DESCRIPTOR"),
        "UnsupportedValue" => Ok("UNSUPPORTED_VALUE"),
        "InvalidSequence" => Ok("INVALID_SEQUENCE"),
        "UNRESOLVED" => Ok("UNRESOLVED"),
        other => Err(output_failure(format!(
            "Rust SI snapshotのparseStatusが未知です: {other}"
        ))),
    }
}

fn parse_status<'local>(
    env: &mut JNIEnv<'local>,
    value: &str,
) -> Result<JObject<'local>, SiJniFailure> {
    enum_object(
        env,
        &format!("{ARIB_PREFIX}SiParseStatus"),
        parse_status_constant(value)?,
    )
}

fn enum_by_wire<'local>(
    env: &mut JNIEnv<'local>,
    class_name: &str,
    value: &str,
    allowed: &[&str],
) -> Result<JObject<'local>, SiJniFailure> {
    if !allowed.contains(&value) {
        return Err(output_failure(format!(
            "Rust SI snapshotの{class_name}値が未知です: {value}"
        )));
    }
    enum_object(env, &format!("{ARIB_PREFIX}{class_name}"), value)
}

fn build_broadcast_clock<'local>(
    env: &mut JNIEnv<'local>,
    value: &super::BroadcastClockFactDto,
) -> Result<JObject<'local>, SiJniFailure> {
    call_factory(
        env,
        "broadcastClock",
        "(IIJ)Lcom/maleicacid/tvinput/aribsi/AribBroadcastClockFact;",
        &[
            JValue::Int(i32::from(value.table_id)),
            JValue::Int(i32::from(value.mjd)),
            JValue::Long(i64::from(value.millis_of_day)),
        ],
    )
}

fn build_table_requirement<'local>(
    env: &mut JNIEnv<'local>,
    value: &TableRequirementStatusDto,
) -> Result<JObject<'local>, SiJniFailure> {
    let component = string_object(env, value.component)?;
    let onid = boxed_int(env, value.original_network_id.map(i32::from))?;
    let tsid = boxed_int(env, value.transport_stream_id.map(i32::from))?;
    let sid = boxed_int(env, value.service_id.map(i32::from))?;
    let result = call_factory(
        env,
        "tableRequirement",
        "(Ljava/lang/String;Ljava/lang/Integer;Ljava/lang/Integer;Ljava/lang/Integer;ZZ)Lcom/maleicacid/tvinput/aribsi/TableRequirementStatus;",
        &[
            JValue::Object(&component),
            JValue::Object(&onid),
            JValue::Object(&tsid),
            JValue::Object(&sid),
            JValue::Bool(u8::from(value.required)),
            JValue::Bool(u8::from(value.complete)),
        ],
    );
    jni_result(env.delete_local_ref(component))?;
    if !onid.is_null() {
        jni_result(env.delete_local_ref(onid))?;
    }
    if !tsid.is_null() {
        jni_result(env.delete_local_ref(tsid))?;
    }
    if !sid.is_null() {
        jni_result(env.delete_local_ref(sid))?;
    }
    result
}

fn build_ca_metadata<'local>(
    env: &mut JNIEnv<'local>,
    value: &CaMetadataDto,
) -> Result<JObject<'local>, SiJniFailure> {
    let onid = boxed_int(
        env,
        value.service_key.map(|key| i32::from(key.original_network_id)),
    )?;
    let tsid = boxed_int(
        env,
        value.service_key.map(|key| i32::from(key.transport_stream_id)),
    )?;
    let sid = boxed_int(env, value.service_key.map(|key| i32::from(key.service_id)))?;
    let ecm = boxed_int(env, value.ecm_pid.map(i32::from))?;
    let emm = boxed_int(env, value.emm_pid.map(i32::from))?;
    let elementary = boxed_int(env, value.elementary_pid.map(i32::from))?;
    let bytes = hex_bytes(&value.private_data_hex)?;
    let private_data = byte_array(env, &bytes)?;
    let source = enum_by_wire(
        env,
        "CaMetadataSource",
        value.source,
        &["PROGRAM", "ELEMENTARY_STREAM", "CAT"],
    )?;
    let result = call_factory(
        env,
        "caMetadata",
        "(Ljava/lang/Integer;Ljava/lang/Integer;Ljava/lang/Integer;ILjava/lang/Integer;Ljava/lang/Integer;Ljava/lang/Integer;[BLcom/maleicacid/tvinput/aribsi/CaMetadataSource;)Lcom/maleicacid/tvinput/aribsi/CaMetadata;",
        &[
            JValue::Object(&onid),
            JValue::Object(&tsid),
            JValue::Object(&sid),
            JValue::Int(i32::from(value.ca_system_id)),
            JValue::Object(&ecm),
            JValue::Object(&emm),
            JValue::Object(&elementary),
            JValue::Object(&private_data),
            JValue::Object(&source),
        ],
    );
    for object in [onid, tsid, sid, ecm, emm, elementary, private_data, source] {
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
    let extension = boxed_int(env, value.table_id_extension.map(i32::from))?;
    let service = boxed_int(env, value.service_id.map(i32::from))?;
    let elementary = boxed_int(env, value.elementary_pid.map(i32::from))?;
    let scope = string_object(env, value.scope)?;
    let reason = string_object(env, value.reason)?;
    let raw_prefix = string_object(env, &value.raw_prefix_hex)?;
    let result = call_factory(
        env,
        "malformedCaDescriptorDiagnostic",
        "(IILjava/lang/Integer;Ljava/lang/Integer;Ljava/lang/Integer;Ljava/lang/String;IIILjava/lang/String;Ljava/lang/String;)Lcom/maleicacid/tvinput/aribsi/MalformedCaDescriptorDiagnostic;",
        &[
            JValue::Int(i32::from(value.pid)),
            JValue::Int(i32::from(value.table_id)),
            JValue::Object(&extension),
            JValue::Object(&service),
            JValue::Object(&elementary),
            JValue::Object(&scope),
            JValue::Int(i32::try_from(value.offset).map_err(output_failure)?),
            JValue::Int(i32::try_from(value.declared_length).map_err(output_failure)?),
            JValue::Int(i32::try_from(value.actual_remaining_length).map_err(output_failure)?),
            JValue::Object(&reason),
            JValue::Object(&raw_prefix),
        ],
    );
    for object in [extension, service, elementary, scope, reason, raw_prefix] {
        if !object.is_null() {
            jni_result(env.delete_local_ref(object))?;
        }
    }
    result
}

fn build_transport<'local>(
    env: &mut JNIEnv<'local>,
    value: &TransportSemanticFactsDto,
) -> Result<JObject<'local>, SiJniFailure> {
    let network_name = optional_string_object(env, value.network_name.as_deref())?;
    let transport_name = optional_string_object(env, value.transport_stream_name.as_deref())?;
    let remote_key = boxed_int(env, value.remote_control_key_id.map(i32::from))?;
    let result = call_factory(
        env,
        "transport",
        "(IILjava/lang/String;Ljava/lang/String;ZLjava/lang/Integer;)Lcom/maleicacid/tvinput/aribsi/AribTransport;",
        &[
            JValue::Int(i32::from(value.original_network_id)),
            JValue::Int(i32::from(value.transport_stream_id)),
            JValue::Object(&network_name),
            JValue::Object(&transport_name),
            JValue::Bool(u8::from(value.sdt_actual)),
            JValue::Object(&remote_key),
        ],
    );
    for object in [network_name, transport_name, remote_key] {
        if !object.is_null() {
            jni_result(env.delete_local_ref(object))?;
        }
    }
    result
}

fn build_parser_diagnostic<'local>(
    env: &mut JNIEnv<'local>,
    value: &ParserDiagnosticDto,
) -> Result<JObject<'local>, SiJniFailure> {
    let code = string_object(env, value.code)?;
    let message = string_object(env, &value.message)?;
    let severity = string_object(env, value.severity)?;
    let result = call_factory(
        env,
        "parserDiagnostic",
        "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Lcom/maleicacid/tvinput/aribsi/ParserDiagnostic;",
        &[
            JValue::Object(&code),
            JValue::Object(&message),
            JValue::Object(&severity),
        ],
    );
    for object in [code, message, severity] {
        jni_result(env.delete_local_ref(object))?;
    }
    result
}

fn build_eit_instance<'local>(
    env: &mut JNIEnv<'local>,
    value: &EitInstanceState,
) -> Result<JObject<'local>, SiJniFailure> {
    let received = int_list(env, value.received_sections.iter().copied().map(i32::from))?;
    let missing = int_list(env, value.missing_sections.iter().copied().map(i32::from))?;
    let safe = int_list(env, value.safe_sections.iter().copied().map(i32::from))?;
    let result = call_factory(
        env,
        "eitInstance",
        "(IIIIIZILjava/util/List;Ljava/util/List;Ljava/util/List;ZZ)Lcom/maleicacid/tvinput/aribsi/EitInstanceState;",
        &[
            JValue::Int(i32::from(value.original_network_id)),
            JValue::Int(i32::from(value.transport_stream_id)),
            JValue::Int(i32::from(value.service_id)),
            JValue::Int(i32::from(value.table_id)),
            JValue::Int(i32::from(value.version)),
            JValue::Bool(u8::from(value.current_next_indicator)),
            JValue::Int(i32::from(value.last_section_number)),
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

fn build_codec_facts<'local>(
    env: &mut JNIEnv<'local>,
    stream: &ElementaryStreamDto,
) -> Result<JObject<'local>, SiJniFailure> {
    let avc = match stream.codec_facts.avc {
        Some(value) => call_factory(
            env,
            "avcSignaling",
            "(III)Lcom/maleicacid/tvinput/aribsi/AribAvcSignaling;",
            &[
                JValue::Int(i32::from(value.profile_idc)),
                JValue::Int(i32::from(value.constraint_flags)),
                JValue::Int(i32::from(value.level_idc)),
            ],
        )?,
        None => JObject::null(),
    };
    let audio_hex;
    let audio_header;
    if let Some(extension) = &stream.codec_facts.audio_extension {
        audio_hex = optional_string_object(env, extension.audio_specific_config_hex.as_deref())?;
        audio_header = match extension.header {
            Some(header) => {
                let extension_rate =
                    boxed_int(env, header.extension_sampling_frequency.and_then(|v| i32::try_from(v).ok()))?;
                let core_type = boxed_int(env, header.core_audio_object_type.map(i32::from))?;
                let channel_count = boxed_int(env, header.channel_count.map(i32::from))?;
                let value = call_factory(
                    env,
                    "audioConfigHeader",
                    "(IIILjava/lang/Integer;Ljava/lang/Integer;Ljava/lang/Integer;)Lcom/maleicacid/tvinput/aribsi/AribAudioConfigHeader;",
                    &[
                        JValue::Int(i32::from(header.audio_object_type)),
                        JValue::Int(i32::try_from(header.sampling_frequency).map_err(output_failure)?),
                        JValue::Int(i32::from(header.channel_configuration)),
                        JValue::Object(&extension_rate),
                        JValue::Object(&core_type),
                        JValue::Object(&channel_count),
                    ],
                )?;
                for object in [extension_rate, core_type, channel_count] {
                    if !object.is_null() {
                        jni_result(env.delete_local_ref(object))?;
                    }
                }
                value
            }
            None => JObject::null(),
        };
    } else {
        audio_hex = JObject::null();
        audio_header = JObject::null();
    }
    let raw = if stream.codec_facts.raw_descriptors_hex.is_empty() {
        JObject::null()
    } else {
        string_object(env, &stream.codec_facts.raw_descriptors_hex)?
    };
    let profile = optional_string_object(env, stream.codec_profile_level.as_deref())?;
    let result = call_factory(
        env,
        "codecFacts",
        "(Lcom/maleicacid/tvinput/aribsi/AribAvcSignaling;Ljava/lang/String;Lcom/maleicacid/tvinput/aribsi/AribAudioConfigHeader;Ljava/lang/String;Ljava/lang/String;Z)Lcom/maleicacid/tvinput/aribsi/AribCodecFacts;",
        &[
            JValue::Object(&avc),
            JValue::Object(&audio_hex),
            JValue::Object(&audio_header),
            JValue::Object(&raw),
            JValue::Object(&profile),
            JValue::Bool(u8::from(stream.codec_signaling_resolved)),
        ],
    );
    for object in [avc, audio_hex, audio_header, raw, profile] {
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
    let component_tag = boxed_int(env, value.component_tag.map(i32::from))?;
    let component_type = boxed_int(env, value.component_type.map(i32::from))?;
    let stream_content = boxed_int(env, value.stream_content.map(i32::from))?;
    let languages = string_list(env, &value.language_codes)?;
    let data_component = boxed_int(env, value.data_component_id.map(i32::from))?;
    let caption_dmf = boxed_int(env, value.caption_dmf.map(i32::from))?;
    let caption_timing = boxed_int(env, value.caption_timing.map(i32::from))?;
    let automatic = boxed_bool(env, value.automatic_presentation_on_reception)?;
    let codec = optional_string_object(env, value.codec)?;
    let codec_kind = match value.codec_kind {
        Some(kind) => enum_by_wire(env, "ElementaryStreamKind", kind, &["VIDEO", "AUDIO"])?,
        None => JObject::null(),
    };
    let codec_facts = build_codec_facts(env, value)?;
    let result = call_factory(
        env,
        "elementaryStream",
        "(IILjava/lang/Integer;Ljava/lang/Integer;Ljava/lang/Integer;Ljava/util/List;Ljava/lang/Integer;Ljava/lang/Integer;Ljava/lang/Integer;Ljava/lang/Boolean;ZZLjava/lang/String;Lcom/maleicacid/tvinput/aribsi/ElementaryStreamKind;Lcom/maleicacid/tvinput/aribsi/AribCodecFacts;)Lcom/maleicacid/tvinput/aribsi/AribElementaryStream;",
        &[
            JValue::Int(i32::from(value.elementary_pid)),
            JValue::Int(i32::from(value.stream_type)),
            JValue::Object(&component_tag),
            JValue::Object(&component_type),
            JValue::Object(&stream_content),
            JValue::Object(&languages),
            JValue::Object(&data_component),
            JValue::Object(&caption_dmf),
            JValue::Object(&caption_timing),
            JValue::Object(&automatic),
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
        languages,
        data_component,
        caption_dmf,
        caption_timing,
        automatic,
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
    let ca_pid = boxed_int(env, Some(i32::from(value.ca_pid)))?;
    let scope = enum_by_wire(env, "CaDescriptorScope", value.scope, &["PROGRAM", "ES"])?;
    let es_pid = boxed_int(env, value.es_pid.map(i32::from))?;
    let raw = byte_array(env, &hex_bytes(&value.raw_descriptor_hex)?)?;
    let private_data = byte_array(env, &hex_bytes(&value.private_data_hex)?)?;
    let result = call_factory(
        env,
        "caDescriptor",
        "(ILjava/lang/Integer;Lcom/maleicacid/tvinput/aribsi/CaDescriptorScope;Ljava/lang/Integer;[B[B)Lcom/maleicacid/tvinput/aribsi/CaDescriptor;",
        &[
            JValue::Int(i32::from(value.ca_system_id)),
            JValue::Object(&ca_pid),
            JValue::Object(&scope),
            JValue::Object(&es_pid),
            JValue::Object(&raw),
            JValue::Object(&private_data),
        ],
    );
    for object in [ca_pid, scope, es_pid, raw, private_data] {
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
    let service_type = boxed_int(env, value.service_type.map(i32::from))?;
    let streams = object_list(env, &value.elementary_streams, build_elementary_stream)?;
    let free_ca = boxed_bool(env, value.free_ca_mode)?;
    let system_management_id = boxed_int(env, value.smd.system_management_id.map(i32::from))?;
    let broadcasting_flag = boxed_int(env, value.smd.broadcasting_flag.map(i32::from))?;
    let broadcasting_identifier = boxed_int(env, value.smd.broadcasting_identifier.map(i32::from))?;
    let broadcast_system = match value.smd.broadcast_system {
        Some(system) => enum_by_wire(
            env,
            "BroadcastSystem",
            system,
            &["ISDB_T", "ISDB_S_BS", "ISDB_S_110CS"],
        )?,
        None => JObject::null(),
    };
    let additional_id = boxed_int(
        env,
        value
            .smd
            .additional_broadcasting_identification
            .map(i32::from),
    )?;
    let additional_hex = string_object(env, &value.smd.additional_identification_info_hex)?;
    let semantic_state = enum_by_wire(
        env,
        "SmdSemanticState",
        value.smd.semantic_state,
        &[
            "SUPPORTED_BROADCAST",
            "NON_BROADCAST",
            "UNDEFINED_BROADCAST_CLASS",
            "UNSUPPORTED_BROADCAST_SYSTEM",
            "UNDETERMINED_SMD",
        ],
    )?;
    let smd_diagnostic = optional_string_object(env, value.smd.diagnostic)?;
    let smd = call_factory(
        env,
        "smdSemanticFacts",
        "(ZZLjava/lang/Integer;Ljava/lang/Integer;Ljava/lang/Integer;Lcom/maleicacid/tvinput/aribsi/BroadcastSystem;Ljava/lang/Integer;Ljava/lang/String;Lcom/maleicacid/tvinput/aribsi/SmdSemanticState;Ljava/lang/String;)Lcom/maleicacid/tvinput/aribsi/SmdSemanticFacts;",
        &[
            JValue::Bool(u8::from(value.smd.descriptor_present)),
            JValue::Bool(u8::from(value.smd.syntax_valid)),
            JValue::Object(&system_management_id),
            JValue::Object(&broadcasting_flag),
            JValue::Object(&broadcasting_identifier),
            JValue::Object(&broadcast_system),
            JValue::Object(&additional_id),
            JValue::Object(&additional_hex),
            JValue::Object(&semantic_state),
            JValue::Object(&smd_diagnostic),
        ],
    )?;
    for object in [
        system_management_id,
        broadcasting_flag,
        broadcasting_identifier,
        broadcast_system,
        additional_id,
        additional_hex,
        semantic_state,
        smd_diagnostic,
    ] {
        if !object.is_null() {
            jni_result(env.delete_local_ref(object))?;
        }
    }
    let missing = static_string_list(env, &value.missing_components)?;
    let semantic = static_string_list(env, &value.semantic_diagnostics)?;
    let name = optional_string_object(env, value.name.as_deref())?;
    let provider_name = optional_string_object(env, value.provider_name.as_deref())?;
    let pmt_pid = boxed_int(env, value.pmt_pid.map(i32::from))?;
    let pcr_pid = boxed_int(env, value.pcr_pid.map(i32::from))?;
    let ca_descriptors =
        object_list(env, &value.service_scoped_ca_descriptors, build_ca_descriptor)?;
    let cas_json = serde_json::to_string(&value.cas_facts_canonical_json)
        .map_err(output_failure)?;
    let cas_json = string_object(env, &cas_json)?;
    let result = call_factory(
        env,
        "serviceSemanticFacts",
        "(IIILjava/lang/Integer;ZZZLjava/util/List;ZZLjava/lang/Boolean;Lcom/maleicacid/tvinput/aribsi/SmdSemanticFacts;Ljava/util/List;Ljava/util/List;Ljava/lang/String;Ljava/lang/String;Ljava/lang/Integer;Ljava/lang/Integer;Ljava/util/List;Ljava/lang/String;)Lcom/maleicacid/tvinput/aribsi/ServiceSemanticFacts;",
        &[
            JValue::Int(i32::from(value.original_network_id)),
            JValue::Int(i32::from(value.transport_stream_id)),
            JValue::Int(i32::from(value.service_id)),
            JValue::Object(&service_type),
            JValue::Bool(u8::from(value.pmt_pid_resolved)),
            JValue::Bool(u8::from(value.pmt_parsed)),
            JValue::Bool(u8::from(value.pcr_pid_resolved)),
            JValue::Object(&streams),
            JValue::Bool(u8::from(value.requires_cas)),
            JValue::Bool(u8::from(value.ca_descriptors_resolved)),
            JValue::Object(&free_ca),
            JValue::Object(&smd),
            JValue::Object(&missing),
            JValue::Object(&semantic),
            JValue::Object(&name),
            JValue::Object(&provider_name),
            JValue::Object(&pmt_pid),
            JValue::Object(&pcr_pid),
            JValue::Object(&ca_descriptors),
            JValue::Object(&cas_json),
        ],
    );
    for object in [
        service_type,
        streams,
        free_ca,
        smd,
        missing,
        semantic,
        name,
        provider_name,
        pmt_pid,
        pcr_pid,
        ca_descriptors,
        cas_json,
    ] {
        if !object.is_null() {
            jni_result(env.delete_local_ref(object))?;
        }
    }
    result
}

fn build_short_event<'local>(
    env: &mut JNIEnv<'local>,
    value: &Value,
) -> Result<JObject<'local>, SiJniFailure> {
    let object = json_object(value, "shortEvent")?;
    let language = string_object(env, json_string(object, "languageCode", "shortEvent")?)?;
    let title = string_object(env, json_string(object, "title", "shortEvent")?)?;
    let text = string_object(env, json_string(object, "text", "shortEvent")?)?;
    let status = parse_status(env, json_string(object, "parseStatus", "shortEvent")?)?;
    let result = call_factory(
        env,
        "shortEvent",
        "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Lcom/maleicacid/tvinput/aribsi/SiParseStatus;)Lcom/maleicacid/tvinput/aribsi/AribShortEventText;",
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
    value: &Value,
) -> Result<JObject<'local>, SiJniFailure> {
    let object = json_object(value, "extendedText")?;
    let language = string_object(env, json_string(object, "languageCode", "extendedText")?)?;
    let text = string_object(env, json_string(object, "text", "extendedText")?)?;
    let status = parse_status(env, json_string(object, "parseStatus", "extendedText")?)?;
    let result = call_factory(
        env,
        "extendedText",
        "(Ljava/lang/String;Ljava/lang/String;Lcom/maleicacid/tvinput/aribsi/SiParseStatus;)Lcom/maleicacid/tvinput/aribsi/AribExtendedEventText;",
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
    value: &Value,
) -> Result<JObject<'local>, SiJniFailure> {
    let object = json_object(value, "extendedItem")?;
    let language = string_object(env, json_string(object, "languageCode", "extendedItem")?)?;
    let description = string_object(env, json_string(object, "description", "extendedItem")?)?;
    let text = string_object(env, json_string(object, "text", "extendedItem")?)?;
    let result = call_factory(
        env,
        "extendedItem",
        "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Lcom/maleicacid/tvinput/aribsi/AribExtendedItem;",
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

fn build_parental_rating<'local>(
    env: &mut JNIEnv<'local>,
    value: &Value,
) -> Result<JObject<'local>, SiJniFailure> {
    let object = json_object(value, "parentalRating")?;
    let country = string_object(env, json_string(object, "countryCode", "parentalRating")?)?;
    let status = parse_status(env, json_string(object, "parseStatus", "parentalRating")?)?;
    let result = call_factory(
        env,
        "parentalRating",
        "(Ljava/lang/String;ILcom/maleicacid/tvinput/aribsi/SiParseStatus;)Lcom/maleicacid/tvinput/aribsi/AribParentalRating;",
        &[
            JValue::Object(&country),
            JValue::Int(json_i32(object, "rawRatingByte", "parentalRating")?),
            JValue::Object(&status),
        ],
    );
    for object in [country, status] {
        jni_result(env.delete_local_ref(object))?;
    }
    result
}

fn build_content_genre<'local>(
    env: &mut JNIEnv<'local>,
    value: &Value,
) -> Result<JObject<'local>, SiJniFailure> {
    let object = json_object(value, "genre")?;
    let arib_name = string_object(env, json_string(object, "aribName", "genre")?)?;
    let status = parse_status(env, json_string(object, "parseStatus", "genre")?)?;
    let result = call_factory(
        env,
        "contentGenre",
        "(IIILjava/lang/String;Lcom/maleicacid/tvinput/aribsi/SiParseStatus;)Lcom/maleicacid/tvinput/aribsi/AribContentGenre;",
        &[
            JValue::Int(json_i32(object, "level1", "genre")?),
            JValue::Int(json_i32(object, "level2", "genre")?),
            JValue::Int(json_i32(object, "userNibble", "genre")?),
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
    value: &Value,
) -> Result<JObject<'local>, SiJniFailure> {
    let object = json_object(value, "eventGroupRef")?;
    call_factory(
        env,
        "eventGroupReference",
        "(II)Lcom/maleicacid/tvinput/aribsi/AribEventGroupReference;",
        &[
            JValue::Int(json_i32(object, "serviceId", "eventGroupRef")?),
            JValue::Int(json_i32(object, "eventId", "eventGroupRef")?),
        ],
    )
}

fn build_other_event_group_reference<'local>(
    env: &mut JNIEnv<'local>,
    value: &Value,
) -> Result<JObject<'local>, SiJniFailure> {
    let object = json_object(value, "otherEventGroupRef")?;
    call_factory(
        env,
        "otherNetworkEventGroupReference",
        "(IIII)Lcom/maleicacid/tvinput/aribsi/AribOtherNetworkEventGroupReference;",
        &[
            JValue::Int(json_i32(object, "originalNetworkId", "otherEventGroupRef")?),
            JValue::Int(json_i32(object, "transportStreamId", "otherEventGroupRef")?),
            JValue::Int(json_i32(object, "serviceId", "otherEventGroupRef")?),
            JValue::Int(json_i32(object, "eventId", "otherEventGroupRef")?),
        ],
    )
}

fn build_event_group<'local>(
    env: &mut JNIEnv<'local>,
    value: &Value,
) -> Result<JObject<'local>, SiJniFailure> {
    let object = json_object(value, "eventGroup")?;
    let events = object_list(
        env,
        json_array(object, "events", "eventGroup")?,
        build_event_group_reference,
    )?;
    let others = object_list(
        env,
        json_array(object, "otherNetworkEvents", "eventGroup")?,
        build_other_event_group_reference,
    )?;
    let private_data = string_object(env, json_string(object, "privateDataHex", "eventGroup")?)?;
    let status = parse_status(env, json_string(object, "parseStatus", "eventGroup")?)?;
    let result = call_factory(
        env,
        "eventGroup",
        "(ILjava/util/List;Ljava/util/List;Ljava/lang/String;Lcom/maleicacid/tvinput/aribsi/SiParseStatus;)Lcom/maleicacid/tvinput/aribsi/AribEventGroup;",
        &[
            JValue::Int(json_i32(object, "groupType", "eventGroup")?),
            JValue::Object(&events),
            JValue::Object(&others),
            JValue::Object(&private_data),
            JValue::Object(&status),
        ],
    );
    for child in [events, others, private_data, status] {
        jni_result(env.delete_local_ref(child))?;
    }
    result
}

fn build_component_group<'local>(
    env: &mut JNIEnv<'local>,
    value: &Value,
) -> Result<JObject<'local>, SiJniFailure> {
    let object = json_object(value, "componentGroup")?;
    let tags = int_list(
        env,
        json_array(object, "componentTags", "componentGroup")?
            .iter()
            .map(|value| {
                value
                    .as_i64()
                    .and_then(|value| i32::try_from(value).ok())
                    .ok_or_else(|| output_failure("componentTagがintegerではありません"))
            })
            .collect::<Result<Vec<_>, _>>()?,
    )?;
    let result = call_factory(
        env,
        "componentGroup",
        "(ILjava/util/List;)Lcom/maleicacid/tvinput/aribsi/AribComponentGroup;",
        &[
            JValue::Int(json_i32(object, "componentGroupId", "componentGroup")?),
            JValue::Object(&tags),
        ],
    );
    jni_result(env.delete_local_ref(tags))?;
    result
}

fn build_component_group_descriptor<'local>(
    env: &mut JNIEnv<'local>,
    value: &Value,
) -> Result<JObject<'local>, SiJniFailure> {
    let object = json_object(value, "componentGroupDescriptor")?;
    let groups = object_list(
        env,
        json_array(object, "groups", "componentGroupDescriptor")?,
        build_component_group,
    )?;
    let status = parse_status(
        env,
        json_string(object, "parseStatus", "componentGroupDescriptor")?,
    )?;
    let result = call_factory(
        env,
        "componentGroupDescriptor",
        "(ILjava/util/List;Lcom/maleicacid/tvinput/aribsi/SiParseStatus;)Lcom/maleicacid/tvinput/aribsi/AribComponentGroupDescriptor;",
        &[
            JValue::Int(json_i32(
                object,
                "componentGroupType",
                "componentGroupDescriptor",
            )?),
            JValue::Object(&groups),
            JValue::Object(&status),
        ],
    );
    for child in [groups, status] {
        jni_result(env.delete_local_ref(child))?;
    }
    result
}

fn build_linkage<'local>(
    env: &mut JNIEnv<'local>,
    value: &Value,
) -> Result<JObject<'local>, SiJniFailure> {
    let object = json_object(value, "linkage")?;
    let private_data = string_object(env, json_string(object, "privateDataPrefixHex", "linkage")?)?;
    let status = parse_status(env, json_string(object, "parseStatus", "linkage")?)?;
    let result = call_factory(
        env,
        "linkage",
        "(IIIILjava/lang/String;Lcom/maleicacid/tvinput/aribsi/SiParseStatus;)Lcom/maleicacid/tvinput/aribsi/AribLinkage;",
        &[
            JValue::Int(json_i32(object, "linkageType", "linkage")?),
            JValue::Int(json_i32(object, "originalNetworkId", "linkage")?),
            JValue::Int(json_i32(object, "transportStreamId", "linkage")?),
            JValue::Int(json_i32(object, "serviceId", "linkage")?),
            JValue::Object(&private_data),
            JValue::Object(&status),
        ],
    );
    for child in [private_data, status] {
        jni_result(env.delete_local_ref(child))?;
    }
    result
}

fn build_free_ca_mode<'local>(
    env: &mut JNIEnv<'local>,
    object: &Map<String, Value>,
) -> Result<JObject<'local>, SiJniFailure> {
    let raw = boxed_int(env, json_optional_i32(object, "raw", "freeCaMode")?)?;
    let scrambled = boxed_bool(env, json_optional_bool(object, "scrambled", "freeCaMode")?)?;
    let status = parse_status(env, json_string(object, "parseStatus", "freeCaMode")?)?;
    let result = call_factory(
        env,
        "freeCaMode",
        "(Ljava/lang/Integer;Ljava/lang/Boolean;Lcom/maleicacid/tvinput/aribsi/SiParseStatus;)Lcom/maleicacid/tvinput/aribsi/AribFreeCaMode;",
        &[
            JValue::Object(&raw),
            JValue::Object(&scrambled),
            JValue::Object(&status),
        ],
    );
    for child in [raw, scrambled, status] {
        if !child.is_null() {
            jni_result(env.delete_local_ref(child))?;
        }
    }
    result
}

fn build_series<'local>(
    env: &mut JNIEnv<'local>,
    object: &Map<String, Value>,
) -> Result<JObject<'local>, SiJniFailure> {
    let series_id = boxed_int(env, json_optional_i32(object, "seriesId", "series")?)?;
    let expire_date = boxed_int(env, json_optional_i32(object, "expireDate", "series")?)?;
    let episode = boxed_int(env, json_optional_i32(object, "episodeNumber", "series")?)?;
    let last_episode = boxed_int(env, json_optional_i32(object, "lastEpisodeNumber", "series")?)?;
    let name = optional_string_object(env, json_optional_string(object, "name", "series")?)?;
    let status = parse_status(env, json_string(object, "parseStatus", "series")?)?;
    let result = call_factory(
        env,
        "series",
        "(Ljava/lang/Integer;IIZLjava/lang/Integer;Ljava/lang/Integer;Ljava/lang/Integer;Ljava/lang/String;Lcom/maleicacid/tvinput/aribsi/SiParseStatus;)Lcom/maleicacid/tvinput/aribsi/AribSeries;",
        &[
            JValue::Object(&series_id),
            JValue::Int(json_i32(object, "repeatLabel", "series")?),
            JValue::Int(json_i32(object, "programPattern", "series")?),
            JValue::Bool(u8::from(json_bool(object, "expireDateValid", "series")?)),
            JValue::Object(&expire_date),
            JValue::Object(&episode),
            JValue::Object(&last_episode),
            JValue::Object(&name),
            JValue::Object(&status),
        ],
    );
    for child in [series_id, expire_date, episode, last_episode, name, status] {
        if !child.is_null() {
            jni_result(env.delete_local_ref(child))?;
        }
    }
    result
}

fn build_video_component<'local>(
    env: &mut JNIEnv<'local>,
    value: &Value,
) -> Result<JObject<'local>, SiJniFailure> {
    let object = json_object(value, "videoComponent")?;
    let stream_content = boxed_int(env, json_optional_i32(object, "streamContent", "videoComponent")?)?;
    let component_tag = boxed_int(env, json_optional_i32(object, "componentTag", "videoComponent")?)?;
    let component_type = boxed_int(env, json_optional_i32(object, "componentType", "videoComponent")?)?;
    let language = optional_string_object(env, json_optional_string(object, "language", "videoComponent")?)?;
    let text = optional_string_object(env, json_optional_string(object, "text", "videoComponent")?)?;
    let source = optional_string_object(env, json_optional_string(object, "sourceDescriptor", "videoComponent")?)?;
    let resolution = optional_string_object(env, json_optional_string(object, "resolution", "videoComponent")?)?;
    let scan = optional_string_object(env, json_optional_string(object, "scan", "videoComponent")?)?;
    let aspect = optional_string_object(env, json_optional_string(object, "aspect", "videoComponent")?)?;
    let profile = optional_string_object(env, json_optional_string(object, "profileLevel", "videoComponent")?)?;
    let status = parse_status(env, json_string(object, "parseStatus", "videoComponent")?)?;
    let result = call_factory(
        env,
        "videoComponentEntry",
        "(Ljava/lang/Integer;Ljava/lang/Integer;Ljava/lang/Integer;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Lcom/maleicacid/tvinput/aribsi/SiParseStatus;)Lcom/maleicacid/tvinput/aribsi/AribComponentEntry;",
        &[
            JValue::Object(&stream_content),
            JValue::Object(&component_tag),
            JValue::Object(&component_type),
            JValue::Object(&language),
            JValue::Object(&text),
            JValue::Object(&source),
            JValue::Object(&resolution),
            JValue::Object(&scan),
            JValue::Object(&aspect),
            JValue::Object(&profile),
            JValue::Object(&status),
        ],
    );
    for child in [
        stream_content,
        component_tag,
        component_type,
        language,
        text,
        source,
        resolution,
        scan,
        aspect,
        profile,
        status,
    ] {
        if !child.is_null() {
            jni_result(env.delete_local_ref(child))?;
        }
    }
    result
}

fn build_audio_component<'local>(
    env: &mut JNIEnv<'local>,
    value: &Value,
) -> Result<JObject<'local>, SiJniFailure> {
    let object = json_object(value, "audioComponent")?;
    let stream_type = boxed_int(env, json_optional_i32(object, "streamType", "audioComponent")?)?;
    let stream_content = boxed_int(env, json_optional_i32(object, "streamContent", "audioComponent")?)?;
    let component_tag = boxed_int(env, json_optional_i32(object, "componentTag", "audioComponent")?)?;
    let component_type = boxed_int(env, json_optional_i32(object, "componentType", "audioComponent")?)?;
    let language = optional_string_object(env, json_optional_string(object, "language", "audioComponent")?)?;
    let second_language = optional_string_object(env, json_optional_string(object, "secondLanguage", "audioComponent")?)?;
    let channel_configuration = optional_string_object(env, json_optional_string(object, "channelConfiguration", "audioComponent")?)?;
    let simulcast = boxed_int(env, json_optional_i32(object, "simulcastGroupTag", "audioComponent")?)?;
    let sampling_rate = boxed_int(env, json_optional_i32(object, "samplingRate", "audioComponent")?)?;
    let sampling_info = optional_string_object(env, json_optional_string(object, "samplingInfo", "audioComponent")?)?;
    let text = optional_string_object(env, json_optional_string(object, "text", "audioComponent")?)?;
    let source = optional_string_object(env, json_optional_string(object, "sourceDescriptor", "audioComponent")?)?;
    let main = boxed_bool(env, json_optional_bool(object, "main", "audioComponent")?)?;
    let multi = boxed_bool(env, json_optional_bool(object, "multiLingual", "audioComponent")?)?;
    let quality = boxed_int(env, json_optional_i32(object, "qualityIndicator", "audioComponent")?)?;
    let status = parse_status(env, json_string(object, "parseStatus", "audioComponent")?)?;
    let channel_count = boxed_int(env, json_optional_i32(object, "channelCount", "audioComponent")?)?;
    let sample_rate = boxed_int(env, json_optional_i32(object, "sampleRateHz", "audioComponent")?)?;
    let audio_description = boxed_bool(env, json_optional_bool(object, "audioDescription", "audioComponent")?)?;
    let hard_of_hearing = boxed_bool(env, json_optional_bool(object, "hardOfHearing", "audioComponent")?)?;
    let dual_mono = boxed_bool(env, json_optional_bool(object, "dualMono", "audioComponent")?)?;
    let result = call_factory(
        env,
        "audioComponentEntry",
        "(Ljava/lang/Integer;Ljava/lang/Integer;Ljava/lang/Integer;Ljava/lang/Integer;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/Integer;Ljava/lang/Integer;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/Boolean;Ljava/lang/Boolean;Ljava/lang/Integer;Lcom/maleicacid/tvinput/aribsi/SiParseStatus;Ljava/lang/Integer;Ljava/lang/Integer;Ljava/lang/Boolean;Ljava/lang/Boolean;Ljava/lang/Boolean;)Lcom/maleicacid/tvinput/aribsi/AribComponentEntry;",
        &[
            JValue::Object(&stream_type),
            JValue::Object(&stream_content),
            JValue::Object(&component_tag),
            JValue::Object(&component_type),
            JValue::Object(&language),
            JValue::Object(&second_language),
            JValue::Object(&channel_configuration),
            JValue::Object(&simulcast),
            JValue::Object(&sampling_rate),
            JValue::Object(&sampling_info),
            JValue::Object(&text),
            JValue::Object(&source),
            JValue::Object(&main),
            JValue::Object(&multi),
            JValue::Object(&quality),
            JValue::Object(&status),
            JValue::Object(&channel_count),
            JValue::Object(&sample_rate),
            JValue::Object(&audio_description),
            JValue::Object(&hard_of_hearing),
            JValue::Object(&dual_mono),
        ],
    );
    for child in [
        stream_type,
        stream_content,
        component_tag,
        component_type,
        language,
        second_language,
        channel_configuration,
        simulcast,
        sampling_rate,
        sampling_info,
        text,
        source,
        main,
        multi,
        quality,
        status,
        channel_count,
        sample_rate,
        audio_description,
        hard_of_hearing,
        dual_mono,
    ] {
        if !child.is_null() {
            jni_result(env.delete_local_ref(child))?;
        }
    }
    result
}

fn build_components<'local>(
    env: &mut JNIEnv<'local>,
    object: &Map<String, Value>,
) -> Result<JObject<'local>, SiJniFailure> {
    let video = object_list(env, json_array(object, "video", "components")?, build_video_component)?;
    let audio = object_list(env, json_array(object, "audio", "components")?, build_audio_component)?;
    let result = call_factory(
        env,
        "components",
        "(Ljava/util/List;Ljava/util/List;)Lcom/maleicacid/tvinput/aribsi/AribComponents;",
        &[JValue::Object(&video), JValue::Object(&audio)],
    );
    for child in [video, audio] {
        jni_result(env.delete_local_ref(child))?;
    }
    result
}

fn build_truncated_loop<'local>(
    env: &mut JNIEnv<'local>,
    object: &Map<String, Value>,
) -> Result<JObject<'local>, SiJniFailure> {
    let raw = string_object(env, json_string(object, "rawBytesHex", "truncatedDescriptorLoop")?)?;
    let status = parse_status(env, json_string(object, "parseStatus", "truncatedDescriptorLoop")?)?;
    let result = call_factory(
        env,
        "truncatedDescriptorLoop",
        "(ILjava/lang/String;Lcom/maleicacid/tvinput/aribsi/SiParseStatus;)Lcom/maleicacid/tvinput/aribsi/AribTruncatedDescriptorLoop;",
        &[
            JValue::Int(json_i32(object, "declaredLength", "truncatedDescriptorLoop")?),
            JValue::Object(&raw),
            JValue::Object(&status),
        ],
    );
    for child in [raw, status] {
        jni_result(env.delete_local_ref(child))?;
    }
    result
}

fn build_descriptor_diagnostic<'local>(
    env: &mut JNIEnv<'local>,
    value: &Value,
) -> Result<JObject<'local>, SiJniFailure> {
    let object = json_object(value, "descriptorDiagnostic")?;
    let scope = json_object_field(object, "scope", "descriptorDiagnostic")?;
    let descriptor = json_object_field(object, "descriptor", "descriptorDiagnostic")?;
    let schema = string_object(env, json_string(object, "schema", "descriptorDiagnostic")?)?;
    let severity = string_object(env, json_string(object, "severity", "descriptorDiagnostic")?)?;
    let code = string_object(env, json_string(object, "code", "descriptorDiagnostic")?)?;
    let pid = boxed_int(env, json_optional_i32(scope, "pid", "descriptorDiagnostic.scope")?)?;
    let table_id = boxed_int(env, json_optional_i32(scope, "tableId", "descriptorDiagnostic.scope")?)?;
    let table_extension = boxed_int(env, json_optional_i32(scope, "tableIdExtension", "descriptorDiagnostic.scope")?)?;
    let version = boxed_int(env, json_optional_i32(scope, "version", "descriptorDiagnostic.scope")?)?;
    let section = boxed_int(env, json_optional_i32(scope, "sectionNumber", "descriptorDiagnostic.scope")?)?;
    let onid = boxed_int(env, json_optional_i32(scope, "originalNetworkId", "descriptorDiagnostic.scope")?)?;
    let tsid = boxed_int(env, json_optional_i32(scope, "transportStreamId", "descriptorDiagnostic.scope")?)?;
    let sid = boxed_int(env, json_optional_i32(scope, "serviceId", "descriptorDiagnostic.scope")?)?;
    let event_id = boxed_int(env, json_optional_i32(scope, "eventId", "descriptorDiagnostic.scope")?)?;
    let name = optional_string_object(env, json_optional_string(descriptor, "name", "descriptorDiagnostic.descriptor")?)?;
    let parse_status = string_object(env, json_string(descriptor, "parseStatus", "descriptorDiagnostic.descriptor")?)?;
    let raw_prefix = string_object(env, json_string(descriptor, "rawPrefixHex", "descriptorDiagnostic.descriptor")?)?;
    let message = string_object(env, json_string(object, "message", "descriptorDiagnostic")?)?;
    let result = call_factory(
        env,
        "descriptorDiagnostic",
        "(Ljava/lang/String;ILjava/lang/String;Ljava/lang/String;Ljava/lang/Integer;Ljava/lang/Integer;Ljava/lang/Integer;Ljava/lang/Integer;Ljava/lang/Integer;Ljava/lang/Integer;Ljava/lang/Integer;Ljava/lang/Integer;Ljava/lang/Integer;ILjava/lang/String;IIILjava/lang/String;Ljava/lang/String;Ljava/lang/String;)Lcom/maleicacid/tvinput/aribsi/DescriptorDiagnostic;",
        &[
            JValue::Object(&schema),
            JValue::Int(json_i32(object, "schemaVersion", "descriptorDiagnostic")?),
            JValue::Object(&severity),
            JValue::Object(&code),
            JValue::Object(&pid),
            JValue::Object(&table_id),
            JValue::Object(&table_extension),
            JValue::Object(&version),
            JValue::Object(&section),
            JValue::Object(&onid),
            JValue::Object(&tsid),
            JValue::Object(&sid),
            JValue::Object(&event_id),
            JValue::Int(json_i32(descriptor, "tag", "descriptorDiagnostic.descriptor")?),
            JValue::Object(&name),
            JValue::Int(json_i32(descriptor, "offset", "descriptorDiagnostic.descriptor")?),
            JValue::Int(json_i32(descriptor, "declaredLength", "descriptorDiagnostic.descriptor")?),
            JValue::Int(json_i32(descriptor, "actualRemainingLength", "descriptorDiagnostic.descriptor")?),
            JValue::Object(&parse_status),
            JValue::Object(&raw_prefix),
            JValue::Object(&message),
        ],
    );
    for child in [
        schema,
        severity,
        code,
        pid,
        table_id,
        table_extension,
        version,
        section,
        onid,
        tsid,
        sid,
        event_id,
        name,
        parse_status,
        raw_prefix,
        message,
    ] {
        if !child.is_null() {
            jni_result(env.delete_local_ref(child))?;
        }
    }
    result
}

fn text_diagnostics(summary: &str) -> Vec<String> {
    summary
        .split([' ', '\n'])
        .filter(|item| {
            item.contains("unknownCount=")
                || item.contains("component=")
                || item.contains("audio=")
        })
        .map(str::to_string)
        .collect()
}

fn build_event_diagnostics<'local>(
    env: &mut JNIEnv<'local>,
    object: &Map<String, Value>,
) -> Result<JObject<'local>, SiJniFailure> {
    let summary_value = json_string(object, "summary", "eventDiagnostics")?;
    let summary = string_object(env, summary_value)?;
    let descriptor_diagnostics = object_list(
        env,
        json_array(object, "descriptorDiagnostics", "eventDiagnostics")?,
        build_descriptor_diagnostic,
    )?;
    let canonical = string_object(
        env,
        json_string(
            object,
            "descriptorDiagnosticsCanonicalJson",
            "eventDiagnostics",
        )?,
    )?;
    let facts = optional_string_object(
        env,
        json_optional_string(object, "descriptorFactsCanonicalJson", "eventDiagnostics")?,
    )?;
    let texts = string_list(env, &text_diagnostics(summary_value))?;
    let truncated = match json_optional_object_field(
        object,
        "truncatedDescriptorLoop",
        "eventDiagnostics",
    )? {
        Some(loop_object) => build_truncated_loop(env, loop_object)?,
        None => JObject::null(),
    };
    let result = call_factory(
        env,
        "eventDiagnostics",
        "(Ljava/lang/String;Ljava/util/List;Ljava/lang/String;Ljava/lang/String;Ljava/util/List;Lcom/maleicacid/tvinput/aribsi/AribTruncatedDescriptorLoop;)Lcom/maleicacid/tvinput/aribsi/AribEventDiagnostics;",
        &[
            JValue::Object(&summary),
            JValue::Object(&descriptor_diagnostics),
            JValue::Object(&canonical),
            JValue::Object(&facts),
            JValue::Object(&texts),
            JValue::Object(&truncated),
        ],
    );
    for child in [summary, descriptor_diagnostics, canonical, facts, texts, truncated] {
        if !child.is_null() {
            jni_result(env.delete_local_ref(child))?;
        }
    }
    result
}

fn build_event_descriptors<'local>(
    env: &mut JNIEnv<'local>,
    object: &Map<String, Value>,
) -> Result<JObject<'local>, SiJniFailure> {
    let short_values = json_array(object, "shortEvents", "eventDescriptors")?;
    let mut seen_short = BTreeSet::new();
    let filtered_short = short_values
        .iter()
        .filter(|value| {
            value
                .as_object()
                .and_then(|item| item.get("languageCode"))
                .and_then(Value::as_str)
                .is_some_and(|language| seen_short.insert(language.to_string()))
        })
        .cloned()
        .collect::<Vec<_>>();
    let short_events = object_list(env, &filtered_short, build_short_event)?;

    let extended_values = json_array(object, "extendedTexts", "eventDescriptors")?;
    let mut seen_extended = BTreeSet::new();
    let filtered_extended = extended_values
        .iter()
        .filter(|value| {
            value
                .as_object()
                .and_then(|item| item.get("languageCode"))
                .and_then(Value::as_str)
                .is_some_and(|language| seen_extended.insert(language.to_string()))
        })
        .cloned()
        .collect::<Vec<_>>();
    let extended_texts = object_list(env, &filtered_extended, build_extended_text)?;
    let extended_items = object_list(
        env,
        json_array(object, "extendedItems", "eventDescriptors")?,
        build_extended_item,
    )?;

    let component = json_object_field(object, "component", "eventDescriptors")?;
    let audio = json_object_field(object, "audio", "eventDescriptors")?;
    let genres = json_object_field(object, "genres", "eventDescriptors")?;
    let component_text =
        optional_string_object(env, json_optional_string(component, "text", "eventDescriptors.component")?)?;
    let audio_text = optional_string_object(
        env,
        json_optional_string(audio, "componentText", "eventDescriptors.audio")?,
    )?;
    let content_genres = object_list(
        env,
        json_array(genres, "content", "eventDescriptors.genres")?,
        build_content_genre,
    )?;
    let genre_supplement = optional_string_object(
        env,
        json_optional_string(
            genres,
            "genreSupplementText",
            "eventDescriptors.genres",
        )?,
    )?;
    let event_groups = object_list(
        env,
        json_array(object, "eventGroups", "eventDescriptors")?,
        build_event_group,
    )?;
    let component_groups = object_list(
        env,
        json_array(object, "componentGroups", "eventDescriptors")?,
        build_component_group_descriptor,
    )?;
    let linkage = object_list(
        env,
        json_array(object, "linkage", "eventDescriptors")?,
        build_linkage,
    )?;
    let free_ca = build_free_ca_mode(
        env,
        json_object_field(object, "freeCaMode", "eventDescriptors")?,
    )?;
    let series = match json_optional_object_field(object, "series", "eventDescriptors")? {
        Some(value) => build_series(env, value)?,
        None => JObject::null(),
    };
    let candidates = object_list(
        env,
        json_array(object, "seriesCandidates", "eventDescriptors")?,
        |env, value| build_series(env, json_object(value, "seriesCandidate")?),
    )?;
    let candidates_json = optional_string_object(
        env,
        json_optional_string(
            object,
            "seriesCandidatesCanonicalJson",
            "eventDescriptors",
        )?,
    )?;
    let parental = object_list(
        env,
        json_array(object, "parentalRatings", "eventDescriptors")?,
        build_parental_rating,
    )?;
    let components = build_components(
        env,
        json_object_field(object, "components", "eventDescriptors")?,
    )?;
    let diagnostics = build_event_diagnostics(
        env,
        json_object_field(object, "diagnostics", "eventDescriptors")?,
    )?;
    let result = call_factory(
        env,
        "eventDescriptors",
        "(Ljava/util/List;Ljava/util/List;Ljava/util/List;Ljava/lang/String;Ljava/lang/String;Ljava/util/List;Ljava/lang/String;Ljava/util/List;Ljava/util/List;Ljava/util/List;Lcom/maleicacid/tvinput/aribsi/AribFreeCaMode;Lcom/maleicacid/tvinput/aribsi/AribSeries;Ljava/util/List;Ljava/lang/String;Ljava/util/List;Lcom/maleicacid/tvinput/aribsi/AribComponents;Lcom/maleicacid/tvinput/aribsi/AribEventDiagnostics;)Lcom/maleicacid/tvinput/aribsi/AribEventDescriptors;",
        &[
            JValue::Object(&short_events),
            JValue::Object(&extended_texts),
            JValue::Object(&extended_items),
            JValue::Object(&component_text),
            JValue::Object(&audio_text),
            JValue::Object(&content_genres),
            JValue::Object(&genre_supplement),
            JValue::Object(&event_groups),
            JValue::Object(&component_groups),
            JValue::Object(&linkage),
            JValue::Object(&free_ca),
            JValue::Object(&series),
            JValue::Object(&candidates),
            JValue::Object(&candidates_json),
            JValue::Object(&parental),
            JValue::Object(&components),
            JValue::Object(&diagnostics),
        ],
    );
    for child in [
        short_events,
        extended_texts,
        extended_items,
        component_text,
        audio_text,
        content_genres,
        genre_supplement,
        event_groups,
        component_groups,
        linkage,
        free_ca,
        series,
        candidates,
        candidates_json,
        parental,
        components,
        diagnostics,
    ] {
        if !child.is_null() {
            jni_result(env.delete_local_ref(child))?;
        }
    }
    result
}

fn build_event<'local>(
    env: &mut JNIEnv<'local>,
    value: &Value,
) -> Result<JObject<'local>, SiJniFailure> {
    let object = json_object(value, "event")?;
    let service_key = json_object_field(object, "serviceKey", "event")?;
    let timing = json_object_field(object, "timing", "event")?;
    let source = json_object_field(object, "source", "event")?;
    let stable_identity =
        optional_string_object(env, json_optional_string(object, "stableIdentity", "event")?)?;
    let timing_state = enum_by_wire(
        env,
        "EitTimingState",
        json_string(timing, "state", "event.timing")?,
        &[
            "DEFINED",
            "UNDEFINED_TIME",
            "BOTH_TIMING_UNDEFINED",
            "MALFORMED_TIMING",
        ],
    )?;
    let raw_start = string_object(env, json_string(timing, "rawStartTimeHex", "event.timing")?)?;
    let raw_duration = string_object(env, json_string(timing, "rawDurationHex", "event.timing")?)?;
    let title = string_object(env, json_string(object, "title", "event")?)?;
    let description = string_object(env, json_string(object, "description", "event")?)?;
    let extended = string_object(env, json_string(object, "extendedDescription", "event")?)?;
    let event_scope = string_object(env, json_string(object, "eventScope", "event")?)?;
    let source_object = call_factory(
        env,
        "programSource",
        "(IIIII)Lcom/maleicacid/tvinput/aribsi/AribProgramSource;",
        &[
            JValue::Int(json_i32(source, "pid", "event.source")?),
            JValue::Int(json_i32(source, "tableId", "event.source")?),
            JValue::Int(json_i32(source, "version", "event.source")?),
            JValue::Int(json_i32(source, "sectionNumber", "event.source")?),
            JValue::Int(json_i32(source, "lastSectionNumber", "event.source")?),
        ],
    )?;
    let descriptors = build_event_descriptors(
        env,
        json_object_field(object, "descriptors", "event")?,
    )?;
    let result = call_factory(
        env,
        "event",
        "(IIILjava/lang/String;ILcom/maleicacid/tvinput/aribsi/EitTimingState;Ljava/lang/String;Ljava/lang/String;JJLjava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Lcom/maleicacid/tvinput/aribsi/AribProgramSource;Lcom/maleicacid/tvinput/aribsi/AribEventDescriptors;)Lcom/maleicacid/tvinput/aribsi/AribEvent;",
        &[
            JValue::Int(json_i32(service_key, "originalNetworkId", "event.serviceKey")?),
            JValue::Int(json_i32(service_key, "transportStreamId", "event.serviceKey")?),
            JValue::Int(json_i32(service_key, "serviceId", "event.serviceKey")?),
            JValue::Object(&stable_identity),
            JValue::Int(json_i32(object, "eventId", "event")?),
            JValue::Object(&timing_state),
            JValue::Object(&raw_start),
            JValue::Object(&raw_duration),
            JValue::Long(json_i64(timing, "startUtcMillis", "event.timing")?),
            JValue::Long(json_i64(timing, "durationMillis", "event.timing")?),
            JValue::Object(&title),
            JValue::Object(&description),
            JValue::Object(&extended),
            JValue::Object(&event_scope),
            JValue::Object(&source_object),
            JValue::Object(&descriptors),
        ],
    );
    for child in [
        stable_identity,
        timing_state,
        raw_start,
        raw_duration,
        title,
        description,
        extended,
        event_scope,
        source_object,
        descriptors,
    ] {
        if !child.is_null() {
            jni_result(env.delete_local_ref(child))?;
        }
    }
    result
}

fn build_malformed_count_map<'local>(
    env: &mut JNIEnv<'local>,
    snapshot: &BulkSnapshot,
) -> Result<JObject<'local>, SiJniFailure> {
    let service_ids = int_list(
        env,
        snapshot
            .malformed_ca_descriptor_counts
            .iter()
            .map(|value| i32::from(value.service_id)),
    )?;
    let counts = int_list(
        env,
        snapshot
            .malformed_ca_descriptor_counts
            .iter()
            .map(|value| i32::try_from(value.count).map_err(output_failure))
            .collect::<Result<Vec<_>, _>>()?,
    )?;
    let result = call_factory(
        env,
        "malformedCaDescriptorCountMap",
        "(Ljava/util/List;Ljava/util/List;)Ljava/util/Map;",
        &[JValue::Object(&service_ids), JValue::Object(&counts)],
    );
    for child in [service_ids, counts] {
        jni_result(env.delete_local_ref(child))?;
    }
    result
}

pub(super) fn snapshot_to_java<'local>(
    env: &mut JNIEnv<'local>,
    snapshot: BulkSnapshot,
) -> Result<JObject<'local>, SiJniFailure> {
    let broadcast_clock = match snapshot.broadcast_clock.as_ref() {
        Some(value) => build_broadcast_clock(env, value)?,
        None => JObject::null(),
    };
    let table_requirements =
        object_list(env, &snapshot.table_requirements, build_table_requirement)?;
    let cat_ca_metadata = object_list(env, &snapshot.cat_ca_metadata, build_ca_metadata)?;
    let malformed = object_list(
        env,
        &snapshot.malformed_ca_descriptor_diagnostics,
        build_malformed_ca,
    )?;
    let malformed_counts = build_malformed_count_map(env, &snapshot)?;
    let transports = object_list(env, &snapshot.transport_semantic_facts, build_transport)?;
    let events = object_list(env, &snapshot.events, build_event)?;
    let eit_instances = object_list(env, &snapshot.eit_instances, build_eit_instance)?;
    let service_facts =
        object_list(env, &snapshot.service_semantic_facts, build_service_semantic_facts)?;
    let parser_diagnostics =
        object_list(env, &snapshot.parser_diagnostics, build_parser_diagnostic)?;
    let result = call_factory(
        env,
        "snapshot",
        "(JJILcom/maleicacid/tvinput/aribsi/AribBroadcastClockFact;Ljava/util/List;Ljava/util/List;Ljava/util/List;Ljava/util/Map;Ljava/util/List;Ljava/util/List;Ljava/util/List;Ljava/util/List;Ljava/util/List;)Lcom/maleicacid/tvinput/aribsi/NativeSiSnapshot;",
        &[
            JValue::Long(i64::try_from(snapshot.collection_generation).map_err(output_failure)?),
            JValue::Long(i64::try_from(snapshot.ingest_sequence).map_err(output_failure)?),
            JValue::Int(snapshot.discovery_stage),
            JValue::Object(&broadcast_clock),
            JValue::Object(&table_requirements),
            JValue::Object(&cat_ca_metadata),
            JValue::Object(&malformed),
            JValue::Object(&malformed_counts),
            JValue::Object(&transports),
            JValue::Object(&events),
            JValue::Object(&eit_instances),
            JValue::Object(&service_facts),
            JValue::Object(&parser_diagnostics),
        ],
    );
    for child in [
        broadcast_clock,
        table_requirements,
        cat_ca_metadata,
        malformed,
        malformed_counts,
        transports,
        events,
        eit_instances,
        service_facts,
        parser_diagnostics,
    ] {
        if !child.is_null() {
            jni_result(env.delete_local_ref(child))?;
        }
    }
    result
}

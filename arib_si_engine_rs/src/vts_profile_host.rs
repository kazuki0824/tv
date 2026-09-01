mod arib_jis_x0208_table;
mod arib_string;
mod ca_descriptor;
mod descriptors;
mod discovery_requirements;
mod eit;
mod provider_data;
mod sections;

mod service_discovery {
    include!("service_discovery.rs");

    pub(super) fn vts_resolution_json(payloads: &[(u16, Vec<u8>)]) -> Result<serde_json::Value, String> {
        let mut engine = ServiceDiscoveryEngine::default();
        for (pid, bytes) in payloads {
            let mut offset = 0usize;
            while offset < bytes.len() {
                let header = crate::sections::parse_section_header(&bytes[offset..], 12)
                    .ok_or_else(|| format!("invalid section payload on PID {} at byte {}", pid, offset))?;
                let end = offset.checked_add(header.total_length)
                    .ok_or_else(|| "section length overflow".to_string())?;
                if end > bytes.len() {
                    return Err(format!("truncated section payload on PID {}", pid));
                }
                let section = &bytes[offset..end];
                if header.syntax && !crate::sections::section_crc_valid(section, 12) {
                    return Err(format!("CRC error on PID {} table_id {}", pid, header.table_id));
                }
                engine.push_section(*pid, section);
                offset = end;
            }
        }

        let mut programs = Vec::new();
        for ((tsid, service_id), pmt_pid) in &engine.pat_programs {
            programs.push(serde_json::json!({
                "transport_stream_id": tsid,
                "service_id": service_id,
                "pmt_pid": pmt_pid,
            }));
        }
        programs.sort_by_key(|v| (
            v["transport_stream_id"].as_u64().unwrap_or(0),
            v["service_id"].as_u64().unwrap_or(0),
            v["pmt_pid"].as_u64().unwrap_or(0),
        ));

        let mut pmts = Vec::new();
        for ((tsid, service_id, pmt_pid), pmt) in &engine.unresolved_pmts_by_pat {
            let streams = pmt.streams.iter().map(|stream| serde_json::json!({
                "pid": stream.elementary_pid,
                "stream_type": stream.stream_type,
                "component_tag": stream.component_tag,
                "component_type": stream.component_type,
                "stream_content": stream.stream_content,
                "data_component_id": stream.data_component_id,
                "language_codes": stream.language_codes,
                "is_caption": stream.is_caption,
                "is_superimpose": stream.is_superimpose,
            })).collect::<Vec<_>>();
            pmts.push(serde_json::json!({
                "transport_stream_id": tsid,
                "service_id": service_id,
                "pmt_pid": pmt_pid,
                "pcr_pid": pmt.pcr_pid,
                "streams": streams,
            }));
        }
        pmts.sort_by_key(|v| (
            v["transport_stream_id"].as_u64().unwrap_or(0),
            v["service_id"].as_u64().unwrap_or(0),
            v["pmt_pid"].as_u64().unwrap_or(0),
        ));

        Ok(serde_json::json!({"programs": programs, "pmts": pmts}))
    }
}

fn decode_hex(text: &str) -> Result<Vec<u8>, String> {
    if text.len() % 2 != 0 {
        return Err("payload hex length must be even".to_string());
    }
    let mut out = Vec::with_capacity(text.len() / 2);
    for i in (0..text.len()).step_by(2) {
        let byte = u8::from_str_radix(&text[i..i + 2], 16)
            .map_err(|_| "payload contains non-hex characters".to_string())?;
        out.push(byte);
    }
    Ok(out)
}

fn parse_payload(value: &str) -> Result<(u16, Vec<u8>), String> {
    let (pid_text, hex) = value.split_once(':')
        .ok_or_else(|| "--payload must be PID:HEX".to_string())?;
    let pid = pid_text.parse::<u16>()
        .map_err(|_| "payload PID is not an integer".to_string())?;
    if pid > 0x1fff {
        return Err("payload PID must be in 0..8191".to_string());
    }
    Ok((pid, decode_hex(hex)?))
}

fn run() -> Result<(), String> {
    let mut payloads = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg != "--payload" {
            return Err(format!("unknown argument: {arg}"));
        }
        let value = args.next().ok_or_else(|| "--payload requires PID:HEX".to_string())?;
        payloads.push(parse_payload(&value)?);
    }
    if payloads.is_empty() {
        return Err("at least one --payload is required".to_string());
    }
    let value = service_discovery::vts_resolution_json(&payloads)?;
    println!("{}", serde_json::to_string(&value).map_err(|e| e.to_string())?);
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(2);
    }
}

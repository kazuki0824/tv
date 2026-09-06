use maleicacid_arib_si_engine_core::{
    sections::{parse_section_header, section_crc_valid},
    service_discovery::ServiceDiscoveryCollector,
};
use serde_json::{json, Value};

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
    let (pid_text, hex) = value
        .split_once(':')
        .ok_or_else(|| "--payload must be PID:HEX".to_string())?;
    let pid = pid_text
        .parse::<u16>()
        .map_err(|_| "payload PID is not an integer".to_string())?;
    if pid > 0x1fff {
        return Err("payload PID must be in 0..8191".to_string());
    }
    Ok((pid, decode_hex(hex)?))
}

fn resolution_json(payloads: &[(u16, Vec<u8>)]) -> Result<Value, String> {
    let mut collector = ServiceDiscoveryCollector::default();
    for (pid, bytes) in payloads {
        let mut offset = 0usize;
        while offset < bytes.len() {
            let header = parse_section_header(&bytes[offset..]).ok_or_else(|| {
                format!("invalid section payload on PID {pid} at byte {offset}")
            })?;
            let end = offset
                .checked_add(header.total_length)
                .ok_or_else(|| "section length overflow".to_string())?;
            if end > bytes.len() {
                return Err(format!("truncated section payload on PID {pid}"));
            }
            let section = &bytes[offset..end];
            if header.syntax && !section_crc_valid(section) {
                return Err(format!(
                    "CRC error on PID {pid} table_id {}",
                    header.table_id
                ));
            }
            collector.push_section(*pid, section);
            offset = end;
        }
    }

    let pmt_pids = collector.pmt_pids_for_section_filters();
    let snapshot = collector.state().snapshot;
    let services = snapshot
        .services
        .into_iter()
        .filter_map(|service| {
            let pmt_pid = service.pmt_pid?;
            let streams = service
                .streams
                .into_iter()
                .map(|stream| {
                    json!({
                        "pid": stream.elementary_pid,
                        "stream_type": stream.stream_type,
                        "component_tag": stream.component_tag,
                        "component_type": stream.component_type,
                        "stream_content": stream.stream_content,
                        "data_component_id": stream.data_component_id,
                        "language_codes": stream.language_codes,
                        "is_caption": stream.is_caption,
                        "is_superimpose": stream.is_superimpose,
                    })
                })
                .collect::<Vec<_>>();
            Some(json!({
                "original_network_id": service.original_network_id,
                "transport_stream_id": service.transport_stream_id,
                "service_id": service.service_id,
                "pmt_pid": pmt_pid,
                "pcr_pid": service.pcr_pid,
                "streams": streams,
            }))
        })
        .collect::<Vec<_>>();

    Ok(json!({"pmt_pids": pmt_pids, "services": services}))
}

fn run() -> Result<(), String> {
    let mut payloads = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg != "--payload" {
            return Err(format!("unknown argument: {arg}"));
        }
        let value = args
            .next()
            .ok_or_else(|| "--payload requires PID:HEX".to_string())?;
        payloads.push(parse_payload(&value)?);
    }
    if payloads.is_empty() {
        return Err("at least one --payload is required".to_string());
    }
    let value = resolution_json(&payloads)?;
    println!(
        "{}",
        serde_json::to_string(&value).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(2);
    }
}

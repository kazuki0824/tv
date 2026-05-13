use crate::descriptors::json_escape;

const PROVIDER_SCHEMA_VERSION: i32 = 1;
const SOFT_LIMIT_BYTES: usize = 16 * 1024;
const HARD_LIMIT_BYTES: usize = 32 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderDataResult {
    pub json: String,
    pub signature: String,
    pub extracted_key: String,
}

pub fn build_program_key(onid: i32, tsid: i32, sid: i32, event_id: i32) -> String {
    format!("onid={};tsid={};sid={};event={}", onid, tsid, sid, event_id)
}

pub fn build_program_provider_data(request_json: &str) -> ProviderDataResult {
    let onid = json_i64(request_json, "originalNetworkId").unwrap_or(-1) as i32;
    let tsid = json_i64(request_json, "transportStreamId").unwrap_or(-1) as i32;
    let sid = json_i64(request_json, "serviceId").unwrap_or(-1) as i32;
    let event_id = json_i64(request_json, "eventId").unwrap_or(-1) as i32;
    let key = json_string_field(request_json, "programKey")
        .unwrap_or_else(|| build_program_key(onid, tsid, sid, event_id));
    let mut json = format!(
        "{{\"schemaVersion\":{},\"programKeyB64\":{},\"programKey\":{},\"serviceKey\":{{\"originalNetworkId\":{},\"transportStreamId\":{},\"serviceId\":{}}},\"eventId\":{},\"timing\":{{\"startUtcMillis\":{},\"durationMillis\":{}}},\"requiresCas\":{},\"unsupportedCas\":{},\"clearLivePlaybackSupported\":{},\"channelRegistrationReady\":{},\"epgPublishable\":{},\"publishStateSource\":{},\"extendedItems\":{},\"componentText\":{},\"audioComponentText\":{},\"audioLanguage\":{},\"broadcastGenre\":{},\"genreSupplementText\":{},\"eventGroupText\":{},\"freeCaText\":{},\"seriesName\":{},\"diagnosticText\":{},\"descriptorDiagnostics\":{},\"contentRatings\":{},\"parentalRatingDiagnostics\":{},\"unsupportedDescriptorDiagnostics\":{},\"videoFormat\":{},\"diagnostics\":{{\"currentProgramOverlapCount\":{},\"selectedProgramId\":{},\"selectionRule\":{},\"skippedUnresolvedTransport\":{},\"malformedCaDescriptorCount\":{},\"droppedRetryWindowCount\":{}}}}}",
        PROVIDER_SCHEMA_VERSION,
        json_string(&base64_url_no_pad(key.as_bytes())),
        json_string(&key),
        onid, tsid, sid, event_id,
        json_i64(request_json, "startTimeMillis").unwrap_or(0),
        json_i64(request_json, "durationMillis").unwrap_or(0),
        json_bool(json_bool_field(request_json, "requiresCas").unwrap_or(false)),
        json_bool(json_bool_field(request_json, "unsupportedCas").unwrap_or(false)),
        json_bool(json_bool_field(request_json, "clearLivePlaybackSupported").unwrap_or(false)),
        json_bool(json_bool_field(request_json, "channelRegistrationReady").unwrap_or(false)),
        json_bool(json_bool_field(request_json, "epgPublishable").unwrap_or(false)),
        json_string(&json_string_field(request_json, "publishStateSource").unwrap_or_else(|| "none".to_string())),
        json_raw_array_or_empty(request_json, "extendedItems"),
        json_string(&json_string_field(request_json, "componentText").unwrap_or_default()),
        json_string(&json_string_field(request_json, "audioComponentText").unwrap_or_default()),
        json_string(&json_string_field(request_json, "audioLanguage").unwrap_or_default()),
        json_string(&json_string_field(request_json, "broadcastGenre").unwrap_or_default()),
        json_string(&json_string_field(request_json, "genreSupplementText").unwrap_or_default()),
        json_string(&json_string_field(request_json, "eventGroupText").unwrap_or_default()),
        json_string(&json_string_field(request_json, "freeCaText").unwrap_or_default()),
        json_string(&json_string_field(request_json, "seriesName").unwrap_or_default()),
        json_string(&json_string_field(request_json, "diagnosticText").unwrap_or_default()),
        normalize_diagnostics_json(&json_raw_object_or_array(request_json, "descriptorDiagnostics").unwrap_or_else(|| "{\"schemaVersion\":1,\"diagnostics\":[]}".to_string())),
        json_raw_array_or_empty(request_json, "contentRatings"),
        json_raw_object_or_array(request_json, "parentalRatingDiagnostics").unwrap_or_else(|| "{\"schemaVersion\":1,\"diagnostics\":[]}".to_string()),
        normalize_diagnostics_json(&json_raw_object_or_array(request_json, "unsupportedDescriptorDiagnostics").unwrap_or_else(|| "{\"schemaVersion\":1,\"diagnostics\":[]}".to_string())),
        json_string(&json_string_field(request_json, "videoFormat").unwrap_or_default()),
        json_i64(request_json, "currentProgramOverlapCount").unwrap_or(0),
        json_i64(request_json, "selectedProgramId").unwrap_or(-1),
        json_string(&json_string_field(request_json, "selectionRule").unwrap_or_default()),
        json_bool(json_bool_field(request_json, "skippedUnresolvedTransport").unwrap_or(false)),
        json_i64(request_json, "malformedCaDescriptorCount").unwrap_or(0),
        json_i64(request_json, "droppedRetryWindowCount").unwrap_or(0),
    );
    enforce_program_provider_data_limit(&mut json, &key, onid, tsid, sid, event_id, json_i64(request_json, "startTimeMillis").unwrap_or(0), json_i64(request_json, "durationMillis").unwrap_or(0));
    ProviderDataResult { signature: sha256_hex(json.as_bytes()), json, extracted_key: key }
}

pub fn build_channel_provider_data(request_json: &str) -> ProviderDataResult {
    let onid = json_i64(request_json, "originalNetworkId").unwrap_or(-1);
    let tsid = json_i64(request_json, "transportStreamId").unwrap_or(-1);
    let sid = json_i64(request_json, "serviceId").unwrap_or(-1);
    let system = json_string_field(request_json, "system").unwrap_or_default();
    let freq = json_i64(request_json, "frequencyHz").unwrap_or(0);
    let selector_type = json_string_field(request_json, "streamSelectorType").unwrap_or_else(|| "NONE".to_string());
    let selector_value = json_string_field(request_json, "streamSelectorValue").unwrap_or_default();
    let key = format!("onid={};tsid={};sid={}", onid, tsid, sid);
    let mut json = format!(
        "{{\"schemaVersion\":{},\"channelKey\":{},\"serviceKey\":{{\"originalNetworkId\":{},\"transportStreamId\":{},\"serviceId\":{}}},\"system\":{},\"frequencyHz\":{},\"streamSelector\":{{\"type\":{},\"value\":{}}},\"streamSelectorType\":{},\"streamSelectorValue\":{},\"physicalChannel\":{},\"backendHint\":{},\"satelliteBand\":{},\"remoteControlKeyId\":{},\"requiresCas\":{},\"unsupportedCas\":{},\"clearLivePlaybackSupported\":{},\"channelRegistrationReady\":{},\"epgPublishable\":{}}}",
        PROVIDER_SCHEMA_VERSION,
        json_string(&key),
        onid, tsid, sid,
        json_string(&system),
        freq,
        json_string(&selector_type), json_string(&selector_value),
        json_string(&selector_type), json_string(&selector_value),
        json_nullable_i64(json_i64(request_json, "physicalChannel")),
        json_string(&json_string_field(request_json, "backendHint").unwrap_or_default()),
        json_string(&json_string_field(request_json, "satelliteBand").unwrap_or_default()),
        json_nullable_i64(json_i64(request_json, "remoteControlKeyId")),
        json_bool(json_bool_field(request_json, "requiresCas").unwrap_or(false)),
        json_bool(json_bool_field(request_json, "unsupportedCas").unwrap_or(false)),
        json_bool(json_bool_field(request_json, "clearLivePlaybackSupported").unwrap_or(false)),
        json_bool(json_bool_field(request_json, "channelRegistrationReady").unwrap_or(false)),
        json_bool(json_bool_field(request_json, "epgPublishable").unwrap_or(false)),
    );
    enforce_provider_data_limit(&mut json);
    ProviderDataResult { signature: sha256_hex(json.as_bytes()), json, extracted_key: key }
}

pub fn normalize_program_provider_data(provider_data: &str) -> ProviderDataResult {
    let key = extract_program_key(provider_data).unwrap_or_default();
    let json = if provider_data.trim_start().starts_with('{') { provider_data.to_string() } else { format!("{{\"schemaVersion\":{},\"programKeyB64\":{},\"programKey\":{}}}", PROVIDER_SCHEMA_VERSION, json_string(&base64_url_no_pad(key.as_bytes())), json_string(&key)) };
    ProviderDataResult { signature: sha256_hex(json.as_bytes()), json, extracted_key: key }
}

pub fn append_current_program_diagnostics(provider_data: &str, overlap_count: i64, selected_program_id: i64, selection_rule: &str) -> ProviderDataResult {
    let key = extract_program_key(provider_data).unwrap_or_default();
    let mut root = if provider_data.trim_start().starts_with('{') { provider_data.trim().trim_end_matches('}').to_string() } else { format!("{{\"schemaVersion\":{},\"programKeyB64\":{},\"programKey\":{}", PROVIDER_SCHEMA_VERSION, json_string(&base64_url_no_pad(key.as_bytes())), json_string(&key)) };
    if root.ends_with('{') {
        root.push_str(&format!("\"diagnostics\":{{\"currentProgramOverlapCount\":{},\"selectedProgramId\":{},\"selectionRule\":{}}}}}", overlap_count.max(0), selected_program_id, json_string(selection_rule)));
    } else {
        root.push_str(&format!(",\"diagnostics\":{{\"currentProgramOverlapCount\":{},\"selectedProgramId\":{},\"selectionRule\":{}}}}}", overlap_count.max(0), selected_program_id, json_string(selection_rule)));
    }
    ProviderDataResult { signature: sha256_hex(root.as_bytes()), json: root, extracted_key: key }
}

pub fn extract_program_key(provider_data: &str) -> Option<String> {
    let raw = provider_data.trim();
    if raw.is_empty() { return None; }
    if raw.starts_with('{') {
        if let Some(key) = json_string_field(raw, "programKey") { return Some(key); }
        let encoded = json_string_field(raw, "programKeyB64")?;
        return base64_url_decode(&encoded).and_then(|v| String::from_utf8(v).ok());
    }
    if !raw.contains('=') { return Some(raw.to_string()); }
    for part in raw.split(';') {
        if let Some((k, v)) = part.split_once('=') {
            if k == "programKeyB64" { return base64_url_decode(v).and_then(|b| String::from_utf8(b).ok()); }
        }
    }
    None
}

pub fn extract_channel_tune_key(provider_data: &str) -> String {
    let raw = provider_data.trim();
    if raw.starts_with('{') {
        let onid = json_i64(raw, "originalNetworkId").unwrap_or_else(|| nested_service_key_i64(raw, "originalNetworkId").unwrap_or(-1));
        let tsid = json_i64(raw, "transportStreamId").unwrap_or_else(|| nested_service_key_i64(raw, "transportStreamId").unwrap_or(-1));
        let sid = json_i64(raw, "serviceId").unwrap_or_else(|| nested_service_key_i64(raw, "serviceId").unwrap_or(-1));
        let system = json_string_field(raw, "system").unwrap_or_default();
        let freq = json_i64(raw, "frequencyHz").unwrap_or(0);
        let selector_type = json_string_field(raw, "streamSelectorType").unwrap_or_else(|| json_string_field(raw, "type").unwrap_or_else(|| "NONE".to_string()));
        let selector_value = json_string_field(raw, "streamSelectorValue").unwrap_or_else(|| json_string_field(raw, "value").unwrap_or_default());
        return format!("originalNetworkId={};transportStreamId={};serviceId={};system={};frequencyHz={};streamSelectorType={};streamSelectorValue={};physicalChannel={};backendHint={};satelliteBand={};remoteControlKeyId={};requiresCas={};unsupportedCas={};clearLivePlaybackSupported={};channelRegistrationReady={};epgPublishable={}",
            onid, tsid, sid, system, freq, selector_type, selector_value,
            json_i64(raw, "physicalChannel").map(|v| v.to_string()).unwrap_or_default(),
            json_string_field(raw, "backendHint").unwrap_or_default(),
            json_string_field(raw, "satelliteBand").unwrap_or_default(),
            json_i64(raw, "remoteControlKeyId").map(|v| v.to_string()).unwrap_or_default(),
            json_bool_field(raw, "requiresCas").unwrap_or(false),
            json_bool_field(raw, "unsupportedCas").unwrap_or(false),
            json_bool_field(raw, "clearLivePlaybackSupported").unwrap_or(false),
            json_bool_field(raw, "channelRegistrationReady").unwrap_or(false),
            json_bool_field(raw, "epgPublishable").unwrap_or(false));
    }
    raw.to_string()
}

fn enforce_program_provider_data_limit(
    json: &mut String,
    key: &str,
    onid: i32,
    tsid: i32,
    sid: i32,
    event_id: i32,
    start_utc_millis: i64,
    duration_millis: i64,
) {
    if json.len() <= HARD_LIMIT_BYTES { return; }
    // 上限到達時も識別情報と時刻情報は残し、欠落を明示する。
    // TvProvider row は stable key extraction と obsolete delete の安全性のため、
    // JSONとして解析可能な形を維持する。
    *json = format!(
        "{{\"schemaVersion\":{},\"programKeyB64\":{},\"programKey\":{},\"serviceKey\":{{\"originalNetworkId\":{},\"transportStreamId\":{},\"serviceId\":{}}},\"eventId\":{},\"timing\":{{\"startUtcMillis\":{},\"durationMillis\":{}}},\"providerDataTruncated\":true,\"diagnostics\":{{\"providerDataHardLimitBytes\":{},\"providerDataSoftLimitBytes\":{}}}}}",
        PROVIDER_SCHEMA_VERSION,
        json_string(&base64_url_no_pad(key.as_bytes())),
        json_string(key),
        onid, tsid, sid, event_id, start_utc_millis, duration_millis, HARD_LIMIT_BYTES, SOFT_LIMIT_BYTES,
    );
}

fn enforce_provider_data_limit(json: &mut String) {
    if json.len() <= HARD_LIMIT_BYTES { return; }
    *json = format!(
        "{{\"schemaVersion\":{},\"providerDataTruncated\":true,\"diagnostics\":{{\"providerDataHardLimitBytes\":{},\"providerDataSoftLimitBytes\":{}}}}}",
        PROVIDER_SCHEMA_VERSION, HARD_LIMIT_BYTES, SOFT_LIMIT_BYTES,
    );
}

fn normalize_diagnostics_json(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.starts_with('{') || trimmed.starts_with('[') { trimmed.to_string() } else { "{\"schemaVersion\":1,\"diagnostics\":[]}".to_string() }
}

fn json_string(value: &str) -> String { format!("\"{}\"", json_escape(value)) }
fn json_bool(value: bool) -> &'static str { if value { "true" } else { "false" } }
fn json_nullable_i64(value: Option<i64>) -> String { value.map(|v| v.to_string()).unwrap_or_else(|| "null".to_string()) }

fn json_string_field(input: &str, key: &str) -> Option<String> {
    let marker = format!("\"{}\"", key);
    let mut pos = input.find(&marker)? + marker.len();
    pos = input[pos..].find(':')? + pos + 1;
    let bytes = input.as_bytes();
    while pos < bytes.len() && bytes[pos].is_ascii_whitespace() { pos += 1; }
    if pos >= bytes.len() || bytes[pos] != b'\"' { return None; }
    pos += 1;
    let mut out = String::new();
    let mut escaped = false;
    for ch in input[pos..].chars() {
        if escaped {
            out.push(match ch { 'n' => '\n', 'r' => '\r', 't' => '\t', '\"' => '\"', '\\' => '\\', '/' => '/', _ => ch });
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '\"' {
            return Some(out);
        } else {
            out.push(ch);
        }
    }
    None
}

fn json_i64(input: &str, key: &str) -> Option<i64> {
    let marker = format!("\"{}\"", key);
    let mut pos = input.find(&marker)? + marker.len();
    pos = input[pos..].find(':')? + pos + 1;
    let rest = input[pos..].trim_start();
    let end = rest.find(|c: char| !(c == '-' || c == '+' || c.is_ascii_digit())).unwrap_or(rest.len());
    rest[..end].parse().ok()
}

fn nested_service_key_i64(input: &str, key: &str) -> Option<i64> {
    let service_pos = input.find("\"serviceKey\"")?;
    json_i64(&input[service_pos..], key)
}

fn json_bool_field(input: &str, key: &str) -> Option<bool> {
    let marker = format!("\"{}\"", key);
    let mut pos = input.find(&marker)? + marker.len();
    pos = input[pos..].find(':')? + pos + 1;
    let rest = input[pos..].trim_start();
    if rest.starts_with("true") { Some(true) } else if rest.starts_with("false") { Some(false) } else { None }
}

fn json_raw_array_or_empty(input: &str, key: &str) -> String { json_raw_object_or_array(input, key).filter(|s| s.trim_start().starts_with('[')).unwrap_or_else(|| "[]".to_string()) }

fn json_raw_object_or_array(input: &str, key: &str) -> Option<String> {
    let marker = format!("\"{}\"", key);
    let mut pos = input.find(&marker)? + marker.len();
    pos = input[pos..].find(':')? + pos + 1;
    let bytes = input.as_bytes();
    while pos < bytes.len() && bytes[pos].is_ascii_whitespace() { pos += 1; }
    if pos >= bytes.len() { return None; }
    let open = bytes[pos] as char;
    let close = (match open { '{' => '}', '[' => ']', _ => return None }) as u8;
    let open_b = open as u8;
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    for (i, &b) in bytes[pos..].iter().enumerate() {
        if in_string {
            if escaped { escaped = false; }
            else if b == b'\\' { escaped = true; }
            else if b == b'\"' { in_string = false; }
        } else if b == b'\"' { in_string = true; }
        else if b == open_b { depth += 1; }
        else if b == close { depth -= 1; if depth == 0 { return Some(input[pos..pos+i+1].to_string()); } }
    }
    None
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
fn base64_url_no_pad(bytes: &[u8]) -> String {
    let mut out = String::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let b0 = bytes[i];
        let b1 = if i + 1 < bytes.len() { bytes[i+1] } else { 0 };
        let b2 = if i + 2 < bytes.len() { bytes[i+2] } else { 0 };
        out.push(B64[(b0 >> 2) as usize] as char);
        out.push(B64[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        if i + 1 < bytes.len() { out.push(B64[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char); }
        if i + 2 < bytes.len() { out.push(B64[(b2 & 0x3f) as usize] as char); }
        i += 3;
    }
    out
}

fn base64_url_decode(s: &str) -> Option<Vec<u8>> {
    let mut vals = Vec::new();
    for b in s.bytes() {
        let v = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            b'=' => continue,
            _ => return None,
        };
        vals.push(v);
    }
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + 1 < vals.len() {
        let v0 = vals[i];
        let v1 = vals[i+1];
        out.push((v0 << 2) | (v1 >> 4));
        if i + 2 < vals.len() {
            let v2 = vals[i+2];
            out.push(((v1 & 0x0f) << 4) | (v2 >> 2));
            if i + 3 < vals.len() {
                let v3 = vals[i+3];
                out.push(((v2 & 0x03) << 6) | v3);
            }
        }
        i += 4;
    }
    Some(out)
}

fn sha256_hex(data: &[u8]) -> String {
    let digest = sha256(data);
    let mut out = String::with_capacity(64);
    for b in digest { out.push_str(&format!("{:02x}", b)); }
    out
}

fn sha256(data: &[u8]) -> [u8; 32] {
    const H0: [u32; 8] = [0x6a09e667,0xbb67ae85,0x3c6ef372,0xa54ff53a,0x510e527f,0x9b05688c,0x1f83d9ab,0x5be0cd19];
    const K: [u32; 64] = [
        0x428a2f98,0x71374491,0xb5c0fbcf,0xe9b5dba5,0x3956c25b,0x59f111f1,0x923f82a4,0xab1c5ed5,
        0xd807aa98,0x12835b01,0x243185be,0x550c7dc3,0x72be5d74,0x80deb1fe,0x9bdc06a7,0xc19bf174,
        0xe49b69c1,0xefbe4786,0x0fc19dc6,0x240ca1cc,0x2de92c6f,0x4a7484aa,0x5cb0a9dc,0x76f988da,
        0x983e5152,0xa831c66d,0xb00327c8,0xbf597fc7,0xc6e00bf3,0xd5a79147,0x06ca6351,0x14292967,
        0x27b70a85,0x2e1b2138,0x4d2c6dfc,0x53380d13,0x650a7354,0x766a0abb,0x81c2c92e,0x92722c85,
        0xa2bfe8a1,0xa81a664b,0xc24b8b70,0xc76c51a3,0xd192e819,0xd6990624,0xf40e3585,0x106aa070,
        0x19a4c116,0x1e376c08,0x2748774c,0x34b0bcb5,0x391c0cb3,0x4ed8aa4a,0x5b9cca4f,0x682e6ff3,
        0x748f82ee,0x78a5636f,0x84c87814,0x8cc70208,0x90befffa,0xa4506ceb,0xbef9a3f7,0xc67178f2];
    let mut msg = data.to_vec();
    let bit_len = (msg.len() as u64) * 8;
    msg.push(0x80);
    while (msg.len() % 64) != 56 { msg.push(0); }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    let mut h = H0;
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 { w[i] = u32::from_be_bytes([chunk[i*4], chunk[i*4+1], chunk[i*4+2], chunk[i*4+3]]); }
        for i in 16..64 {
            let s0 = w[i-15].rotate_right(7) ^ w[i-15].rotate_right(18) ^ (w[i-15] >> 3);
            let s1 = w[i-2].rotate_right(17) ^ w[i-2].rotate_right(19) ^ (w[i-2] >> 10);
            w[i] = w[i-16].wrapping_add(s0).wrapping_add(w[i-7]).wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) = (h[0],h[1],h[2],h[3],h[4],h[5],h[6],h[7]);
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            hh = g; g = f; f = e; e = d.wrapping_add(temp1); d = c; c = b; b = a; a = temp1.wrapping_add(temp2);
        }
        h[0]=h[0].wrapping_add(a); h[1]=h[1].wrapping_add(b); h[2]=h[2].wrapping_add(c); h[3]=h[3].wrapping_add(d);
        h[4]=h[4].wrapping_add(e); h[5]=h[5].wrapping_add(f); h[6]=h[6].wrapping_add(g); h[7]=h[7].wrapping_add(hh);
    }
    let mut out = [0u8; 32];
    for (i, v) in h.iter().enumerate() { out[i*4..i*4+4].copy_from_slice(&v.to_be_bytes()); }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sha256_known_vector() { assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"); }
    #[test]
    fn program_key_excludes_time() {
        assert_eq!(build_program_key(4, 100, 101, 300), "onid=4;tsid=100;sid=101;event=300");
    }
}

#[cfg(test)]
mod provider_data_completion_tests {
    use super::*;

    const PROGRAM_REQ: &str = r#"{"originalNetworkId":4,"transportStreamId":100,"serviceId":101,"eventId":300,"programKey":"onid=4;tsid=100;sid=101;event=300","startTimeMillis":1710000000000,"durationMillis":1800000,"requiresCas":false,"unsupportedCas":false,"clearLivePlaybackSupported":true,"channelRegistrationReady":true,"epgPublishable":true,"publishStateSource":"current","extendedItems":[{"description":"出演","text":"A"}],"descriptorDiagnostics":{"schemaVersion":1,"diagnostics":[{"parseStatus":"TruncatedDescriptor","tag":9,"offset":10,"declaredLength":4,"remainingLength":2,"rawPrefixHex":"0904","message":"short","serviceKey":{"originalNetworkId":4,"transportStreamId":100,"serviceId":101},"eventId":300,"pid":4096,"tableId":78,"sectionNumber":0}]},"contentRatings":["JPN_TV_PG12"]}"#;

    #[test]
    fn program_provider_data_json_v1_golden_shape() {
        let result = build_program_provider_data(PROGRAM_REQ);
        assert!(result.json.contains("\"schemaVersion\":1"));
        assert!(result.json.contains("\"programKey\":\"onid=4;tsid=100;sid=101;event=300\""));
        assert!(result.json.contains("\"descriptorDiagnostics\""));
        assert_eq!(result.extracted_key, "onid=4;tsid=100;sid=101;event=300");
        assert_eq!(extract_program_key(&result.json).as_deref(), Some("onid=4;tsid=100;sid=101;event=300"));
    }

    #[test]
    fn channel_provider_data_json_v1_extracts_tune_key() {
        let result = build_channel_provider_data(r#"{"originalNetworkId":4,"transportStreamId":100,"serviceId":101,"system":"ISDB_S","frequencyHz":1049480000,"streamSelectorType":"STREAM_ID","streamSelectorValue":"16433","requiresCas":true,"unsupportedCas":false,"clearLivePlaybackSupported":false,"channelRegistrationReady":true,"epgPublishable":true}"#);
        assert!(result.json.contains("\"channelKey\""));
        let tune = extract_channel_tune_key(&result.json);
        assert!(tune.contains("originalNetworkId=4"));
        assert!(tune.contains("frequencyHz=1049480000"));
        assert!(tune.contains("streamSelectorType=STREAM_ID"));
    }

    #[test]
    fn signature_is_deterministic() {
        let a = build_program_provider_data(PROGRAM_REQ);
        let b = build_program_provider_data(PROGRAM_REQ);
        assert_eq!(a.signature, b.signature);
        assert_eq!(a.json, b.json);
    }

    #[test]
    fn hard_limit_fallback_keeps_identity_and_valid_json() {
        let mut req = PROGRAM_REQ.to_string();
        req.insert_str(req.len() - 1, &format!(",\"diagnosticText\":\"{}\"", "x".repeat(HARD_LIMIT_BYTES + 1024)));
        let result = build_program_provider_data(&req);
        assert!(result.json.contains("\"providerDataTruncated\":true"));
        assert_eq!(extract_program_key(&result.json).as_deref(), Some("onid=4;tsid=100;sid=101;event=300"));
    }
}

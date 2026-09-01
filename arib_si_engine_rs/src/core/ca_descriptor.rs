#[derive(Clone, Debug, Default, Eq, PartialEq, Ord, PartialOrd)]
pub struct CaDescriptor {
    pub ca_system_id: u16,
    pub ca_pid: u16,
    pub private_data: Vec<u8>,
    pub raw_descriptor: Vec<u8>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Ord, PartialOrd)]
pub struct CaDescriptorParseContext {
    pub pid: u16,
    pub table_id: u8,
    pub table_id_extension: Option<u16>,
    pub service_id: Option<u16>,
    pub elementary_pid: Option<u16>,
    pub scope: &'static str,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Ord, PartialOrd)]
pub struct MalformedCaDescriptorDiagnostic {
    pub pid: u16,
    pub table_id: u8,
    pub table_id_extension: Option<u16>,
    pub service_id: Option<u16>,
    pub elementary_pid: Option<u16>,
    pub scope: &'static str,
    pub offset: usize,
    pub declared_length: usize,
    pub actual_remaining_length: usize,
    pub reason: &'static str,
    pub raw_prefix_hex: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CaDescriptorParseResult {
    pub descriptors: Vec<CaDescriptor>,
    pub diagnostics: Vec<MalformedCaDescriptorDiagnostic>,
}

pub fn parse_ca_descriptors(descriptors: &[u8]) -> Vec<CaDescriptor> {
    parse_ca_descriptors_with_diagnostics(descriptors, CaDescriptorParseContext::default())
        .descriptors
}

pub fn parse_ca_descriptors_with_diagnostics(
    descriptors: &[u8],
    context: CaDescriptorParseContext,
) -> CaDescriptorParseResult {
    let mut out = Vec::new();
    let mut diagnostics = Vec::new();
    let mut cursor = 0usize;
    while cursor + 2 <= descriptors.len() {
        let tag = descriptors[cursor];
        let len = descriptors[cursor + 1] as usize;
        let body_start = cursor + 2;
        let body_end = body_start.saturating_add(len);
        if body_end > descriptors.len() {
            if tag == 0x09 {
                diagnostics.push(malformed_diagnostic(
                    &context,
                    cursor,
                    len,
                    descriptors.len().saturating_sub(body_start),
                    "TRUNCATED_CA_DESCRIPTOR",
                    &descriptors[cursor..],
                ));
            }
            break;
        }
        if tag == 0x09 {
            if len >= 4 {
                let ca_system_id =
                    u16::from_be_bytes([descriptors[body_start], descriptors[body_start + 1]]);
                let ca_pid = (((descriptors[body_start + 2] & 0x1f) as u16) << 8)
                    | descriptors[body_start + 3] as u16;
                let private_data = descriptors[body_start + 4..body_end].to_vec();
                let raw_descriptor = descriptors[cursor..body_end].to_vec();
                out.push(CaDescriptor {
                    ca_system_id,
                    ca_pid,
                    private_data,
                    raw_descriptor,
                });
            } else {
                diagnostics.push(malformed_diagnostic(
                    &context,
                    cursor,
                    len,
                    descriptors.len().saturating_sub(body_start),
                    "SHORT_CA_DESCRIPTOR",
                    &descriptors[cursor..body_end],
                ));
            }
        }
        cursor = body_end;
    }
    CaDescriptorParseResult {
        descriptors: out,
        diagnostics,
    }
}

fn malformed_diagnostic(
    context: &CaDescriptorParseContext,
    offset: usize,
    declared_length: usize,
    actual_remaining_length: usize,
    reason: &'static str,
    raw: &[u8],
) -> MalformedCaDescriptorDiagnostic {
    MalformedCaDescriptorDiagnostic {
        pid: context.pid,
        table_id: context.table_id,
        table_id_extension: context.table_id_extension,
        service_id: context.service_id,
        elementary_pid: context.elementary_pid,
        scope: context.scope,
        offset,
        declared_length,
        actual_remaining_length,
        reason,
        raw_prefix_hex: hex_prefix(raw, 16),
    }
}

fn hex_prefix(bytes: &[u8], limit: usize) -> String {
    let mut out = String::new();
    for b in bytes.iter().take(limit) {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ca_descriptor_with_private_data() {
        let descriptors = [0x09, 0x06, 0x00, 0x05, 0xe1, 0x23, 0xaa, 0xbb];
        let ca = parse_ca_descriptors(&descriptors);
        assert_eq!(ca.len(), 1);
        assert_eq!(ca[0].ca_system_id, 0x0005);
        assert_eq!(ca[0].ca_pid, 0x0123);
        assert_eq!(ca[0].private_data, vec![0xaa, 0xbb]);
        assert_eq!(ca[0].raw_descriptor, descriptors.to_vec());
    }

    #[test]
    fn reports_truncated_ca_descriptor() {
        let descriptors = [0x09, 0x06, 0x00, 0x05];
        let result = parse_ca_descriptors_with_diagnostics(
            &descriptors,
            CaDescriptorParseContext {
                pid: 0x0100,
                table_id: 0x02,
                table_id_extension: Some(1),
                service_id: Some(1),
                elementary_pid: None,
                scope: "PMT_PROGRAM",
            },
        );
        assert!(result.descriptors.is_empty());
        assert_eq!(result.diagnostics.len(), 1);
        assert_eq!(result.diagnostics[0].reason, "TRUNCATED_CA_DESCRIPTOR");
        assert_eq!(result.diagnostics[0].pid, 0x0100);
    }
}

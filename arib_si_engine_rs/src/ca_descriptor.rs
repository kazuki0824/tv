#[derive(Clone, Debug, Default, Eq, PartialEq, Ord, PartialOrd)]
pub struct CaDescriptor {
    pub ca_system_id: u16,
    pub ca_pid: u16,
    pub private_data: Vec<u8>,
}

pub fn parse_ca_descriptors(descriptors: &[u8]) -> Vec<CaDescriptor> {
    let mut out = Vec::new();
    let mut cursor = 0usize;
    while cursor + 2 <= descriptors.len() {
        let tag = descriptors[cursor];
        let len = descriptors[cursor + 1] as usize;
        let body_start = cursor + 2;
        let body_end = body_start.saturating_add(len);
        if body_end > descriptors.len() {
            break;
        }
        if tag == 0x09 && len >= 4 {
            let ca_system_id = u16::from_be_bytes([descriptors[body_start], descriptors[body_start + 1]]);
            let ca_pid = (((descriptors[body_start + 2] & 0x1f) as u16) << 8) | descriptors[body_start + 3] as u16;
            let private_data = descriptors[body_start + 4..body_end].to_vec();
            out.push(CaDescriptor { ca_system_id, ca_pid, private_data });
        }
        cursor = body_end;
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
    }
}

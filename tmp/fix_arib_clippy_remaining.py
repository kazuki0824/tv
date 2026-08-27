from pathlib import Path

p = Path("arib_si_engine_rs/src/service_discovery.rs")
s = p.read_text()
cas = '''/// Raw conditional-access signaling facts. These fields deliberately remain
/// independent: PMT CA-descriptor observation, PMT resolution, and SDT/EIT
/// free_CA_mode are different broadcast facts and product policy must not
/// collapse one into another inside the SI engine.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CasSignalingFacts {
    pub ca_descriptors_resolved: bool,
    pub ca_descriptors_present: bool,
    pub free_ca_mode: Option<bool>,
}

impl ServiceSemanticFacts {
    pub fn cas_signaling_facts(&self) -> CasSignalingFacts {
        CasSignalingFacts {
            ca_descriptors_resolved: self.ca_descriptors_resolved,
            ca_descriptors_present: self.requires_cas,
            free_ca_mode: self.free_ca_mode,
        }
    }
}

'''
if cas not in s:
    raise SystemExit("CAS signaling helper block not found")
s = s.replace(cas, "", 1)
collector = '''    pub fn is_complete(&self) -> bool {
        self.state().is_complete()
    }

'''
if collector not in s:
    raise SystemExit("collector is_complete helper not found")
s = s.replace(collector, "", 1)
p.write_text(s)

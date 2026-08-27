use crate::arib_string::decode_arib_string_lossy;
use crate::ca_descriptor::{
    parse_ca_descriptors_with_diagnostics, CaDescriptor, CaDescriptorParseContext,
    MalformedCaDescriptorDiagnostic,
};
use crate::discovery_requirements::{optional_table_requirement, DiscoveryProfile};
use crate::eit::{EitEvent, EitStore, EitUpdateWindow};
use crate::sections::{parse_section_header, section_crc_valid};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DiscoveredElementaryStream {
    pub elementary_pid: u16,
    pub stream_type: u8,
    pub component_tag: Option<u8>,
    pub stream_content: Option<u8>,
    pub component_type: Option<u8>,
    pub data_component_id: Option<u16>,
    pub language_codes: Vec<String>,
    pub is_caption: bool,
    pub is_superimpose: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SmdSemanticState {
    SupportedBroadcast,
    NonBroadcast,
    UndefinedBroadcastClass,
    UnsupportedBroadcastSystem,
    #[default]
    UndeterminedSmd,
}

impl SmdSemanticState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SupportedBroadcast => "SUPPORTED_BROADCAST",
            Self::NonBroadcast => "NON_BROADCAST",
            Self::UndefinedBroadcastClass => "UNDEFINED_BROADCAST_CLASS",
            Self::UnsupportedBroadcastSystem => "UNSUPPORTED_BROADCAST_SYSTEM",
            Self::UndeterminedSmd => "UNDETERMINED_SMD",
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SystemManagementFacts {
    pub descriptor_present: bool,
    pub syntax_valid: bool,
    pub system_management_id: Option<u16>,
    pub broadcasting_flag: Option<u8>,
    pub broadcasting_identifier: Option<u8>,
    pub additional_broadcasting_identification: Option<u8>,
    pub additional_identification_info: Vec<u8>,
    pub semantic_state: SmdSemanticState,
    pub diagnostic: Option<&'static str>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DiscoveredService {
    pub transport_stream_id: u16,
    pub original_network_id: u16,
    pub service_id: u16,
    pub service_type: Option<u8>,
    pub service_name: Option<String>,
    pub provider_name: Option<String>,
    pub bouquet_name: Option<String>,
    pub network_name: Option<String>,
    pub ts_name: Option<String>,
    pub remote_control_key_id: Option<u8>,
    pub system_management: SystemManagementFacts,
    pub running_status: Option<u8>,
    pub free_ca_mode: Option<bool>,
    pub pmt_pid: Option<u16>,
    pub pmt_parsed: bool,
    pub pcr_pid: Option<u16>,
    pub streams: Vec<DiscoveredElementaryStream>,
    pub program_ca_descriptors: Vec<CaDescriptor>,
    pub es_ca_descriptors: Vec<EsCaMetadata>,
    pub text_decode_diagnostics: Vec<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EsCaMetadata {
    pub elementary_pid: u16,
    pub descriptors: Vec<CaDescriptor>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CatCaMetadata {
    pub descriptors: Vec<CaDescriptor>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DiscoveredTransport {
    pub transport_stream_id: u16,
    pub original_network_id: u16,
    pub network_name: Option<String>,
    pub ts_name: Option<String>,
    pub remote_control_key_id: Option<u8>,
    pub system_management: SystemManagementFacts,
    pub services: BTreeSet<u16>,
    pub text_decode_diagnostics: Vec<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DiscoverySnapshot {
    pub services: Vec<DiscoveredService>,
    pub transports: Vec<DiscoveredTransport>,
    pub pmt_pids_by_service: Vec<PmtPidMapping>,
    pub cat_ca: CatCaMetadata,
    pub malformed_ca_descriptor_diagnostics: Vec<MalformedCaDescriptorDiagnostic>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PmtPidMapping {
    pub transport_stream_id: u16,
    pub original_network_id: u16,
    pub service_id: u16,
    pub pmt_pid: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiscoveryPublishStage {
    Incomplete,
    Partial,
    Complete,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoverySnapshotEnvelope {
    pub stage: DiscoveryPublishStage,
    pub snapshot: DiscoverySnapshot,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TableRequirementStatus {
    pub component: &'static str,
    pub original_network_id: Option<u16>,
    pub transport_stream_id: Option<u16>,
    pub service_id: Option<u16>,
    pub required: bool,
    pub complete: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ServiceSemanticFacts {
    pub original_network_id: u16,
    pub transport_stream_id: u16,
    pub service_id: u16,
    pub service_type: Option<u8>,
    pub pmt_pid_resolved: bool,
    pub pmt_parsed: bool,
    pub pcr_pid_resolved: bool,
    pub elementary_streams: Vec<DiscoveredElementaryStream>,
    pub requires_cas: bool,
    pub ca_descriptors_resolved: bool,
    pub free_ca_mode: Option<bool>,
    pub system_management: SystemManagementFacts,
    pub missing_components: Vec<&'static str>,
    pub semantic_diagnostics: Vec<&'static str>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DiscoveryCollectionState {
    pub snapshot: DiscoverySnapshot,
    pub table_requirements: Vec<TableRequirementStatus>,
    pub semantic_facts_by_service: Vec<ServiceSemanticFacts>,
}

impl DiscoveryCollectionState {
    pub fn is_complete(&self) -> bool {
        !self.table_requirements.is_empty()
            && self
                .table_requirements
                .iter()
                .filter(|status| status.required)
                .all(|status| status.complete)
    }

    pub fn missing_components(&self) -> Vec<&'static str> {
        let mut missing = self
            .table_requirements
            .iter()
            .filter(|status| status.required && !status.complete)
            .map(|status| status.component)
            .collect::<Vec<_>>();
        missing.sort_unstable();
        missing.dedup();
        missing
    }

    pub fn missing_components_by_scope(&self) -> Vec<TableRequirementStatus> {
        self.table_requirements
            .iter()
            .filter(|status| status.required && !status.complete)
            .cloned()
            .collect()
    }

    #[cfg(test)]
    fn component_complete(&self, component: &str) -> bool {
        let statuses = self
            .table_requirements
            .iter()
            .filter(|status| status.required && status.component == component)
            .collect::<Vec<_>>();
        !statuses.is_empty() && statuses.iter().all(|status| status.complete)
    }

    pub fn is_partially_complete(&self) -> bool {
        !self.snapshot.services.is_empty()
    }

    pub fn publish_stage(&self) -> DiscoveryPublishStage {
        if self.is_complete() {
            DiscoveryPublishStage::Complete
        } else if self.is_partially_complete() {
            DiscoveryPublishStage::Partial
        } else {
            DiscoveryPublishStage::Incomplete
        }
    }

    #[cfg(test)]
    pub fn partial_snapshot(&self) -> Option<DiscoverySnapshot> {
        (!self.snapshot.services.is_empty()).then(|| self.snapshot.clone())
    }

    #[cfg(test)]
    pub fn best_available_snapshot(&self) -> Option<DiscoverySnapshotEnvelope> {
        if let Some(snapshot) = self.complete_snapshot() {
            return Some(DiscoverySnapshotEnvelope {
                stage: DiscoveryPublishStage::Complete,
                snapshot,
            });
        }
        self.partial_snapshot()
            .map(|snapshot| DiscoverySnapshotEnvelope {
                stage: DiscoveryPublishStage::Partial,
                snapshot,
            })
    }

    #[cfg(test)]
    pub fn complete_snapshot(&self) -> Option<DiscoverySnapshot> {
        self.is_complete().then(|| self.snapshot.clone())
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct PendingPmtInfo {
    pmt_pid: u16,
    pcr_pid: Option<u16>,
    streams: Vec<DiscoveredElementaryStream>,
    program_ca_descriptors: Vec<CaDescriptor>,
    es_ca_descriptors: Vec<EsCaMetadata>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct SectionTracker {
    version: Option<u8>,
    last_section_number: Option<u8>,
    seen_sections: BTreeSet<u8>,
    inconsistent: bool,
}

impl SectionTracker {
    fn mark_seen(&mut self, version: u8, section_number: u8, last_section_number: u8) {
        if self.version != Some(version) {
            self.version = Some(version);
            self.last_section_number = None;
            self.seen_sections.clear();
            self.inconsistent = false;
        }
        if section_number > last_section_number {
            self.inconsistent = true;
            return;
        }
        match self.last_section_number {
            Some(expected) if expected != last_section_number => {
                self.inconsistent = true;
                return;
            }
            None => self.last_section_number = Some(last_section_number),
            Some(_) => {}
        }
        self.seen_sections.insert(section_number);
    }

    fn is_complete(&self) -> bool {
        if self.inconsistent {
            return false;
        }
        let Some(last) = self.last_section_number else {
            return false;
        };
        (0..=last).all(|section_number| self.seen_sections.contains(&section_number))
    }
}

#[derive(Default)]
pub struct ServiceDiscoveryCollector {
    engine: ServiceDiscoveryEngine,
    discovery_profile: DiscoveryProfile,
    section_trackers: BTreeMap<(u16, u8, u16, u16), SectionTracker>,
    nit_transport_scopes: BTreeMap<(u8, u16), BTreeSet<(u16, u16)>>,
    bat_transport_scopes: BTreeMap<u16, BTreeSet<(u16, u16)>>,
    sdt_actual_transport_scopes: BTreeSet<(u16, u16)>,
    sdt_other_transport_scopes: BTreeSet<(u16, u16)>,
}

#[derive(Clone, Debug, Default)]
pub struct ServiceDiscoveryEngine {
    pat_programs: BTreeMap<(u16, u16), u16>,
    transports: BTreeMap<(u16, u16), DiscoveredTransport>,
    services: BTreeMap<(u16, u16, u16), DiscoveredService>,
    unresolved_pmts_by_pat: BTreeMap<(u16, u16, u16), PendingPmtInfo>,
    pending_pmts: BTreeMap<(u16, u16, u16, u16), PendingPmtInfo>,
    cat_ca: CatCaMetadata,
    malformed_ca_descriptor_diagnostics: Vec<MalformedCaDescriptorDiagnostic>,
    eit_store: EitStore,
}

impl ServiceDiscoveryEngine {
    pub fn push_section(&mut self, pid: u16, section: &[u8]) {
        if !valid_current_section(section) {
            return;
        }
        match section.first().copied() {
            Some(0x00) if pid == 0x0000 => self.parse_pat(section),
            Some(0x02) if self.pat_programs.values().any(|p| *p == pid) => {
                self.parse_pmt(pid, section)
            }
            Some(0x01) if pid == 0x0001 => self.parse_cat(section),
            Some(0x40 | 0x41) if pid == 0x0010 => self.parse_nit(section),
            Some(0x4a) if pid == 0x0011 => self.parse_bat(section),
            Some(0x42 | 0x46) if pid == 0x0011 => self.parse_sdt(section),
            Some(table) if pid == 0x0012 && (0x4e..=0x6f).contains(&table) => {
                self.eit_store.upsert_section(section)
            }
            _ => {}
        }
    }

    pub fn events(&self) -> Vec<EitEvent> {
        self.eit_store.snapshot_all_for_diagnostic()
    }

    pub fn take_epg_update_windows(&mut self) -> Vec<EitUpdateWindow> {
        self.eit_store
            .take_present_following_actual_update_windows()
    }

    pub fn is_known_pmt_pid(&self, pid: u16) -> bool {
        self.pat_programs.values().any(|pmt_pid| *pmt_pid == pid)
    }

    pub fn snapshot(&self) -> DiscoverySnapshot {
        let mut transports: Vec<DiscoveredTransport> = self.transports.values().cloned().collect();
        transports.sort_by_key(|t| (t.original_network_id, t.transport_stream_id));
        let mut services: Vec<DiscoveredService> = self.services.values().cloned().collect();
        services.sort_by_key(|s| (s.original_network_id, s.transport_stream_id, s.service_id));
        let mut pmt_pids_by_service = Vec::new();
        for service in &services {
            if let Some(pmt_pid) = service.pmt_pid {
                pmt_pids_by_service.push(PmtPidMapping {
                    transport_stream_id: service.transport_stream_id,
                    original_network_id: service.original_network_id,
                    service_id: service.service_id,
                    pmt_pid,
                });
            }
        }
        pmt_pids_by_service.sort_by_key(|m| {
            (
                m.original_network_id,
                m.transport_stream_id,
                m.service_id,
                m.pmt_pid,
            )
        });
        pmt_pids_by_service.dedup_by_key(|m| {
            (
                m.original_network_id,
                m.transport_stream_id,
                m.service_id,
                m.pmt_pid,
            )
        });
        DiscoverySnapshot {
            services,
            transports,
            pmt_pids_by_service,
            cat_ca: self.cat_ca.clone(),
            malformed_ca_descriptor_diagnostics: self.malformed_ca_descriptor_diagnostics.clone(),
        }
    }

    fn invalidate_table(&mut self, pid: u16, table_id: u8, table_extension: u16) {
        match (pid, table_id) {
            (0x0001, 0x01) => {
                self.cat_ca.descriptors.clear();
                self.malformed_ca_descriptor_diagnostics.retain(|d| {
                    !(d.pid == pid
                        && d.table_id == table_id
                        && d.table_id_extension == Some(table_extension))
                });
            }
            (0x0000, 0x00) => {
                self.pat_programs.clear();
                self.unresolved_pmts_by_pat.clear();
                self.pending_pmts.clear();
                for service in self.services.values_mut() {
                    service.pmt_pid = None;
                    service.pmt_parsed = false;
                    service.pcr_pid = None;
                    service.streams.clear();
                    service.program_ca_descriptors.clear();
                    service.es_ca_descriptors.clear();
                }
            }
            (_, 0x02) => {
                let program_number = table_extension;
                let affected_tsids: BTreeSet<u16> = self
                    .pat_programs
                    .iter()
                    .filter_map(|((tsid, pat_program), pmt_pid)| {
                        (*pat_program == program_number && *pmt_pid == pid).then_some(*tsid)
                    })
                    .collect();
                self.unresolved_pmts_by_pat
                    .retain(|(tsid, pat_program, pmt_pid), _| {
                        !(*pat_program == program_number
                            && *pmt_pid == pid
                            && affected_tsids.contains(tsid))
                    });
                self.pending_pmts
                    .retain(|(_, tsid, pat_program, pmt_pid), _| {
                        !(*pat_program == program_number
                            && *pmt_pid == pid
                            && affected_tsids.contains(tsid))
                    });
                for service in self.services.values_mut().filter(|service| {
                    service.pmt_pid == Some(pid)
                        && service.service_id == program_number
                        && affected_tsids.contains(&service.transport_stream_id)
                }) {
                    Self::clear_pmt_state(service);
                    self.malformed_ca_descriptor_diagnostics.retain(|d| {
                        !(d.pid == pid
                            && d.table_id == table_id
                            && d.service_id == Some(program_number))
                    });
                }
            }
            (0x0010, 0x40 | 0x41) => {}
            (0x0011, 0x42 | 0x46) => {}
            (0x0011, 0x4a) => {}
            _ => {}
        }
    }

    fn invalidate_nit_transport_metadata(&mut self, scopes: &BTreeSet<(u16, u16)>) {
        for (tsid, onid) in scopes {
            if let Some(transport) = self.transports.get_mut(&(*tsid, *onid)) {
                transport.network_name = None;
                transport.ts_name = None;
                transport.remote_control_key_id = None;
                transport.system_management = SystemManagementFacts::default();
                transport.text_decode_diagnostics.clear();
            }
            for service in self.services.values_mut().filter(|service| {
                service.transport_stream_id == *tsid && service.original_network_id == *onid
            }) {
                service.network_name = None;
                service.ts_name = None;
                service.remote_control_key_id = None;
                service.system_management = SystemManagementFacts::default();
            }
        }
    }

    fn invalidate_sdt_service_metadata(&mut self, tsid: u16, onid: u16) {
        for service in self.services.values_mut().filter(|service| {
            service.transport_stream_id == tsid && service.original_network_id == onid
        }) {
            service.provider_name = None;
            service.service_name = None;
            service.running_status = None;
            service.free_ca_mode = None;
            service.text_decode_diagnostics.retain(|diagnostic| {
                !diagnostic.starts_with("field=serviceProviderName")
                    && !diagnostic.starts_with("field=serviceName")
            });
        }
    }

    fn invalidate_bat_transport_membership(&mut self, scopes: &BTreeSet<(u16, u16)>) {
        for (tsid, onid) in scopes {
            for service in self.services.values_mut().filter(|service| {
                service.transport_stream_id == *tsid && service.original_network_id == *onid
            }) {
                service.bouquet_name = None;
                service
                    .text_decode_diagnostics
                    .retain(|diagnostic| !diagnostic.starts_with("field=bouquetName"));
            }
        }
    }

    fn service_entry_mut(
        &mut self,
        tsid: u16,
        onid: u16,
        service_id: u16,
    ) -> &mut DiscoveredService {
        self.services
            .entry((tsid, onid, service_id))
            .or_insert_with(|| DiscoveredService {
                transport_stream_id: tsid,
                original_network_id: onid,
                service_id,
                service_type: None,
                service_name: None,
                provider_name: None,
                bouquet_name: None,
                network_name: None,
                ts_name: None,
                remote_control_key_id: None,
                system_management: SystemManagementFacts::default(),
                running_status: None,
                free_ca_mode: None,
                pmt_pid: None,
                pmt_parsed: false,
                pcr_pid: None,
                streams: Vec::new(),
                program_ca_descriptors: Vec::new(),
                es_ca_descriptors: Vec::new(),
                text_decode_diagnostics: Vec::new(),
            })
    }

    fn pat_pmt_pid_candidate_for_service(&self, tsid: u16, service_id: u16) -> Option<u16> {
        self.pat_programs.get(&(tsid, service_id)).copied()
    }

    fn resolved_onid_for_pat_program(&self, tsid: u16, service_id: u16) -> Option<u16> {
        let mut onids: Vec<u16> = self
            .services
            .keys()
            .filter(|(service_tsid, _, sid)| *service_tsid == tsid && *sid == service_id)
            .map(|(_, onid, _)| *onid)
            .collect();
        onids.sort_unstable();
        onids.dedup();
        if onids.len() == 1 {
            Some(onids[0])
        } else {
            None
        }
    }

    fn clear_pmt_state(service: &mut DiscoveredService) {
        service.pmt_parsed = false;
        service.pcr_pid = None;
        service.streams.clear();
        service.program_ca_descriptors.clear();
        service.es_ca_descriptors.clear();
    }

    fn apply_pending_pmt_to_service(&mut self, tsid: u16, onid: u16, service_id: u16) {
        let Some(pmt_pid) = self.pat_pmt_pid_candidate_for_service(tsid, service_id) else {
            return;
        };
        let Some(resolved_onid) = self.resolved_onid_for_pat_program(tsid, service_id) else {
            return;
        };
        if resolved_onid != onid {
            return;
        }
        let full_key = (onid, tsid, service_id, pmt_pid);
        if let std::collections::btree_map::Entry::Vacant(e) = self.pending_pmts.entry(full_key) {
            if let Some(parsed) = self
                .unresolved_pmts_by_pat
                .get(&(tsid, service_id, pmt_pid))
                .cloned()
            {
                e.insert(parsed);
            }
        }
        let Some(pending) = self.pending_pmts.get(&full_key).cloned() else {
            return;
        };
        let entry = self.service_entry_mut(tsid, onid, service_id);
        entry.pmt_pid = Some(pending.pmt_pid);
        entry.pmt_parsed = true;
        entry.pcr_pid = pending.pcr_pid;
        entry.streams = pending.streams;
        entry.program_ca_descriptors = pending.program_ca_descriptors;
        entry.es_ca_descriptors = pending.es_ca_descriptors;
    }

    fn transport_entry_mut(&mut self, tsid: u16, onid: u16) -> &mut DiscoveredTransport {
        self.transports
            .entry((tsid, onid))
            .or_insert_with(|| DiscoveredTransport {
                transport_stream_id: tsid,
                original_network_id: onid,
                network_name: None,
                ts_name: None,
                remote_control_key_id: None,
                system_management: SystemManagementFacts::default(),
                services: BTreeSet::new(),
                text_decode_diagnostics: Vec::new(),
            })
    }

    fn propagate_transport_metadata_to_services(&mut self, tsid: u16, onid: u16) {
        let Some(transport) = self.transports.get(&(tsid, onid)).cloned() else {
            return;
        };
        for service in self.services.values_mut().filter(|service| {
            service.transport_stream_id == tsid && service.original_network_id == onid
        }) {
            if service.network_name.is_none() {
                service.network_name = transport.network_name.clone();
            }
            if service.ts_name.is_none() {
                service.ts_name = transport.ts_name.clone();
            }
            if service.remote_control_key_id.is_none() {
                service.remote_control_key_id = transport.remote_control_key_id;
            }
            service.system_management = transport.system_management.clone();
        }
    }

    fn parse_pat(&mut self, section: &[u8]) {
        if section.len() < 8 {
            return;
        }
        let tsid = u16::from_be_bytes([section[3], section[4]]);
        let body_end = 3 + section_len(section) - 4;
        let mut cursor = 8usize;
        while cursor + 4 <= body_end {
            let program_number = u16::from_be_bytes([section[cursor], section[cursor + 1]]);
            let pid = (((section[cursor + 2] & 0x1f) as u16) << 8) | section[cursor + 3] as u16;
            if program_number != 0 {
                self.pat_programs.insert((tsid, program_number), pid);
                if let Some(onid) = self.resolved_onid_for_pat_program(tsid, program_number) {
                    let entry = self.service_entry_mut(tsid, onid, program_number);
                    entry.pmt_pid = Some(pid);
                    Self::clear_pmt_state(entry);
                    self.apply_pending_pmt_to_service(tsid, onid, program_number);
                }
            }
            cursor += 4;
        }
    }

    fn parse_cat(&mut self, section: &[u8]) {
        if section.len() < 8 {
            return;
        }
        let body_end = 3 + section_len(section) - 4;
        if body_end <= 8 || body_end > section.len() {
            return;
        }
        let result = parse_ca_descriptors_with_diagnostics(
            &section[8..body_end],
            CaDescriptorParseContext {
                pid: 0x0001,
                table_id: 0x01,
                table_id_extension: parse_section_header(section)
                    .and_then(|h| h.table_id_extension),
                service_id: None,
                elementary_pid: None,
                scope: "CAT",
            },
        );
        self.cat_ca.descriptors.extend(result.descriptors);
        self.malformed_ca_descriptor_diagnostics
            .extend(result.diagnostics);
        dedup_ca_descriptors(&mut self.cat_ca.descriptors);
        dedup_malformed_ca_diagnostics(&mut self.malformed_ca_descriptor_diagnostics);
    }

    fn parse_pmt(&mut self, pid: u16, section: &[u8]) {
        if section.len() < 12 {
            return;
        }
        let service_id = u16::from_be_bytes([section[3], section[4]]);
        let body_end = 3 + section_len(section) - 4;
        let raw_pcr_pid = (((section[8] & 0x1f) as u16) << 8) | section[9] as u16;
        let pcr_pid = (raw_pcr_pid != 0x1fff).then_some(raw_pcr_pid);
        let program_info_length = (((section[10] & 0x0f) as usize) << 8) | section[11] as usize;
        let program_info_start = 12usize;
        let Some(program_info_end) = checked_end(program_info_start, program_info_length, body_end)
        else {
            return;
        };
        let program_ca_result = parse_ca_descriptors_with_diagnostics(
            &section[program_info_start..program_info_end],
            CaDescriptorParseContext {
                pid,
                table_id: 0x02,
                table_id_extension: Some(service_id),
                service_id: Some(service_id),
                elementary_pid: None,
                scope: "PMT_PROGRAM",
            },
        );
        let program_ca_descriptors = program_ca_result.descriptors;
        self.malformed_ca_descriptor_diagnostics
            .extend(program_ca_result.diagnostics);
        let mut service_es_ca_descriptors = Vec::new();
        let mut cursor = program_info_end;
        let mut streams = Vec::new();
        while cursor + 5 <= body_end {
            let stream_type = section[cursor];
            let elementary_pid =
                (((section[cursor + 1] & 0x1f) as u16) << 8) | section[cursor + 2] as u16;
            let es_info_length =
                (((section[cursor + 3] & 0x0f) as usize) << 8) | section[cursor + 4] as usize;
            let desc_start = cursor + 5;
            let Some(desc_end) = checked_end(desc_start, es_info_length, body_end) else {
                break;
            };
            let mut stream = DiscoveredElementaryStream {
                elementary_pid,
                stream_type,
                component_tag: None,
                stream_content: None,
                component_type: None,
                data_component_id: None,
                language_codes: Vec::new(),
                is_caption: false,
                is_superimpose: false,
            };
            let descriptor_bytes = &section[desc_start..desc_end];
            apply_es_descriptors(&mut stream, descriptor_bytes);
            let es_ca_result = parse_ca_descriptors_with_diagnostics(
                descriptor_bytes,
                CaDescriptorParseContext {
                    pid,
                    table_id: 0x02,
                    table_id_extension: Some(service_id),
                    service_id: Some(service_id),
                    elementary_pid: Some(elementary_pid),
                    scope: "PMT_ES",
                },
            );
            self.malformed_ca_descriptor_diagnostics
                .extend(es_ca_result.diagnostics);
            let es_ca = es_ca_result.descriptors;
            if !es_ca.is_empty() {
                service_es_ca_descriptors.push(EsCaMetadata {
                    elementary_pid,
                    descriptors: es_ca,
                });
            }
            streams.push(stream);
            cursor = cursor.saturating_add(5 + es_info_length);
        }
        dedup_malformed_ca_diagnostics(&mut self.malformed_ca_descriptor_diagnostics);
        let pending = PendingPmtInfo {
            pmt_pid: pid,
            pcr_pid,
            streams: streams.clone(),
            program_ca_descriptors: program_ca_descriptors.clone(),
            es_ca_descriptors: service_es_ca_descriptors.clone(),
        };
        let candidate_tsids: Vec<u16> = self
            .pat_programs
            .iter()
            .filter_map(|((tsid, pat_service_id), pmt_pid)| {
                (*pat_service_id == service_id && *pmt_pid == pid).then_some(*tsid)
            })
            .collect();
        for tsid in &candidate_tsids {
            self.unresolved_pmts_by_pat
                .insert((*tsid, service_id, pid), pending.clone());
        }
        for tsid in candidate_tsids {
            let Some(onid) = self.resolved_onid_for_pat_program(tsid, service_id) else {
                continue;
            };
            self.pending_pmts
                .insert((onid, tsid, service_id, pid), pending.clone());
            self.apply_pending_pmt_to_service(tsid, onid, service_id);
        }
    }

    fn parse_nit(&mut self, section: &[u8]) {
        if section.len() < 10 {
            return;
        }
        let body_end = 3 + section_len(section) - 4;
        let descriptors_length = (((section[8] & 0x0f) as usize) << 8) | section[9] as usize;
        let Some(network_desc_end) = checked_end(10, descriptors_length, body_end) else {
            return;
        };
        let network_descriptors = &section[10..network_desc_end];
        let network_name = parse_network_name(network_descriptors);
        let mut cursor = 10usize + descriptors_length;
        if cursor + 2 > body_end {
            return;
        }
        let transport_loop_length =
            (((section[cursor] & 0x0f) as usize) << 8) | section[cursor + 1] as usize;
        cursor += 2;
        let Some(transport_end) = checked_end(cursor, transport_loop_length, body_end) else {
            return;
        };
        while cursor + 6 <= transport_end {
            let tsid = u16::from_be_bytes([section[cursor], section[cursor + 1]]);
            let onid = u16::from_be_bytes([section[cursor + 2], section[cursor + 3]]);
            let desc_len =
                (((section[cursor + 4] & 0x0f) as usize) << 8) | section[cursor + 5] as usize;
            let desc_start = cursor + 6;
            let Some(desc_end) = checked_end(desc_start, desc_len, transport_end) else {
                break;
            };
            self.transport_entry_mut(tsid, onid);
            self.transport_entry_mut(tsid, onid).system_management =
                parse_system_management_descriptor(network_descriptors);
            let (desc_network_name, ts_name, remote_control_key_id) =
                parse_nit_transport_metadata(&section[desc_start..desc_end])
                    .unwrap_or((None, None, None));
            let transport = self.transport_entry_mut(tsid, onid);
            if transport.network_name.is_none() {
                transport.network_name = desc_network_name
                    .as_ref()
                    .map(|decoded| decoded.value.clone())
                    .or_else(|| network_name.as_ref().map(|decoded| decoded.value.clone()));
            }
            if transport.ts_name.is_none() {
                transport.ts_name = ts_name.as_ref().map(|decoded| decoded.value.clone());
            }
            if transport.remote_control_key_id.is_none() {
                transport.remote_control_key_id = remote_control_key_id;
            }
            retain_text_decode_diagnostic(
                &mut transport.text_decode_diagnostics,
                network_name
                    .as_ref()
                    .and_then(|decoded| decoded.diagnostic.clone()),
            );
            retain_text_decode_diagnostic(
                &mut transport.text_decode_diagnostics,
                desc_network_name.and_then(|decoded| decoded.diagnostic),
            );
            retain_text_decode_diagnostic(
                &mut transport.text_decode_diagnostics,
                ts_name.and_then(|decoded| decoded.diagnostic),
            );
            self.parse_service_list_descriptor(tsid, onid, &section[desc_start..desc_end]);
            if let Some(entry) = self.transports.get_mut(&(tsid, onid)) {
                entry.services.extend(
                    self.services
                        .keys()
                        .filter(|(t, o, _)| *t == tsid && *o == onid)
                        .map(|(_, _, sid)| *sid),
                );
            }
            self.propagate_transport_metadata_to_services(tsid, onid);
            cursor = desc_end;
        }
    }

    fn parse_bat(&mut self, section: &[u8]) {
        if section.len() < 10 {
            return;
        }
        let body_end = 3 + section_len(section) - 4;
        let bouquet_desc_len = (((section[8] & 0x0f) as usize) << 8) | section[9] as usize;
        let Some(bouquet_desc_end) = checked_end(10, bouquet_desc_len, body_end) else {
            return;
        };
        let bouquet_name = parse_bouquet_name(&section[10..bouquet_desc_end]);
        let mut cursor = 10usize + bouquet_desc_len;
        if cursor + 2 > body_end {
            return;
        }
        let transport_loop_length =
            (((section[cursor] & 0x0f) as usize) << 8) | section[cursor + 1] as usize;
        cursor += 2;
        let Some(transport_end) = checked_end(cursor, transport_loop_length, body_end) else {
            return;
        };
        while cursor + 6 <= transport_end {
            let tsid = u16::from_be_bytes([section[cursor], section[cursor + 1]]);
            let onid = u16::from_be_bytes([section[cursor + 2], section[cursor + 3]]);
            let desc_len =
                (((section[cursor + 4] & 0x0f) as usize) << 8) | section[cursor + 5] as usize;
            let desc_start = cursor + 6;
            let Some(desc_end) = checked_end(desc_start, desc_len, transport_end) else {
                break;
            };
            let services = parse_service_ids_from_descriptors(&section[desc_start..desc_end]);
            for service_id in services {
                {
                    let entry = self.service_entry_mut(tsid, onid, service_id);
                    if entry.bouquet_name.is_none() {
                        entry.bouquet_name =
                            bouquet_name.as_ref().map(|decoded| decoded.value.clone());
                    }
                    retain_text_decode_diagnostic(
                        &mut entry.text_decode_diagnostics,
                        bouquet_name
                            .as_ref()
                            .and_then(|decoded| decoded.diagnostic.clone()),
                    );
                }
                self.apply_pending_pmt_to_service(tsid, onid, service_id);
            }
            cursor = desc_end;
        }
    }

    fn parse_service_list_descriptor(&mut self, tsid: u16, onid: u16, descriptors: &[u8]) {
        for (service_id, service_type) in parse_service_list(descriptors) {
            {
                let entry = self.service_entry_mut(tsid, onid, service_id);
                entry.service_type = Some(service_type);
            }
            self.transport_entry_mut(tsid, onid)
                .services
                .insert(service_id);
            self.apply_pending_pmt_to_service(tsid, onid, service_id);
        }
    }

    fn parse_sdt(&mut self, section: &[u8]) {
        if section.len() < 11 {
            return;
        }
        let tsid = u16::from_be_bytes([section[3], section[4]]);
        let onid = u16::from_be_bytes([section[8], section[9]]);
        let body_end = 3 + section_len(section) - 4;
        let mut cursor = 11usize;
        while cursor + 5 <= body_end {
            let service_id = u16::from_be_bytes([section[cursor], section[cursor + 1]]);
            let running_status = (section[cursor + 3] >> 5) & 0x07;
            let free_ca_mode = ((section[cursor + 3] >> 4) & 0x01) != 0;
            let descriptors_loop_length =
                (((section[cursor + 3] & 0x0f) as usize) << 8) | section[cursor + 4] as usize;
            let desc_start = cursor + 5;
            let Some(desc_end) = checked_end(desc_start, descriptors_loop_length, body_end) else {
                break;
            };
            self.transport_entry_mut(tsid, onid)
                .services
                .insert(service_id);
            {
                let entry = self.service_entry_mut(tsid, onid, service_id);
                entry.running_status = Some(running_status);
                entry.free_ca_mode = Some(free_ca_mode);
                let mut dc = desc_start;
                while dc + 2 <= desc_end {
                    let tag = section[dc];
                    let len = section[dc + 1] as usize;
                    let body_start = dc + 2;
                    let Some(body_end) = checked_end(body_start, len, desc_end) else {
                        break;
                    };
                    if tag == 0x48 && body_end >= body_start + 3 {
                        entry.service_type = Some(section[body_start]);
                        let provider_len = section[body_start + 1] as usize;
                        let provider_start = body_start + 2;
                        let Some(provider_end) =
                            checked_end(provider_start, provider_len, body_end)
                        else {
                            break;
                        };
                        let name_len_idx = provider_end;
                        if name_len_idx < body_end {
                            let name_len = section[name_len_idx] as usize;
                            let name_start = name_len_idx + 1;
                            let Some(name_end) = checked_end(name_start, name_len, body_end) else {
                                break;
                            };
                            let provider_name = decode_si_text_lossy(
                                "serviceProviderName",
                                &section[provider_start..provider_end],
                            );
                            let service_name =
                                decode_si_text_lossy("serviceName", &section[name_start..name_end]);
                            entry.provider_name = Some(provider_name.value);
                            entry.service_name = Some(service_name.value);
                            retain_text_decode_diagnostic(
                                &mut entry.text_decode_diagnostics,
                                provider_name.diagnostic,
                            );
                            retain_text_decode_diagnostic(
                                &mut entry.text_decode_diagnostics,
                                service_name.diagnostic,
                            );
                        }
                    }
                    dc = body_end;
                }
            }
            self.apply_pending_pmt_to_service(tsid, onid, service_id);
            cursor = desc_end;
        }
    }
}

impl ServiceDiscoveryCollector {
    pub fn set_discovery_profile(&mut self, profile: DiscoveryProfile) {
        self.discovery_profile = profile;
    }

    /// サービスが r51 視聴可能になる前に PMTセクションフィルター を開く必要がある。
    /// サービス の公開可否や視聴可否に依存せず、PAT 由来の PMT PID を返す。
    pub fn pmt_pids_for_section_filters(&self) -> Vec<u16> {
        let mut pids: Vec<u16> = self.engine.pat_programs.values().copied().collect();
        pids.extend(
            self.engine
                .snapshot()
                .pmt_pids_by_service
                .iter()
                .map(|mapping| mapping.pmt_pid),
        );
        pids.sort_unstable();
        pids.dedup();
        pids
    }
}

impl ServiceDiscoveryCollector {
    pub fn push_section(&mut self, pid: u16, section: &[u8]) {
        if !valid_current_section(section) {
            return;
        }
        if self.section_version_changed(pid, section) {
            self.invalidate_changed_table(pid, section);
        }
        self.track_section(pid, section);
        self.track_transport_scopes(section);
        self.engine.push_section(pid, section);
    }

    pub fn events(&self) -> Vec<EitEvent> {
        self.engine.events()
    }

    pub fn take_epg_update_windows(&mut self) -> Vec<EitUpdateWindow> {
        self.engine.take_epg_update_windows()
    }

    pub fn is_known_pmt_pid(&self, pid: u16) -> bool {
        self.engine.is_known_pmt_pid(pid)
    }

    pub fn state(&self) -> DiscoveryCollectionState {
        let snapshot = self.engine.snapshot();
        let pat_complete = self.table_complete(0x0000, 0x00);
        let sdt_complete = if snapshot.transports.is_empty() {
            self.table_complete(0x0011, 0x42)
        } else {
            snapshot.transports.iter().all(|transport| {
                self.sdt_actual_complete_for_transport(
                    transport.original_network_id,
                    transport.transport_stream_id,
                )
            })
        };
        let nit_complete = if snapshot.transports.is_empty() {
            self.table_complete(0x0010, 0x40)
        } else {
            snapshot.transports.iter().all(|transport| {
                self.nit_complete_for_transport(
                    0x40,
                    transport.original_network_id,
                    transport.transport_stream_id,
                )
            })
        };

        let req = optional_table_requirement(self.discovery_profile);
        let bat_complete = self.table_complete(0x0011, 0x4a);
        let sdt_other_complete = self.table_complete(0x0011, 0x46);
        let nit_other_complete = self.table_complete(0x0010, 0x41);

        let mut table_requirements = vec![TableRequirementStatus {
            component: "PAT",
            original_network_id: None,
            transport_stream_id: None,
            service_id: None,
            required: true,
            complete: pat_complete,
        }];
        if snapshot.transports.is_empty() {
            table_requirements.push(TableRequirementStatus {
                component: "SDT",
                original_network_id: None,
                transport_stream_id: None,
                service_id: None,
                required: true,
                complete: sdt_complete,
            });
            table_requirements.push(TableRequirementStatus {
                component: "NIT",
                original_network_id: None,
                transport_stream_id: None,
                service_id: None,
                required: true,
                complete: nit_complete,
            });
            table_requirements.push(TableRequirementStatus {
                component: "SDT-other",
                original_network_id: None,
                transport_stream_id: None,
                service_id: None,
                required: req.require_sdt_other,
                complete: sdt_other_complete,
            });
            table_requirements.push(TableRequirementStatus {
                component: "NIT-other",
                original_network_id: None,
                transport_stream_id: None,
                service_id: None,
                required: req.require_nit_other,
                complete: nit_other_complete,
            });
        } else {
            for transport in &snapshot.transports {
                let onid = transport.original_network_id;
                let tsid = transport.transport_stream_id;
                table_requirements.push(TableRequirementStatus {
                    component: "SDT",
                    original_network_id: Some(onid),
                    transport_stream_id: Some(tsid),
                    service_id: None,
                    required: true,
                    complete: self.sdt_actual_complete_for_transport(onid, tsid),
                });
                table_requirements.push(TableRequirementStatus {
                    component: "NIT",
                    original_network_id: Some(onid),
                    transport_stream_id: Some(tsid),
                    service_id: None,
                    required: true,
                    complete: self.nit_complete_for_transport(0x40, onid, tsid),
                });
                table_requirements.push(TableRequirementStatus {
                    component: "SDT-other",
                    original_network_id: Some(onid),
                    transport_stream_id: Some(tsid),
                    service_id: None,
                    required: req.require_sdt_other,
                    complete: self.sdt_other_complete_for_transport(onid, tsid),
                });
                table_requirements.push(TableRequirementStatus {
                    component: "NIT-other",
                    original_network_id: Some(onid),
                    transport_stream_id: Some(tsid),
                    service_id: None,
                    required: req.require_nit_other,
                    complete: self.nit_complete_for_transport(0x41, onid, tsid),
                });
            }
        }
        table_requirements.push(TableRequirementStatus {
            component: "BAT",
            original_network_id: None,
            transport_stream_id: None,
            service_id: None,
            required: false,
            complete: bat_complete,
        });
        if snapshot.services.is_empty() && snapshot.pmt_pids_by_service.is_empty() {
            table_requirements.push(TableRequirementStatus {
                component: "PMT",
                original_network_id: None,
                transport_stream_id: None,
                service_id: None,
                required: true,
                complete: false,
            });
        }
        for service in &snapshot.services {
            table_requirements.push(TableRequirementStatus {
                component: "PMT",
                original_network_id: Some(service.original_network_id),
                transport_stream_id: Some(service.transport_stream_id),
                service_id: Some(service.service_id),
                required: true,
                complete: service.pmt_pid.is_some() && service.pcr_pid.is_some(),
            });
        }
        for mapping in &snapshot.pmt_pids_by_service {
            if !snapshot.services.iter().any(|service| {
                service.original_network_id == mapping.original_network_id
                    && service.transport_stream_id == mapping.transport_stream_id
                    && service.service_id == mapping.service_id
            }) {
                table_requirements.push(TableRequirementStatus {
                    component: "PMT",
                    original_network_id: Some(mapping.original_network_id),
                    transport_stream_id: Some(mapping.transport_stream_id),
                    service_id: Some(mapping.service_id),
                    required: true,
                    complete: false,
                });
            }
        }
        table_requirements.sort_by_key(|status| {
            (
                status.component,
                status.original_network_id,
                status.transport_stream_id,
                status.service_id,
            )
        });

        let mut semantic_facts_by_service = Vec::new();
        for service in &snapshot.services {
            let mut missing_for_service = table_requirements
                .iter()
                .filter(|status| {
                    status.required
                        && !status.complete
                        && status
                            .original_network_id
                            .map(|value| value == service.original_network_id)
                            .unwrap_or(true)
                        && status
                            .transport_stream_id
                            .map(|value| value == service.transport_stream_id)
                            .unwrap_or(true)
                        && status
                            .service_id
                            .map(|value| value == service.service_id)
                            .unwrap_or(true)
                })
                .map(|status| status.component)
                .collect::<Vec<_>>();
            missing_for_service.sort_unstable();
            missing_for_service.dedup();
            let has_program_ca_descriptor = !service.program_ca_descriptors.is_empty();
            let pmt_pid_resolved = service.pmt_pid.is_some();

            let semantic_requires_cas = has_program_ca_descriptor
                || service
                    .es_ca_descriptors
                    .iter()
                    .any(|metadata| !metadata.descriptors.is_empty());
            let mut semantic_diagnostics = Vec::new();
            if service.pmt_parsed && service.free_ca_mode == Some(true) && !semantic_requires_cas {
                semantic_diagnostics.push("FREE_CA_MODE_WITHOUT_CA_DESCRIPTOR");
            }
            if service.service_type.is_none() {
                semantic_diagnostics.push("SERVICE_TYPE_UNRESOLVED");
            }
            if !service.pmt_parsed {
                semantic_diagnostics.push("PMT_NOT_PARSED");
            }
            if service.pcr_pid.is_none() {
                semantic_diagnostics.push("PCR_PID_UNRESOLVED");
            }
            if let Some(diagnostic) = service.system_management.diagnostic {
                semantic_diagnostics.push(diagnostic);
            }
            semantic_diagnostics.sort_unstable();
            semantic_diagnostics.dedup();
            semantic_facts_by_service.push(ServiceSemanticFacts {
                original_network_id: service.original_network_id,
                transport_stream_id: service.transport_stream_id,
                service_id: service.service_id,
                service_type: service.service_type,
                pmt_pid_resolved,
                pmt_parsed: service.pmt_parsed,
                pcr_pid_resolved: service.pcr_pid.is_some(),
                elementary_streams: service.streams.clone(),
                requires_cas: semantic_requires_cas,
                ca_descriptors_resolved: service.pmt_parsed,
                free_ca_mode: service.free_ca_mode,
                system_management: service.system_management.clone(),
                missing_components: missing_for_service.clone(),
                semantic_diagnostics,
            });
        }

        DiscoveryCollectionState {
            snapshot,
            table_requirements,
            semantic_facts_by_service,
        }
    }

    pub fn sdt_actual_transport_keys(&self) -> Vec<(u16, u16)> {
        self.sdt_actual_transport_scopes.iter().cloned().collect()
    }

    fn invalidate_changed_table(&mut self, pid: u16, section: &[u8]) {
        let Some(header) = parse_section_header(section) else {
            return;
        };
        let Some(table_extension) = header.table_id_extension else {
            return;
        };
        let scope_extension = tracker_scope_extension(section).unwrap_or(TRACKER_GLOBAL_SCOPE);
        self.section_trackers
            .remove(&(pid, header.table_id, table_extension, scope_extension));
        match (pid, header.table_id) {
            (0x0010, 0x40 | 0x41) => {
                if let Some(scopes) = self
                    .nit_transport_scopes
                    .get(&(header.table_id, table_extension))
                    .cloned()
                {
                    self.engine.invalidate_nit_transport_metadata(&scopes);
                }
                self.nit_transport_scopes
                    .remove(&(header.table_id, table_extension));
            }
            (0x0011, 0x42 | 0x46) => {
                if let Some((tsid, onid)) = sdt_transport_scope_from_section(section) {
                    self.engine.invalidate_sdt_service_metadata(tsid, onid);
                    if header.table_id == 0x42 {
                        self.sdt_actual_transport_scopes.remove(&(tsid, onid));
                    } else {
                        self.sdt_other_transport_scopes.remove(&(tsid, onid));
                    }
                }
            }
            (0x0011, 0x4a) => {
                if let Some(scopes) = self.bat_transport_scopes.get(&table_extension).cloned() {
                    self.engine.invalidate_bat_transport_membership(&scopes);
                }
                self.bat_transport_scopes.remove(&table_extension);
            }
            _ => {}
        }
        self.engine
            .invalidate_table(pid, header.table_id, table_extension);
    }

    fn section_version_changed(&self, pid: u16, section: &[u8]) -> bool {
        let Some(header) = parse_section_header(section) else {
            return false;
        };
        if header.current_next_indicator != Some(true) {
            return false;
        }
        let Some(table_extension) = header.table_id_extension else {
            return false;
        };
        let Some(version) = header.version else {
            return false;
        };
        let scope_extension = tracker_scope_extension(section).unwrap_or(TRACKER_GLOBAL_SCOPE);
        self.section_trackers
            .get(&(pid, header.table_id, table_extension, scope_extension))
            .and_then(|tracker| tracker.version)
            .is_some_and(|old_version| old_version != version)
    }

    fn track_section(&mut self, pid: u16, section: &[u8]) {
        let Some(header) = parse_section_header(section) else {
            return;
        };
        if header.current_next_indicator != Some(true) {
            return;
        }
        let Some(table_extension) = header.table_id_extension else {
            return;
        };
        let Some(version) = header.version else {
            return;
        };
        let Some(section_number) = header.section_number else {
            return;
        };
        let Some(last_section_number) = header.last_section_number else {
            return;
        };
        let scope_extension = tracker_scope_extension(section).unwrap_or(TRACKER_GLOBAL_SCOPE);
        self.section_trackers
            .entry((pid, header.table_id, table_extension, scope_extension))
            .or_default()
            .mark_seen(version, section_number, last_section_number);
    }

    fn track_transport_scopes(&mut self, section: &[u8]) {
        let Some(header) = parse_section_header(section) else {
            return;
        };
        if header.current_next_indicator != Some(true) {
            return;
        }
        let Some(table_extension) = header.table_id_extension else {
            return;
        };
        match header.table_id {
            0x40 | 0x41 => {
                if let Some(scopes) = nit_transport_scopes_from_section(section) {
                    self.nit_transport_scopes
                        .entry((header.table_id, table_extension))
                        .or_default()
                        .extend(scopes);
                }
            }
            0x4a => {
                if let Some(scopes) = bat_transport_scopes_from_section(section) {
                    self.bat_transport_scopes
                        .entry(table_extension)
                        .or_default()
                        .extend(scopes);
                }
            }
            0x42 | 0x46 => {
                if let Some(scope) = sdt_transport_scope_from_section(section) {
                    if header.table_id == 0x42 {
                        self.sdt_actual_transport_scopes.insert(scope);
                    } else {
                        self.sdt_other_transport_scopes.insert(scope);
                    }
                }
            }
            _ => {}
        }
    }

    fn table_complete_for_extension(&self, pid: u16, table_id: u8, table_extension: u16) -> bool {
        self.table_complete_for_extension_scope(
            pid,
            table_id,
            table_extension,
            TRACKER_GLOBAL_SCOPE,
        )
    }

    fn table_complete_for_extension_scope(
        &self,
        pid: u16,
        table_id: u8,
        table_extension: u16,
        scope_extension: u16,
    ) -> bool {
        self.section_trackers
            .get(&(pid, table_id, table_extension, scope_extension))
            .is_some_and(|tracker| tracker.is_complete())
    }

    fn sdt_actual_complete_for_transport(
        &self,
        original_network_id: u16,
        transport_stream_id: u16,
    ) -> bool {
        self.table_complete_for_extension_scope(
            0x0011,
            0x42,
            transport_stream_id,
            original_network_id,
        ) && self
            .sdt_actual_transport_scopes
            .contains(&(transport_stream_id, original_network_id))
    }

    fn sdt_other_complete_for_transport(
        &self,
        original_network_id: u16,
        transport_stream_id: u16,
    ) -> bool {
        self.table_complete_for_extension_scope(
            0x0011,
            0x46,
            transport_stream_id,
            original_network_id,
        ) && self
            .sdt_other_transport_scopes
            .contains(&(transport_stream_id, original_network_id))
    }

    fn nit_complete_for_transport(
        &self,
        table_id: u8,
        original_network_id: u16,
        transport_stream_id: u16,
    ) -> bool {
        self.nit_transport_scopes
            .iter()
            .any(|((tracked_table_id, table_extension), transports)| {
                *tracked_table_id == table_id
                    && self.table_complete_for_extension(0x0010, table_id, *table_extension)
                    && transports.contains(&(transport_stream_id, original_network_id))
            })
    }

    fn bat_complete_for_transport(
        &self,
        original_network_id: u16,
        transport_stream_id: u16,
    ) -> bool {
        self.bat_transport_scopes
            .iter()
            .any(|(bouquet_id, transports)| {
                self.table_complete_for_extension(0x0011, 0x4a, *bouquet_id)
                    && transports.contains(&(transport_stream_id, original_network_id))
            })
    }

    fn table_complete(&self, pid: u16, table_id: u8) -> bool {
        self.section_trackers
            .iter()
            .any(|((tracked_pid, tracked_table_id, _, _), tracker)| {
                *tracked_pid == pid && *tracked_table_id == table_id && tracker.is_complete()
            })
    }
}

fn checked_end(start: usize, len: usize, limit: usize) -> Option<usize> {
    start.checked_add(len).filter(|end| *end <= limit)
}

fn section_len(section: &[u8]) -> usize {
    parse_section_header(section)
        .map(|header| header.section_length)
        .unwrap_or_default()
}

fn valid_current_section(section: &[u8]) -> bool {
    let Some(header) = parse_section_header(section) else {
        return false;
    };
    if header.current_next_indicator == Some(false) {
        return false;
    }
    section_crc_valid(section)
}

const TRACKER_GLOBAL_SCOPE: u16 = 0xffff;

fn tracker_scope_extension(section: &[u8]) -> Option<u16> {
    let header = parse_section_header(section)?;
    match header.table_id {
        0x42 | 0x46 => sdt_transport_scope_from_section(section).map(|(_, onid)| onid),
        _ => Some(TRACKER_GLOBAL_SCOPE),
    }
}

fn sdt_transport_scope_from_section(section: &[u8]) -> Option<(u16, u16)> {
    let header = parse_section_header(section)?;
    let tsid = header.table_id_extension?;
    if section.len() < 11 {
        return None;
    }
    let onid = u16::from_be_bytes([section[8], section[9]]);
    Some((tsid, onid))
}

fn nit_transport_scopes_from_section(section: &[u8]) -> Option<BTreeSet<(u16, u16)>> {
    if section.len() < 10 {
        return None;
    }
    let body_end = 3 + section_len(section) - 4;
    let descriptors_length = (((section[8] & 0x0f) as usize) << 8) | section[9] as usize;
    let mut cursor = checked_end(10, descriptors_length, body_end)?;
    if cursor + 2 > body_end {
        return Some(BTreeSet::new());
    }
    let transport_loop_length =
        (((section[cursor] & 0x0f) as usize) << 8) | section[cursor + 1] as usize;
    cursor += 2;
    let transport_end = checked_end(cursor, transport_loop_length, body_end)?;
    let mut scopes = BTreeSet::new();
    while cursor + 6 <= transport_end {
        let tsid = u16::from_be_bytes([section[cursor], section[cursor + 1]]);
        let onid = u16::from_be_bytes([section[cursor + 2], section[cursor + 3]]);
        let desc_len =
            (((section[cursor + 4] & 0x0f) as usize) << 8) | section[cursor + 5] as usize;
        let desc_start = cursor + 6;
        let desc_end = checked_end(desc_start, desc_len, transport_end)?;
        scopes.insert((tsid, onid));
        cursor = desc_end;
    }
    Some(scopes)
}

fn bat_transport_scopes_from_section(section: &[u8]) -> Option<BTreeSet<(u16, u16)>> {
    if section.len() < 10 {
        return None;
    }
    let body_end = 3 + section_len(section) - 4;
    let bouquet_desc_len = (((section[8] & 0x0f) as usize) << 8) | section[9] as usize;
    let mut cursor = checked_end(10, bouquet_desc_len, body_end)?;
    if cursor + 2 > body_end {
        return Some(BTreeSet::new());
    }
    let transport_loop_length =
        (((section[cursor] & 0x0f) as usize) << 8) | section[cursor + 1] as usize;
    cursor += 2;
    let transport_end = checked_end(cursor, transport_loop_length, body_end)?;
    let mut scopes = BTreeSet::new();
    while cursor + 6 <= transport_end {
        let tsid = u16::from_be_bytes([section[cursor], section[cursor + 1]]);
        let onid = u16::from_be_bytes([section[cursor + 2], section[cursor + 3]]);
        let desc_len =
            (((section[cursor + 4] & 0x0f) as usize) << 8) | section[cursor + 5] as usize;
        let desc_start = cursor + 6;
        let desc_end = checked_end(desc_start, desc_len, transport_end)?;
        scopes.insert((tsid, onid));
        cursor = desc_end;
    }
    Some(scopes)
}

fn parse_network_name(descriptors: &[u8]) -> Option<DecodedSiText> {
    let mut cursor = 0usize;
    while cursor + 2 <= descriptors.len() {
        let tag = descriptors[cursor];
        let len = descriptors[cursor + 1] as usize;
        let body_start = cursor + 2;
        let Some(body_end) = checked_end(body_start, len, descriptors.len()) else {
            break;
        };
        if tag == 0x40 {
            return Some(decode_si_text_lossy(
                "networkName",
                &descriptors[body_start..body_end],
            ));
        }
        cursor = body_end;
    }
    None
}

fn parse_system_management_descriptor(descriptors: &[u8]) -> SystemManagementFacts {
    let mut cursor = 0usize;
    let mut parsed: Option<SystemManagementFacts> = None;
    while cursor + 2 <= descriptors.len() {
        let tag = descriptors[cursor];
        let len = descriptors[cursor + 1] as usize;
        let body_start = cursor + 2;
        let Some(body_end) = checked_end(body_start, len, descriptors.len()) else {
            return SystemManagementFacts {
                descriptor_present: tag == 0xfe || parsed.is_some(),
                diagnostic: Some("SMD_DESCRIPTOR_LENGTH_INVALID"),
                ..SystemManagementFacts::default()
            };
        };
        if tag == 0xfe {
            if parsed.is_some() {
                return SystemManagementFacts {
                    descriptor_present: true,
                    diagnostic: Some("SMD_DESCRIPTOR_DUPLICATE"),
                    ..SystemManagementFacts::default()
                };
            }
            if len < 2 {
                return SystemManagementFacts {
                    descriptor_present: true,
                    diagnostic: Some("SMD_DESCRIPTOR_TOO_SHORT"),
                    ..SystemManagementFacts::default()
                };
            }
            let system_management_id =
                u16::from_be_bytes([descriptors[body_start], descriptors[body_start + 1]]);
            let broadcasting_flag = ((system_management_id >> 14) & 0x03) as u8;
            let broadcasting_identifier = ((system_management_id >> 8) & 0x3f) as u8;
            let semantic_state = match broadcasting_flag {
                0 if matches!(broadcasting_identifier, 0b000010..=0b000100) => {
                    SmdSemanticState::SupportedBroadcast
                }
                0 => SmdSemanticState::UnsupportedBroadcastSystem,
                1 | 2 => SmdSemanticState::NonBroadcast,
                _ => SmdSemanticState::UndefinedBroadcastClass,
            };
            parsed = Some(SystemManagementFacts {
                descriptor_present: true,
                syntax_valid: true,
                system_management_id: Some(system_management_id),
                broadcasting_flag: Some(broadcasting_flag),
                broadcasting_identifier: Some(broadcasting_identifier),
                additional_broadcasting_identification: Some(system_management_id as u8),
                additional_identification_info: descriptors[body_start + 2..body_end].to_vec(),
                semantic_state,
                diagnostic: None,
            });
        }
        cursor = body_end;
    }
    parsed.unwrap_or(SystemManagementFacts {
        diagnostic: Some("SMD_DESCRIPTOR_MISSING"),
        ..SystemManagementFacts::default()
    })
}

fn parse_nit_transport_metadata(
    descriptors: &[u8],
) -> Option<(Option<DecodedSiText>, Option<DecodedSiText>, Option<u8>)> {
    let mut network_name = None;
    let mut ts_name = None;
    let mut remote_control_key_id = None;
    let mut cursor = 0usize;
    while cursor + 2 <= descriptors.len() {
        let tag = descriptors[cursor];
        let len = descriptors[cursor + 1] as usize;
        let body_start = cursor + 2;
        let Some(body_end) = checked_end(body_start, len, descriptors.len()) else {
            break;
        };
        match tag {
            0x40 => {
                network_name = Some(decode_si_text_lossy(
                    "networkName",
                    &descriptors[body_start..body_end],
                ))
            }
            0xcd => {
                let body_len = body_end.saturating_sub(body_start);
                if body_len >= 2 {
                    remote_control_key_id = Some(descriptors[body_start]);
                    let ts_name_len = ((descriptors[body_start + 1] >> 2) & 0x3f) as usize;
                    let ts_name_start = body_start + 2;
                    let remaining = body_end.saturating_sub(ts_name_start);
                    if ts_name_len <= remaining {
                        let ts_name_end = ts_name_start + ts_name_len;
                        ts_name = Some(decode_si_text_lossy(
                            "transportStreamName",
                            &descriptors[ts_name_start..ts_name_end],
                        ));
                    }
                }
            }
            _ => {}
        }
        cursor = body_end;
    }
    if network_name.is_none() && ts_name.is_none() && remote_control_key_id.is_none() {
        None
    } else {
        Some((network_name, ts_name, remote_control_key_id))
    }
}

fn apply_es_descriptors(stream: &mut DiscoveredElementaryStream, descriptors: &[u8]) {
    let mut cursor = 0usize;
    while cursor + 2 <= descriptors.len() {
        let tag = descriptors[cursor];
        let len = descriptors[cursor + 1] as usize;
        let body_start = cursor + 2;
        let Some(body_end) = checked_end(body_start, len, descriptors.len()) else {
            break;
        };
        let body = &descriptors[body_start..body_end];
        match tag {
            0x52 if !body.is_empty() => stream.component_tag = Some(body[0]),
            0x50 if body.len() >= 5 => {
                stream.stream_content = Some(body[0] & 0x0f);
                stream.component_type = Some(body[1]);
                stream.component_tag = Some(body[2]);
                if body.len() >= 6 {
                    let lang = String::from_utf8_lossy(&body[3..6]).to_string();
                    if !lang.is_empty() {
                        stream.language_codes.push(lang);
                    }
                }
            }
            0xc4 if body.len() >= 9 => {
                stream.stream_content = Some(body[0] & 0x0f);
                stream.component_type = Some(body[1]);
                stream.component_tag = Some(body[2]);
                let lang = String::from_utf8_lossy(&body[6..9]).to_string();
                if !lang.is_empty() {
                    stream.language_codes.push(lang);
                }
                if body.len() >= 12 && (body[5] & 0x80) != 0 {
                    let lang2 = String::from_utf8_lossy(&body[9..12]).to_string();
                    if !lang2.is_empty() {
                        stream.language_codes.push(lang2);
                    }
                }
            }
            0x0a => {
                let mut lc = 0usize;
                while lc + 4 <= body.len() {
                    let lang = String::from_utf8_lossy(&body[lc..lc + 3]).to_string();
                    if !lang.is_empty() {
                        stream.language_codes.push(lang);
                    }
                    lc += 4;
                }
            }
            0xfd if body.len() >= 2 => {
                let data_component_id = u16::from_be_bytes([body[0], body[1]]);
                stream.data_component_id = Some(data_component_id);
                if matches!(data_component_id, 0x0008 | 0x0012) {
                    stream.is_caption = true;
                }
                if data_component_id == 0x0008 && body.get(2).copied() == Some(0x31) {
                    stream.is_superimpose = true;
                }
            }
            _ => {}
        }
        cursor = body_end;
    }
    let mut seen = BTreeSet::new();
    stream
        .language_codes
        .retain(|lang| seen.insert(lang.clone()));
}

fn parse_service_list(descriptors: &[u8]) -> Vec<(u16, u8)> {
    let mut out = Vec::new();
    let mut cursor = 0usize;
    while cursor + 2 <= descriptors.len() {
        let tag = descriptors[cursor];
        let len = descriptors[cursor + 1] as usize;
        let body_start = cursor + 2;
        let Some(body_end) = checked_end(body_start, len, descriptors.len()) else {
            break;
        };
        if tag == 0x41 {
            let mut sc = body_start;
            while sc + 3 <= body_end {
                out.push((
                    u16::from_be_bytes([descriptors[sc], descriptors[sc + 1]]),
                    descriptors[sc + 2],
                ));
                sc += 3;
            }
        }
        cursor = body_end;
    }
    out
}

fn parse_service_ids_from_descriptors(descriptors: &[u8]) -> Vec<u16> {
    parse_service_list(descriptors)
        .into_iter()
        .map(|(sid, _)| sid)
        .collect()
}

fn parse_bouquet_name(descriptors: &[u8]) -> Option<DecodedSiText> {
    let mut cursor = 0usize;
    while cursor + 2 <= descriptors.len() {
        let tag = descriptors[cursor];
        let len = descriptors[cursor + 1] as usize;
        let body_start = cursor + 2;
        let Some(body_end) = checked_end(body_start, len, descriptors.len()) else {
            break;
        };
        if tag == 0x47 {
            return Some(decode_si_text_lossy(
                "bouquetName",
                &descriptors[body_start..body_end],
            ));
        }
        cursor = body_end;
    }
    None
}

fn dedup_ca_descriptors(descriptors: &mut Vec<CaDescriptor>) {
    let mut seen = BTreeSet::new();
    descriptors.retain(|d| seen.insert((d.ca_system_id, d.ca_pid, d.private_data.clone())));
}

fn dedup_malformed_ca_diagnostics(diagnostics: &mut Vec<MalformedCaDescriptorDiagnostic>) {
    let mut seen = BTreeSet::new();
    diagnostics.retain(|d| {
        seen.insert((
            d.pid,
            d.table_id,
            d.table_id_extension,
            d.service_id,
            d.elementary_pid,
            d.scope,
            d.offset,
            d.declared_length,
            d.reason,
        ))
    });
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DecodedSiText {
    value: String,
    diagnostic: Option<String>,
}

fn decode_si_text_lossy(field: &str, bytes: &[u8]) -> DecodedSiText {
    let (value, diagnostic) = decode_arib_string_lossy(bytes);
    DecodedSiText {
        value,
        diagnostic: (diagnostic.replacement_count != 0).then(|| {
            format!(
                "field={} used lossy ARIB SI decoding: {}",
                field,
                diagnostic.summary(),
            )
        }),
    }
}

fn retain_text_decode_diagnostic(diagnostics: &mut Vec<String>, diagnostic: Option<String>) {
    let Some(diagnostic) = diagnostic else {
        return;
    };
    if diagnostics.contains(&diagnostic) {
        return;
    }
    const MAX_TEXT_DECODE_DIAGNOSTICS_PER_OWNER: usize = 32;
    if diagnostics.len() >= MAX_TEXT_DECODE_DIAGNOSTICS_PER_OWNER {
        diagnostics.remove(0);
    }
    diagnostics.push(diagnostic);
}

#[cfg(test)]
mod tests {
    use super::{DiscoveryPublishStage, ServiceDiscoveryCollector, ServiceDiscoveryEngine};
    use crate::sections::crc32_mpeg;

    #[test]
    fn scoped_table_completeness_does_not_mix_transport_scopes() {
        let mut collector = ServiceDiscoveryCollector::default();
        let mut nit_tracker = super::SectionTracker::default();
        nit_tracker.mark_seen(1, 0, 0);
        collector.section_trackers.insert(
            (0x0010, 0x41, 0x1000, super::TRACKER_GLOBAL_SCOPE),
            nit_tracker,
        );
        collector.nit_transport_scopes.insert(
            (0x41, 0x1000),
            std::collections::BTreeSet::from([(0x4010, 0x0004)]),
        );
        assert!(collector.nit_complete_for_transport(0x41, 0x0004, 0x4010));
        assert!(!collector.nit_complete_for_transport(0x41, 0x0004, 0x4020));

        let mut bat_tracker = super::SectionTracker::default();
        bat_tracker.mark_seen(1, 0, 0);
        collector.section_trackers.insert(
            (0x0011, 0x4a, 0x0004, super::TRACKER_GLOBAL_SCOPE),
            bat_tracker,
        );
        collector
            .bat_transport_scopes
            .insert(0x0004, std::collections::BTreeSet::from([(0x4010, 0x0004)]));
        assert!(collector.bat_complete_for_transport(0x0004, 0x4010));
        assert!(!collector.bat_complete_for_transport(0x0004, 0x4020));
    }

    #[test]
    fn sdt_completeness_is_scoped_by_onid_and_tsid() {
        let mut collector = ServiceDiscoveryCollector::default();
        let mut sdt_tracker = super::SectionTracker::default();
        sdt_tracker.mark_seen(1, 0, 0);
        collector
            .section_trackers
            .insert((0x0011, 0x42, 0x4010, 0x0004), sdt_tracker);
        collector
            .sdt_actual_transport_scopes
            .insert((0x4010, 0x0004));
        assert!(collector.sdt_actual_complete_for_transport(0x0004, 0x4010));
        assert!(!collector.sdt_actual_complete_for_transport(0x7fe0, 0x4010));
    }

    fn section_with_crc(mut body: Vec<u8>) -> Vec<u8> {
        let crc = crc32_mpeg(&body);
        body.extend_from_slice(&crc.to_be_bytes());
        body
    }

    #[test]
    fn pmt_arriving_before_sdt_is_applied_later() {
        let pat = section_with_crc(vec![
            0x00, 0xb0, 0x0d, 0x00, 0x11, 0xc1, 0x00, 0x00, 0x00, 0x01, 0xe1, 0x00,
        ]);
        let pmt = section_with_crc(vec![
            0x02, 0xb0, 0x17, 0x00, 0x01, 0xc1, 0x00, 0x00, 0xe1, 0x01, 0xf0, 0x00, 0x1b, 0xe1,
            0x01, 0xf0, 0x00, 0x0f, 0xe1, 0x02, 0xf0, 0x00,
        ]);
        let sdt = section_with_crc(vec![
            0x42, 0xf0, 0x18, 0x00, 0x11, 0xc1, 0x00, 0x00, 0x00, 0x22, 0x00, 0x00, 0x01, 0xfc,
            0xe0, 0x07, 0x48, 0x05, 0x01, 0x00, 0x02, b'T', b'1',
        ]);
        let mut eng = ServiceDiscoveryEngine::default();
        eng.push_section(0x0000, &pat);
        eng.push_section(0x0100, &pmt);
        eng.push_section(0x0011, &sdt);
        let snap = eng.snapshot();
        assert_eq!(snap.services.len(), 1);
        assert_eq!(snap.services[0].pcr_pid, Some(0x0101));
        assert_eq!(snap.services[0].streams.len(), 2);
    }

    #[test]
    fn sdt_lossy_service_name_diagnostic_is_retained_with_input_prefix() {
        let sdt = section_with_crc(vec![
            0x42, 0xf0, 0x19, 0x00, 0x11, 0xc1, 0x00, 0x00, 0x00, 0x22, 0x00, 0x00, 0x01, 0xfc,
            0xe0, 0x08, 0x48, 0x06, 0x01, 0x00, 0x03, 0x1b, b'$', b'X',
        ]);
        let mut engine = ServiceDiscoveryEngine::default();
        engine.push_section(0x0011, &sdt);
        let snapshot = engine.snapshot();
        let service = snapshot.services.first().expect("SDT service");
        assert_eq!(service.service_name.as_deref(), Some("�"));
        assert!(service.text_decode_diagnostics.iter().any(|diagnostic| {
            diagnostic.contains("field=serviceName")
                && diagnostic.contains("replacement_count=1")
                && diagnostic.contains("input_prefix_hex:1b2458")
        }));
    }

    #[test]
    fn pmt_with_null_packet_pcr_pid_is_not_claimable() {
        let pat = section_with_crc(vec![
            0x00, 0xb0, 0x0d, 0x00, 0x11, 0xc1, 0x00, 0x00, 0x00, 0x01, 0xe1, 0x00,
        ]);
        let pmt = section_with_crc(vec![
            0x02, 0xb0, 0x17, 0x00, 0x01, 0xc1, 0x00, 0x00, 0xff, 0xff, 0xf0, 0x00, 0x1b, 0xe1,
            0x01, 0xf0, 0x00, 0x0f, 0xe1, 0x02, 0xf0, 0x00,
        ]);
        let sdt = section_with_crc(vec![
            0x42, 0xf0, 0x18, 0x00, 0x11, 0xc1, 0x00, 0x00, 0x00, 0x22, 0x00, 0x00, 0x01, 0xfc,
            0xe0, 0x07, 0x48, 0x05, 0x01, 0x00, 0x02, b'T', b'1',
        ]);
        let mut collector = ServiceDiscoveryCollector::default();
        collector.push_section(0x0000, &pat);
        collector.push_section(0x0100, &pmt);
        collector.push_section(0x0011, &sdt);
        let state = collector.state();
        let service = state
            .snapshot
            .services
            .iter()
            .find(|s| s.service_id == 1)
            .expect("service");
        assert_eq!(service.pcr_pid, None);
        let facts = state
            .semantic_facts_by_service
            .iter()
            .find(|p| p.service_id == 1)
            .expect("semantic facts");
        assert!(!facts.pcr_pid_resolved);
        assert!(facts.semantic_diagnostics.contains(&"PCR_PID_UNRESOLVED"));
    }

    #[test]
    fn pmt_is_not_applied_to_same_service_id_on_other_transport() {
        let pmt = section_with_crc(vec![
            0x02, 0xb0, 0x17, 0x00, 0x01, 0xc1, 0x00, 0x00, 0xe1, 0x01, 0xf0, 0x00, 0x1b, 0xe1,
            0x01, 0xf0, 0x00, 0x0f, 0xe1, 0x02, 0xf0, 0x00,
        ]);
        let mut eng = ServiceDiscoveryEngine::default();
        eng.pat_programs.insert((0x0011, 0x0001), 0x0100);
        eng.service_entry_mut(0x0011, 0x0022, 0x0001);
        eng.service_entry_mut(0x0033, 0x0044, 0x0001);
        eng.push_section(0x0100, &pmt);
        let snap = eng.snapshot();
        let target = snap
            .services
            .iter()
            .find(|s| s.transport_stream_id == 0x0011 && s.original_network_id == 0x0022)
            .unwrap();
        let other = snap
            .services
            .iter()
            .find(|s| s.transport_stream_id == 0x0033 && s.original_network_id == 0x0044)
            .unwrap();
        assert_eq!(target.pmt_pid, Some(0x0100));
        assert_eq!(target.pcr_pid, Some(0x0101));
        assert_eq!(other.pmt_pid, None);
        assert_eq!(other.pcr_pid, None);
    }

    #[test]
    fn pmt_is_not_applied_when_same_tsid_service_id_has_multiple_onids() {
        let pmt = section_with_crc(vec![
            0x02, 0xb0, 0x17, 0x00, 0x01, 0xc1, 0x00, 0x00, 0xe1, 0x01, 0xf0, 0x00, 0x1b, 0xe1,
            0x01, 0xf0, 0x00, 0x0f, 0xe1, 0x02, 0xf0, 0x00,
        ]);
        let mut eng = ServiceDiscoveryEngine::default();
        eng.pat_programs.insert((0x0011, 0x0001), 0x0100);
        eng.service_entry_mut(0x0011, 0x0022, 0x0001);
        eng.service_entry_mut(0x0011, 0x0033, 0x0001);
        eng.push_section(0x0100, &pmt);
        let snap = eng.snapshot();
        for service in snap
            .services
            .iter()
            .filter(|s| s.transport_stream_id == 0x0011 && s.service_id == 0x0001)
        {
            assert_eq!(service.pcr_pid, None);
            assert!(service.streams.is_empty());
        }
    }

    #[test]
    fn satellite_missing_components_are_scoped_to_satellite_transport() {
        let mut collector = ServiceDiscoveryCollector::default();
        collector.set_discovery_profile(DiscoveryProfile::Bs);
        collector.engine.transport_entry_mut(0x4010, 0x0004);
        collector.engine.transport_entry_mut(0x7fe0, 0x7fe0);
        let state = collector.state();
        assert!(state
            .missing_components_by_scope()
            .iter()
            .any(|m| m.component == "SDT-other"
                && m.original_network_id == Some(0x0004)
                && m.transport_stream_id == Some(0x4010)));
        assert!(state
            .missing_components_by_scope()
            .iter()
            .all(|m| m.component != "BAT"));
    }

    #[test]
    fn explicit_profile_controls_only_optional_satellite_tables() {
        let mut collector = ServiceDiscoveryCollector::default();
        collector.engine.transport_entry_mut(0x4010, 0x0004);

        let terrestrial = collector.state();
        assert!(terrestrial
            .table_requirements
            .iter()
            .filter(|status| matches!(status.component, "SDT-other" | "NIT-other" | "BAT"))
            .all(|status| !status.required));

        collector.set_discovery_profile(DiscoveryProfile::Bs);
        let bs = collector.state();
        assert!(bs
            .table_requirements
            .iter()
            .any(|status| status.component == "SDT-other" && status.required));
        assert!(bs
            .table_requirements
            .iter()
            .any(|status| status.component == "NIT-other" && !status.required));
        assert!(bs
            .table_requirements
            .iter()
            .any(|status| status.component == "BAT" && !status.required));

        collector.set_discovery_profile(DiscoveryProfile::Cs110);
        let cs110 = collector.state();
        assert!(cs110
            .table_requirements
            .iter()
            .filter(|status| matches!(status.component, "SDT-other" | "NIT-other"))
            .all(|status| status.required));
        assert!(cs110
            .table_requirements
            .iter()
            .any(|status| status.component == "BAT" && !status.required));
    }

    #[test]
    fn collector_exposes_partial_snapshot_before_full_nit() {
        let pat = section_with_crc(vec![
            0x00, 0xb0, 0x0d, 0x00, 0x11, 0xc1, 0x00, 0x00, 0x00, 0x01, 0xe1, 0x00,
        ]);
        let pmt = section_with_crc(vec![
            0x02, 0xb0, 0x17, 0x00, 0x01, 0xc1, 0x00, 0x00, 0xe1, 0x01, 0xf0, 0x00, 0x1b, 0xe1,
            0x01, 0xf0, 0x00, 0x0f, 0xe1, 0x02, 0xf0, 0x00,
        ]);
        let sdt = section_with_crc(vec![
            0x42, 0xf0, 0x18, 0x00, 0x11, 0xc1, 0x00, 0x00, 0x00, 0x22, 0x00, 0x00, 0x01, 0xfc,
            0xe0, 0x07, 0x48, 0x05, 0x01, 0x00, 0x02, b'T', b'1',
        ]);
        let mut collector = ServiceDiscoveryCollector::default();
        collector.push_section(0x0000, &pat);
        collector.push_section(0x0100, &pmt);
        collector.push_section(0x0011, &sdt);
        let state = collector.state();
        assert_eq!(state.publish_stage(), DiscoveryPublishStage::Partial);
        assert!(state.complete_snapshot().is_none());
        assert!(state.partial_snapshot().is_some());
    }

    #[test]
    fn collector_complete_snapshot_supersedes_partial_snapshot() {
        let pat = section_with_crc(vec![
            0x00, 0xb0, 0x0d, 0x00, 0x11, 0xc1, 0x00, 0x00, 0x00, 0x01, 0xe1, 0x00,
        ]);
        let nit = section_with_crc(vec![
            0x40, 0xf0, 0x18, 0x00, 0x01, 0xc1, 0x00, 0x00, 0xf0, 0x00, 0xf0, 0x0a, 0x00, 0x11,
            0x00, 0x22, 0xf0, 0x04, 0x41, 0x03, 0x00, 0x01, 0x01,
        ]);
        let sdt = section_with_crc(vec![
            0x42, 0xf0, 0x18, 0x00, 0x11, 0xc1, 0x00, 0x00, 0x00, 0x22, 0x00, 0x00, 0x01, 0xfc,
            0xe0, 0x07, 0x48, 0x05, 0x01, 0x00, 0x02, b'T', b'1',
        ]);
        let pmt = section_with_crc(vec![
            0x02, 0xb0, 0x17, 0x00, 0x01, 0xc1, 0x00, 0x00, 0xe1, 0x01, 0xf0, 0x00, 0x1b, 0xe1,
            0x01, 0xf0, 0x00, 0x0f, 0xe1, 0x02, 0xf0, 0x00,
        ]);
        let mut collector = ServiceDiscoveryCollector::default();
        collector.push_section(0x0000, &pat);
        collector.push_section(0x0100, &pmt);
        collector.push_section(0x0011, &sdt);
        assert_eq!(
            collector.state().publish_stage(),
            DiscoveryPublishStage::Partial
        );
        collector.push_section(0x0010, &nit);
        assert_eq!(
            collector.state().publish_stage(),
            DiscoveryPublishStage::Complete
        );
        assert!(collector.state().complete_snapshot().is_some());
    }

    #[test]
    fn collector_reports_raw_partial_snapshot_until_complete() {
        let pat = section_with_crc(vec![
            0x00, 0xb0, 0x0d, 0x00, 0x11, 0xc1, 0x00, 0x00, 0x00, 0x01, 0xe1, 0x00,
        ]);
        let sdt = section_with_crc(vec![
            0x42, 0xf0, 0x18, 0x00, 0x11, 0xc1, 0x00, 0x00, 0x00, 0x22, 0x00, 0x00, 0x01, 0xfc,
            0xe0, 0x07, 0x48, 0x05, 0x01, 0x00, 0x02, b'T', b'1',
        ]);
        let mut collector = ServiceDiscoveryCollector::default();
        collector.push_section(0x0000, &pat);
        collector.push_section(0x0011, &sdt);
        let state = collector.state();
        assert!(!state.is_complete());
        assert_eq!(state.publish_stage(), DiscoveryPublishStage::Partial);
        assert_eq!(state.snapshot.services.len(), 1);
        assert_eq!(state.semantic_facts_by_service.len(), 1);
        assert_eq!(state.missing_components(), vec!["NIT", "PMT"]);
    }

    #[test]
    fn pmt_mapping_uses_full_service_identity() {
        let pat = section_with_crc(vec![
            0x00, 0xb0, 0x0d, 0x00, 0x11, 0xc1, 0x00, 0x00, 0x00, 0x01, 0xe1, 0x00,
        ]);
        let nit = section_with_crc(vec![
            0x40, 0xf0, 0x18, 0x00, 0x01, 0xc1, 0x00, 0x00, 0xf0, 0x00, 0xf0, 0x0a, 0x00, 0x11,
            0x00, 0x22, 0xf0, 0x04, 0x41, 0x03, 0x00, 0x01, 0x01,
        ]);
        let sdt = section_with_crc(vec![
            0x42, 0xf0, 0x18, 0x00, 0x11, 0xc1, 0x00, 0x00, 0x00, 0x22, 0x00, 0x00, 0x01, 0xfc,
            0xe0, 0x07, 0x48, 0x05, 0x01, 0x00, 0x02, b'T', b'1',
        ]);
        let pmt = section_with_crc(vec![
            0x02, 0xb0, 0x17, 0x00, 0x01, 0xc1, 0x00, 0x00, 0xe1, 0x01, 0xf0, 0x00, 0x1b, 0xe1,
            0x01, 0xf0, 0x00, 0x0f, 0xe1, 0x02, 0xf0, 0x00,
        ]);
        let mut collector = ServiceDiscoveryCollector::default();
        collector.push_section(0x0000, &pat);
        collector.push_section(0x0010, &nit);
        collector.push_section(0x0011, &sdt);
        collector.push_section(0x0100, &pmt);
        let mappings = &collector.state().snapshot.pmt_pids_by_service;
        assert_eq!(mappings.len(), 1);
        assert_eq!(mappings[0].original_network_id, 0x0022);
        assert_eq!(mappings[0].transport_stream_id, 0x0011);
        assert_eq!(mappings[0].service_id, 0x0001);
        assert_eq!(mappings[0].pmt_pid, 0x0100);
    }

    #[test]
    fn collector_reports_complete_only_after_pat_nit_sdt_and_required_pmts() {
        let pat = section_with_crc(vec![
            0x00, 0xb0, 0x0d, 0x00, 0x11, 0xc1, 0x00, 0x00, 0x00, 0x01, 0xe1, 0x00,
        ]);
        let nit = section_with_crc(vec![
            0x40, 0xf0, 0x18, 0x00, 0x01, 0xc1, 0x00, 0x00, 0xf0, 0x00, 0xf0, 0x0a, 0x00, 0x11,
            0x00, 0x22, 0xf0, 0x04, 0x41, 0x03, 0x00, 0x01, 0x01,
        ]);
        let sdt = section_with_crc(vec![
            0x42, 0xf0, 0x18, 0x00, 0x11, 0xc1, 0x00, 0x00, 0x00, 0x22, 0x00, 0x00, 0x01, 0xfc,
            0xe0, 0x07, 0x48, 0x05, 0x01, 0x00, 0x02, b'T', b'1',
        ]);
        let pmt = section_with_crc(vec![
            0x02, 0xb0, 0x17, 0x00, 0x01, 0xc1, 0x00, 0x00, 0xe1, 0x01, 0xf0, 0x00, 0x1b, 0xe1,
            0x01, 0xf0, 0x00, 0x0f, 0xe1, 0x02, 0xf0, 0x00,
        ]);
        let mut collector = ServiceDiscoveryCollector::default();
        collector.push_section(0x0000, &pat);
        let state = collector.state();
        assert!(state.component_complete("PAT"));
        assert!(!state.is_complete());

        collector.push_section(0x0010, &nit);
        collector.push_section(0x0011, &sdt);
        let state = collector.state();
        assert!(state.component_complete("NIT"));
        assert!(state.component_complete("SDT"));
        assert!(!state.component_complete("PMT"));
        assert!(!state.is_complete());

        collector.push_section(0x0100, &pmt);
        let state = collector.state();
        assert!(state.component_complete("PMT"));
        assert!(state.is_complete());
    }

    #[test]
    fn pat_pmt_mapping_is_not_published_when_onid_is_ambiguous() {
        let mut engine = ServiceDiscoveryEngine::default();
        engine.pat_programs.insert((0x0011, 0x0001), 0x0100);
        engine.service_entry_mut(0x0011, 0x0022, 0x0001);
        engine.service_entry_mut(0x0011, 0x0033, 0x0001);
        engine.apply_pending_pmt_to_service(0x0011, 0x0022, 0x0001);
        engine.apply_pending_pmt_to_service(0x0011, 0x0033, 0x0001);
        let snapshot = engine.snapshot();
        assert!(snapshot.pmt_pids_by_service.is_empty());
        assert!(snapshot
            .services
            .iter()
            .all(|service| service.pmt_pid.is_none()));
    }
}

#[cfg(test)]
mod staged_tests {
    use super::*;

    fn required_status(component: &'static str, complete: bool) -> TableRequirementStatus {
        TableRequirementStatus {
            component,
            original_network_id: None,
            transport_stream_id: None,
            service_id: None,
            required: true,
            complete,
        }
    }

    #[test]
    fn last_discovery_retains_publish_stage() {
        let mut state = DiscoveryCollectionState::default();
        state.snapshot.services.push(DiscoveredService {
            original_network_id: 0x0022,
            transport_stream_id: 0x0011,
            service_id: 1,
            pmt_pid: Some(0x0100),
            ..DiscoveredService::default()
        });
        state.snapshot.pmt_pids_by_service.push(PmtPidMapping {
            original_network_id: 0x0022,
            transport_stream_id: 0x0011,
            service_id: 1,
            pmt_pid: 0x0100,
        });
        state.table_requirements = vec![
            required_status("PAT", true),
            required_status("PMT", true),
            required_status("SDT", false),
            required_status("NIT", false),
        ];
        let env = state.best_available_snapshot().expect("partial env");
        assert_eq!(env.stage, DiscoveryPublishStage::Partial);
    }

    #[test]
    fn complete_snapshot_replaces_partial_snapshot_with_stage_update() {
        let mut state = DiscoveryCollectionState::default();
        state.snapshot.services.push(DiscoveredService {
            original_network_id: 0x0022,
            transport_stream_id: 0x0011,
            service_id: 1,
            pmt_pid: Some(0x0100),
            ..DiscoveredService::default()
        });
        state.snapshot.pmt_pids_by_service.push(PmtPidMapping {
            original_network_id: 0x0022,
            transport_stream_id: 0x0011,
            service_id: 1,
            pmt_pid: 0x0100,
        });
        state.table_requirements = vec![
            required_status("PAT", true),
            required_status("PMT", true),
            required_status("SDT", false),
            required_status("NIT", false),
        ];
        assert_eq!(
            state.best_available_snapshot().unwrap().stage,
            DiscoveryPublishStage::Partial
        );
        state
            .table_requirements
            .iter_mut()
            .filter(|status| matches!(status.component, "SDT" | "NIT"))
            .for_each(|status| status.complete = true);
        assert_eq!(
            state.best_available_snapshot().unwrap().stage,
            DiscoveryPublishStage::Complete
        );
    }
}

#[cfg(test)]
mod semantic_facts_coverage_tests {
    use super::*;
    use crate::ca_descriptor::CaDescriptor;

    fn complete_tracker() -> SectionTracker {
        let mut tracker = SectionTracker::default();
        tracker.mark_seen(1, 0, 0);
        tracker
    }

    fn base_service(streams: Vec<DiscoveredElementaryStream>) -> DiscoveredService {
        DiscoveredService {
            original_network_id: 0x0022,
            transport_stream_id: 0x0011,
            service_id: 1,
            service_type: Some(0x01),
            free_ca_mode: Some(false),
            pmt_pid: Some(0x0100),
            pcr_pid: Some(0x0101),
            streams,
            ..DiscoveredService::default()
        }
    }

    fn stream(pid: u16, stream_type: u8) -> DiscoveredElementaryStream {
        DiscoveredElementaryStream {
            elementary_pid: pid,
            stream_type,
            ..DiscoveredElementaryStream::default()
        }
    }

    fn semantic_facts_for(service: DiscoveredService) -> ServiceSemanticFacts {
        let mut collector = ServiceDiscoveryCollector::default();
        let key = (
            service.transport_stream_id,
            service.original_network_id,
            service.service_id,
        );
        collector.engine.transports.insert(
            (key.0, key.1),
            DiscoveredTransport {
                transport_stream_id: key.0,
                original_network_id: key.1,
                services: BTreeSet::from([key.2]),
                ..DiscoveredTransport::default()
            },
        );
        collector.engine.services.insert(key, service);
        collector.section_trackers.insert(
            (0x0000, 0x00, 0x0011, super::TRACKER_GLOBAL_SCOPE),
            complete_tracker(),
        );
        collector
            .section_trackers
            .insert((0x0011, 0x42, 0x0011, 0x0022), complete_tracker());
        collector
            .sdt_actual_transport_scopes
            .insert((0x0011, 0x0022));
        collector.section_trackers.insert(
            (0x0010, 0x40, 0x0001, super::TRACKER_GLOBAL_SCOPE),
            complete_tracker(),
        );
        collector
            .nit_transport_scopes
            .insert((0x40, 0x0001), BTreeSet::from([(0x0011, 0x0022)]));
        collector
            .state()
            .semantic_facts_by_service
            .into_iter()
            .next()
            .expect("service semantic facts")
    }

    #[test]
    fn semantic_facts_preserve_mpeg2_video_without_product_policy() {
        let facts = semantic_facts_for(base_service(vec![stream(0x0101, 0x02)]));
        assert_eq!(facts.service_type, Some(0x01));
        assert_eq!(facts.elementary_streams.len(), 1);
        assert_eq!(facts.elementary_streams[0].stream_type, 0x02);
        assert!(!facts.requires_cas);
    }

    #[test]
    fn semantic_facts_preserve_avc_video_without_product_policy() {
        let facts = semantic_facts_for(base_service(vec![stream(0x0101, 0x1b)]));
        assert_eq!(facts.elementary_streams[0].stream_type, 0x1b);
        assert_eq!(facts.free_ca_mode, Some(false));
        assert!(!facts.requires_cas);
    }

    #[test]
    fn semantic_facts_report_transport_level_nit_incompleteness() {
        let mut collector = ServiceDiscoveryCollector::default();
        let service = base_service(vec![stream(0x0101, 0x1b)]);
        let key = (
            service.transport_stream_id,
            service.original_network_id,
            service.service_id,
        );
        collector.engine.transports.insert(
            (key.0, key.1),
            DiscoveredTransport {
                transport_stream_id: key.0,
                original_network_id: key.1,
                services: BTreeSet::from([key.2]),
                ..DiscoveredTransport::default()
            },
        );
        collector.engine.services.insert(key, service);
        collector.section_trackers.insert(
            (0x0000, 0x00, 0x0011, super::TRACKER_GLOBAL_SCOPE),
            complete_tracker(),
        );
        collector
            .section_trackers
            .insert((0x0011, 0x42, 0x0011, 0x0022), complete_tracker());
        collector
            .sdt_actual_transport_scopes
            .insert((0x0011, 0x0022));

        let state = collector.state();
        let facts = state
            .semantic_facts_by_service
            .first()
            .expect("service semantic facts");

        assert!(facts.missing_components.contains(&"NIT"));
        assert_eq!(facts.elementary_streams[0].stream_type, 0x1b);
        assert_eq!(state.publish_stage(), DiscoveryPublishStage::Partial);
    }

    #[test]
    fn semantic_facts_preserve_audio_data_and_hevc_stream_types() {
        for stream_type in [0x0f, 0x0d, 0x24] {
            let facts = semantic_facts_for(base_service(vec![stream(0x0101, stream_type)]));
            assert_eq!(facts.elementary_streams.len(), 1);
            assert_eq!(facts.elementary_streams[0].stream_type, stream_type);
            assert!(!facts.requires_cas);
        }
    }

    #[test]
    fn semantic_cas_fact_comes_only_from_ca_descriptors() {
        let mut sdt_scrambled = base_service(vec![stream(0x0101, 0x1b)]);
        sdt_scrambled.free_ca_mode = Some(true);
        let facts = semantic_facts_for(sdt_scrambled);
        assert_eq!(facts.free_ca_mode, Some(true));
        assert!(!facts.requires_cas);

        let mut program_ca = base_service(vec![stream(0x0101, 0x1b)]);
        program_ca.program_ca_descriptors.push(CaDescriptor {
            ca_system_id: 0x0005,
            ca_pid: 0x0123,
            private_data: Vec::new(),
            raw_descriptor: vec![0x09, 0x04, 0x00, 0x05, 0xe1, 0x23],
        });
        let facts = semantic_facts_for(program_ca);
        assert!(facts.requires_cas);

        let mut es_ca = base_service(vec![stream(0x0101, 0x1b)]);
        es_ca.es_ca_descriptors.push(EsCaMetadata {
            elementary_pid: 0x0101,
            descriptors: vec![CaDescriptor {
                ca_system_id: 0x0005,
                ca_pid: 0x0124,
                private_data: Vec::new(),
                raw_descriptor: vec![0x09, 0x04, 0x00, 0x05, 0xe1, 0x24],
            }],
        });
        let facts = semantic_facts_for(es_ca);
        assert!(facts.requires_cas);
    }

    #[test]
    fn semantic_facts_do_not_filter_services_by_product_codec_support() {
        let video = semantic_facts_for(base_service(vec![stream(0x0101, 0x1b)]));
        let mut audio_service = base_service(vec![stream(0x0201, 0x0f)]);
        audio_service.service_id = 2;
        audio_service.service_type = Some(0x02);
        let audio = semantic_facts_for(audio_service);

        assert_eq!(video.service_id, 1);
        assert_eq!(video.elementary_streams[0].stream_type, 0x1b);
        assert_eq!(audio.service_id, 2);
        assert_eq!(audio.service_type, Some(0x02));
        assert_eq!(audio.elementary_streams[0].stream_type, 0x0f);
    }
}

#[cfg(test)]
mod current_version_tests {
    use super::ServiceDiscoveryCollector;
    use crate::sections::crc32_mpeg;

    fn section_with_crc(mut body: Vec<u8>) -> Vec<u8> {
        let crc = crc32_mpeg(&body);
        body.extend_from_slice(&crc.to_be_bytes());
        body
    }

    #[test]
    fn next_section_is_not_used_for_current_discovery() {
        let pat_next = section_with_crc(vec![
            0x00, 0xb0, 0x0d, 0x00, 0x11, 0xc0, 0x00, 0x00, 0x00, 0x01, 0xe1, 0x00,
        ]);
        let mut collector = ServiceDiscoveryCollector::default();
        collector.push_section(0x0000, &pat_next);
        assert!(!collector.state().component_complete("PAT"));
    }

    #[test]
    fn section_filter_pmt_pids_are_available_from_pat_before_viewable_snapshot() {
        let pat = section_with_crc(vec![
            0x00, 0xb0, 0x0d, 0x00, 0x11, 0xc1, 0x00, 0x00, 0x00, 0x01, 0xe1, 0x00,
        ]);
        let mut collector = ServiceDiscoveryCollector::default();
        collector.push_section(0x0000, &pat);
        assert_eq!(collector.pmt_pids_for_section_filters(), vec![0x0100]);
        assert!(collector.state().snapshot.pmt_pids_by_service.is_empty());
        assert!(collector.state().semantic_facts_by_service.is_empty());
    }

    #[test]
    fn version_change_replaces_changed_pat_state() {
        let mut collector = ServiceDiscoveryCollector::default();
        let pat_v1 = section_with_crc(vec![
            0x00, 0xb0, 0x0d, 0x00, 0x11, 0xc3, 0x00, 0x00, 0x00, 0x01, 0xe1, 0x00,
        ]);
        let pat_v2_sec0 = section_with_crc(vec![
            0x00, 0xb0, 0x0d, 0x00, 0x11, 0xc5, 0x00, 0x01, 0x00, 0x02, 0xe2, 0x00,
        ]);
        collector.push_section(0x0000, &pat_v1);
        assert!(collector.state().snapshot.pmt_pids_by_service.is_empty());
        assert!(collector.state().component_complete("PAT"));

        collector.push_section(0x0000, &pat_v2_sec0);
        let state = collector.state();
        assert!(!state.component_complete("PAT"));
        assert!(state
            .snapshot
            .pmt_pids_by_service
            .iter()
            .all(|m| m.service_id != 1));
        assert_eq!(
            state
                .snapshot
                .pmt_pids_by_service
                .iter()
                .find(|m| m.service_id == 2)
                .map(|m| m.pmt_pid),
            None
        );
    }

    #[test]
    fn tracker_does_not_mix_versions() {
        let mut collector = ServiceDiscoveryCollector::default();
        let pat_v1_sec0 = section_with_crc(vec![
            0x00, 0xb0, 0x0d, 0x00, 0x11, 0xc3, 0x00, 0x01, 0x00, 0x01, 0xe1, 0x00,
        ]);
        let pat_v2_sec1 = section_with_crc(vec![
            0x00, 0xb0, 0x0d, 0x00, 0x11, 0xc5, 0x01, 0x01, 0x00, 0x01, 0xe1, 0x00,
        ]);
        collector.push_section(0x0000, &pat_v1_sec0);
        collector.push_section(0x0000, &pat_v2_sec1);
        assert!(!collector.state().component_complete("PAT"));
    }
}

#[cfg(test)]
mod ca_metadata_tests {
    use super::*;

    fn section_with_crc(mut body: Vec<u8>) -> Vec<u8> {
        let crc = crate::sections::crc32_mpeg(&body);
        body.extend_from_slice(&crc.to_be_bytes());
        body
    }

    #[test]
    fn cat_ca_descriptor_is_preserved() {
        let cat = section_with_crc(vec![
            0x01, 0xb0, 0x0f, 0x00, 0x01, 0xc1, 0x00, 0x00, 0x09, 0x04, 0x00, 0x05, 0xe1, 0x23,
        ]);
        let mut collector = ServiceDiscoveryCollector::default();
        collector.push_section(0x0001, &cat);
        let state = collector.state();
        assert_eq!(state.snapshot.cat_ca.descriptors.len(), 1);
        assert_eq!(state.snapshot.cat_ca.descriptors[0].ca_pid, 0x0123);
    }

    #[test]
    fn cat_version_change_replaces_removed_ca_descriptor() {
        let cat_v1 = section_with_crc(vec![
            0x01, 0xb0, 0x0f, 0x00, 0x01, 0xc3, 0x00, 0x00, 0x09, 0x04, 0x00, 0x05, 0xe1, 0x23,
        ]);
        let cat_v2 = section_with_crc(vec![
            0x01, 0xb0, 0x0f, 0x00, 0x01, 0xc5, 0x00, 0x00, 0x09, 0x04, 0x00, 0x05, 0xe1, 0x24,
        ]);
        let mut collector = ServiceDiscoveryCollector::default();
        collector.push_section(0x0001, &cat_v1);
        assert_eq!(
            collector.state().snapshot.cat_ca.descriptors[0].ca_pid,
            0x0123
        );
        collector.push_section(0x0001, &cat_v2);
        let descriptors = &collector.state().snapshot.cat_ca.descriptors;
        assert_eq!(descriptors.len(), 1);
        assert_eq!(descriptors[0].ca_pid, 0x0124);
    }

    #[test]
    fn cat_multiple_sections_are_merged_for_same_version() {
        let cat_sec0 = section_with_crc(vec![
            0x01, 0xb0, 0x0f, 0x00, 0x01, 0xc3, 0x00, 0x01, 0x09, 0x04, 0x00, 0x05, 0xe1, 0x23,
        ]);
        let cat_sec1 = section_with_crc(vec![
            0x01, 0xb0, 0x0f, 0x00, 0x01, 0xc3, 0x01, 0x01, 0x09, 0x04, 0x00, 0x05, 0xe1, 0x24,
        ]);
        let mut collector = ServiceDiscoveryCollector::default();
        collector.push_section(0x0001, &cat_sec0);
        collector.push_section(0x0001, &cat_sec1);
        let descriptors = &collector.state().snapshot.cat_ca.descriptors;
        assert_eq!(descriptors.len(), 2);
        assert!(descriptors.iter().any(|d| d.ca_pid == 0x0123));
        assert!(descriptors.iter().any(|d| d.ca_pid == 0x0124));
    }

    #[test]
    fn ca_metadata_is_preserved_in_raw_service_and_semantic_facts() {
        let pat = section_with_crc(vec![
            0x00, 0xb0, 0x0d, 0x00, 0x11, 0xc1, 0x00, 0x00, 0x00, 0x01, 0xe1, 0x00,
        ]);
        let sdt_scrambled = section_with_crc(vec![
            0x42, 0xf0, 0x18, 0x00, 0x11, 0xc1, 0x00, 0x00, 0x00, 0x22, 0x00, 0x00, 0x01, 0xfc,
            0xf0, 0x07, 0x48, 0x05, 0x01, 0x00, 0x02, b'T', b'1',
        ]);
        let pmt_with_ca = section_with_crc(vec![
            0x02, 0xb0, 0x23, 0x00, 0x01, 0xc1, 0x00, 0x00, 0xe1, 0x01, 0xf0, 0x06, 0x09, 0x04,
            0x00, 0x05, 0xe1, 0x23, 0x1b, 0xe1, 0x01, 0xf0, 0x06, 0x09, 0x04, 0x00, 0x05, 0xe1,
            0x24, 0x0f, 0xe1, 0x02, 0xf0, 0x00,
        ]);
        let cat = section_with_crc(vec![
            0x01, 0xb0, 0x0f, 0x00, 0x01, 0xc1, 0x00, 0x00, 0x09, 0x04, 0x00, 0x05, 0xe1, 0x00,
        ]);

        let mut collector = ServiceDiscoveryCollector::default();
        collector.push_section(0x0000, &pat);
        collector.push_section(0x0011, &sdt_scrambled);
        collector.push_section(0x0100, &pmt_with_ca);
        collector.push_section(0x0001, &cat);

        let state = collector.state();
        let service = state
            .snapshot
            .services
            .iter()
            .find(|s| s.service_id == 1)
            .expect("raw service");
        assert_eq!(service.program_ca_descriptors.len(), 1);
        assert_eq!(service.program_ca_descriptors[0].ca_pid, 0x0123);
        let video_ca = service
            .es_ca_descriptors
            .iter()
            .find(|m| m.elementary_pid == 0x0101)
            .expect("video ES CA情報");
        assert_eq!(video_ca.descriptors[0].ca_pid, 0x0124);
        assert_eq!(state.snapshot.cat_ca.descriptors[0].ca_pid, 0x0100);

        let facts = state
            .semantic_facts_by_service
            .iter()
            .find(|p| p.service_id == 1)
            .expect("semantic facts");
        assert_eq!(facts.free_ca_mode, Some(true));
        assert!(facts.requires_cas);
        assert_eq!(facts.elementary_streams.len(), 2);
    }
}

#[cfg(test)]
mod service_scoped_ca_metadata_tests {
    use super::*;
    use crate::ca_descriptor::CaDescriptor;

    #[test]
    fn cat_only_does_not_make_service_scoped_ca() {
        let service = DiscoveredService {
            service_id: 101,
            transport_stream_id: 16625,
            original_network_id: 4,
            ..Default::default()
        };
        assert!(service.program_ca_descriptors.is_empty());
        assert!(service.es_ca_descriptors.is_empty());
    }

    #[test]
    fn pmt_program_or_es_ca_is_service_scoped() {
        let ca = CaDescriptor {
            ca_system_id: 0x0005,
            ca_pid: 0x0100,
            private_data: vec![1, 2, 3],
            raw_descriptor: vec![0x09, 0x07, 0x00, 0x05, 0xe1, 0x00, 1, 2, 3],
        };
        let service = DiscoveredService {
            service_id: 101,
            transport_stream_id: 16625,
            original_network_id: 4,
            program_ca_descriptors: vec![ca.clone()],
            es_ca_descriptors: vec![EsCaMetadata {
                elementary_pid: 0x0200,
                descriptors: vec![ca],
            }],
            ..Default::default()
        };
        assert!(!service.program_ca_descriptors.is_empty());
        assert!(!service.es_ca_descriptors.is_empty());
        assert_eq!(service.es_ca_descriptors[0].elementary_pid, 0x0200);
    }
}

#[cfg(test)]
mod section_tracker_consistency_tests {
    use super::SectionTracker;

    #[test]
    fn conflicting_last_section_number_never_becomes_complete() {
        let mut tracker = SectionTracker::default();
        tracker.mark_seen(3, 0, 1);
        tracker.mark_seen(3, 1, 0);
        assert!(!tracker.is_complete());
    }

    #[test]
    fn section_number_above_last_section_number_is_inconsistent() {
        let mut tracker = SectionTracker::default();
        tracker.mark_seen(3, 2, 1);
        assert!(!tracker.is_complete());
    }

    #[test]
    fn consistent_version_completes_normally() {
        let mut tracker = SectionTracker::default();
        tracker.mark_seen(3, 0, 1);
        tracker.mark_seen(3, 1, 1);
        assert!(tracker.is_complete());
    }
}

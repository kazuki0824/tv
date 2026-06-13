//! TS packet 検証と配送前の正規化を集約する入口。
//!
//! TEI / adaptation field / discontinuity / payload有無を1か所で決定する。

use maleicacid_tuner_hal2_common::{TS_PACKET_SIZE, TsPacketCompletionBuffer};
use crate::ts_core::PesDropReason;
use std::collections::BTreeMap;

const PIPELINE_GENERATION_INITIAL: u64 = 0;

#[cfg(test)]
pub fn lock_test_mutex<T>(
    mutex: &std::sync::Mutex<T>,
) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().expect("test mutex must be available")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TsPacketValidationError {
    WrongLength,
    MissingSyncByte,
    InvalidAdaptationControl,
    InvalidAdaptationLength,
}

#[derive(Clone, Copy, Debug)]
pub struct TsPacketView<'a> {
    pub pid: i32,
    pub transport_error_indicator: bool,
    pub payload_unit_start: bool,
    pub priority: bool,
    pub scrambling_control: u8,
    pub continuity_counter: u8,
    pub discontinuity_indicator: bool,
    pub random_access_indicator: bool,
    pub pcr_flag: bool,
    pub opcr_flag: bool,
    pub splicing_point_flag: bool,
    pub private_data_flag: bool,
    pub adaptation_extension_flag: bool,
    pub payload: Option<&'a [u8]>,
    pub pcr_90khz: Option<u64>,
}

impl<'a> TsPacketView<'a> {
    pub fn parse(packet: &'a [u8]) -> Option<Self> {
        Self::validate(packet).ok()
    }

    pub fn validate(packet: &'a [u8]) -> Result<Self, TsPacketValidationError> {
        if packet.len() != TS_PACKET_SIZE {
            return Err(TsPacketValidationError::WrongLength);
        }
        if packet[0] != 0x47 {
            return Err(TsPacketValidationError::MissingSyncByte);
        }
        let transport_error_indicator = (packet[1] & 0x80) != 0;
        let payload_unit_start = (packet[1] & 0x40) != 0;
        let priority = (packet[1] & 0x20) != 0;
        let pid = (((packet[1] & 0x1f) as i32) << 8) | packet[2] as i32;
        let scrambling_control = (packet[3] >> 6) & 0x03;
        let adaptation_control = (packet[3] >> 4) & 0x03;
        let continuity_counter = packet[3] & 0x0f;
        if adaptation_control == 0 {
            return Err(TsPacketValidationError::InvalidAdaptationControl);
        }
        let mut offset = 4usize;
        let mut pcr_90khz = None;
        let mut discontinuity_indicator = false;
        let mut random_access_indicator = false;
        let mut pcr_flag = false;
        let mut opcr_flag = false;
        let mut splicing_point_flag = false;
        let mut private_data_flag = false;
        let mut adaptation_extension_flag = false;
        if adaptation_control == 2 || adaptation_control == 3 {
            if offset >= packet.len() {
                return Err(TsPacketValidationError::InvalidAdaptationLength);
            }
            let adaptation_len = packet[offset] as usize;
            if offset + 1 + adaptation_len > packet.len() {
                return Err(TsPacketValidationError::InvalidAdaptationLength);
            }
            if adaptation_len > 0 {
                let flags = packet[offset + 1];
                discontinuity_indicator = (flags & 0x80) != 0;
                random_access_indicator = (flags & 0x40) != 0;
                let adaptation_end = offset + 1 + adaptation_len;
                let mut cursor = offset + 2;
                // adaptation field の flag は MPEG-TS の構造境界である。
                // flag が立っているのに対応する byte 数が不足する packet は malformed として拒否する。
                // 不足時に flag を単に無視すると、PCR/OPCR/record index の観測前提が崩れる。
                if (flags & 0x10) != 0 {
                    if cursor + 6 > adaptation_end {
                        return Err(TsPacketValidationError::InvalidAdaptationLength);
                    }
                    pcr_flag = true;
                    let p = &packet[cursor..cursor + 6];
                    let base = ((p[0] as u64) << 25)
                        | ((p[1] as u64) << 17)
                        | ((p[2] as u64) << 9)
                        | ((p[3] as u64) << 1)
                        | ((p[4] as u64) >> 7);
                    pcr_90khz = Some(base);
                    cursor += 6;
                }
                if (flags & 0x08) != 0 {
                    if cursor + 6 > adaptation_end {
                        return Err(TsPacketValidationError::InvalidAdaptationLength);
                    }
                    opcr_flag = true;
                    cursor += 6;
                }
                if (flags & 0x04) != 0 {
                    if cursor >= adaptation_end {
                        return Err(TsPacketValidationError::InvalidAdaptationLength);
                    }
                    splicing_point_flag = true;
                    cursor += 1;
                }
                if (flags & 0x02) != 0 {
                    if cursor >= adaptation_end {
                        return Err(TsPacketValidationError::InvalidAdaptationLength);
                    }
                    let private_len = packet[cursor] as usize;
                    cursor += 1;
                    if cursor + private_len > adaptation_end {
                        return Err(TsPacketValidationError::InvalidAdaptationLength);
                    }
                    private_data_flag = true;
                    cursor += private_len;
                }
                if (flags & 0x01) != 0 {
                    if cursor >= adaptation_end {
                        return Err(TsPacketValidationError::InvalidAdaptationLength);
                    }
                    let extension_len = packet[cursor] as usize;
                    cursor += 1;
                    if cursor + extension_len > adaptation_end {
                        return Err(TsPacketValidationError::InvalidAdaptationLength);
                    }
                    adaptation_extension_flag = true;
                }
            }
            offset += 1 + adaptation_len;
            if adaptation_control == 2 {
                return Ok(Self {
                    pid,
                    transport_error_indicator,
                    payload_unit_start,
                    priority,
                    scrambling_control,
                    continuity_counter,
                    discontinuity_indicator,
                    random_access_indicator,
                    pcr_flag,
                    opcr_flag,
                    splicing_point_flag,
                    private_data_flag,
                    adaptation_extension_flag,
                    payload: None,
                    pcr_90khz,
                });
            }
        }
        Ok(Self {
            pid,
            transport_error_indicator,
            payload_unit_start,
            priority,
            scrambling_control,
            continuity_counter,
            discontinuity_indicator,
            random_access_indicator,
            pcr_flag,
            opcr_flag,
            splicing_point_flag,
            private_data_flag,
            adaptation_extension_flag,
            payload: (offset < packet.len()).then(|| &packet[offset..]),
            pcr_90khz,
        })
    }
}


// assembler / tracker 所有権は packet_pipeline に閉じ込める。
// 呼び出し元は Pipeline* の操作だけを使い、sections.rs / ts_core.rs の状態型を直接保持しない。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PipelineSectionState {
    inner: crate::sections::SectionAssembler,
}

impl PipelineSectionState {
    pub fn push_payload_with_outcome(
        &mut self,
        payload_unit_start: bool,
        payload: &[u8],
    ) -> crate::sections::SectionPushOutcome {
        self.inner.push_payload_with_outcome(payload_unit_start, payload)
    }

    pub fn oversized_section_drops(&self) -> u64 {
        self.inner.oversized_section_drops()
    }

    pub fn stale_partial_section_discards(&self) -> u64 {
        self.inner.stale_partial_section_discards()
    }
}

#[derive(Clone, Debug, Default)]
pub struct PipelinePesState {
    inner: crate::ts_core::PesAssembler,
}

impl PipelinePesState {
    pub fn push(
        &mut self,
        pid: u16,
        payload_unit_start: bool,
        payload: &[u8],
    ) -> Vec<crate::ts_core::PesPacket> {
        self.inner.push(pid, payload_unit_start, payload)
    }

    pub fn take_drop_diagnostic(&mut self) -> Option<(PesDropReason, u64)> {
        self.inner.take_drop_diagnostic()
    }
}

#[derive(Clone, Debug, Default)]
pub struct PipelineContinuityState {
    inner: crate::ts_core::ContinuityTracker,
}

impl PipelineContinuityState {
    pub fn observe(
        &mut self,
        pid: u16,
        continuity_counter: u8,
        has_payload: bool,
    ) -> crate::ts_core::ContinuityOutcome {
        self.inner.observe(pid, continuity_counter, has_payload)
    }

    pub fn reset_pid(&mut self, pid: u16) {
        self.inner.reset_pid(pid);
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PipelineResyncState {
    inner: TsPacketCompletionBuffer,
}

impl PipelineResyncState {
    pub fn push(&mut self, data: &[u8]) -> Vec<[u8; TS_PACKET_SIZE]> {
        self.inner.push(data).packets
    }

    pub fn drain_for_boundary(&mut self) -> maleicacid_tuner_hal2_common::TsPacketBufferDrain {
        self.inner.drain_for_boundary()
    }
}

/// TS入力からcontinuity、section/PES組立、resync状態までを所有する単一入口。
///
/// `DemuxHandle` はこの構造体を1つだけ保持し、assembler/tracker/resyncを
/// 個別フィールドとして持たない。これにより stream境界、source filter経路、
/// DVR/record配送が同じpacket pipeline状態を共有する。
#[derive(Clone, Debug, Default)]
pub struct PacketPipeline {
    pub(crate) section_assemblers: BTreeMap<(crate::TsInputOrigin, i32, i32), PipelineSectionState>,
    pub(crate) pes_assemblers: BTreeMap<(crate::TsInputOrigin, i32, i32), PipelinePesState>,
    pub(crate) section_assembler_generations: BTreeMap<(crate::TsInputOrigin, i32), u64>,
    pub(crate) pes_assembler_generations: BTreeMap<(crate::TsInputOrigin, i32), u64>,
    pub(crate) filter_section_flush_generations: BTreeMap<(crate::TsInputOrigin, i32, i32), u64>,
    pub(crate) filter_pes_flush_generations: BTreeMap<(crate::TsInputOrigin, i32, i32), u64>,
    pub(crate) continuity_trackers: BTreeMap<crate::TsInputOrigin, PipelineContinuityState>,
    pub(crate) resync: PipelineResyncState,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum PipelineInputKind {
    Live,
    Playback,
    SourceFilter { source_filter_id: i32, source_filter_generation: u64 },
}

#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct PipelineReport {
    pub accepted_packets: usize,
    pub dropped_packets: usize,
    pub malformed_packets: usize,
    pub drop_reasons: Vec<PipelineDropReason>,
    pub delivery_actions: Vec<PipelineDeliveryAction>,
    pub generated_events: Vec<PipelineGeneratedEvent>,
    pub diagnostics: Vec<PipelineDiagnostic>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum PipelineDropReason {
    TransportErrorIndicator,
    MalformedPacket,
    DuplicatePacket,
    NoPayload,
    AssemblyDrop,
    ResidualBytes,
    PesAssemblerOverflow,
    SectionGenerationOverflow,
    PesGenerationOverflow,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum PipelineDeliveryAction {
    RawPacket { filter_id: i32 },
    RecordPacket { filter_id: i32 },
    DvrMirror { dvr_id: i32 },
    SectionPayload { filter_id: i32 },
    PesPayload { filter_id: i32 },
    AvPayload { filter_id: i32 },
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum PipelineGeneratedEvent {
    DataReady { filter_id: i32 },
    Section { filter_id: i32, raw: bool },
    Pes { filter_id: i32, raw: bool },
    Record { filter_id: i32 },
    SectionPayloadReady {
        filter_id: i32,
        pid: i32,
        generation: u64,
        bytes: Vec<u8>,
    },
    PesPacketReady {
        filter_id: i32,
        pid: i32,
        generation: u64,
        packet: crate::ts_core::PesPacket,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PipelineDiagnosticKind {
    MalformedTsPacket,
    TeiPacketDrop,
    DuplicatePacketDrop,
    NoPayloadPacketDrop,
    SectionAssemblyDrop,
    SectionGenerationOverflow,
    PesGenerationOverflow,
    PesAssemblerDrop(PesDropReason),
    ResidualBytesDrop,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PipelineDiagnostic { pub kind: PipelineDiagnosticKind, pub pid: Option<i32> }

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum PipelineOpenKind { Raw, Record, Section, Pes, Av, Other }

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct PipelineFilterView {
    pub filter_id: i32,
    pub tpid: Option<i32>,
    pub started: bool,
    pub has_upstream: bool,
    pub open_kind: PipelineOpenKind,
    pub section_raw: bool,
    pub pes_raw: bool,
    pub wants_record_index: bool,
}

impl PipelineFilterView {
    fn accepts_pid(self, pid: i32) -> bool {
        self.started && !self.has_upstream && self.tpid == Some(pid)
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct FilterPipelineConfig { pub tpid: Option<i32>, pub raw: bool }
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum PipelineError { InvalidState, InvalidPacket, Internal }
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum PipelineBoundaryReason { TuneStart, ScanStart, FrontendClose, FrontendUnbind, SourceFilterChange, DvrPlaybackDiscontinuity }

#[derive(Debug, Clone, Copy)]
pub enum PacketAcceptOutcome<'a> {
    Accepted(TsPacketView<'a>),
    Malformed,
    TransportError,
    Duplicate,
    NoPayload,
}

impl PacketPipeline {
    pub fn validate_packet(bytes: &[u8]) -> Result<TsPacketView<'_>, TsPacketValidationError> {
        TsPacketView::validate(bytes)
    }



    pub fn push_ts_packet(&mut self, packet: &[u8], kind: PipelineInputKind) -> PipelineReport {
        let mut report = PipelineReport::default();
        let origin = match kind {
            PipelineInputKind::Live => crate::TsInputOrigin::Frontend,
            PipelineInputKind::Playback => crate::TsInputOrigin::Playback,
            PipelineInputKind::SourceFilter { source_filter_id, source_filter_generation } => {
                crate::TsInputOrigin::SourceFilter { source_filter_id, source_filter_generation }
            }
        };
        let view = match Self::validate_packet(packet) {
            Ok(view) => view,
            Err(_) => {
                report.dropped_packets += 1;
                report.malformed_packets += 1;
                report.drop_reasons.push(PipelineDropReason::MalformedPacket);
                report.diagnostics.push(PipelineDiagnostic { kind: PipelineDiagnosticKind::MalformedTsPacket, pid: None });
                return report;
            }
        };
        if view.transport_error_indicator {
            report.dropped_packets += 1;
            report.drop_reasons.push(PipelineDropReason::TransportErrorIndicator);
            report.diagnostics.push(PipelineDiagnostic { kind: PipelineDiagnosticKind::TeiPacketDrop, pid: Some(view.pid) });
            return report;
        }
        if view.discontinuity_indicator {
            self.reset_continuity_pid(origin, view.pid as u16);
            self.reset_assembly_for_origin_pid(origin, view.pid);
        }
        let continuity = self.check_continuity(
            origin,
            view.pid as u16,
            view.continuity_counter,
            view.payload.is_some(),
        );
        if matches!(continuity, crate::ts_core::ContinuityOutcome::Duplicate) {
            report.dropped_packets += 1;
            report.drop_reasons.push(PipelineDropReason::DuplicatePacket);
            report.diagnostics.push(PipelineDiagnostic { kind: PipelineDiagnosticKind::DuplicatePacketDrop, pid: Some(view.pid) });
            return report;
        }
        if matches!(continuity, crate::ts_core::ContinuityOutcome::Discontinuity) {
            self.reset_assembly_for_origin_pid(origin, view.pid);
        }
        if view.payload.is_none() {
            report.dropped_packets += 1;
            report.drop_reasons.push(PipelineDropReason::NoPayload);
            report.diagnostics.push(PipelineDiagnostic { kind: PipelineDiagnosticKind::NoPayloadPacketDrop, pid: Some(view.pid) });
            return report;
        }
        report.accepted_packets += 1;
        report
    }

    pub fn inspect_ts_packet<'a>(&self, packet: &'a [u8]) -> Option<TsPacketView<'a>> {
        Self::validate_packet(packet).ok().filter(|view| !view.transport_error_indicator)
    }

    pub(crate) fn accept_ts_packet_with_outcome<'a>(&mut self, packet: &'a [u8], origin: crate::TsInputOrigin) -> PacketAcceptOutcome<'a> {
        let view = match Self::validate_packet(packet) {
            Ok(view) => view,
            Err(_) => return PacketAcceptOutcome::Malformed,
        };
        if view.transport_error_indicator {
            return PacketAcceptOutcome::TransportError;
        }
        if view.discontinuity_indicator {
            self.reset_continuity_pid(origin, view.pid as u16);
            self.reset_assembly_for_origin_pid(origin, view.pid);
        }
        let continuity = self.check_continuity(
            origin,
            view.pid as u16,
            view.continuity_counter,
            view.payload.is_some(),
        );
        if matches!(continuity, crate::ts_core::ContinuityOutcome::Duplicate) {
            return PacketAcceptOutcome::Duplicate;
        }
        if matches!(continuity, crate::ts_core::ContinuityOutcome::Discontinuity) {
            self.reset_assembly_for_origin_pid(origin, view.pid);
        }
        if view.payload.is_none() {
            return PacketAcceptOutcome::NoPayload;
        }
        PacketAcceptOutcome::Accepted(view)
    }

    pub(crate) fn accept_ts_packet<'a>(&mut self, packet: &'a [u8], origin: crate::TsInputOrigin) -> Option<TsPacketView<'a>> {
        match self.accept_ts_packet_with_outcome(packet, origin) {
            PacketAcceptOutcome::Accepted(view) => Some(view),
            _ => None,
        }
    }


    pub(crate) fn plan_packet_delivery(
        &self,
        pid: i32,
        origin: crate::TsInputOrigin,
        filters: &[PipelineFilterView],
    ) -> Vec<PipelineDeliveryAction> {
        let mut actions = Vec::new();
        for filter in filters.iter().copied().filter(|filter| filter.accepts_pid(pid)) {
            match filter.open_kind {
                PipelineOpenKind::Raw => actions.push(PipelineDeliveryAction::RawPacket { filter_id: filter.filter_id }),
                PipelineOpenKind::Record => {
                    if origin.allows_record_mirror() {
                        actions.push(PipelineDeliveryAction::DvrMirror { dvr_id: filter.filter_id });
                    }
                    if filter.wants_record_index {
                        actions.push(PipelineDeliveryAction::RecordPacket { filter_id: filter.filter_id });
                    }
                }
                _ => {}
            }
        }
        actions
    }

    pub fn plan_section_filters(&self, pid: i32, filters: &[PipelineFilterView]) -> Vec<i32> {
        filters
            .iter()
            .copied()
            .filter(|filter| filter.accepts_pid(pid) && filter.open_kind == PipelineOpenKind::Section)
            .map(|filter| filter.filter_id)
            .collect()
    }

    pub fn plan_pes_actions(&self, pid: i32, filters: &[PipelineFilterView]) -> Vec<PipelineDeliveryAction> {
        filters
            .iter()
            .copied()
            .filter(|filter| filter.accepts_pid(pid) && matches!(filter.open_kind, PipelineOpenKind::Pes | PipelineOpenKind::Av))
            .map(|filter| match filter.open_kind {
                PipelineOpenKind::Av => PipelineDeliveryAction::AvPayload { filter_id: filter.filter_id },
                _ => PipelineDeliveryAction::PesPayload { filter_id: filter.filter_id },
            })
            .collect()
    }

    pub fn plan_pes_filters(&self, pid: i32, filters: &[PipelineFilterView]) -> Vec<i32> {
        self.plan_pes_actions(pid, filters)
            .into_iter()
            .filter_map(|action| match action {
                PipelineDeliveryAction::PesPayload { filter_id } | PipelineDeliveryAction::AvPayload { filter_id } => Some(filter_id),
                _ => None,
            })
            .collect()
    }


    pub(crate) fn plan_ts_packet_report(
        &self,
        view: &TsPacketView<'_>,
        origin: crate::TsInputOrigin,
        filters: &[PipelineFilterView],
    ) -> PipelineReport {
        let mut report = PipelineReport::default();
        report.accepted_packets = 1;
        report.delivery_actions.extend(self.plan_packet_delivery(view.pid, origin, filters));
        if view.payload.is_some() {
            for filter_id in self.plan_section_filters(view.pid, filters) {
                report.delivery_actions.push(PipelineDeliveryAction::SectionPayload { filter_id });
            }
            report.delivery_actions.extend(self.plan_pes_actions(view.pid, filters));
        }
        for action in report.delivery_actions.iter() {
            match *action {
                PipelineDeliveryAction::RawPacket { filter_id } => {
                    report.generated_events.push(PipelineGeneratedEvent::DataReady { filter_id });
                }
                PipelineDeliveryAction::RecordPacket { filter_id } => {
                    report.generated_events.push(PipelineGeneratedEvent::Record { filter_id });
                }
                PipelineDeliveryAction::SectionPayload { filter_id } => {
                    report.generated_events.push(PipelineGeneratedEvent::Section { filter_id, raw: filters.iter().find(|filter| filter.filter_id == filter_id).map(|filter| filter.section_raw).unwrap_or(false) });
                }
                PipelineDeliveryAction::PesPayload { filter_id } | PipelineDeliveryAction::AvPayload { filter_id } => {
                    report.generated_events.push(PipelineGeneratedEvent::Pes { filter_id, raw: filters.iter().find(|filter| filter.filter_id == filter_id).map(|filter| filter.pes_raw).unwrap_or(false) });
                }
                PipelineDeliveryAction::DvrMirror { .. } => {}
            }
        }
        report
    }
    pub(crate) fn plan_and_assemble_ts_packet_report(
        &mut self,
        view: &TsPacketView<'_>,
        origin: crate::TsInputOrigin,
        filters: &[PipelineFilterView],
    ) -> PipelineReport {
        let mut report = self.plan_ts_packet_report(view, origin, filters);
        let Some(payload) = view.payload else {
            return report;
        };

        let has_section_action = report.delivery_actions.iter().any(|action| matches!(action, PipelineDeliveryAction::SectionPayload { .. }));
        if has_section_action {
            let section_generation = if view.payload_unit_start {
                self.bump_section_generation(origin, view.pid)
            } else {
                Some(self.current_section_generation(origin, view.pid))
            };
            if let Some(section_generation) = section_generation {
                let section_filter_ids: Vec<i32> = report.delivery_actions.iter().filter_map(|action| match action {
                    PipelineDeliveryAction::SectionPayload { filter_id } => Some(*filter_id),
                    _ => None,
                }).collect();
                for filter_id in section_filter_ids {
                    let outcome = self.assemble_section_for_filter(
                        origin,
                        view.pid,
                        filter_id,
                        view.payload_unit_start,
                        payload,
                    );
                    if outcome.has_drop_or_discard() {
                        report.dropped_packets += 1;
                        report.drop_reasons.push(PipelineDropReason::AssemblyDrop);
                        report.diagnostics.push(PipelineDiagnostic { kind: PipelineDiagnosticKind::SectionAssemblyDrop, pid: Some(view.pid) });
                    }
                    for section in outcome.sections {
                        report.generated_events.push(PipelineGeneratedEvent::SectionPayloadReady {
                            filter_id,
                            pid: view.pid,
                            generation: section_generation,
                            bytes: section,
                        });
                    }
                }
            } else {
                report.dropped_packets += 1;
                report.drop_reasons.push(PipelineDropReason::SectionGenerationOverflow);
                report.diagnostics.push(PipelineDiagnostic { kind: PipelineDiagnosticKind::SectionGenerationOverflow, pid: Some(view.pid) });
                self.reset_assembly_for_origin_pid(origin, view.pid);
            }
        } else {
            self.drop_generations_for_pid_origin(origin, view.pid, true, false);
        }

        let has_pes_action = report.delivery_actions.iter().any(|action| matches!(action, PipelineDeliveryAction::PesPayload { .. } | PipelineDeliveryAction::AvPayload { .. }));
        if has_pes_action {
            let pes_generation = if view.payload_unit_start {
                self.bump_pes_generation(origin, view.pid)
            } else {
                Some(self.current_pes_generation(origin, view.pid))
            };
            let Some(pes_generation) = pes_generation else {
                report.dropped_packets += 1;
                report.drop_reasons.push(PipelineDropReason::PesGenerationOverflow);
                report.diagnostics.push(PipelineDiagnostic { kind: PipelineDiagnosticKind::PesGenerationOverflow, pid: Some(view.pid) });
                self.reset_assembly_for_origin_pid(origin, view.pid);
                return report;
            };
            let pes_filter_ids: Vec<i32> = report.delivery_actions.iter().filter_map(|action| match action {
                PipelineDeliveryAction::PesPayload { filter_id } | PipelineDeliveryAction::AvPayload { filter_id } => Some(*filter_id),
                _ => None,
            }).collect();
            for filter_id in pes_filter_ids {
                let packets = self.assemble_pes_for_filter(
                    origin,
                    filter_id,
                    view.pid as u16,
                    view.payload_unit_start,
                    payload,
                );
                if let Some((reason, _generation)) = self
                    .pes_assemblers
                    .get_mut(&(origin, view.pid, filter_id))
                    .and_then(|state| state.take_drop_diagnostic())
                {
                    report.dropped_packets += 1;
                    report.drop_reasons.push(PipelineDropReason::PesAssemblerOverflow);
                    report.diagnostics.push(PipelineDiagnostic { kind: PipelineDiagnosticKind::PesAssemblerDrop(reason), pid: Some(view.pid) });
                }
                for packet in packets {
                    report.generated_events.push(PipelineGeneratedEvent::PesPacketReady {
                        filter_id,
                        pid: view.pid,
                        generation: pes_generation,
                        packet,
                    });
                }
            }
        } else {
            self.drop_generations_for_pid_origin(origin, view.pid, false, true);
        }

        report
    }

    pub fn configure_filter(&mut self, filter_id: i32, _config: FilterPipelineConfig) -> Result<(), PipelineError> {
        self.clear_filter_state(filter_id);
        Ok(())
    }
    pub fn start_filter(&mut self, _filter_id: i32) -> Result<(), PipelineError> { Ok(()) }
    pub fn stop_filter(&mut self, _filter_id: i32) -> Result<(), PipelineError> { Ok(()) }
    pub fn remove_filter(&mut self, filter_id: i32) -> Result<(), PipelineError> { self.clear_filter_state(filter_id); Ok(()) }
    pub fn reset_boundary_for_reason(&mut self, _reason: PipelineBoundaryReason) -> Result<PipelineResetReport, PipelineError> { Ok(self.reset_boundary()) }

    pub fn clear_filter_state(&mut self, filter_id: i32) {
        self.section_assemblers
            .retain(|(_, _, stored_filter_id), _| *stored_filter_id != filter_id);
        self.pes_assemblers
            .retain(|(_, _, stored_filter_id), _| *stored_filter_id != filter_id);
        self.filter_section_flush_generations
            .retain(|(_, id, _), _| *id != filter_id);
        self.filter_pes_flush_generations
            .retain(|(_, id, _), _| *id != filter_id);
    }

    pub fn oversized_section_drop_count(&self) -> u64 {
        self.section_assemblers
            .values()
            .map(|assembler| assembler.oversized_section_drops())
            .sum()
    }

    pub fn stale_partial_section_discard_count(&self) -> u64 {
        self.section_assemblers
            .values()
            .map(|assembler| assembler.stale_partial_section_discards())
            .sum()
    }


    pub(crate) fn reset_assembly_for_origin_pid(&mut self, origin: crate::TsInputOrigin, pid: i32) {
        // discontinuity は対象 PID の section/PES assembler だけを破棄する。
        // 無関係な PID の途中 section/PES を同一 origin というだけで破棄してはならない。
        self.section_assemblers.retain(|(stored_origin, stored_pid, _), _| {
            *stored_origin != origin || *stored_pid != pid
        });
        self.pes_assemblers.retain(|(stored_origin, stored_pid, _), _| {
            *stored_origin != origin || *stored_pid != pid
        });
        self.section_assembler_generations.retain(|(stored_origin, stored_pid), _| {
            *stored_origin != origin || *stored_pid != pid
        });
        self.pes_assembler_generations.retain(|(stored_origin, stored_pid), _| {
            *stored_origin != origin || *stored_pid != pid
        });
        self.filter_section_flush_generations.retain(|(stored_origin, _, stored_pid), _| {
            *stored_origin != origin || *stored_pid != pid
        });
        self.filter_pes_flush_generations.retain(|(stored_origin, _, stored_pid), _| {
            *stored_origin != origin || *stored_pid != pid
        });
    }

    pub(crate) fn reset_downstream_assembly_for_origin_pid_filter(
        &mut self,
        origin: crate::TsInputOrigin,
        pid: i32,
        filter_id: i32,
    ) {
        self.section_assemblers.retain(|(stored_origin, stored_pid, stored_filter), _| {
            !(*stored_origin == origin && *stored_pid == pid && *stored_filter == filter_id)
        });
        self.pes_assemblers.retain(|(stored_origin, stored_pid, stored_filter), _| {
            !(*stored_origin == origin && *stored_pid == pid && *stored_filter == filter_id)
        });
    }

    pub(crate) fn reset_continuity_pid(&mut self, origin: crate::TsInputOrigin, pid: u16) {
        self.continuity_trackers.entry(origin).or_default().reset_pid(pid);
    }

    fn check_continuity(
        &mut self,
        origin: crate::TsInputOrigin,
        pid: u16,
        continuity_counter: u8,
        has_payload: bool,
    ) -> crate::ts_core::ContinuityOutcome {
        self.continuity_trackers
            .entry(origin)
            .or_default()
            .observe(pid, continuity_counter, has_payload)
    }

    pub(crate) fn assemble_section_for_filter(
        &mut self,
        origin: crate::TsInputOrigin,
        pid: i32,
        filter_id: i32,
        payload_unit_start: bool,
        payload: &[u8],
    ) -> crate::sections::SectionPushOutcome {
        self.section_assemblers
            .entry((origin, pid, filter_id))
            .or_default()
            .push_payload_with_outcome(payload_unit_start, payload)
    }

    pub(crate) fn assemble_pes_for_filter(
        &mut self,
        origin: crate::TsInputOrigin,
        filter_id: i32,
        pid: u16,
        payload_unit_start: bool,
        payload: &[u8],
    ) -> Vec<crate::ts_core::PesPacket> {
        self.pes_assemblers
            .entry((origin, pid as i32, filter_id))
            .or_default()
            .push(pid, payload_unit_start, payload)
    }

    pub(crate) fn assembly_origins_for_pid(&self, pid: i32) -> Vec<crate::TsInputOrigin> {
        let mut origins = std::collections::BTreeSet::new();
        for (origin, stored_pid, _) in self.section_assemblers.keys() {
            if *stored_pid == pid {
                origins.insert(*origin);
            }
        }
        for (origin, stored_pid, _) in self.pes_assemblers.keys() {
            if *stored_pid == pid {
                origins.insert(*origin);
            }
        }
        for (origin, stored_pid) in self.section_assembler_generations.keys() {
            if *stored_pid == pid {
                origins.insert(*origin);
            }
        }
        for (origin, stored_pid) in self.pes_assembler_generations.keys() {
            if *stored_pid == pid {
                origins.insert(*origin);
            }
        }
        for (origin, _, stored_pid) in self.filter_section_flush_generations.keys() {
            if *stored_pid == pid {
                origins.insert(*origin);
            }
        }
        for (origin, _, stored_pid) in self.filter_pes_flush_generations.keys() {
            if *stored_pid == pid {
                origins.insert(*origin);
            }
        }
        origins.into_iter().collect()
    }

    pub(crate) fn remove_section_for_filter_ids_origin_pid(
        &mut self,
        origin: crate::TsInputOrigin,
        pid: i32,
        filter_ids: &[i32],
    ) {
        self.section_assemblers.retain(|(stored_origin, stored_pid, filter_id), _| {
            !(*stored_origin == origin
                && *stored_pid == pid
                && filter_ids.iter().any(|id| id == filter_id))
        });
        self.filter_section_flush_generations.retain(|(stored_origin, filter_id, stored_pid), _| {
            !(*stored_origin == origin
                && *stored_pid == pid
                && filter_ids.iter().any(|id| id == filter_id))
        });
        self.section_assembler_generations
            .retain(|(stored_origin, stored_pid), _| !(*stored_origin == origin && *stored_pid == pid));
    }

    pub(crate) fn remove_pes_for_filter_ids_origin_pid(
        &mut self,
        origin: crate::TsInputOrigin,
        pid: i32,
        filter_ids: &[i32],
    ) {
        self.pes_assemblers.retain(|(stored_origin, stored_pid, filter_id), _| {
            !(*stored_origin == origin
                && *stored_pid == pid
                && filter_ids.iter().any(|id| id == filter_id))
        });
        self.filter_pes_flush_generations.retain(|(stored_origin, filter_id, stored_pid), _| {
            !(*stored_origin == origin
                && *stored_pid == pid
                && filter_ids.iter().any(|id| id == filter_id))
        });
        self.pes_assembler_generations
            .retain(|(stored_origin, stored_pid), _| !(*stored_origin == origin && *stored_pid == pid));
    }

    pub(crate) fn drop_generations_for_pid_origin(&mut self, origin: crate::TsInputOrigin, pid: i32, section: bool, pes: bool) {
        if section {
            self.section_assembler_generations
                .retain(|(stored_origin, stored_pid), _| !(*stored_origin == origin && *stored_pid == pid));
            self.filter_section_flush_generations
                .retain(|(stored_origin, _, stored_pid), _| !(*stored_origin == origin && *stored_pid == pid));
        }
        if pes {
            self.pes_assembler_generations
                .retain(|(stored_origin, stored_pid), _| !(*stored_origin == origin && *stored_pid == pid));
            self.filter_pes_flush_generations
                .retain(|(stored_origin, _, stored_pid), _| !(*stored_origin == origin && *stored_pid == pid));
        }
    }


    pub(crate) fn current_section_generation(&self, origin: crate::TsInputOrigin, pid: i32) -> u64 {
        self.section_assembler_generations
            .get(&(origin, pid))
            .copied()
            .unwrap_or(PIPELINE_GENERATION_INITIAL)
    }

    pub(crate) fn current_pes_generation(&self, origin: crate::TsInputOrigin, pid: i32) -> u64 {
        self.pes_assembler_generations
            .get(&(origin, pid))
            .copied()
            .unwrap_or(PIPELINE_GENERATION_INITIAL)
    }

    pub(crate) fn bump_section_generation(&mut self, origin: crate::TsInputOrigin, pid: i32) -> Option<u64> {
        let generation = self.section_assembler_generations.entry((origin, pid)).or_insert(0);
        let next = generation.checked_add(1)?;
        *generation = next;
        Some(next)
    }

    pub(crate) fn bump_pes_generation(&mut self, origin: crate::TsInputOrigin, pid: i32) -> Option<u64> {
        let generation = self.pes_assembler_generations.entry((origin, pid)).or_insert(0);
        let next = generation.checked_add(1)?;
        *generation = next;
        Some(next)
    }

    pub(crate) fn section_generation_allows_delivery(
        &self,
        origin: crate::TsInputOrigin,
        filter_id: i32,
        pid: i32,
        generation: u64,
    ) -> bool {
        self.filter_section_flush_generations
            .get(&(origin, filter_id, pid))
            .map_or(true, |flushed_generation| generation > *flushed_generation)
    }

    pub(crate) fn pes_generation_allows_delivery(
        &self,
        origin: crate::TsInputOrigin,
        filter_id: i32,
        pid: i32,
        generation: u64,
    ) -> bool {
        self.filter_pes_flush_generations
            .get(&(origin, filter_id, pid))
            .map_or(true, |flushed_generation| generation > *flushed_generation)
    }

    pub(crate) fn mark_filter_flush_generation_for_origin(
        &mut self,
        filter_id: i32,
        pid: i32,
        origin: crate::TsInputOrigin,
    ) {
        self.filter_section_flush_generations.insert(
            (origin, filter_id, pid),
            self.current_section_generation(origin, pid),
        );
        self.filter_pes_flush_generations.insert(
            (origin, filter_id, pid),
            self.current_pes_generation(origin, pid),
        );
    }

    pub(crate) fn flush_filter(&mut self, filter_id: i32, origins: &[(crate::TsInputOrigin, i32)]) {
        for (origin, pid) in origins.iter().copied() {
            self.mark_filter_flush_generation_for_origin(filter_id, pid, origin);
        }
        self.clear_filter_state_after_flush(filter_id);
    }

    pub fn clear_filter_state_after_flush(&mut self, filter_id: i32) {
        self.clear_filter_state(filter_id);
    }

    pub fn drop_all_pes(&mut self) {
        self.pes_assemblers.clear();
        self.pes_assembler_generations.clear();
        self.filter_pes_flush_generations.clear();
    }

    pub fn has_pending_pes(&self) -> bool {
        !self.pes_assemblers.is_empty()
    }

    pub fn has_pending_section(&self) -> bool {
        !self.section_assemblers.is_empty()
    }

    #[cfg(test)]
    pub(crate) fn test_assemble_pes_for_filter(
        &mut self,
        origin: crate::TsInputOrigin,
        pid: u16,
        payload_unit_start: bool,
        payload: &[u8],
    ) -> Vec<crate::ts_core::PesPacket> {
        self.assemble_pes_for_filter(origin, pid as i32, pid, payload_unit_start, payload)
    }

    #[cfg(test)]
    pub(crate) fn test_record_oversized_section_drop(&mut self, origin: crate::TsInputOrigin, filter_id: i32) -> bool {
        self.section_assemblers
            .entry((origin, filter_id, filter_id))
            .or_default()
            .inner
            .set_expected_len_or_drop(maleicacid_tuner_hal2_common::MAX_SECTION_PAYLOAD_BYTES + 1)
    }

    #[cfg(test)]
    pub(crate) fn test_assemble_section_for_filter(
        &mut self,
        origin: crate::TsInputOrigin,
        filter_id: i32,
        payload_unit_start: bool,
        payload: &[u8],
    ) -> Vec<Vec<u8>> {
        self.assemble_section_for_filter(origin, filter_id, filter_id, payload_unit_start, payload).sections
    }

    #[cfg(test)]
    pub(crate) fn test_seed_section(&mut self, origin: crate::TsInputOrigin, filter_id: i32) {
        self.test_seed_section_for_pid(origin, filter_id, filter_id);
    }

    #[cfg(test)]
    pub(crate) fn test_seed_section_for_pid(&mut self, origin: crate::TsInputOrigin, pid: i32, filter_id: i32) {
        self.section_assemblers.entry((origin, pid, filter_id)).or_default();
    }

    #[cfg(test)]
    pub(crate) fn test_seed_pes(&mut self, origin: crate::TsInputOrigin, filter_id: i32) {
        self.test_seed_pes_for_pid(origin, filter_id, filter_id);
    }

    #[cfg(test)]
    pub(crate) fn test_seed_pes_for_pid(&mut self, origin: crate::TsInputOrigin, pid: i32, filter_id: i32) {
        self.pes_assemblers.entry((origin, pid, filter_id)).or_default();
    }

    pub fn split_ts_bytes(&mut self, input: &[u8], kind: PipelineInputKind) -> Vec<[u8; TS_PACKET_SIZE]> {
        match kind {
            PipelineInputKind::Live | PipelineInputKind::SourceFilter { .. } => self.resync.push(input),
            PipelineInputKind::Playback => input
                .chunks_exact(TS_PACKET_SIZE)
                .map(|chunk| {
                    let mut packet = [0u8; TS_PACKET_SIZE];
                    packet.copy_from_slice(chunk);
                    packet
                })
                .collect(),
        }
    }

    pub fn push_ts_bytes(&mut self, input: &[u8], kind: PipelineInputKind) -> PipelineReport {
        let mut report = PipelineReport::default();
        let remainder = if matches!(kind, PipelineInputKind::Playback) { input.len() % TS_PACKET_SIZE } else { 0 };
        for packet in self.split_ts_bytes(input, kind) {
            let packet_report = self.push_ts_packet(&packet, kind);
            report.accepted_packets += packet_report.accepted_packets;
            report.dropped_packets += packet_report.dropped_packets;
            report.malformed_packets += packet_report.malformed_packets;
            report.drop_reasons.extend(packet_report.drop_reasons);
            report.delivery_actions.extend(packet_report.delivery_actions);
            report.generated_events.extend(packet_report.generated_events);
            report.diagnostics.extend(packet_report.diagnostics);
        }
        report.dropped_packets += remainder;
        if remainder > 0 {
            report.malformed_packets += remainder;
            report.drop_reasons.push(PipelineDropReason::ResidualBytes);
            report.diagnostics.push(PipelineDiagnostic { kind: PipelineDiagnosticKind::ResidualBytesDrop, pid: None });
        }
        report
    }

    pub fn reset_boundary(&mut self) -> PipelineResetReport {
        let residual = self.resync.drain_for_boundary();
        self.section_assemblers.clear();
        self.pes_assemblers.clear();
        self.section_assembler_generations.clear();
        self.pes_assembler_generations.clear();
        self.filter_section_flush_generations.clear();
        self.filter_pes_flush_generations.clear();
        self.continuity_trackers.clear();
        self.resync = PipelineResyncState::default();
        PipelineResetReport {
            cleared: true,
            residual_packets: residual.packets.len(),
            residual_malformed_bytes: residual.malformed_bytes,
        }
    }
}

#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct PipelineResetReport {
    pub cleared: bool,
    pub residual_packets: usize,
    pub residual_malformed_bytes: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validator_rejects_missing_sync_byte() {
        let mut packet = [0u8; TS_PACKET_SIZE];
        packet[0] = 0x00;
        assert_eq!(TsPacketView::validate(&packet).unwrap_err(), TsPacketValidationError::MissingSyncByte);
    }

    #[test]
    fn adaptation_only_packet_has_no_payload() {
        let mut packet = [0xffu8; TS_PACKET_SIZE];
        packet[0] = 0x47;
        packet[1] = 0x00;
        packet[2] = 0x20;
        packet[3] = 0x20;
        packet[4] = 183;
        let view = TsPacketView::validate(&packet).unwrap();
        assert!(view.payload.is_none());
    }

    #[test]
    fn discontinuity_indicator_is_exposed() {
        let mut packet = [0xffu8; TS_PACKET_SIZE];
        packet[0] = 0x47;
        packet[1] = 0x00;
        packet[2] = 0x20;
        packet[3] = 0x30;
        packet[4] = 1;
        packet[5] = 0x80;
        let view = TsPacketView::validate(&packet).unwrap();
        assert!(view.discontinuity_indicator);
    }

    fn adaptation_packet_with_flags(flags: u8, body: &[u8]) -> [u8; TS_PACKET_SIZE] {
        let mut packet = [0xffu8; TS_PACKET_SIZE];
        packet[0] = 0x47;
        packet[1] = 0x00;
        packet[2] = 0x20;
        packet[3] = 0x30;
        packet[4] = (1 + body.len()) as u8;
        packet[5] = flags;
        packet[6..6 + body.len()].copy_from_slice(body);
        packet
    }

    #[test]
    fn validator_rejects_short_pcr_field() {
        let packet = adaptation_packet_with_flags(0x10, &[0; 5]);
        assert_eq!(
            TsPacketView::validate(&packet).unwrap_err(),
            TsPacketValidationError::InvalidAdaptationLength
        );
    }

    #[test]
    fn validator_rejects_short_opcr_field() {
        let packet = adaptation_packet_with_flags(0x08, &[0; 5]);
        assert_eq!(
            TsPacketView::validate(&packet).unwrap_err(),
            TsPacketValidationError::InvalidAdaptationLength
        );
    }

    #[test]
    fn validator_rejects_missing_splice_countdown() {
        let packet = adaptation_packet_with_flags(0x04, &[]);
        assert_eq!(
            TsPacketView::validate(&packet).unwrap_err(),
            TsPacketValidationError::InvalidAdaptationLength
        );
    }

    #[test]
    fn validator_rejects_private_data_length_past_end() {
        let packet = adaptation_packet_with_flags(0x02, &[3, 0xaa]);
        assert_eq!(
            TsPacketView::validate(&packet).unwrap_err(),
            TsPacketValidationError::InvalidAdaptationLength
        );
    }

    #[test]
    fn validator_rejects_extension_length_past_end() {
        let packet = adaptation_packet_with_flags(0x01, &[2, 0xaa]);
        assert_eq!(
            TsPacketView::validate(&packet).unwrap_err(),
            TsPacketValidationError::InvalidAdaptationLength
        );
    }

    #[test]
    fn validator_accepts_valid_pcr_opcr_private_and_extension_fields() {
        let mut body = Vec::new();
        body.extend_from_slice(&[0, 0, 0, 0, 0x80, 0]); // PCR
        body.extend_from_slice(&[0, 0, 0, 0, 0x80, 0]); // OPCR
        body.push(0x7f); // splice_countdown
        body.extend_from_slice(&[2, 0xaa, 0xbb]); // private data
        body.extend_from_slice(&[1, 0xcc]); // extension
        let packet = adaptation_packet_with_flags(0x1f, &body);
        let view = TsPacketView::validate(&packet).unwrap();
        assert!(view.pcr_flag);
        assert!(view.opcr_flag);
        assert!(view.splicing_point_flag);
        assert!(view.private_data_flag);
        assert!(view.adaptation_extension_flag);
        assert_eq!(view.pcr_90khz, Some(1));
    }

    #[test]
    fn plan_report_keeps_av_payload_action_distinct_from_pes() {
        let mut packet = [0xffu8; TS_PACKET_SIZE];
        packet[0] = 0x47;
        packet[1] = 0x41;
        packet[2] = 0x00;
        packet[3] = 0x10;
        let view = TsPacketView::validate(&packet).unwrap();
        let filters = [
            PipelineFilterView { filter_id: 1, tpid: Some(0x0100), started: true, has_upstream: false, open_kind: PipelineOpenKind::Pes, section_raw: false, pes_raw: false, wants_record_index: false },
            PipelineFilterView { filter_id: 2, tpid: Some(0x0100), started: true, has_upstream: false, open_kind: PipelineOpenKind::Av, section_raw: false, pes_raw: false, wants_record_index: false },
        ];
        let report = PacketPipeline::default().plan_ts_packet_report(&view, crate::TsInputOrigin::Frontend, &filters);
        assert!(report.delivery_actions.contains(&PipelineDeliveryAction::PesPayload { filter_id: 1 }));
        assert!(report.delivery_actions.contains(&PipelineDeliveryAction::AvPayload { filter_id: 2 }));
    }

    #[test]
    fn plan_report_generates_callback_event_kinds_for_delivery_actions() {
        let mut packet = [0xffu8; TS_PACKET_SIZE];
        packet[0] = 0x47;
        packet[1] = 0x41;
        packet[2] = 0x00;
        packet[3] = 0x10;
        let view = TsPacketView::validate(&packet).unwrap();
        let filters = [
            PipelineFilterView { filter_id: 10, tpid: Some(0x0100), started: true, has_upstream: false, open_kind: PipelineOpenKind::Raw, section_raw: false, pes_raw: false, wants_record_index: false },
            PipelineFilterView { filter_id: 11, tpid: Some(0x0100), started: true, has_upstream: false, open_kind: PipelineOpenKind::Section, section_raw: false, pes_raw: false, wants_record_index: false },
            PipelineFilterView { filter_id: 12, tpid: Some(0x0100), started: true, has_upstream: false, open_kind: PipelineOpenKind::Pes, section_raw: false, pes_raw: false, wants_record_index: false },
        ];
        let report = PacketPipeline::default().plan_ts_packet_report(&view, crate::TsInputOrigin::Frontend, &filters);
        assert!(report.generated_events.contains(&PipelineGeneratedEvent::DataReady { filter_id: 10 }));
        assert!(report.generated_events.contains(&PipelineGeneratedEvent::Section { filter_id: 11, raw: false }));
        assert!(report.generated_events.contains(&PipelineGeneratedEvent::Pes { filter_id: 12, raw: false }));
    }

}

#[cfg(test)]
mod adaptation_payload_boundary_tests {
    use super::*;
    use maleicacid_tuner_hal2_common::TS_PACKET_SIZE;

    fn packet_with_discontinuity(pid: u16, cc: u8) -> [u8; TS_PACKET_SIZE] {
        let mut packet = [0xffu8; TS_PACKET_SIZE];
        packet[0] = 0x47;
        packet[1] = ((pid >> 8) as u8) & 0x1f;
        packet[2] = pid as u8;
        packet[3] = 0x30 | (cc & 0x0f);
        packet[4] = 1;
        packet[5] = 0x80;
        packet
    }

    #[test]
    fn discontinuity_resets_only_target_pid_section() {
        let origin = crate::TsInputOrigin::Frontend;
        let pid = 0x0100u16;
        let other_pid = 0x0101i32;
        let mut pipeline = PacketPipeline::default();
        pipeline.test_seed_section_for_pid(origin, pid as i32, 10);
        pipeline.test_seed_section_for_pid(origin, other_pid, 12);

        let packet = packet_with_discontinuity(pid, 0);
        let report = pipeline.push_ts_packet(&packet, PipelineInputKind::Live);

        assert_eq!(report.accepted_packets, 1);
        assert!(!pipeline.section_assemblers.contains_key(&(origin, pid as i32, 10)));
        assert!(pipeline.section_assemblers.contains_key(&(origin, other_pid, 12)));
    }

    #[test]
    fn discontinuity_resets_only_target_pid_pes() {
        let origin = crate::TsInputOrigin::Frontend;
        let pid = 0x0100u16;
        let other_pid = 0x0101i32;
        let mut pipeline = PacketPipeline::default();
        pipeline.test_seed_pes_for_pid(origin, pid as i32, 11);
        pipeline.test_seed_pes_for_pid(origin, other_pid, 13);

        let packet = packet_with_discontinuity(pid, 0);
        let report = pipeline.push_ts_packet(&packet, PipelineInputKind::Live);

        assert_eq!(report.accepted_packets, 1);
        assert!(!pipeline.pes_assemblers.contains_key(&(origin, pid as i32, 11)));
        assert!(pipeline.pes_assemblers.contains_key(&(origin, other_pid, 13)));
    }

    #[test]
    fn discontinuity_resets_only_target_pid_assemblers() {
        let origin = crate::TsInputOrigin::Frontend;
        let pid = 0x0100u16;
        let other_pid = 0x0101i32;
        let mut pipeline = PacketPipeline::default();
        pipeline.test_seed_section_for_pid(origin, pid as i32, 10);
        pipeline.test_seed_pes_for_pid(origin, pid as i32, 11);
        pipeline.test_seed_section_for_pid(origin, other_pid, 12);
        pipeline.test_seed_pes_for_pid(origin, other_pid, 13);

        let packet = packet_with_discontinuity(pid, 0);
        let report = pipeline.push_ts_packet(&packet, PipelineInputKind::Live);

        assert_eq!(report.accepted_packets, 1);
        assert!(!pipeline.section_assemblers.contains_key(&(origin, pid as i32, 10)));
        assert!(!pipeline.pes_assemblers.contains_key(&(origin, pid as i32, 11)));
        assert!(pipeline.section_assemblers.contains_key(&(origin, other_pid, 12)));
        assert!(pipeline.pes_assemblers.contains_key(&(origin, other_pid, 13)));
    }
}



#[cfg(test)]
mod continuity_duplicate_tests {
    use super::*;
    use maleicacid_tuner_hal2_common::TS_PACKET_SIZE;

    fn payload_packet(pid: u16, cc: u8) -> [u8; TS_PACKET_SIZE] {
        let mut packet = [0xffu8; TS_PACKET_SIZE];
        packet[0] = 0x47;
        packet[1] = ((pid >> 8) as u8) & 0x1f;
        packet[2] = pid as u8;
        packet[3] = 0x10 | (cc & 0x0f);
        packet[4] = 0xaa;
        packet
    }

    fn adaptation_only_packet(pid: u16, cc: u8) -> [u8; TS_PACKET_SIZE] {
        let mut packet = [0xffu8; TS_PACKET_SIZE];
        packet[0] = 0x47;
        packet[1] = ((pid >> 8) as u8) & 0x1f;
        packet[2] = pid as u8;
        packet[3] = 0x20 | (cc & 0x0f);
        packet[4] = 183;
        packet
    }

    #[test]
    fn source_filter_packets_use_pipeline_drop_and_continuity_rules() {
        let mut tei = payload_packet(0x0100, 0);
        tei[1] |= 0x80;
        let tei_report = PacketPipeline::default().push_ts_packet(&tei, PipelineInputKind::SourceFilter { source_filter_id: -1, source_filter_generation: 0 });
        assert_eq!(tei_report.accepted_packets, 0);
        assert_eq!(tei_report.dropped_packets, 1);
        assert!(tei_report.drop_reasons.contains(&PipelineDropReason::TransportErrorIndicator));

        let mut pipeline = PacketPipeline::default();
        let first = payload_packet(0x0100, 1);
        assert_eq!(pipeline.push_ts_packet(&first, PipelineInputKind::SourceFilter { source_filter_id: -1, source_filter_generation: 0 }).accepted_packets, 1);
        let duplicate = pipeline.push_ts_packet(&first, PipelineInputKind::SourceFilter { source_filter_id: -1, source_filter_generation: 0 });
        assert!(duplicate.drop_reasons.contains(&PipelineDropReason::DuplicatePacket));

        let no_payload = adaptation_only_packet(0x0100, 2);
        let no_payload_report = PacketPipeline::default().push_ts_packet(&no_payload, PipelineInputKind::SourceFilter { source_filter_id: -1, source_filter_generation: 0 });
        assert!(no_payload_report.drop_reasons.contains(&PipelineDropReason::NoPayload));
    }
}

#[cfg(test)]
mod malformed_adaptation_tests {
    use super::*;
    use maleicacid_tuner_hal2_common::TS_PACKET_SIZE;

    #[test]
    fn pipeline_report_carries_drop_reasons_and_diagnostics() {
        let wrong_len = [0u8; 3];
        let malformed = PacketPipeline::default().push_ts_packet(&wrong_len, PipelineInputKind::Live);
        assert_eq!(malformed.accepted_packets, 0);
        assert_eq!(malformed.dropped_packets, 1);
        assert_eq!(malformed.malformed_packets, 1);
        assert!(malformed.drop_reasons.contains(&PipelineDropReason::MalformedPacket));
        assert!(malformed.diagnostics.iter().any(|diag| diag.code == "malformed_ts_packet"));

        let mut tei = [0xffu8; TS_PACKET_SIZE];
        tei[0] = 0x47;
        tei[1] = 0x80;
        tei[2] = 0x10;
        tei[3] = 0x10;
        let report = PacketPipeline::default().push_ts_packet(&tei, PipelineInputKind::Live);
        assert!(report.drop_reasons.contains(&PipelineDropReason::TransportErrorIndicator));
        assert!(report.diagnostics.iter().any(|diag| diag.code == "tei_packet_drop" && diag.pid == Some(0x10)));
    }
}


#[cfg(test)]
mod discontinuity_generation_tests {
    use super::*;
    use maleicacid_tuner_hal2_common::TS_PACKET_SIZE;

    fn packet_with_payload(pid: u16, cc: u8, pusi: bool, payload: &[u8]) -> [u8; TS_PACKET_SIZE] {
        let mut packet = [0xffu8; TS_PACKET_SIZE];
        packet[0] = 0x47;
        packet[1] = ((pid >> 8) as u8) & 0x1f;
        if pusi {
            packet[1] |= 0x40;
        }
        packet[2] = pid as u8;
        let adaptation_len = TS_PACKET_SIZE - 5 - payload.len();
        packet[3] = 0x30 | (cc & 0x0f);
        packet[4] = adaptation_len as u8;
        let start = 5 + adaptation_len;
        packet[start..start + payload.len()].copy_from_slice(payload);
        packet
    }

    #[test]
    fn raw_section_split_across_ts_packets_emits_one_complete_section_event() {
        let pid = 0x0123u16;
        let filter = PipelineFilterView {
            filter_id: 17,
            tpid: Some(pid as i32),
            started: true,
            has_upstream: false,
            open_kind: PipelineOpenKind::Section,
            section_raw: true,
            pes_raw: false,
            wants_record_index: false,
        };
        let section = vec![0x7f, 0x30, 0x05, 0xaa, 0xbb, 0xcc, 0xdd, 0xee];
        let mut first_payload = vec![0x00];
        first_payload.extend_from_slice(&section[..4]);
        let first = packet_with_payload(pid, 0, true, &first_payload);
        let second = packet_with_payload(pid, 1, false, &section[4..]);

        let mut pipeline = PacketPipeline::default();
        let first_view = PacketPipeline::validate_packet(&first).unwrap();
        let first_report = pipeline.plan_and_assemble_ts_packet_report(
            &first_view,
            crate::TsInputOrigin::Frontend,
            &[filter],
        );
        assert!(!first_report.generated_events.iter().any(|event| matches!(event, PipelineGeneratedEvent::SectionPayloadReady { .. })));

        let second_view = PacketPipeline::validate_packet(&second).unwrap();
        let second_report = pipeline.plan_and_assemble_ts_packet_report(
            &second_view,
            crate::TsInputOrigin::Frontend,
            &[filter],
        );
        let ready = second_report
            .generated_events
            .iter()
            .filter_map(|event| match event {
                PipelineGeneratedEvent::SectionPayloadReady { filter_id, pid, bytes, .. } => {
                    Some((*filter_id, *pid, bytes.clone()))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(ready, vec![(17, pid as i32, section)]);
    }
}

#[cfg(test)]
mod resync_boundary_tests {
    use super::*;



    #[test]
    fn origin_aware_filter_prune_does_not_remove_other_origin_assemblers() {
        let mut pipeline = PacketPipeline::default();
        let frontend = crate::TsInputOrigin::Frontend;
        let source = crate::TsInputOrigin::SourceFilter { source_filter_id: 42, source_filter_generation: 3 };
        pipeline.test_seed_section_for_pid(frontend, 0x0100, 7);
        pipeline.test_seed_section_for_pid(source, 0x0100, 7);
        pipeline.test_seed_pes_for_pid(frontend, 0x0100, 8);
        pipeline.test_seed_pes_for_pid(source, 0x0100, 8);

        pipeline.remove_section_for_filter_ids_origin_pid(source, 0x0100, &[7]);
        pipeline.remove_pes_for_filter_ids_origin_pid(source, 0x0100, &[8]);

        assert!(pipeline.section_assemblers.contains_key(&(frontend, 0x0100, 7)));
        assert!(!pipeline.section_assemblers.contains_key(&(source, 0x0100, 7)));
        assert!(pipeline.pes_assemblers.contains_key(&(frontend, 0x0100, 8)));
        assert!(!pipeline.pes_assemblers.contains_key(&(source, 0x0100, 8)));
    }

    #[test]
    fn source_filter_origin_keeps_generation_and_assembler_state_separate_from_frontend() {
        let mut pipeline = PacketPipeline::default();
        let frontend = crate::TsInputOrigin::Frontend;
        let source = crate::TsInputOrigin::SourceFilter { source_filter_id: 42, source_filter_generation: 0 };
        pipeline.test_seed_section_for_pid(frontend, 0x0100, 7);
        pipeline.test_seed_section_for_pid(source, 0x0100, 7);
        pipeline.test_seed_pes_for_pid(frontend, 0x0100, 8);
        pipeline.test_seed_pes_for_pid(source, 0x0100, 8);
        assert_eq!(pipeline.bump_section_generation(frontend, 0x0100), Some(1));
        assert_eq!(pipeline.bump_section_generation(source, 0x0100), Some(1));

        pipeline.reset_assembly_for_origin_pid(source, 0x0100);

        assert!(pipeline.section_assemblers.contains_key(&(frontend, 0x0100, 7)));
        assert!(!pipeline.section_assemblers.contains_key(&(source, 0x0100, 7)));
        assert!(pipeline.pes_assemblers.contains_key(&(frontend, 0x0100, 8)));
        assert!(!pipeline.pes_assemblers.contains_key(&(source, 0x0100, 8)));
        assert_eq!(pipeline.current_section_generation(frontend, 0x0100), 1);
        assert_eq!(pipeline.current_section_generation(source, 0x0100), 0);
    }
}

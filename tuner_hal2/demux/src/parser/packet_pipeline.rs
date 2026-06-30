//! TS packet 検証と配送前の正規化を集約する入口。
//!
//! TEI / adaptation field / discontinuity / payload有無を1か所で決定する。

use crate::av::AvSharedBackingError;
use crate::config::ConfigInputPid;
use crate::runtime::DemuxRuntimeError;
use crate::ts_core::PesDropReason;
use maleicacid_tuner_hal2_common::{HalError, TransportStreamPid, TsPacketCompletionBuffer, TS_PACKET_SIZE};
use maleicacid_tuner_hal2_descrambler::DescramblerPid;
use std::collections::BTreeMap;

const PIPELINE_GENERATION_INITIAL: u64 = 0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TsPacketValidationError {
    WrongLength,
    MissingSyncByte,
    InvalidAdaptationControl,
    InvalidAdaptationLength,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TsPacketView<'a> {
    pid: i32,
    transport_error_indicator: bool,
    payload_unit_start: bool,
    priority: bool,
    scrambling_control: u8,
    continuity_counter: u8,
    discontinuity_indicator: bool,
    random_access_indicator: bool,
    pcr_flag: bool,
    opcr_flag: bool,
    splicing_point_flag: bool,
    private_data_flag: bool,
    adaptation_extension_flag: bool,
    payload: Option<&'a [u8]>,
}

impl<'a> TsPacketView<'a> {
    pub const fn transport_error_indicator(&self) -> bool { self.transport_error_indicator }
    pub const fn payload_unit_start(&self) -> bool { self.payload_unit_start }
    pub const fn priority(&self) -> bool { self.priority }
    pub const fn scrambling_control(&self) -> u8 { self.scrambling_control }
    pub const fn discontinuity_indicator(&self) -> bool { self.discontinuity_indicator }
    pub const fn random_access_indicator(&self) -> bool { self.random_access_indicator }
    pub const fn pcr_flag(&self) -> bool { self.pcr_flag }
    pub const fn opcr_flag(&self) -> bool { self.opcr_flag }
    pub const fn splicing_point_flag(&self) -> bool { self.splicing_point_flag }
    pub const fn private_data_flag(&self) -> bool { self.private_data_flag }
    pub const fn adaptation_extension_flag(&self) -> bool { self.adaptation_extension_flag }
    pub const fn payload(&self) -> Option<&'a [u8]> { self.payload }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct PacketPid(TransportStreamPid);

impl PacketPid {
    fn from_validated_pid(pid: i32) -> Self {
        Self(TransportStreamPid::validate_i32(pid).expect("validated TS packet PID"))
    }

    pub const fn to_i32_for_aidl_boundary(self) -> i32 {
        self.0.to_i32_for_aidl_boundary()
    }

    pub const fn from_descrambler_pid_for_service_runtime_boundary(pid: DescramblerPid) -> Self {
        Self(pid.to_transport_stream_pid_for_packet_pid_bridge())
    }

    pub(crate) fn from_config_pid(pid: ConfigInputPid) -> Self {
        Self(TransportStreamPid::validate_i32(pid.raw()).expect("validated filter config PID"))
    }

    pub(crate) const fn matches_config_tpid(self, tpid: Option<i32>) -> bool {
        self.0.matches_i32_config(tpid)
    }

    pub const fn matches_config_tpid_for_service_runtime_boundary(
        self,
        tpid: Option<i32>,
    ) -> bool {
        self.0.matches_i32_config(tpid)
    }
}


#[derive(Clone, Copy, Debug)]
pub struct ValidatedTsPacket<'a> {
    view: TsPacketView<'a>,
}

impl<'a> ValidatedTsPacket<'a> {
    pub fn validate(packet: &'a [u8]) -> Result<Self, TsPacketValidationError> {
        Ok(Self {
            view: TsPacketView::validate(packet)?,
        })
    }
    pub(crate) const fn view(&self) -> TsPacketView<'a> {
        self.view
    }
    pub fn pid(&self) -> PacketPid {
        PacketPid::from_validated_pid(self.view.pid)
    }

    pub const fn scrambling_control(&self) -> u8 {
        self.view.scrambling_control
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PacketDescramblePolicyFailure {
    NoKey,
    ScrambledPidNotRegistered,
    TransportErrorRecord,
    InvalidTsc,
    ScrambledNullPid,
    ScrambledWithoutPayload,
    BadToken,
    Multi2Fail,
    InvalidPacket,
}

impl<'a> TsPacketView<'a> {
    pub(crate) fn packet_pid(&self) -> PacketPid {
        PacketPid::from_validated_pid(self.pid)
    }
}

impl<'a> TsPacketView<'a> {
    pub(crate) fn validate(packet: &'a [u8]) -> Result<Self, TsPacketValidationError> {
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
        self.inner
            .push_payload_with_outcome(payload_unit_start, payload)
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
        pid: PacketPid,
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
        pid: PacketPid,
        continuity_counter: u8,
        has_payload: bool,
    ) -> crate::ts_core::ContinuityOutcome {
        self.inner.observe(pid, continuity_counter, has_payload)
    }

    pub fn reset_pid(&mut self, pid: PacketPid) {
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
    pub(crate) section_assemblers: BTreeMap<(crate::TsInputOrigin, PacketPid, i32), PipelineSectionState>,
    pub(crate) pes_assemblers: BTreeMap<(crate::TsInputOrigin, PacketPid, i32), PipelinePesState>,
    pub(crate) section_assembler_generations: BTreeMap<(crate::TsInputOrigin, PacketPid), u64>,
    pub(crate) pes_assembler_generations: BTreeMap<(crate::TsInputOrigin, PacketPid), u64>,
    pub(crate) filter_section_flush_generations: BTreeMap<(crate::TsInputOrigin, i32, PacketPid), u64>,
    pub(crate) filter_pes_flush_generations: BTreeMap<(crate::TsInputOrigin, i32, PacketPid), u64>,
    pub(crate) continuity_trackers: BTreeMap<crate::TsInputOrigin, PipelineContinuityState>,
    pub(crate) resync: PipelineResyncState,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum PipelineInputKind {
    Live,
    Playback,
    SourceFilter {
        source_filter_id: i32,
        source_filter_generation: u64,
    },
}

#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct PipelineReport {
    pub accepted_packets: usize,
    pub dropped_packets: usize,
    pub malformed_packets: usize,
    pub drop_reasons: Vec<PipelineDropReason>,
    pub assembly_suppression_reasons: Vec<PipelineAssemblySuppressionReason>,
    pub delivery_actions: Vec<PipelineDeliveryAction>,
    pub generated_events: Vec<PipelineGeneratedEvent>,
    pub diagnostics: Vec<PipelineDiagnostic>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum PipelineDropReason {
    MalformedPacket,
    AssemblyDrop,
    ResidualBytes,
    PesAssemblerOverflow,
    SectionGenerationOverflow,
    PesGenerationOverflow,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum PipelineAssemblySuppressionReason {
    TransportErrorIndicator,
    DuplicatePacket,
    NoPayload,
    KeylessScrambledWithoutDescrambler,
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
    AvMedia {
        filter_id: i32,
        descriptor: crate::av::AvMediaEventDescriptor,
    },
    DataReady {
        filter_id: i32,
    },
    Section {
        filter_id: i32,
        raw: bool,
    },
    Pes {
        filter_id: i32,
        raw: bool,
    },
    Record {
        filter_id: i32,
    },
    SectionPayloadReady {
        filter_id: i32,
        pid: PacketPid,
        generation: u64,
        bytes: Vec<u8>,
    },
    PesPacketReady {
        filter_id: i32,
        pid: PacketPid,
        generation: u64,
        packet: crate::ts_core::PesPacket,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PipelineDiagnostic {
    MalformedTsPacket,
    TeiAssemblySuppressed {
        pid: PacketPid,
    },
    DuplicatePacketAssemblySuppressed {
        pid: PacketPid,
    },
    NoPayloadAssemblySuppressed {
        pid: PacketPid,
    },
    KeylessScrambledAssemblySuppressed {
        pid: PacketPid,
    },
    SectionAssemblyDrop {
        pid: PacketPid,
    },
    SectionGenerationOverflow {
        pid: PacketPid,
    },
    PesGenerationOverflow {
        pid: PacketPid,
    },
    PesAssemblerDrop {
        pid: PacketPid,
        reason: PesDropReason,
    },
    ResidualBytesDrop,
    SourceFilterValidationFailure {
        pid: PacketPid,
        source_filter_id: i32,
        error: HalError,
    },
    SourceFilterDescramblePolicyFailure {
        pid: PacketPid,
        source_filter_id: i32,
        failure: PacketDescramblePolicyFailure,
    },
    RecordDvrMirrorFailure {
        pid: PacketPid,
        source_filter_id: i32,
        dvr_id: i32,
        error: DemuxRuntimeError,
    },
    FilterQueuePayloadDeliveryFailure {
        pid: PacketPid,
        filter_id: i32,
        error: DemuxRuntimeError,
    },
    AvSharedBackingFailure {
        pid: PacketPid,
        filter_id: i32,
        error: AvSharedBackingError,
    },
    AvSharedBackingMissing {
        pid: PacketPid,
        filter_id: i32,
    },
    AvSharedHandleNotExported {
        pid: PacketPid,
        filter_id: i32,
    },
    AvClientHandleReleased {
        pid: PacketPid,
        filter_id: i32,
    },
    AvPayloadOversized {
        pid: PacketPid,
        filter_id: i32,
    },
    AvNoFreeSlot {
        pid: PacketPid,
        filter_id: i32,
    },
    AvDataIdExhausted {
        pid: PacketPid,
        filter_id: i32,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PipelineDiagnosticPidContext {
    Present(PacketPid),
    NotApplicable,
}

impl PipelineDiagnosticPidContext {
    pub const fn to_i32_for_aidl_boundary(self) -> Option<i32> {
        match self {
            Self::Present(pid) => Some(pid.to_i32_for_aidl_boundary()),
            Self::NotApplicable => None,
        }
    }
}

impl PipelineDiagnostic {
    pub const fn pid_context(&self) -> PipelineDiagnosticPidContext {
        match self {
            Self::RecordDvrMirrorFailure { pid, .. }
            | Self::FilterQueuePayloadDeliveryFailure { pid, .. }
            | Self::AvSharedBackingFailure { pid, .. }
            | Self::AvSharedBackingMissing { pid, .. }
            | Self::AvSharedHandleNotExported { pid, .. }
            | Self::AvClientHandleReleased { pid, .. }
            | Self::AvPayloadOversized { pid, .. }
            | Self::AvNoFreeSlot { pid, .. }
            | Self::AvDataIdExhausted { pid, .. }
            | Self::TeiAssemblySuppressed { pid }
            | Self::DuplicatePacketAssemblySuppressed { pid }
            | Self::NoPayloadAssemblySuppressed { pid }
            | Self::KeylessScrambledAssemblySuppressed { pid }
            | Self::SectionAssemblyDrop { pid }
            | Self::SectionGenerationOverflow { pid }
            | Self::PesGenerationOverflow { pid }
            | Self::PesAssemblerDrop { pid, .. }
            | Self::SourceFilterValidationFailure { pid, .. }
            | Self::SourceFilterDescramblePolicyFailure { pid, .. } => {
                PipelineDiagnosticPidContext::Present(*pid)
            }
            Self::MalformedTsPacket | Self::ResidualBytesDrop => PipelineDiagnosticPidContext::NotApplicable,
        }
    }

    pub fn source_filter_validation_failure(
        pid: PacketPid,
        source_filter_id: i32,
        error: HalError,
    ) -> Self {
        Self::SourceFilterValidationFailure {
            pid,
            source_filter_id,
            error,
        }
    }

    pub fn source_filter_descramble_policy_failure(
        pid: PacketPid,
        source_filter_id: i32,
        failure: PacketDescramblePolicyFailure,
    ) -> Self {
        Self::SourceFilterDescramblePolicyFailure {
            pid,
            source_filter_id,
            failure,
        }
    }

    pub fn record_dvr_mirror_failure(
        pid: PacketPid,
        source_filter_id: i32,
        dvr_id: i32,
        error: DemuxRuntimeError,
    ) -> Self {
        Self::RecordDvrMirrorFailure {
            pid,
            source_filter_id,
            dvr_id,
            error,
        }
    }

    pub fn filter_queue_payload_delivery_failure(
        pid: PacketPid,
        filter_id: i32,
        error: DemuxRuntimeError,
    ) -> Self {
        Self::FilterQueuePayloadDeliveryFailure {
            pid,
            filter_id,
            error,
        }
    }

    pub fn av_shared_backing_failure(
        pid: PacketPid,
        filter_id: i32,
        error: AvSharedBackingError,
    ) -> Self {
        Self::AvSharedBackingFailure {
            pid,
            filter_id,
            error,
        }
    }

    pub fn av_shared_backing_missing(pid: PacketPid, filter_id: i32) -> Self {
        Self::AvSharedBackingMissing { pid, filter_id }
    }

    pub fn av_shared_handle_not_exported(pid: PacketPid, filter_id: i32) -> Self {
        Self::AvSharedHandleNotExported { pid, filter_id }
    }

    pub fn av_client_handle_released(pid: PacketPid, filter_id: i32) -> Self {
        Self::AvClientHandleReleased { pid, filter_id }
    }

    pub fn av_payload_oversized(pid: PacketPid, filter_id: i32) -> Self {
        Self::AvPayloadOversized { pid, filter_id }
    }

    pub fn av_no_free_slot(pid: PacketPid, filter_id: i32) -> Self {
        Self::AvNoFreeSlot { pid, filter_id }
    }

    pub fn av_data_id_exhausted(pid: PacketPid, filter_id: i32) -> Self {
        Self::AvDataIdExhausted { pid, filter_id }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum PipelineOpenKind {
    Raw,
    Record,
    Section,
    Pes,
    Av,
    Other,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct PipelineFilterView {
    pub filter_id: i32,
    pub tpid: Option<i32>,
    pub started: bool,
    pub source_filter: Option<(i32, u64)>,
    pub open_kind: PipelineOpenKind,
    pub section_raw: bool,
    pub pes_raw: bool,
    pub wants_record_index: bool,
}

impl PipelineFilterView {
    fn accepts_packet_pid_from_origin(self, pid: PacketPid, origin: crate::TsInputOrigin) -> bool {
        if !self.started || !pid.matches_config_tpid(self.tpid) {
            return false;
        }
        match origin {
            crate::TsInputOrigin::Frontend | crate::TsInputOrigin::Playback => {
                self.source_filter.is_none()
            }
            crate::TsInputOrigin::SourceFilter {
                source_filter_id,
                source_filter_generation,
            } => self.source_filter == Some((source_filter_id, source_filter_generation)),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct FilterPipelineConfig {
    pub tpid: Option<i32>,
    pub raw: bool,
}
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum PipelineError {
    InvalidState,
    InvalidPacket,
    Internal,
}
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum PipelineBoundaryReason {
    TuneStart,
    ScanStart,
    FrontendClose,
    FrontendUnbind,
    SourceFilterChange,
    DvrPlaybackDiscontinuity,
}

impl PacketPipeline {
    pub fn validate_packet(bytes: &[u8]) -> Result<ValidatedTsPacket<'_>, TsPacketValidationError> {
        ValidatedTsPacket::validate(bytes)
    }

    pub fn push_ts_packet(&mut self, packet: &[u8], kind: PipelineInputKind) -> PipelineReport {
        let validated = match Self::validate_packet(packet) {
            Ok(packet) => packet,
            Err(_) => {
                let mut report = PipelineReport::default();
                report.dropped_packets += 1;
                report.malformed_packets += 1;
                report
                    .drop_reasons
                    .push(PipelineDropReason::MalformedPacket);
                report
                    .diagnostics
                    .push(PipelineDiagnostic::MalformedTsPacket);
                return report;
            }
        };
        self.accept_validated_ts_packet(&validated, kind)
    }

    pub fn push_validated_ts_packet(
        &mut self,
        validated: &ValidatedTsPacket<'_>,
        kind: PipelineInputKind,
    ) -> PipelineReport {
        self.accept_validated_ts_packet(validated, kind)
    }

    fn accept_validated_ts_packet(
        &mut self,
        validated: &ValidatedTsPacket<'_>,
        kind: PipelineInputKind,
    ) -> PipelineReport {
        let mut report = PipelineReport::default();
        let origin = match kind {
            PipelineInputKind::Live => crate::TsInputOrigin::Frontend,
            PipelineInputKind::Playback => crate::TsInputOrigin::Playback,
            PipelineInputKind::SourceFilter {
                source_filter_id,
                source_filter_generation,
            } => crate::TsInputOrigin::SourceFilter {
                source_filter_id,
                source_filter_generation,
            },
        };
        let view = validated.view();
        let pid = validated.pid();
        if view.transport_error_indicator {
            report
                .assembly_suppression_reasons
                .push(PipelineAssemblySuppressionReason::TransportErrorIndicator);
            report
                .diagnostics
                .push(PipelineDiagnostic::TeiAssemblySuppressed { pid });
        }
        if view.discontinuity_indicator {
            self.reset_continuity_pid(origin, pid);
            self.reset_assembly_for_origin_pid(origin, pid);
        }
        let continuity = self.check_continuity(
            origin,
            pid,
            view.continuity_counter,
            view.payload.is_some(),
        );
        if matches!(continuity, crate::ts_core::ContinuityOutcome::Duplicate) {
            report
                .assembly_suppression_reasons
                .push(PipelineAssemblySuppressionReason::DuplicatePacket);
            report
                .diagnostics
                .push(PipelineDiagnostic::DuplicatePacketAssemblySuppressed { pid });
        }
        if matches!(continuity, crate::ts_core::ContinuityOutcome::Discontinuity) {
            self.reset_assembly_for_origin_pid(origin, pid);
        }
        if view.payload.is_none() {
            report
                .assembly_suppression_reasons
                .push(PipelineAssemblySuppressionReason::NoPayload);
            report
                .diagnostics
                .push(PipelineDiagnostic::NoPayloadAssemblySuppressed { pid });
        }
        report.accepted_packets += 1;
        report
    }

    pub fn inspect_ts_packet<'a>(&self, packet: &'a [u8]) -> Option<ValidatedTsPacket<'a>> {
        Self::validate_packet(packet).ok()
    }

    pub(crate) fn plan_packet_delivery(
        &self,
        pid: PacketPid,
        origin: crate::TsInputOrigin,
        filters: &[PipelineFilterView],
    ) -> Vec<PipelineDeliveryAction> {
        let mut actions = Vec::new();
        for filter in filters
            .iter()
            .copied()
            .filter(|filter| filter.accepts_packet_pid_from_origin(pid, origin))
        {
            match filter.open_kind {
                PipelineOpenKind::Raw => actions.push(PipelineDeliveryAction::RawPacket {
                    filter_id: filter.filter_id,
                }),
                PipelineOpenKind::Record => {
                    if origin.allows_record_mirror() {
                        actions.push(PipelineDeliveryAction::DvrMirror {
                            dvr_id: filter.filter_id,
                        });
                    }
                    if filter.wants_record_index {
                        actions.push(PipelineDeliveryAction::RecordPacket {
                            filter_id: filter.filter_id,
                        });
                    }
                }
                _ => {}
            }
        }
        actions
    }

    pub(crate) fn plan_section_filters(
        &self,
        pid: PacketPid,
        origin: crate::TsInputOrigin,
        filters: &[PipelineFilterView],
    ) -> Vec<i32> {
        filters
            .iter()
            .copied()
            .filter(|filter| {
                filter.accepts_packet_pid_from_origin(pid, origin)
                    && filter.open_kind == PipelineOpenKind::Section
            })
            .map(|filter| filter.filter_id)
            .collect()
    }

    pub(crate) fn plan_pes_actions(
        &self,
        pid: PacketPid,
        origin: crate::TsInputOrigin,
        filters: &[PipelineFilterView],
    ) -> Vec<PipelineDeliveryAction> {
        filters
            .iter()
            .copied()
            .filter(|filter| {
                filter.accepts_packet_pid_from_origin(pid, origin)
                    && matches!(
                        filter.open_kind,
                        PipelineOpenKind::Pes | PipelineOpenKind::Av
                    )
            })
            .map(|filter| match filter.open_kind {
                PipelineOpenKind::Av => PipelineDeliveryAction::AvPayload {
                    filter_id: filter.filter_id,
                },
                _ => PipelineDeliveryAction::PesPayload {
                    filter_id: filter.filter_id,
                },
            })
            .collect()
    }

    pub(crate) fn plan_ts_packet_report(
        &self,
        packet: &ValidatedTsPacket<'_>,
        origin: crate::TsInputOrigin,
        filters: &[PipelineFilterView],
    ) -> PipelineReport {
        let view = packet.view();
        let pid = packet.pid();
        let mut report = PipelineReport::default();
        report.accepted_packets = 1;
        report
            .delivery_actions
            .extend(self.plan_packet_delivery(pid, origin, filters));
        if view.payload.is_some() {
            let section_filter_ids = self.plan_section_filters(pid, origin, filters);
            let pes_actions = self.plan_pes_actions(pid, origin, filters);
            if view.transport_error_indicator {
                // TEI付きpacketはrecord/raw TSへは届かせるが、破損payloadを
                // section/PES/AV assembly へ入れない。TEI診断はpreflight側で出す。
            } else if view.scrambling_control != 0 {
                if !section_filter_ids.is_empty() || !pes_actions.is_empty() {
                    report.assembly_suppression_reasons.push(
                        PipelineAssemblySuppressionReason::KeylessScrambledWithoutDescrambler,
                    );
                    report
                        .diagnostics
                        .push(PipelineDiagnostic::KeylessScrambledAssemblySuppressed { pid });
                }
            } else {
                for filter_id in section_filter_ids {
                    report
                        .delivery_actions
                        .push(PipelineDeliveryAction::SectionPayload { filter_id });
                }
                report.delivery_actions.extend(pes_actions);
            }
        }
        for action in report.delivery_actions.iter() {
            match *action {
                PipelineDeliveryAction::RawPacket { filter_id } => {
                    report
                        .generated_events
                        .push(PipelineGeneratedEvent::DataReady { filter_id });
                }
                PipelineDeliveryAction::RecordPacket { filter_id } => {
                    report
                        .generated_events
                        .push(PipelineGeneratedEvent::Record { filter_id });
                }
                PipelineDeliveryAction::SectionPayload { filter_id } => {
                    report
                        .generated_events
                        .push(PipelineGeneratedEvent::Section {
                            filter_id,
                            raw: filters
                                .iter()
                                .find(|filter| filter.filter_id == filter_id)
                                .map(|filter| filter.section_raw)
                                .unwrap_or(false),
                        });
                }
                PipelineDeliveryAction::PesPayload { filter_id }
                | PipelineDeliveryAction::AvPayload { filter_id } => {
                    report.generated_events.push(PipelineGeneratedEvent::Pes {
                        filter_id,
                        raw: filters
                            .iter()
                            .find(|filter| filter.filter_id == filter_id)
                            .map(|filter| filter.pes_raw)
                            .unwrap_or(false),
                    });
                }
                PipelineDeliveryAction::DvrMirror { .. } => {}
            }
        }
        report
    }
    pub(crate) fn plan_and_assemble_ts_packet_report_after_preflight(
        &mut self,
        packet: &ValidatedTsPacket<'_>,
        origin: crate::TsInputOrigin,
        filters: &[PipelineFilterView],
        preflight_suppression_reasons: &[PipelineAssemblySuppressionReason],
    ) -> PipelineReport {
        let view = packet.view();
        let pid = packet.pid();
        let mut report = self.plan_ts_packet_report(packet, origin, filters);
        let preflight_tei = preflight_suppression_reasons.iter().any(|reason| {
            matches!(
                reason,
                PipelineAssemblySuppressionReason::TransportErrorIndicator
            )
        });
        let preflight_duplicate = preflight_suppression_reasons
            .iter()
            .any(|reason| matches!(reason, PipelineAssemblySuppressionReason::DuplicatePacket));
        if view.transport_error_indicator || preflight_tei {
            self.reset_assembly_for_origin_pid(origin, pid);
            return report;
        }
        if preflight_duplicate {
            report.delivery_actions.retain(|action| {
                matches!(
                    action,
                    PipelineDeliveryAction::RawPacket { .. }
                        | PipelineDeliveryAction::RecordPacket { .. }
                        | PipelineDeliveryAction::DvrMirror { .. }
                )
            });
            report.generated_events.retain(|event| {
                matches!(
                    event,
                    PipelineGeneratedEvent::DataReady { .. }
                        | PipelineGeneratedEvent::Record { .. }
                )
            });
            return report;
        }
        let Some(payload) = view.payload else {
            return report;
        };
        if view.scrambling_control != 0 {
            if report
                .assembly_suppression_reasons
                .contains(&PipelineAssemblySuppressionReason::KeylessScrambledWithoutDescrambler)
            {
                self.reset_assembly_for_origin_pid(origin, pid);
            }
            return report;
        }

        let has_section_action = report
            .delivery_actions
            .iter()
            .any(|action| matches!(action, PipelineDeliveryAction::SectionPayload { .. }));
        if has_section_action {
            let section_generation = if view.payload_unit_start {
                self.bump_section_generation(origin, pid)
            } else {
                Some(self.current_section_generation(origin, pid))
            };
            if let Some(section_generation) = section_generation {
                let section_filter_ids: Vec<i32> = report
                    .delivery_actions
                    .iter()
                    .filter_map(|action| match action {
                        PipelineDeliveryAction::SectionPayload { filter_id } => Some(*filter_id),
                        _ => None,
                    })
                    .collect();
                for filter_id in section_filter_ids {
                    let outcome = self.assemble_section_for_filter(
                        origin,
                        pid,
                        filter_id,
                        view.payload_unit_start,
                        payload,
                    );
                    if outcome.has_drop_or_discard() {
                        report.dropped_packets += 1;
                        report.drop_reasons.push(PipelineDropReason::AssemblyDrop);
                        report
                            .diagnostics
                            .push(PipelineDiagnostic::SectionAssemblyDrop { pid: pid });
                    }
                    for section in outcome.sections {
                        report
                            .generated_events
                            .push(PipelineGeneratedEvent::SectionPayloadReady {
                                filter_id,
                                pid,
                                generation: section_generation,
                                bytes: section,
                            });
                    }
                }
            } else {
                report.dropped_packets += 1;
                report
                    .drop_reasons
                    .push(PipelineDropReason::SectionGenerationOverflow);
                report
                    .diagnostics
                    .push(PipelineDiagnostic::SectionGenerationOverflow { pid: pid });
                self.reset_assembly_for_origin_pid(origin, pid);
            }
        } else {
            self.drop_generations_for_pid_origin(origin, pid, true, false);
        }

        let has_pes_action = report.delivery_actions.iter().any(|action| {
            matches!(
                action,
                PipelineDeliveryAction::PesPayload { .. }
                    | PipelineDeliveryAction::AvPayload { .. }
            )
        });
        if has_pes_action {
            let pes_generation = if view.payload_unit_start {
                self.bump_pes_generation(origin, pid)
            } else {
                Some(self.current_pes_generation(origin, pid))
            };
            let Some(pes_generation) = pes_generation else {
                report.dropped_packets += 1;
                report
                    .drop_reasons
                    .push(PipelineDropReason::PesGenerationOverflow);
                report
                    .diagnostics
                    .push(PipelineDiagnostic::PesGenerationOverflow { pid: pid });
                self.reset_assembly_for_origin_pid(origin, pid);
                return report;
            };
            let pes_filter_ids: Vec<i32> = report
                .delivery_actions
                .iter()
                .filter_map(|action| match action {
                    PipelineDeliveryAction::PesPayload { filter_id }
                    | PipelineDeliveryAction::AvPayload { filter_id } => Some(*filter_id),
                    _ => None,
                })
                .collect();
            for filter_id in pes_filter_ids {
                let packets = self.assemble_pes_for_filter(
                    origin,
                    filter_id,
                    pid,
                    view.payload_unit_start,
                    payload,
                );
                if let Some((reason, _generation)) = self
                    .pes_assemblers
                    .get_mut(&(origin, pid, filter_id))
                    .and_then(|state| state.take_drop_diagnostic())
                {
                    report.dropped_packets += 1;
                    report
                        .drop_reasons
                        .push(PipelineDropReason::PesAssemblerOverflow);
                    report
                        .diagnostics
                        .push(PipelineDiagnostic::PesAssemblerDrop {
                            pid,
                            reason,
                        });
                }
                for packet in packets {
                    report
                        .generated_events
                        .push(PipelineGeneratedEvent::PesPacketReady {
                            filter_id,
                            pid,
                            generation: pes_generation,
                            packet,
                        });
                }
            }
        } else {
            self.drop_generations_for_pid_origin(origin, pid, false, true);
        }

        report
    }

    pub fn configure_filter(
        &mut self,
        filter_id: i32,
        _config: FilterPipelineConfig,
    ) -> Result<(), PipelineError> {
        self.clear_filter_state(filter_id);
        Ok(())
    }
    pub fn start_filter(&mut self, _filter_id: i32) -> Result<(), PipelineError> {
        Ok(())
    }
    pub fn stop_filter(&mut self, _filter_id: i32) -> Result<(), PipelineError> {
        Ok(())
    }
    pub fn remove_filter(&mut self, filter_id: i32) -> Result<(), PipelineError> {
        self.clear_filter_state(filter_id);
        Ok(())
    }
    pub fn reset_boundary_for_reason(
        &mut self,
        _reason: PipelineBoundaryReason,
    ) -> Result<PipelineResetReport, PipelineError> {
        Ok(self.reset_boundary())
    }

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

    pub(crate) fn reset_assembly_for_origin_pid(&mut self, origin: crate::TsInputOrigin, pid: PacketPid) {
        // discontinuity は対象 PID の section/PES assembler だけを破棄する。
        // 無関係な PID の途中 section/PES を同一 origin というだけで破棄してはならない。
        self.section_assemblers
            .retain(|(stored_origin, stored_pid, _), _| {
                *stored_origin != origin || *stored_pid != pid
            });
        self.pes_assemblers
            .retain(|(stored_origin, stored_pid, _), _| {
                *stored_origin != origin || *stored_pid != pid
            });
        self.section_assembler_generations
            .retain(|(stored_origin, stored_pid), _| {
                *stored_origin != origin || *stored_pid != pid
            });
        self.pes_assembler_generations
            .retain(|(stored_origin, stored_pid), _| {
                *stored_origin != origin || *stored_pid != pid
            });
        self.filter_section_flush_generations
            .retain(|(stored_origin, _, stored_pid), _| {
                *stored_origin != origin || *stored_pid != pid
            });
        self.filter_pes_flush_generations
            .retain(|(stored_origin, _, stored_pid), _| {
                *stored_origin != origin || *stored_pid != pid
            });
    }

    pub(crate) fn reset_continuity_pid(&mut self, origin: crate::TsInputOrigin, pid: PacketPid) {
        self.continuity_trackers
            .entry(origin)
            .or_default()
            .reset_pid(pid);
    }

    fn check_continuity(
        &mut self,
        origin: crate::TsInputOrigin,
        pid: PacketPid,
        continuity_counter: u8,
        has_payload: bool,
    ) -> crate::ts_core::ContinuityOutcome {
        self.continuity_trackers.entry(origin).or_default().observe(
            pid,
            continuity_counter,
            has_payload,
        )
    }

    pub(crate) fn assemble_section_for_filter(
        &mut self,
        origin: crate::TsInputOrigin,
        pid: PacketPid,
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
        pid: PacketPid,
        payload_unit_start: bool,
        payload: &[u8],
    ) -> Vec<crate::ts_core::PesPacket> {
        self.pes_assemblers
            .entry((origin, pid, filter_id))
            .or_default()
            .push(pid, payload_unit_start, payload)
    }

    pub(crate) fn drop_generations_for_pid_origin(
        &mut self,
        origin: crate::TsInputOrigin,
        pid: PacketPid,
        section: bool,
        pes: bool,
    ) {
        if section {
            self.section_assembler_generations
                .retain(|(stored_origin, stored_pid), _| {
                    !(*stored_origin == origin && *stored_pid == pid)
                });
            self.filter_section_flush_generations
                .retain(|(stored_origin, _, stored_pid), _| {
                    !(*stored_origin == origin && *stored_pid == pid)
                });
        }
        if pes {
            self.pes_assembler_generations
                .retain(|(stored_origin, stored_pid), _| {
                    !(*stored_origin == origin && *stored_pid == pid)
                });
            self.filter_pes_flush_generations
                .retain(|(stored_origin, _, stored_pid), _| {
                    !(*stored_origin == origin && *stored_pid == pid)
                });
        }
    }

    pub(crate) fn current_section_generation(&self, origin: crate::TsInputOrigin, pid: PacketPid) -> u64 {
        self.section_assembler_generations
            .get(&(origin, pid))
            .copied()
            .unwrap_or(PIPELINE_GENERATION_INITIAL)
    }

    pub(crate) fn current_pes_generation(&self, origin: crate::TsInputOrigin, pid: PacketPid) -> u64 {
        self.pes_assembler_generations
            .get(&(origin, pid))
            .copied()
            .unwrap_or(PIPELINE_GENERATION_INITIAL)
    }

    pub(crate) fn bump_section_generation(
        &mut self,
        origin: crate::TsInputOrigin,
        pid: PacketPid,
    ) -> Option<u64> {
        let generation = self
            .section_assembler_generations
            .entry((origin, pid))
            .or_insert(0);
        let next = generation.checked_add(1)?;
        *generation = next;
        Some(next)
    }

    pub(crate) fn bump_pes_generation(
        &mut self,
        origin: crate::TsInputOrigin,
        pid: PacketPid,
    ) -> Option<u64> {
        let generation = self
            .pes_assembler_generations
            .entry((origin, pid))
            .or_insert(0);
        let next = generation.checked_add(1)?;
        *generation = next;
        Some(next)
    }

    fn current_section_generation_for_config_pid(
        &self,
        origin: crate::TsInputOrigin,
        pid: ConfigInputPid,
    ) -> u64 {
        let pid = PacketPid::from_config_pid(pid);
        self.section_assembler_generations
            .get(&(origin, pid))
            .copied()
            .unwrap_or(PIPELINE_GENERATION_INITIAL)
    }

    fn current_pes_generation_for_config_pid(
        &self,
        origin: crate::TsInputOrigin,
        pid: ConfigInputPid,
    ) -> u64 {
        let pid = PacketPid::from_config_pid(pid);
        self.pes_assembler_generations
            .get(&(origin, pid))
            .copied()
            .unwrap_or(PIPELINE_GENERATION_INITIAL)
    }

    fn mark_filter_flush_generation_for_origin(
        &mut self,
        filter_id: i32,
        pid: ConfigInputPid,
        origin: crate::TsInputOrigin,
    ) {
        let pipeline_pid = PacketPid::from_config_pid(pid);
        self.filter_section_flush_generations.insert(
            (origin, filter_id, pipeline_pid),
            self.current_section_generation_for_config_pid(origin, pid),
        );
        self.filter_pes_flush_generations.insert(
            (origin, filter_id, pipeline_pid),
            self.current_pes_generation_for_config_pid(origin, pid),
        );
    }

    pub(crate) fn flush_filter(
        &mut self,
        filter_id: i32,
        origins: &[(crate::TsInputOrigin, ConfigInputPid)],
    ) {
        for (origin, pid) in origins.iter().copied() {
            self.mark_filter_flush_generation_for_origin(filter_id, pid, origin);
        }
        self.clear_filter_state_after_flush(filter_id);
    }

    pub(crate) fn clear_filter_state_after_flush(&mut self, filter_id: i32) {
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

    pub fn split_ts_bytes(
        &mut self,
        input: &[u8],
        kind: PipelineInputKind,
    ) -> Vec<[u8; TS_PACKET_SIZE]> {
        match kind {
            PipelineInputKind::Live | PipelineInputKind::SourceFilter { .. } => {
                self.resync.push(input)
            }
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
mod test_support {
    use super::*;

    impl PacketPipeline {
        pub(crate) fn remove_section_for_filter_ids_origin_pid(
            &mut self,
            origin: crate::TsInputOrigin,
            pid: PacketPid,
            filter_ids: &[i32],
        ) {
            self.section_assemblers
                .retain(|(stored_origin, stored_pid, filter_id), _| {
                    !(*stored_origin == origin
                        && *stored_pid == pid
                        && filter_ids.iter().any(|id| id == filter_id))
                });
            self.filter_section_flush_generations.retain(
                |(stored_origin, filter_id, stored_pid), _| {
                    !(*stored_origin == origin
                        && *stored_pid == pid
                        && filter_ids.iter().any(|id| id == filter_id))
                },
            );
            self.section_assembler_generations
                .retain(|(stored_origin, stored_pid), _| {
                    !(*stored_origin == origin && *stored_pid == pid)
                });
        }

        pub(crate) fn remove_pes_for_filter_ids_origin_pid(
            &mut self,
            origin: crate::TsInputOrigin,
            pid: PacketPid,
            filter_ids: &[i32],
        ) {
            self.pes_assemblers
                .retain(|(stored_origin, stored_pid, filter_id), _| {
                    !(*stored_origin == origin
                        && *stored_pid == pid
                        && filter_ids.iter().any(|id| id == filter_id))
                });
            self.filter_pes_flush_generations.retain(
                |(stored_origin, filter_id, stored_pid), _| {
                    !(*stored_origin == origin
                        && *stored_pid == pid
                        && filter_ids.iter().any(|id| id == filter_id))
                },
            );
            self.pes_assembler_generations
                .retain(|(stored_origin, stored_pid), _| {
                    !(*stored_origin == origin && *stored_pid == pid)
                });
        }

        pub(crate) fn test_seed_section_for_pid(
            &mut self,
            origin: crate::TsInputOrigin,
            pid: PacketPid,
            filter_id: i32,
        ) {
            self.section_assemblers
                .entry((origin, pid, filter_id))
                .or_default();
        }

        pub(crate) fn test_seed_pes_for_pid(
            &mut self,
            origin: crate::TsInputOrigin,
            pid: PacketPid,
            filter_id: i32,
        ) {
            self.pes_assemblers
                .entry((origin, pid, filter_id))
                .or_default();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validator_rejects_missing_sync_byte() {
        let mut packet = [0u8; TS_PACKET_SIZE];
        packet[0] = 0x00;
        assert_eq!(
            TsPacketView::validate(&packet).unwrap_err(),
            TsPacketValidationError::MissingSyncByte
        );
    }

    #[test]
    fn validator_rejects_wrong_ts_packet_lengths() {
        let packet_187 = [0x47u8; TS_PACKET_SIZE - 1];
        let packet_189 = [0x47u8; TS_PACKET_SIZE + 1];
        assert_eq!(
            TsPacketView::validate(&packet_187).unwrap_err(),
            TsPacketValidationError::WrongLength
        );
        assert_eq!(
            TsPacketView::validate(&packet_189).unwrap_err(),
            TsPacketValidationError::WrongLength
        );
    }

    #[test]
    fn validator_rejects_reserved_adaptation_control() {
        let mut packet = [0xffu8; TS_PACKET_SIZE];
        packet[0] = 0x47;
        packet[1] = 0x00;
        packet[2] = 0x20;
        packet[3] = 0x00;
        assert_eq!(
            TsPacketView::validate(&packet).unwrap_err(),
            TsPacketValidationError::InvalidAdaptationControl
        );
    }

    #[test]
    fn validator_rejects_adaptation_length_past_packet_end() {
        let mut packet = [0xffu8; TS_PACKET_SIZE];
        packet[0] = 0x47;
        packet[1] = 0x00;
        packet[2] = 0x20;
        packet[3] = 0x30;
        packet[4] = 184;
        assert_eq!(
            TsPacketView::validate(&packet).unwrap_err(),
            TsPacketValidationError::InvalidAdaptationLength
        );
    }

    #[test]
    fn adaptation_only_packet_has_no_payload() {
        let mut packet = [0xffu8; TS_PACKET_SIZE];
        packet[0] = 0x47;
        packet[1] = 0x00;
        packet[2] = 0x20;
        packet[3] = 0x20;
        packet[4] = 183;
        packet[5] = 0x00;
        let packet = PacketPipeline::validate_packet(&packet).unwrap();
        assert!(packet.view().payload().is_none());
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
        let packet = PacketPipeline::validate_packet(&packet).unwrap();
        assert!(packet.view().discontinuity_indicator());
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
        let packet = PacketPipeline::validate_packet(&packet).unwrap();
        let view = packet.view();
        assert!(view.pcr_flag());
        assert!(view.opcr_flag());
        assert!(view.splicing_point_flag());
        assert!(view.private_data_flag());
        assert!(view.adaptation_extension_flag());
    }

    #[test]
    fn plan_report_keeps_av_payload_action_distinct_from_pes() {
        let mut packet = [0xffu8; TS_PACKET_SIZE];
        packet[0] = 0x47;
        packet[1] = 0x41;
        packet[2] = 0x00;
        packet[3] = 0x10;
        let view = PacketPipeline::validate_packet(&packet).unwrap();
        let filters = [
            PipelineFilterView {
                filter_id: 1,
                tpid: Some(0x0100),
                started: true,
                source_filter: None,
                open_kind: PipelineOpenKind::Pes,
                section_raw: false,
                pes_raw: false,
                wants_record_index: false,
            },
            PipelineFilterView {
                filter_id: 2,
                tpid: Some(0x0100),
                started: true,
                source_filter: None,
                open_kind: PipelineOpenKind::Av,
                section_raw: false,
                pes_raw: false,
                wants_record_index: false,
            },
        ];
        let report = PacketPipeline::default().plan_ts_packet_report(
            &view,
            crate::TsInputOrigin::Frontend,
            &filters,
        );
        assert!(report
            .delivery_actions
            .contains(&PipelineDeliveryAction::PesPayload { filter_id: 1 }));
        assert!(report
            .delivery_actions
            .contains(&PipelineDeliveryAction::AvPayload { filter_id: 2 }));
    }

    #[test]
    fn source_filter_origin_delivers_to_matching_record_sink() {
        let mut packet = [0xffu8; TS_PACKET_SIZE];
        packet[0] = 0x47;
        packet[1] = 0x41;
        packet[2] = 0x00;
        packet[3] = 0x10;
        let view = PacketPipeline::validate_packet(&packet).unwrap();
        let filters = [
            PipelineFilterView {
                filter_id: 20,
                tpid: Some(0x0100),
                started: true,
                source_filter: Some((10, 1)),
                open_kind: PipelineOpenKind::Record,
                section_raw: false,
                pes_raw: false,
                wants_record_index: true,
            },
            PipelineFilterView {
                filter_id: 21,
                tpid: Some(0x0100),
                started: true,
                source_filter: None,
                open_kind: PipelineOpenKind::Record,
                section_raw: false,
                pes_raw: false,
                wants_record_index: true,
            },
        ];
        let origin = crate::TsInputOrigin::SourceFilter {
            source_filter_id: 10,
            source_filter_generation: 1,
        };

        let report = PacketPipeline::default().plan_ts_packet_report(&view, origin, &filters);

        assert!(report
            .delivery_actions
            .contains(&PipelineDeliveryAction::RecordPacket { filter_id: 20 }));
        assert!(report
            .delivery_actions
            .contains(&PipelineDeliveryAction::DvrMirror { dvr_id: 20 }));
        assert!(!report
            .delivery_actions
            .contains(&PipelineDeliveryAction::RecordPacket { filter_id: 21 }));
    }

    #[test]
    fn plan_report_generates_callback_event_kinds_for_delivery_actions() {
        let mut packet = [0xffu8; TS_PACKET_SIZE];
        packet[0] = 0x47;
        packet[1] = 0x41;
        packet[2] = 0x00;
        packet[3] = 0x10;
        let view = PacketPipeline::validate_packet(&packet).unwrap();
        let filters = [
            PipelineFilterView {
                filter_id: 10,
                tpid: Some(0x0100),
                started: true,
                source_filter: None,
                open_kind: PipelineOpenKind::Raw,
                section_raw: false,
                pes_raw: false,
                wants_record_index: false,
            },
            PipelineFilterView {
                filter_id: 11,
                tpid: Some(0x0100),
                started: true,
                source_filter: None,
                open_kind: PipelineOpenKind::Section,
                section_raw: false,
                pes_raw: false,
                wants_record_index: false,
            },
            PipelineFilterView {
                filter_id: 12,
                tpid: Some(0x0100),
                started: true,
                source_filter: None,
                open_kind: PipelineOpenKind::Pes,
                section_raw: false,
                pes_raw: false,
                wants_record_index: false,
            },
        ];
        let report = PacketPipeline::default().plan_ts_packet_report(
            &view,
            crate::TsInputOrigin::Frontend,
            &filters,
        );
        assert!(report
            .generated_events
            .contains(&PipelineGeneratedEvent::DataReady { filter_id: 10 }));
        assert!(report
            .generated_events
            .contains(&PipelineGeneratedEvent::Section {
                filter_id: 11,
                raw: false
            }));
        assert!(report
            .generated_events
            .contains(&PipelineGeneratedEvent::Pes {
                filter_id: 12,
                raw: false
            }));
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
        let pid_key = PacketPid::from_validated_pid(pid as i32);
        let other_pid_key = PacketPid::from_validated_pid(other_pid);
        let mut pipeline = PacketPipeline::default();
        pipeline.test_seed_section_for_pid(origin, pid_key, 10);
        pipeline.test_seed_section_for_pid(origin, other_pid_key, 12);

        let packet = packet_with_discontinuity(pid, 0);
        let report = pipeline.push_ts_packet(&packet, PipelineInputKind::Live);

        assert_eq!(report.accepted_packets, 1);
        assert!(!pipeline
            .section_assemblers
            .contains_key(&(origin, pid_key, 10)));
        assert!(pipeline
            .section_assemblers
            .contains_key(&(origin, other_pid_key, 12)));
    }

    #[test]
    fn discontinuity_resets_only_target_pid_pes() {
        let origin = crate::TsInputOrigin::Frontend;
        let pid = 0x0100u16;
        let other_pid = 0x0101i32;
        let pid_key = PacketPid::from_validated_pid(pid as i32);
        let other_pid_key = PacketPid::from_validated_pid(other_pid);
        let mut pipeline = PacketPipeline::default();
        pipeline.test_seed_pes_for_pid(origin, pid_key, 11);
        pipeline.test_seed_pes_for_pid(origin, other_pid_key, 13);

        let packet = packet_with_discontinuity(pid, 0);
        let report = pipeline.push_ts_packet(&packet, PipelineInputKind::Live);

        assert_eq!(report.accepted_packets, 1);
        assert!(!pipeline
            .pes_assemblers
            .contains_key(&(origin, pid_key, 11)));
        assert!(pipeline
            .pes_assemblers
            .contains_key(&(origin, other_pid_key, 13)));
    }

    #[test]
    fn discontinuity_resets_only_target_pid_assemblers() {
        let origin = crate::TsInputOrigin::Frontend;
        let pid = 0x0100u16;
        let other_pid = 0x0101i32;
        let pid_key = PacketPid::from_validated_pid(pid as i32);
        let other_pid_key = PacketPid::from_validated_pid(other_pid);
        let mut pipeline = PacketPipeline::default();
        pipeline.test_seed_section_for_pid(origin, pid_key, 10);
        pipeline.test_seed_pes_for_pid(origin, pid_key, 11);
        pipeline.test_seed_section_for_pid(origin, other_pid_key, 12);
        pipeline.test_seed_pes_for_pid(origin, other_pid_key, 13);

        let packet = packet_with_discontinuity(pid, 0);
        let report = pipeline.push_ts_packet(&packet, PipelineInputKind::Live);

        assert_eq!(report.accepted_packets, 1);
        assert!(!pipeline
            .section_assemblers
            .contains_key(&(origin, pid_key, 10)));
        assert!(!pipeline
            .pes_assemblers
            .contains_key(&(origin, pid_key, 11)));
        assert!(pipeline
            .section_assemblers
            .contains_key(&(origin, other_pid_key, 12)));
        assert!(pipeline
            .pes_assemblers
            .contains_key(&(origin, other_pid_key, 13)));
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
        packet[5] = 0x00;
        packet
    }

    #[test]
    fn source_filter_packets_use_pipeline_drop_and_continuity_rules() {
        let mut tei = payload_packet(0x0100, 0);
        tei[1] |= 0x80;
        let tei_report = PacketPipeline::default().push_ts_packet(
            &tei,
            PipelineInputKind::SourceFilter {
                source_filter_id: -1,
                source_filter_generation: 0,
            },
        );
        assert_eq!(tei_report.accepted_packets, 1);
        assert_eq!(tei_report.dropped_packets, 0);
        assert!(tei_report
            .assembly_suppression_reasons
            .contains(&PipelineAssemblySuppressionReason::TransportErrorIndicator));

        let mut pipeline = PacketPipeline::default();
        let first = payload_packet(0x0100, 1);
        assert_eq!(
            pipeline
                .push_ts_packet(
                    &first,
                    PipelineInputKind::SourceFilter {
                        source_filter_id: -1,
                        source_filter_generation: 0
                    }
                )
                .accepted_packets,
            1
        );
        let duplicate = pipeline.push_ts_packet(
            &first,
            PipelineInputKind::SourceFilter {
                source_filter_id: -1,
                source_filter_generation: 0,
            },
        );
        assert!(duplicate
            .assembly_suppression_reasons
            .contains(&PipelineAssemblySuppressionReason::DuplicatePacket));

        let no_payload = adaptation_only_packet(0x0100, 2);
        let no_payload_report = PacketPipeline::default().push_ts_packet(
            &no_payload,
            PipelineInputKind::SourceFilter {
                source_filter_id: -1,
                source_filter_generation: 0,
            },
        );
        assert!(no_payload_report
            .assembly_suppression_reasons
            .contains(&PipelineAssemblySuppressionReason::NoPayload));
    }
}

#[cfg(test)]
mod malformed_adaptation_tests {
    use super::*;
    use maleicacid_tuner_hal2_common::TS_PACKET_SIZE;

    #[test]
    fn pipeline_report_carries_drop_reasons_and_diagnostics() {
        let wrong_len = [0u8; 3];
        let malformed =
            PacketPipeline::default().push_ts_packet(&wrong_len, PipelineInputKind::Live);
        assert_eq!(malformed.accepted_packets, 0);
        assert_eq!(malformed.dropped_packets, 1);
        assert_eq!(malformed.malformed_packets, 1);
        assert!(malformed
            .drop_reasons
            .contains(&PipelineDropReason::MalformedPacket));
        assert!(malformed
            .diagnostics
            .iter()
            .any(|diag| matches!(diag, PipelineDiagnostic::MalformedTsPacket)));

        let mut tei = [0xffu8; TS_PACKET_SIZE];
        tei[0] = 0x47;
        tei[1] = 0x80;
        tei[2] = 0x10;
        tei[3] = 0x10;
        let report = PacketPipeline::default().push_ts_packet(&tei, PipelineInputKind::Live);
        assert!(report
            .assembly_suppression_reasons
            .contains(&PipelineAssemblySuppressionReason::TransportErrorIndicator));
        assert!(report.diagnostics.iter().any(|diag| matches!(
            diag,
            PipelineDiagnostic::TeiAssemblySuppressed { pid } if pid.to_i32_for_aidl_boundary() == 0x10
        )));
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
        if adaptation_len > 0 {
            packet[5] = 0;
        }
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
            source_filter: None,
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
        let first_report = pipeline.plan_and_assemble_ts_packet_report_after_preflight(
            &first_view,
            crate::TsInputOrigin::Frontend,
            &[filter],
            &[],
        );
        assert!(!first_report
            .generated_events
            .iter()
            .any(|event| matches!(event, PipelineGeneratedEvent::SectionPayloadReady { .. })));

        let second_view = PacketPipeline::validate_packet(&second).unwrap();
        let second_report = pipeline.plan_and_assemble_ts_packet_report_after_preflight(
            &second_view,
            crate::TsInputOrigin::Frontend,
            &[filter],
            &[],
        );
        let ready = second_report
            .generated_events
            .iter()
            .filter_map(|event| match event {
                PipelineGeneratedEvent::SectionPayloadReady {
                    filter_id,
                    pid,
                    bytes,
                    ..
                } => Some((*filter_id, pid.to_i32_for_aidl_boundary(), bytes.clone())),
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
        let source = crate::TsInputOrigin::SourceFilter {
            source_filter_id: 42,
            source_filter_generation: 3,
        };
        pipeline.test_seed_section_for_pid(frontend, PacketPid::from_validated_pid(0x0100), 7);
        pipeline.test_seed_section_for_pid(source, PacketPid::from_validated_pid(0x0100), 7);
        pipeline.test_seed_pes_for_pid(frontend, PacketPid::from_validated_pid(0x0100), 8);
        pipeline.test_seed_pes_for_pid(source, PacketPid::from_validated_pid(0x0100), 8);

        pipeline.remove_section_for_filter_ids_origin_pid(source, PacketPid::from_validated_pid(0x0100), &[7]);
        pipeline.remove_pes_for_filter_ids_origin_pid(source, PacketPid::from_validated_pid(0x0100), &[8]);

        assert!(pipeline
            .section_assemblers
            .contains_key(&(frontend, PacketPid::from_validated_pid(0x0100), 7)));
        assert!(!pipeline
            .section_assemblers
            .contains_key(&(source, PacketPid::from_validated_pid(0x0100), 7)));
        assert!(pipeline.pes_assemblers.contains_key(&(frontend, PacketPid::from_validated_pid(0x0100), 8)));
        assert!(!pipeline.pes_assemblers.contains_key(&(source, PacketPid::from_validated_pid(0x0100), 8)));
    }

    #[test]
    fn source_filter_origin_keeps_generation_and_assembler_state_separate_from_frontend() {
        let mut pipeline = PacketPipeline::default();
        let frontend = crate::TsInputOrigin::Frontend;
        let source = crate::TsInputOrigin::SourceFilter {
            source_filter_id: 42,
            source_filter_generation: 0,
        };
        pipeline.test_seed_section_for_pid(frontend, PacketPid::from_validated_pid(0x0100), 7);
        pipeline.test_seed_section_for_pid(source, PacketPid::from_validated_pid(0x0100), 7);
        pipeline.test_seed_pes_for_pid(frontend, PacketPid::from_validated_pid(0x0100), 8);
        pipeline.test_seed_pes_for_pid(source, PacketPid::from_validated_pid(0x0100), 8);
        assert_eq!(pipeline.bump_section_generation(frontend, PacketPid::from_validated_pid(0x0100)), Some(1));
        assert_eq!(pipeline.bump_section_generation(source, PacketPid::from_validated_pid(0x0100)), Some(1));

        pipeline.reset_assembly_for_origin_pid(source, PacketPid::from_validated_pid(0x0100));

        assert!(pipeline
            .section_assemblers
            .contains_key(&(frontend, PacketPid::from_validated_pid(0x0100), 7)));
        assert!(!pipeline
            .section_assemblers
            .contains_key(&(source, PacketPid::from_validated_pid(0x0100), 7)));
        assert!(pipeline.pes_assemblers.contains_key(&(frontend, PacketPid::from_validated_pid(0x0100), 8)));
        assert!(!pipeline.pes_assemblers.contains_key(&(source, PacketPid::from_validated_pid(0x0100), 8)));
        assert_eq!(pipeline.current_section_generation(frontend, PacketPid::from_validated_pid(0x0100)), 1);
        assert_eq!(pipeline.current_section_generation(source, PacketPid::from_validated_pid(0x0100)), 0);
    }
}

#[cfg(test)]
mod record_raw_passthrough_policy_tests {
    use super::*;
    use maleicacid_tuner_hal2_common::TS_PACKET_SIZE;

    fn payload_packet(pid: u16, cc: u8) -> [u8; TS_PACKET_SIZE] {
        let mut packet = [0xffu8; TS_PACKET_SIZE];
        packet[0] = 0x47;
        packet[1] = 0x40 | (((pid >> 8) as u8) & 0x1f);
        packet[2] = pid as u8;
        packet[3] = 0x10 | (cc & 0x0f);
        packet[4] = 0x00;
        packet
    }

    fn record_filter(filter_id: i32) -> PipelineFilterView {
        PipelineFilterView {
            filter_id,
            tpid: Some(0x0100),
            started: true,
            source_filter: None,
            open_kind: PipelineOpenKind::Record,
            section_raw: false,
            pes_raw: false,
            wants_record_index: true,
        }
    }

    fn section_filter(filter_id: i32) -> PipelineFilterView {
        PipelineFilterView {
            filter_id,
            tpid: Some(0x0100),
            started: true,
            source_filter: None,
            open_kind: PipelineOpenKind::Section,
            section_raw: false,
            pes_raw: false,
            wants_record_index: false,
        }
    }

    fn raw_filter(filter_id: i32) -> PipelineFilterView {
        PipelineFilterView {
            filter_id,
            tpid: Some(0x0100),
            started: true,
            source_filter: None,
            open_kind: PipelineOpenKind::Raw,
            section_raw: false,
            pes_raw: false,
            wants_record_index: false,
        }
    }

    fn adaptation_only_packet(pid: u16, cc: u8) -> [u8; TS_PACKET_SIZE] {
        let mut packet = [0xffu8; TS_PACKET_SIZE];
        packet[0] = 0x47;
        packet[1] = ((pid >> 8) as u8) & 0x1f;
        packet[2] = pid as u8;
        packet[3] = 0x20 | (cc & 0x0f);
        packet[4] = (TS_PACKET_SIZE - 5) as u8;
        packet[5] = 0x00;
        packet
    }

    #[test]
    fn adaptation_only_packet_passes_raw_record_but_not_assembly_path() {
        let packet = adaptation_only_packet(0x0100, 0);
        let mut pipeline = PacketPipeline::default();
        let preflight = pipeline.push_ts_packet(&packet, PipelineInputKind::Live);
        assert_eq!(preflight.accepted_packets, 1);
        assert!(preflight
            .assembly_suppression_reasons
            .contains(&PipelineAssemblySuppressionReason::NoPayload));
        let validated = match pipeline.inspect_ts_packet(&packet) {
            Some(packet) => packet,
            None => return,
        };
        assert!(validated.view().payload().is_none());
        let report = pipeline.plan_and_assemble_ts_packet_report_after_preflight(
            &validated,
            crate::TsInputOrigin::Frontend,
            &[raw_filter(3), record_filter(1), section_filter(2)],
            &preflight.assembly_suppression_reasons,
        );

        assert!(report
            .delivery_actions
            .contains(&PipelineDeliveryAction::RawPacket { filter_id: 3 }));
        assert!(report
            .delivery_actions
            .contains(&PipelineDeliveryAction::RecordPacket { filter_id: 1 }));
        assert!(!report
            .delivery_actions
            .contains(&PipelineDeliveryAction::SectionPayload { filter_id: 2 }));
        assert!(!report.generated_events.iter().any(|event| {
            matches!(
                event,
                PipelineGeneratedEvent::SectionPayloadReady { .. }
                    | PipelineGeneratedEvent::PesPacketReady { .. }
            )
        }));
    }

    #[test]
    fn tei_packet_passes_record_but_not_assembly_path() {
        let mut packet = payload_packet(0x0100, 0);
        packet[1] |= 0x80;
        let mut pipeline = PacketPipeline::default();
        let preflight = pipeline.push_ts_packet(&packet, PipelineInputKind::Live);
        assert_eq!(preflight.accepted_packets, 1);
        assert!(preflight
            .assembly_suppression_reasons
            .contains(&PipelineAssemblySuppressionReason::TransportErrorIndicator));
        let validated = match pipeline.inspect_ts_packet(&packet) {
            Some(packet) => packet,
            None => return,
        };
        let report = pipeline.plan_and_assemble_ts_packet_report_after_preflight(
            &validated,
            crate::TsInputOrigin::Frontend,
            &[record_filter(1), section_filter(2)],
            &preflight.assembly_suppression_reasons,
        );

        assert!(report
            .delivery_actions
            .contains(&PipelineDeliveryAction::RecordPacket { filter_id: 1 }));
        assert!(!report
            .delivery_actions
            .contains(&PipelineDeliveryAction::SectionPayload { filter_id: 2 }));
    }

    #[test]
    fn duplicate_packet_passes_record_but_not_assembly_path() {
        let packet = payload_packet(0x0100, 0);
        let mut pipeline = PacketPipeline::default();
        assert_eq!(
            pipeline
                .push_ts_packet(&packet, PipelineInputKind::Live)
                .accepted_packets,
            1
        );
        let preflight = pipeline.push_ts_packet(&packet, PipelineInputKind::Live);
        assert_eq!(preflight.accepted_packets, 1);
        assert!(preflight
            .assembly_suppression_reasons
            .contains(&PipelineAssemblySuppressionReason::DuplicatePacket));
        let validated = match pipeline.inspect_ts_packet(&packet) {
            Some(packet) => packet,
            None => return,
        };
        let report = pipeline.plan_and_assemble_ts_packet_report_after_preflight(
            &validated,
            crate::TsInputOrigin::Frontend,
            &[record_filter(1), section_filter(2)],
            &preflight.assembly_suppression_reasons,
        );

        assert!(report
            .delivery_actions
            .contains(&PipelineDeliveryAction::RecordPacket { filter_id: 1 }));
        assert!(!report
            .delivery_actions
            .contains(&PipelineDeliveryAction::SectionPayload { filter_id: 2 }));
    }
}

#[cfg(test)]
mod keyless_scrambled_policy_tests {
    use super::*;
    use maleicacid_tuner_hal2_common::TS_PACKET_SIZE;

    fn payload_packet(pid: u16, scrambling_control: u8) -> [u8; TS_PACKET_SIZE] {
        let mut packet = [0xffu8; TS_PACKET_SIZE];
        packet[0] = 0x47;
        packet[1] = 0x40 | (((pid >> 8) as u8) & 0x1f);
        packet[2] = pid as u8;
        packet[3] = ((scrambling_control & 0x03) << 6) | 0x10;
        packet[4] = 0x00;
        packet
    }

    fn filter(filter_id: i32, open_kind: PipelineOpenKind) -> PipelineFilterView {
        PipelineFilterView {
            filter_id,
            tpid: Some(0x0100),
            started: true,
            source_filter: None,
            open_kind,
            section_raw: false,
            pes_raw: false,
            wants_record_index: matches!(open_kind, PipelineOpenKind::Record),
        }
    }

    #[test]
    fn keyless_scrambled_packet_passes_record_but_not_assembly_paths() {
        let packet = payload_packet(0x0100, 2);
        let parsed = PacketPipeline::validate_packet(&packet);
        assert!(parsed.is_ok());
        let validated = match parsed {
            Ok(packet) => packet,
            Err(_) => return,
        };
        let filters = [
            filter(1, PipelineOpenKind::Record),
            filter(2, PipelineOpenKind::Section),
            filter(3, PipelineOpenKind::Pes),
            filter(4, PipelineOpenKind::Av),
        ];

        let mut pipeline = PacketPipeline::default();
        let report = pipeline.plan_and_assemble_ts_packet_report_after_preflight(
            &validated,
            crate::TsInputOrigin::Frontend,
            &filters,
            &[],
        );

        assert!(report
            .delivery_actions
            .contains(&PipelineDeliveryAction::RecordPacket { filter_id: 1 }));
        assert!(!report
            .delivery_actions
            .contains(&PipelineDeliveryAction::SectionPayload { filter_id: 2 }));
        assert!(!report
            .delivery_actions
            .contains(&PipelineDeliveryAction::PesPayload { filter_id: 3 }));
        assert!(!report
            .delivery_actions
            .contains(&PipelineDeliveryAction::AvPayload { filter_id: 4 }));
        assert!(report
            .assembly_suppression_reasons
            .contains(&PipelineAssemblySuppressionReason::KeylessScrambledWithoutDescrambler));
        assert!(report.generated_events.iter().all(|event| !matches!(
            event,
            PipelineGeneratedEvent::SectionPayloadReady { .. }
                | PipelineGeneratedEvent::PesPacketReady { .. }
        )));
    }

    #[test]
    fn keyless_scrambled_packet_resets_partial_assembly_for_pid() {
        let packet = payload_packet(0x0100, 3);
        let parsed = PacketPipeline::validate_packet(&packet);
        assert!(parsed.is_ok());
        let validated = match parsed {
            Ok(packet) => packet,
            Err(_) => return,
        };
        let filters = [
            filter(2, PipelineOpenKind::Section),
            filter(3, PipelineOpenKind::Pes),
        ];
        let origin = crate::TsInputOrigin::Frontend;
        let mut pipeline = PacketPipeline::default();
        pipeline.test_seed_section_for_pid(origin, PacketPid::from_validated_pid(0x0100), 2);
        pipeline.test_seed_pes_for_pid(origin, PacketPid::from_validated_pid(0x0100), 3);

        let report = pipeline.plan_and_assemble_ts_packet_report_after_preflight(
            &validated,
            origin,
            &filters,
            &[],
        );

        assert!(report
            .assembly_suppression_reasons
            .contains(&PipelineAssemblySuppressionReason::KeylessScrambledWithoutDescrambler));
        assert!(!pipeline
            .section_assemblers
            .contains_key(&(origin, PacketPid::from_validated_pid(0x0100), 2)));
        assert!(!pipeline.pes_assemblers.contains_key(&(origin, PacketPid::from_validated_pid(0x0100), 3)));
    }
}


#[cfg(test)]
mod validated_packet_boundary_additional_tests {
    use super::*;
    use maleicacid_tuner_hal2_common::TS_PACKET_SIZE;

    fn payload_packet(pid: u16, cc: u8) -> [u8; TS_PACKET_SIZE] {
        let mut packet = [0xffu8; TS_PACKET_SIZE];
        packet[0] = 0x47;
        packet[1] = 0x40 | (((pid >> 8) as u8) & 0x1f);
        packet[2] = pid as u8;
        packet[3] = 0x10 | (cc & 0x0f);
        packet[4] = 0xaa;
        packet
    }

    #[test]
    fn validated_packet_exposes_packet_pid_only_after_validation() {
        let packet = payload_packet(0x0123, 0);
        let validated = ValidatedTsPacket::validate(&packet).unwrap();

        assert_eq!(validated.pid().to_i32_for_aidl_boundary(), 0x0123);
        assert_eq!(validated.view().packet_pid().to_i32_for_aidl_boundary(), 0x0123);
    }

    #[test]
    fn malformed_packet_does_not_produce_validated_packet_pid() {
        let mut packet = payload_packet(0x0123, 0);
        packet[0] = 0x00;

        let result = ValidatedTsPacket::validate(&packet);

        assert!(matches!(
            result,
            Err(TsPacketValidationError::MissingSyncByte)
        ));
    }

    #[test]
    fn duplicate_packet_diagnostic_carries_validated_packet_pid() {
        let packet = payload_packet(0x0124, 2);
        let mut pipeline = PacketPipeline::default();
        pipeline.push_ts_packet(&packet, PipelineInputKind::Live);

        let report = pipeline.push_ts_packet(&packet, PipelineInputKind::Live);

        assert!(report.diagnostics.iter().any(|diag| matches!(
            diag,
            PipelineDiagnostic::DuplicatePacketAssemblySuppressed { pid } if pid.to_i32_for_aidl_boundary() == 0x0124
        )));
    }
}

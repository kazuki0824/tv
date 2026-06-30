use super::{
    demux_runtime_error_to_hal, descramble_ts_packet_in_place,
    diagnostic_kind_for_descramble_failure, packet_policy_for_descramble_failure,
    ActiveDescramblerSnapshot, BTreeMap, BTreeSet, DemuxRuntimeId, DemuxStreamGeneration,
    DescrambleFailure, DescrambleOutcome, DescramblePacketDecision, DescramblePacketFlow,
    DescramblerDiagnosticKind, DescramblerDiagnosticRecord, FrontendRuntimeId,
    FrontendRuntimeState, GenerationBoundaryReport, GenerationBoundaryTxn, HalError,
    HalInternalKind, HalInvalidStateKind, PacketDescramblePolicyFailure, PacketPid,
    PacketPolicyAction, PipelineAssemblySuppressionReason, PipelineBoundaryReason,
    PipelineDiagnostic, PipelineReport, TsInputOrigin, TsPacketValidationError,
    TunerServiceRuntime, ValidatedTsPacket, TS_PACKET_SIZE,
};

fn descramble_failure_for_ts_validation_error(error: TsPacketValidationError) -> DescrambleFailure {
    match error {
        TsPacketValidationError::WrongLength => DescrambleFailure::InvalidPacketSize,
        TsPacketValidationError::MissingSyncByte => DescrambleFailure::BadSyncByte,
        TsPacketValidationError::InvalidAdaptationControl => DescrambleFailure::InvalidAfc,
        TsPacketValidationError::InvalidAdaptationLength => {
            DescrambleFailure::InvalidAdaptationField
        }
    }
}

fn packet_pid_from_descrambler_pid(pid: super::DescramblerPid) -> PacketPid {
    PacketPid::from_descrambler_pid_for_service_runtime_boundary(pid)
}

fn packet_descramble_policy_failure(failure: DescrambleFailure) -> PacketDescramblePolicyFailure {
    match failure {
        DescrambleFailure::NoKey => PacketDescramblePolicyFailure::NoKey,
        DescrambleFailure::ScrambledPidNotRegistered => {
            PacketDescramblePolicyFailure::ScrambledPidNotRegistered
        }
        DescrambleFailure::TransportErrorRecord => {
            PacketDescramblePolicyFailure::TransportErrorRecord
        }
        DescrambleFailure::InvalidTsc => PacketDescramblePolicyFailure::InvalidTsc,
        DescrambleFailure::ScrambledNullPid => PacketDescramblePolicyFailure::ScrambledNullPid,
        DescrambleFailure::ScrambledWithoutPayload => {
            PacketDescramblePolicyFailure::ScrambledWithoutPayload
        }
        DescrambleFailure::BadToken => PacketDescramblePolicyFailure::BadToken,
        DescrambleFailure::Multi2Fail => PacketDescramblePolicyFailure::Multi2Fail,
        DescrambleFailure::InvalidPacketSize
        | DescrambleFailure::BadSyncByte
        | DescrambleFailure::InvalidAfc
        | DescrambleFailure::InvalidAdaptationField => PacketDescramblePolicyFailure::InvalidPacket,
    }
}

impl TunerServiceRuntime {
    fn transact_set_demux_frontend_data_source(
        &mut self,
        demux_id: i32,
        frontend_id: i32,
    ) -> Result<GenerationBoundaryReport, HalError> {
        let demux_key = DemuxRuntimeId(demux_id);
        let frontend_key = FrontendRuntimeId(frontend_id);

        let Some(frontend_runtime) = self.registry.frontend_runtime(frontend_key) else {
            return Err(HalError::Unsupported(
                "frontend id is not available for demux source binding",
            ));
        };
        match frontend_runtime.state() {
            FrontendRuntimeState::Closing | FrontendRuntimeState::Failed => {
                return Err(HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "frontend runtime is closing or failed",
                ));
            }
            FrontendRuntimeState::Idle
            | FrontendRuntimeState::Tuning { .. }
            | FrontendRuntimeState::Scanning { .. } => {}
        }

        let Some(demux_runtime) = self.registry.demux_runtime_mut(demux_key) else {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "demux runtime is missing",
            ));
        };
        let generation = DemuxStreamGeneration(demux_runtime.generation());
        let (_, report) =
            GenerationBoundaryTxn::for_reason(generation, PipelineBoundaryReason::TuneStart)
                .apply(demux_runtime);
        let report = report.map_err(demux_runtime_error_to_hal)?;
        self.registry.bind_demux_frontend(demux_key, frontend_key);
        Ok(report)
    }

    fn transact_reset_bound_demuxes_for_frontend_tune_start(
        &mut self,
        frontend_id: i32,
    ) -> Result<Vec<GenerationBoundaryReport>, HalError> {
        let frontend_key = FrontendRuntimeId(frontend_id);
        if self.registry.frontend(frontend_key).is_none() {
            return Err(HalError::Unsupported(
                "frontend id is not available for tune boundary reset",
            ));
        }
        let demux_ids = self.registry.frontend_bound_demux_ids(frontend_key);
        let mut reports = Vec::with_capacity(demux_ids.len());
        for demux_id in demux_ids {
            let Some(demux_runtime) = self.registry.demux_runtime_mut(demux_id) else {
                return Err(HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "bound demux runtime is missing during tune boundary reset",
                ));
            };
            let generation = DemuxStreamGeneration(demux_runtime.generation());
            let (_, report) =
                GenerationBoundaryTxn::for_reason(generation, PipelineBoundaryReason::TuneStart)
                    .apply(demux_runtime);
            reports.push(report.map_err(demux_runtime_error_to_hal)?);
        }
        Ok(reports)
    }

    pub(super) fn transact_reset_and_unbind_bound_demuxes_for_frontend(
        &mut self,
        frontend_id: i32,
        reason: PipelineBoundaryReason,
    ) -> Result<Vec<GenerationBoundaryReport>, HalError> {
        let frontend_key = FrontendRuntimeId(frontend_id);
        if self.registry.frontend(frontend_key).is_none() {
            return Err(HalError::Unsupported(
                "frontend id is not available for demux unbind",
            ));
        }
        let demux_ids = self.registry.frontend_bound_demux_ids(frontend_key);
        let mut reports = Vec::with_capacity(demux_ids.len());
        for demux_id in &demux_ids {
            let Some(demux_runtime) = self.registry.demux_runtime_mut(*demux_id) else {
                return Err(HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "bound demux runtime is missing during frontend unbind",
                ));
            };
            let generation = DemuxStreamGeneration(demux_runtime.generation());
            let (_, report) =
                GenerationBoundaryTxn::for_reason(generation, reason).apply(demux_runtime);
            reports.push(report.map_err(demux_runtime_error_to_hal)?);
        }
        self.registry.unbind_frontend_demuxes(frontend_key);
        Ok(reports)
    }

    fn transact_quarantine_frontend_and_bound_demuxes(
        &mut self,
        frontend_id: i32,
        error: HalError,
    ) -> Result<Vec<DemuxRuntimeId>, HalError> {
        let frontend_key = FrontendRuntimeId(frontend_id);
        let demux_ids = self
            .registry
            .quarantine_bound_demuxes_for_frontend(frontend_key);
        let runtime = self
            .registry
            .frontend_runtime_mut(frontend_key)
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for quarantine",
                )
            })?;
        runtime.mark_failed(error);
        Ok(demux_ids)
    }

    fn transact_push_frontend_ts_packet_to_bound_demuxes(
        &mut self,
        frontend_id: i32,
        packet: &[u8],
    ) -> Result<Vec<PipelineReport>, HalError> {
        let demux_ids = self.query().ensure_frontend_demux_sink_ready(frontend_id)?;
        let mut reports = Vec::with_capacity(demux_ids.len());
        for demux_id in demux_ids {
            let packet_for_demux = match <&[u8; TS_PACKET_SIZE]>::try_from(packet) {
                Ok(packet) => {
                    let generation = self
                        .registry
                        .demux_runtime(demux_id)
                        .map(|runtime| runtime.generation())
                        .ok_or_else(|| {
                            HalError::invalid_state(
                                HalInvalidStateKind::InvalidLifecycle,
                                "bound demux runtime is missing",
                            )
                        })?;
                    let decision = self.decide_descrambled_packet(demux_id.0, generation, packet);
                    match decision.flow {
                        DescramblePacketFlow::Drop | DescramblePacketFlow::DiagnoseOnly => {
                            let mut report = PipelineReport::default();
                            report.diagnostics.extend(decision.diagnostics);
                            reports.push(report);
                            continue;
                        }
                        DescramblePacketFlow::Clear
                        | DescramblePacketFlow::Descrambled
                        | DescramblePacketFlow::RecordPassThroughAndDropAssembly => {
                            Some((decision.packet, decision.diagnostics))
                        }
                    }
                }
                Err(_) => None,
            };
            let packet = packet_for_demux
                .as_ref()
                .map_or(packet, |(packet, _)| &packet[..]);
            let pending_descrambler_diagnostics = packet_for_demux
                .as_ref()
                .map_or_else(Vec::new, |(_, diagnostics)| diagnostics.clone());
            let (demux_generation, mut report) = {
                let Some(demux_runtime) = self.registry.demux_runtime_mut(demux_id) else {
                    return Err(HalError::invalid_state(
                        HalInvalidStateKind::InvalidLifecycle,
                        "bound demux runtime is missing",
                    ));
                };
                let demux_generation = demux_runtime.generation();
                let report =
                    demux_runtime.push_ts_packet_from_origin(packet, TsInputOrigin::Frontend);
                (demux_generation, report)
            };
            report.diagnostics.extend(pending_descrambler_diagnostics);
            self.record_descrambler_packet_diagnostics(demux_id.0, demux_generation, &report);
            reports.push(report);
        }
        Ok(reports)
    }

    fn active_descrambler_snapshots_for_demux(
        &mut self,
        demux_id: i32,
        demux_generation: u64,
        packet_pid: PacketPid,
    ) -> (Vec<ActiveDescramblerSnapshot>, Vec<PipelineDiagnostic>) {
        let current_pid = packet_pid;
        let claim_sets = self
            .registry
            .resolved_descrambler_claims_for_demux(demux_id, demux_generation);
        let mut snapshots = Vec::with_capacity(claim_sets.len());
        let mut diagnostics = Vec::new();
        for claim_set in claim_sets {
            let claims = claim_set.claims;
            let mut descrambler_pids = BTreeSet::new();
            let mut packet_pids = BTreeSet::new();
            let mut source_filter_ids_by_pid: BTreeMap<PacketPid, BTreeSet<i32>> = BTreeMap::new();
            for claim in claims {
                let descrambler_pid = claim.pid();
                let pid = packet_pid_from_descrambler_pid(descrambler_pid);
                if pid != current_pid {
                    continue;
                }
                let Some(source) = claim.source_filter_ref() else {
                    descrambler_pids.insert(descrambler_pid);
                    packet_pids.insert(pid);
                    continue;
                };
                match self.validate_descrambler_source_filter(
                    demux_id,
                    demux_generation,
                    source.filter_id,
                    descrambler_pid,
                ) {
                    Ok(generation) if generation == source.generation => {
                        descrambler_pids.insert(descrambler_pid);
                    packet_pids.insert(pid);
                        source_filter_ids_by_pid
                            .entry(pid)
                            .or_default()
                            .insert(source.filter_id);
                    }
                    Ok(_) => {
                        let error = HalError::invalid_state(
                            HalInvalidStateKind::InvalidLifecycle,
                            "source filter generation changed before packet descramble",
                        );
                        diagnostics.push(PipelineDiagnostic::source_filter_validation_failure(
                            packet_pid,
                            source.filter_id,
                            error.clone(),
                        ));
                        self.record_descrambler_diagnostic(
                            DescramblerDiagnosticRecord::packet_source_filter_validation(
                                demux_id,
                                pid,
                                source.filter_id,
                                DescramblerDiagnosticKind::PacketSourceFilterGenerationMismatch,
                                error,
                            ),
                        );
                    }
                    Err(error) => {
                        diagnostics.push(PipelineDiagnostic::source_filter_validation_failure(
                            packet_pid,
                            source.filter_id,
                            error.clone(),
                        ));
                        self.record_descrambler_diagnostic(
                            DescramblerDiagnosticRecord::packet_source_filter_validation(
                                demux_id,
                                pid,
                                source.filter_id,
                                DescramblerDiagnosticKind::PacketSourceFilterInvalid,
                                error,
                            ),
                        );
                    }
                }
            }
            if packet_pids.is_empty() {
                continue;
            }
            snapshots.push(ActiveDescramblerSnapshot {
                descrambler_pids,
                packet_pids,
                key_slot: claim_set.key_slot,
                source_filter_ids_by_pid,
            });
        }
        (snapshots, diagnostics)
    }

    fn record_descramble_failure_policy(
        &mut self,
        demux_id: i32,
        pid: PacketPid,
        failure: DescrambleFailure,
    ) -> DescramblePacketFlow {
        self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::packet_policy(
            demux_id,
            pid,
            diagnostic_kind_for_descramble_failure(failure),
        ));
        match packet_policy_for_descramble_failure(failure) {
            PacketPolicyAction::RecordPassThroughAndDropAssembly => {
                self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::packet_policy(
                    demux_id,
                    pid,
                    DescramblerDiagnosticKind::PacketAssemblySuppressed,
                ));
                DescramblePacketFlow::RecordPassThroughAndDropAssembly
            }
            PacketPolicyAction::DropAndDiagnose => DescramblePacketFlow::Drop,
            PacketPolicyAction::DiagnoseOnly => DescramblePacketFlow::DiagnoseOnly,
        }
    }

    fn record_descramble_failure_policy_without_pid(
        &mut self,
        demux_id: i32,
        failure: DescrambleFailure,
    ) -> DescramblePacketFlow {
        self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::packet_policy_without_pid(
            demux_id,
            diagnostic_kind_for_descramble_failure(failure),
        ));
        match packet_policy_for_descramble_failure(failure) {
            PacketPolicyAction::RecordPassThroughAndDropAssembly => {
                self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::packet_policy_without_pid(
                    demux_id,
                    DescramblerDiagnosticKind::PacketAssemblySuppressed,
                ));
                DescramblePacketFlow::RecordPassThroughAndDropAssembly
            }
            PacketPolicyAction::DropAndDiagnose => DescramblePacketFlow::Drop,
            PacketPolicyAction::DiagnoseOnly => DescramblePacketFlow::DiagnoseOnly,
        }
    }

    fn source_filter_descramble_policy_diagnostics(
        snapshots: &[ActiveDescramblerSnapshot],
        packet_pid: PacketPid,
        failure: DescrambleFailure,
    ) -> Vec<PipelineDiagnostic> {
        snapshots
            .iter()
            .filter_map(|snapshot| snapshot.source_filter_ids_for_packet_pid(packet_pid))
            .flat_map(|ids| ids.iter().copied())
            .map(|filter_id| {
                PipelineDiagnostic::source_filter_descramble_policy_failure(
                    packet_pid,
                    filter_id,
                    packet_descramble_policy_failure(failure),
                )
            })
            .collect()
    }

    fn decide_descrambled_packet(
        &mut self,
        demux_id: i32,
        demux_generation: u64,
        packet: &[u8; TS_PACKET_SIZE],
    ) -> DescramblePacketDecision {
        let validated_packet = match ValidatedTsPacket::validate(packet) {
            Ok(packet) => packet,
            Err(failure) => {
                let flow = self.record_descramble_failure_policy_without_pid(
                    demux_id,
                    descramble_failure_for_ts_validation_error(failure),
                );
                return DescramblePacketDecision {
                    packet: *packet,
                    flow,
                    diagnostics: Vec::new(),
                };
            }
        };
        let packet_pid = validated_packet.pid();
        if validated_packet.scrambling_control() == 0 {
            return DescramblePacketDecision {
                packet: *packet,
                flow: DescramblePacketFlow::Clear,
                diagnostics: Vec::new(),
            };
        }

        let (snapshots, validation_diagnostics) =
            self.active_descrambler_snapshots_for_demux(demux_id, demux_generation, packet_pid);
        let mut saw_target_descrambler = false;
        for snapshot in snapshots
            .iter()
            .filter(|snapshot| snapshot.targets_packet_pid(packet_pid))
        {
            saw_target_descrambler = true;
            let Some(key_slot) = snapshot.key_slot.as_ref() else {
                continue;
            };
            let mut candidate = *packet;
            match descramble_ts_packet_in_place(&mut candidate, &snapshot.descrambler_pids, key_slot) {
                Ok(DescrambleOutcome::Descrambled { .. }) => {
                    self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::packet_policy(
                        demux_id,
                        packet_pid,
                        DescramblerDiagnosticKind::PacketDescrambled,
                    ));
                    return DescramblePacketDecision {
                        packet: candidate,
                        flow: DescramblePacketFlow::Descrambled,
                        diagnostics: validation_diagnostics.clone(),
                    };
                }
                Ok(DescrambleOutcome::PassedThrough { .. }) => {
                    return DescramblePacketDecision {
                        packet: *packet,
                        flow: DescramblePacketFlow::Clear,
                        diagnostics: validation_diagnostics.clone(),
                    };
                }
                Err(failure) => {
                    let flow = self.record_descramble_failure_policy(demux_id, packet_pid, failure);
                    let mut diagnostics = validation_diagnostics.clone();
                    diagnostics.extend(Self::source_filter_descramble_policy_diagnostics(
                        &snapshots, packet_pid, failure,
                    ));
                    return DescramblePacketDecision {
                        packet: *packet,
                        flow,
                        diagnostics,
                    };
                }
            }
        }

        let failure = if saw_target_descrambler {
            DescrambleFailure::NoKey
        } else {
            DescrambleFailure::ScrambledPidNotRegistered
        };
        let flow = self.record_descramble_failure_policy(demux_id, packet_pid, failure);
        let mut diagnostics = validation_diagnostics;
        diagnostics.extend(Self::source_filter_descramble_policy_diagnostics(
            &snapshots, packet_pid, failure,
        ));
        DescramblePacketDecision {
            packet: *packet,
            flow,
            diagnostics,
        }
    }

    fn record_descrambler_packet_diagnostics(
        &mut self,
        demux_id: i32,
        demux_generation: u64,
        report: &PipelineReport,
    ) {
        let keyless_suppressed = report
            .assembly_suppression_reasons
            .contains(&PipelineAssemblySuppressionReason::KeylessScrambledWithoutDescrambler);
        if !keyless_suppressed {
            return;
        }
        let pids = report.diagnostics.iter().filter_map(|diagnostic| {
            let pid = match diagnostic {
                PipelineDiagnostic::KeylessScrambledAssemblySuppressed { pid } => *pid,
                _ => return None,
            };
            Some(pid)
        });
        for pid in pids {
            let keyless_claim = self
                .registry
                .descrambler_keyless_claim_exists_for_demux_packet_pid(demux_id, demux_generation, pid);
            if keyless_claim {
                self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::packet_policy(
                    demux_id,
                    pid,
                    DescramblerDiagnosticKind::PacketScrambledWithoutKey,
                ));
            }
            self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::packet_policy(
                demux_id,
                pid,
                DescramblerDiagnosticKind::PacketAssemblySuppressed,
            ));
        }
    }
}

pub(crate) struct PacketTxn<'a> {
    runtime: &'a mut TunerServiceRuntime,
}

impl TunerServiceRuntime {
    pub(crate) fn packet_txn(&mut self) -> PacketTxn<'_> {
        PacketTxn { runtime: self }
    }
}

impl<'a> PacketTxn<'a> {
    pub(crate) fn set_demux_frontend_data_source(
        &mut self,
        demux_id: i32,
        frontend_id: i32,
    ) -> Result<GenerationBoundaryReport, HalError> {
        self.runtime
            .transact_set_demux_frontend_data_source(demux_id, frontend_id)
    }

    pub(crate) fn reset_bound_demuxes_for_frontend_tune_start(
        &mut self,
        frontend_id: i32,
    ) -> Result<Vec<GenerationBoundaryReport>, HalError> {
        self.runtime
            .transact_reset_bound_demuxes_for_frontend_tune_start(frontend_id)
    }

    pub(crate) fn reset_and_unbind_bound_demuxes_for_frontend(
        &mut self,
        frontend_id: i32,
        reason: PipelineBoundaryReason,
    ) -> Result<Vec<GenerationBoundaryReport>, HalError> {
        self.runtime
            .transact_reset_and_unbind_bound_demuxes_for_frontend(frontend_id, reason)
    }

    pub(crate) fn quarantine_frontend_and_bound_demuxes(
        &mut self,
        frontend_id: i32,
        error: HalError,
    ) -> Result<Vec<DemuxRuntimeId>, HalError> {
        self.runtime
            .transact_quarantine_frontend_and_bound_demuxes(frontend_id, error)
    }

    pub(crate) fn push_frontend_ts_packet_to_bound_demuxes(
        &mut self,
        frontend_id: i32,
        packet: &[u8],
    ) -> Result<Vec<PipelineReport>, HalError> {
        self.runtime
            .transact_push_frontend_ts_packet_to_bound_demuxes(frontend_id, packet)
    }
}

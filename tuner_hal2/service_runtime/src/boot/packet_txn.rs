use super::{
    BTreeSet, DemuxRuntimeId, DemuxStreamGeneration, DescrambleFailure, DescrambleOutcome,
    DescramblerDiagnosticKind, DescramblerDiagnosticRecord, FrontendRuntimeId,
    FrontendRuntimeState, GenerationBoundaryReport, GenerationBoundaryTxn, HalError,
    HalInternalKind, HalInvalidStateKind, PacketPolicyAction,
    PipelineAssemblySuppressionReason, PipelineBoundaryReason, PipelineDiagnosticKind,
    PipelineReport, TS_PACKET_SIZE, TsInputOrigin, TunerServiceRuntime,
    descramble_ts_packet_in_place, packet_policy_for_descramble_failure,
    parse_ts_packet_header,
};

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

    fn transact_reset_and_unbind_bound_demuxes_for_frontend(
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
                            reports.push(PipelineReport::default());
                            continue;
                        }
                        DescramblePacketFlow::Clear
                        | DescramblePacketFlow::Descrambled
                        | DescramblePacketFlow::RecordPassThroughAndDropAssembly => {
                            Some(decision.packet)
                        }
                    }
                }
                Err(_) => None
};
            let packet = packet_for_demux
                .as_ref()
                .map_or(packet, |packet| &packet[..]);
            let (demux_generation, report) = {
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
            self.record_descrambler_packet_diagnostics(demux_id.0, demux_generation, &report);
            reports.push(report);
        }
        Ok(reports)
    }

    fn active_descrambler_snapshots_for_demux(
        &self,
        demux_id: i32,
        demux_generation: u64,
    ) -> Vec<ActiveDescramblerSnapshot> {
        self.registry
            .descrambler_claims_for_demux(demux_id, demux_generation)
            .into_iter()
            .filter_map(|(claims, key_slot_id)| {
                let pids: BTreeSet<u16> = claims
                    .into_iter()
                    .filter_map(|claim| {
                        let source = claim.source_filter();
                        self.validate_descrambler_source_filter(
                            demux_id,
                            demux_generation,
                            source.filter_id,
                            claim.pid().0,
                        )
                        .ok()
                        .filter(|generation| *generation == source.generation)
                        .map(|_| claim.pid().0)
                    })
                    .collect();
                if pids.is_empty() {
                    return None;
                }
                let key_slot = key_slot_id
                    .and_then(|slot_id| self.registry.descrambler_key_table().key_slot(slot_id));
                Some(ActiveDescramblerSnapshot { pids, key_slot })
            })
            .collect()
    }

    fn record_descramble_failure_policy(
        &mut self,
        demux_id: i32,
        pid: u16,
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

    fn decide_descrambled_packet(
        &mut self,
        demux_id: i32,
        demux_generation: u64,
        packet: &[u8; TS_PACKET_SIZE],
    ) -> DescramblePacketDecision {
        let header = match parse_ts_packet_header(packet) {
            Ok(header) => header,
            Err(failure) => {
                let flow = self.record_descramble_failure_policy(demux_id, 0, failure);
                return DescramblePacketDecision {
                    packet: *packet,
                    flow
};
            }
        };
        let pid = header.pid;
        if header.transport_scrambling_control == 0 {
            return DescramblePacketDecision {
                packet: *packet,
                flow: DescramblePacketFlow::Clear
};
        }

        let snapshots = self.active_descrambler_snapshots_for_demux(demux_id, demux_generation);
        let mut saw_target_descrambler = false;
        for snapshot in snapshots
            .iter()
            .filter(|snapshot| snapshot.targets_pid(pid))
        {
            saw_target_descrambler = true;
            let Some(key_slot) = snapshot.key_slot.as_ref() else {
                continue;
            };
            let mut candidate = *packet;
            match descramble_ts_packet_in_place(&mut candidate, &snapshot.pids, key_slot) {
                Ok(DescrambleOutcome::Descrambled { .. }) => {
                    self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::packet_policy(
                        demux_id,
                        pid,
                        DescramblerDiagnosticKind::PacketDescrambled,
                    ));
                    return DescramblePacketDecision {
                        packet: candidate,
                        flow: DescramblePacketFlow::Descrambled
};
                }
                Ok(DescrambleOutcome::PassedThrough { .. }) => {
                    return DescramblePacketDecision {
                        packet: *packet,
                        flow: DescramblePacketFlow::Clear
};
                }
                Err(failure) => {
                    let flow = self.record_descramble_failure_policy(demux_id, pid, failure);
                    return DescramblePacketDecision {
                        packet: *packet,
                        flow
};
                }
            }
        }

        let failure = if saw_target_descrambler {
            DescrambleFailure::NoKey
        } else {
            DescrambleFailure::ScrambledPidNotRegistered
        };
        let flow = self.record_descramble_failure_policy(demux_id, pid, failure);
        DescramblePacketDecision {
            packet: *packet,
            flow,
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
            if diagnostic.kind != PipelineDiagnosticKind::KeylessScrambledAssemblySuppressed {
                return None;
            }
            diagnostic.pid.and_then(|pid| u16::try_from(pid).ok())
        });
        for pid in pids {
            let keyless_claim = self
                .registry
                .descrambler_key_slot_for_demux_pid(demux_id, demux_generation, pid)
                .is_some_and(|key_slot| key_slot.is_none());
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

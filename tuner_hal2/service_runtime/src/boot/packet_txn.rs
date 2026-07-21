use super::{
    demux_runtime_error_to_hal, DemuxRuntimeId, DescrambleFailure, DescramblePacketDecision,
    DescramblePacketFlow, FrontendRuntimeId, FrontendRuntimeState, GenerationBoundaryReport,
    HalError, HalInvalidStateKind, PipelineBoundaryReason, PipelineReport, TsInputOrigin,
    TsPacketValidationError, TunerServiceRuntime, ValidatedTsPacket, TS_PACKET_SIZE,
};
use crate::registry::ResolvedDescramblerPacketFlow;

#[derive(Clone, Debug)]
pub(super) struct PacketTransactionFailure {
    pub phase: crate::diagnostics::DvrPlaybackPacketFailurePhase,
    pub error: HalError,
}

impl PacketTransactionFailure {
    fn new(
        phase: crate::diagnostics::DvrPlaybackPacketFailurePhase,
        error: HalError,
    ) -> Self {
        Self { phase, error }
    }
}
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
        match frontend_runtime.query().status_snapshot().state() {
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
        let report = demux_runtime
            .apply_generation_boundary_from_typed_request(
                maleicacid_tuner_hal2_demux::DemuxGenerationBoundaryRequest::new(
                    PipelineBoundaryReason::TuneStart,
                ),
            )
            .map_err(demux_runtime_error_to_hal)?;
        self.registry.bind_demux_frontend(demux_key, frontend_key);
        Ok(report)
    }

    fn transact_reset_bound_demuxes_for_frontend_tune_start(
        &mut self,
        frontend_id: i32,
        rollback_tokens: &[(DemuxRuntimeId, maleicacid_tuner_hal2_demux::DemuxRuntimeRollbackToken)],
    ) -> Result<Vec<GenerationBoundaryReport>, HalError> {
        let frontend_key = FrontendRuntimeId(frontend_id);
        if self.registry.frontend(frontend_key).is_none() {
            return Err(HalError::Unsupported(
                "frontend id is not available for tune boundary reset",
            ));
        }
        let demux_ids = self.registry.frontend_bound_demux_ids(frontend_key);

        // First validate every runtime/token pair and mint one-shot boundary commit
        // capabilities. No pipeline state is mutated until all bound demuxes have accepted the
        // same boundary attempt, avoiding a fail-fast partial reset across the set.
        let mut authorizations = Vec::with_capacity(demux_ids.len());
        for demux_id in &demux_ids {
            let token = rollback_tokens
                .iter()
                .find_map(|(id, token)| (*id == *demux_id).then_some(token))
                .ok_or_else(|| {
                    HalError::invalid_state(
                        HalInvalidStateKind::InvalidLifecycle,
                        "bound demux rollback token is missing during tune boundary reset",
                    )
                })?;
            let demux_runtime = self.registry.demux_runtime_mut(*demux_id).ok_or_else(|| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "bound demux runtime is missing during tune boundary reset",
                )
            })?;
            let expected_generation = demux_runtime
                .generation()
                .checked_add(1)
                .ok_or_else(|| {
                    HalError::invalid_state(
                        HalInvalidStateKind::InvalidLifecycle,
                        "bound demux generation exhausted during tune boundary preparation",
                    )
                })?;
            let authorization = demux_runtime
                .authorize_rollback_post_generation(token, expected_generation)
                .map_err(demux_runtime_error_to_hal)?;
            authorizations.push((*demux_id, authorization));
        }

        self.registry
            .commit_authorized_demux_generation_boundaries(
                authorizations,
                PipelineBoundaryReason::TuneStart,
            )
            .map_err(demux_runtime_error_to_hal)
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
            reports.push(
                demux_runtime
                    .apply_generation_boundary_from_typed_request(
                        maleicacid_tuner_hal2_demux::DemuxGenerationBoundaryRequest::new(reason),
                    )
                    .map_err(demux_runtime_error_to_hal)?,
            );
        }
        self.registry.unbind_frontend_demuxes(frontend_key);
        Ok(reports)
    }

    pub(super) fn transact_push_ts_packet_to_demux(
        &mut self,
        demux_id: DemuxRuntimeId,
        packet: &[u8; TS_PACKET_SIZE],
        origin: TsInputOrigin,
    ) -> Result<PipelineReport, PacketTransactionFailure> {
        let generation = self
            .registry
            .demux_runtime(demux_id)
            .map(|runtime| runtime.generation())
            .ok_or_else(|| {
                PacketTransactionFailure::new(
                    crate::diagnostics::DvrPlaybackPacketFailurePhase::RegistryLifecycle,
                    HalError::invalid_state(
                        HalInvalidStateKind::InvalidLifecycle,
                        "demux runtime is missing while routing TS packet",
                    ),
                )
            })?;
        let decision = self.decide_descrambled_packet(demux_id.0, generation, packet);
        let packet_for_demux = match decision.flow {
            DescramblePacketFlow::Drop | DescramblePacketFlow::DiagnoseOnly => {
                let mut report = PipelineReport::default();
                report.diagnostics.extend(decision.diagnostics);
                self.record_descrambler_packet_diagnostics(demux_id.0, generation, &report);
                return Ok(report);
            }
            DescramblePacketFlow::Clear
            | DescramblePacketFlow::Descrambled
            | DescramblePacketFlow::RecordPassThroughAndDropAssembly => {
                (decision.packet, decision.diagnostics)
            }
        };
        let pending_descrambler_diagnostics = packet_for_demux.1;
        let (demux_generation, mut report) = {
            let demux_runtime = self.registry.demux_runtime_mut(demux_id).ok_or_else(|| {
                PacketTransactionFailure::new(
                    crate::diagnostics::DvrPlaybackPacketFailurePhase::RegistryLifecycle,
                    HalError::invalid_state(
                        HalInvalidStateKind::InvalidLifecycle,
                        "demux runtime is missing while delivering TS packet",
                    ),
                )
            })?;
            let demux_generation = demux_runtime.generation();
            let validated = ValidatedTsPacket::validate(&packet_for_demux.0).map_err(|_| {
                PacketTransactionFailure::new(
                    crate::diagnostics::DvrPlaybackPacketFailurePhase::PacketValidation,
                    HalError::internal(
                        super::HalInternalKind::InvariantViolation,
                        "descrambler produced an invalid TS packet",
                    ),
                )
            })?;
            let report = demux_runtime.push_validated_ts_packet_from_typed_request(
                maleicacid_tuner_hal2_demux::ValidatedPacketIngressRequest::new(
                    &validated,
                    origin,
                ),
            );
            (demux_generation, report)
        };
        report.diagnostics.extend(pending_descrambler_diagnostics);
        self.record_descrambler_packet_diagnostics(demux_id.0, demux_generation, &report);
        Ok(report)
    }

    fn transact_push_frontend_ts_packet_to_bound_demuxes(
        &mut self,
        frontend_id: i32,
        packet: &[u8; TS_PACKET_SIZE],
    ) -> Result<Vec<PipelineReport>, HalError> {
        let demux_ids = self.query().ensure_frontend_demux_sink_ready(frontend_id)?;
        let mut reports = Vec::with_capacity(demux_ids.len());
        for demux_id in demux_ids {
            reports.push(
                self.transact_push_ts_packet_to_demux(
                    demux_id,
                    packet,
                    TsInputOrigin::Frontend,
                )
                .map_err(|failure| failure.error)?,
            );
        }
        Ok(reports)
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
                let failure = descramble_failure_for_ts_validation_error(failure);
                let decision = self
                    .registry
                    .descrambler_validation_failure_without_pid_decision(demux_id, failure);
                for record in decision.diagnostic_records {
                    self.record_descrambler_diagnostic(record);
                }
                return DescramblePacketDecision {
                    packet: *packet,
                    flow: match decision.flow {
                        ResolvedDescramblerPacketFlow::Clear => DescramblePacketFlow::Clear,
                        ResolvedDescramblerPacketFlow::Descrambled => {
                            DescramblePacketFlow::Descrambled
                        }
                        ResolvedDescramblerPacketFlow::RecordPassThroughAndDropAssembly => {
                            DescramblePacketFlow::RecordPassThroughAndDropAssembly
                        }
                        ResolvedDescramblerPacketFlow::Drop => DescramblePacketFlow::Drop,
                        ResolvedDescramblerPacketFlow::DiagnoseOnly => {
                            DescramblePacketFlow::DiagnoseOnly
                        }
                    },
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

        let decision = self
            .registry
            .resolved_descrambler_packet_material_for_demux(demux_id, demux_generation, packet_pid)
            .decide_descrambled_packet(demux_id, packet_pid, packet);
        for record in decision.diagnostic_records {
            self.record_descrambler_diagnostic(record);
        }
        DescramblePacketDecision {
            packet: decision.packet,
            flow: match decision.flow {
                ResolvedDescramblerPacketFlow::Clear => DescramblePacketFlow::Clear,
                ResolvedDescramblerPacketFlow::Descrambled => DescramblePacketFlow::Descrambled,
                ResolvedDescramblerPacketFlow::RecordPassThroughAndDropAssembly => {
                    DescramblePacketFlow::RecordPassThroughAndDropAssembly
                }
                ResolvedDescramblerPacketFlow::Drop => DescramblePacketFlow::Drop,
                ResolvedDescramblerPacketFlow::DiagnoseOnly => DescramblePacketFlow::DiagnoseOnly,
            },
            diagnostics: decision.diagnostics,
        }
    }

    fn record_descrambler_packet_diagnostics(
        &mut self,
        demux_id: i32,
        demux_generation: u64,
        report: &PipelineReport,
    ) {
        let records = self
            .registry
            .packet_pipeline_diagnostic_records_for_demux_report(
                demux_id,
                demux_generation,
                report,
            );
        for record in records {
            self.record_descrambler_diagnostic(record);
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
        rollback_tokens: &[(DemuxRuntimeId, maleicacid_tuner_hal2_demux::DemuxRuntimeRollbackToken)],
    ) -> Result<Vec<GenerationBoundaryReport>, HalError> {
        self.runtime
            .transact_reset_bound_demuxes_for_frontend_tune_start(frontend_id, rollback_tokens)
    }

    pub(crate) fn push_frontend_ts_packet_to_bound_demuxes(
        &mut self,
        frontend_id: i32,
        packet: &[u8; TS_PACKET_SIZE],
    ) -> Result<Vec<PipelineReport>, HalError> {
        self.runtime
            .transact_push_frontend_ts_packet_to_bound_demuxes(frontend_id, packet)
    }
}

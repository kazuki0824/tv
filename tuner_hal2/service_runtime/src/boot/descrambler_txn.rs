use super::{
    descrambler_key_lookup_error_to_hal, descrambler_key_release_error_to_hal,
    descrambler_key_token_error_to_hal, descrambler_pid_claim_error_to_hal,
    descrambler_session_failure_to_hal, DemuxRuntimeId, DemuxRuntimeState,
    DescramblerCleanupTxnError, DescramblerClearKeyOutcome, DescramblerClearKeyTxnError,
    DescramblerDiagnosticKind, DescramblerDiagnosticPhase, DescramblerDiagnosticRecord,
    DescramblerKeyToken,
    DescramblerKeyTokenError, DescramblerPid, DescramblerPidClaim, DescramblerReplaceKeyOutcome,
    DescramblerReplaceKeyTxnError, DescramblerRuntimeId, HalError, HalInvalidArgumentKind,
    HalInvalidStateKind, RegistryCommitError, TunerServiceRuntime,
};
use crate::descrambler_key_table::DescramblerKeyLookupError;
use maleicacid_tuner_hal2_common::{compose_primary_cleanup_failure, FirstErrorCollector};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AidlInputPid(u16);

impl AidlInputPid {
    pub(crate) fn validate_descrambler_pid(pid: u16) -> Result<Self, HalError> {
        if pid > 0x1fff {
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "descrambler PID is outside the MPEG-TS PID range",
            ));
        }
        Ok(Self(pid))
    }

    pub(crate) fn to_demux_input_claim(
        self,
    ) -> Result<DescramblerPidClaim, maleicacid_tuner_hal2_descrambler::DescramblerPidClaimError> {
        DescramblerPidClaim::from_demux_input(self.0)
    }

    pub(crate) fn to_source_filter_claim(
        self,
        source_filter_id: i32,
        generation: u64,
    ) -> Result<DescramblerPidClaim, maleicacid_tuner_hal2_descrambler::DescramblerPidClaimError> {
        DescramblerPidClaim::from_source_filter(self.0, source_filter_id, generation)
    }

    pub(crate) fn to_descrambler_pid(self) -> Result<DescramblerPid, HalError> {
        self.to_demux_input_claim()
            .map(|claim| claim.pid())
            .map_err(|error| descrambler_pid_claim_error_to_hal(error))
    }
}

impl TunerServiceRuntime {
    fn transact_allocate_descrambler_runtime(
        &mut self,
    ) -> Result<crate::registry::DescramblerRegistryEntry, RegistryCommitError> {
        self.registry.allocate_descrambler()
    }

    fn descrambler_bound_demux(&self, descrambler_id: i32) -> Result<(i32, u64), HalError> {
        self.registry
            .descrambler_bound_demux(DescramblerRuntimeId(descrambler_id))
            .ok_or_else(|| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "descrambler demux source is not bound",
                )
            })
    }

    pub(super) fn validate_descrambler_source_filter(
        &self,
        expected_demux_id: i32,
        expected_demux_generation: u64,
        source_filter_id: i32,
        pid: DescramblerPid,
    ) -> Result<u64, HalError> {
        self.registry.validate_descrambler_source_filter(
            expected_demux_id,
            expected_demux_generation,
            source_filter_id,
            pid,
        )
    }

    fn transact_set_descrambler_demux_source(
        &mut self,
        descrambler_id: i32,
        demux_id: i32,
    ) -> Result<(), HalError> {
        let demux_runtime = self
            .registry
            .demux_runtime(DemuxRuntimeId(demux_id))
            .ok_or(HalError::Unsupported("demux id is not available"))?;
        match demux_runtime.state() {
            DemuxRuntimeState::Open => {}
            DemuxRuntimeState::Closing
            | DemuxRuntimeState::CleanupFailed
            | DemuxRuntimeState::Closed
            | DemuxRuntimeState::Failed
            | DemuxRuntimeState::Quarantined => {
                return Err(HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "demux runtime is not live",
                ));
            }
        }
        let demux_generation = demux_runtime.generation();
        self.registry
            .bind_descrambler_demux_use_case(
                DescramblerRuntimeId(descrambler_id),
                demux_id,
                demux_generation,
            )
            .map_err(|failure| descrambler_session_failure_to_hal(failure.kind))
    }

    fn transact_set_descrambler_key_token(
        &mut self,
        descrambler_id: i32,
        key_token: &[u8],
    ) -> Result<(), HalError> {
        if key_token == [0x00].as_slice() {
            return match self
                .registry
                .clear_descrambler_key_use_case(DescramblerRuntimeId(descrambler_id))
            {
                Ok(
                    DescramblerClearKeyOutcome::AlreadyClear
                    | DescramblerClearKeyOutcome::Cleared,
                ) => Ok(()),
                Ok(DescramblerClearKeyOutcome::ClearedWithOldKeyReleaseFailure {
                    release_old,
                }) => {
                    let hal_error = descrambler_key_release_error_to_hal(release_old);
                    self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::set_key_token(
                        descrambler_id,
                        DescramblerDiagnosticKind::KeyTokenReleaseFailed,
                        hal_error,
                    ));
                    Ok(())
                }
                Err(DescramblerClearKeyTxnError::Session(failure)) => {
                    let error = descrambler_session_failure_to_hal(failure.kind);
                    self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::set_key_token(
                        descrambler_id,
                        DescramblerDiagnosticKind::SessionClosed,
                        error.clone(),
                    ));
                    Err(error)
                }
            };
        }
        let token = match DescramblerKeyToken::try_from_bytes(key_token.to_vec()) {
            Ok(token) => token,
            Err(error) => {
                let kind = match error {
                    DescramblerKeyTokenError::Empty => DescramblerDiagnosticKind::KeyTokenEmpty,
                    DescramblerKeyTokenError::InvalidLength { .. } => {
                        DescramblerDiagnosticKind::KeyTokenInvalidLength
                    }
                };
                let hal_error = descrambler_key_token_error_to_hal(error);
                self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::set_key_token(
                    descrambler_id,
                    kind,
                    hal_error.clone(),
                ));
                return Err(hal_error);
            }
        };
        if !self.registry.descrambler_token_resolution_available() {
            let error = HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "descrambler CAS token producer is not connected",
            );
            self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::set_key_token(
                descrambler_id,
                DescramblerDiagnosticKind::CasTokenProducerUnavailable,
                error.clone(),
            ));
            return Err(error);
        }
        match self
            .registry
            .replace_descrambler_key_use_case(DescramblerRuntimeId(descrambler_id), token)
        {
            Ok(
                DescramblerReplaceKeyOutcome::AlreadyCurrent
                | DescramblerReplaceKeyOutcome::Replaced,
            ) => Ok(()),
            Ok(DescramblerReplaceKeyOutcome::ReplacedWithOldKeyReleaseFailure { release_old }) => {
                let hal_error = descrambler_key_release_error_to_hal(release_old);
                self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::set_key_token(
                    descrambler_id,
                    DescramblerDiagnosticKind::KeyTokenReleaseFailed,
                    hal_error,
                ));
                Ok(())
            }
            Err(DescramblerReplaceKeyTxnError::Session(failure)) => {
                let hal_error = descrambler_session_failure_to_hal(failure.kind);
                self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::set_key_token(
                    descrambler_id,
                    DescramblerDiagnosticKind::SessionClosed,
                    hal_error.clone(),
                ));
                Err(hal_error)
            }
            Err(DescramblerReplaceKeyTxnError::Acquire(error)) => {
                let kind = match error {
                    DescramblerKeyLookupError::UnknownToken => {
                        DescramblerDiagnosticKind::KeyTokenUnknown
                    }
                    DescramblerKeyLookupError::ExpiredToken => {
                        DescramblerDiagnosticKind::KeyTokenExpired
                    }
                };
                let hal_error = descrambler_key_lookup_error_to_hal(error);
                self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::set_key_token(
                    descrambler_id,
                    kind,
                    hal_error.clone(),
                ));
                Err(hal_error)
            }
            Err(DescramblerReplaceKeyTxnError::Commit {
                failure,
                rollback_release,
            }) => {
                let hal_error = descrambler_session_failure_to_hal(failure.kind);
                let final_error = match rollback_release {
                    Some(rollback_error) => compose_primary_cleanup_failure(
                        "descrambler setKeyToken session replace rollback release",
                        hal_error.clone(),
                        descrambler_key_release_error_to_hal(rollback_error),
                    ),
                    None => hal_error.clone(),
                };
                self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::set_key_token(
                    descrambler_id,
                    DescramblerDiagnosticKind::SessionClosed,
                    final_error.clone(),
                ));
                Err(final_error)
            }
        }
    }

    fn transact_add_descrambler_pid_demux_input(
        &mut self,
        descrambler_id: i32,
        pid: u16,
    ) -> Result<(), HalError> {
        let validated_pid = match AidlInputPid::validate_descrambler_pid(pid) {
            Ok(pid) => pid,
            Err(hal_error) => {
                self.record_descrambler_diagnostic(
                    DescramblerDiagnosticRecord::pid_claim_invalid_pid_without_demux(
                        DescramblerDiagnosticPhase::AddPid,
                        descrambler_id,
                        pid,
                        -1,
                        hal_error.clone(),
                    ),
                );
                return Err(hal_error);
            }
        };
        let claim = match validated_pid.to_demux_input_claim() {
            Ok(claim) => claim,
            Err(error) => {
                let hal_error = descrambler_pid_claim_error_to_hal(error);
                self.record_descrambler_diagnostic(
                    DescramblerDiagnosticRecord::pid_claim_invalid_pid_without_demux(
                        DescramblerDiagnosticPhase::AddPid,
                        descrambler_id,
                        pid,
                        -1,
                        hal_error.clone(),
                    ),
                );
                return Err(hal_error);
            }
        };
        let claim_pid = claim.pid();
        let (demux_id, demux_generation) = match self.descrambler_bound_demux(descrambler_id) {
            Ok(bound) => bound,
            Err(error) => {
                self.record_descrambler_diagnostic(
                    DescramblerDiagnosticRecord::pid_claim_without_demux(
                        DescramblerDiagnosticPhase::AddPid,
                        descrambler_id,
                        claim_pid,
                        -1,
                        error.clone(),
                    ),
                );
                return Err(error);
            }
        };
        if self.registry.descrambler_pid_claimed_by_other(
            DescramblerRuntimeId(descrambler_id),
            demux_id,
            demux_generation,
            claim_pid,
        ) {
            let error = HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "descrambler PID is already claimed by another session",
            );
            self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::pid_claim_with_demux(
                DescramblerDiagnosticPhase::AddPid,
                descrambler_id,
                demux_id,
                claim_pid,
                -1,
                error.clone(),
            ));
            return Err(error);
        }
        let add_result = self
            .registry
            .add_descrambler_pid_claim_use_case(DescramblerRuntimeId(descrambler_id), claim)
            .map_err(|failure| descrambler_session_failure_to_hal(failure.kind));
        if let Err(error) = add_result {
            self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::pid_claim_with_demux(
                DescramblerDiagnosticPhase::AddPid,
                descrambler_id,
                demux_id,
                claim_pid,
                -1,
                error.clone(),
            ));
            return Err(error);
        }
        Ok(())
    }

    fn transact_remove_descrambler_pid_demux_input(
        &mut self,
        descrambler_id: i32,
        pid: u16,
    ) -> Result<(), HalError> {
        let validated_pid = match AidlInputPid::validate_descrambler_pid(pid) {
            Ok(pid) => pid,
            Err(hal_error) => {
                self.record_descrambler_diagnostic(
                    DescramblerDiagnosticRecord::pid_claim_invalid_pid_without_demux(
                        DescramblerDiagnosticPhase::RemovePid,
                        descrambler_id,
                        pid,
                        -1,
                        hal_error.clone(),
                    ),
                );
                return Err(hal_error);
            }
        };
        let claim = match validated_pid.to_demux_input_claim() {
            Ok(claim) => claim,
            Err(error) => {
                let hal_error = descrambler_pid_claim_error_to_hal(error);
                self.record_descrambler_diagnostic(
                    DescramblerDiagnosticRecord::pid_claim_invalid_pid_without_demux(
                        DescramblerDiagnosticPhase::RemovePid,
                        descrambler_id,
                        pid,
                        -1,
                        hal_error.clone(),
                    ),
                );
                return Err(hal_error);
            }
        };
        let claim_pid = claim.pid();
        let (demux_id, _demux_generation) = match self.descrambler_bound_demux(descrambler_id) {
            Ok(bound) => bound,
            Err(error) => {
                self.record_descrambler_diagnostic(
                    DescramblerDiagnosticRecord::pid_claim_without_demux(
                        DescramblerDiagnosticPhase::RemovePid,
                        descrambler_id,
                        claim_pid,
                        -1,
                        error.clone(),
                    ),
                );
                return Err(error);
            }
        };
        let remove_result = self
            .registry
            .remove_descrambler_pid_claim_use_case(DescramblerRuntimeId(descrambler_id), claim)
            .map_err(|failure| descrambler_session_failure_to_hal(failure.kind));
        if let Err(error) = remove_result {
            self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::pid_claim_with_demux(
                DescramblerDiagnosticPhase::RemovePid,
                descrambler_id,
                demux_id,
                claim_pid,
                -1,
                error.clone(),
            ));
            return Err(error);
        }
        Ok(())
    }

    fn transact_add_descrambler_pid_non_null_source(
        &mut self,
        descrambler_id: i32,
        pid: u16,
        source_filter_id: i32,
    ) -> Result<(), HalError> {
        let validated_pid = match AidlInputPid::validate_descrambler_pid(pid) {
            Ok(pid) => pid,
            Err(hal_error) => {
                self.record_descrambler_diagnostic(
                    DescramblerDiagnosticRecord::pid_claim_invalid_pid_without_demux(
                        DescramblerDiagnosticPhase::AddPid,
                        descrambler_id,
                        pid,
                        source_filter_id,
                        hal_error.clone(),
                    ),
                );
                return Err(hal_error);
            }
        };
        let diagnostic_pid = match validated_pid.to_descrambler_pid() {
            Ok(pid) => pid,
            Err(hal_error) => {
                self.record_descrambler_diagnostic(
                    DescramblerDiagnosticRecord::pid_claim_invalid_pid_without_demux(
                        DescramblerDiagnosticPhase::AddPid,
                        descrambler_id,
                        pid,
                        source_filter_id,
                        hal_error.clone(),
                    ),
                );
                return Err(hal_error);
            }
        };
        let (demux_id, demux_generation) = match self.descrambler_bound_demux(descrambler_id) {
            Ok(bound) => bound,
            Err(error) => {
                self.record_descrambler_diagnostic(
                    DescramblerDiagnosticRecord::pid_claim_without_demux(
                        DescramblerDiagnosticPhase::AddPid,
                        descrambler_id,
                        diagnostic_pid,
                        source_filter_id,
                        error.clone(),
                    ),
                );
                return Err(error);
            }
        };
        let source_generation = match self.validate_descrambler_source_filter(
            demux_id,
            demux_generation,
            source_filter_id,
            diagnostic_pid,
        ) {
            Ok(source_generation) => source_generation,
            Err(error) => {
                self.record_descrambler_diagnostic(
                    DescramblerDiagnosticRecord::pid_claim_with_demux(
                        DescramblerDiagnosticPhase::AddPid,
                        descrambler_id,
                        demux_id,
                        diagnostic_pid,
                        source_filter_id,
                        error.clone(),
                    ),
                );
                return Err(error);
            }
        };
        if self.registry.descrambler_pid_claimed_by_other(
            DescramblerRuntimeId(descrambler_id),
            demux_id,
            demux_generation,
            diagnostic_pid,
        ) {
            let error = HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "descrambler PID is already claimed by another session",
            );
            self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::pid_claim_with_demux(
                DescramblerDiagnosticPhase::AddPid,
                descrambler_id,
                demux_id,
                diagnostic_pid,
                source_filter_id,
                error.clone(),
            ));
            return Err(error);
        }
        let claim = match validated_pid.to_source_filter_claim(source_filter_id, source_generation) {
            Ok(claim) => claim,
            Err(error) => {
                let hal_error = descrambler_pid_claim_error_to_hal(error);
                self.record_descrambler_diagnostic(
                    DescramblerDiagnosticRecord::pid_claim_with_demux(
                        DescramblerDiagnosticPhase::AddPid,
                        descrambler_id,
                        demux_id,
                        diagnostic_pid,
                        source_filter_id,
                        hal_error.clone(),
                    ),
                );
                return Err(hal_error);
            }
        };
        let add_result = self
            .registry
            .add_descrambler_pid_claim_use_case(DescramblerRuntimeId(descrambler_id), claim)
            .map_err(|failure| descrambler_session_failure_to_hal(failure.kind));
        if let Err(error) = add_result {
            self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::pid_claim_with_demux(
                DescramblerDiagnosticPhase::AddPid,
                descrambler_id,
                demux_id,
                diagnostic_pid,
                source_filter_id,
                error.clone(),
            ));
            return Err(error);
        }
        Ok(())
    }

    fn transact_remove_descrambler_pid_non_null_source(
        &mut self,
        descrambler_id: i32,
        pid: u16,
        source_filter_id: i32,
    ) -> Result<(), HalError> {
        let validated_pid = match AidlInputPid::validate_descrambler_pid(pid) {
            Ok(pid) => pid,
            Err(hal_error) => {
                self.record_descrambler_diagnostic(
                    DescramblerDiagnosticRecord::pid_claim_invalid_pid_without_demux(
                        DescramblerDiagnosticPhase::RemovePid,
                        descrambler_id,
                        pid,
                        source_filter_id,
                        hal_error.clone(),
                    ),
                );
                return Err(hal_error);
            }
        };
        let diagnostic_pid = match validated_pid.to_descrambler_pid() {
            Ok(pid) => pid,
            Err(hal_error) => {
                self.record_descrambler_diagnostic(
                    DescramblerDiagnosticRecord::pid_claim_invalid_pid_without_demux(
                        DescramblerDiagnosticPhase::RemovePid,
                        descrambler_id,
                        pid,
                        source_filter_id,
                        hal_error.clone(),
                    ),
                );
                return Err(hal_error);
            }
        };
        let (demux_id, demux_generation) = match self.descrambler_bound_demux(descrambler_id) {
            Ok(bound) => bound,
            Err(error) => {
                self.record_descrambler_diagnostic(
                    DescramblerDiagnosticRecord::pid_claim_without_demux(
                        DescramblerDiagnosticPhase::RemovePid,
                        descrambler_id,
                        diagnostic_pid,
                        source_filter_id,
                        error.clone(),
                    ),
                );
                return Err(error);
            }
        };
        let source_generation = match self.validate_descrambler_source_filter(
            demux_id,
            demux_generation,
            source_filter_id,
            diagnostic_pid,
        ) {
            Ok(source_generation) => source_generation,
            Err(error) => {
                self.record_descrambler_diagnostic(
                    DescramblerDiagnosticRecord::pid_claim_with_demux(
                        DescramblerDiagnosticPhase::RemovePid,
                        descrambler_id,
                        demux_id,
                        diagnostic_pid,
                        source_filter_id,
                        error.clone(),
                    ),
                );
                return Err(error);
            }
        };
        let claim = match validated_pid.to_source_filter_claim(source_filter_id, source_generation) {
            Ok(claim) => claim,
            Err(error) => {
                let hal_error = descrambler_pid_claim_error_to_hal(error);
                self.record_descrambler_diagnostic(
                    DescramblerDiagnosticRecord::pid_claim_with_demux(
                        DescramblerDiagnosticPhase::RemovePid,
                        descrambler_id,
                        demux_id,
                        diagnostic_pid,
                        source_filter_id,
                        hal_error.clone(),
                    ),
                );
                return Err(hal_error);
            }
        };
        let stale_source_generation = self.registry.descrambler_has_stale_source_generation(
            DescramblerRuntimeId(descrambler_id),
            diagnostic_pid,
            source_filter_id,
            source_generation,
        );
        if stale_source_generation {
            let error = HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "source filter generation changed before PID removal",
            );
            self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::pid_claim_with_demux(
                DescramblerDiagnosticPhase::RemovePid,
                descrambler_id,
                demux_id,
                diagnostic_pid,
                source_filter_id,
                error.clone(),
            ));
            return Err(error);
        }
        let remove_result = self
            .registry
            .remove_descrambler_pid_claim_use_case(DescramblerRuntimeId(descrambler_id), claim)
            .map_err(|failure| descrambler_session_failure_to_hal(failure.kind));
        if let Err(error) = remove_result {
            self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::pid_claim_with_demux(
                DescramblerDiagnosticPhase::RemovePid,
                descrambler_id,
                demux_id,
                diagnostic_pid,
                source_filter_id,
                error.clone(),
            ));
            return Err(error);
        }
        Ok(())
    }

    fn transact_unregister_descrambler_runtime(
        &mut self,
        id: i32,
    ) -> Result<Option<crate::registry::DescramblerRegistryEntry>, HalError> {
        self.cleanup_descrambler_session(id)?;
        Ok(self
            .registry
            .unregister_descrambler(DescramblerRuntimeId(id)))
    }

    fn transact_cleanup_descramblers_for_demux_owner_loss(
        &mut self,
        demux_id: i32,
    ) -> Result<(), HalError> {
        let descrambler_ids = self.registry.descrambler_ids_bound_to_demux(demux_id);
        let mut collector = FirstErrorCollector::new();
        for descrambler_id in descrambler_ids {
            collector.push_result(self.cleanup_descrambler_session(descrambler_id.0));
        }
        collector.into_result()
    }

    fn cleanup_descrambler_session(&mut self, id: i32) -> Result<(), HalError> {
        match self
            .registry
            .cleanup_descrambler_use_case(DescramblerRuntimeId(id))
        {
            Ok(_cleanup_report) => Ok(()),
            Err(DescramblerCleanupTxnError::ReleaseKey(error)) => {
                let hal_error = descrambler_key_release_error_to_hal(error);
                self.record_descrambler_diagnostic(
                    DescramblerDiagnosticRecord::cleanup_release_failed(id, hal_error.clone()),
                );
                Err(hal_error)
            }
            Err(DescramblerCleanupTxnError::Session(failure)) => {
                let hal_error = descrambler_session_failure_to_hal(failure.kind);
                self.record_descrambler_diagnostic(
                    DescramblerDiagnosticRecord::cleanup_release_failed(id, hal_error.clone()),
                );
                Err(hal_error)
            }
            Err(DescramblerCleanupTxnError::ReleaseKeyAndSession { release, session }) => {
                let release_error = descrambler_key_release_error_to_hal(release);
                let session_error = descrambler_session_failure_to_hal(session.kind);
                self.record_descrambler_diagnostic(
                    DescramblerDiagnosticRecord::cleanup_release_failed(id, release_error.clone()),
                );
                self.record_descrambler_diagnostic(
                    DescramblerDiagnosticRecord::cleanup_release_failed(id, session_error.clone()),
                );
                Err(compose_primary_cleanup_failure(
                    "descrambler cleanup key release plus session cleanup",
                    release_error,
                    session_error,
                ))
            }
        }
    }
}

pub(crate) struct DescramblerTxn<'a> {
    runtime: &'a mut TunerServiceRuntime,
}

impl TunerServiceRuntime {
    pub(crate) fn descrambler_txn(&mut self) -> DescramblerTxn<'_> {
        DescramblerTxn { runtime: self }
    }
}

impl<'a> DescramblerTxn<'a> {
    pub(crate) fn allocate_descrambler_runtime(
        &mut self,
    ) -> Result<crate::registry::DescramblerRegistryEntry, RegistryCommitError> {
        self.runtime.transact_allocate_descrambler_runtime()
    }

    pub(crate) fn set_descrambler_demux_source(
        &mut self,
        descrambler_id: i32,
        demux_id: i32,
    ) -> Result<(), HalError> {
        self.runtime
            .transact_set_descrambler_demux_source(descrambler_id, demux_id)
    }

    pub(crate) fn set_descrambler_key_token(
        &mut self,
        descrambler_id: i32,
        key_token: &[u8],
    ) -> Result<(), HalError> {
        self.runtime
            .transact_set_descrambler_key_token(descrambler_id, key_token)
    }

    pub(crate) fn add_descrambler_pid_demux_input(
        &mut self,
        descrambler_id: i32,
        pid: u16,
    ) -> Result<(), HalError> {
        self.runtime
            .transact_add_descrambler_pid_demux_input(descrambler_id, pid)
    }

    pub(crate) fn remove_descrambler_pid_demux_input(
        &mut self,
        descrambler_id: i32,
        pid: u16,
    ) -> Result<(), HalError> {
        self.runtime
            .transact_remove_descrambler_pid_demux_input(descrambler_id, pid)
    }

    pub(crate) fn add_descrambler_pid_non_null_source(
        &mut self,
        descrambler_id: i32,
        pid: u16,
        source_filter_id: i32,
    ) -> Result<(), HalError> {
        self.runtime.transact_add_descrambler_pid_non_null_source(
            descrambler_id,
            pid,
            source_filter_id,
        )
    }

    pub(crate) fn remove_descrambler_pid_non_null_source(
        &mut self,
        descrambler_id: i32,
        pid: u16,
        source_filter_id: i32,
    ) -> Result<(), HalError> {
        self.runtime
            .transact_remove_descrambler_pid_non_null_source(descrambler_id, pid, source_filter_id)
    }

    pub(crate) fn unregister_descrambler_runtime(
        &mut self,
        id: i32,
    ) -> Result<Option<crate::registry::DescramblerRegistryEntry>, HalError> {
        self.runtime.transact_unregister_descrambler_runtime(id)
    }

    pub(crate) fn cleanup_descramblers_for_demux_owner_loss(
        &mut self,
        demux_id: i32,
    ) -> Result<(), HalError> {
        self.runtime
            .transact_cleanup_descramblers_for_demux_owner_loss(demux_id)
    }
}

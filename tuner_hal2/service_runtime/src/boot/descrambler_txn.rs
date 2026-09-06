use super::{
    descrambler_key_lookup_error_to_hal, descrambler_key_release_error_to_hal,
    descrambler_key_token_error_to_hal, descrambler_pid_claim_error_to_hal,
    descrambler_session_failure_to_hal, DemuxRuntimeId, DemuxRuntimeState,
    DescramblerCleanupTxnError, DescramblerClearKeyOutcome, DescramblerClearKeyTxnError,
    DescramblerDiagnosticKind, DescramblerDiagnosticPhase, DescramblerDiagnosticRecord,
    DescramblerKeyToken, DescramblerKeyTokenError, DescramblerPid, DescramblerPidClaim,
    DescramblerReplaceKeyOutcome, DescramblerReplaceKeyTxnError, DescramblerRuntimeId, HalError,
    HalInvalidArgumentKind, HalInvalidStateKind, RegistryCommitError, TunerServiceRuntime,
};
use crate::descrambler_key_table::DescramblerKeyLookupError;
use crate::descrambler_session::DescramblerSourceCallFailure;
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
    ) -> Result<DescramblerPidClaim, maleicacid_tuner_hal2_descrambler::DescramblerPidClaimError>
    {
        DescramblerPidClaim::from_demux_input(self.0)
    }

    pub(crate) fn to_source_filter_claim(
        self,
        source_filter_id: i32,
        generation: u64,
    ) -> Result<DescramblerPidClaim, maleicacid_tuner_hal2_descrambler::DescramblerPidClaimError>
    {
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
        let (demux_id, generation) = self
            .registry
            .descrambler_bound_demux(DescramblerRuntimeId(descrambler_id))
            .ok_or_else(|| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "descrambler demux source is not bound",
                )
            })?;
        let demux = self
            .registry
            .demux_runtime(DemuxRuntimeId(demux_id))
            .ok_or_else(|| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "descrambler source demux runtime no longer exists",
                )
            })?;
        if demux.state() != DemuxRuntimeState::Open || demux.generation() != generation {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "descrambler source demux generation is no longer live",
            ));
        }
        Ok((demux_id, generation))
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
        self.registry
            .begin_descrambler_demux_source_call_use_case(DescramblerRuntimeId(descrambler_id))
            .map_err(|failure| descrambler_session_failure_to_hal(failure.kind))?;
        let Some(demux_runtime) = self.registry.demux_runtime(DemuxRuntimeId(demux_id)) else {
            let error = HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "demux id is not available",
            );
            return Err(self.finish_failed_descrambler_source_call(
                descrambler_id,
                DescramblerSourceCallFailure::InvalidDemuxId,
                error,
            ));
        };
        match demux_runtime.state() {
            DemuxRuntimeState::Open => {}
            DemuxRuntimeState::Closing
            | DemuxRuntimeState::CleanupFailed
            | DemuxRuntimeState::Closed
            | DemuxRuntimeState::Failed
            | DemuxRuntimeState::Quarantined => {
                let error = HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "demux runtime is not live",
                );
                return Err(self.finish_failed_descrambler_source_call(
                    descrambler_id,
                    DescramblerSourceCallFailure::InvalidDemuxState,
                    error,
                ));
            }
        }
        let demux_generation = demux_runtime.generation();
        match self.registry.bind_descrambler_demux_use_case(
            DescramblerRuntimeId(descrambler_id),
            demux_id,
            demux_generation,
        ) {
            Ok(()) => Ok(()),
            Err(failure) => {
                let error = descrambler_session_failure_to_hal(failure.kind);
                Err(self.finish_failed_descrambler_source_call(
                    descrambler_id,
                    DescramblerSourceCallFailure::BindingCommitFailed,
                    error,
                ))
            }
        }
    }

    fn finish_failed_descrambler_source_call(
        &mut self,
        descrambler_id: i32,
        failure: DescramblerSourceCallFailure,
        primary: HalError,
    ) -> HalError {
        match self
            .registry
            .record_descrambler_demux_source_call_failure_use_case(
                DescramblerRuntimeId(descrambler_id),
                failure,
            ) {
            Ok(()) => primary,
            Err(record_failure) => compose_primary_cleanup_failure(
                "descrambler source-call failure state commit failed",
                primary,
                descrambler_session_failure_to_hal(record_failure.kind),
            ),
        }
    }
}

/// Descrambler 鍵変更の検証、backend 適用、commit、旧鍵解放を所有する transaction。
///
/// registry/session は既存の atomic primitive のまま使い、追加の状態や lifecycle は
/// 持たない call-local owner とする。
pub(crate) struct DescramblerKeyTxn<'a> {
    runtime: &'a mut TunerServiceRuntime,
}

impl std::ops::Deref for DescramblerKeyTxn<'_> {
    type Target = TunerServiceRuntime;

    fn deref(&self) -> &Self::Target {
        self.runtime
    }
}

impl std::ops::DerefMut for DescramblerKeyTxn<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.runtime
    }
}

impl TunerServiceRuntime {
    pub(crate) fn descrambler_key_txn(&mut self) -> DescramblerKeyTxn<'_> {
        DescramblerKeyTxn { runtime: self }
    }
}

impl DescramblerKeyTxn<'_> {
    pub(crate) fn set_key_token(
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
                    DescramblerClearKeyOutcome::AlreadyClear | DescramblerClearKeyOutcome::Cleared,
                ) => Ok(()),
                Ok(DescramblerClearKeyOutcome::ClearedWithOldKeyReleaseFailure { release_old }) => {
                    let hal_error = descrambler_key_release_error_to_hal(release_old);
                    self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::set_key_token(
                        descrambler_id,
                        DescramblerDiagnosticKind::KeyTokenReleaseFailed,
                        hal_error.clone(),
                    ));
                    Err(hal_error)
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
        if let Err(error) = self.descrambler_bound_demux(descrambler_id) {
            self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::set_key_token(
                descrambler_id,
                DescramblerDiagnosticKind::SessionClosed,
                error.clone(),
            ));
            return Err(error);
        }
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
                    hal_error.clone(),
                ));
                Err(hal_error)
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
}

/// Descrambler PID 変更の検証から commit までを所有する call-local transaction。
///
/// registry の session transaction は atomic commit primitive として使用し、source
/// 検証、排他確認、commit、失敗診断はこの transaction 境界から外へ分散させない。
pub(crate) struct DescramblerPidTxn<'a> {
    runtime: &'a mut TunerServiceRuntime,
}

impl std::ops::Deref for DescramblerPidTxn<'_> {
    type Target = TunerServiceRuntime;

    fn deref(&self) -> &Self::Target {
        self.runtime
    }
}

impl std::ops::DerefMut for DescramblerPidTxn<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.runtime
    }
}

impl TunerServiceRuntime {
    pub(crate) fn descrambler_pid_txn(&mut self) -> DescramblerPidTxn<'_> {
        DescramblerPidTxn { runtime: self }
    }
}

impl DescramblerPidTxn<'_> {
    pub(crate) fn add_demux_input(
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

    pub(crate) fn remove_demux_input(
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

    pub(crate) fn add_non_null_source(
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
        let claim = match validated_pid.to_source_filter_claim(source_filter_id, source_generation)
        {
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

    pub(crate) fn remove_non_null_source(
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
        let claim = match validated_pid.to_source_filter_claim(source_filter_id, source_generation)
        {
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
}

/// Descrambler close と owner-loss cleanup の全対象処理・失敗集約を所有する transaction。
///
/// session/key table の状態所有者は既存 registry primitive のままとし、この型は
/// call-local な処理順序だけを所有する。
pub(crate) struct DescramblerSessionCleanupTxn<'a> {
    runtime: &'a mut TunerServiceRuntime,
}

impl std::ops::Deref for DescramblerSessionCleanupTxn<'_> {
    type Target = TunerServiceRuntime;

    fn deref(&self) -> &Self::Target {
        self.runtime
    }
}

impl std::ops::DerefMut for DescramblerSessionCleanupTxn<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.runtime
    }
}

impl TunerServiceRuntime {
    pub(crate) fn descrambler_session_cleanup_txn(&mut self) -> DescramblerSessionCleanupTxn<'_> {
        DescramblerSessionCleanupTxn { runtime: self }
    }
}

impl DescramblerSessionCleanupTxn<'_> {
    pub(crate) fn unregister_runtime(
        &mut self,
        id: i32,
    ) -> Result<Option<crate::registry::DescramblerRegistryEntry>, HalError> {
        self.cleanup_descrambler_session(id)?;
        Ok(self
            .registry
            .unregister_descrambler(DescramblerRuntimeId(id)))
    }

    pub(crate) fn cleanup_for_demux_owner_loss(&mut self, demux_id: i32) -> Result<(), HalError> {
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
        }
    }
}

/// Descrambler registry primitive への call-local access。
///
/// 鍵、PID、cleanup の transaction ownership は各 canonical owner が持つ。この
/// context は allocation と source relation の registry primitive だけを束ね、共有状態や
/// lifecycle を持たない。
pub(crate) struct DescramblerMutationContext<'a> {
    runtime: &'a mut TunerServiceRuntime,
}

impl TunerServiceRuntime {
    pub(crate) fn descrambler_mutation_context(&mut self) -> DescramblerMutationContext<'_> {
        DescramblerMutationContext { runtime: self }
    }
}

impl DescramblerMutationContext<'_> {
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
}

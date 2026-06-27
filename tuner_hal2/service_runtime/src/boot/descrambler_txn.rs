use super::{
    add_pid_claim_with_session_txn, bind_demux_with_session_txn, cleanup_all_with_session_txn,
    descrambler_key_lookup_error_to_hal, descrambler_key_release_error_to_hal,
    descrambler_key_token_error_to_hal, descrambler_pid_claim_error_to_hal,
    descrambler_session_failure_to_hal, DemuxRuntimeId, DemuxRuntimeState,
    DescramblerClearKeyTxnError, DescramblerDiagnosticKind, DescramblerDiagnosticPhase,
    DescramblerDiagnosticRecord, DescramblerKeyLookupError, DescramblerKeyToken,
    DescramblerKeyTokenError, DescramblerPidClaim, DescramblerReplaceKeyOutcome,
    DescramblerReplaceKeyTxnError, DescramblerRuntimeId, FilterOpenType, FilterRuntimeId,
    FilterRuntimeState, HalError, HalInvalidArgumentKind, HalInvalidStateKind, RegistryCommitError,
    TunerServiceRuntime,
};
use maleicacid_tuner_hal2_common::{compose_primary_cleanup_failure, FirstErrorCollector};

impl TunerServiceRuntime {
    fn transact_allocate_descrambler_runtime(
        &mut self,
    ) -> Result<crate::registry::DescramblerRegistryEntry, RegistryCommitError> {
        self.registry.allocate_descrambler()
    }

    fn descrambler_runtime_mut(
        &mut self,
        descrambler_id: i32,
    ) -> Result<&mut maleicacid_tuner_hal2_descrambler::DescramblerRuntime, HalError> {
        self.registry
            .descrambler_runtime_mut(DescramblerRuntimeId(descrambler_id))
            .ok_or_else(|| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "descrambler runtime is missing",
                )
            })
    }

    fn descrambler_bound_demux(&self, descrambler_id: i32) -> Result<(i32, u64), HalError> {
        let runtime = self
            .registry
            .descrambler_runtime(DescramblerRuntimeId(descrambler_id))
            .ok_or_else(|| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "descrambler runtime is missing",
                )
            })?;
        let demux_id = runtime.session().demux_id().ok_or_else(|| {
            HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "descrambler demux source is not bound",
            )
        })?;
        let demux_generation = runtime.session().demux_generation().ok_or_else(|| {
            HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "descrambler demux generation is not bound",
            )
        })?;
        Ok((demux_id, demux_generation))
    }

    pub(super) fn validate_descrambler_source_filter(
        &self,
        expected_demux_id: i32,
        expected_demux_generation: u64,
        source_filter_id: i32,
        pid: u16,
    ) -> Result<u64, HalError> {
        let filter_entry = self
            .registry
            .filter(FilterRuntimeId(source_filter_id))
            .ok_or_else(|| {
                HalError::invalid_argument(
                    HalInvalidArgumentKind::NumericRange,
                    "source filter registry entry is missing",
                )
            })?;
        if filter_entry.owner_demux_id != expected_demux_id {
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "source filter belongs to another demux",
            ));
        }
        let Some(demux_runtime) = self
            .registry
            .demux_runtime(DemuxRuntimeId(filter_entry.owner_demux_id))
        else {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "owner demux runtime is missing",
            ));
        };
        if demux_runtime.generation() != expected_demux_generation {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "descrambler demux generation is stale",
            ));
        }
        let source_snapshot = demux_runtime
            .filter_snapshot(source_filter_id)
            .map_err(Self::map_filter_runtime_error)?;
        if source_snapshot.state == FilterRuntimeState::Open
            || source_snapshot.state.is_closed_or_failed()
            || source_snapshot.tpid.is_none()
        {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "source filter is not configured",
            ));
        }
        if source_snapshot.tpid != Some(i32::from(pid)) {
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "source filter PID does not match descrambler PID",
            ));
        }
        if !matches!(
            source_snapshot.open_type,
            FilterOpenType::TsAudio
                | FilterOpenType::TsVideo
                | FilterOpenType::TsPes
                | FilterOpenType::TsRecord
        ) {
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "source filter subtype is not valid for descrambler PID source",
            ));
        }
        Ok(source_snapshot.generation)
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
        let runtime = self.descrambler_runtime_mut(descrambler_id)?;
        bind_demux_with_session_txn(runtime.session_mut(), demux_id, demux_generation)
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
                .clear_descrambler_key_with_session_txn(DescramblerRuntimeId(descrambler_id))
            {
                Ok(()) => Ok(()),
                Err(DescramblerClearKeyTxnError::Session(failure)) => {
                    let error = descrambler_session_failure_to_hal(failure.kind);
                    self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::set_key_token(
                        descrambler_id,
                        DescramblerDiagnosticKind::SessionClosed,
                        error.clone(),
                    ));
                    Err(error)
                }
                Err(DescramblerClearKeyTxnError::ReleaseOld(error)) => {
                    let hal_error = descrambler_key_release_error_to_hal(error);
                    self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::set_key_token(
                        descrambler_id,
                        DescramblerDiagnosticKind::KeyTokenReleaseFailed,
                        hal_error.clone(),
                    ));
                    Err(hal_error)
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
        if !self
            .registry
            .descrambler_key_table()
            .has_token_resolution_state()
        {
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
            .replace_descrambler_key_with_session_txn(DescramblerRuntimeId(descrambler_id), token)
        {
            Ok(
                DescramblerReplaceKeyOutcome::AlreadyCurrent
                | DescramblerReplaceKeyOutcome::Replaced,
            ) => Ok(()),
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
            Err(DescramblerReplaceKeyTxnError::ReleaseOld(error)) => {
                let hal_error = descrambler_key_release_error_to_hal(error);
                self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::set_key_token(
                    descrambler_id,
                    DescramblerDiagnosticKind::KeyTokenReleaseFailed,
                    hal_error.clone(),
                ));
                Err(hal_error)
            }
        }
    }

    fn transact_add_descrambler_pid_demux_input(
        &mut self,
        descrambler_id: i32,
        pid: u16,
    ) -> Result<(), HalError> {
        let (demux_id, demux_generation) = match self.descrambler_bound_demux(descrambler_id) {
            Ok(bound) => bound,
            Err(error) => {
                self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::pid_claim(
                    DescramblerDiagnosticPhase::AddPid,
                    descrambler_id,
                    None,
                    pid,
                    -1,
                    error.clone(),
                ));
                return Err(error);
            }
        };
        if self.registry.descrambler_pid_claimed_by_other(
            DescramblerRuntimeId(descrambler_id),
            demux_id,
            demux_generation,
            pid,
        ) {
            let error = HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "descrambler PID is already claimed by another session",
            );
            self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::pid_claim(
                DescramblerDiagnosticPhase::AddPid,
                descrambler_id,
                Some(demux_id),
                pid,
                -1,
                error.clone(),
            ));
            return Err(error);
        }
        let claim = match DescramblerPidClaim::from_demux_input(pid, demux_id, demux_generation) {
            Ok(claim) => claim,
            Err(error) => {
                let hal_error = descrambler_pid_claim_error_to_hal(error);
                self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::pid_claim(
                    DescramblerDiagnosticPhase::AddPid,
                    descrambler_id,
                    Some(demux_id),
                    pid,
                    -1,
                    hal_error.clone(),
                ));
                return Err(hal_error);
            }
        };
        let add_result = {
            let runtime = self.descrambler_runtime_mut(descrambler_id)?;
            add_pid_claim_with_session_txn(runtime.session_mut(), claim)
                .map_err(|failure| descrambler_session_failure_to_hal(failure.kind))
        };
        if let Err(error) = add_result {
            self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::pid_claim(
                DescramblerDiagnosticPhase::AddPid,
                descrambler_id,
                Some(demux_id),
                pid,
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
        let (demux_id, demux_generation) = match self.descrambler_bound_demux(descrambler_id) {
            Ok(bound) => bound,
            Err(error) => {
                self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::pid_claim(
                    DescramblerDiagnosticPhase::RemovePid,
                    descrambler_id,
                    None,
                    pid,
                    -1,
                    error.clone(),
                ));
                return Err(error);
            }
        };
        let claim = match DescramblerPidClaim::from_demux_input(pid, demux_id, demux_generation) {
            Ok(claim) => claim,
            Err(error) => {
                let hal_error = descrambler_pid_claim_error_to_hal(error);
                self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::pid_claim(
                    DescramblerDiagnosticPhase::RemovePid,
                    descrambler_id,
                    Some(demux_id),
                    pid,
                    -1,
                    hal_error.clone(),
                ));
                return Err(hal_error);
            }
        };
        let remove_result = {
            let runtime = self.descrambler_runtime_mut(descrambler_id)?;
            remove_pid_claim_with_session_txn(runtime.session_mut(), claim)
                .map_err(|failure| descrambler_session_failure_to_hal(failure.kind))
        };
        if let Err(error) = remove_result {
            self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::pid_claim(
                DescramblerDiagnosticPhase::RemovePid,
                descrambler_id,
                Some(demux_id),
                pid,
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
        let (demux_id, demux_generation) = match self.descrambler_bound_demux(descrambler_id) {
            Ok(bound) => bound,
            Err(error) => {
                self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::pid_claim(
                    DescramblerDiagnosticPhase::AddPid,
                    descrambler_id,
                    None,
                    pid,
                    source_filter_id,
                    error.clone(),
                ));
                return Err(error);
            }
        };
        let source_generation = match self.validate_descrambler_source_filter(
            demux_id,
            demux_generation,
            source_filter_id,
            pid,
        ) {
            Ok(source_generation) => source_generation,
            Err(error) => {
                self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::pid_claim(
                    DescramblerDiagnosticPhase::AddPid,
                    descrambler_id,
                    Some(demux_id),
                    pid,
                    source_filter_id,
                    error.clone(),
                ));
                return Err(error);
            }
        };
        if self.registry.descrambler_pid_claimed_by_other(
            DescramblerRuntimeId(descrambler_id),
            demux_id,
            demux_generation,
            pid,
        ) {
            let error = HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "descrambler PID is already claimed by another session",
            );
            self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::pid_claim(
                DescramblerDiagnosticPhase::AddPid,
                descrambler_id,
                Some(demux_id),
                pid,
                source_filter_id,
                error.clone(),
            ));
            return Err(error);
        }
        let claim =
            match DescramblerPidClaim::from_source_filter(pid, source_filter_id, source_generation)
            {
                Ok(claim) => claim,
                Err(error) => {
                    let hal_error = descrambler_pid_claim_error_to_hal(error);
                    self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::pid_claim(
                        DescramblerDiagnosticPhase::AddPid,
                        descrambler_id,
                        Some(demux_id),
                        pid,
                        source_filter_id,
                        hal_error.clone(),
                    ));
                    return Err(hal_error);
                }
            };
        let add_result = {
            let runtime = self.descrambler_runtime_mut(descrambler_id)?;
            add_pid_claim_with_session_txn(runtime.session_mut(), claim)
                .map_err(|failure| descrambler_session_failure_to_hal(failure.kind))
        };
        if let Err(error) = add_result {
            self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::pid_claim(
                DescramblerDiagnosticPhase::AddPid,
                descrambler_id,
                Some(demux_id),
                pid,
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
        let (demux_id, demux_generation) = match self.descrambler_bound_demux(descrambler_id) {
            Ok(bound) => bound,
            Err(error) => {
                self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::pid_claim(
                    DescramblerDiagnosticPhase::RemovePid,
                    descrambler_id,
                    None,
                    pid,
                    source_filter_id,
                    error.clone(),
                ));
                return Err(error);
            }
        };
        let source_generation = match self.validate_descrambler_source_filter(
            demux_id,
            demux_generation,
            source_filter_id,
            pid,
        ) {
            Ok(source_generation) => source_generation,
            Err(error) => {
                self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::pid_claim(
                    DescramblerDiagnosticPhase::RemovePid,
                    descrambler_id,
                    Some(demux_id),
                    pid,
                    source_filter_id,
                    error.clone(),
                ));
                return Err(error);
            }
        };
        let claim =
            match DescramblerPidClaim::from_source_filter(pid, source_filter_id, source_generation)
            {
                Ok(claim) => claim,
                Err(error) => {
                    let hal_error = descrambler_pid_claim_error_to_hal(error);
                    self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::pid_claim(
                        DescramblerDiagnosticPhase::RemovePid,
                        descrambler_id,
                        Some(demux_id),
                        pid,
                        source_filter_id,
                        hal_error.clone(),
                    ));
                    return Err(hal_error);
                }
            };
        let stale_source_generation = {
            let runtime = self.descrambler_runtime_mut(descrambler_id)?;
            runtime.session().pid_claims().iter().any(|stored| {
                stored.pid().0 == pid
                    && stored
                        .source_filter_ref()
                        .map(|source| {
                            source.filter_id == source_filter_id
                                && source.generation != source_generation
                        })
                        .unwrap_or(false)
            })
        };
        if stale_source_generation {
            let error = HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "source filter generation changed before PID removal",
            );
            self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::pid_claim(
                DescramblerDiagnosticPhase::RemovePid,
                descrambler_id,
                Some(demux_id),
                pid,
                source_filter_id,
                error.clone(),
            ));
            return Err(error);
        }
        let remove_result = {
            let runtime = self.descrambler_runtime_mut(descrambler_id)?;
            remove_pid_claim_with_session_txn(runtime.session_mut(), claim)
                .map_err(|failure| descrambler_session_failure_to_hal(failure.kind))
        };
        if let Err(error) = remove_result {
            self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::pid_claim(
                DescramblerDiagnosticPhase::RemovePid,
                descrambler_id,
                Some(demux_id),
                pid,
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
        let mut cleanup_collector = FirstErrorCollector::new();
        let old_token = self
            .registry
            .descrambler_runtime(DescramblerRuntimeId(id))
            .and_then(|runtime| runtime.session().key_token().cloned());
        if let Some(old_token) = old_token {
            if let Err(error) = self
                .registry
                .descrambler_key_table_mut()
                .release(&old_token)
                .map_err(descrambler_key_release_error_to_hal)
            {
                self.record_descrambler_diagnostic(
                    DescramblerDiagnosticRecord::cleanup_release_failed(id, error.clone()),
                );
                cleanup_collector.push_error(error);
            }
        }
        if let Some(runtime) = self
            .registry
            .descrambler_runtime_mut(DescramblerRuntimeId(id))
        {
            let cleanup_report = cleanup_all_with_session_txn(runtime.session_mut());
            if let Some(failure) = cleanup_report.failure() {
                let error = descrambler_session_failure_to_hal(failure.kind);
                self.record_descrambler_diagnostic(
                    DescramblerDiagnosticRecord::cleanup_release_failed(id, error.clone()),
                );
                cleanup_collector.push_error(error);
            }
        }
        cleanup_collector.into_result()
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

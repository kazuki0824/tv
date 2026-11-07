use std::collections::BTreeMap;

use maleicacid_tuner_hal2_domain_request::{AidlObjectGeneration, AidlObjectId, AidlObjectKind};
use maleicacid_tuner_hal2_resource_ledger::{CleanupStep, LedgerGeneration, LedgerId};

use crate::object_close_txn::{
    CloseCleanupAttemptKey, CloseCleanupAttemptOutcome, CloseCleanupAuthorityKey, ObjectCloseTxn,
    ObjectCloseTxnStateError,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeOwnerRelation {
    Root,
    Frontend {
        frontend: AidlObjectId,
        generation: AidlObjectGeneration,
    },
    Demux {
        demux: AidlObjectId,
        generation: AidlObjectGeneration,
    },
    Filter {
        filter: AidlObjectId,
        generation: AidlObjectGeneration,
    },
    Dvr {
        dvr: AidlObjectId,
        generation: AidlObjectGeneration,
    },
    Descrambler {
        descrambler: AidlObjectId,
        generation: AidlObjectGeneration,
    },
    Lnb {
        lnb: AidlObjectId,
        generation: AidlObjectGeneration,
    },
}

impl RuntimeOwnerRelation {
    pub const fn referenced_object(
        self,
    ) -> Option<(AidlObjectKind, AidlObjectId, AidlObjectGeneration)> {
        match self {
            Self::Root => None,
            Self::Frontend {
                frontend,
                generation,
            } => Some((AidlObjectKind::Frontend, frontend, generation)),
            Self::Demux { demux, generation } => Some((AidlObjectKind::Demux, demux, generation)),
            Self::Filter { filter, generation } => {
                Some((AidlObjectKind::Filter, filter, generation))
            }
            Self::Dvr { dvr, generation } => Some((AidlObjectKind::Dvr, dvr, generation)),
            Self::Descrambler {
                descrambler,
                generation,
            } => Some((AidlObjectKind::Descrambler, descrambler, generation)),
            Self::Lnb { lnb, generation } => Some((AidlObjectKind::Lnb, lnb, generation)),
        }
    }

    pub fn owns(self, owner_id: AidlObjectId, owner_generation: AidlObjectGeneration) -> bool {
        self.referenced_object()
            .map(|(_, id, generation)| id == owner_id && generation == owner_generation)
            .unwrap_or(false)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeObjectLifecycle {
    Prepared,
    Live,
    Closing { step: CleanupStep },
    CleanupPending { step: CleanupStep },
    Closed,
    Quarantined,
}

impl RuntimeObjectLifecycle {
    pub const fn is_live(self) -> bool {
        matches!(self, Self::Live)
    }
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Closed | Self::Quarantined)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeObjectLifecycleSnapshot {
    Prepared,
    Live,
    Closing { step: CleanupStep },
    CleanupPending { step: CleanupStep },
    Closed,
    Quarantined,
}

impl From<RuntimeObjectLifecycle> for RuntimeObjectLifecycleSnapshot {
    fn from(lifecycle: RuntimeObjectLifecycle) -> Self {
        match lifecycle {
            RuntimeObjectLifecycle::Prepared => Self::Prepared,
            RuntimeObjectLifecycle::Live => Self::Live,
            RuntimeObjectLifecycle::Closing { step } => Self::Closing { step },
            RuntimeObjectLifecycle::CleanupPending { step } => Self::CleanupPending { step },
            RuntimeObjectLifecycle::Closed => Self::Closed,
            RuntimeObjectLifecycle::Quarantined => Self::Quarantined,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeObjectDiagnosticSnapshot {
    object_kind: AidlObjectKind,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
    public_runtime_id: LedgerId,
    lifecycle: RuntimeObjectLifecycleSnapshot,
}

impl RuntimeObjectDiagnosticSnapshot {
    pub const fn object_kind(&self) -> AidlObjectKind {
        self.object_kind
    }

    pub const fn object_id(&self) -> AidlObjectId {
        self.object_id
    }

    pub const fn generation(&self) -> AidlObjectGeneration {
        self.generation
    }

    pub const fn public_runtime_id(&self) -> LedgerId {
        self.public_runtime_id
    }

    pub const fn lifecycle(&self) -> RuntimeObjectLifecycleSnapshot {
        self.lifecycle
    }
}

impl From<&RuntimeObjectEntry> for RuntimeObjectDiagnosticSnapshot {
    fn from(entry: &RuntimeObjectEntry) -> Self {
        Self {
            object_kind: entry.object_kind,
            object_id: entry.object_id,
            generation: entry.generation,
            public_runtime_id: entry.ledger_id,
            lifecycle: entry.lifecycle.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeObjectEntry {
    pub(crate) last_reference_ended: bool,
    pub(crate) object_kind: AidlObjectKind,
    pub(crate) object_id: AidlObjectId,
    pub(crate) generation: AidlObjectGeneration,
    pub(crate) ledger_id: LedgerId,
    pub(crate) ledger_generation: LedgerGeneration,
    pub(crate) owner: RuntimeOwnerRelation,
    pub(crate) lifecycle: RuntimeObjectLifecycle,
}

impl RuntimeObjectEntry {
    pub const fn object_kind(&self) -> AidlObjectKind {
        self.object_kind
    }

    pub const fn object_id(&self) -> AidlObjectId {
        self.object_id
    }

    pub const fn generation(&self) -> AidlObjectGeneration {
        self.generation
    }

    pub const fn public_runtime_id(&self) -> LedgerId {
        self.ledger_id
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeObjectTableError {
    DuplicateObjectId {
        object_id: AidlObjectId,
        existing_kind: AidlObjectKind,
        attempted_kind: AidlObjectKind,
    },
    DuplicateRuntimeBinding {
        object_kind: AidlObjectKind,
        runtime_id: LedgerId,
    },
    MissingObject {
        object_id: AidlObjectId,
    },
    ObjectKindMismatch {
        object_id: AidlObjectId,
        expected: AidlObjectKind,
        actual: AidlObjectKind,
    },
    GenerationMismatch {
        object_id: AidlObjectId,
        expected: AidlObjectGeneration,
        actual: AidlObjectGeneration,
    },
    InvalidOwner {
        object_id: AidlObjectId,
        expected: RuntimeOwnerRelation,
        actual: RuntimeOwnerRelation,
    },
    MissingOwner {
        object_id: AidlObjectId,
        owner_id: AidlObjectId,
        owner_kind: AidlObjectKind,
    },
    OwnerGenerationMismatch {
        object_id: AidlObjectId,
        owner_id: AidlObjectId,
        expected: AidlObjectGeneration,
        actual: AidlObjectGeneration,
    },
    OwnerKindMismatch {
        object_id: AidlObjectId,
        owner_id: AidlObjectId,
        expected: AidlObjectKind,
        actual: AidlObjectKind,
    },
    OwnerNotLive {
        object_id: AidlObjectId,
        owner_id: AidlObjectId,
        lifecycle: RuntimeObjectLifecycle,
    },
    InvalidLifecycle {
        object_id: AidlObjectId,
        lifecycle: RuntimeObjectLifecycle,
    },
    UnsupportedObjectKind {
        object_kind: AidlObjectKind,
    },
    GenerationOverflow,
    ObjectIdOverflow,
}

#[derive(Debug)]
pub struct RuntimeObjectTable {
    entries: BTreeMap<AidlObjectId, RuntimeObjectEntry>,
    object_close_txn: ObjectCloseTxn,
}

impl Default for RuntimeObjectTable {
    fn default() -> Self {
        Self {
            entries: BTreeMap::new(),
            object_close_txn: ObjectCloseTxn::new(),
        }
    }
}

impl RuntimeObjectTable {
    fn insert_with_lifecycle(
        &mut self,
        mut entry: RuntimeObjectEntry,
        lifecycle: RuntimeObjectLifecycle,
    ) -> Result<(), RuntimeObjectTableError> {
        if let Some(existing) = self.entries.get(&entry.object_id) {
            if !existing.lifecycle.is_terminal()
                || self
                    .object_close_txn
                    .has_outstanding_obligation(existing.object_id, existing.generation)
            {
                return Err(RuntimeObjectTableError::DuplicateObjectId {
                    object_id: entry.object_id,
                    existing_kind: existing.object_kind,
                    attempted_kind: entry.object_kind,
                });
            }
        }
        if self.entries.values().any(|existing| {
            existing.object_kind == entry.object_kind
                && existing.ledger_id == entry.ledger_id
                && (!existing.lifecycle.is_terminal()
                    || self
                        .object_close_txn
                        .has_outstanding_obligation(existing.object_id, existing.generation))
        }) {
            return Err(RuntimeObjectTableError::DuplicateRuntimeBinding {
                object_kind: entry.object_kind,
                runtime_id: entry.ledger_id,
            });
        }
        self.ensure_owner_live_for(entry.object_id, entry.owner)?;
        self.object_close_txn.clear_obligation(entry.object_id);
        entry.lifecycle = lifecycle;
        self.entries.insert(entry.object_id, entry);
        Ok(())
    }

    pub fn insert(&mut self, entry: RuntimeObjectEntry) -> Result<(), RuntimeObjectTableError> {
        self.insert_with_lifecycle(entry, RuntimeObjectLifecycle::Live)
    }

    pub(crate) fn insert_prepared(
        &mut self,
        entry: RuntimeObjectEntry,
    ) -> Result<(), RuntimeObjectTableError> {
        self.insert_with_lifecycle(entry, RuntimeObjectLifecycle::Prepared)
    }

    pub(crate) fn commit_prepared(
        &mut self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> Result<RuntimeObjectEntry, RuntimeObjectTableError> {
        let entry = self.entry_mut_checked_any(object_id, generation)?;
        if entry.lifecycle != RuntimeObjectLifecycle::Prepared {
            return Err(RuntimeObjectTableError::InvalidLifecycle {
                object_id,
                lifecycle: entry.lifecycle,
            });
        }
        entry.lifecycle = RuntimeObjectLifecycle::Live;
        Ok(entry.clone())
    }

    pub fn remove(
        &mut self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> Result<RuntimeObjectEntry, RuntimeObjectTableError> {
        self.entry_checked(object_id, generation)?;
        self.object_close_txn.clear_obligation(object_id);
        self.entries
            .remove(&object_id)
            .ok_or(RuntimeObjectTableError::MissingObject { object_id })
    }

    #[cfg(test)]
    pub fn begin_close_cascade(
        &mut self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        step: CleanupStep,
    ) -> Result<Vec<RuntimeObjectEntry>, RuntimeObjectTableError> {
        let root = self.entry_checked_any(object_id, generation)?;
        match root.lifecycle {
            RuntimeObjectLifecycle::Live | RuntimeObjectLifecycle::CleanupPending { .. } => {}
            lifecycle => {
                return Err(RuntimeObjectTableError::InvalidLifecycle {
                    object_id,
                    lifecycle,
                });
            }
        }
        let mut targets = self.descendant_object_keys(object_id, generation);
        targets.push((object_id, generation));
        let mut changed = Vec::with_capacity(targets.len());
        for (target_id, target_generation) in targets {
            let entry = self.entry_mut_checked_any(target_id, target_generation)?;
            match entry.lifecycle {
                RuntimeObjectLifecycle::Live | RuntimeObjectLifecycle::CleanupPending { .. } => {
                    entry.lifecycle = RuntimeObjectLifecycle::Closing { step };
                    changed.push(entry.clone());
                }
                RuntimeObjectLifecycle::Closing { .. } => {
                    changed.push(entry.clone());
                }
                RuntimeObjectLifecycle::Closed | RuntimeObjectLifecycle::Quarantined => {}
                RuntimeObjectLifecycle::Prepared => {
                    return Err(RuntimeObjectTableError::InvalidLifecycle {
                        object_id: target_id,
                        lifecycle: entry.lifecycle,
                    });
                }
            }
        }
        Ok(changed)
    }

    pub(crate) fn begin_close_cascade_with_cleanup_authority(
        &mut self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        step: CleanupStep,
    ) -> Result<(Vec<RuntimeObjectEntry>, CloseCleanupAuthorityKey), RuntimeObjectTableError> {
        let root_lifecycle = self.entry_checked_any(object_id, generation)?.lifecycle;
        match root_lifecycle {
            RuntimeObjectLifecycle::Live
            | RuntimeObjectLifecycle::Closing { .. }
            | RuntimeObjectLifecycle::CleanupPending { .. } => {}
            lifecycle => {
                return Err(RuntimeObjectTableError::InvalidLifecycle {
                    object_id,
                    lifecycle,
                });
            }
        }
        let mut targets = self.descendant_object_keys(object_id, generation);
        targets.push((object_id, generation));
        let mut changed = Vec::with_capacity(targets.len());
        for (target_id, target_generation) in targets {
            let mut entry = self
                .entry_checked_any(target_id, target_generation)?
                .clone();
            let is_root = target_id == object_id && target_generation == generation;
            match entry.lifecycle {
                RuntimeObjectLifecycle::Live
                | RuntimeObjectLifecycle::Closing { .. }
                | RuntimeObjectLifecycle::CleanupPending { .. } => {
                    entry.lifecycle = RuntimeObjectLifecycle::Closing { step };
                    changed.push(entry);
                }
                RuntimeObjectLifecycle::Closed | RuntimeObjectLifecycle::Quarantined
                    if !is_root => {}
                lifecycle => {
                    return Err(RuntimeObjectTableError::InvalidLifecycle {
                        object_id: target_id,
                        lifecycle,
                    });
                }
            }
        }
        let authority = self
            .object_close_txn
            .issue_cleanup_authority(object_id, generation)
            .map_err(|error| Self::close_txn_error(object_id, root_lifecycle, error))?;
        for entry in &changed {
            self.entries.insert(entry.object_id, entry.clone());
        }
        Ok((changed, authority))
    }

    pub(crate) fn begin_close_cleanup_attempt(
        &mut self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        authority: CloseCleanupAuthorityKey,
    ) -> Result<CloseCleanupAttemptKey, RuntimeObjectTableError> {
        let lifecycle = self.entry_checked_any(object_id, generation)?.lifecycle;
        if !matches!(
            lifecycle,
            RuntimeObjectLifecycle::Closing { .. } | RuntimeObjectLifecycle::CleanupPending { .. }
        ) {
            return Err(RuntimeObjectTableError::InvalidLifecycle {
                object_id,
                lifecycle,
            });
        }
        self.object_close_txn
            .begin_cleanup_attempt(object_id, generation, authority)
            .map_err(|error| Self::close_txn_error(object_id, lifecycle, error))
    }

    pub(crate) fn finish_close_cleanup_attempt(
        &mut self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        attempt: CloseCleanupAttemptKey,
        outcome: CloseCleanupAttemptOutcome,
    ) -> Result<Vec<RuntimeObjectEntry>, RuntimeObjectTableError> {
        let lifecycle = self.entry_checked_any(object_id, generation)?.lifecycle;
        if !self
            .object_close_txn
            .attempt_is_current(object_id, generation, attempt)
        {
            return Err(RuntimeObjectTableError::InvalidLifecycle {
                object_id,
                lifecycle,
            });
        }

        match outcome {
            CloseCleanupAttemptOutcome::Complete => {
                let changed = self.commit_close_cascade(object_id, generation)?;
                self.object_close_txn
                    .finish_cleanup_attempt(object_id, generation, attempt, true)
                    .map_err(|error| Self::close_txn_error(object_id, lifecycle, error))?;
                Ok(changed)
            }
            CloseCleanupAttemptOutcome::Pending { step } => {
                let changed = self.mark_cleanup_failed_cascade(object_id, generation, step)?;
                self.object_close_txn
                    .finish_cleanup_attempt(object_id, generation, attempt, false)
                    .map_err(|error| Self::close_txn_error(object_id, lifecycle, error))?;
                Ok(changed)
            }
        }
    }

    pub fn mark_cleanup_failed_cascade(
        &mut self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        step: CleanupStep,
    ) -> Result<Vec<RuntimeObjectEntry>, RuntimeObjectTableError> {
        self.ensure_root_ready_for_close_finalization(object_id, generation)?;
        let mut targets = self.descendant_object_keys(object_id, generation);
        targets.push((object_id, generation));
        let mut changed = Vec::with_capacity(targets.len());
        for (target_id, target_generation) in targets {
            let mut entry = self
                .entry_checked_any(target_id, target_generation)?
                .clone();
            let is_root = target_id == object_id && target_generation == generation;
            match entry.lifecycle {
                RuntimeObjectLifecycle::Closing { .. }
                | RuntimeObjectLifecycle::CleanupPending { .. } => {
                    entry.lifecycle = RuntimeObjectLifecycle::CleanupPending { step };
                    changed.push(entry);
                }
                RuntimeObjectLifecycle::Closed | RuntimeObjectLifecycle::Quarantined
                    if !is_root => {}
                lifecycle => {
                    return Err(RuntimeObjectTableError::InvalidLifecycle {
                        object_id: target_id,
                        lifecycle,
                    });
                }
            }
        }
        for entry in &changed {
            self.entries.insert(entry.object_id, entry.clone());
        }
        Ok(changed)
    }

    pub fn close_cascade_entries(
        &self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> Result<Vec<RuntimeObjectEntry>, RuntimeObjectTableError> {
        self.ensure_root_ready_for_close_finalization(object_id, generation)?;
        let mut targets = self.descendant_object_keys(object_id, generation);
        targets.push((object_id, generation));
        let mut entries = Vec::with_capacity(targets.len());
        for (target_id, target_generation) in targets {
            let entry = self.entry_checked_any(target_id, target_generation)?;
            let is_root = target_id == object_id && target_generation == generation;
            match entry.lifecycle {
                RuntimeObjectLifecycle::Closing { .. }
                | RuntimeObjectLifecycle::CleanupPending { .. } => entries.push(entry.clone()),
                RuntimeObjectLifecycle::Closed | RuntimeObjectLifecycle::Quarantined
                    if !is_root => {}
                lifecycle => {
                    return Err(RuntimeObjectTableError::InvalidLifecycle {
                        object_id: target_id,
                        lifecycle,
                    });
                }
            }
        }
        Ok(entries)
    }

    pub fn commit_close_cascade(
        &mut self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> Result<Vec<RuntimeObjectEntry>, RuntimeObjectTableError> {
        self.ensure_root_ready_for_close_finalization(object_id, generation)?;
        let mut targets = self.descendant_object_keys(object_id, generation);
        targets.push((object_id, generation));
        let mut changed = Vec::with_capacity(targets.len());
        for (target_id, target_generation) in targets {
            let mut entry = self
                .entry_checked_any(target_id, target_generation)?
                .clone();
            let is_root = target_id == object_id && target_generation == generation;
            match entry.lifecycle {
                RuntimeObjectLifecycle::Closing { .. }
                | RuntimeObjectLifecycle::CleanupPending { .. } => {
                    entry.lifecycle = RuntimeObjectLifecycle::Closed;
                    changed.push(entry);
                }
                RuntimeObjectLifecycle::Closed | RuntimeObjectLifecycle::Quarantined
                    if !is_root => {}
                lifecycle => {
                    return Err(RuntimeObjectTableError::InvalidLifecycle {
                        object_id: target_id,
                        lifecycle,
                    });
                }
            }
        }
        for entry in &changed {
            self.entries.insert(entry.object_id, entry.clone());
        }
        Ok(changed)
    }

    /// rootと子孫のどれかに既存ObjectCloseTxnの実行中cleanupがあるかを判定する。
    /// drop-leak隔離はこの検査を通過した場合だけ外部副作用を開始できる。
    pub(crate) fn has_active_cleanup_attempt_in_cascade(
        &self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> Result<bool, RuntimeObjectTableError> {
        self.entry_checked_any(object_id, generation)?;
        let mut targets = self.descendant_object_keys(object_id, generation);
        targets.push((object_id, generation));
        // 親ownerのcloseも子を物理cleanupするため、対象から親へ遡る。
        // 不正な親循環で停止しないよう登録object数で探索を制限する。
        let mut parent = self.entry_checked_any(object_id, generation)?.owner;
        for _ in 0..self.entries.len() {
            let Some((_, parent_id, parent_generation)) = parent.referenced_object() else {
                break;
            };
            targets.push((parent_id, parent_generation));
            parent = self.entry_checked_any(parent_id, parent_generation)?.owner;
        }
        Ok(targets
            .into_iter()
            .any(|(id, gen)| self.object_close_txn.has_active_cleanup_attempt(id, gen)))
    }

    pub(crate) fn has_related_outstanding_cleanup(
        &self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> Result<bool, RuntimeObjectTableError> {
        let mut parent = self.entry_checked_any(object_id, generation)?.owner;
        if self
            .descendant_object_keys(object_id, generation)
            .into_iter()
            .any(|(id, gen)| self.object_close_txn.has_outstanding_obligation(id, gen))
        {
            return Ok(true);
        }
        // 先行して権限発行済みの親closeも、同じ子資源を回収できる。
        for _ in 0..self.entries.len() {
            let Some((_, id, gen)) = parent.referenced_object() else {
                break;
            };
            if self.object_close_txn.has_outstanding_obligation(id, gen) {
                return Ok(true);
            }
            parent = self.entry_checked_any(id, gen)?.owner;
        }
        Ok(false)
    }

    /// 対象・子孫・祖先の既存cleanup正本を世代付きで返す。子孫を先に
    /// 列挙することで、子の独立した権限を親の物理cleanupで上書きしない。
    pub(crate) fn related_cleanup_owners(
        &self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> Result<Vec<(RuntimeObjectEntry, bool)>, RuntimeObjectTableError> {
        let mut targets = self.descendant_object_keys(object_id, generation);
        targets.push((object_id, generation));
        let mut parent = self.entry_checked_any(object_id, generation)?.owner;
        for _ in 0..self.entries.len() {
            let Some((_, id, gen)) = parent.referenced_object() else {
                break;
            };
            let entry = self.entry_checked_any(id, gen)?;
            targets.push((id, gen));
            parent = entry.owner;
        }
        let mut owners = Vec::new();
        for (id, gen) in targets {
            let entry = self.entry_checked_any(id, gen)?;
            if self.object_close_txn.has_outstanding_obligation(id, gen)
                && !owners.iter().any(|(known, _): &(RuntimeObjectEntry, bool)| {
                    known.object_id == id && known.generation == gen
                })
            {
                owners.push((
                    entry.clone(),
                    self.object_close_txn.has_active_cleanup_attempt(id, gen),
                ));
            }
        }
        Ok(owners)
    }

    /// 隔離対象を変更せず全件検証する。後続の権限発行が失敗しても状態を変えない。
    pub(crate) fn quarantine_candidate_entries(
        &self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> Result<Vec<RuntimeObjectEntry>, RuntimeObjectTableError> {
        let root = self.entry_checked_any(object_id, generation)?;
        if matches!(
            root.lifecycle,
            RuntimeObjectLifecycle::Closed | RuntimeObjectLifecycle::Quarantined
        ) {
            return Err(RuntimeObjectTableError::InvalidLifecycle {
                object_id,
                lifecycle: root.lifecycle,
            });
        }
        let mut targets = self.descendant_object_keys(object_id, generation);
        targets.push((object_id, generation));
        let mut entries = Vec::with_capacity(targets.len());
        for (id, gen) in targets {
            let entry = self.entry_checked_any(id, gen)?;
            if !matches!(
                entry.lifecycle,
                RuntimeObjectLifecycle::Closed | RuntimeObjectLifecycle::Quarantined
            ) {
                entries.push(entry.clone());
            }
        }
        Ok(entries)
    }

    pub(crate) fn begin_quarantine_cleanup(
        &mut self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        notifier_candidates: impl IntoIterator<Item = (AidlObjectId, AidlObjectGeneration)>,
    ) -> Result<Vec<RuntimeObjectEntry>, RuntimeObjectTableError> {
        // 前検証・権限発行・lifecycle確定を同一runtime lockで実行する。
        let entries = self.quarantine_candidate_entries(object_id, generation)?;
        let lifecycle = self.entry_checked_any(object_id, generation)?.lifecycle;
        // 子objectの独立したcleanup権限を上書きしてはならない。
        for (child_id, child_generation) in self.descendant_object_keys(object_id, generation) {
            if self
                .object_close_txn
                .has_outstanding_obligation(child_id, child_generation)
            {
                return Err(RuntimeObjectTableError::InvalidLifecycle {
                    object_id: child_id,
                    lifecycle: self
                        .entry_checked_any(child_id, child_generation)?
                        .lifecycle,
                });
            }
        }
        // 識別子枯渇・先行権限競合による失敗を、破壊的mutationより先に確定する。
        self.object_close_txn
            .begin_quarantined_cleanup(object_id, generation, notifier_candidates)
            .map_err(|error| Self::close_txn_error(object_id, lifecycle, error))?;
        // 全entryを検証済みのため、ここからは失敗するlookupを行わない。
        let mut changed = Vec::with_capacity(entries.len());
        for mut entry in entries {
            entry.lifecycle = RuntimeObjectLifecycle::Quarantined;
            self.entries.insert(entry.object_id, entry.clone());
            changed.push(entry);
        }
        Ok(changed)
    }

    fn close_completed_quarantine_cascade(
        &mut self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> Result<(), RuntimeObjectTableError> {
        let mut keys = self.descendant_object_keys(object_id, generation);
        keys.push((object_id, generation));
        // lifecycle変更前に対象世代を全件検証する。
        for (id, gen) in &keys {
            let entry = self.entry_checked_any(*id, *gen)?;
            if !matches!(
                entry.lifecycle,
                RuntimeObjectLifecycle::Quarantined | RuntimeObjectLifecycle::Closed
            ) {
                return Err(RuntimeObjectTableError::InvalidLifecycle {
                    object_id: *id,
                    lifecycle: entry.lifecycle,
                });
            }
        }
        for (id, gen) in keys {
            let entry = self.entry_mut_checked_any(id, gen)?;
            if entry.lifecycle == RuntimeObjectLifecycle::Quarantined {
                entry.lifecycle = RuntimeObjectLifecycle::Closed;
            }
        }
        Ok(())
    }

    pub(crate) fn finish_quarantined_cleanup_report(
        &mut self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        pending: &[(AidlObjectId, AidlObjectGeneration)],
    ) -> Result<(), RuntimeObjectTableError> {
        let lifecycle = self.entry_checked_any(object_id, generation)?.lifecycle;
        if lifecycle != RuntimeObjectLifecycle::Quarantined {
            return Err(RuntimeObjectTableError::InvalidLifecycle {
                object_id,
                lifecycle,
            });
        }
        let completed = self
            .object_close_txn
            .accept_quarantine_cleanup_report(object_id, generation, pending)
            .map_err(|error| Self::close_txn_error(object_id, lifecycle, error))?;
        if completed {
            self.close_completed_quarantine_cascade(object_id, generation)?;
        }
        Ok(())
    }

    pub(crate) fn notify_supervised_dvr_joined(
        &mut self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> Result<(), RuntimeObjectTableError> {
        let finished = self
            .object_close_txn
            .notifier_joined((object_id, generation));
        for (root_id, root_generation) in finished {
            self.close_completed_quarantine_cascade(root_id, root_generation)?;
        }
        Ok(())
    }

    pub(crate) fn quarantine_cleanup_is_managed(
        &self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> bool {
        self.object_close_txn
            .has_managed_quarantine(object_id, generation)
    }

    pub fn quarantine_cascade(
        &mut self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> Result<Vec<RuntimeObjectEntry>, RuntimeObjectTableError> {
        self.entry_checked_any(object_id, generation)?;
        let mut targets = self.descendant_object_keys(object_id, generation);
        targets.push((object_id, generation));
        let mut changed = Vec::with_capacity(targets.len());
        for (target_id, target_generation) in targets {
            if let Some(entry) =
                self.quarantine_one_if_live_or_nonterminal(target_id, target_generation)?
            {
                changed.push(entry);
            }
        }
        Ok(changed)
    }

    pub(crate) fn mark_last_reference_ended(
        &mut self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> Result<(), RuntimeObjectTableError> {
        self.entry_mut_checked_any(object_id, generation)?
            .last_reference_ended = true;
        Ok(())
    }

    pub fn entry(&self, object_id: AidlObjectId) -> Option<&RuntimeObjectEntry> {
        self.entries.get(&object_id)
    }

    pub fn entry_checked(
        &self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> Result<&RuntimeObjectEntry, RuntimeObjectTableError> {
        let entry = self.entry_checked_any(object_id, generation)?;
        if !entry.lifecycle.is_live() {
            return Err(RuntimeObjectTableError::InvalidLifecycle {
                object_id,
                lifecycle: entry.lifecycle,
            });
        }
        Ok(entry)
    }

    pub fn entry_for_kind(
        &self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        expected_kind: AidlObjectKind,
    ) -> Result<&RuntimeObjectEntry, RuntimeObjectTableError> {
        let entry = self.entry_checked(object_id, generation)?;
        if entry.object_kind != expected_kind {
            return Err(RuntimeObjectTableError::ObjectKindMismatch {
                object_id,
                expected: expected_kind,
                actual: entry.object_kind,
            });
        }
        self.ensure_owner_live_for(object_id, entry.owner)?;
        Ok(entry)
    }

    pub fn live_entry_for_runtime(
        &self,
        kind: AidlObjectKind,
        ledger_id: LedgerId,
    ) -> Option<RuntimeObjectEntry> {
        self.entries
            .values()
            .find(|entry| {
                entry.object_kind == kind
                    && entry.ledger_id == ledger_id
                    && entry.lifecycle.is_live()
            })
            .cloned()
    }

    pub(crate) fn active_entry_for_runtime(
        &self,
        kind: AidlObjectKind,
        ledger_id: LedgerId,
    ) -> Option<RuntimeObjectEntry> {
        self.entries
            .values()
            .find(|entry| {
                entry.object_kind == kind
                    && entry.ledger_id == ledger_id
                    && !matches!(entry.lifecycle, RuntimeObjectLifecycle::Closed)
            })
            .cloned()
    }

    pub(crate) fn active_diagnostic_snapshots(&self) -> Vec<RuntimeObjectDiagnosticSnapshot> {
        self.entries
            .values()
            .filter(|entry| !matches!(entry.lifecycle, RuntimeObjectLifecycle::Closed))
            .map(RuntimeObjectDiagnosticSnapshot::from)
            .collect()
    }

    pub fn active_public_runtime_ids(&self, kind: AidlObjectKind) -> Vec<LedgerId> {
        self.entries
            .values()
            .filter(|entry| {
                entry.object_kind == kind
                    && !matches!(entry.lifecycle, RuntimeObjectLifecycle::Closed)
            })
            .map(|entry| entry.ledger_id)
            .collect()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.object_close_txn.clear();
    }

    fn close_txn_error(
        object_id: AidlObjectId,
        lifecycle: RuntimeObjectLifecycle,
        error: ObjectCloseTxnStateError,
    ) -> RuntimeObjectTableError {
        match error {
            ObjectCloseTxnStateError::InvalidAuthority => {
                RuntimeObjectTableError::InvalidLifecycle {
                    object_id,
                    lifecycle,
                }
            }
            ObjectCloseTxnStateError::IdentifierExhausted => {
                RuntimeObjectTableError::GenerationOverflow
            }
        }
    }

    fn ensure_owner_live_for(
        &self,
        object_id: AidlObjectId,
        owner: RuntimeOwnerRelation,
    ) -> Result<(), RuntimeObjectTableError> {
        let Some((owner_kind, owner_id, owner_generation)) = owner.referenced_object() else {
            return Ok(());
        };
        let owner_entry =
            self.entries
                .get(&owner_id)
                .ok_or(RuntimeObjectTableError::MissingOwner {
                    object_id,
                    owner_id,
                    owner_kind,
                })?;
        if owner_entry.object_kind != owner_kind {
            return Err(RuntimeObjectTableError::OwnerKindMismatch {
                object_id,
                owner_id,
                expected: owner_kind,
                actual: owner_entry.object_kind,
            });
        }
        if owner_entry.generation != owner_generation {
            return Err(RuntimeObjectTableError::OwnerGenerationMismatch {
                object_id,
                owner_id,
                expected: owner_entry.generation,
                actual: owner_generation,
            });
        }
        if !owner_entry.lifecycle.is_live() {
            return Err(RuntimeObjectTableError::OwnerNotLive {
                object_id,
                owner_id,
                lifecycle: owner_entry.lifecycle,
            });
        }
        self.ensure_owner_live_for(owner_id, owner_entry.owner)
    }

    fn descendant_object_keys(
        &self,
        owner_id: AidlObjectId,
        owner_generation: AidlObjectGeneration,
    ) -> Vec<(AidlObjectId, AidlObjectGeneration)> {
        let direct: Vec<(AidlObjectId, AidlObjectGeneration)> = self
            .entries
            .values()
            .filter(|entry| entry.owner.owns(owner_id, owner_generation))
            .map(|entry| (entry.object_id, entry.generation))
            .collect();
        let mut result = Vec::new();
        for (child_id, child_generation) in direct {
            result.extend(self.descendant_object_keys(child_id, child_generation));
            result.push((child_id, child_generation));
        }
        result
    }

    fn entry_checked_any(
        &self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> Result<&RuntimeObjectEntry, RuntimeObjectTableError> {
        let entry = self
            .entries
            .get(&object_id)
            .ok_or(RuntimeObjectTableError::MissingObject { object_id })?;
        if entry.generation != generation {
            return Err(RuntimeObjectTableError::GenerationMismatch {
                object_id,
                expected: entry.generation,
                actual: generation,
            });
        }
        Ok(entry)
    }

    fn entry_mut_checked_any(
        &mut self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> Result<&mut RuntimeObjectEntry, RuntimeObjectTableError> {
        let entry = self
            .entries
            .get_mut(&object_id)
            .ok_or(RuntimeObjectTableError::MissingObject { object_id })?;
        if entry.generation != generation {
            return Err(RuntimeObjectTableError::GenerationMismatch {
                object_id,
                expected: entry.generation,
                actual: generation,
            });
        }
        Ok(entry)
    }

    fn ensure_root_ready_for_close_finalization(
        &self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> Result<(), RuntimeObjectTableError> {
        let root = self.entry_checked_any(object_id, generation)?;
        match root.lifecycle {
            RuntimeObjectLifecycle::Closing { .. }
            | RuntimeObjectLifecycle::CleanupPending { .. } => Ok(()),
            lifecycle => Err(RuntimeObjectTableError::InvalidLifecycle {
                object_id,
                lifecycle,
            }),
        }
    }

    fn quarantine_one_if_live_or_nonterminal(
        &mut self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> Result<Option<RuntimeObjectEntry>, RuntimeObjectTableError> {
        let entry = self.entry_mut_checked_any(object_id, generation)?;
        match entry.lifecycle {
            RuntimeObjectLifecycle::Closed | RuntimeObjectLifecycle::Quarantined => Ok(None),
            _ => {
                entry.lifecycle = RuntimeObjectLifecycle::Quarantined;
                Ok(Some(entry.clone()))
            }
        }
    }
}

#[cfg(test)]
mod qg_object_lifecycle_tests {
    use super::*;
    use maleicacid_tuner_hal2_domain_request::{
        AidlObjectGeneration, AidlObjectId, AidlObjectKind,
    };
    use maleicacid_tuner_hal2_resource_ledger::{LedgerGeneration, LedgerId};

    fn entry(
        object_kind: AidlObjectKind,
        object_id: i64,
        ledger_id: i64,
        owner: RuntimeOwnerRelation,
    ) -> RuntimeObjectEntry {
        RuntimeObjectEntry {
            last_reference_ended: false,
            object_kind,
            object_id: AidlObjectId(object_id),
            generation: AidlObjectGeneration(1),
            ledger_id: LedgerId(ledger_id),
            ledger_generation: LedgerGeneration(1),
            owner,
            lifecycle: RuntimeObjectLifecycle::Live,
        }
    }

    #[test]
    fn failed_cleanup_authority_prepare_leaves_all_lifecycles_unchanged() {
        let mut table = RuntimeObjectTable::default();
        let id = AidlObjectId(991);
        let generation = AidlObjectGeneration(1);
        table
            .insert(entry(
                AidlObjectKind::Demux,
                991,
                991,
                RuntimeOwnerRelation::Root,
            ))
            .expect("root登録");
        table
            .insert(entry(
                AidlObjectKind::Filter,
                992,
                992,
                RuntimeOwnerRelation::Demux {
                    demux: id,
                    generation,
                },
            ))
            .expect("子登録");
        table.object_close_txn.exhaust_cleanup_ids_for_test();
        assert!(table.begin_quarantine_cleanup(id, generation, []).is_err());
        for target in [id, AidlObjectId(992)] {
            assert_eq!(
                table.entry(target).unwrap().lifecycle,
                RuntimeObjectLifecycle::Live,
                "権限発行に失敗した場合は隔離だけを確定してはならない"
            );
        }
        assert!(!table
            .object_close_txn
            .has_outstanding_obligation(id, generation));
    }

    #[test]
    fn issued_parent_cleanup_is_selected_for_autonomous_retry() {
        let mut table = RuntimeObjectTable::default();
        let parent = AidlObjectId(993);
        let child = AidlObjectId(994);
        let generation = AidlObjectGeneration(1);
        table
            .insert(entry(
                AidlObjectKind::Demux,
                993,
                993,
                RuntimeOwnerRelation::Root,
            ))
            .expect("親登録");
        table
            .insert(entry(
                AidlObjectKind::Filter,
                994,
                994,
                RuntimeOwnerRelation::Demux {
                    demux: parent,
                    generation,
                },
            ))
            .expect("子登録");
        let (_, authority) = table
            .begin_close_cascade_with_cleanup_authority(parent, generation, CleanupStep::StopWorker)
            .expect("物理cleanupを開始せず親の権限を発行する");
        let owners = table.related_cleanup_owners(child, generation).expect("回収先owner取得");
        let (owner, active) = owners
            .into_iter()
            .find(|(owner, _)| owner.object_id == parent)
            .expect("未開始の親ownerを発見");
        assert_eq!(owner.object_id, parent);
        assert!(!active, "未開始権限は実行中とみなしてはならない");
        table
            .begin_close_cleanup_attempt(parent, generation, authority)
            .expect("親物理cleanupを開始");
        assert!(
            table
                .related_cleanup_owners(child, generation)
                .expect("実行中を検査")
                .into_iter()
                .find(|(owner, _)| owner.object_id == parent)
                .expect("親owner")
                .1
        );
    }

    #[test]
    fn active_child_cleanup_is_visible_from_owner_cascade() {
        let mut table = RuntimeObjectTable::default();
        table
            .insert(entry(
                AidlObjectKind::Demux,
                100,
                100,
                RuntimeOwnerRelation::Root,
            ))
            .expect("親Demux登録");
        table
            .insert(entry(
                AidlObjectKind::Filter,
                101,
                101,
                RuntimeOwnerRelation::Demux {
                    demux: AidlObjectId(100),
                    generation: AidlObjectGeneration(1),
                },
            ))
            .expect("子Filter登録");
        let (_, authority) = table
            .begin_close_cascade_with_cleanup_authority(
                AidlObjectId(101),
                AidlObjectGeneration(1),
                CleanupStep::StopWorker,
            )
            .expect("子Filterのclose実行権限");
        table
            .begin_close_cleanup_attempt(AidlObjectId(101), AidlObjectGeneration(1), authority)
            .expect("子Filterのcleanup実行を開始");
        assert!(table
            .has_active_cleanup_attempt_in_cascade(AidlObjectId(100), AidlObjectGeneration(1))
            .expect("親の隔離時に子の実行中cleanupを検出"));
    }

    #[test]
    fn issued_parent_authority_blocks_child_quarantine_before_side_effects() {
        let mut table = RuntimeObjectTable::default();
        table
            .insert(entry(
                AidlObjectKind::Demux,
                150,
                150,
                RuntimeOwnerRelation::Root,
            ))
            .expect("親Demux登録");
        table
            .insert(entry(
                AidlObjectKind::Dvr,
                151,
                151,
                RuntimeOwnerRelation::Demux {
                    demux: AidlObjectId(150),
                    generation: AidlObjectGeneration(1),
                },
            ))
            .expect("子DVR登録");
        table
            .begin_close_cascade_with_cleanup_authority(
                AidlObjectId(150),
                AidlObjectGeneration(1),
                CleanupStep::StopWorker,
            )
            .expect("親close権限発行、物理cleanupは未開始");
        assert!(table
            .has_related_outstanding_cleanup(AidlObjectId(151), AidlObjectGeneration(1))
            .expect("親権限の検証"));
        assert_eq!(
            table.entry(AidlObjectId(151)).unwrap().lifecycle,
            RuntimeObjectLifecycle::Closing {
                step: CleanupStep::StopWorker
            }
        );
    }

    #[test]
    fn active_parent_cleanup_blocks_child_drop_leak_quarantine() {
        let mut table = RuntimeObjectTable::default();
        table
            .insert(entry(
                AidlObjectKind::Demux,
                100,
                100,
                RuntimeOwnerRelation::Root,
            ))
            .expect("親Demux登録");
        table
            .insert(entry(
                AidlObjectKind::Filter,
                101,
                101,
                RuntimeOwnerRelation::Demux {
                    demux: AidlObjectId(100),
                    generation: AidlObjectGeneration(1),
                },
            ))
            .expect("子Filter登録");
        let (_, authority) = table
            .begin_close_cascade_with_cleanup_authority(
                AidlObjectId(100),
                AidlObjectGeneration(1),
                CleanupStep::StopWorker,
            )
            .expect("親Demux close権限");
        table
            .begin_close_cleanup_attempt(AidlObjectId(100), AidlObjectGeneration(1), authority)
            .expect("親Demux cleanup実行開始");
        assert!(table
            .has_active_cleanup_attempt_in_cascade(AidlObjectId(101), AidlObjectGeneration(1))
            .expect("親のcleanup権限を子の隔離で検出する"));
    }

    #[test]
    fn quarantine_cascade_terminalizes_owner_and_descendants() {
        let mut table = RuntimeObjectTable::default();
        table
            .insert(entry(
                AidlObjectKind::Demux,
                10,
                10,
                RuntimeOwnerRelation::Root,
            ))
            .unwrap();
        table
            .insert(entry(
                AidlObjectKind::Filter,
                11,
                11,
                RuntimeOwnerRelation::Demux {
                    demux: AidlObjectId(10),
                    generation: AidlObjectGeneration(1),
                },
            ))
            .unwrap();
        let changed = table
            .quarantine_cascade(AidlObjectId(10), AidlObjectGeneration(1))
            .unwrap();
        assert_eq!(changed.len(), 2);
        assert_eq!(
            table.entry(AidlObjectId(10)).unwrap().lifecycle,
            RuntimeObjectLifecycle::Quarantined
        );
        assert_eq!(
            table.entry(AidlObjectId(11)).unwrap().lifecycle,
            RuntimeObjectLifecycle::Quarantined
        );
    }

    #[test]
    fn active_diagnostic_snapshot_tracks_typed_lifecycle_and_drops_closed_binding() {
        let mut table = RuntimeObjectTable::default();
        let object_id = AidlObjectId(19);
        table
            .insert(entry(
                AidlObjectKind::Frontend,
                object_id.0,
                7,
                RuntimeOwnerRelation::Root,
            ))
            .expect("insertが成功する");

        assert_eq!(
            table.active_diagnostic_snapshots(),
            vec![RuntimeObjectDiagnosticSnapshot {
                object_kind: AidlObjectKind::Frontend,
                object_id,
                generation: AidlObjectGeneration(1),
                public_runtime_id: LedgerId(7),
                lifecycle: RuntimeObjectLifecycleSnapshot::Live,
            }]
        );

        table
            .begin_close_cascade(
                object_id,
                AidlObjectGeneration(1),
                CleanupStep::ReleaseBackend,
            )
            .expect("close開始が成功する");
        assert_eq!(
            table.active_diagnostic_snapshots()[0].lifecycle(),
            RuntimeObjectLifecycleSnapshot::Closing {
                step: CleanupStep::ReleaseBackend
            }
        );

        table
            .commit_close_cascade(object_id, AidlObjectGeneration(1))
            .expect("close commitが成功する");
        assert!(table.active_diagnostic_snapshots().is_empty());
    }

    #[test]
    fn pending_supervised_quarantine_prevents_runtime_reuse_until_join() {
        let mut table = RuntimeObjectTable::default();
        let object_id = AidlObjectId(20);
        let generation = AidlObjectGeneration(1);
        table
            .insert(entry(
                AidlObjectKind::Dvr,
                20,
                20,
                RuntimeOwnerRelation::Root,
            ))
            .expect("DVR登録");
        table
            .begin_quarantine_cleanup(object_id, generation, [(object_id, generation)])
            .expect("隔離遷移とObjectCloseTxn未完義務を同時確定");
        table
            .finish_quarantined_cleanup_report(object_id, generation, &[(object_id, generation)])
            .expect("DVR WorkerをSupervisorに移管");
        assert!(table
            .insert(entry(
                AidlObjectKind::Dvr,
                21,
                20,
                RuntimeOwnerRelation::Root,
            ))
            .is_err());
        assert_eq!(
            table.entry(object_id).unwrap().lifecycle,
            RuntimeObjectLifecycle::Quarantined
        );
        table
            .notify_supervised_dvr_joined(object_id, generation)
            .expect("DVR Workerの回収が完了");
        assert_eq!(
            table.entry(object_id).unwrap().lifecycle,
            RuntimeObjectLifecycle::Closed
        );
        table
            .insert(entry(
                AidlObjectKind::Dvr,
                21,
                20,
                RuntimeOwnerRelation::Root,
            ))
            .expect("物理回収の完了後ならruntimeを再利用できる");
    }

    #[test]
    fn quarantined_runtime_binding_can_be_reinserted() {
        let mut table = RuntimeObjectTable::default();
        table
            .insert(entry(
                AidlObjectKind::Filter,
                20,
                20,
                RuntimeOwnerRelation::Root,
            ))
            .unwrap();
        table
            .quarantine_cascade(AidlObjectId(20), AidlObjectGeneration(1))
            .unwrap();
        table
            .insert(entry(
                AidlObjectKind::Filter,
                21,
                20,
                RuntimeOwnerRelation::Root,
            ))
            .unwrap();
        assert_eq!(
            table
                .live_entry_for_runtime(AidlObjectKind::Filter, LedgerId(20))
                .unwrap()
                .object_id,
            AidlObjectId(21)
        );
    }

    #[test]
    fn commit_close_cascade_rejects_terminal_root_before_mutating_descendants() {
        let mut table = RuntimeObjectTable::default();
        table
            .insert(entry(
                AidlObjectKind::Demux,
                30,
                30,
                RuntimeOwnerRelation::Root,
            ))
            .unwrap();
        table
            .insert(entry(
                AidlObjectKind::Filter,
                31,
                31,
                RuntimeOwnerRelation::Demux {
                    demux: AidlObjectId(30),
                    generation: AidlObjectGeneration(1),
                },
            ))
            .unwrap();
        table.entries.get_mut(&AidlObjectId(30)).unwrap().lifecycle =
            RuntimeObjectLifecycle::Closed;
        table.entries.get_mut(&AidlObjectId(31)).unwrap().lifecycle =
            RuntimeObjectLifecycle::Closing {
                step: CleanupStep::ReleaseLedger,
            };

        let err = table
            .commit_close_cascade(AidlObjectId(30), AidlObjectGeneration(1))
            .expect_err("terminal root must be rejected before descendant mutation");
        assert_eq!(
            err,
            RuntimeObjectTableError::InvalidLifecycle {
                object_id: AidlObjectId(30),
                lifecycle: RuntimeObjectLifecycle::Closed,
            }
        );
        assert_eq!(
            table.entry(AidlObjectId(31)).unwrap().lifecycle,
            RuntimeObjectLifecycle::Closing {
                step: CleanupStep::ReleaseLedger,
            }
        );
    }

    #[test]
    fn mark_cleanup_failed_cascade_rejects_terminal_root_before_mutating_descendants() {
        let mut table = RuntimeObjectTable::default();
        table
            .insert(entry(
                AidlObjectKind::Demux,
                40,
                40,
                RuntimeOwnerRelation::Root,
            ))
            .unwrap();
        table
            .insert(entry(
                AidlObjectKind::Filter,
                41,
                41,
                RuntimeOwnerRelation::Demux {
                    demux: AidlObjectId(40),
                    generation: AidlObjectGeneration(1),
                },
            ))
            .unwrap();
        table.entries.get_mut(&AidlObjectId(40)).unwrap().lifecycle =
            RuntimeObjectLifecycle::Quarantined;
        table.entries.get_mut(&AidlObjectId(41)).unwrap().lifecycle =
            RuntimeObjectLifecycle::Closing {
                step: CleanupStep::ReleaseLedger,
            };

        let err = table
            .mark_cleanup_failed_cascade(
                AidlObjectId(40),
                AidlObjectGeneration(1),
                CleanupStep::UnregisterRuntime,
            )
            .expect_err("terminal root must be rejected before descendant mutation");
        assert_eq!(
            err,
            RuntimeObjectTableError::InvalidLifecycle {
                object_id: AidlObjectId(40),
                lifecycle: RuntimeObjectLifecycle::Quarantined,
            }
        );
        assert_eq!(
            table.entry(AidlObjectId(41)).unwrap().lifecycle,
            RuntimeObjectLifecycle::Closing {
                step: CleanupStep::ReleaseLedger,
            }
        );
    }
}

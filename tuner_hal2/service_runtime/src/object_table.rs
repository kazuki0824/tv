use std::collections::BTreeMap;

use maleicacid_tuner_hal2_domain_request::{AidlObjectGeneration, AidlObjectId, AidlObjectKind};
use maleicacid_tuner_hal2_resource_ledger::{CleanupStep, LedgerGeneration, LedgerId};

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
    Live,
    Closing { step: CleanupStep },
    CleanupFailed { step: CleanupStep },
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeObjectEntry {
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

#[derive(Debug, Default)]
pub struct RuntimeObjectTable {
    entries: BTreeMap<AidlObjectId, RuntimeObjectEntry>,
}

impl RuntimeObjectTable {
    pub fn insert(&mut self, mut entry: RuntimeObjectEntry) -> Result<(), RuntimeObjectTableError> {
        if let Some(existing) = self.entries.get(&entry.object_id) {
            if !existing.lifecycle.is_terminal() {
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
                && !existing.lifecycle.is_terminal()
        }) {
            return Err(RuntimeObjectTableError::DuplicateRuntimeBinding {
                object_kind: entry.object_kind,
                runtime_id: entry.ledger_id,
            });
        }
        self.ensure_owner_live_for(entry.object_id, entry.owner)?;
        entry.lifecycle = RuntimeObjectLifecycle::Live;
        self.entries.insert(entry.object_id, entry);
        Ok(())
    }

    pub fn remove(
        &mut self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> Result<RuntimeObjectEntry, RuntimeObjectTableError> {
        self.entry_checked(object_id, generation)?;
        self.entries
            .remove(&object_id)
            .ok_or(RuntimeObjectTableError::MissingObject { object_id })
    }

    pub fn begin_close_cascade(
        &mut self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        step: CleanupStep,
    ) -> Result<Vec<RuntimeObjectEntry>, RuntimeObjectTableError> {
        let root = self.entry_checked_any(object_id, generation)?;
        match root.lifecycle {
            RuntimeObjectLifecycle::Live | RuntimeObjectLifecycle::CleanupFailed { .. } => {}
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
                RuntimeObjectLifecycle::Live | RuntimeObjectLifecycle::CleanupFailed { .. } => {
                    entry.lifecycle = RuntimeObjectLifecycle::Closing { step };
                    changed.push(entry.clone());
                }
                RuntimeObjectLifecycle::Closing { .. } => {
                    changed.push(entry.clone());
                }
                RuntimeObjectLifecycle::Closed | RuntimeObjectLifecycle::Quarantined => {}
            }
        }
        Ok(changed)
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
            let entry = self.entry_mut_checked_any(target_id, target_generation)?;
            let is_root = target_id == object_id && target_generation == generation;
            match entry.lifecycle {
                RuntimeObjectLifecycle::Closing { .. }
                | RuntimeObjectLifecycle::CleanupFailed { .. } => {
                    entry.lifecycle = RuntimeObjectLifecycle::CleanupFailed { step };
                    changed.push(entry.clone());
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
                | RuntimeObjectLifecycle::CleanupFailed { .. } => entries.push(entry.clone()),
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
            let entry = self.entry_mut_checked_any(target_id, target_generation)?;
            let is_root = target_id == object_id && target_generation == generation;
            match entry.lifecycle {
                RuntimeObjectLifecycle::Closing { .. }
                | RuntimeObjectLifecycle::CleanupFailed { .. } => {
                    entry.lifecycle = RuntimeObjectLifecycle::Closed;
                    changed.push(entry.clone());
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
        Ok(changed)
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

    pub fn clear(&mut self) {
        self.entries.clear();
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
            | RuntimeObjectLifecycle::CleanupFailed { .. } => Ok(()),
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

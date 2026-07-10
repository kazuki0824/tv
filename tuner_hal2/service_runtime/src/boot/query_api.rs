use super::{
    AidlObjectGeneration, AidlObjectId, AidlObjectKind, DemuxRuntimeId, DemuxRuntimeRollbackToken, DemuxRuntimeSnapshot,
    DvrRuntimeId, FilterOpenType, FilterRuntimeId, FrontendLiveReaderDescriptor, FrontendRuntimeId,
    FrontendRuntimeSnapshot, HalError, HalInternalKind, HalInvalidArgumentKind,
    HalInvalidStateKind, LnbRuntimeId, RuntimeObjectTable, RuntimeObjectTableError,
    RuntimeOwnerRelation, RuntimeRegistry, TunerServiceRuntime,
};
use crate::object_method_txn::ObjectFrontendStatusSnapshot;
use maleicacid_tuner_hal2_demux::{
    DemuxRuntimeRollbackTokenPrepareRequest, DvrRuntimeState, DvrStatusEvent,
    QueueDescriptorExportPlan as DemuxQueueDescriptorExportPlan,
    QueueDescriptorExportTarget, QueueDescriptorQueryError, QueueDescriptorSnapshot,
    QueueRuntimeError,
};

#[derive(Debug)]
pub(crate) struct QueueDescriptorExportPlan {
    object_kind: AidlObjectKind,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
    runtime_id: i32,
    handle: DemuxQueueDescriptorExportPlan,
}

impl QueueDescriptorExportPlan {
    fn new(
        object_kind: AidlObjectKind,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        runtime_id: i32,
        handle: DemuxQueueDescriptorExportPlan,
    ) -> Self {
        debug_assert!(
            matches!(
                (object_kind, handle.target()),
                (
                    AidlObjectKind::Filter,
                    QueueDescriptorExportTarget::Filter { filter_id }
                ) if filter_id == runtime_id
            ) || matches!(
                (object_kind, handle.target()),
                (AidlObjectKind::Dvr, QueueDescriptorExportTarget::Dvr { dvr_id }) if dvr_id == runtime_id
            )
        );
        Self {
            object_kind,
            object_id,
            generation,
            runtime_id,
            handle,
        }
    }

    pub(crate) const fn object_kind(&self) -> AidlObjectKind {
        self.object_kind
    }

    pub(crate) const fn object_id(&self) -> AidlObjectId {
        self.object_id
    }

    pub(crate) const fn generation(&self) -> AidlObjectGeneration {
        self.generation
    }

    pub(crate) const fn runtime_id(&self) -> i32 {
        self.runtime_id
    }

    pub(crate) fn export_descriptor(self) -> Result<QueueDescriptorSnapshot, QueueRuntimeError> {
        self.handle.export_descriptor()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RuntimeObjectQueryError {
    DuplicateObjectId,
    DuplicateRuntimeBinding,
    MissingObject,
    ObjectKindMismatch,
    GenerationMismatch,
    InvalidOwner,
    MissingOwner,
    OwnerGenerationMismatch,
    OwnerKindMismatch,
    OwnerNotLive,
    InvalidLifecycle,
    UnsupportedObjectKind,
    GenerationOverflow,
    ObjectIdOverflow,
    PublicIdOutOfRange,
}

impl RuntimeObjectQueryError {
    fn from_object_table_error(error: RuntimeObjectTableError) -> Self {
        match error {
            RuntimeObjectTableError::DuplicateObjectId { .. } => Self::DuplicateObjectId,
            RuntimeObjectTableError::DuplicateRuntimeBinding { .. } => {
                Self::DuplicateRuntimeBinding
            }
            RuntimeObjectTableError::MissingObject { .. } => Self::MissingObject,
            RuntimeObjectTableError::ObjectKindMismatch { .. } => Self::ObjectKindMismatch,
            RuntimeObjectTableError::GenerationMismatch { .. } => Self::GenerationMismatch,
            RuntimeObjectTableError::InvalidOwner { .. } => Self::InvalidOwner,
            RuntimeObjectTableError::MissingOwner { .. } => Self::MissingOwner,
            RuntimeObjectTableError::OwnerGenerationMismatch { .. } => {
                Self::OwnerGenerationMismatch
            }
            RuntimeObjectTableError::OwnerKindMismatch { .. } => Self::OwnerKindMismatch,
            RuntimeObjectTableError::OwnerNotLive { .. } => Self::OwnerNotLive,
            RuntimeObjectTableError::InvalidLifecycle { .. } => Self::InvalidLifecycle,
            RuntimeObjectTableError::UnsupportedObjectKind { .. } => Self::UnsupportedObjectKind,
            RuntimeObjectTableError::GenerationOverflow => Self::GenerationOverflow,
            RuntimeObjectTableError::ObjectIdOverflow => Self::ObjectIdOverflow,
        }
    }

    fn into_hal_for_object_method(self) -> HalError {
        match self {
            Self::DuplicateObjectId => HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "AIDL object id has a duplicate live registration",
            ),
            Self::DuplicateRuntimeBinding => HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "AIDL public runtime id has a duplicate live binding",
            ),
            Self::MissingObject => HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "AIDL object is missing for object method",
            ),
            Self::ObjectKindMismatch => HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "AIDL object kind mismatch for object method",
            ),
            Self::GenerationMismatch => HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "AIDL object generation mismatch for object method",
            ),
            Self::InvalidOwner => HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "AIDL object owner relation is invalid for object method",
            ),
            Self::MissingOwner => HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "AIDL object owner is missing for object method",
            ),
            Self::OwnerGenerationMismatch => HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "AIDL object owner generation mismatch for object method",
            ),
            Self::OwnerKindMismatch => HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "AIDL object owner kind mismatch for object method",
            ),
            Self::OwnerNotLive => HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "AIDL object owner is not live for object method",
            ),
            Self::InvalidLifecycle => HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "AIDL object is not live for object method",
            ),
            Self::UnsupportedObjectKind => HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "AIDL object kind is unsupported for object method",
            ),
            Self::GenerationOverflow => HalError::internal(
                HalInternalKind::InvariantViolation,
                "AIDL object generation overflow while resolving object query",
            ),
            Self::ObjectIdOverflow => HalError::internal(
                HalInternalKind::InvariantViolation,
                "AIDL object id overflow while resolving object query",
            ),
            Self::PublicIdOutOfRange => HalError::internal(
                HalInternalKind::InvariantViolation,
                "AIDL public runtime id is out of range for object method",
            ),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RuntimeObjectPublicEntry {
    public_id: i32,
    owner: RuntimeOwnerRelation,
}

impl RuntimeObjectPublicEntry {
    pub(crate) fn public_id(&self) -> i32 {
        self.public_id
    }

    pub(crate) fn owner(&self) -> RuntimeOwnerRelation {
        self.owner
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DvrStatusPollSnapshot {
    pub event: Option<DvrStatusEvent>,
    pub interval_ms: u64,
    pub started: bool,
    pub callback_present: bool,
    pub callback_unhealthy: bool,
    pub status_reporting_enabled: bool,
}

pub(crate) struct RuntimeQuery<'a> {
    registry: &'a RuntimeRegistry,
    object_table: &'a RuntimeObjectTable,
}

impl TunerServiceRuntime {
    pub(crate) fn query(&self) -> RuntimeQuery<'_> {
        RuntimeQuery {
            registry: &self.registry,
            object_table: &self.object_table,
        }
    }

    pub(crate) fn prepare_bound_demux_runtime_rollback_tokens(
        &mut self,
        frontend_id: i32,
    ) -> Result<Vec<(DemuxRuntimeId, DemuxRuntimeRollbackToken)>, HalError> {
        let frontend_key = FrontendRuntimeId(frontend_id);
        let demux_ids = self.registry.frontend_bound_demux_ids(frontend_key);
        let mut tokens = Vec::with_capacity(demux_ids.len());
        for demux_id in demux_ids {
            let demux = self.registry.demux_runtime_mut(demux_id).ok_or_else(|| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "bound demux runtime is missing while preparing tune rollback token",
                )
            })?;
            tokens.push((
                demux_id,
                demux
                    .rollback_token_from_typed_request(
                        DemuxRuntimeRollbackTokenPrepareRequest::new(demux_id.0),
                    )
                    .map_err(super::demux_runtime_error_to_hal)?,
            ));
        }
        Ok(tokens)
    }

    pub(crate) fn has_frontend_id(&self, id: i32) -> bool {
        self.query().has_frontend_id(id)
    }

    pub(crate) fn frontend_entry(&self, id: i32) -> Option<crate::registry::FrontendRegistryEntry> {
        self.query().frontend_entry(id)
    }

    pub(crate) fn frontend_entry_for_aidl_object(
        &self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> Result<crate::registry::FrontendRegistryEntry, HalError> {
        self.query()
            .frontend_entry_for_aidl_object(object_id, generation)
    }

    pub(crate) fn has_demux_id(&self, id: i32) -> bool {
        self.query().has_demux_id(id)
    }

    pub(crate) fn has_lnb_id(&self, id: i32) -> bool {
        self.query().has_lnb_id(id)
    }

    pub(crate) fn lnb_id_by_name(&self, name: &str) -> Option<i32> {
        self.query().lnb_id_by_name(name)
    }

    pub(crate) fn lnb_for_frontend_id(
        &self,
        frontend_id: i32,
    ) -> Option<crate::registry::LnbRegistryEntry> {
        self.query().lnb_for_frontend_id(frontend_id)
    }

    pub(crate) fn filter_open_type(&self, filter_id: i32) -> Option<FilterOpenType> {
        self.query().filter_open_type(filter_id)
    }

    pub fn dvr_status_poll_snapshot_for_aidl_object(
        &self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> Result<DvrStatusPollSnapshot, HalError> {
        self.query()
            .dvr_status_poll_snapshot_for_aidl_object(object_id, generation)
    }

    pub(crate) fn public_entry_for_aidl_object(
        &self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        expected_kind: AidlObjectKind,
    ) -> Result<RuntimeObjectPublicEntry, RuntimeObjectQueryError> {
        self.query()
            .public_entry_for_aidl_object(object_id, generation, expected_kind)
    }

    pub(crate) fn public_runtime_id_for_aidl_object(
        &self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        expected_kind: AidlObjectKind,
    ) -> Result<i32, RuntimeObjectQueryError> {
        self.public_entry_for_aidl_object(object_id, generation, expected_kind)
            .map(|entry| entry.public_id())
    }

    pub(crate) fn public_runtime_id_for_object_method(
        &self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        expected_kind: AidlObjectKind,
    ) -> Result<i32, HalError> {
        self.public_runtime_id_for_aidl_object(object_id, generation, expected_kind)
            .map_err(RuntimeObjectQueryError::into_hal_for_object_method)
    }

    pub(crate) fn public_entry_for_object_method(
        &self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        expected_kind: AidlObjectKind,
    ) -> Result<RuntimeObjectPublicEntry, HalError> {
        self.public_entry_for_aidl_object(object_id, generation, expected_kind)
            .map_err(RuntimeObjectQueryError::into_hal_for_object_method)
    }
}

pub(crate) fn map_queue_descriptor_query_error(error: QueueDescriptorQueryError) -> HalError {
    match error {
        QueueDescriptorQueryError::FilterMissing(id)
        | QueueDescriptorQueryError::DvrMissing(id)
        | QueueDescriptorQueryError::InvalidState(id) => HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            format!("queue descriptor runtime is not available: id={id}"),
        ),
        QueueDescriptorQueryError::Unavailable(_) => {
            HalError::Unsupported("queue descriptor is unavailable in current runtime state")
        }
        QueueDescriptorQueryError::RuntimeMissing(id) => HalError::internal(
            HalInternalKind::InvariantViolation,
            format!("queue descriptor runtime is missing: id={id}"),
        ),
        QueueDescriptorQueryError::Runtime(error) => HalError::fmq_failed(
            "queue_descriptor_export",
            format!(
                "queue descriptor export failed: runtime_kind={:?} detail={}",
                error.kind, error.detail
            ),
        ),
    }
}
impl<'a> RuntimeQuery<'a> {
    pub(crate) fn filter_queue_descriptor_export_plan_for_aidl_object(
        &self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> Result<QueueDescriptorExportPlan, HalError> {
        let filter_id = self
            .public_runtime_id_for_aidl_object(object_id, generation, AidlObjectKind::Filter)
            .map_err(RuntimeObjectQueryError::into_hal_for_object_method)?;
        let owner_demux_id = self
            .registry
            .filter(FilterRuntimeId(filter_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "filter runtime entry is missing",
                )
            })?
            .owner_demux_id;
        let demux = self
            .registry
            .demux_runtime(DemuxRuntimeId(owner_demux_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "owner demux runtime is missing for filter queue descriptor",
                )
            })?;
        demux
            .filter_queue_descriptor_export_plan(filter_id)
            .map(|handle| {
                QueueDescriptorExportPlan::new(
                    AidlObjectKind::Filter,
                    object_id,
                    generation,
                    filter_id,
                    handle,
                )
            })
            .map_err(map_queue_descriptor_query_error)
    }

    pub(crate) fn dvr_queue_descriptor_export_plan_for_aidl_object(
        &self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> Result<QueueDescriptorExportPlan, HalError> {
        let dvr_id = self
            .public_runtime_id_for_aidl_object(object_id, generation, AidlObjectKind::Dvr)
            .map_err(RuntimeObjectQueryError::into_hal_for_object_method)?;
        let owner_demux_id = self
            .registry
            .dvr(DvrRuntimeId(dvr_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "DVR runtime entry is missing",
                )
            })?
            .owner_demux_id;
        let demux = self
            .registry
            .demux_runtime(DemuxRuntimeId(owner_demux_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "owner demux runtime is missing for DVR queue descriptor",
                )
            })?;
        demux
            .dvr_queue_descriptor_export_plan(dvr_id)
            .map(|handle| {
                QueueDescriptorExportPlan::new(
                    AidlObjectKind::Dvr,
                    object_id,
                    generation,
                    dvr_id,
                    handle,
                )
            })
            .map_err(map_queue_descriptor_query_error)
    }

    pub(crate) fn dvr_status_poll_snapshot_for_aidl_object(
        &self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> Result<DvrStatusPollSnapshot, HalError> {
        let dvr_id = self
            .public_runtime_id_for_aidl_object(object_id, generation, AidlObjectKind::Dvr)
            .map_err(RuntimeObjectQueryError::into_hal_for_object_method)?;
        let owner_demux_id = self
            .registry
            .dvr(DvrRuntimeId(dvr_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "DVR runtime entry is missing",
                )
            })?
            .owner_demux_id;
        let demux = self
            .registry
            .demux_runtime(DemuxRuntimeId(owner_demux_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "owner demux runtime is missing for DVR status poll",
                )
            })?;
        let dvr = demux.dvr_snapshot(dvr_id).map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "DVR runtime is missing for DVR status poll",
            )
        })?;
        let started = matches!(dvr.state, DvrRuntimeState::Started);
        let callback_unhealthy = dvr.callback_unhealthy;
        let event = if started && !callback_unhealthy {
            demux.dvr_status_event(dvr_id).map_err(|_| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "DVR status event is not available",
                )
            })?
        } else {
            None
        };
        Ok(DvrStatusPollSnapshot {
            event,
            interval_ms: dvr.status_check_interval_ms,
            started,
            callback_present: dvr.callback_present,
            callback_unhealthy,
            status_reporting_enabled: dvr.status_mask != 0,
        })
    }

    pub(crate) fn public_entry_for_aidl_object(
        &self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        expected_kind: AidlObjectKind,
    ) -> Result<RuntimeObjectPublicEntry, RuntimeObjectQueryError> {
        let entry = self
            .object_table
            .entry_for_kind(object_id, generation, expected_kind)
            .map_err(RuntimeObjectQueryError::from_object_table_error)?;
        let public_id = i32::try_from(entry.ledger_id.0)
            .map_err(|_| RuntimeObjectQueryError::PublicIdOutOfRange)?;
        Ok(RuntimeObjectPublicEntry {
            public_id,
            owner: entry.owner,
        })
    }

    pub(crate) fn public_runtime_id_for_aidl_object(
        &self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        expected_kind: AidlObjectKind,
    ) -> Result<i32, RuntimeObjectQueryError> {
        self.public_entry_for_aidl_object(object_id, generation, expected_kind)
            .map(|entry| entry.public_id())
    }

    pub(crate) fn public_runtime_id_for_object_method(
        &self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        expected_kind: AidlObjectKind,
    ) -> Result<i32, HalError> {
        self.public_runtime_id_for_aidl_object(object_id, generation, expected_kind)
            .map_err(RuntimeObjectQueryError::into_hal_for_object_method)
    }

    pub(crate) fn public_entry_for_object_method(
        &self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        expected_kind: AidlObjectKind,
    ) -> Result<RuntimeObjectPublicEntry, HalError> {
        self.public_entry_for_aidl_object(object_id, generation, expected_kind)
            .map_err(RuntimeObjectQueryError::into_hal_for_object_method)
    }

    pub(crate) fn frontend_ids(&self) -> Vec<i32> {
        self.registry
            .frontend_ids()
            .into_iter()
            .map(|id| id.0)
            .collect()
    }

    pub(crate) fn has_frontend_id(&self, id: i32) -> bool {
        self.registry.frontend(FrontendRuntimeId(id)).is_some()
    }

    pub(crate) fn frontend_entry(&self, id: i32) -> Option<crate::registry::FrontendRegistryEntry> {
        self.registry.frontend(FrontendRuntimeId(id)).cloned()
    }

    pub(crate) fn frontend_entry_for_aidl_object(
        &self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> Result<crate::registry::FrontendRegistryEntry, HalError> {
        let public_id = self
            .public_runtime_id_for_aidl_object(object_id, generation, AidlObjectKind::Frontend)
            .map_err(RuntimeObjectQueryError::into_hal_for_object_method)?;
        self.frontend_entry(public_id)
            .ok_or_else(|| HalError::Unsupported("frontend runtime entry is not available"))
    }

    pub(crate) fn frontend_status_query_for_aidl_object(
        &self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> Result<ObjectFrontendStatusSnapshot, HalError> {
        let entry = self.frontend_entry_for_aidl_object(object_id, generation)?;
        let runtime = self
            .registry
            .frontend_runtime(FrontendRuntimeId(entry.id.0))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for advertised frontend",
                )
            })?;
        let snapshot = runtime.snapshot();
        Ok(ObjectFrontendStatusSnapshot {
            lnb_profile: entry.lnb_profile,
            runtime_state: snapshot.state,
            signal_state: snapshot.signal_state,
        })
    }

    pub(crate) fn demux_ids(&self) -> Vec<i32> {
        self.registry
            .demux_ids()
            .into_iter()
            .map(|id| id.0)
            .collect()
    }

    pub(crate) fn has_demux_id(&self, id: i32) -> bool {
        self.registry.demux(DemuxRuntimeId(id)).is_some()
    }

    pub(crate) fn lnb_ids(&self) -> Vec<i32> {
        self.registry.lnb_ids().into_iter().map(|id| id.0).collect()
    }

    pub(crate) fn has_lnb_id(&self, id: i32) -> bool {
        self.registry.lnb(LnbRuntimeId(id)).is_some()
    }

    pub(crate) fn lnb_id_by_name(&self, name: &str) -> Option<i32> {
        self.registry.lnb_by_name(name).map(|entry| entry.id.0)
    }

    pub(crate) fn lnb_for_frontend_id(
        &self,
        frontend_id: i32,
    ) -> Option<crate::registry::LnbRegistryEntry> {
        self.registry
            .lnb_for_frontend(FrontendRuntimeId(frontend_id))
            .cloned()
    }

    pub(crate) fn frontend_runtime_snapshot(
        &self,
        frontend_id: i32,
    ) -> Result<FrontendRuntimeSnapshot, HalError> {
        let runtime = self
            .registry
            .frontend_runtime(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for advertised frontend",
                )
            })?;
        Ok(runtime.snapshot())
    }

    pub(crate) fn bound_demux_runtime_generations(
        &self,
        frontend_id: i32,
    ) -> Result<Vec<(DemuxRuntimeId, u64)>, HalError> {
        let frontend_key = FrontendRuntimeId(frontend_id);
        let demux_ids = self.registry.frontend_bound_demux_ids(frontend_key);
        let mut generations = Vec::with_capacity(demux_ids.len());
        for demux_id in demux_ids {
            let demux = self.registry.demux_runtime(demux_id).ok_or_else(|| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "bound demux runtime is missing while reading tune rollback generation",
                )
            })?;
            generations.push((demux_id, demux.snapshot().generation()));
        }
        Ok(generations)
    }

    pub(crate) fn frontend_live_reader_descriptor_for_live_pump(
        &self,
        frontend_id: i32,
    ) -> Result<Option<FrontendLiveReaderDescriptor>, HalError> {
        let frontend_key = crate::registry::FrontendRuntimeId(frontend_id);
        if self.registry.frontend(frontend_key).is_none() {
            return Err(HalError::Unsupported(
                "frontend id is not available for live pump",
            ));
        }
        if self
            .registry
            .frontend_bound_demux_ids(frontend_key)
            .is_empty()
        {
            return Ok(None);
        }
        let runtime = self
            .registry
            .frontend_runtime(frontend_key)
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for advertised frontend",
                )
            })?;
        runtime
            .live_reader_descriptor()
            .cloned()
            .map(Some)
            .ok_or_else(|| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "frontend has bound demux but no live reader descriptor",
                )
            })
    }

    pub(crate) fn filter_open_type(&self, filter_id: i32) -> Option<FilterOpenType> {
        let entry = self.registry.filter(FilterRuntimeId(filter_id))?;
        let demux = self
            .registry
            .demux_runtime(DemuxRuntimeId(entry.owner_demux_id))?;
        demux
            .filter_snapshot(filter_id)
            .map(|snapshot| snapshot.open_type)
            .ok()
    }

    pub(crate) fn first_pcr_filter_id_for_demux_object(
        &self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> Result<Option<i32>, HalError> {
        let demux_id =
            self.public_runtime_id_for_object_method(object_id, generation, AidlObjectKind::Demux)?;
        Ok(self.first_pcr_filter_id_for_demux(demux_id))
    }

    pub(crate) fn ensure_media_filter_for_demux_object(
        &self,
        demux_object_id: AidlObjectId,
        demux_generation: AidlObjectGeneration,
        filter_object_id: AidlObjectId,
        filter_generation: AidlObjectGeneration,
    ) -> Result<(), HalError> {
        self.public_entry_for_object_method(
            demux_object_id,
            demux_generation,
            AidlObjectKind::Demux,
        )?;
        let filter_entry = self.public_entry_for_object_method(
            filter_object_id,
            filter_generation,
            AidlObjectKind::Filter,
        )?;
        let RuntimeOwnerRelation::Demux { demux, generation } = filter_entry.owner() else {
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "AV sync filter must be owned by a demux",
            ));
        };
        if demux != demux_object_id || generation != demux_generation {
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "AV sync filter must belong to the target demux",
            ));
        }
        let open_type = self
            .filter_open_type(filter_entry.public_id())
            .ok_or_else(|| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "AV sync filter runtime is not live",
                )
            })?;
        if !open_type.is_media_filter() {
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "AV sync hardware id requires an audio or video media filter",
            ));
        }
        Ok(())
    }

    pub(crate) fn is_live_pcr_filter_for_demux_object(
        &self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        filter_id: i32,
    ) -> Result<bool, HalError> {
        let demux_id =
            self.public_runtime_id_for_object_method(object_id, generation, AidlObjectKind::Demux)?;
        Ok(self.is_live_pcr_filter_for_demux(demux_id, filter_id))
    }

    pub(crate) fn first_pcr_filter_id_for_demux(&self, demux_id: i32) -> Option<i32> {
        let demux = self.registry.demux_runtime(DemuxRuntimeId(demux_id))?;
        self.registry
            .filters_for_demux(demux_id)
            .into_iter()
            .map(|entry| entry.id.0)
            .find(|filter_id| {
                demux
                    .filter_snapshot(*filter_id)
                    .map(|snapshot| {
                        snapshot.open_type == FilterOpenType::TsPcr
                            && !snapshot.state.is_closed_or_failed()
                    })
                    .unwrap_or(false)
            })
    }

    pub(crate) fn is_live_pcr_filter_for_demux(&self, demux_id: i32, filter_id: i32) -> bool {
        let Some(entry) = self.registry.filter(FilterRuntimeId(filter_id)) else {
            return false;
        };
        if entry.owner_demux_id != demux_id {
            return false;
        }
        let Some(demux) = self.registry.demux_runtime(DemuxRuntimeId(demux_id)) else {
            return false;
        };
        demux
            .filter_snapshot(filter_id)
            .map(|snapshot| {
                snapshot.open_type == FilterOpenType::TsPcr && !snapshot.state.is_closed_or_failed()
            })
            .unwrap_or(false)
    }

    pub(crate) fn ensure_frontend_demux_sink_ready(
        &self,
        frontend_id: i32,
    ) -> Result<Vec<DemuxRuntimeId>, HalError> {
        let frontend_key = FrontendRuntimeId(frontend_id);
        if self.registry.frontend(frontend_key).is_none() {
            return Err(HalError::Unsupported(
                "frontend id is not available for live TS delivery",
            ));
        }
        let demux_ids = self.registry.frontend_bound_demux_ids(frontend_key);
        if demux_ids.is_empty() {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "frontend has no bound demux for live TS delivery",
            ));
        }
        Ok(demux_ids)
    }
}

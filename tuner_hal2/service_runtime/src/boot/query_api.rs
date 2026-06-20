use super::{
    AidlObjectGeneration, AidlObjectId, AidlObjectKind, DemuxRuntimeId, DemuxRuntimeSnapshot,
    DvrRuntimeId, FilterOpenType, FilterRuntimeId, FrontendLiveReaderDescriptor, FrontendRuntimeId,
    FrontendRuntimeSnapshot, FrontendRuntimeState, FrontendSignalState, FrontendTuneRequest,
    HalError, HalInternalKind, HalInvalidStateKind, LnbRuntimeId, RuntimeObjectTable,
    RuntimeObjectTableError, RuntimeOwnerRelation, RuntimeRegistry, TunerServiceRuntime,
};
use maleicacid_tuner_hal2_demux::{QueueDescriptorQueryError, QueueDescriptorSnapshot};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeObjectQueryError {
    KindOrOwnerMismatch,
    NotLive,
    PublicIdOutOfRange,
}

impl RuntimeObjectQueryError {
    fn from_object_table_error(error: RuntimeObjectTableError) -> Self {
        match error {
            RuntimeObjectTableError::ObjectKindMismatch { .. }
            | RuntimeObjectTableError::InvalidOwner { .. }
            | RuntimeObjectTableError::OwnerKindMismatch { .. } => Self::KindOrOwnerMismatch,
            RuntimeObjectTableError::DuplicateObjectId { .. }
            | RuntimeObjectTableError::DuplicateRuntimeBinding { .. }
            | RuntimeObjectTableError::MissingObject { .. }
            | RuntimeObjectTableError::GenerationMismatch { .. }
            | RuntimeObjectTableError::MissingOwner { .. }
            | RuntimeObjectTableError::OwnerGenerationMismatch { .. }
            | RuntimeObjectTableError::OwnerNotLive { .. }
            | RuntimeObjectTableError::InvalidLifecycle { .. }
            | RuntimeObjectTableError::UnsupportedObjectKind { .. }
            | RuntimeObjectTableError::GenerationOverflow => Self::NotLive,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeObjectPublicEntry {
    public_id: i32,
    owner: RuntimeOwnerRelation,
}

impl RuntimeObjectPublicEntry {
    pub fn public_id(&self) -> i32 {
        self.public_id
    }

    pub fn owner(&self) -> RuntimeOwnerRelation {
        self.owner
    }
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

    pub fn frontend_ids(&self) -> Vec<i32> {
        self.query().frontend_ids()
    }

    pub fn has_frontend_id(&self, id: i32) -> bool {
        self.query().has_frontend_id(id)
    }

    pub fn frontend_entry(&self, id: i32) -> Option<crate::registry::FrontendRegistryEntry> {
        self.query().frontend_entry(id)
    }

    pub fn frontend_entry_for_aidl_object(
        &self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> Result<crate::registry::FrontendRegistryEntry, HalError> {
        self.query()
            .frontend_entry_for_aidl_object(object_id, generation)
    }

    pub fn frontend_runtime_state_for_aidl_object(
        &self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> Result<FrontendRuntimeState, HalError> {
        self.query()
            .frontend_runtime_state_for_aidl_object(object_id, generation)
    }

    pub fn frontend_signal_state_for_aidl_object(
        &self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> Result<FrontendSignalState, HalError> {
        self.query()
            .frontend_signal_state_for_aidl_object(object_id, generation)
    }

    pub fn frontend_status_query_for_aidl_object(
        &self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> Result<
        (
            crate::registry::FrontendRegistryEntry,
            FrontendRuntimeState,
            FrontendSignalState,
        ),
        HalError,
    > {
        self.query()
            .frontend_status_query_for_aidl_object(object_id, generation)
    }

    pub fn demux_ids(&self) -> Vec<i32> {
        self.query().demux_ids()
    }

    pub fn has_demux_id(&self, id: i32) -> bool {
        self.query().has_demux_id(id)
    }

    pub fn lnb_ids(&self) -> Vec<i32> {
        self.query().lnb_ids()
    }

    pub fn has_lnb_id(&self, id: i32) -> bool {
        self.query().has_lnb_id(id)
    }

    pub fn lnb_id_by_name(&self, name: &str) -> Option<i32> {
        self.query().lnb_id_by_name(name)
    }

    pub fn lnb_for_frontend_id(
        &self,
        frontend_id: i32,
    ) -> Option<crate::registry::LnbRegistryEntry> {
        self.query().lnb_for_frontend_id(frontend_id)
    }

    pub fn frontend_signal_state(&self, frontend_id: i32) -> Result<FrontendSignalState, HalError> {
        self.query().frontend_signal_state(frontend_id)
    }

    pub fn filter_open_type(&self, filter_id: i32) -> Option<FilterOpenType> {
        self.query().filter_open_type(filter_id)
    }

    pub fn filter_queue_descriptor_snapshot_for_aidl_object(
        &self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> Result<QueueDescriptorSnapshot, HalError> {
        self.query()
            .filter_queue_descriptor_snapshot_for_aidl_object(object_id, generation)
    }

    pub fn dvr_queue_descriptor_snapshot_for_aidl_object(
        &self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> Result<QueueDescriptorSnapshot, HalError> {
        self.query()
            .dvr_queue_descriptor_snapshot_for_aidl_object(object_id, generation)
    }

    pub fn public_entry_for_aidl_object(
        &self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        expected_kind: AidlObjectKind,
    ) -> Result<RuntimeObjectPublicEntry, RuntimeObjectQueryError> {
        self.query()
            .public_entry_for_aidl_object(object_id, generation, expected_kind)
    }

    pub fn public_runtime_id_for_aidl_object(
        &self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        expected_kind: AidlObjectKind,
    ) -> Result<i32, RuntimeObjectQueryError> {
        self.public_entry_for_aidl_object(object_id, generation, expected_kind)
            .map(|entry| entry.public_id())
    }

    pub fn public_runtime_id_for_object_method(
        &self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        expected_kind: AidlObjectKind,
    ) -> Result<i32, HalError> {
        self.public_runtime_id_for_aidl_object(object_id, generation, expected_kind)
            .map_err(|_| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "AIDL object is not live for object method",
                )
            })
    }

    pub fn public_entry_for_object_method(
        &self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        expected_kind: AidlObjectKind,
    ) -> Result<RuntimeObjectPublicEntry, HalError> {
        self.public_entry_for_aidl_object(object_id, generation, expected_kind)
            .map_err(|_| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "AIDL object is not live for object method",
                )
            })
    }
}

fn map_queue_descriptor_query_error(error: QueueDescriptorQueryError) -> HalError {
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
        QueueDescriptorQueryError::Runtime(error) => HalError::internal(
            HalInternalKind::InvariantViolation,
            format!(
                "queue descriptor export failed: kind={:?} detail={}",
                error.kind, error.detail
            ),
        ),
    }
}
impl<'a> RuntimeQuery<'a> {
    pub(crate) fn filter_queue_descriptor_snapshot_for_aidl_object(
        &self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> Result<QueueDescriptorSnapshot, HalError> {
        let filter_id = self
            .public_runtime_id_for_aidl_object(object_id, generation, AidlObjectKind::Filter)
            .map_err(|_| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "filter AIDL object is not live",
                )
            })?;
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
            .export_filter_queue_descriptor(filter_id)
            .map_err(map_queue_descriptor_query_error)
    }

    pub(crate) fn dvr_queue_descriptor_snapshot_for_aidl_object(
        &self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> Result<QueueDescriptorSnapshot, HalError> {
        let dvr_id = self
            .public_runtime_id_for_aidl_object(object_id, generation, AidlObjectKind::Dvr)
            .map_err(|_| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "DVR AIDL object is not live",
                )
            })?;
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
            .export_dvr_queue_descriptor(dvr_id)
            .map_err(map_queue_descriptor_query_error)
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
            .map_err(|_| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "frontend AIDL object is not live",
                )
            })?;
        self.frontend_entry(public_id)
            .ok_or_else(|| HalError::Unsupported("frontend runtime entry is not available"))
    }

    pub(crate) fn frontend_runtime_state_for_aidl_object(
        &self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> Result<FrontendRuntimeState, HalError> {
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
        Ok(runtime.state())
    }

    pub(crate) fn frontend_signal_state_for_aidl_object(
        &self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> Result<FrontendSignalState, HalError> {
        let entry = self.frontend_entry_for_aidl_object(object_id, generation)?;
        self.frontend_signal_state(entry.id.0)
    }

    pub(crate) fn frontend_status_query_for_aidl_object(
        &self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> Result<
        (
            crate::registry::FrontendRegistryEntry,
            FrontendRuntimeState,
            FrontendSignalState,
        ),
        HalError,
    > {
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
        Ok((entry, runtime.state(), runtime.signal_state()))
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

    pub(crate) fn bound_demux_runtime_snapshots(
        &self,
        frontend_id: i32,
    ) -> Result<Vec<(DemuxRuntimeId, DemuxRuntimeSnapshot)>, HalError> {
        let frontend_key = FrontendRuntimeId(frontend_id);
        let demux_ids = self.registry.frontend_bound_demux_ids(frontend_key);
        let mut snapshots = Vec::with_capacity(demux_ids.len());
        for demux_id in demux_ids {
            let demux = self.registry.demux_runtime(demux_id).ok_or_else(|| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "bound demux runtime is missing while taking tune rollback snapshot",
                )
            })?;
            snapshots.push((demux_id, demux.snapshot()));
        }
        Ok(snapshots)
    }

    pub(crate) fn frontend_has_same_active_tune(
        &self,
        frontend_id: i32,
        request: &FrontendTuneRequest,
    ) -> Result<bool, HalError> {
        let runtime = self
            .registry
            .frontend_runtime(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for advertised frontend",
                )
            })?;
        Ok(runtime.same_active_tune(request))
    }

    pub(crate) fn frontend_signal_state(
        &self,
        frontend_id: i32,
    ) -> Result<FrontendSignalState, HalError> {
        let runtime = self
            .registry
            .frontend_runtime(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for advertised frontend",
                )
            })?;
        Ok(runtime.signal_state())
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
        demux.filter(filter_id).map(|filter| filter.open_type())
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

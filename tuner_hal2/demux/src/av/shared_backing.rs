use super::event::{AvMediaEventDescriptor, AvMediaEventMetadata};
use super::release_txn::{
    AvDataIdState, AvEventLocalHandleLeaseState, AvFilterReleaseState, AvHandleReleaseInput,
    AvHandleReleaseKind, AvHandleReleaseOutcome, AvHandleReleaseTxn,
};
use super::slot::{AvDataId, AvSlotId};
use std::collections::BTreeMap;
use std::fs::File;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::MetadataExt;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};
use std::sync::Arc;

pub const DEFAULT_AV_SHARED_SLOT_SIZE_BYTES: usize = 1024 * 1024;
pub const DEFAULT_AV_SHARED_SLOT_COUNT: usize = 8;
pub const DEFAULT_AV_MAX_EVENT_BYTES: usize = DEFAULT_AV_SHARED_SLOT_SIZE_BYTES;
pub const DEFAULT_AV_MAX_OUTSTANDING_EVENTS_PER_FILTER: usize = DEFAULT_AV_SHARED_SLOT_COUNT;
pub const DEFAULT_AV_PER_FILTER_LIVE_BYTES: usize =
    DEFAULT_AV_MAX_EVENT_BYTES * DEFAULT_AV_MAX_OUTSTANDING_EVENTS_PER_FILTER;

#[derive(Debug)]
pub struct AvRuntimeBudget {
    limit_bytes: usize,
    used_bytes: AtomicUsize,
    corrupt: AtomicBool,
}

impl AvRuntimeBudget {
    pub const fn new(limit_bytes: usize) -> Self {
        Self {
            limit_bytes,
            used_bytes: AtomicUsize::new(0),
            corrupt: AtomicBool::new(false),
        }
    }

    pub const fn unlimited() -> Self {
        Self::new(usize::MAX)
    }

    fn try_claim(&self, bytes: usize) -> Result<bool, AvSharedBackingError> {
        if self.corrupt.load(Ordering::Acquire) {
            return Err(AvSharedBackingError::AllocationFailed);
        }
        let mut current = self.used_bytes.load(Ordering::Acquire);
        loop {
            let next = current
                .checked_add(bytes)
                .ok_or(AvSharedBackingError::AllocationFailed)?;
            if next > self.limit_bytes {
                return Ok(false);
            }
            match self.used_bytes.compare_exchange_weak(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Ok(true),
                Err(observed) => current = observed,
            }
        }
    }

    fn release(&self, bytes: usize) -> bool {
        let released = self
            .used_bytes
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current.checked_sub(bytes)
            })
            .is_ok();
        if !released {
            self.corrupt.store(true, Ordering::Release);
        }
        released
    }

    fn release_after_owner_drop(&self, bytes: usize) {
        if self.release(bytes) {
            return;
        }
        // Dropから公開エラーは返せない。release()が台帳をcorruptへ固定し、
        // 後続claimをfail-closedにする。
    }

    #[cfg(test)]
    fn used_bytes(&self) -> usize {
        self.used_bytes.load(Ordering::Acquire)
    }
}

struct PendingAvRuntimeClaim {
    budget: Arc<AvRuntimeBudget>,
    bytes: usize,
    committed: bool,
}

impl PendingAvRuntimeClaim {
    fn prepare(
        budget: Arc<AvRuntimeBudget>,
        bytes: usize,
    ) -> Result<Option<Self>, AvSharedBackingError> {
        if !budget.try_claim(bytes)? {
            return Ok(None);
        }
        Ok(Some(Self {
            budget,
            bytes,
            committed: false,
        }))
    }

    fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for PendingAvRuntimeClaim {
    fn drop(&mut self) {
        if !self.committed {
            self.budget.release_after_owner_drop(self.bytes);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientHandleState {
    NotExported,
    ExportedActive,
    ClientReleased,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AvPayloadDeliveryOutcome {
    Delivered(AvMediaEventDescriptor),
    SharedHandleNotExported,
    ClientHandleReleased,
    PayloadEmpty,
    PayloadOversized,
    NoFreeSlot,
    DataIdExhausted,
}

#[derive(Debug)]
pub struct AvSharedHandleExport {
    pub file: File,
    pub size_bytes: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AvSharedBackingError {
    AllocationFailed,
    DuplicateFailed,
    IdentityFailed,
    MappingFailed,
    UnmappingFailed,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct AvFileIdentity {
    device: u64,
    inode: u64,
    size_bytes: u64,
}

impl AvFileIdentity {
    pub const fn new(device: u64, inode: u64, size_bytes: u64) -> Self {
        Self {
            device,
            inode,
            size_bytes,
        }
    }

    fn from_file(file: &File) -> Result<Self, AvSharedBackingError> {
        let metadata = file
            .metadata()
            .map_err(|_| AvSharedBackingError::IdentityFailed)?;
        Ok(Self::new(metadata.dev(), metadata.ino(), metadata.size()))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AvHandleReleaseDescriptor {
    Empty,
    File(AvFileIdentity),
}

#[derive(Debug, Default)]
pub struct AvDataIdAllocator {
    issued_high_watermark: AtomicI64,
}

impl AvDataIdAllocator {
    pub fn can_issue(&self) -> bool {
        self.issued_high_watermark.load(Ordering::Relaxed) < i64::MAX
    }

    pub fn issue(&self) -> Option<AvDataId> {
        self.issued_high_watermark
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(1).filter(|next| *next > 0)
            })
            .ok()
            .and_then(|previous| previous.checked_add(1))
            .map(AvDataId)
    }

    #[cfg(test)]
    fn set_issued_high_watermark(&self, value: i64) {
        self.issued_high_watermark.store(value, Ordering::Relaxed);
    }

    #[cfg(test)]
    fn issued_high_watermark(&self) -> i64 {
        self.issued_high_watermark.load(Ordering::Relaxed)
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct AvSlotState {
    slot_id: AvSlotId,
    active_data_id: Option<AvDataId>,
    data_length: usize,
}

#[derive(Debug)]
struct EventLocalAllocation {
    _file: Arc<File>,
    file_identity: AvFileIdentity,
    handle_lease_state: AvEventLocalHandleLeaseState,
    length: usize,
}

#[derive(Debug)]
pub struct AvSharedBacking {
    state: ClientHandleState,
    data_id_allocator: Arc<AvDataIdAllocator>,
    runtime_budget: Arc<AvRuntimeBudget>,
    slot_size: usize,
    max_event_bytes: usize,
    max_outstanding_events: usize,
    per_filter_live_bytes: usize,
    slots: Vec<AvSlotState>,
    ever_exported: bool,
    file: Option<File>,
    shared_handle_identity: Option<AvFileIdentity>,
    event_local_allocations: BTreeMap<AvDataId, EventLocalAllocation>,
    event_local_bytes: usize,
}

impl AvSharedBacking {
    pub fn new() -> Self {
        Self::with_data_id_allocator(Arc::new(AvDataIdAllocator::default()))
    }

    pub fn with_data_id_allocator(data_id_allocator: Arc<AvDataIdAllocator>) -> Self {
        Self::with_profile_limits(
            DEFAULT_AV_SHARED_SLOT_COUNT,
            DEFAULT_AV_SHARED_SLOT_SIZE_BYTES,
            DEFAULT_AV_MAX_EVENT_BYTES,
            DEFAULT_AV_MAX_OUTSTANDING_EVENTS_PER_FILTER,
            DEFAULT_AV_PER_FILTER_LIVE_BYTES,
            data_id_allocator,
            Arc::new(AvRuntimeBudget::unlimited()),
        )
    }

    pub fn with_runtime_limits(
        max_event_bytes: usize,
        max_outstanding_events: usize,
        per_filter_live_bytes: usize,
        data_id_allocator: Arc<AvDataIdAllocator>,
        runtime_budget: Arc<AvRuntimeBudget>,
    ) -> Self {
        Self::with_profile_limits(
            max_outstanding_events,
            max_event_bytes,
            max_event_bytes,
            max_outstanding_events,
            per_filter_live_bytes,
            data_id_allocator,
            runtime_budget,
        )
    }

    pub fn with_layout(slot_count: usize, slot_size: usize) -> Self {
        let per_filter_live_bytes = match slot_count.checked_mul(slot_size) {
            Some(bytes) => bytes,
            None => 0,
        };
        Self::with_profile_limits(
            slot_count,
            slot_size,
            per_filter_live_bytes,
            slot_count,
            per_filter_live_bytes,
            Arc::new(AvDataIdAllocator::default()),
            Arc::new(AvRuntimeBudget::unlimited()),
        )
    }

    fn with_profile_limits(
        slot_count: usize,
        slot_size: usize,
        max_event_bytes: usize,
        max_outstanding_events: usize,
        per_filter_live_bytes: usize,
        data_id_allocator: Arc<AvDataIdAllocator>,
        runtime_budget: Arc<AvRuntimeBudget>,
    ) -> Self {
        let slots = (0..slot_count)
            .map(|idx| AvSlotState {
                slot_id: AvSlotId(idx as u32),
                active_data_id: None,
                data_length: 0,
            })
            .collect();
        Self {
            state: ClientHandleState::NotExported,
            data_id_allocator,
            runtime_budget,
            slot_size,
            max_event_bytes,
            max_outstanding_events,
            per_filter_live_bytes,
            slots,
            ever_exported: false,
            file: None,
            shared_handle_identity: None,
            event_local_allocations: BTreeMap::new(),
            event_local_bytes: 0,
        }
    }

    #[cfg(test)]
    fn client_state(&self) -> ClientHandleState {
        self.state
    }
    #[cfg(test)]
    pub(crate) fn active_slot_count(&self) -> usize {
        self.slots
            .iter()
            .filter(|slot| slot.active_data_id.is_some())
            .count()
    }
    pub fn mark_exported(&mut self) {
        self.state = ClientHandleState::ExportedActive;
        self.ever_exported = true;
    }

    pub fn export_handle(&mut self) -> Result<AvSharedHandleExport, AvSharedBackingError> {
        let size_bytes = self
            .slot_size
            .checked_mul(self.slots.len())
            .ok_or(AvSharedBackingError::AllocationFailed)?;
        if self.file.is_none() {
            // 安全性: size_bytesは上で検査済みで、allocatorへRust pointerを渡さず、新規FDまたは負のerrorだけを受け取る。
            let raw_fd = unsafe { tuner_dmabuf_heap_alloc_system(size_bytes) };
            // 事後条件: 非負の新規FDだけを一意なFile所有権へ移管する。
            if raw_fd < 0 {
                return Err(AvSharedBackingError::AllocationFailed);
            }
            // 安全性: raw_fdは非負かつ新規でRust ownerを持たず、ここで所有権を正確に1回だけ移管する。
            self.file = Some(unsafe { File::from_raw_fd(raw_fd) });
            // 事後条件: self.fileがFDのcloseを担当する唯一のRust ownerになる。
        }
        if self.shared_handle_identity.is_none() {
            self.shared_handle_identity = Some(AvFileIdentity::from_file(
                self.file
                    .as_ref()
                    .ok_or(AvSharedBackingError::AllocationFailed)?,
            )?);
        }
        let file = self
            .file
            .as_ref()
            .ok_or(AvSharedBackingError::AllocationFailed)?
            .try_clone()
            .map_err(|_| AvSharedBackingError::DuplicateFailed)?;
        self.mark_exported();
        Ok(AvSharedHandleExport { file, size_bytes })
    }

    pub fn allocate_payload_bytes(
        &mut self,
        payload: &[u8],
        metadata: AvMediaEventMetadata,
    ) -> Result<AvPayloadDeliveryOutcome, AvSharedBackingError> {
        let slot_index = match self.shared_slot_candidate(payload.len()) {
            Ok(slot_index) => slot_index,
            Err(_) => return self.allocate_event_local_payload(payload, metadata),
        };
        let Some(runtime_claim) =
            PendingAvRuntimeClaim::prepare(Arc::clone(&self.runtime_budget), payload.len())?
        else {
            return Ok(AvPayloadDeliveryOutcome::NoFreeSlot);
        };
        let file = self
            .file
            .as_ref()
            .ok_or(AvSharedBackingError::AllocationFailed)?;
        let map_len = self
            .slot_size
            .checked_mul(self.slots.len())
            .ok_or(AvSharedBackingError::MappingFailed)?;
        // 安全性: fileはliveで、map_lenは検査済みの全backing範囲、offsetは0で、要求mappingとaliasするRust参照はない。
        let mapped = unsafe {
            mmap(
                ptr::null_mut(),
                map_len,
                PROT_READ | PROT_WRITE,
                MAP_SHARED,
                file.as_raw_fd(),
                0,
            )
        };
        // 事後条件: MAP_FAILEDは拒否し、それ以外のmappedはmunmapまでmap_len byteを書込み可能な領域を表す。
        if mapped == MAP_FAILED {
            return Err(AvSharedBackingError::MappingFailed);
        }
        // 安全性: slot検証でoffset + payload.len() <= map_lenを保証し、sourceは読取り可能、destinationは書込み可能で領域は重ならない。
        unsafe {
            ptr::copy_nonoverlapping(
                payload.as_ptr(),
                (mapped as *mut u8).add(slot_index * self.slot_size),
                payload.len(),
            );
        }
        // 事後条件: 選択したmapped slotへpayload.len() byteを正確にcopyする。
        // 安全性: mappedは成功した同一mmap pointerで、map_lenも同一mapping長であり、mapped参照を外へ持ち出さない。
        if unsafe { munmap(mapped, map_len) } != 0 {
            return Err(AvSharedBackingError::UnmappingFailed);
        }
        // 事後条件: munmap後はmappingを無効として扱い、再アクセスしない。
        let Some(data_id) = self.data_id_allocator.issue() else {
            return Ok(AvPayloadDeliveryOutcome::DataIdExhausted);
        };
        let slot = &mut self.slots[slot_index];
        slot.active_data_id = Some(data_id);
        slot.data_length = payload.len();
        runtime_claim.commit();
        Ok(AvPayloadDeliveryOutcome::Delivered(
            AvMediaEventDescriptor {
                data_id,
                slot_id: slot.slot_id,
                offset: slot.slot_id.0 as usize * self.slot_size,
                data_length: payload.len(),
                metadata,
                event_local_file: None,
            },
        ))
    }

    fn allocate_event_local_payload(
        &mut self,
        payload: &[u8],
        metadata: AvMediaEventMetadata,
    ) -> Result<AvPayloadDeliveryOutcome, AvSharedBackingError> {
        if payload.is_empty() {
            return Ok(AvPayloadDeliveryOutcome::PayloadEmpty);
        }
        if payload.len() > self.max_event_bytes {
            return Ok(AvPayloadDeliveryOutcome::PayloadOversized);
        }
        let active_shared_count = self
            .slots
            .iter()
            .filter(|slot| slot.active_data_id.is_some())
            .count();
        let active_count = active_shared_count
            .checked_add(self.event_local_allocations.len())
            .ok_or(AvSharedBackingError::AllocationFailed)?;
        if active_count >= self.max_outstanding_events {
            return Ok(AvPayloadDeliveryOutcome::NoFreeSlot);
        }
        let active_shared_bytes = self
            .slots
            .iter()
            .filter(|slot| slot.active_data_id.is_some())
            .try_fold(0usize, |total, slot| total.checked_add(slot.data_length))
            .ok_or(AvSharedBackingError::AllocationFailed)?;
        let next_live_bytes = active_shared_bytes
            .checked_add(self.event_local_bytes)
            .and_then(|total| total.checked_add(payload.len()))
            .ok_or(AvSharedBackingError::AllocationFailed)?;
        if next_live_bytes > self.per_filter_live_bytes {
            return Ok(AvPayloadDeliveryOutcome::NoFreeSlot);
        }
        if !self.data_id_allocator.can_issue() {
            return Ok(AvPayloadDeliveryOutcome::DataIdExhausted);
        }
        let Some(runtime_claim) =
            PendingAvRuntimeClaim::prepare(Arc::clone(&self.runtime_budget), payload.len())?
        else {
            return Ok(AvPayloadDeliveryOutcome::NoFreeSlot);
        };
        // 安全性: payloadは非emptyでlenは値渡しし、allocatorは新規FDまたは負のerrorを返す。
        let raw_fd = unsafe { tuner_dmabuf_heap_alloc_system(payload.len()) };
        // 事後条件: 非負の新規FDだけを一意なFile所有権へ移管する。
        if raw_fd < 0 {
            return Err(AvSharedBackingError::AllocationFailed);
        }
        // 安全性: raw_fdは新規かつ非負でRust ownerを持たず、Fileへ所有権を正確に1回だけ移管する。
        let file = Arc::new(unsafe { File::from_raw_fd(raw_fd) });
        // 事後条件: Arc<File>がclose責務を所有し、raw_fdを別途wrapまたはcloseしない。
        // 安全性: fileはliveで、payload.len()は非0のmapping範囲、offsetは0で、要求mappingとaliasするRust参照はない。
        let mapped = unsafe {
            mmap(
                ptr::null_mut(),
                payload.len(),
                PROT_READ | PROT_WRITE,
                MAP_SHARED,
                file.as_raw_fd(),
                0,
            )
        };
        // 事後条件: MAP_FAILEDは拒否し、それ以外のmappedはmunmapまでpayload.len() byteを書込み可能な領域を表す。
        if mapped == MAP_FAILED {
            return Err(AvSharedBackingError::MappingFailed);
        }
        // 安全性: payloadはlen byte読取り可能、mappedは同じlenを書込み可能で、独立dmabuf storageはsliceと重ならない。
        unsafe {
            ptr::copy_nonoverlapping(payload.as_ptr(), mapped as *mut u8, payload.len());
        }
        // 事後条件: 完全なevent payloadがmapped dmabuf領域を占有する。
        // 安全性: mappedは成功した同一mmap pointerで、payload.len()も同一mapping長であり、mapped参照を外へ持ち出さない。
        if unsafe { munmap(mapped, payload.len()) } != 0 {
            return Err(AvSharedBackingError::UnmappingFailed);
        }
        // 事後条件: munmap後はevent-local mappingを無効として扱い、再アクセスしない。
        let file_identity = AvFileIdentity::from_file(&file)?;
        let Some(data_id) = self.data_id_allocator.issue() else {
            return Ok(AvPayloadDeliveryOutcome::DataIdExhausted);
        };
        self.event_local_bytes = self
            .event_local_bytes
            .checked_add(payload.len())
            .ok_or(AvSharedBackingError::AllocationFailed)?;
        self.event_local_allocations.insert(
            data_id,
            EventLocalAllocation {
                _file: Arc::clone(&file),
                file_identity,
                handle_lease_state: AvEventLocalHandleLeaseState::Active,
                length: payload.len(),
            },
        );
        runtime_claim.commit();
        Ok(AvPayloadDeliveryOutcome::Delivered(
            AvMediaEventDescriptor {
                data_id,
                slot_id: AvSlotId(u32::MAX),
                offset: 0,
                data_length: payload.len(),
                metadata,
                event_local_file: Some(file),
            },
        ))
    }

    pub fn mark_client_released(&mut self) {
        self.state = ClientHandleState::ClientReleased;
    }

    #[cfg(test)]
    fn reactivate_client_handle(&mut self) {
        if self.ever_exported {
            self.state = ClientHandleState::ExportedActive;
        }
    }

    pub fn data_id_state(&self, data_id: AvDataId) -> AvDataIdState {
        if self
            .slots
            .iter()
            .any(|slot| slot.active_data_id == Some(data_id))
        {
            AvDataIdState::ActiveShared
        } else if self.event_local_allocations.contains_key(&data_id) {
            AvDataIdState::ActiveEventLocal
        } else {
            AvDataIdState::Unknown
        }
    }

    pub fn allocate_payload(&mut self, data_length: usize) -> AvPayloadDeliveryOutcome {
        let slot_index = match self.shared_slot_candidate(data_length) {
            Ok(slot_index) => slot_index,
            Err(outcome) => return outcome,
        };
        let runtime_claim =
            match PendingAvRuntimeClaim::prepare(Arc::clone(&self.runtime_budget), data_length) {
                Ok(Some(claim)) => claim,
                Ok(None) | Err(_) => return AvPayloadDeliveryOutcome::NoFreeSlot,
            };
        let Some(data_id) = self.data_id_allocator.issue() else {
            return AvPayloadDeliveryOutcome::DataIdExhausted;
        };
        let slot = &mut self.slots[slot_index];
        slot.active_data_id = Some(data_id);
        slot.data_length = data_length;
        runtime_claim.commit();
        AvPayloadDeliveryOutcome::Delivered(AvMediaEventDescriptor {
            data_id,
            slot_id: slot.slot_id,
            offset: slot.slot_id.0 as usize * self.slot_size,
            data_length,
            metadata: AvMediaEventMetadata::default(),
            event_local_file: None,
        })
    }

    fn shared_slot_candidate(&self, data_length: usize) -> Result<usize, AvPayloadDeliveryOutcome> {
        if !self.ever_exported {
            return Err(AvPayloadDeliveryOutcome::SharedHandleNotExported);
        }
        if self.state == ClientHandleState::ClientReleased {
            return Err(AvPayloadDeliveryOutcome::ClientHandleReleased);
        }
        if data_length == 0 {
            return Err(AvPayloadDeliveryOutcome::PayloadEmpty);
        }
        if data_length > self.max_event_bytes {
            return Err(AvPayloadDeliveryOutcome::PayloadOversized);
        }
        if data_length > self.slot_size {
            return Err(AvPayloadDeliveryOutcome::PayloadOversized);
        }
        let active_count = self
            .slots
            .iter()
            .filter(|slot| slot.active_data_id.is_some())
            .count()
            .checked_add(self.event_local_allocations.len());
        if !matches!(active_count, Some(count) if count < self.max_outstanding_events) {
            return Err(AvPayloadDeliveryOutcome::NoFreeSlot);
        }
        let live_bytes = self
            .slots
            .iter()
            .filter(|slot| slot.active_data_id.is_some())
            .try_fold(self.event_local_bytes, |total, slot| {
                total.checked_add(slot.data_length)
            })
            .and_then(|total| total.checked_add(data_length));
        if !matches!(live_bytes, Some(bytes) if bytes <= self.per_filter_live_bytes) {
            return Err(AvPayloadDeliveryOutcome::NoFreeSlot);
        }
        let Some(slot_index) = self
            .slots
            .iter()
            .position(|slot| slot.active_data_id.is_none())
        else {
            return Err(AvPayloadDeliveryOutcome::NoFreeSlot);
        };
        if !self.data_id_allocator.can_issue() {
            return Err(AvPayloadDeliveryOutcome::DataIdExhausted);
        }
        Ok(slot_index)
    }

    fn release_slot(&mut self, data_id: AvDataId) -> bool {
        if let Some(slot) = self
            .slots
            .iter_mut()
            .find(|slot| slot.active_data_id == Some(data_id))
        {
            if !self.runtime_budget.release(slot.data_length) {
                return false;
            }
            slot.active_data_id = None;
            slot.data_length = 0;
            true
        } else {
            false
        }
    }

    pub(crate) fn discard_undelivered_data_id(&mut self, data_id: AvDataId) -> bool {
        if let Some(allocation) = self.event_local_allocations.get(&data_id) {
            let Some(next_live_bytes) = self.event_local_bytes.checked_sub(allocation.length)
            else {
                return false;
            };
            if !self.runtime_budget.release(allocation.length) {
                return false;
            }
            self.event_local_allocations.remove(&data_id);
            self.event_local_bytes = next_live_bytes;
            true
        } else {
            self.release_slot(data_id)
        }
    }

    pub fn classify_release(
        &self,
        descriptor: AvHandleReleaseDescriptor,
        data_id: AvDataId,
        filter_state: AvFilterReleaseState,
    ) -> AvHandleReleaseOutcome {
        let handle_kind = match descriptor {
            AvHandleReleaseDescriptor::Empty => AvHandleReleaseKind::Empty,
            AvHandleReleaseDescriptor::File(identity)
                if self.shared_handle_identity == Some(identity) =>
            {
                AvHandleReleaseKind::Shared
            }
            AvHandleReleaseDescriptor::File(identity) => match self
                .event_local_allocations
                .iter()
                .find_map(|(allocation_data_id, allocation)| {
                    (allocation.file_identity == identity).then_some(
                        AvHandleReleaseKind::EventLocal {
                            data_id: *allocation_data_id,
                            lease_state: allocation.handle_lease_state,
                        },
                    )
                }) {
                Some(kind) => kind,
                None => AvHandleReleaseKind::UnknownFile,
            },
        };
        AvHandleReleaseTxn::classify(AvHandleReleaseInput {
            handle_kind,
            data_id,
            client_state: self.state,
            filter_state,
            data_id_state: self.data_id_state(data_id),
        })
    }

    pub fn apply_release(
        &mut self,
        descriptor: AvHandleReleaseDescriptor,
        data_id: AvDataId,
        filter_state: AvFilterReleaseState,
    ) -> AvHandleReleaseOutcome {
        let outcome = self.classify_release(descriptor, data_id, filter_state);
        match outcome {
            AvHandleReleaseOutcome::ClientHandleReleased
            | AvHandleReleaseOutcome::ClientHandleReleaseAfterClose => self.mark_client_released(),
            AvHandleReleaseOutcome::EventLocalHandleReleased { data_id } => {
                let Some(allocation) = self.event_local_allocations.get_mut(&data_id) else {
                    return AvHandleReleaseOutcome::RegistryFailure;
                };
                allocation.handle_lease_state = AvEventLocalHandleLeaseState::Finalized;
            }
            AvHandleReleaseOutcome::SlotReleased { data_id } => {
                if let Some(allocation) = self.event_local_allocations.get(&data_id) {
                    let Some(next_live_bytes) =
                        self.event_local_bytes.checked_sub(allocation.length)
                    else {
                        return AvHandleReleaseOutcome::RegistryFailure;
                    };
                    if !self.runtime_budget.release(allocation.length) {
                        return AvHandleReleaseOutcome::RegistryFailure;
                    }
                    self.event_local_allocations.remove(&data_id);
                    self.event_local_bytes = next_live_bytes;
                } else {
                    if !self.release_slot(data_id) {
                        return AvHandleReleaseOutcome::RegistryFailure;
                    }
                }
            }
            _ => {}
        }
        outcome
    }

    pub fn apply_release_after_close(
        &mut self,
        descriptor: AvHandleReleaseDescriptor,
        data_id: AvDataId,
    ) -> AvHandleReleaseOutcome {
        self.apply_release(descriptor, data_id, AvFilterReleaseState::Closed)
    }

    pub fn release_is_complete(&self) -> bool {
        self.state != ClientHandleState::ExportedActive
            && self.slots.iter().all(|slot| slot.active_data_id.is_none())
            && self.event_local_allocations.is_empty()
    }

    pub fn released_shared_handle_identity(&self) -> Option<AvFileIdentity> {
        (self.state == ClientHandleState::ClientReleased)
            .then_some(self.shared_handle_identity)
            .flatten()
    }

    pub fn flush_slots_keep_exported_handle(&mut self) {
        // flush対象は未配送eventとparserの一過性状態だけである。
        // 公開済みdataIdの領域はreleaseAvHandle()まで維持する。
    }
}

const PROT_READ: i32 = 0x1;
const PROT_WRITE: i32 = 0x2;
const MAP_SHARED: i32 = 0x01;
const MAP_FAILED: *mut std::ffi::c_void = !0usize as *mut std::ffi::c_void;

extern "C" {
    fn mmap(
        addr: *mut std::ffi::c_void,
        length: usize,
        prot: i32,
        flags: i32,
        fd: i32,
        offset: i64,
    ) -> *mut std::ffi::c_void;
    fn munmap(addr: *mut std::ffi::c_void, length: usize) -> i32;
    fn tuner_dmabuf_heap_alloc_system(len: usize) -> i32;
}

impl Default for AvSharedBacking {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for AvSharedBacking {
    fn drop(&mut self) {
        let shared_bytes = self
            .slots
            .iter()
            .filter(|slot| slot.active_data_id.is_some())
            .fold(0usize, |total, slot| total.saturating_add(slot.data_length));
        let total = shared_bytes.saturating_add(self.event_local_bytes);
        if total != 0 {
            self.runtime_budget.release_after_owner_drop(total);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_delivery_requires_exported_active_client_handle() {
        let mut backing = AvSharedBacking::with_layout(1, 188);
        assert_eq!(
            backing.allocate_payload(188),
            AvPayloadDeliveryOutcome::SharedHandleNotExported
        );
        backing.mark_exported();
        backing.mark_client_released();
        assert_eq!(
            backing.allocate_payload(188),
            AvPayloadDeliveryOutcome::ClientHandleReleased
        );
        backing.reactivate_client_handle();
        assert!(matches!(
            backing.allocate_payload(188),
            AvPayloadDeliveryOutcome::Delivered(_)
        ));
    }

    #[test]
    fn empty_data_id_zero_release_is_a_noop() {
        let mut backing = AvSharedBacking::with_layout(2, 188);
        backing.mark_exported();
        let delivered = match backing.allocate_payload(188) {
            AvPayloadDeliveryOutcome::Delivered(event) => event,
            other => panic!("unexpected outcome: {other:?}"),
        };
        assert_eq!(backing.active_slot_count(), 1);
        assert_eq!(
            backing.apply_release(
                AvHandleReleaseDescriptor::Empty,
                AvDataId(0),
                AvFilterReleaseState::OpenAv,
            ),
            AvHandleReleaseOutcome::EmptyHandleAccepted
        );
        assert_eq!(backing.client_state(), ClientHandleState::ExportedActive);
        assert_eq!(backing.active_slot_count(), 1);
        assert_eq!(
            backing.data_id_state(delivered.data_id),
            AvDataIdState::ActiveShared
        );
    }

    #[test]
    fn active_slot_release_removes_token_without_a_tombstone() {
        let mut backing = AvSharedBacking::with_layout(1, 188);
        backing.mark_exported();
        let delivered = match backing.allocate_payload(100) {
            AvPayloadDeliveryOutcome::Delivered(event) => event,
            other => panic!("unexpected outcome: {other:?}"),
        };
        assert_eq!(
            backing.apply_release(
                AvHandleReleaseDescriptor::Empty,
                delivered.data_id,
                AvFilterReleaseState::OpenAv,
            ),
            AvHandleReleaseOutcome::SlotReleased {
                data_id: delivered.data_id
            }
        );
        assert_eq!(backing.active_slot_count(), 0);
        assert_eq!(
            backing.data_id_state(delivered.data_id),
            AvDataIdState::Unknown
        );
        assert_eq!(
            backing.apply_release(
                AvHandleReleaseDescriptor::Empty,
                delivered.data_id,
                AvFilterReleaseState::OpenAv,
            ),
            AvHandleReleaseOutcome::UnknownDataId
        );
    }

    #[test]
    fn release_only_backing_releases_active_slot_after_logical_close() {
        let mut backing = AvSharedBacking::with_layout(1, 188);
        backing.mark_exported();
        let shared_identity = AvFileIdentity::new(1, 2, 188);
        backing.shared_handle_identity = Some(shared_identity);
        let delivered = match backing.allocate_payload(188) {
            AvPayloadDeliveryOutcome::Delivered(event) => event,
            other => panic!("unexpected outcome: {other:?}"),
        };
        assert_eq!(
            backing.apply_release_after_close(AvHandleReleaseDescriptor::Empty, delivered.data_id,),
            AvHandleReleaseOutcome::SlotReleased {
                data_id: delivered.data_id
            }
        );
        assert!(!backing.release_is_complete());
        assert_eq!(
            backing.apply_release_after_close(
                AvHandleReleaseDescriptor::File(shared_identity),
                AvDataId(0),
            ),
            AvHandleReleaseOutcome::ClientHandleReleaseAfterClose
        );
        assert!(backing.release_is_complete());
    }

    #[test]
    fn zero_length_payload_does_not_publish_an_allocation() {
        let mut backing = AvSharedBacking::with_layout(1, 188);
        backing.mark_exported();

        assert_eq!(
            backing
                .allocate_payload_bytes(&[], AvMediaEventMetadata::default())
                .unwrap(),
            AvPayloadDeliveryOutcome::PayloadEmpty
        );
        assert_eq!(backing.active_slot_count(), 0);
        assert_eq!(backing.data_id_allocator.issued_high_watermark(), 0);
    }

    #[test]
    fn data_id_high_watermark_issues_i64_max_once_without_tombstones() {
        let mut backing = AvSharedBacking::with_layout(1, 188);
        backing.mark_exported();
        backing
            .data_id_allocator
            .set_issued_high_watermark(i64::MAX - 1);

        let delivered = match backing.allocate_payload(188) {
            AvPayloadDeliveryOutcome::Delivered(event) => event,
            other => panic!("unexpected outcome: {other:?}"),
        };
        assert_eq!(delivered.data_id, AvDataId(i64::MAX));
        assert_eq!(backing.data_id_allocator.issued_high_watermark(), i64::MAX);
        assert_eq!(
            backing.apply_release(
                AvHandleReleaseDescriptor::Empty,
                delivered.data_id,
                AvFilterReleaseState::OpenAv,
            ),
            AvHandleReleaseOutcome::SlotReleased {
                data_id: delivered.data_id
            }
        );
        assert_eq!(
            backing.data_id_state(delivered.data_id),
            AvDataIdState::Unknown
        );
        assert_eq!(
            backing.allocate_payload(188),
            AvPayloadDeliveryOutcome::DataIdExhausted
        );
    }

    #[test]
    fn flush_preserves_exported_handle_and_delivered_slots() {
        let mut backing = AvSharedBacking::with_layout(1, 188);
        backing.mark_exported();
        let delivered = match backing.allocate_payload(100) {
            AvPayloadDeliveryOutcome::Delivered(event) => event,
            other => panic!("unexpected outcome: {other:?}"),
        };
        backing.flush_slots_keep_exported_handle();
        assert_eq!(backing.client_state(), ClientHandleState::ExportedActive);
        assert_eq!(backing.active_slot_count(), 1);
        assert_eq!(
            backing.data_id_state(delivered.data_id),
            AvDataIdState::ActiveShared
        );
    }

    #[test]
    fn shared_allocator_never_reuses_ids_across_backings() {
        let allocator = Arc::new(AvDataIdAllocator::default());
        let runtime_budget = Arc::new(AvRuntimeBudget::new(376));
        let mut first = AvSharedBacking::with_profile_limits(
            1,
            188,
            188,
            1,
            188,
            Arc::clone(&allocator),
            Arc::clone(&runtime_budget),
        );
        let mut second =
            AvSharedBacking::with_profile_limits(1, 188, 188, 1, 188, allocator, runtime_budget);
        first.mark_exported();
        second.mark_exported();
        let first_id = match first.allocate_payload(188) {
            AvPayloadDeliveryOutcome::Delivered(event) => event.data_id,
            other => panic!("unexpected outcome: {other:?}"),
        };
        let second_id = match second.allocate_payload(188) {
            AvPayloadDeliveryOutcome::Delivered(event) => event.data_id,
            other => panic!("unexpected outcome: {other:?}"),
        };
        assert_eq!(first_id, AvDataId(1));
        assert_eq!(second_id, AvDataId(2));
    }

    #[test]
    fn runtime_budget_tracks_actual_live_payload_bytes_across_backings() {
        let allocator = Arc::new(AvDataIdAllocator::default());
        let runtime_budget = Arc::new(AvRuntimeBudget::new(250));
        let mut first = AvSharedBacking::with_runtime_limits(
            188,
            1,
            188,
            Arc::clone(&allocator),
            Arc::clone(&runtime_budget),
        );
        let mut second = AvSharedBacking::with_runtime_limits(
            188,
            1,
            188,
            allocator,
            Arc::clone(&runtime_budget),
        );
        first.mark_exported();
        second.mark_exported();
        let first_id = match first.allocate_payload(188) {
            AvPayloadDeliveryOutcome::Delivered(event) => event.data_id,
            other => panic!("unexpected outcome: {other:?}"),
        };
        assert_eq!(runtime_budget.used_bytes(), 188);
        assert_eq!(
            second.allocate_payload(100),
            AvPayloadDeliveryOutcome::NoFreeSlot
        );
        assert_eq!(
            first.apply_release(
                AvHandleReleaseDescriptor::Empty,
                first_id,
                AvFilterReleaseState::OpenAv,
            ),
            AvHandleReleaseOutcome::SlotReleased { data_id: first_id }
        );
        assert_eq!(runtime_budget.used_bytes(), 0);
        assert!(matches!(
            second.allocate_payload(100),
            AvPayloadDeliveryOutcome::Delivered(_)
        ));
        assert_eq!(runtime_budget.used_bytes(), 100);
    }
}

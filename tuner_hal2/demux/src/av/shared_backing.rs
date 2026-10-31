use super::event::{AvMediaEventDescriptor, AvMediaEventMetadata};
use super::release_txn::{
    AvDataIdState, AvEventLocalHandleLeaseState, AvHandleReleaseInput, AvHandleReleaseKind,
    AvHandleReleaseOutcome, AvHandleReleaseTxn,
};
use super::slot::AvDataId;
use std::collections::BTreeMap;
use std::fs::File;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::MetadataExt;
use std::ptr;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

pub const DEFAULT_AV_MAX_EVENT_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AvPayloadDeliveryOutcome {
    Delivered(AvMediaEventDescriptor),
    PayloadEmpty,
    PayloadOversized,
    DataIdExhausted,
}

#[derive(Debug)]
pub struct AvSharedHandleExport {
    pub file: Option<File>,
    pub size_bytes: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AvSharedBackingError {
    InvariantViolation,
    AllocationFailed,
    DuplicateFailed,
    IdentityFailed,
    MappingFailed,
    UnmappingFailed,
}

impl AvSharedBackingError {
    pub(crate) const fn is_retryable_allocation_failure(self) -> bool {
        matches!(
            self,
            Self::AllocationFailed
                | Self::DuplicateFailed
                | Self::IdentityFailed
                | Self::MappingFailed
        )
    }
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

#[derive(Debug)]
struct EventLocalAllocation {
    _file: Arc<File>,
    file_identity: AvFileIdentity,
    handle_lease_state: AvEventLocalHandleLeaseState,
    length: usize,
}

/// AV tokenの所有権を変更せず、割当・解放の進捗を診断する。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AvFlowDiagnosticSnapshot {
    pub delivered: u64,
    pub released: u64,
    pub discarded_undelivered: u64,
    pub allocation_failures: u64,
    pub active_event_local: usize,
    pub active_bytes: usize,
}

#[derive(Debug, Default)]
struct AvFlowDiagnosticCounters {
    delivered: u64,
    released: u64,
    discarded_undelivered: u64,
    allocation_failures: u64,
    last_sampled_delivery_threshold: u64,
    last_sampled_shortage_threshold: u64,
}

#[derive(Debug)]
pub struct AvSharedBacking {
    data_id_allocator: Arc<AvDataIdAllocator>,
    max_event_bytes: usize,
    event_local_allocations: BTreeMap<AvDataId, EventLocalAllocation>,
    event_local_bytes: usize,
    diagnostics: AvFlowDiagnosticCounters,
}

impl AvSharedBacking {
    pub fn new() -> Self {
        Self::with_data_id_allocator(Arc::new(AvDataIdAllocator::default()))
    }

    pub fn with_data_id_allocator(data_id_allocator: Arc<AvDataIdAllocator>) -> Self {
        Self::with_runtime_limits(DEFAULT_AV_MAX_EVENT_BYTES, data_id_allocator)
    }

    pub fn with_runtime_limits(
        max_event_bytes: usize,
        data_id_allocator: Arc<AvDataIdAllocator>,
    ) -> Self {
        Self {
            data_id_allocator,
            max_event_bytes,
            event_local_allocations: BTreeMap::new(),
            event_local_bytes: 0,
            diagnostics: AvFlowDiagnosticCounters::default(),
        }
    }

    /// 正本の使用中スロットと診断専用の累計値を取得する。
    pub fn diagnostic_snapshot(&self) -> AvFlowDiagnosticSnapshot {
        AvFlowDiagnosticSnapshot {
            delivered: self.diagnostics.delivered,
            released: self.diagnostics.released,
            discarded_undelivered: self.diagnostics.discarded_undelivered,
            allocation_failures: self.diagnostics.allocation_failures,
            active_event_local: self.event_local_allocations.len(),
            active_bytes: self.event_local_bytes,
        }
    }

    // 配送イベントはreport内で複数件まとめられる。実割当済みの累積値で
    // 通過した2の累乗閾値を一度だけ消費し、同じreportの複数eventで再出力しない。
    // callback保留中でreport.generated_eventsが空でも割当進捗を観測できる。
    pub(crate) fn take_delivery_sample(&mut self) -> Option<(u64, u64, AvFlowDiagnosticSnapshot)> {
        let delivered = self.diagnostics.delivered;
        if delivered == 0 {
            return None;
        }
        let threshold = 1u64 << (u64::BITS - 1 - delivered.leading_zeros());
        let previous = self.diagnostics.last_sampled_delivery_threshold;
        if threshold <= previous {
            return None;
        }
        self.diagnostics.last_sampled_delivery_threshold = threshold;
        Some((previous, threshold, self.diagnostic_snapshot()))
    }

    // 直前に観測した閾値より高い2の累乗へ達した場合だけ消費する。
    pub(crate) fn take_shortage_sample(&mut self) -> Option<(u64, u64, AvFlowDiagnosticSnapshot)> {
        let shortage = self.diagnostics.allocation_failures;
        if shortage == 0 {
            return None;
        }
        let threshold = 1u64 << (u64::BITS - 1 - shortage.leading_zeros());
        let previous = self.diagnostics.last_sampled_shortage_threshold;
        if threshold <= previous {
            return None;
        }
        self.diagnostics.last_sampled_shortage_threshold = threshold;
        Some((previous, threshold, self.diagnostic_snapshot()))
    }

    fn increment_diagnostic_counter(counter: &mut u64, name: &'static str) {
        if *counter == u64::MAX - 1 {
            eprintln!(
                "diagnostic_counter_saturated counter={name} owner=AvSharedBacking instance={counter:p}"
            );
        }
        *counter = counter.saturating_add(1);
    }

    fn record_delivery_outcome(&mut self, outcome: &AvPayloadDeliveryOutcome) {
        let (counter, name) = match outcome {
            AvPayloadDeliveryOutcome::Delivered(_) => {
                (&mut self.diagnostics.delivered, "delivered")
            }
            _ => return,
        };
        Self::increment_diagnostic_counter(counter, name);
    }

    #[cfg(test)]
    pub(crate) fn active_allocation_count(&self) -> usize {
        self.event_local_allocations.len()
    }

    pub fn export_handle(&mut self) -> Result<AvSharedHandleExport, AvSharedBackingError> {
        // イベントごとのFDを使うため、共有ハンドルは未初期化として返す。
        Ok(AvSharedHandleExport {
            file: None,
            size_bytes: 0,
        })
    }

    pub fn allocate_payload_bytes(
        &mut self,
        payload: &[u8],
        metadata: AvMediaEventMetadata,
    ) -> Result<AvPayloadDeliveryOutcome, AvSharedBackingError> {
        let result = self.allocate_event_local_payload(payload, metadata);
        match &result {
            Ok(outcome) => self.record_delivery_outcome(outcome),
            Err(error) if error.is_retryable_allocation_failure() => {
                Self::increment_diagnostic_counter(
                    &mut self.diagnostics.allocation_failures,
                    "allocation_failures",
                )
            }
            Err(_) => {}
        }
        result
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
        let next_live_bytes = self
            .event_local_bytes
            .checked_add(payload.len())
            .ok_or(AvSharedBackingError::InvariantViolation)?;
        if !self.data_id_allocator.can_issue() {
            return Ok(AvPayloadDeliveryOutcome::DataIdExhausted);
        }
        // SAFETY: payloadは非emptyでlenは値渡しし、allocatorは新規FDまたは負のerrorを返す。
        let raw_fd = unsafe { tuner_dmabuf_heap_alloc_system(payload.len()) };
        // 事後条件: 非負の新規FDだけを一意なFile所有権へ移管する。
        if raw_fd < 0 {
            return Err(AvSharedBackingError::AllocationFailed);
        }
        // SAFETY: raw_fdは新規かつ非負でRust ownerを持たず、Fileへ所有権を正確に1回だけ移管する。
        let file = Arc::new(unsafe { File::from_raw_fd(raw_fd) });
        // 事後条件: Arc<File>がclose責務を所有し、raw_fdを別途wrapまたはcloseしない。
        // SAFETY: fileはliveで、payload.len()は非0のmapping範囲、offsetは0で、要求mappingとaliasするRust参照はない。
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
        // SAFETY: payloadはlen byte読取り可能、mappedは同じlenを書込み可能で、独立dmabuf storageはsliceと重ならない。
        unsafe {
            ptr::copy_nonoverlapping(payload.as_ptr(), mapped as *mut u8, payload.len());
        }
        // 事後条件: 完全なevent payloadがmapped dmabuf領域を占有する。
        // SAFETY: mappedは成功した同一mmap pointerで、payload.len()も同一mapping長であり、mapped参照を外へ持ち出さない。
        if unsafe { munmap(mapped, payload.len()) } != 0 {
            return Err(AvSharedBackingError::UnmappingFailed);
        }
        // 事後条件: munmap後はevent-local mappingを無効として扱い、再アクセスしない。
        let file_identity = AvFileIdentity::from_file(&file)?;
        let Some(data_id) = self.data_id_allocator.issue() else {
            return Ok(AvPayloadDeliveryOutcome::DataIdExhausted);
        };
        self.event_local_bytes = next_live_bytes;
        self.event_local_allocations.insert(
            data_id,
            EventLocalAllocation {
                _file: Arc::clone(&file),
                file_identity,
                handle_lease_state: AvEventLocalHandleLeaseState::Active,
                length: payload.len(),
            },
        );
        Ok(AvPayloadDeliveryOutcome::Delivered(
            AvMediaEventDescriptor {
                data_id,

                offset: 0,
                data_length: payload.len(),
                metadata,
                event_local_file: Some(file),
            },
        ))
    }

    pub fn data_id_state(&self, data_id: AvDataId) -> AvDataIdState {
        if self.event_local_allocations.contains_key(&data_id) {
            AvDataIdState::ActiveEventLocal
        } else {
            AvDataIdState::Unknown
        }
    }

    #[cfg(test)]
    pub fn allocate_payload(&mut self, data_length: usize) -> AvPayloadDeliveryOutcome {
        self.allocate_payload_bytes(&vec![0; data_length], AvMediaEventMetadata::default())
            .expect("test allocation failed")
    }

    pub(crate) fn discard_undelivered_data_id(&mut self, data_id: AvDataId) -> bool {
        let discarded = if let Some(allocation) = self.event_local_allocations.get(&data_id) {
            let Some(next_live_bytes) = self.event_local_bytes.checked_sub(allocation.length)
            else {
                return false;
            };
            self.event_local_allocations.remove(&data_id);
            self.event_local_bytes = next_live_bytes;
            true
        } else {
            false
        };
        if discarded {
            Self::increment_diagnostic_counter(
                &mut self.diagnostics.discarded_undelivered,
                "discarded_undelivered",
            );
        }
        discarded
    }

    pub fn classify_release(
        &self,
        descriptor: AvHandleReleaseDescriptor,
        data_id: AvDataId,
    ) -> AvHandleReleaseOutcome {
        let handle_kind = match descriptor {
            AvHandleReleaseDescriptor::Empty => AvHandleReleaseKind::Empty,
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
            data_id_state: self.data_id_state(data_id),
        })
    }

    pub fn apply_release(
        &mut self,
        descriptor: AvHandleReleaseDescriptor,
        data_id: AvDataId,
    ) -> AvHandleReleaseOutcome {
        let outcome = self.classify_release(descriptor, data_id);
        match outcome {
            AvHandleReleaseOutcome::EventLocalHandleReleased { data_id } => {
                let Some(allocation) = self.event_local_allocations.get_mut(&data_id) else {
                    return AvHandleReleaseOutcome::RegistryFailure;
                };
                allocation.handle_lease_state = AvEventLocalHandleLeaseState::Finalized;
            }
            AvHandleReleaseOutcome::AllocationReleased { data_id } => {
                if let Some(allocation) = self.event_local_allocations.get(&data_id) {
                    let Some(next_live_bytes) =
                        self.event_local_bytes.checked_sub(allocation.length)
                    else {
                        return AvHandleReleaseOutcome::RegistryFailure;
                    };
                    self.event_local_allocations.remove(&data_id);
                    self.event_local_bytes = next_live_bytes;
                } else {
                    return AvHandleReleaseOutcome::RegistryFailure;
                }
                Self::increment_diagnostic_counter(&mut self.diagnostics.released, "released");
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
        self.apply_release(descriptor, data_id)
    }

    pub fn release_is_complete(&self) -> bool {
        self.event_local_allocations.is_empty()
    }

    pub fn preserve_delivered_allocations_on_flush(&mut self) {
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
    #[cfg(not(test))]
    fn tuner_dmabuf_heap_alloc_system(len: usize) -> i32;
    #[cfg(test)]
    fn memfd_create(name: *const std::ffi::c_char, flags: u32) -> i32;
    #[cfg(test)]
    fn ftruncate(fd: i32, length: i64) -> i32;
    #[cfg(test)]
    fn close(fd: i32) -> i32;
}

#[cfg(test)]
std::thread_local! {
    static FAIL_DMABUF_ALLOCATION: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
pub(crate) fn with_failed_dmabuf_allocations_for_test<T>(run: impl FnOnce() -> T) -> T {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            FAIL_DMABUF_ALLOCATION.set(self.0);
        }
    }
    let _restore = Restore(FAIL_DMABUF_ALLOCATION.replace(true));
    run()
}

#[cfg(test)]
#[no_mangle]
unsafe extern "C" fn tuner_dmabuf_heap_alloc_system(len: usize) -> i32 {
    if FAIL_DMABUF_ALLOCATION.get() {
        return -1;
    }
    let Ok(length) = i64::try_from(len) else {
        return -1;
    };
    let fd = memfd_create(b"maleicacid_tuner_hal2_av\0".as_ptr().cast(), 0);
    if fd < 0 {
        return -1;
    }
    if ftruncate(fd, length) != 0 {
        let _ = close(fd);
        return -1;
    }
    fd
}

impl Default for AvSharedBacking {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_payloads_can_exceed_the_former_service_byte_budget() {
        let mut backing = AvSharedBacking::new();
        let payload = vec![0x5c; DEFAULT_AV_MAX_EVENT_BYTES];
        for _ in 0..17 {
            assert!(matches!(
                backing
                    .allocate_payload_bytes(&payload, AvMediaEventMetadata::default())
                    .unwrap(),
                AvPayloadDeliveryOutcome::Delivered(_)
            ));
        }
        assert_eq!(
            backing.diagnostic_snapshot().active_bytes,
            17 * payload.len()
        );
    }

    #[test]
    fn delivery_and_failure_samples_are_consumed_once_per_crossed_threshold() {
        let mut backing = AvSharedBacking::new();
        for _ in 0..3 {
            assert!(matches!(
                backing
                    .allocate_payload_bytes(&[1], AvMediaEventMetadata::default())
                    .unwrap(),
                AvPayloadDeliveryOutcome::Delivered(_)
            ));
        }
        let (previous, threshold, snapshot) = backing.take_delivery_sample().unwrap();
        assert_eq!((previous, threshold, snapshot.delivered), (0, 2, 3));
        assert!(backing.take_delivery_sample().is_none());
        with_failed_dmabuf_allocations_for_test(|| {
            for _ in 0..3 {
                assert_eq!(
                    backing.allocate_payload_bytes(&[1], AvMediaEventMetadata::default()),
                    Err(AvSharedBackingError::AllocationFailed)
                );
            }
        });
        let (previous, threshold, snapshot) = backing.take_shortage_sample().unwrap();
        assert_eq!(
            (previous, threshold, snapshot.allocation_failures),
            (0, 2, 3)
        );
        assert!(backing.take_shortage_sample().is_none());
        assert_eq!(snapshot.active_event_local, 3);
    }

    #[test]
    fn event_allocations_are_not_limited_by_the_former_pool_or_count_budget() {
        use std::os::unix::fs::FileExt;
        let mut backing = AvSharedBacking::new();
        let shared = backing.export_handle().unwrap();
        assert!(shared.file.is_none());
        assert_eq!(shared.size_bytes, 0);
        let mut events = Vec::new();
        for value in 0..128u8 {
            let event = match backing
                .allocate_payload_bytes(&[value; 32], AvMediaEventMetadata::default())
                .unwrap()
            {
                AvPayloadDeliveryOutcome::Delivered(event) => event,
                other => panic!("unexpected allocation: {other:?}"),
            };
            assert_eq!(event.offset, 0);
            events.push(event);
        }
        assert_eq!(backing.diagnostic_snapshot().active_event_local, 128);
        assert_eq!(backing.diagnostic_snapshot().active_bytes, 4096);
        for (value, event) in events.into_iter().enumerate().rev() {
            let file = event.event_local_file.as_ref().unwrap();
            let mut observed = [0; 32];
            file.read_exact_at(&mut observed, 0).unwrap();
            assert_eq!(observed, [value as u8; 32]);
            let handle = AvHandleReleaseDescriptor::File(AvFileIdentity::from_file(file).unwrap());
            assert_eq!(
                backing.apply_release_after_close(handle, event.data_id),
                AvHandleReleaseOutcome::AllocationReleased {
                    data_id: event.data_id
                }
            );
        }
        assert!(backing.release_is_complete());
        assert_eq!(backing.diagnostic_snapshot().active_bytes, 0);
    }

    #[test]
    fn event_ids_do_not_wrap_or_reappear_after_release() {
        let allocator = Arc::new(AvDataIdAllocator::default());
        allocator.set_issued_high_watermark(i64::MAX - 1);
        let mut backing = AvSharedBacking::with_data_id_allocator(allocator);
        let event = match backing
            .allocate_payload_bytes(&[1], AvMediaEventMetadata::default())
            .unwrap()
        {
            AvPayloadDeliveryOutcome::Delivered(event) => event,
            other => panic!("unexpected allocation: {other:?}"),
        };
        assert_eq!(event.data_id, AvDataId(i64::MAX));
        assert!(backing.discard_undelivered_data_id(event.data_id));
        assert_eq!(
            backing
                .allocate_payload_bytes(&[2], AvMediaEventMetadata::default())
                .unwrap(),
            AvPayloadDeliveryOutcome::DataIdExhausted
        );
        assert_eq!(backing.data_id_state(event.data_id), AvDataIdState::Unknown);
    }

    #[test]
    fn saturated_diagnostics_do_not_block_allocation_and_discard() {
        let mut backing = AvSharedBacking::new();
        backing.diagnostics.delivered = u64::MAX;
        backing.diagnostics.discarded_undelivered = u64::MAX;
        let event = match backing
            .allocate_payload_bytes(&[1], AvMediaEventMetadata::default())
            .unwrap()
        {
            AvPayloadDeliveryOutcome::Delivered(event) => event,
            other => panic!("unexpected allocation: {other:?}"),
        };
        assert!(backing.discard_undelivered_data_id(event.data_id));
        assert_eq!(backing.diagnostic_snapshot().delivered, u64::MAX);
        assert_eq!(
            backing.diagnostic_snapshot().discarded_undelivered,
            u64::MAX
        );
        assert!(backing.release_is_complete());
    }

    #[test]
    fn event_local_payload_survives_flush_and_releases_after_close_without_shared_export() {
        use std::os::unix::fs::FileExt;

        let mut backing = AvSharedBacking::new();
        let payload = [0x5a; 17];
        let event = match backing
            .allocate_payload_bytes(&payload, AvMediaEventMetadata::default())
            .unwrap()
        {
            AvPayloadDeliveryOutcome::Delivered(event) => event,
            other => panic!("unexpected allocation: {other:?}"),
        };
        let file = event.event_local_file.as_ref().unwrap();
        let handle = AvHandleReleaseDescriptor::File(AvFileIdentity::from_file(file).unwrap());
        backing.preserve_delivered_allocations_on_flush();
        assert!(!backing.release_is_complete());
        let mut observed = [0; 17];
        file.read_exact_at(&mut observed, 0).unwrap();
        assert_eq!(observed, payload);
        assert_eq!(
            backing.apply_release_after_close(handle, event.data_id),
            AvHandleReleaseOutcome::AllocationReleased {
                data_id: event.data_id
            }
        );
        assert!(backing.release_is_complete());
        assert_eq!(backing.diagnostic_snapshot().active_bytes, 0);
        assert_eq!(backing.diagnostic_snapshot().released, 1);
    }

    #[test]
    fn event_local_handle_finalization_preserves_data_until_separate_token_return() {
        let mut backing = AvSharedBacking::new();
        let event = match backing
            .allocate_payload_bytes(&[0x35; 17], AvMediaEventMetadata::default())
            .unwrap()
        {
            AvPayloadDeliveryOutcome::Delivered(event) => event,
            other => panic!("unexpected allocation: {other:?}"),
        };
        let file = event.event_local_file.as_ref().unwrap();
        let handle = AvHandleReleaseDescriptor::File(AvFileIdentity::from_file(file).unwrap());
        assert_eq!(
            backing.apply_release_after_close(handle, AvDataId(0)),
            AvHandleReleaseOutcome::EventLocalHandleReleased {
                data_id: event.data_id
            }
        );
        assert_eq!(backing.diagnostic_snapshot().active_bytes, 17);
        assert!(!backing.release_is_complete());
        assert_eq!(
            backing.apply_release_after_close(AvHandleReleaseDescriptor::Empty, event.data_id),
            AvHandleReleaseOutcome::AllocationReleased {
                data_id: event.data_id
            }
        );
        assert!(backing.release_is_complete());
    }

    #[test]
    fn failed_event_local_allocation_keeps_existing_payload_and_does_not_issue_token() {
        use std::os::unix::fs::FileExt;

        let mut backing = AvSharedBacking::new();
        let event = match backing
            .allocate_payload_bytes(&[0x73; 17], AvMediaEventMetadata::default())
            .unwrap()
        {
            AvPayloadDeliveryOutcome::Delivered(event) => event,
            other => panic!("unexpected allocation: {other:?}"),
        };
        let before = backing.diagnostic_snapshot();
        let last_id = backing.data_id_allocator.issued_high_watermark();
        with_failed_dmabuf_allocations_for_test(|| {
            assert_eq!(
                backing.allocate_payload_bytes(&[0x19; 17], AvMediaEventMetadata::default()),
                Err(AvSharedBackingError::AllocationFailed)
            );
        });
        let after = backing.diagnostic_snapshot();
        assert_eq!(after.active_bytes, before.active_bytes);
        assert_eq!(after.delivered, before.delivered);
        assert_eq!(after.allocation_failures, before.allocation_failures + 1);
        assert_eq!(backing.data_id_allocator.issued_high_watermark(), last_id);
        let mut observed = [0; 17];
        event
            .event_local_file
            .as_ref()
            .unwrap()
            .read_exact_at(&mut observed, 0)
            .unwrap();
        assert_eq!(observed, [0x73; 17]);
        assert!(matches!(
            backing
                .allocate_payload_bytes(&[0x19; 17], AvMediaEventMetadata::default())
                .unwrap(),
            AvPayloadDeliveryOutcome::Delivered(_)
        ));
    }
}

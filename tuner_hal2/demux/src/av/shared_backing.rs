use super::event::AvMediaEventDescriptor;
use super::release_txn::{
    AvDataIdState, AvFilterReleaseState, AvHandleReleaseInput, AvHandleReleaseOutcome,
    AvHandleReleaseTxn,
};
use super::slot::{AvDataId, AvSlotId};

pub const DEFAULT_AV_SHARED_SLOT_SIZE_BYTES: usize = 1024 * 1024;
pub const DEFAULT_AV_SHARED_SLOT_COUNT: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientHandleState {
    NotExported,
    ExportedActive,
    ClientReleased,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AvPayloadDeliveryOutcome {
    Delivered(AvMediaEventDescriptor),
    SharedHandleNotExported,
    ClientHandleReleased,
    PayloadOversized,
    NoFreeSlot,
    DataIdExhausted,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct AvSlotState {
    slot_id: AvSlotId,
    active_data_id: Option<AvDataId>,
    data_length: usize,
}

#[derive(Debug)]
pub struct AvSharedBacking {
    state: ClientHandleState,
    next_data_id: i64,
    slot_size: usize,
    slots: Vec<AvSlotState>,
    stale_data_ids: Vec<AvDataId>,
    ever_exported: bool,
}

impl AvSharedBacking {
    pub fn new() -> Self {
        Self::with_layout(
            DEFAULT_AV_SHARED_SLOT_COUNT,
            DEFAULT_AV_SHARED_SLOT_SIZE_BYTES,
        )
    }

    pub fn with_layout(slot_count: usize, slot_size: usize) -> Self {
        let slots = (0..slot_count)
            .map(|idx| AvSlotState {
                slot_id: AvSlotId(idx as u32),
                active_data_id: None,
                data_length: 0,
            })
            .collect();
        Self {
            state: ClientHandleState::NotExported,
            next_data_id: 1,
            slot_size,
            slots,
            stale_data_ids: Vec::new(),
            ever_exported: false,
        }
    }

    pub fn client_state(&self) -> ClientHandleState {
        self.state
    }
    pub fn slot_size(&self) -> usize {
        self.slot_size
    }
    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }
    pub fn shared_handle_exported(&self) -> bool {
        self.ever_exported
    }
    pub fn active_slot_count(&self) -> usize {
        self.slots
            .iter()
            .filter(|slot| slot.active_data_id.is_some())
            .count()
    }
    pub fn active_data_ids(&self) -> Vec<AvDataId> {
        self.slots
            .iter()
            .filter_map(|slot| slot.active_data_id)
            .collect()
    }

    pub fn mark_exported(&mut self) {
        self.state = ClientHandleState::ExportedActive;
        self.ever_exported = true;
    }

    pub fn mark_client_released(&mut self) {
        self.state = ClientHandleState::ClientReleased;
    }

    pub fn reactivate_client_handle(&mut self) {
        if self.ever_exported {
            self.state = ClientHandleState::ExportedActive;
        }
    }

    fn next_data_id(&mut self) -> Option<AvDataId> {
        if self.next_data_id <= 0 || self.next_data_id == i64::MAX {
            return None;
        }
        let id = AvDataId(self.next_data_id);
        self.next_data_id += 1;
        Some(id)
    }

    pub fn data_id_state(&self, data_id: AvDataId) -> AvDataIdState {
        if self
            .slots
            .iter()
            .any(|slot| slot.active_data_id == Some(data_id))
        {
            AvDataIdState::Active
        } else if self.stale_data_ids.contains(&data_id) {
            AvDataIdState::Stale
        } else {
            AvDataIdState::Unknown
        }
    }

    pub fn allocate_payload(&mut self, data_length: usize) -> AvPayloadDeliveryOutcome {
        if !self.ever_exported {
            return AvPayloadDeliveryOutcome::SharedHandleNotExported;
        }
        if self.state == ClientHandleState::ClientReleased {
            return AvPayloadDeliveryOutcome::ClientHandleReleased;
        }
        if data_length > self.slot_size {
            return AvPayloadDeliveryOutcome::PayloadOversized;
        }
        let Some(slot_index) = self
            .slots
            .iter()
            .position(|slot| slot.active_data_id.is_none())
        else {
            return AvPayloadDeliveryOutcome::NoFreeSlot;
        };
        let Some(data_id) = self.next_data_id() else {
            return AvPayloadDeliveryOutcome::DataIdExhausted;
        };
        let slot = &mut self.slots[slot_index];
        slot.active_data_id = Some(data_id);
        slot.data_length = data_length;
        AvPayloadDeliveryOutcome::Delivered(AvMediaEventDescriptor {
            data_id,
            slot_id: slot.slot_id,
            offset: slot.slot_id.0 as usize * self.slot_size,
            data_length,
        })
    }

    fn release_slot(&mut self, data_id: AvDataId) -> bool {
        if let Some(slot) = self
            .slots
            .iter_mut()
            .find(|slot| slot.active_data_id == Some(data_id))
        {
            slot.active_data_id = None;
            slot.data_length = 0;
            if !self.stale_data_ids.contains(&data_id) {
                self.stale_data_ids.push(data_id);
            }
            true
        } else {
            false
        }
    }

    pub fn classify_release(
        &self,
        has_fd: bool,
        data_id: AvDataId,
        filter_state: AvFilterReleaseState,
    ) -> AvHandleReleaseOutcome {
        AvHandleReleaseTxn::classify(AvHandleReleaseInput {
            has_fd,
            data_id,
            client_state: self.state,
            filter_state,
            shared_handle_exported: self.ever_exported,
            data_id_state: self.data_id_state(data_id),
        })
    }

    pub fn apply_release(
        &mut self,
        has_fd: bool,
        data_id: AvDataId,
        filter_state: AvFilterReleaseState,
    ) -> AvHandleReleaseOutcome {
        let outcome = self.classify_release(has_fd, data_id, filter_state);
        match outcome {
            AvHandleReleaseOutcome::ClientHandleReleased => self.mark_client_released(),
            AvHandleReleaseOutcome::SlotReleased { data_id } => {
                self.release_slot(data_id);
            }
            _ => {}
        }
        outcome
    }

    pub fn flush_slots_keep_exported_handle(&mut self) {
        for slot in &mut self.slots {
            if let Some(data_id) = slot.active_data_id.take() {
                if !self.stale_data_ids.contains(&data_id) {
                    self.stale_data_ids.push(data_id);
                }
            }
            slot.data_length = 0;
        }
    }
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
    fn data_id_zero_release_does_not_clear_active_slots() {
        let mut backing = AvSharedBacking::with_layout(2, 188);
        backing.mark_exported();
        let delivered = match backing.allocate_payload(188) {
            AvPayloadDeliveryOutcome::Delivered(event) => event,
            other => panic!("unexpected outcome: {other:?}"),
        };
        assert_eq!(backing.active_slot_count(), 1);
        assert_eq!(
            backing.apply_release(false, AvDataId(0), AvFilterReleaseState::OpenAv),
            AvHandleReleaseOutcome::ClientHandleReleased
        );
        assert_eq!(backing.client_state(), ClientHandleState::ClientReleased);
        assert_eq!(backing.active_slot_count(), 1);
        assert_eq!(
            backing.data_id_state(delivered.data_id),
            AvDataIdState::Active
        );
    }

    #[test]
    fn active_slot_release_marks_data_id_stale() {
        let mut backing = AvSharedBacking::with_layout(1, 188);
        backing.mark_exported();
        let delivered = match backing.allocate_payload(100) {
            AvPayloadDeliveryOutcome::Delivered(event) => event,
            other => panic!("unexpected outcome: {other:?}"),
        };
        assert_eq!(
            backing.apply_release(false, delivered.data_id, AvFilterReleaseState::OpenAv),
            AvHandleReleaseOutcome::SlotReleased {
                data_id: delivered.data_id
            }
        );
        assert_eq!(backing.active_slot_count(), 0);
        assert_eq!(
            backing.data_id_state(delivered.data_id),
            AvDataIdState::Stale
        );
        assert_eq!(
            backing.apply_release(false, delivered.data_id, AvFilterReleaseState::OpenAv),
            AvHandleReleaseOutcome::StaleReleaseAccepted {
                data_id: delivered.data_id
            }
        );
    }

    #[test]
    fn flush_preserves_exported_handle_and_stales_slots() {
        let mut backing = AvSharedBacking::with_layout(1, 188);
        backing.mark_exported();
        let delivered = match backing.allocate_payload(100) {
            AvPayloadDeliveryOutcome::Delivered(event) => event,
            other => panic!("unexpected outcome: {other:?}"),
        };
        backing.flush_slots_keep_exported_handle();
        assert_eq!(backing.client_state(), ClientHandleState::ExportedActive);
        assert_eq!(backing.active_slot_count(), 0);
        assert_eq!(
            backing.data_id_state(delivered.data_id),
            AvDataIdState::Stale
        );
    }
}

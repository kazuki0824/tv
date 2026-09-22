#[cfg(test)]
use maleicacid_tuner_hal2_common::HalError;
#[cfg(test)]
use maleicacid_tuner_hal2_control_core::{
    WorkerRuntimeReaperQueue, WorkerRuntimeReaperReservation,
};
#[cfg(test)]
use maleicacid_tuner_hal2_device::FrontendWorkerStopTicket;

#[test]
fn frontend_worker_stop_ticket_is_opaque_single_use_contract() {
    static_assertions::assert_not_impl_any!(FrontendWorkerStopTicket: Clone, Copy);
}

#[test]
fn worker_reaper_reservation_is_opaque_single_use_contract() {
    type Queue = WorkerRuntimeReaperQueue<u32, u32, ()>;
    type Reservation = WorkerRuntimeReaperReservation<u32, u32>;

    static_assertions::assert_not_impl_any!(Reservation: Clone, Copy);

    fn release_by_value(
        queue: &Queue,
        reservation: Reservation,
    ) -> Result<(), HalError> {
        queue.release_reservation(reservation)
    }

    fn transfer_by_value(
        queue: &Queue,
        reservation: Reservation,
    ) -> Result<(), HalError> {
        queue.enqueue_with_reservation((), reservation)
    }

    let _: fn(&Queue, Reservation) -> Result<(), HalError> = release_by_value;
    let _: fn(&Queue, Reservation) -> Result<(), HalError> = transfer_by_value;
}

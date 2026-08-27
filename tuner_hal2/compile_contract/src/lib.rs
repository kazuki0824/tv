#[cfg(test)]
use maleicacid_tuner_hal2_device::FrontendWorkerStopTicket;

#[test]
fn frontend_worker_stop_ticket_is_opaque_single_use_contract() {
    static_assertions::assert_not_impl_any!(FrontendWorkerStopTicket: Clone, Copy);
}

use super::queue_runtime::{FilterDrainBoundary, FilterProducerDrainGate};

#[test]
fn filter_producer_drain_gate_rejects_new_producer_while_draining() {
    let gate = FilterProducerDrainGate::new(4).expect("gate creation must succeed");
    let drain = gate
        .begin_drain(FilterDrainBoundary::Flush)
        .expect("drain must begin with no admitted producers");

    assert!(gate.begin_producer().is_err());

    drain.commit().expect("drain commit must reopen gate");
    let permit = gate
        .begin_producer()
        .expect("producer admission must resume after commit");
    permit.commit().expect("producer permit must commit once");
}

#[test]
fn filter_producer_drain_gate_precommit_drop_restores_open_state() {
    let gate = FilterProducerDrainGate::new(4).expect("gate creation must succeed");
    {
        let _drain = gate
            .begin_drain(FilterDrainBoundary::Reconfigure)
            .expect("drain must begin with no admitted producers");
        assert!(gate.begin_producer().is_err());
    }

    let permit = gate
        .begin_producer()
        .expect("dropping an uncommitted drain must restore the open state");
    permit.commit().expect("producer permit must commit once");
}

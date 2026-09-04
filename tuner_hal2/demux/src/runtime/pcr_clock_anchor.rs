use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::OnceLock;
use std::time::Instant;

const PCR_MODULUS_90KHZ: u64 = 1_u64 << 33;
const PCR_FORWARD_LIMIT_90KHZ: u64 = PCR_MODULUS_90KHZ / 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PcrClockAnchor {
    pub(crate) raw_pcr_base_33: u64,
    unwrapped_pcr_90k: u128,
    monotonic_base_ns: u64,
    generation: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct PcrClockAnchorStore {
    anchors: RefCell<BTreeMap<i32, PcrClockAnchor>>,
}

#[derive(Debug)]
#[must_use = "この準備済み一回限り権限は型付き完了入口で消費する必要があります"]
pub(crate) struct PreparedPcrInvalidation {
    filter_ids: Vec<i32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PcrObservationOutcome {
    Observed,
    Invalidated,
    StaleGeneration,
    ClockUnavailable,
}

impl PcrClockAnchorStore {
    pub(crate) fn observe(
        &self,
        filter_id: i32,
        generation: u64,
        raw_pcr_base_33: u64,
        discontinuity: bool,
    ) -> PcrObservationOutcome {
        if discontinuity {
            self.anchors.borrow_mut().remove(&filter_id);
            return PcrObservationOutcome::Invalidated;
        }
        let Some(monotonic_base_ns) = monotonic_now_ns() else {
            self.anchors.borrow_mut().remove(&filter_id);
            return PcrObservationOutcome::ClockUnavailable;
        };
        let raw_pcr_base_33 = raw_pcr_base_33 & (PCR_MODULUS_90KHZ - 1);
        let mut anchors = self.anchors.borrow_mut();
        let next = match anchors.get(&filter_id).copied() {
            Some(previous) if previous.generation != generation => {
                return PcrObservationOutcome::StaleGeneration;
            }
            Some(previous) => {
                let forward = raw_pcr_base_33.wrapping_sub(previous.raw_pcr_base_33)
                    & (PCR_MODULUS_90KHZ - 1);
                if forward > PCR_FORWARD_LIMIT_90KHZ {
                    anchors.remove(&filter_id);
                    return PcrObservationOutcome::Invalidated;
                }
                let Some(unwrapped_pcr_90k) =
                    previous.unwrapped_pcr_90k.checked_add(u128::from(forward))
                else {
                    anchors.remove(&filter_id);
                    return PcrObservationOutcome::Invalidated;
                };
                PcrClockAnchor {
                    raw_pcr_base_33,
                    unwrapped_pcr_90k,
                    monotonic_base_ns,
                    generation,
                }
            }
            None => PcrClockAnchor {
                raw_pcr_base_33,
                unwrapped_pcr_90k: u128::from(raw_pcr_base_33),
                monotonic_base_ns,
                generation,
            },
        };
        anchors.insert(filter_id, next);
        PcrObservationOutcome::Observed
    }

    pub(crate) fn current_time_90khz(&self, filter_id: i32, generation: u64) -> Option<u64> {
        let now_ns = monotonic_now_ns()?;
        let mut anchors = self.anchors.borrow_mut();
        let anchor = *anchors.get(&filter_id)?;
        if anchor.generation != generation || now_ns < anchor.monotonic_base_ns {
            anchors.remove(&filter_id);
            return None;
        }
        let elapsed_ns = u128::from(now_ns - anchor.monotonic_base_ns);
        let Some(elapsed_ticks) = elapsed_ns
            .checked_mul(90_000)
            .map(|ticks| ticks / 1_000_000_000)
        else {
            anchors.remove(&filter_id);
            return None;
        };
        let Some(current) = anchor.unwrapped_pcr_90k.checked_add(elapsed_ticks) else {
            anchors.remove(&filter_id);
            return None;
        };
        Some((current % u128::from(PCR_MODULUS_90KHZ)) as u64)
    }

    pub(crate) fn prepare_invalidate_filter(&self, filter_id: i32) -> PreparedPcrInvalidation {
        PreparedPcrInvalidation {
            filter_ids: vec![filter_id],
        }
    }

    pub(crate) fn prepare_invalidate_all(&self) -> PreparedPcrInvalidation {
        PreparedPcrInvalidation {
            filter_ids: self.anchors.borrow().keys().copied().collect(),
        }
    }

    pub(crate) fn commit_invalidation(&self, prepared: PreparedPcrInvalidation) {
        let mut anchors = self.anchors.borrow_mut();
        for filter_id in prepared.filter_ids {
            anchors.remove(&filter_id);
        }
    }

    #[cfg(test)]
    pub(crate) fn observation_for_test(&self, filter_id: i32) -> Option<(u64, u64)> {
        self.anchors
            .borrow()
            .get(&filter_id)
            .map(|anchor| (anchor.raw_pcr_base_33, anchor.monotonic_base_ns))
    }
}

fn monotonic_now_ns() -> Option<u64> {
    static MONOTONIC_ORIGIN: OnceLock<Instant> = OnceLock::new();
    let origin = MONOTONIC_ORIGIN.get_or_init(Instant::now);
    u64::try_from(origin.elapsed().as_nanos()).ok()
}

/// The immutable comparison shape selected by a domain owner from committed
/// settings and its queue contract. The variants describe measurements, not
/// public API domains.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WatermarkPolicy {
    OccupancyBand { low: usize, high: usize },
    ReadableWritableBand { low: usize, high: usize },
}

/// One coherent queue observation supplied by the domain owner for a single
/// status evaluation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WatermarkQueueSnapshot {
    pub readable_bytes: usize,
    pub writable_bytes: usize,
}

impl WatermarkQueueSnapshot {
    pub const fn new(readable_bytes: usize, writable_bytes: usize) -> Self {
        Self {
            readable_bytes,
            writable_bytes,
        }
    }
}

/// A classification result whose projection and transition state are owned
/// by the caller.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WatermarkDecision {
    Empty,
    Low,
    High,
    Full,
    NoTransition,
}

/// Pure category-C classifier shared across queue domains. Its policy is fixed
/// for the lifetime of this per-evaluation instance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WatermarkClassifier {
    policy: WatermarkPolicy,
}

impl WatermarkClassifier {
    pub const fn new(policy: WatermarkPolicy) -> Self {
        Self { policy }
    }

    pub const fn classify(&self, snapshot: WatermarkQueueSnapshot) -> WatermarkDecision {
        match self.policy {
            WatermarkPolicy::OccupancyBand { low, high } => {
                if snapshot.readable_bytes > high {
                    WatermarkDecision::High
                } else if snapshot.readable_bytes < low {
                    WatermarkDecision::Low
                } else {
                    WatermarkDecision::NoTransition
                }
            }
            WatermarkPolicy::ReadableWritableBand { low, high } => {
                if snapshot.writable_bytes == 0 {
                    WatermarkDecision::Full
                } else if snapshot.readable_bytes > high {
                    WatermarkDecision::High
                } else if snapshot.readable_bytes < low {
                    WatermarkDecision::Low
                } else if snapshot.readable_bytes == 0 {
                    WatermarkDecision::Empty
                } else {
                    WatermarkDecision::NoTransition
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn occupancy_band_uses_strict_thresholds() {
        let classifier =
            WatermarkClassifier::new(WatermarkPolicy::OccupancyBand { low: 3, high: 8 });

        assert_eq!(
            classifier.classify(WatermarkQueueSnapshot::new(2, 8)),
            WatermarkDecision::Low
        );
        assert_eq!(
            classifier.classify(WatermarkQueueSnapshot::new(3, 7)),
            WatermarkDecision::NoTransition
        );
        assert_eq!(
            classifier.classify(WatermarkQueueSnapshot::new(8, 2)),
            WatermarkDecision::NoTransition
        );
        assert_eq!(
            classifier.classify(WatermarkQueueSnapshot::new(9, 1)),
            WatermarkDecision::High
        );
    }

    #[test]
    fn readable_writable_band_uses_contract_order() {
        let classifier =
            WatermarkClassifier::new(WatermarkPolicy::ReadableWritableBand { low: 3, high: 8 });

        assert_eq!(
            classifier.classify(WatermarkQueueSnapshot::new(9, 0)),
            WatermarkDecision::Full
        );
        assert_eq!(
            classifier.classify(WatermarkQueueSnapshot::new(9, 1)),
            WatermarkDecision::High
        );
        assert_eq!(
            classifier.classify(WatermarkQueueSnapshot::new(2, 8)),
            WatermarkDecision::Low
        );
        assert_eq!(
            classifier.classify(WatermarkQueueSnapshot::new(0, 10)),
            WatermarkDecision::Low
        );

        let zero_low =
            WatermarkClassifier::new(WatermarkPolicy::ReadableWritableBand { low: 0, high: 8 });
        assert_eq!(
            zero_low.classify(WatermarkQueueSnapshot::new(0, 10)),
            WatermarkDecision::Empty
        );
    }

    #[test]
    fn policy_is_bound_to_each_classifier_instance() {
        let occupancy =
            WatermarkClassifier::new(WatermarkPolicy::OccupancyBand { low: 3, high: 8 });
        let readable_writable =
            WatermarkClassifier::new(WatermarkPolicy::ReadableWritableBand { low: 3, high: 8 });
        let snapshot = WatermarkQueueSnapshot::new(9, 0);

        assert_eq!(occupancy.classify(snapshot), WatermarkDecision::High);
        assert_eq!(
            readable_writable.classify(snapshot),
            WatermarkDecision::Full
        );
    }
}

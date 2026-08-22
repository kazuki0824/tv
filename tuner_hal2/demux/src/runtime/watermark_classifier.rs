use super::dvr::{DvrKind, DvrStatusEvent};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilterStatusEvent {
    DataReady,
    LowWater,
    HighWater,
    Overflow,
}

/// Stateless classifier for the AOSP default 25% / 75% Filter FMQ
/// watermarks. Queue occupancy and duplicate suppression remain with their
/// existing owners.
pub struct FilterWatermarkClassifier;

impl FilterWatermarkClassifier {
    pub fn classify(capacity_bytes: usize, readable_bytes: usize) -> Option<FilterStatusEvent> {
        if capacity_bytes == 0 || readable_bytes > capacity_bytes {
            return None;
        }
        let quotient = capacity_bytes / 4;
        let remainder = capacity_bytes % 4;
        let low = quotient + usize::from(remainder != 0);
        let high = quotient
            .checked_mul(3)?
            .checked_add((remainder * 3 + 3) / 4)?;
        if readable_bytes < low {
            Some(FilterStatusEvent::LowWater)
        } else if readable_bytes > high {
            Some(FilterStatusEvent::HighWater)
        } else {
            None
        }
    }
}

/// Stateless classifier for committed DVR settings and one queue snapshot.
/// Pending DATA_READY / OVERFLOW events and status-mask policy are deliberately
/// outside this classifier.
pub struct DvrWatermarkClassifier;

impl DvrWatermarkClassifier {
    pub fn classify(
        kind: DvrKind,
        readable_bytes: usize,
        writable_bytes: usize,
        low_threshold_bytes: usize,
        high_threshold_bytes: usize,
    ) -> Option<DvrStatusEvent> {
        match kind {
            DvrKind::Record if readable_bytes > high_threshold_bytes => {
                Some(DvrStatusEvent::RecordHighWater)
            }
            DvrKind::Record if readable_bytes < low_threshold_bytes => {
                Some(DvrStatusEvent::RecordLowWater)
            }
            DvrKind::Record => None,
            DvrKind::Playback if writable_bytes == 0 => {
                Some(DvrStatusEvent::PlaybackSpaceFull)
            }
            DvrKind::Playback if readable_bytes > high_threshold_bytes => {
                Some(DvrStatusEvent::PlaybackSpaceAlmostFull)
            }
            DvrKind::Playback if readable_bytes < low_threshold_bytes => {
                Some(DvrStatusEvent::PlaybackSpaceAlmostEmpty)
            }
            DvrKind::Playback if readable_bytes == 0 => {
                Some(DvrStatusEvent::PlaybackSpaceEmpty)
            }
            DvrKind::Playback => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_thresholds_are_strict_and_rounded_up() {
        assert_eq!(
            FilterWatermarkClassifier::classify(10, 2),
            Some(FilterStatusEvent::LowWater)
        );
        assert_eq!(FilterWatermarkClassifier::classify(10, 3), None);
        assert_eq!(FilterWatermarkClassifier::classify(10, 8), None);
        assert_eq!(
            FilterWatermarkClassifier::classify(10, 9),
            Some(FilterStatusEvent::HighWater)
        );
    }

    #[test]
    fn playback_full_precedes_other_matching_conditions() {
        assert_eq!(
            DvrWatermarkClassifier::classify(DvrKind::Playback, 8, 0, 10, 4),
            Some(DvrStatusEvent::PlaybackSpaceFull)
        );
    }

    #[test]
    fn filter_non_divisible_capacity_uses_ceiling_thresholds() {
        assert_eq!(
            FilterWatermarkClassifier::classify(5, 1),
            Some(FilterStatusEvent::LowWater)
        );
        assert_eq!(FilterWatermarkClassifier::classify(5, 2), None);
        assert_eq!(FilterWatermarkClassifier::classify(5, 4), None);
        assert_eq!(
            FilterWatermarkClassifier::classify(5, 5),
            Some(FilterStatusEvent::HighWater)
        );
    }

    #[test]
    fn playback_uses_contract_order_and_strict_thresholds() {
        assert_eq!(
            DvrWatermarkClassifier::classify(DvrKind::Playback, 5, 5, 3, 4),
            Some(DvrStatusEvent::PlaybackSpaceAlmostFull)
        );
        assert_eq!(
            DvrWatermarkClassifier::classify(DvrKind::Playback, 2, 8, 3, 8),
            Some(DvrStatusEvent::PlaybackSpaceAlmostEmpty)
        );
        assert_eq!(
            DvrWatermarkClassifier::classify(DvrKind::Playback, 0, 10, 3, 8),
            Some(DvrStatusEvent::PlaybackSpaceAlmostEmpty)
        );
        assert_eq!(
            DvrWatermarkClassifier::classify(DvrKind::Playback, 0, 10, 0, 8),
            Some(DvrStatusEvent::PlaybackSpaceEmpty)
        );
        assert_eq!(
            DvrWatermarkClassifier::classify(DvrKind::Playback, 3, 7, 3, 8),
            None
        );
        assert_eq!(
            DvrWatermarkClassifier::classify(DvrKind::Playback, 8, 2, 3, 8),
            None
        );
    }

    #[test]
    fn record_uses_unconsumed_bytes_and_preserves_equality() {
        assert_eq!(
            DvrWatermarkClassifier::classify(DvrKind::Record, 9, 1, 3, 8),
            Some(DvrStatusEvent::RecordHighWater)
        );
        assert_eq!(
            DvrWatermarkClassifier::classify(DvrKind::Record, 2, 8, 3, 8),
            Some(DvrStatusEvent::RecordLowWater)
        );
        assert_eq!(
            DvrWatermarkClassifier::classify(DvrKind::Record, 5, 0, 5, 5),
            None
        );
    }
}

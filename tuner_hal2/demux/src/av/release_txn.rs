use super::{AvDataId, ClientHandleState};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AvFilterReleaseState {
    OpenAv,
    OpenNonAv,
    Closed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AvDataIdState {
    Active,
    Stale,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AvHandleReleaseInput {
    pub has_fd: bool,
    pub data_id: AvDataId,
    pub client_state: ClientHandleState,
    pub filter_state: AvFilterReleaseState,
    pub shared_handle_exported: bool,
    pub data_id_state: AvDataIdState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AvHandleReleaseOutcome {
    ClientHandleReleased,
    ClientHandleReleaseAfterClose,
    ClientHandleAlreadyReleased,
    SlotReleased { data_id: AvDataId },
    StaleReleaseAccepted { data_id: AvDataId },
    StaleReleaseAfterClose { data_id: AvDataId },
    InvalidDataId,
    InvalidHandleForSlotRelease,
    UnavailableForNonAvFilter,
    InvalidStateWithoutSharedHandle,
    UnknownDataId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AvHandleReleaseTxn;

impl AvHandleReleaseTxn {
    pub fn classify(input: AvHandleReleaseInput) -> AvHandleReleaseOutcome {
        // DESIGN_JA.md 表1-C-AVH priority 1.
        if input.data_id.0 < 0 {
            return AvHandleReleaseOutcome::InvalidDataId;
        }

        // priority 2/3: fd付き shared handle + dataId==0 は client handle release通知。
        if input.has_fd && input.data_id.0 == 0 {
            return match input.filter_state {
                AvFilterReleaseState::Closed => {
                    AvHandleReleaseOutcome::ClientHandleReleaseAfterClose
                }
                _ if input.client_state == ClientHandleState::ClientReleased => {
                    AvHandleReleaseOutcome::ClientHandleAlreadyReleased
                }
                _ => AvHandleReleaseOutcome::ClientHandleReleased,
            };
        }

        // priority 4: fd付き handle は slot release に使わない。
        if input.has_fd && input.data_id.0 > 0 {
            return AvHandleReleaseOutcome::InvalidHandleForSlotRelease;
        }

        // priority 5: close後の遅延releaseは状態を壊さない。
        if input.filter_state == AvFilterReleaseState::Closed {
            return match (input.data_id.0, input.data_id_state) {
                (0, _) => AvHandleReleaseOutcome::ClientHandleReleaseAfterClose,
                (_, AvDataIdState::Stale) => AvHandleReleaseOutcome::StaleReleaseAfterClose {
                    data_id: input.data_id,
                },
                _ => AvHandleReleaseOutcome::UnknownDataId,
            };
        }

        // priority 9: empty handle + dataId==0 は全slot解放ではなくclient release通知。
        if input.data_id.0 == 0 {
            if input.shared_handle_exported
                && input.client_state == ClientHandleState::ClientReleased
            {
                return AvHandleReleaseOutcome::ClientHandleAlreadyReleased;
            }
            return AvHandleReleaseOutcome::ClientHandleReleased;
        }

        // priority 6/7: non-AV filterでの旧AV dataId release。
        if input.filter_state == AvFilterReleaseState::OpenNonAv {
            return if input.shared_handle_exported {
                AvHandleReleaseOutcome::StaleReleaseAccepted {
                    data_id: input.data_id,
                }
            } else {
                AvHandleReleaseOutcome::UnavailableForNonAvFilter
            };
        }

        // priority 8: AV filterだがshared handle未公開。
        if !input.shared_handle_exported {
            return AvHandleReleaseOutcome::InvalidStateWithoutSharedHandle;
        }

        // priority 10/11: active slotだけ解放し、staleは成功扱いの無処理。
        match input.data_id_state {
            AvDataIdState::Active => AvHandleReleaseOutcome::SlotReleased {
                data_id: input.data_id,
            },
            AvDataIdState::Stale => AvHandleReleaseOutcome::StaleReleaseAccepted {
                data_id: input.data_id,
            },
            AvDataIdState::Unknown => AvHandleReleaseOutcome::UnknownDataId,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_av_input(data_id: i64) -> AvHandleReleaseInput {
        AvHandleReleaseInput {
            has_fd: false,
            data_id: AvDataId(data_id),
            client_state: ClientHandleState::ExportedActive,
            filter_state: AvFilterReleaseState::OpenAv,
            shared_handle_exported: true,
            data_id_state: AvDataIdState::Active,
        }
    }

    #[test]
    fn release_priority_rejects_negative_data_id_first() {
        let mut input = open_av_input(-1);
        input.has_fd = true;
        assert_eq!(
            AvHandleReleaseTxn::classify(input),
            AvHandleReleaseOutcome::InvalidDataId
        );
    }

    #[test]
    fn fd_handle_zero_is_client_release_not_slot_release() {
        let mut input = open_av_input(0);
        input.has_fd = true;
        assert_eq!(
            AvHandleReleaseTxn::classify(input),
            AvHandleReleaseOutcome::ClientHandleReleased
        );
    }

    #[test]
    fn fd_handle_positive_data_id_is_invalid_for_slot_release() {
        let mut input = open_av_input(7);
        input.has_fd = true;
        assert_eq!(
            AvHandleReleaseTxn::classify(input),
            AvHandleReleaseOutcome::InvalidHandleForSlotRelease
        );
    }

    #[test]
    fn empty_zero_after_client_release_is_duplicate_not_full_cleanup() {
        let mut input = open_av_input(0);
        input.client_state = ClientHandleState::ClientReleased;
        assert_eq!(
            AvHandleReleaseTxn::classify(input),
            AvHandleReleaseOutcome::ClientHandleAlreadyReleased
        );
    }

    #[test]
    fn active_and_stale_data_id_are_distinguished() {
        let mut input = open_av_input(3);
        input.data_id_state = AvDataIdState::Active;
        assert_eq!(
            AvHandleReleaseTxn::classify(input),
            AvHandleReleaseOutcome::SlotReleased {
                data_id: AvDataId(3)
            }
        );
        input.data_id_state = AvDataIdState::Stale;
        assert_eq!(
            AvHandleReleaseTxn::classify(input),
            AvHandleReleaseOutcome::StaleReleaseAccepted {
                data_id: AvDataId(3)
            }
        );
    }

    #[test]
    fn closed_filter_accepts_only_known_stale_positive_data_id() {
        let mut input = open_av_input(5);
        input.filter_state = AvFilterReleaseState::Closed;
        input.shared_handle_exported = false;
        input.client_state = ClientHandleState::NotExported;
        input.data_id_state = AvDataIdState::Unknown;
        assert_eq!(
            AvHandleReleaseTxn::classify(input),
            AvHandleReleaseOutcome::UnknownDataId
        );
        input.data_id_state = AvDataIdState::Stale;
        assert_eq!(
            AvHandleReleaseTxn::classify(input),
            AvHandleReleaseOutcome::StaleReleaseAfterClose {
                data_id: AvDataId(5)
            }
        );
    }
}

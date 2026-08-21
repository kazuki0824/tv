use super::{AvDataId, ClientHandleState};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AvFilterReleaseState {
    OpenAv,
    OpenNonAv,
    Closed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AvDataIdState {
    ActiveShared,
    ActiveEventLocal,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AvEventLocalHandleLeaseState {
    Active,
    Finalized,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AvHandleReleaseKind {
    Empty,
    Shared,
    EventLocal {
        data_id: AvDataId,
        lease_state: AvEventLocalHandleLeaseState,
    },
    UnknownFile,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AvHandleReleaseInput {
    pub handle_kind: AvHandleReleaseKind,
    pub data_id: AvDataId,
    pub client_state: ClientHandleState,
    pub filter_state: AvFilterReleaseState,
    pub data_id_state: AvDataIdState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AvHandleReleaseOutcome {
    EmptyHandleAccepted,
    ClientHandleReleased,
    ClientHandleReleaseAfterClose,
    ClientHandleAlreadyReleased,
    EventLocalHandleReleased { data_id: AvDataId },
    EventLocalHandleAlreadyReleased { data_id: AvDataId },
    SlotReleased { data_id: AvDataId },
    InvalidDataId,
    InvalidHandleForSlotRelease,
    UnknownDataId,
    RegistryFailure,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AvHandleReleaseTxn;

impl AvHandleReleaseTxn {
    pub fn classify(input: AvHandleReleaseInput) -> AvHandleReleaseOutcome {
        // DESIGN_JA.md 表1-C-AVHの優先順1。
        if input.data_id.0 < 0 {
            return AvHandleReleaseOutcome::InvalidDataId;
        }

        match input.handle_kind {
            // AVH-008/009/010: empty+zeroは無処理。正のtokenは有界の
            // active allocation registryに現存する場合だけ解放する。
            AvHandleReleaseKind::Empty => {
                if input.data_id.0 == 0 {
                    AvHandleReleaseOutcome::EmptyHandleAccepted
                } else {
                    match input.data_id_state {
                        AvDataIdState::ActiveShared | AvDataIdState::ActiveEventLocal => {
                            AvHandleReleaseOutcome::SlotReleased {
                                data_id: input.data_id,
                            }
                        }
                        AvDataIdState::Unknown => AvHandleReleaseOutcome::UnknownDataId,
                    }
                }
            }

            // AVH-005/006。shared handleの一致は、この純粋transactionを
            // 評価する前にbacking側で確定する。
            AvHandleReleaseKind::Shared => {
                if input.data_id.0 > 0 {
                    return AvHandleReleaseOutcome::InvalidHandleForSlotRelease;
                }
                match input.client_state {
                    ClientHandleState::ExportedActive => {
                        if input.filter_state == AvFilterReleaseState::Closed {
                            AvHandleReleaseOutcome::ClientHandleReleaseAfterClose
                        } else {
                            AvHandleReleaseOutcome::ClientHandleReleased
                        }
                    }
                    ClientHandleState::ClientReleased => {
                        AvHandleReleaseOutcome::ClientHandleAlreadyReleased
                    }
                    ClientHandleState::NotExported => {
                        AvHandleReleaseOutcome::InvalidHandleForSlotRelease
                    }
                }
            }

            // AVH-012〜016。file handleが解放できるのは、backing側でactive
            // tokenとhandle identityの両方が一致したevent-local allocationだけとする。
            AvHandleReleaseKind::EventLocal {
                data_id,
                lease_state,
            } => {
                if input.data_id.0 == 0 {
                    return match lease_state {
                        AvEventLocalHandleLeaseState::Active => {
                            AvHandleReleaseOutcome::EventLocalHandleReleased { data_id }
                        }
                        AvEventLocalHandleLeaseState::Finalized => {
                            AvHandleReleaseOutcome::EventLocalHandleAlreadyReleased { data_id }
                        }
                    };
                }
                if input.data_id == data_id
                    && input.data_id_state == AvDataIdState::ActiveEventLocal
                {
                    AvHandleReleaseOutcome::SlotReleased {
                        data_id: input.data_id,
                    }
                } else {
                    AvHandleReleaseOutcome::InvalidHandleForSlotRelease
                }
            }

            // AVH-007/015/016。不明または外部のfileでleaseやallocationを変更しない。
            AvHandleReleaseKind::UnknownFile => {
                AvHandleReleaseOutcome::InvalidHandleForSlotRelease
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(handle_kind: AvHandleReleaseKind, data_id: i64) -> AvHandleReleaseInput {
        AvHandleReleaseInput {
            handle_kind,
            data_id: AvDataId(data_id),
            client_state: ClientHandleState::ExportedActive,
            filter_state: AvFilterReleaseState::OpenAv,
            data_id_state: AvDataIdState::ActiveShared,
        }
    }

    #[test]
    fn negative_data_id_is_rejected_before_handle_classification() {
        assert_eq!(
            AvHandleReleaseTxn::classify(input(AvHandleReleaseKind::UnknownFile, -1)),
            AvHandleReleaseOutcome::InvalidDataId
        );
    }

    #[test]
    fn empty_zero_is_a_noop_and_inactive_positive_is_invalid() {
        assert_eq!(
            AvHandleReleaseTxn::classify(input(AvHandleReleaseKind::Empty, 0)),
            AvHandleReleaseOutcome::EmptyHandleAccepted
        );
        let mut inactive = input(AvHandleReleaseKind::Empty, 7);
        inactive.data_id_state = AvDataIdState::Unknown;
        assert_eq!(
            AvHandleReleaseTxn::classify(inactive),
            AvHandleReleaseOutcome::UnknownDataId
        );
    }

    #[test]
    fn event_local_handle_requires_the_matching_active_token() {
        let handle_kind = AvHandleReleaseKind::EventLocal {
            data_id: AvDataId(9),
            lease_state: AvEventLocalHandleLeaseState::Active,
        };
        let mut matching = input(handle_kind, 9);
        matching.data_id_state = AvDataIdState::ActiveEventLocal;
        assert_eq!(
            AvHandleReleaseTxn::classify(matching),
            AvHandleReleaseOutcome::SlotReleased {
                data_id: AvDataId(9)
            }
        );
        matching.data_id = AvDataId(10);
        assert_eq!(
            AvHandleReleaseTxn::classify(matching),
            AvHandleReleaseOutcome::InvalidHandleForSlotRelease
        );
    }

    #[test]
    fn event_local_zero_finalizes_only_its_bounded_handle_lease() {
        let active = input(
            AvHandleReleaseKind::EventLocal {
                data_id: AvDataId(11),
                lease_state: AvEventLocalHandleLeaseState::Active,
            },
            0,
        );
        assert_eq!(
            AvHandleReleaseTxn::classify(active),
            AvHandleReleaseOutcome::EventLocalHandleReleased {
                data_id: AvDataId(11)
            }
        );
        let finalized = AvHandleReleaseInput {
            handle_kind: AvHandleReleaseKind::EventLocal {
                data_id: AvDataId(11),
                lease_state: AvEventLocalHandleLeaseState::Finalized,
            },
            ..active
        };
        assert_eq!(
            AvHandleReleaseTxn::classify(finalized),
            AvHandleReleaseOutcome::EventLocalHandleAlreadyReleased {
                data_id: AvDataId(11)
            }
        );
    }
}

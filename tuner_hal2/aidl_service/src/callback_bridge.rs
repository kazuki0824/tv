use maleicacid_tuner_hal2_binder_adapter::{AidlApi, AidlObjectKind};
use maleicacid_tuner_hal2_common::HalError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallbackOwnerKind {
    Frontend,
    Filter,
    Dvr,
    Lnb,
}

impl CallbackOwnerKind {
    pub const fn object_kind(self) -> AidlObjectKind {
        match self {
            Self::Frontend => AidlObjectKind::Frontend,
            Self::Filter => AidlObjectKind::Filter,
            Self::Dvr => AidlObjectKind::Dvr,
            Self::Lnb => AidlObjectKind::Lnb,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallbackApi {
    SetCallback,
    FrontendEvent,
    FrontendScanEvent,
    FilterStatus,
    FilterEvent,
    DvrStatus,
    LnbEvent,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallbackFailureRecord {
    pub owner: CallbackOwnerKind,
    pub api: CallbackApi,
    pub error: HalError,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CallbackBridge {
    last_failure: Option<CallbackFailureRecord>,
}

impl CallbackBridge {
    pub const fn new() -> Self {
        Self { last_failure: None }
    }

    pub fn record_failure(&mut self, owner: CallbackOwnerKind, api: CallbackApi, error: HalError) {
        self.last_failure = Some(CallbackFailureRecord { owner, api, error });
    }

    pub fn last_failure(&self) -> Option<&CallbackFailureRecord> {
        self.last_failure.as_ref()
    }

    pub const fn api_owner_for_set_callback(api: AidlApi) -> Option<CallbackOwnerKind> {
        match api {
            AidlApi::FrontendSetCallback => Some(CallbackOwnerKind::Frontend),
            AidlApi::DemuxOpenFilter => Some(CallbackOwnerKind::Filter),
            AidlApi::DemuxOpenDvr => Some(CallbackOwnerKind::Dvr),
            AidlApi::LnbSetCallback => Some(CallbackOwnerKind::Lnb),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use maleicacid_tuner_hal2_common::HalInternalKind;

    #[test]
    fn callback_failure_is_typed() {
        let mut bridge = CallbackBridge::new();
        bridge.record_failure(
            CallbackOwnerKind::Filter,
            CallbackApi::FilterStatus,
            HalError::internal(HalInternalKind::InvariantViolation, "callback"),
        );
        let record = bridge.last_failure().unwrap();
        assert_eq!(record.owner, CallbackOwnerKind::Filter);
        assert_eq!(record.api, CallbackApi::FilterStatus);
    }
}

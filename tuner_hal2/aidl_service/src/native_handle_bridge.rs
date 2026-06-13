use maleicacid_tuner_hal2_common::{HalError, HalInternalKind, HalInvalidArgumentKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeHandleBridgeKind {
    FmqDescriptor,
    AvSharedHandle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeHandleBridgeErrorKind {
    MissingHandle,
    UnexpectedFdCount,
    UnexpectedIntCount,
    InvalidDataId,
    UnsupportedHandleShape,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeHandleBridgeError {
    pub kind: NativeHandleBridgeErrorKind,
    pub object: NativeHandleBridgeKind,
}

impl NativeHandleBridgeError {
    pub const fn new(object: NativeHandleBridgeKind, kind: NativeHandleBridgeErrorKind) -> Self {
        Self { object, kind }
    }

    pub fn into_hal_error(self) -> HalError {
        match self.kind {
            NativeHandleBridgeErrorKind::InvalidDataId
            | NativeHandleBridgeErrorKind::UnsupportedHandleShape => HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "native handle shape",
            ),
            NativeHandleBridgeErrorKind::MissingHandle
            | NativeHandleBridgeErrorKind::UnexpectedFdCount
            | NativeHandleBridgeErrorKind::UnexpectedIntCount => {
                HalError::internal(HalInternalKind::InvariantViolation, "native handle bridge")
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeHandleBridge {
    bridge_kind: NativeHandleBridgeKind,
}

impl NativeHandleBridge {
    pub const fn new(bridge_kind: NativeHandleBridgeKind) -> Self {
        Self { bridge_kind }
    }
    pub const fn bridge_kind(&self) -> NativeHandleBridgeKind {
        self.bridge_kind
    }

    pub fn validate_av_data_id(&self, data_id: i64) -> Result<(), NativeHandleBridgeError> {
        if data_id < 0 {
            Err(NativeHandleBridgeError::new(
                self.bridge_kind,
                NativeHandleBridgeErrorKind::InvalidDataId,
            ))
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negative_av_data_id_is_typed_error() {
        let bridge = NativeHandleBridge::new(NativeHandleBridgeKind::AvSharedHandle);
        let err = bridge.validate_av_data_id(-1).unwrap_err();
        assert_eq!(err.kind, NativeHandleBridgeErrorKind::InvalidDataId);
        assert_eq!(err.object, NativeHandleBridgeKind::AvSharedHandle);
    }
}

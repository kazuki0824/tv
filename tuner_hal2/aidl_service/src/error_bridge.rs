use std::ffi::CString;

use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::Result::Result as TunerResult;
use binder::Status;
use maleicacid_tuner_hal2_binder_adapter::{AidlStatusMapper, TunerStatusCode};
use maleicacid_tuner_hal2_common::HalError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AidlErrorMapping {
    pub status: TunerStatusCode,
    pub source_error: HalError,
}

pub struct AidlErrorBridge {
    mapper: AidlStatusMapper,
}

impl AidlErrorBridge {
    pub const fn new(aidl_version: u32) -> Self {
        Self {
            mapper: AidlStatusMapper::new(aidl_version),
        }
    }

    pub fn map_domain_error(&self, error: HalError) -> AidlErrorMapping {
        let status = AidlStatusMapper::map_error(&error);
        AidlErrorMapping {
            status,
            source_error: error,
        }
    }

    pub fn map_domain_error_ref(&self, error: &HalError) -> TunerStatusCode {
        AidlStatusMapper::map_error(error)
    }

    pub const fn aidl_version(&self) -> u32 {
        self.mapper.aidl_version()
    }
}

pub(crate) fn status_unknown_error(message: &str) -> Status {
    service_error(TunerResult::UNKNOWN_ERROR.0, message)
}

pub(crate) fn status_invalid_state(message: &str) -> Status {
    service_error(TunerResult::INVALID_STATE.0, message)
}

pub(crate) fn status_from_hal_error(error: HalError) -> Status {
    let status = AidlStatusMapper::map_error(&error);
    status_from_tuner_status(status, &error.to_string())
}

pub(crate) fn status_from_hal_error_ref(error: &HalError) -> Status {
    let status = AidlStatusMapper::map_error(error);
    status_from_tuner_status(status, &error.to_string())
}

pub(crate) fn status_from_tuner_status(status: TunerStatusCode, message: &str) -> Status {
    match status {
        TunerStatusCode::Ok => service_error(
            TunerResult::UNKNOWN_ERROR.0,
            "attempted to convert OK into an error status",
        ),
        TunerStatusCode::InvalidArgument => service_error(TunerResult::INVALID_ARGUMENT.0, message),
        TunerStatusCode::InvalidState => service_error(TunerResult::INVALID_STATE.0, message),
        TunerStatusCode::Unavailable => service_error(TunerResult::UNAVAILABLE.0, message),
        TunerStatusCode::UnknownError => service_error(TunerResult::UNKNOWN_ERROR.0, message),
    }
}

pub(crate) fn service_error(code: i32, message: &str) -> Status {
    match CString::new(message) {
        Ok(detail) => Status::new_service_specific_error(code, Some(detail.as_c_str())),
        Err(_) => Status::new_service_specific_error(code, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use maleicacid_tuner_hal2_common::HalInvalidArgumentKind;

    #[test]
    fn maps_by_error_kind() {
        let bridge = AidlErrorBridge::new(2);
        let error = HalError::invalid_argument(HalInvalidArgumentKind::NumericRange, "値域外");
        let mapping = bridge.map_domain_error(error);
        assert_eq!(mapping.status, TunerStatusCode::InvalidArgument);
        assert_eq!(bridge.aidl_version(), 2);
    }

}

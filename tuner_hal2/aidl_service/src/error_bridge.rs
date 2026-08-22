use std::ffi::CString;

use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::Result::Result as TunerResult;
use binder::Status;
use maleicacid_tuner_hal2_binder_adapter::{AidlStatusMapper, TunerStatusCode};
use maleicacid_tuner_hal2_common::HalError;

pub(crate) fn status_unknown_error(message: &str) -> Status {
    service_error(TunerResult::UNKNOWN_ERROR.0, message)
}

pub(crate) fn status_from_hal_error(error: HalError) -> Status {
    let status = AidlStatusMapper::map_error(&error);
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
        TunerStatusCode::OutOfMemory => service_error(TunerResult::OUT_OF_MEMORY.0, message),
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
        let error = HalError::invalid_argument(HalInvalidArgumentKind::NumericRange, "値域外");
        let status = AidlStatusMapper::map_error(&error);
        assert_eq!(status, TunerStatusCode::InvalidArgument);
    }
}

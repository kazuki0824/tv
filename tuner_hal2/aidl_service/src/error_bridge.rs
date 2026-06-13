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
    pub const fn new(aidl_version: u32) -> Self { Self { mapper: AidlStatusMapper::new(aidl_version) } }

    pub fn map_domain_error(&self, error: HalError) -> AidlErrorMapping {
        let status = AidlStatusMapper::map_error(&error);
        AidlErrorMapping { status, source_error: error }
    }

    pub fn map_domain_error_ref(&self, error: &HalError) -> TunerStatusCode {
        AidlStatusMapper::map_error(error)
    }

    pub const fn aidl_version(&self) -> u32 { self.mapper.aidl_version() }
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

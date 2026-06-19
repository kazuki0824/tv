use std::path::PathBuf;

use maleicacid_tuner_hal2_common::{HalError, HalErrorDetail};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeFailureDomain {
    DeviceMissing,
    DeviceOpen,
    DevicePermission,
    DeviceBusy,
    RuntimeIoctl,
    RuntimeReadWrite,
    Callback,
    Fmq,
    EventFlag,
    Cleanup,
    ClientArgument,
    ObjectState,
    UnsupportedByDesign,
    InternalInvariant,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScanCandidateOwner {
    TisExplicitCandidate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransportCapability {
    Ts,
    Mmtp,
    Tlv,
    Alp,
    IpCid,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProfileFeature {
    MonitorEvent,
    AvPassthrough,
    LinkCaps,
    AvSharedHandleRelease,
    DescramblerObject,
    LnbObject,
    FrontendStatusCaps,
}

pub const fn transport_declared(capability: TransportCapability) -> bool {
    matches!(capability, TransportCapability::Ts)
}

pub const fn feature_declared(feature: ProfileFeature) -> bool {
    match feature {
        ProfileFeature::MonitorEvent
        | ProfileFeature::AvPassthrough
        | ProfileFeature::LinkCaps
        | ProfileFeature::AvSharedHandleRelease
        | ProfileFeature::DescramblerObject
        | ProfileFeature::LnbObject
        | ProfileFeature::FrontendStatusCaps => false,
    }
}

pub const fn hal_generates_japanese_scan_plan() -> bool {
    false
}

pub const fn scan_candidate_owner() -> ScanCandidateOwner {
    ScanCandidateOwner::TisExplicitCandidate
}

pub fn configure_ip_cid_result(_ip_cid: i32) -> Result<(), HalError> {
    Err(HalError::Unsupported(
        "IP CID is outside the product TS-only capability/profile",
    ))
}

pub fn configure_monitor_event_result(monitor_event_types: i32) -> Result<(), HalError> {
    if monitor_event_types == 0 {
        Ok(())
    } else {
        Err(HalError::Unsupported(
            "monitor event is not declared by this profile",
        ))
    }
}

pub fn unsupported_by_design(_api_name: &'static str) -> HalError {
    HalError::Unsupported("API is unsupported by product profile")
}

pub fn failure_domain(error: &HalError) -> RuntimeFailureDomain {
    match error {
        HalError::ComposedFailure { primary, .. } => failure_domain(primary),
        HalError::DeviceMissing(_) => RuntimeFailureDomain::DeviceMissing,
        HalError::OpenFailed { .. } => RuntimeFailureDomain::DeviceOpen,
        HalError::PermissionDenied { .. } => RuntimeFailureDomain::DevicePermission,
        HalError::Busy { .. } => RuntimeFailureDomain::DeviceBusy,
        HalError::IoctlFailed { .. } => RuntimeFailureDomain::RuntimeIoctl,
        HalError::Io { .. } => RuntimeFailureDomain::RuntimeReadWrite,
        HalError::CallbackFailed { .. } => RuntimeFailureDomain::Callback,
        HalError::FmqFailed { .. } => RuntimeFailureDomain::Fmq,
        HalError::EventFlagFailed { .. } => RuntimeFailureDomain::EventFlag,
        HalError::CleanupFailed { .. } => RuntimeFailureDomain::Cleanup,
        HalError::InvalidArgument { .. } => RuntimeFailureDomain::ClientArgument,
        HalError::InvalidState { .. } => RuntimeFailureDomain::ObjectState,
        HalError::Unsupported(_) => RuntimeFailureDomain::UnsupportedByDesign,
        HalError::Internal { .. } => RuntimeFailureDomain::InternalInvariant,
    }
}

pub fn open_failed(path: impl Into<PathBuf>, detail: impl Into<String>) -> HalError {
    HalError::OpenFailed {
        path: path.into(),
        detail: HalErrorDetail::new(detail),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use maleicacid_tuner_hal2_common::{
        HalInternalKind, HalInvalidArgumentKind, HalInvalidStateKind,
    };

    #[test]
    fn ts_only_profile_does_not_declare_other_stream_inputs() {
        assert!(transport_declared(TransportCapability::Ts));
        assert!(!transport_declared(TransportCapability::Mmtp));
        assert!(!transport_declared(TransportCapability::Tlv));
        assert!(!transport_declared(TransportCapability::Alp));
        assert!(!transport_declared(TransportCapability::IpCid));
    }

    #[test]
    fn monitor_event_zero_is_noop_nonzero_is_unavailable_boundary() {
        assert!(configure_monitor_event_result(0).is_ok());
        assert!(matches!(
            configure_monitor_event_result(1),
            Err(HalError::Unsupported(_))
        ));
    }

    #[test]
    fn ip_cid_is_never_accepted_as_save_only_success() {
        assert!(matches!(
            configure_ip_cid_result(0),
            Err(HalError::Unsupported(_))
        ));
        assert!(matches!(
            configure_ip_cid_result(-1),
            Err(HalError::Unsupported(_))
        ));
    }

    #[test]
    fn failure_domains_are_not_collapsed_to_internal() {
        let missing = HalError::DeviceMissing(PathBuf::from("/dev/missing"));
        let open = open_failed("/dev/dvb/adapter0/frontend0", "open failed");
        let ioctl = HalError::IoctlFailed {
            backend: "dvb",
            path: None,
            op: "FE_SET_PROPERTY",
            errno: 5,
        };
        let callback = HalError::callback_failed("frontend callback", "binder failure");
        let fmq = HalError::fmq_failed("write", "native write failed");
        let event = HalError::event_flag_failed("wake", "native wake failed");
        let cleanup = HalError::cleanup_failed("filter", "queue clear failed");
        let invalid_arg = HalError::invalid_argument(HalInvalidArgumentKind::NumericRange, "range");
        let invalid_state =
            HalError::invalid_state(HalInvalidStateKind::InvalidLifecycle, "closed");
        let unsupported = HalError::Unsupported("unsupported");
        let internal = HalError::internal(HalInternalKind::InvariantViolation, "broken");

        assert_eq!(
            failure_domain(&missing),
            RuntimeFailureDomain::DeviceMissing
        );
        assert_eq!(failure_domain(&open), RuntimeFailureDomain::DeviceOpen);
        assert_eq!(failure_domain(&ioctl), RuntimeFailureDomain::RuntimeIoctl);
        assert_eq!(failure_domain(&callback), RuntimeFailureDomain::Callback);
        assert_eq!(failure_domain(&fmq), RuntimeFailureDomain::Fmq);
        assert_eq!(failure_domain(&event), RuntimeFailureDomain::EventFlag);
        assert_eq!(failure_domain(&cleanup), RuntimeFailureDomain::Cleanup);
        assert_eq!(
            failure_domain(&invalid_arg),
            RuntimeFailureDomain::ClientArgument
        );
        assert_eq!(
            failure_domain(&invalid_state),
            RuntimeFailureDomain::ObjectState
        );
        assert_eq!(
            failure_domain(&unsupported),
            RuntimeFailureDomain::UnsupportedByDesign
        );
        assert_eq!(
            failure_domain(&internal),
            RuntimeFailureDomain::InternalInvariant
        );
    }

    #[test]
    fn tuner_hal_does_not_own_japanese_scan_plan_generation() {
        assert!(!hal_generates_japanese_scan_plan());
        assert_eq!(
            scan_candidate_owner(),
            ScanCandidateOwner::TisExplicitCandidate
        );
    }
}

use maleicacid_tuner_hal2_common::HalError;
use maleicacid_tuner_hal2_demux::PacketPid;
use maleicacid_tuner_hal2_descrambler::{DescramblerPid, DescramblerPidClaim};
use maleicacid_tuner_hal2_domain_request::{AidlObjectGeneration, AidlObjectId};

use maleicacid_tuner_hal2_service_runtime::{
    CallbackDeliveryFailurePhase, CallbackDeliveryFailureReport, DvrPostCommitNotificationPhase,
};

fn test_descrambler_pid(pid: u16) -> DescramblerPid {
    DescramblerPidClaim::from_demux_input(pid)
        .expect("test PID must be valid")
        .pid()
}

fn test_packet_pid(pid: u16) -> PacketPid {
    PacketPid::from_descrambler_pid_for_service_runtime_boundary(test_descrambler_pid(pid))
}

#[test]
fn callback_artifact_lookup_is_not_binder_delivery_phase() {
    assert_ne!(
        CallbackDeliveryFailurePhase::CallbackArtifactLookup,
        CallbackDeliveryFailurePhase::BinderDelivery,
    );
}

#[test]
fn dvr_callback_artifact_lookup_report_keeps_post_commit_context() {
    let report = CallbackDeliveryFailureReport::dvr(
        AidlObjectId(710_001),
        AidlObjectGeneration(3),
        CallbackDeliveryFailurePhase::CallbackArtifactLookup,
        DvrPostCommitNotificationPhase::InitialStatusDelivery,
        HalError::callback_failed("IDvrCallback.lookup", "artifact missing"),
    );
    assert_eq!(
        report.phase(),
        CallbackDeliveryFailurePhase::CallbackArtifactLookup,
    );
    assert!(matches!(
        report,
        CallbackDeliveryFailureReport::Dvr {
            dvr_post_commit_phase: DvrPostCommitNotificationPhase::InitialStatusDelivery,
            ..
        }
    ));
}

#[test]
fn descrambler_pid_claim_missing_demux_uses_dedicated_variant() {
    use maleicacid_tuner_hal2_service_runtime::{
        DescramblerDiagnosticPhase, DescramblerDiagnosticRecord,
    };

    let record = DescramblerDiagnosticRecord::pid_claim_without_demux(
        DescramblerDiagnosticPhase::AddPid,
        12,
        test_descrambler_pid(0x123),
        -1,
        HalError::callback_failed("test", "pid claim rejected before demux resolution"),
    );

    assert!(matches!(
        record,
        DescramblerDiagnosticRecord::PidClaimRejectedWithoutDemux {
            phase: DescramblerDiagnosticPhase::AddPid,
            descrambler_id: 12,
            pid,
            filter_id: -1,
            ..
        } if pid == test_descrambler_pid(0x123)
    ));
}

#[test]
fn descrambler_pid_claim_with_demux_uses_required_demux_variant() {
    use maleicacid_tuner_hal2_service_runtime::{
        DescramblerDiagnosticPhase, DescramblerDiagnosticRecord,
    };

    let record = DescramblerDiagnosticRecord::pid_claim_with_demux(
        DescramblerDiagnosticPhase::RemovePid,
        12,
        34,
        test_descrambler_pid(0x124),
        56,
        HalError::callback_failed("test", "pid claim rejected after demux resolution"),
    );

    assert!(matches!(
        record,
        DescramblerDiagnosticRecord::PidClaimRejected {
            phase: DescramblerDiagnosticPhase::RemovePid,
            descrambler_id: 12,
            demux_id: 34,
            pid,
            filter_id: 56,
            ..
        } if pid == test_descrambler_pid(0x124)
    ));
}

#[test]
fn descrambler_packet_policy_diagnostic_uses_typed_packet_pid() {
    use maleicacid_tuner_hal2_service_runtime::{
        DescramblerDiagnosticKind, DescramblerDiagnosticRecord,
    };

    let pid = test_packet_pid(0x125);
    let record = DescramblerDiagnosticRecord::packet_policy(
        8,
        pid,
        DescramblerDiagnosticKind::PacketDescrambled,
    );

    assert!(matches!(
        record,
        DescramblerDiagnosticRecord::PacketPolicy {
            demux_id: 8,
            pid: matched_pid,
            kind: DescramblerDiagnosticKind::PacketDescrambled,
        } if matched_pid == pid
    ));
}

#[test]
fn descrambler_packet_validation_without_pid_uses_dedicated_variant() {
    use maleicacid_tuner_hal2_service_runtime::{
        DescramblerDiagnosticKind, DescramblerDiagnosticRecord,
    };

    let record = DescramblerDiagnosticRecord::packet_policy_without_pid(
        9,
        DescramblerDiagnosticKind::InvalidPacketSize,
    );

    assert!(matches!(
        record,
        DescramblerDiagnosticRecord::PacketPolicyWithoutPid {
            demux_id: 9,
            kind: DescramblerDiagnosticKind::InvalidPacketSize,
        }
    ));
}

#[test]
fn service_boot_reset_split_outcomes_are_variant_specific_records() {
    use maleicacid_tuner_hal2_service_runtime::CallbackArtifactRuntimeSplitOutcome;

    let records = CallbackArtifactRuntimeSplitOutcome::service_boot_reset_from_attempt_results(
        Ok(()),
        Err(HalError::internal(
            maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
            "callback artifact reset failed",
        )),
        Err(HalError::internal(
            maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
            "drop leak reset failed",
        )),
        Ok(()),
        Ok(()),
        Ok(()),
    );

    assert_eq!(records.len(), 2);
    assert!(matches!(
        &records[0],
        CallbackArtifactRuntimeSplitOutcome::ServiceBootCallbackArtifactFailure { .. }
    ));
    assert!(matches!(
        &records[1],
        CallbackArtifactRuntimeSplitOutcome::ServiceBootDropLeakFailure { .. }
    ));
}

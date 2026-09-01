mod boot;
mod callback_registry;
mod capability_profile;
mod capability_snapshot;
mod cleanup_execution;
mod command_dispatch;
mod descrambler_key_table;
mod descrambler_ops;
mod descrambler_session;
mod diagnostics;
mod dispatch;
mod error_mapping;
mod frontend_ops;
mod frontend_request_txn;
mod frontend_worker_termination_use_case;
mod frontend_worker_txn;
mod lnb_backend_adapter;
mod lnb_control_txn;
mod lnb_ops;
mod method_dispatch;
mod method_validation;
mod object_close_txn;
mod object_domain_cleanup;
mod object_lifecycle;
mod object_method_use_case;
mod object_table;
mod open_rollback;
mod playback_consume_txn;
mod post_commit_callback_failure_txn;
mod queue_cleanup_use_case;
mod registry;
mod root_method_txn;
mod root_object_ops;
mod transaction_registry;
mod worker_failure_classifier;
mod worker_runtime;

pub use boot::{
    start_frontend_demux_live_pump_from_reader, CallbackArtifactCleanupResult,
    CallbackArtifactResetCommand, CallbackDeliveryFailurePhase, CallbackDeliveryFailureReport,
    CallbackRegistrationArtifactOutcome, ChildOpenTxn, DvrChildRuntimeOpen, DvrStatusPollSnapshot,
    FilterChildRuntimeOpen, FilterEventDelivery, FilterEventDeliverySnapshot,
    FilterEventDispatcher, FrontendDemuxPacketSink, FrontendProbeOutcome,
    OwnerCallbackCleanupArtifactCommand, OwnerCallbackCleanupUseCaseOutcome, ServiceBootOutcome,
    TunerServiceRuntime,
};
pub use capability_profile::{
    configure_ip_cid_result, configure_monitor_event_result, failure_domain,
    hal_generates_japanese_scan_plan, open_failed, scan_candidate_owner, transport_declared,
    RuntimeFailureDomain, ScanCandidateOwner, TransportCapability,
};
pub use capability_snapshot::{CapabilitySnapshot, PublicDemuxCapability};
pub use cleanup_execution::{
    CleanupExecutionDiagnosticSnapshot, CleanupExecutionReport, CleanupExecutionStepOutcome,
    SharedCleanupDiagnostics,
};
pub use command_dispatch::{
    RuntimeCommandDispatchError, RuntimeCommandDispatchPlan, RuntimeCommandDispatcher,
};
pub use diagnostics::{
    BoundedDiagnosticStore, CallbackArtifactRuntimeSplitDiagnosticRecord,
    CallbackArtifactRuntimeSplitDiagnosticSnapshot, CallbackArtifactRuntimeSplitOutcome,
    CallbackArtifactRuntimeSplitPhase, CallbackArtifactRuntimeSplitTarget,
    CapabilitySuppressionReason, ChildOpenRollbackDiagnosticRecord,
    ChildOpenRollbackDiagnosticSnapshot, ChildOpenRollbackKind, ChildOpenRollbackOutcome,
    ChildOpenRollbackPhase, DemuxTransactionDiagnosticId, DemuxTransactionDiagnosticKind,
    DemuxTransactionDiagnosticRecord, DemuxTransactionDiagnosticSnapshot,
    DescramblerDiagnosticKind, DescramblerDiagnosticPhase, DescramblerDiagnosticRecord,
    DescramblerDiagnosticSnapshot, DiagnosticSnapshot, DvrPostCommitNotificationDiagnosticRecord,
    DvrPostCommitNotificationDiagnosticSnapshot, DvrPostCommitNotificationFailureKind,
    DvrPostCommitNotificationPhase, DvrStatusNotifierCleanupDiagnosticRecord,
    DvrStatusNotifierCleanupDiagnosticSnapshot, FilterCallbackDeliveryDiagnosticPhase,
    FilterCallbackDeliveryDiagnosticRecord, FilterCallbackDeliveryDiagnosticSnapshot,
    FrontendCallbackDeliveryDiagnosticPhase, FrontendCallbackDeliveryDiagnosticRecord,
    FrontendCallbackDeliveryDiagnosticSnapshot, QueueDescriptorQueryDiagnosticRecord,
    QueueDescriptorQueryDiagnosticSnapshot, SharedCallbackArtifactRuntimeSplitDiagnostics,
    SharedDvrPostCommitNotificationDiagnostics, SharedDvrStatusNotifierCleanupDiagnostics,
    StartupDiagnosticKind, StartupDiagnosticPhase, StartupDiagnosticRecord,
    StartupDiagnosticSnapshot,
};
pub use dispatch::{dispatch_target_for, ServiceRuntimeDispatchTarget};
pub use frontend_ops::{
    set_frontend_lnb_object_use_case, FrontendOperationEvent, FrontendOperationEventAcceptance,
    FrontendTuneScanTxn, FrontendWorkerTerminalEvent, FrontendWorkerTerminalEventAcceptance,
    SharedFrontendRuntime,
};
pub use frontend_worker_termination_use_case::FrontendWorkerTerminationUseCase;
pub use frontend_worker_txn::{
    FrontendCloseCleanupReport, FrontendScanNotification, FrontendScanNotifier,
    FrontendTuneNotification, FrontendTuneNotifier, FrontendWorkerCleanupDiagnosticKind,
    FrontendWorkerCleanupDiagnosticRecord, FrontendWorkerCleanupDiagnosticSnapshot,
    FrontendWorkerCleanupExecutionReport, FrontendWorkerCleanupStep,
    FrontendWorkerCleanupStepOutcome, FrontendWorkerCleanupTarget,
    FrontendWorkerCleanupWorkerGeneration, SharedFrontendWorkerCleanupDiagnostics,
};
pub use lnb_ops::{
    apply_lnb_satellite_position_object_use_case, apply_lnb_tone_object_use_case,
    apply_lnb_voltage_object_use_case, close_lnb_after_root_open_rollback_use_case,
    close_lnb_explicit_after_object_close_begin_use_case, send_lnb_diseqc_object_use_case,
    SharedLnbRuntime,
};
pub use object_close_txn::{
    close_object_use_case, finish_object_close_use_case, quarantine_object_drop_leak_use_case,
    CloseCleanupAttemptCompletion, CloseCleanupAuthority, ObjectArtifactCleanupCommand,
    ObjectArtifactCleanupExecutor, ObjectArtifactCleanupKind, ObjectCleanupDiagnosticKind,
    ObjectCleanupDiagnosticRecord, ObjectCleanupDiagnosticSnapshot, ObjectCleanupExecutionKind,
    ObjectCleanupExecutionReport, ObjectCleanupObjectTarget, ObjectCleanupStepOutcome,
    ObjectCloseCleanupAttempt, ObjectCloseCleanupFailure, ObjectCloseRuntimeExecutor,
    ObjectCloseTxn, ObjectCloseUseCasePlan, ObjectRuntimeCleanupCommand, ObjectRuntimeCleanupKind,
    SharedObjectCleanupDiagnostics,
};
pub use object_domain_cleanup::{
    ObjectDomainCleanupCommand, ObjectDomainCleanupExecutor, ObjectDomainCleanupKind,
    ObjectDomainCleanupOutcome,
};
pub use object_lifecycle::{aidl_object_cleanup_dependency, aidl_object_cleanup_is_terminal};
pub use object_method_use_case::{
    lnb_profile_supports_voltage_status, ObjectFrontendStatusReadinessValue,
    ObjectFrontendStatusType, ObjectFrontendStatusValue, ObjectMethodExecutionToken,
    ObjectMethodUseCase, ObjectMethodUseCaseBuildError, ObjectQueryRequest, ObjectQueryResponse,
};
pub(crate) use object_table::RuntimeObjectLifecycle;
pub use object_table::{RuntimeObjectEntry, RuntimeObjectTableError, RuntimeOwnerRelation};
pub use registry::{
    FrontendCapabilitySnapshot, FrontendRuntimeId, FrontendScalarCapability,
    IsdbtSegmentCapability, LnbRegistry, LnbRegistryProfile, SatellitePowerTopology,
};
pub use root_method_txn::{
    RootCommandRequest, RootDemuxCapabilitiesSnapshot, RootDemuxInfoSnapshot,
    RootFrontendInfoSnapshot, RootQueryRequest, RootQueryResponse,
};
pub use root_object_ops::RootOpenTxn;
pub use worker_failure_classifier::{ClassifiedWorkerTerminalResult, WorkerFailureCategory};
pub use worker_runtime::{
    filter_delivery_wake_sequence, join_worker_classified, notify_filter_delivery_change,
    wait_filter_delivery_change, WorkerHandle, WorkerRuntime, WorkerRuntimeReaperQueue,
    WorkerRuntimeSupervisor, WorkerTerminalResult, CLEANUP_RETRY_SCHEDULE_MS,
    CLEANUP_TERMINAL_DEADLINE_MS, WORKER_IO_DEADLINE_MS, WORKER_REAPER_DEADLINE_MS,
};
#[cfg(test)]
mod failure_injection_tests;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceState {
    Booting,
    Ready,
    Degraded,
    Closing,
    Failed,
    ServiceCritical,
}

#[cfg(test)]
mod tests {
    use super::*;
    use maleicacid_tuner_hal2_binder_adapter::{AidlApi, AidlMethodCall};
    use maleicacid_tuner_hal2_common::{FrontendBackendKind, FrontendSystem, HalError};
    use maleicacid_tuner_hal2_demux::{
        FilterConfig, FilterConfigKind, FilterOpenType, OpenFilterRequest, PacketPid, PesSettings,
        PipelineAssemblySuppressionReason, PipelineDeliveryAction, QueueRuntimeError,
        QueueRuntimeErrorKind,
    };
    use maleicacid_tuner_hal2_descrambler::{
        multi2_encrypt_payload, DescramblerKeySlot, DescramblerKeyToken, DescramblerPid,
        DescramblerPidClaim, Multi2KeyMaterial,
    };
    use maleicacid_tuner_hal2_domain_request::{
        AidlObjectGeneration, AidlObjectId, AidlObjectKind, DvrConfigureKind, DvrConfigureRequest,
        DvrDataFormat, DvrOpenKind, FilterDelayHintKind, FilterDelayHintRequest, OpenDvrRequest,
        RuntimeExecutableRequest, RuntimeTransactionName, AIDL_TRANSACTION_TABLE,
    };
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    fn test_descrambler_pid(pid: u16) -> DescramblerPid {
        DescramblerPidClaim::from_demux_input(pid)
            .expect("test PID must be valid")
            .pid()
    }

    fn test_packet_pid(pid: u16) -> PacketPid {
        PacketPid::from_descrambler_pid_for_service_runtime_boundary(test_descrambler_pid(pid))
    }

    #[test]
    fn queue_descriptor_query_diagnostic_store_records_typed_runtime_error() {
        let mut runtime = TunerServiceRuntime::default();
        let error = QueueRuntimeError {
            kind: QueueRuntimeErrorKind::ExportTransient,
            detail: "test queue descriptor export failure",
        };
        runtime.record_queue_descriptor_query_diagnostic(
            QueueDescriptorQueryDiagnosticRecord::new(
                AidlObjectKind::Filter,
                AidlObjectId(101),
                AidlObjectGeneration(7),
                25,
                error,
            ),
        );

        assert_eq!(
            runtime.queue_descriptor_query_diagnostics(),
            &[QueueDescriptorQueryDiagnosticRecord::new(
                AidlObjectKind::Filter,
                AidlObjectId(101),
                AidlObjectGeneration(7),
                25,
                error,
            )]
        );
    }

    fn descrambler_set_key_diagnostic_matches(
        record: &DescramblerDiagnosticRecord,
        expected_id: i32,
        expected_kind: DescramblerDiagnosticKind,
    ) -> bool {
        matches!(
            record,
            DescramblerDiagnosticRecord::SetKeyTokenFailure {
                descrambler_id,
                kind,
                ..
            } if *descrambler_id == expected_id && *kind == expected_kind
        )
    }

    fn descrambler_pid_claim_with_demux_matches(
        record: &DescramblerDiagnosticRecord,
        expected_phase: DescramblerDiagnosticPhase,
        expected_descrambler_id: i32,
        expected_demux_id: i32,
        expected_pid: u16,
        expected_filter_id: i32,
    ) -> bool {
        matches!(
            record,
            DescramblerDiagnosticRecord::PidClaimRejected {
                phase,
                descrambler_id,
                demux_id,
                pid,
                filter_id,
                ..
            } if *phase == expected_phase
                && *descrambler_id == expected_descrambler_id
                && *demux_id == expected_demux_id
                && *pid == test_descrambler_pid(expected_pid)
                && *filter_id == expected_filter_id
        )
    }

    fn descrambler_packet_policy_matches(
        record: &DescramblerDiagnosticRecord,
        expected_demux_id: i32,
        expected_pid: u16,
        expected_kind: DescramblerDiagnosticKind,
    ) -> bool {
        matches!(
            record,
            DescramblerDiagnosticRecord::PacketPolicy {
                demux_id,
                pid,
                kind,
            } if *demux_id == expected_demux_id
                && *pid == test_packet_pid(expected_pid)
                && *kind == expected_kind
        )
    }

    fn descrambler_source_filter_validation_matches(
        record: &DescramblerDiagnosticRecord,
        expected_demux_id: i32,
        expected_pid: u16,
        expected_filter_id: i32,
        expected_kind: DescramblerDiagnosticKind,
    ) -> bool {
        matches!(
            record,
            DescramblerDiagnosticRecord::PacketSourceFilterValidation {
                demux_id,
                pid,
                filter_id,
                kind,
                ..
            } if *demux_id == expected_demux_id
                && *pid == test_packet_pid(expected_pid)
                && *filter_id == expected_filter_id
                && *kind == expected_kind
        )
    }

    struct NoopFilterEventDispatcher;

    impl FilterEventDispatcher for NoopFilterEventDispatcher {
        fn dispatch(
            &self,
            _runtime: &Arc<Mutex<TunerServiceRuntime>>,
            _events: Vec<FilterEventDeliverySnapshot>,
        ) -> Result<(), HalError> {
            Ok(())
        }
    }

    fn path(name: &str) -> PathBuf {
        PathBuf::from(name)
    }

    fn available(
        id: i32,
        backend: FrontendBackendKind,
        system: FrontendSystem,
        path_name: &str,
        lnb_profile: Option<LnbRegistryProfile>,
    ) -> FrontendProbeOutcome {
        let exclusive_group_id = match backend {
            FrontendBackendKind::Px4CharDevice => {
                let relative = id.saturating_sub(1_000_000);
                let family = relative.div_euclid(10_000) & 0x03ff;
                let unit = relative.rem_euclid(10_000).div_euclid(10) & 0x3fff;
                0x1000_0000 | (family << 14) | unit
            }
            FrontendBackendKind::LinuxDvb => 0x2000_0000,
        };
        let capability = match system {
            FrontendSystem::IsdbT => crate::registry::FrontendCapabilitySnapshot {
                scalar: crate::registry::FrontendScalarCapability {
                    min_frequency_hz: 110_642_857,
                    max_frequency_hz: 767_642_857,
                    min_symbol_rate: 0,
                    max_symbol_rate: 0,
                    acquire_range_hz: 0,
                },
                exclusive_group_id,
                isdbt_segment: Some(crate::registry::IsdbtSegmentCapability {
                    is_segment_auto: true,
                    is_full_segment: true,
                }),
            },
            FrontendSystem::IsdbS => crate::registry::FrontendCapabilitySnapshot {
                scalar: crate::registry::FrontendScalarCapability {
                    min_frequency_hz: 1_049_480_000,
                    max_frequency_hz: 2_053_000_000,
                    min_symbol_rate: 28_860_000,
                    max_symbol_rate: 28_860_000,
                    acquire_range_hz: 0,
                },
                exclusive_group_id,
                isdbt_segment: None,
            },
            FrontendSystem::IsdbS3 | FrontendSystem::DvbS => unreachable!(),
        };
        FrontendProbeOutcome::Available {
            id: FrontendRuntimeId(id),
            backend,
            system,
            path: path(path_name),
            lnb_profile,
            satellite_power_topology: match (system, lnb_profile) {
                (
                    FrontendSystem::IsdbS,
                    Some(
                        LnbRegistryProfile::Px4Device15VOnly | LnbRegistryProfile::EarthPt1FixedLnb,
                    ),
                ) => crate::registry::SatellitePowerTopology::InternalFixed15V,
                (FrontendSystem::IsdbS, Some(LnbRegistryProfile::NoPower)) => {
                    crate::registry::SatellitePowerTopology::ExternalOrShared
                }
                _ => crate::registry::SatellitePowerTopology::UnknownOrDisabled,
            },
            capability,
        }
    }

    fn isdbt_request(frequency: u64) -> maleicacid_tuner_hal2_common::FrontendTuneRequest {
        maleicacid_tuner_hal2_common::FrontendTuneRequest {
            system: FrontendSystem::IsdbT,
            frequency,
            end_frequency: None,
            stream_id: None,
            stream_id_kind: None,
            bandwidth_hz: Some(6_000_000),
            symbol_rate: None,
            isdbt_layer_settings: Vec::new(),
            partial_reception:
                maleicacid_tuner_hal2_common::FrontendIsdbtPartialReceptionRequirement::Unspecified,
        }
    }

    #[test]
    fn missing_frontend_is_not_advertised() {
        let mut runtime = TunerServiceRuntime::new();
        let outcome = runtime.boot_from_probe_results([FrontendProbeOutcome::DeviceMissing {
            backend: FrontendBackendKind::Px4CharDevice,
            path: path("/dev/px4video0"),
        }]);

        assert_eq!(outcome, ServiceBootOutcome::Degraded);
        assert_eq!(runtime.state(), ServiceState::Degraded);
        assert_eq!(runtime.registry().frontend_count(), 0);
        assert_eq!(
            runtime.diagnostics()[0].kind(),
            StartupDiagnosticKind::DeviceMissing
        );
    }

    #[test]
    fn available_frontend_is_advertised() {
        let mut runtime = TunerServiceRuntime::new();
        let outcome = runtime.boot_from_probe_results([available(
            100,
            FrontendBackendKind::Px4CharDevice,
            FrontendSystem::IsdbT,
            "/dev/px4video0",
            None,
        )]);
        assert_eq!(outcome, ServiceBootOutcome::Ready);
        assert_eq!(runtime.registry().frontend_count(), 1);
    }

    #[test]
    fn duplicate_frontend_ids_are_suppressed() {
        let mut runtime = TunerServiceRuntime::new();
        let outcome = runtime.boot_from_probe_results([
            available(
                101,
                FrontendBackendKind::Px4CharDevice,
                FrontendSystem::IsdbT,
                "/dev/px4video0",
                None,
            ),
            available(
                101,
                FrontendBackendKind::LinuxDvb,
                FrontendSystem::IsdbT,
                "/dev/dvb/adapter0/frontend0",
                None,
            ),
        ]);
        assert_eq!(outcome, ServiceBootOutcome::Degraded);
        assert_eq!(runtime.registry().frontend_count(), 0);
    }

    #[test]
    fn unavailable_lnb_profile_does_not_publish_lnb() {
        let mut runtime = TunerServiceRuntime::new();
        let _ = runtime.boot_from_probe_results([available(
            1_010_001,
            FrontendBackendKind::Px4CharDevice,
            FrontendSystem::IsdbS,
            "/dev/px4video0",
            Some(LnbRegistryProfile::NoPower),
        )]);
        assert_eq!(runtime.registry().lnb_count(), 0);
    }

    #[test]
    fn public_lnb_profile_publishes_lnb() {
        let mut runtime = TunerServiceRuntime::new();
        let _ = runtime.boot_from_probe_results([available(
            1_010_001,
            FrontendBackendKind::Px4CharDevice,
            FrontendSystem::IsdbS,
            "/dev/px4video0",
            Some(LnbRegistryProfile::Px4Device15VOnly),
        )]);
        assert_eq!(runtime.registry().lnb_count(), 1);
    }

    #[test]
    fn non_satellite_frontend_does_not_publish_lnb() {
        let mut runtime = TunerServiceRuntime::new();
        let _ = runtime.boot_from_probe_results([available(
            1_010_000,
            FrontendBackendKind::Px4CharDevice,
            FrontendSystem::IsdbT,
            "/dev/px4video0",
            None,
        )]);
        assert_eq!(runtime.registry().lnb_count(), 0);
    }

    #[test]
    fn explicit_fixed_power_topology_does_not_create_public_lnb_without_controllable_profile() {
        let mut runtime = TunerServiceRuntime::new();
        let mut outcome = available(
            1_010_001,
            FrontendBackendKind::Px4CharDevice,
            FrontendSystem::IsdbS,
            "/dev/px4video0",
            Some(LnbRegistryProfile::NoPower),
        );
        if let FrontendProbeOutcome::Available {
            satellite_power_topology,
            ..
        } = &mut outcome
        {
            *satellite_power_topology = SatellitePowerTopology::InternalFixed15V;
        }
        let _ = runtime.boot_from_probe_results([outcome]);
        assert_eq!(runtime.registry().lnb_count(), 0);
        assert!(runtime
            .registry()
            .frontend_has_fixed_power_lease(FrontendRuntimeId(1_010_001)));
    }

    #[test]
    fn fixed_power_lease_keeps_rail_reference_after_public_lnb_close() {
        let mut runtime = TunerServiceRuntime::new();
        let _ = runtime.boot_from_probe_results([available(
            1_010_001,
            FrontendBackendKind::Px4CharDevice,
            FrontendSystem::IsdbS,
            "/dev/px4video0",
            Some(LnbRegistryProfile::Px4Device15VOnly),
        )]);
        let lnb_id = runtime.registry().lnb_ids()[0];
        assert_eq!(runtime.registry().lnb_rail_reference_count(lnb_id), Some(1));
        runtime.registry_mut().unregister_lnb(lnb_id).unwrap();
        assert_eq!(runtime.registry().lnb_rail_reference_count(lnb_id), Some(1));
    }

    #[test]
    fn fixed_power_lease_survives_frontend_lnb_assignment_release() {
        let mut runtime = TunerServiceRuntime::new();
        let _ = runtime.boot_from_probe_results([available(
            1_010_001,
            FrontendBackendKind::Px4CharDevice,
            FrontendSystem::IsdbS,
            "/dev/px4video0",
            Some(LnbRegistryProfile::Px4Device15VOnly),
        )]);
        let lnb_id = runtime.registry().lnb_ids()[0];
        let frontend_id = FrontendRuntimeId(1_010_001);
        let lease = runtime
            .registry_mut()
            .prepare_frontend_lnb_assignment(frontend_id, lnb_id)
            .unwrap();
        runtime
            .registry_mut()
            .commit_frontend_lnb_assignment(lease)
            .unwrap();
        runtime
            .registry_mut()
            .release_frontend_lnb_assignment(frontend_id)
            .unwrap();
        assert_eq!(runtime.registry().lnb_rail_reference_count(lnb_id), Some(1));
    }

    #[test]
    fn frontend_request_mapping_is_typed() {
        let mut runtime = TunerServiceRuntime::new();
        let _ = runtime.boot_from_probe_results([available(
            3,
            FrontendBackendKind::Px4CharDevice,
            FrontendSystem::IsdbT,
            "/dev/px4video0",
            None,
        )]);
        let request = runtime.frontend_request(3, isdbt_request(473_142_857)).unwrap();
        assert_eq!(request.system, FrontendSystem::IsdbT);
    }

    #[test]
    fn callback_dispatcher_can_be_installed_once() {
        let mut runtime = TunerServiceRuntime::new();
        runtime
            .install_filter_event_dispatcher(Arc::new(NoopFilterEventDispatcher))
            .unwrap();
        assert!(runtime
            .install_filter_event_dispatcher(Arc::new(NoopFilterEventDispatcher))
            .is_err());
    }

    #[test]
    fn demux_frontend_data_source_binds_and_live_sink_reaches_demux_runtime() {
        let mut runtime = TunerServiceRuntime::new();
        runtime
            .install_filter_event_dispatcher(Arc::new(NoopFilterEventDispatcher))
            .unwrap();
        let _ = runtime.boot_from_probe_results([available(
            1,
            FrontendBackendKind::Px4CharDevice,
            FrontendSystem::IsdbT,
            "/dev/px4video0",
            None,
        )]);
        let demux = runtime.allocate_demux_runtime().unwrap();
        runtime
            .set_demux_frontend_data_source(demux.id.0, 1)
            .unwrap();
        let runtime = Arc::new(Mutex::new(runtime));
        let sink = start_frontend_demux_live_pump_from_reader(
            Arc::clone(&runtime),
            1,
            Box::new(std::io::Cursor::new(vec![0u8; 188])),
        )
        .unwrap();
        let report = sink.join_after_stop();
        assert!(report.is_ok());
    }

    #[test]
    fn public_object_dispatch_rejects_wrong_generation() {
        let mut runtime = TunerServiceRuntime::new();
        let object = runtime
            .object_table
            .prepare_open_with_owner(
                AidlObjectKind::Filter,
                LedgerId(1),
                None,
                Some(RuntimeOwnerRelation::new(AidlObjectKind::Demux, AidlObjectId(5))),
                "test",
            )
            .unwrap();
        let entry = runtime.object_table.commit_open(object).unwrap();
        assert!(runtime
            .public_runtime_id_for_object_method(
                entry.object_id(),
                AidlObjectGeneration(entry.generation().0 + 1),
                AidlObjectKind::Filter,
            )
            .is_err());
    }

    #[test]
    fn playback_consume_processing_buffer_is_bounded() {
        assert_eq!(
            crate::playback_consume_txn::required_playback_processing_bytes(4 * 1024 * 1024),
            maleicacid_tuner_hal2_common::TS_PACKET_SIZE * 256
        );
    }

    #[test]
    fn cleanup_retry_schedule_is_bounded() {
        assert_eq!(CLEANUP_RETRY_SCHEDULE_MS, &[0, 10, 100, 1_000]);
        assert_eq!(CLEANUP_TERMINAL_DEADLINE_MS, 30_000);
    }

    #[test]
    fn worker_io_deadline_is_bounded() {
        assert_eq!(WORKER_IO_DEADLINE_MS, 2_000);
    }

    #[test]
    fn worker_reaper_deadline_is_bounded() {
        assert_eq!(WORKER_REAPER_DEADLINE_MS, 10_000);
    }
}

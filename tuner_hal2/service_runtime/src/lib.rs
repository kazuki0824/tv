mod boot;
mod callback_registry;
mod capability_snapshot;
mod capability_profile;
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
    configure_ip_cid_result, configure_monitor_event_result, failure_domain, feature_declared,
    hal_generates_japanese_scan_plan, open_failed, scan_candidate_owner, transport_declared,
    ProfileFeature, RuntimeFailureDomain, ScanCandidateOwner, TransportCapability,
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
    set_frontend_lnb_object_use_case, FrontendOperationEvent,
    FrontendOperationEventAcceptance, FrontendTuneScanTxn, FrontendWorkerTerminalEvent,
    FrontendWorkerTerminalEventAcceptance, SharedFrontendRuntime,
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
    CloseCleanupAttemptCompletion, CloseCleanupAuthority,
    ObjectArtifactCleanupCommand, ObjectArtifactCleanupExecutor, ObjectArtifactCleanupKind,
    ObjectCleanupDiagnosticKind, ObjectCleanupDiagnosticRecord, ObjectCleanupDiagnosticSnapshot,
    ObjectCleanupExecutionKind, ObjectCleanupExecutionReport, ObjectCleanupObjectTarget,
    ObjectCleanupStepOutcome, ObjectCloseCleanupFailure, ObjectCloseRuntimeExecutor,
    ObjectCloseCleanupAttempt, ObjectCloseTxn, ObjectCloseUseCasePlan,
    ObjectRuntimeCleanupCommand, ObjectRuntimeCleanupKind, SharedObjectCleanupDiagnostics,
};
pub use object_domain_cleanup::{
    ObjectDomainCleanupCommand, ObjectDomainCleanupExecutor, ObjectDomainCleanupKind,
    ObjectDomainCleanupOutcome,
};
pub use object_method_use_case::{
    lnb_profile_supports_voltage_status, ObjectFrontendStatusReadinessValue,
    ObjectFrontendStatusType, ObjectFrontendStatusValue, ObjectMethodExecutionToken,
    ObjectMethodUseCase, ObjectMethodUseCaseBuildError, ObjectQueryRequest, ObjectQueryResponse,
};
pub use object_lifecycle::{
    aidl_object_cleanup_dependency, aidl_object_cleanup_is_terminal,
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
pub use worker_runtime::{
    WorkerHandle, WorkerRuntime, WorkerTerminalResult, CLEANUP_RETRY_SCHEDULE_MS,
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
                        LnbRegistryProfile::Px4Device15VOnly
                        | LnbRegistryProfile::EarthPt1FixedLnb,
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
            1_000_000,
            FrontendBackendKind::Px4CharDevice,
            FrontendSystem::IsdbT,
            "/dev/px4video0",
            None,
        )]);

        assert_eq!(outcome, ServiceBootOutcome::Ready);
        assert_eq!(runtime.state(), ServiceState::Ready);
        assert_eq!(
            runtime.registry().frontend_ids(),
            vec![FrontendRuntimeId(1_000_000)]
        );
        assert!(runtime
            .registry()
            .frontend_runtime(FrontendRuntimeId(1_000_000))
            .is_some());
        assert!(matches!(
            runtime
                .execute_root_query(RootQueryRequest::FrontendInfo {
                    frontend_id: 1_000_000,
                })
                .unwrap(),
            RootQueryResponse::FrontendInfo(snapshot)
                if snapshot.capability.scalar.min_frequency_hz == 110_642_857
                    && snapshot.capability.scalar.max_frequency_hz == 767_642_857
                    && snapshot.capability.scalar.acquire_range_hz == 0
                    && snapshot.capability.isdbt_segment.is_some_and(|segment| {
                        segment.is_segment_auto && segment.is_full_segment
                    })
        ));
        assert!(runtime.diagnostics().is_empty());
    }

    #[test]
    fn explicit_isdbt_partial_reception_requires_px4_tmcc_readback() {
        let mut request = isdbt_request(473_142_857);
        request.partial_reception =
            maleicacid_tuner_hal2_common::FrontendIsdbtPartialReceptionRequirement::Required(true);

        let mut px4 = TunerServiceRuntime::new();
        px4.boot_from_probe_results([available(
            1_000_000,
            FrontendBackendKind::Px4CharDevice,
            FrontendSystem::IsdbT,
            "/dev/px4video0",
            None,
        )]);
        assert!(px4
            .validate_frontend_request_for_id(1_000_000, &request)
            .is_ok());

        let mut earth_pt1 = TunerServiceRuntime::new();
        earth_pt1.boot_from_probe_results([available(
            1_000_000,
            FrontendBackendKind::LinuxDvb,
            FrontendSystem::IsdbT,
            "/dev/dvb/adapter0/frontend0",
            None,
        )]);
        assert!(matches!(
            earth_pt1.validate_frontend_request_for_id(1_000_000, &request),
            Err(HalError::UnsupportedDetail {
                feature: "isdbt.partialReceptionFlag",
                ..
            })
        ));
    }

    #[test]
    fn frontend_limit_query_and_open_share_current_lease_limit() {
        let mut runtime = TunerServiceRuntime::new();
        runtime.boot_from_probe_results([
            available(
                1_000_000,
                FrontendBackendKind::Px4CharDevice,
                FrontendSystem::IsdbT,
                "/dev/px4video0",
                None,
            ),
            available(
                1_000_010,
                FrontendBackendKind::Px4CharDevice,
                FrontendSystem::IsdbT,
                "/dev/px4video1",
                None,
            ),
        ]);
        runtime
            .execute_root_command(RootCommandRequest::SetMaxNumberOfFrontends {
                frontend_system: FrontendSystem::IsdbT,
                max_number: 1,
            })
            .unwrap();
        assert_eq!(
            runtime
                .execute_root_query(RootQueryRequest::MaxNumberOfFrontends {
                    frontend_system: FrontendSystem::IsdbT,
                })
                .unwrap(),
            RootQueryResponse::MaxNumberOfFrontends(1)
        );
        runtime
            .root_open_txn().open_frontend_root_object_for_id(
                1_000_000,
                AidlMethodCall::PublicApi {
                    object: AidlObjectKind::Tuner,
                    api: AidlApi::TunerOpenFrontendById,
                },
            )
            .unwrap();
        assert!(matches!(
            runtime.root_open_txn().open_frontend_root_object_for_id(
                1_000_010,
                AidlMethodCall::PublicApi {
                    object: AidlObjectKind::Tuner,
                    api: AidlApi::TunerOpenFrontendById,
                },
            ),
            Err(HalError::Unsupported(_))
        ));
    }

    #[test]
    fn frontend_variants_in_same_physical_group_are_not_leased_concurrently() {
        let mut runtime = TunerServiceRuntime::new();
        runtime.boot_from_probe_results([
            available(
                1_000_000,
                FrontendBackendKind::Px4CharDevice,
                FrontendSystem::IsdbT,
                "/dev/px4video0",
                None,
            ),
            available(
                1_000_001,
                FrontendBackendKind::Px4CharDevice,
                FrontendSystem::IsdbS,
                "/dev/px4video0",
                Some(LnbRegistryProfile::Px4Device15VOnly),
            ),
        ]);
        runtime
            .root_open_txn().open_frontend_root_object_for_id(
                1_000_000,
                AidlMethodCall::PublicApi {
                    object: AidlObjectKind::Tuner,
                    api: AidlApi::TunerOpenFrontendById,
                },
            )
            .unwrap();

        assert!(matches!(
            runtime.root_open_txn().open_frontend_root_object_for_id(
                1_000_001,
                AidlMethodCall::PublicApi {
                    object: AidlObjectKind::Tuner,
                    api: AidlApi::TunerOpenFrontendById,
                },
            ),
            Err(HalError::Unsupported(_))
        ));
    }

    #[test]
    fn demux_inventory_queries_and_open_use_same_fixed_ids() {
        let mut runtime = TunerServiceRuntime::new();
        runtime.boot_from_probe_results([available(
            1_000_000,
            FrontendBackendKind::Px4CharDevice,
            FrontendSystem::IsdbT,
            "/dev/px4video0",
            None,
        )]);
        assert_eq!(
            runtime
                .execute_root_query(RootQueryRequest::DemuxIds)
                .unwrap(),
            RootQueryResponse::DemuxIds(vec![1, 2, 3, 4, 5, 6, 7, 8])
        );
        let opened = runtime
            .root_open_txn().open_demux_root_object(AidlMethodCall::PublicApi {
                object: AidlObjectKind::Tuner,
                api: AidlApi::TunerOpenDemux,
            })
            .unwrap();
        assert_eq!(opened.public_runtime_id().0, 1);
        assert!(matches!(
            runtime.root_open_txn().open_demux_root_object_by_id(
                1,
                AidlMethodCall::PublicApi {
                    object: AidlObjectKind::Tuner,
                    api: AidlApi::TunerOpenDemuxById,
                },
            ),
            Err(HalError::Unsupported(_))
        ));
    }

    #[test]
    fn frontend_worker_prepare_checks_runtime_without_committing_generation() {
        let mut runtime = TunerServiceRuntime::new();
        runtime.boot_from_probe_results([available(
            1_000_000,
            FrontendBackendKind::Px4CharDevice,
            FrontendSystem::IsdbT,
            "/dev/px4video0",
            None,
        )]);

        let generation = runtime
            .frontend_txn()
            .prepare_frontend_worker_generation(
                1_000_000,
                maleicacid_tuner_hal2_device::FrontendWorkerKind::Tune,
            )
            .expect("prepare must observe advertised runtime");
        assert_eq!(generation, 1);
        assert_eq!(
            runtime
                .registry()
                .frontend_runtime(FrontendRuntimeId(1_000_000))
                .unwrap()
                .generation(),
            0,
            "prepare must not fake-commit a tune state",
        );
    }

    #[test]
    fn frontend_worker_prepare_reaps_completed_failure_before_replacement() {
        let mut runtime = TunerServiceRuntime::new();
        runtime.boot_from_probe_results([available(
            1_000_000,
            FrontendBackendKind::Px4CharDevice,
            FrontendSystem::IsdbT,
            "/dev/px4video0",
            None,
        )]);

        let generation = runtime
            .frontend_txn()
            .prepare_frontend_worker_generation(
                1_000_000,
                maleicacid_tuner_hal2_device::FrontendWorkerKind::Tune,
            )
            .unwrap();
        runtime
            .frontend_txn()
            .install_frontend_live_reader_descriptor_for_generation(
                1_000_000,
                maleicacid_tuner_hal2_device::FrontendWorkerKind::Tune,
                generation,
            )
            .unwrap();
        runtime
            .frontend_txn()
            .start_worker(
                1_000_000,
                maleicacid_tuner_hal2_device::FrontendWorkerKind::Tune,
                generation,
                |_ctx| {
                    Err(HalError::internal(
                        maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                        "test frontend worker failure",
                    ))
                },
            )
            .unwrap();

        let mut next_generation = None;
        for _ in 0..100 {
            match runtime.frontend_txn().prepare_frontend_worker_generation(
                1_000_000,
                maleicacid_tuner_hal2_device::FrontendWorkerKind::Tune,
            ) {
                Ok(value) => {
                    next_generation = Some(value);
                    break;
                }
                Err(HalError::InvalidState { .. }) => std::thread::sleep(Duration::from_millis(1)),
                Err(other) => panic!("unexpected prepare error after worker completion: {other:?}"),
            }
        }

        assert_eq!(next_generation, Some(generation + 1));
        let frontend = runtime
            .registry()
            .frontend_runtime(FrontendRuntimeId(1_000_000))
            .unwrap();
        assert_eq!(
            frontend.snapshot().state,
            maleicacid_tuner_hal2_device::FrontendRuntimeState::Failed
        );
        assert!(matches!(
            frontend.last_error(),
            Some(HalError::Internal { .. })
        ));
    }

    #[test]
    fn tune_replacement_reserves_new_generation_before_stopping_old_worker() {
        let mut runtime = TunerServiceRuntime::new();
        runtime.boot_from_probe_results([available(
            1_000_000,
            FrontendBackendKind::Px4CharDevice,
            FrontendSystem::IsdbT,
            "/dev/px4video0",
            None,
        )]);
        let request = isdbt_request(473_142_857);
        let generation = runtime
            .frontend_txn()
            .prepare_frontend_worker_generation(
                1_000_000,
                maleicacid_tuner_hal2_device::FrontendWorkerKind::Tune,
            )
            .unwrap();
        runtime
            .frontend_txn()
            .install_frontend_live_reader_descriptor_for_generation(
                1_000_000,
                maleicacid_tuner_hal2_device::FrontendWorkerKind::Tune,
                generation,
            )
            .unwrap();
        runtime
            .frontend_txn()
            .commit_frontend_active_tune_request(1_000_000, generation, request.clone())
            .unwrap();
        runtime
            .frontend_txn()
            .start_worker(
                1_000_000,
                maleicacid_tuner_hal2_device::FrontendWorkerKind::Tune,
                generation,
                |ctx| {
                    while !ctx.cancel_requested() {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    assert_eq!(
                        ctx.cancel_reason()?,
                        Some(
                            maleicacid_tuner_hal2_device::FrontendWorkerCancelReason::SupersededByNewRequest
                        )
                    );
                    Ok(())
                },
            )
            .unwrap();

        let reserved_generation = runtime
            .frontend_txn()
            .prepare_frontend_worker_replacement_generation(
                1_000_000,
                maleicacid_tuner_hal2_device::FrontendWorkerKind::Tune,
            )
            .unwrap();
        assert_eq!(reserved_generation, generation + 1);
        assert_eq!(
            runtime
                .registry()
                .frontend_runtime(FrontendRuntimeId(1_000_000))
                .unwrap()
                .generation(),
            generation,
            "replacement generation reservation must not commit before old worker stop",
        );

        let outcome = crate::frontend_worker_txn::request_tune_worker_replacement_stop(
            &mut runtime,
            1_000_000,
        )
        .complete();

        assert!(matches!(
            outcome,
            maleicacid_tuner_hal2_device::FrontendWorkerStopOutcome::Completed {
                result: Ok(()),
                ..
            }
        ));
        assert_eq!(
            runtime
                .registry()
                .frontend_runtime(FrontendRuntimeId(1_000_000))
                .unwrap()
                .snapshot()
                .active_tune_request,
            Some(request)
        );
    }

    #[test]
    fn frontend_live_reader_descriptor_install_commits_generation_and_stop_clears_reader() {
        let mut runtime = TunerServiceRuntime::new();
        runtime.boot_from_probe_results([available(
            1_000_000,
            FrontendBackendKind::Px4CharDevice,
            FrontendSystem::IsdbT,
            "/dev/px4video0",
            None,
        )]);

        let generation = runtime
            .frontend_txn()
            .prepare_frontend_worker_generation(
                1_000_000,
                maleicacid_tuner_hal2_device::FrontendWorkerKind::Tune,
            )
            .unwrap();
        runtime
            .frontend_txn()
            .install_frontend_live_reader_descriptor_for_generation(
                1_000_000,
                maleicacid_tuner_hal2_device::FrontendWorkerKind::Tune,
                generation,
            )
            .unwrap();
        let frontend = runtime
            .registry()
            .frontend_runtime(FrontendRuntimeId(1_000_000))
            .unwrap();
        assert_eq!(frontend.generation(), generation);
        assert_eq!(
            frontend.snapshot().state,
            maleicacid_tuner_hal2_device::FrontendRuntimeState::Tuning { generation }
        );
        assert!(frontend.live_reader_descriptor().is_some());

        runtime
            .frontend_txn()
            .clear_frontend_live_reader_descriptor_and_idle(1_000_000)
            .unwrap();
        let frontend = runtime
            .registry()
            .frontend_runtime(FrontendRuntimeId(1_000_000))
            .unwrap();
        assert_eq!(
            frontend.snapshot().state,
            maleicacid_tuner_hal2_device::FrontendRuntimeState::Idle
        );
        assert!(frontend.live_reader_descriptor().is_none());
    }

    #[test]
    fn scan_cancel_records_terminal_event_before_idle() {
        let mut runtime = TunerServiceRuntime::new();
        runtime.boot_from_probe_results([available(
            1_000_000,
            FrontendBackendKind::Px4CharDevice,
            FrontendSystem::IsdbT,
            "/dev/px4video0",
            None,
        )]);

        let generation = runtime
            .frontend_txn()
            .prepare_frontend_worker_generation(
                1_000_000,
                maleicacid_tuner_hal2_device::FrontendWorkerKind::Scan,
            )
            .unwrap();
        runtime
            .frontend_txn()
            .install_frontend_live_reader_descriptor_for_generation(
                1_000_000,
                maleicacid_tuner_hal2_device::FrontendWorkerKind::Scan,
                generation,
            )
            .unwrap();
        runtime
            .registry_mut_for_test()
            .frontend_runtime_mut(FrontendRuntimeId(1_000_000))
            .unwrap()
            .record_scan_cancelled(
                generation,
                maleicacid_tuner_hal2_device::FrontendWorkerCancelReason::StopRequested,
            )
            .unwrap();
        let events = runtime
            .registry()
            .frontend_runtime(FrontendRuntimeId(1_000_000))
            .unwrap()
            .terminal_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].generation, generation);
        assert_eq!(
            events[0].kind,
            maleicacid_tuner_hal2_device::FrontendTerminalEventKind::ScanCancelled,
        );

        runtime
            .frontend_txn()
            .clear_frontend_live_reader_descriptor_and_idle(1_000_000)
            .unwrap();
        let frontend = runtime
            .registry()
            .frontend_runtime(FrontendRuntimeId(1_000_000))
            .unwrap();
        assert_eq!(
            frontend.snapshot().state,
            maleicacid_tuner_hal2_device::FrontendRuntimeState::Idle
        );
    }

    #[test]
    fn close_frontend_workers_requests_tune_and_scan_worker_stop() {
        let runtime = Arc::new(Mutex::new(TunerServiceRuntime::new()));
        let (reason_tx, reason_rx) = std::sync::mpsc::channel();
        {
            let mut guard = runtime.lock().unwrap();
            guard.boot_from_probe_results([available(
                1_000_000,
                FrontendBackendKind::Px4CharDevice,
                FrontendSystem::IsdbT,
                "/dev/px4video0",
                None,
            )]);
            guard
                .install_filter_event_dispatcher(Arc::new(NoopFilterEventDispatcher))
                .unwrap();

            let tune_generation = guard
                .frontend_txn()
                .prepare_frontend_worker_generation(
                    1_000_000,
                    maleicacid_tuner_hal2_device::FrontendWorkerKind::Tune,
                )
                .unwrap();
            guard
                .frontend_txn()
                .install_frontend_live_reader_descriptor_for_generation(
                    1_000_000,
                    maleicacid_tuner_hal2_device::FrontendWorkerKind::Tune,
                    tune_generation,
                )
                .unwrap();
            let tune_tx = reason_tx.clone();
            guard
                .frontend_txn()
                .start_worker(
                    1_000_000,
                    maleicacid_tuner_hal2_device::FrontendWorkerKind::Tune,
                    tune_generation,
                    move |ctx| {
                        while !ctx.cancel_requested() {
                            std::thread::sleep(Duration::from_millis(1));
                        }
                        tune_tx
                            .send((ctx.kind(), ctx.cancel_reason().unwrap()))
                            .unwrap();
                        Ok(())
                    },
                )
                .unwrap();

            let scan_generation = guard
                .frontend_txn()
                .prepare_frontend_worker_generation(
                    1_000_000,
                    maleicacid_tuner_hal2_device::FrontendWorkerKind::Scan,
                )
                .unwrap();
            guard
                .frontend_txn()
                .install_frontend_live_reader_descriptor_for_generation(
                    1_000_000,
                    maleicacid_tuner_hal2_device::FrontendWorkerKind::Scan,
                    scan_generation,
                )
                .unwrap();
            guard
                .frontend_txn()
                .begin_frontend_scan_session(
                    1_000_000,
                    scan_generation,
                    "close-worker-test".to_string(),
                    vec![isdbt_request(473_142_857)],
                )
                .unwrap();
            guard
                .frontend_txn()
                .start_worker(
                    1_000_000,
                    maleicacid_tuner_hal2_device::FrontendWorkerKind::Scan,
                    scan_generation,
                    move |ctx| {
                        while !ctx.cancel_requested() {
                            std::thread::sleep(Duration::from_millis(1));
                        }
                        reason_tx
                            .send((ctx.kind(), ctx.cancel_reason().unwrap()))
                            .unwrap();
                        Ok(())
                    },
                )
                .unwrap();
        }

        crate::frontend_worker_txn::close_frontend_workers_and_live_data(
            Arc::clone(&runtime),
            1_000_000,
            maleicacid_tuner_hal2_device::FrontendWorkerCancelReason::FrontendClosing,
        )
        .unwrap();

        let first = reason_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let second = reason_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let mut reasons = vec![first, second];
        reasons.sort_by_key(|(kind, _)| *kind);
        assert_eq!(
            reasons,
            vec![
                (
                    maleicacid_tuner_hal2_device::FrontendWorkerKind::Tune,
                    Some(maleicacid_tuner_hal2_device::FrontendWorkerCancelReason::FrontendClosing,),
                ),
                (
                    maleicacid_tuner_hal2_device::FrontendWorkerKind::Scan,
                    Some(maleicacid_tuner_hal2_device::FrontendWorkerCancelReason::FrontendClosing,),
                ),
            ]
        );
    }

    #[test]
    fn dvb_frontend_live_reader_descriptor_uses_adapter_dvr_path() {
        let mut runtime = TunerServiceRuntime::new();
        runtime.boot_from_probe_results([available(
            2_000_000,
            FrontendBackendKind::LinuxDvb,
            FrontendSystem::IsdbT,
            "/dev/dvb/adapter3/frontend1",
            None,
        )]);

        let generation = runtime
            .frontend_txn()
            .prepare_frontend_worker_generation(
                2_000_000,
                maleicacid_tuner_hal2_device::FrontendWorkerKind::Tune,
            )
            .unwrap();
        runtime
            .frontend_txn()
            .install_frontend_live_reader_descriptor_for_generation(
                2_000_000,
                maleicacid_tuner_hal2_device::FrontendWorkerKind::Tune,
                generation,
            )
            .unwrap();
        let reader = runtime
            .registry()
            .frontend_runtime(FrontendRuntimeId(2_000_000))
            .unwrap()
            .live_reader_descriptor()
            .unwrap();
        match &reader.kind {
            maleicacid_tuner_hal2_device::FrontendLiveReaderDescriptorKind::DvbDvrDevice {
                dvr_path,
            } => {
                assert_eq!(dvr_path.display(), "/dev/dvb/adapter3/dvr0");
            }
            other => panic!("unexpected reader kind: {other:?}"),
        }
    }

    #[test]
    fn isdbs_frontend_registers_default_lnb_from_probe() {
        let mut runtime = TunerServiceRuntime::new();
        let outcome = runtime.boot_from_probe_results([available(
            1_010_001,
            FrontendBackendKind::Px4CharDevice,
            FrontendSystem::IsdbS,
            "/dev/px4video0",
            Some(LnbRegistryProfile::Px4Device15VOnly),
        )]);

        assert_eq!(outcome, ServiceBootOutcome::Ready);
        assert_eq!(runtime.query().lnb_ids(), vec![1_020_001]);
        assert!(runtime
            .query()
            .lnb_id_by_name("maleicacid-lnb-px4-px4video0-unit-0")
            .is_some());
    }

    #[test]
    fn externally_powered_isdbs_frontend_does_not_publish_an_lnb_endpoint() {
        let mut runtime = TunerServiceRuntime::new();
        let outcome = runtime.boot_from_probe_results([available(
            1_020_001,
            FrontendBackendKind::Px4CharDevice,
            FrontendSystem::IsdbS,
            "/dev/pxmlt5video0",
            Some(LnbRegistryProfile::NoPower),
        )]);

        assert_eq!(outcome, ServiceBootOutcome::Ready);
        assert_eq!(runtime.query().frontend_ids(), vec![1_020_001]);
        assert!(runtime.query().lnb_ids().is_empty());
    }

    #[test]
    fn linux_dvb_isdbs_probe_keeps_fixed_lnb_profile_in_registry() {
        let mut runtime = TunerServiceRuntime::new();
        let outcome = runtime.boot_from_probe_results([available(
            2_000_001,
            FrontendBackendKind::LinuxDvb,
            FrontendSystem::IsdbS,
            "/dev/dvb/adapter0/frontend0",
            Some(LnbRegistryProfile::EarthPt1FixedLnb),
        )]);

        assert_eq!(outcome, ServiceBootOutcome::Ready);
        let frontend = runtime.frontend_entry(2_000_001).unwrap();
        assert_eq!(
            frontend.lnb_profile,
            Some(LnbRegistryProfile::EarthPt1FixedLnb)
        );
        let lnb = runtime.query().lnb_for_frontend_id(2_000_001).unwrap();
        assert_eq!(lnb.profile, LnbRegistryProfile::EarthPt1FixedLnb);
    }

    #[test]
    fn duplicate_frontend_id_is_diagnostic_not_second_entry() {
        let mut runtime = TunerServiceRuntime::new();
        let outcome = runtime.boot_from_probe_results([
            available(
                2_000_000,
                FrontendBackendKind::LinuxDvb,
                FrontendSystem::IsdbT,
                "/dev/dvb/adapter0/frontend0",
                None,
            ),
            available(
                2_000_000,
                FrontendBackendKind::LinuxDvb,
                FrontendSystem::IsdbT,
                "/dev/dvb/adapter1/frontend0",
                None,
            ),
        ]);

        assert_eq!(outcome, ServiceBootOutcome::Degraded);
        assert_eq!(runtime.registry().frontend_count(), 1);
        assert!(runtime
            .diagnostics()
            .iter()
            .any(|record| record.kind() == StartupDiagnosticKind::DuplicateFrontendId));
    }

    #[test]
    fn demux_frontend_data_source_binds_and_live_sink_reaches_demux_runtime() {
        use std::sync::{Arc, Mutex};

        let runtime = Arc::new(Mutex::new(TunerServiceRuntime::new()));
        {
            let mut guard = runtime.lock().unwrap();
            guard.boot_from_probe_results([available(
                1_000_000,
                FrontendBackendKind::Px4CharDevice,
                FrontendSystem::IsdbT,
                "/dev/px4video0",
                None,
            )]);
            guard
                .install_filter_event_dispatcher(Arc::new(NoopFilterEventDispatcher))
                .unwrap();
            let demux = guard.allocate_demux_runtime().unwrap();
            let before = guard
                .registry()
                .demux_runtime(demux.id)
                .unwrap()
                .generation();
            guard
                .set_demux_frontend_data_source(demux.id.0, 1_000_000)
                .unwrap();
            let after = guard
                .registry()
                .demux_runtime(demux.id)
                .unwrap()
                .generation();
            assert_eq!(after, before + 1);
            guard
                .set_demux_frontend_data_source(demux.id.0, 1_000_000)
                .unwrap();
            assert_eq!(
                guard
                    .registry()
                    .demux_runtime(demux.id)
                    .unwrap()
                    .generation(),
                after
            );
        }

        let mut packet = [0xffu8; maleicacid_tuner_hal2_common::TS_PACKET_SIZE];
        packet[0] = 0x47;
        packet[1] = 0x40;
        packet[2] = 0x00;
        packet[3] = 0x10;
        let mut stream = Vec::new();
        stream.extend_from_slice(&packet);
        stream.extend_from_slice(&packet);
        stream.extend_from_slice(&packet);
        let reader = Box::new(std::io::Cursor::new(stream));
        let mut owner = crate::start_frontend_demux_live_pump_from_reader(
            Arc::clone(&runtime),
            1_000_000,
            reader,
        )
        .unwrap();
        let report = {
            let mut completed = None;
            for _ in 0..100 {
                match owner.collect_if_finished() {
                    maleicacid_tuner_hal2_device::FrontendLivePumpJoinOutcome::Running => {
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                    maleicacid_tuner_hal2_device::FrontendLivePumpJoinOutcome::Completed(
                        result,
                    ) => {
                        completed = Some(result);
                        break;
                    }
                }
            }
            completed
                .unwrap_or_else(|| owner.join_after_stop())
                .unwrap()
        };
        assert_eq!(report.packets_delivered, 3);
    }

    #[test]
    fn every_binder_adapter_transaction_has_service_runtime_dispatch_target() {
        for plan in AIDL_TRANSACTION_TABLE {
            assert!(dispatch_target_for(plan.transaction()).is_some());
        }
    }

    #[test]
    fn dispatch_target_for_frontend_tune_is_frontend() {
        assert_eq!(
            dispatch_target_for(RuntimeTransactionName::FrontendTuneTxnApply),
            Some(ServiceRuntimeDispatchTarget::Frontend),
        );
    }

    #[test]
    fn command_plan_reaches_service_runtime_dispatch_target() {
        let command_plan = AIDL_TRANSACTION_TABLE
            .iter()
            .copied()
            .find(|plan| plan.transaction() == RuntimeTransactionName::FrontendTuneTxnApply)
            .expect("frontend tune transaction exists");
        let plan =
            RuntimeCommandDispatcher::plan(command_plan, None).expect("dispatch target exists");
        assert_eq!(
            plan.command_plan.transaction(),
            RuntimeTransactionName::FrontendTuneTxnApply
        );
        assert_eq!(plan.target, ServiceRuntimeDispatchTarget::Frontend);
    }

    #[test]
    fn every_runtime_contract_transaction_reaches_service_runtime_dispatch_target() {
        for command_plan in AIDL_TRANSACTION_TABLE {
            assert!(RuntimeCommandDispatcher::plan(*command_plan, None).is_ok());
        }
    }

    #[test]
    fn runtime_object_table_rejects_duplicate_object_id() {
        use maleicacid_tuner_hal2_domain_request::{
            AidlObjectGeneration, AidlObjectId, AidlObjectKind,
        };
        use maleicacid_tuner_hal2_resource_ledger::{LedgerGeneration, LedgerId};

        let mut table = crate::object_table::RuntimeObjectTable::default();
        let first = RuntimeObjectEntry {
            object_kind: AidlObjectKind::Frontend,
            object_id: AidlObjectId(10),
            generation: AidlObjectGeneration(1),
            ledger_id: LedgerId(10),
            ledger_generation: LedgerGeneration(1),
            owner: RuntimeOwnerRelation::Root,
            lifecycle: RuntimeObjectLifecycle::Live,
        };
        let second = RuntimeObjectEntry {
            object_kind: AidlObjectKind::Demux,
            ..first.clone()
        };
        table.insert(first).expect("first object insert succeeds");
        let err = table
            .insert(second)
            .expect_err("same object id cannot be reused for another kind");
        assert!(matches!(
            err,
            RuntimeObjectTableError::DuplicateObjectId {
                existing_kind: AidlObjectKind::Frontend,
                attempted_kind: AidlObjectKind::Demux,
                ..
            }
        ));
    }

    #[test]
    fn runtime_object_table_rejects_duplicate_live_runtime_binding() {
        use maleicacid_tuner_hal2_domain_request::{
            AidlObjectGeneration, AidlObjectId, AidlObjectKind,
        };
        use maleicacid_tuner_hal2_resource_ledger::{LedgerGeneration, LedgerId};

        let mut table = crate::object_table::RuntimeObjectTable::default();
        table
            .insert(RuntimeObjectEntry {
                object_kind: AidlObjectKind::Demux,
                object_id: AidlObjectId(100),
                generation: AidlObjectGeneration(1),
                ledger_id: LedgerId(7),
                ledger_generation: LedgerGeneration(1),
                owner: RuntimeOwnerRelation::Root,
                lifecycle: RuntimeObjectLifecycle::Live,
            })
            .expect("first wrapper insert succeeds");
        let err = table
            .insert(RuntimeObjectEntry {
                object_kind: AidlObjectKind::Demux,
                object_id: AidlObjectId(101),
                generation: AidlObjectGeneration(2),
                ledger_id: LedgerId(7),
                ledger_generation: LedgerGeneration(2),
                owner: RuntimeOwnerRelation::Root,
                lifecycle: RuntimeObjectLifecycle::Live,
            })
            .expect_err(
                "same runtime demux cannot have two live wrappers without refcount ownership",
            );
        assert!(matches!(
            err,
            RuntimeObjectTableError::DuplicateRuntimeBinding {
                object_kind: AidlObjectKind::Demux,
                runtime_id: LedgerId(7)
            }
        ));
    }

    #[test]
    fn runtime_object_table_reports_generation_mismatch() {
        use maleicacid_tuner_hal2_domain_request::{
            AidlObjectGeneration, AidlObjectId, AidlObjectKind,
        };
        use maleicacid_tuner_hal2_resource_ledger::{LedgerGeneration, LedgerId};

        let mut table = crate::object_table::RuntimeObjectTable::default();
        table
            .insert(RuntimeObjectEntry {
                object_kind: AidlObjectKind::Filter,
                object_id: AidlObjectId(20),
                generation: AidlObjectGeneration(4),
                ledger_id: LedgerId(20),
                ledger_generation: LedgerGeneration(1),
                owner: RuntimeOwnerRelation::Root,
                lifecycle: RuntimeObjectLifecycle::Live,
            })
            .expect("insert succeeds");
        let err = table
            .entry_checked(AidlObjectId(20), AidlObjectGeneration(5))
            .expect_err("generation mismatch is typed");
        assert!(matches!(
            err,
            RuntimeObjectTableError::GenerationMismatch {
                expected: AidlObjectGeneration(4),
                actual: AidlObjectGeneration(5),
                ..
            }
        ));
    }

    #[test]
    fn closed_runtime_object_rejects_later_public_method_lookup() {
        use maleicacid_tuner_hal2_domain_request::{
            AidlObjectGeneration, AidlObjectId, AidlObjectKind,
        };
        use maleicacid_tuner_hal2_resource_ledger::{CleanupStep, LedgerGeneration, LedgerId};

        let mut table = crate::object_table::RuntimeObjectTable::default();
        table
            .insert(RuntimeObjectEntry {
                object_kind: AidlObjectKind::Filter,
                object_id: AidlObjectId(44),
                generation: AidlObjectGeneration(3),
                ledger_id: LedgerId(44),
                ledger_generation: LedgerGeneration(3),
                owner: RuntimeOwnerRelation::Root,
                lifecycle: RuntimeObjectLifecycle::Live,
            })
            .expect("insert succeeds");
        table
            .begin_close_cascade(
                AidlObjectId(44),
                AidlObjectGeneration(3),
                CleanupStep::StopWorker,
            )
            .expect("begin close succeeds");
        table
            .commit_close_cascade(AidlObjectId(44), AidlObjectGeneration(3))
            .expect("commit close succeeds");
        let err = table
            .entry_for_kind(
                AidlObjectId(44),
                AidlObjectGeneration(3),
                AidlObjectKind::Filter,
            )
            .expect_err("closed object is not live");
        assert!(matches!(
            err,
            RuntimeObjectTableError::InvalidLifecycle {
                lifecycle: RuntimeObjectLifecycle::Closed,
                ..
            }
        ));
    }

    #[test]
    fn runtime_object_table_rejects_child_when_owner_is_missing() {
        use maleicacid_tuner_hal2_domain_request::{
            AidlObjectGeneration, AidlObjectId, AidlObjectKind,
        };
        use maleicacid_tuner_hal2_resource_ledger::{LedgerGeneration, LedgerId};

        let mut table = crate::object_table::RuntimeObjectTable::default();
        let err = table
            .insert(RuntimeObjectEntry {
                object_kind: AidlObjectKind::Filter,
                object_id: AidlObjectId(70),
                generation: AidlObjectGeneration(1),
                ledger_id: LedgerId(70),
                ledger_generation: LedgerGeneration(1),
                owner: RuntimeOwnerRelation::Demux {
                    demux: AidlObjectId(7),
                    generation: AidlObjectGeneration(1),
                },
                lifecycle: RuntimeObjectLifecycle::Live,
            })
            .expect_err("child object without live owner must be rejected");
        assert!(matches!(
            err,
            RuntimeObjectTableError::MissingOwner {
                object_id: AidlObjectId(70),
                owner_id: AidlObjectId(7),
                ..
            }
        ));
    }

    #[test]
    fn parent_close_cascades_to_child_object_table_entries() {
        use maleicacid_tuner_hal2_domain_request::{
            AidlObjectGeneration, AidlObjectId, AidlObjectKind,
        };
        use maleicacid_tuner_hal2_resource_ledger::{CleanupStep, LedgerGeneration, LedgerId};

        let mut table = crate::object_table::RuntimeObjectTable::default();
        table
            .insert(RuntimeObjectEntry {
                object_kind: AidlObjectKind::Demux,
                object_id: AidlObjectId(80),
                generation: AidlObjectGeneration(1),
                ledger_id: LedgerId(80),
                ledger_generation: LedgerGeneration(1),
                owner: RuntimeOwnerRelation::Root,
                lifecycle: RuntimeObjectLifecycle::Live,
            })
            .expect("parent insert succeeds");
        table
            .insert(RuntimeObjectEntry {
                object_kind: AidlObjectKind::Filter,
                object_id: AidlObjectId(81),
                generation: AidlObjectGeneration(2),
                ledger_id: LedgerId(81),
                ledger_generation: LedgerGeneration(2),
                owner: RuntimeOwnerRelation::Demux {
                    demux: AidlObjectId(80),
                    generation: AidlObjectGeneration(1),
                },
                lifecycle: RuntimeObjectLifecycle::Live,
            })
            .expect("child insert succeeds");

        table
            .begin_close_cascade(
                AidlObjectId(80),
                AidlObjectGeneration(1),
                CleanupStep::UnregisterRuntime,
            )
            .expect("cascade begin close succeeds");
        assert!(matches!(
            table
                .entry(AidlObjectId(81))
                .expect("child remains tracked")
                .lifecycle,
            RuntimeObjectLifecycle::Closing {
                step: CleanupStep::UnregisterRuntime
            }
        ));
        table
            .commit_close_cascade(AidlObjectId(80), AidlObjectGeneration(1))
            .expect("cascade commit close succeeds");
        let err = table
            .entry_for_kind(
                AidlObjectId(81),
                AidlObjectGeneration(2),
                AidlObjectKind::Filter,
            )
            .expect_err("closed child cannot be used by later public methods");
        assert!(matches!(
            err,
            RuntimeObjectTableError::InvalidLifecycle {
                object_id: AidlObjectId(81),
                lifecycle: RuntimeObjectLifecycle::Closed
            }
        ));
    }

    #[test]
    fn cleanup_failed_runtime_object_can_retry_close() {
        use maleicacid_tuner_hal2_domain_request::{
            AidlObjectGeneration, AidlObjectId, AidlObjectKind,
        };
        use maleicacid_tuner_hal2_resource_ledger::{CleanupStep, LedgerGeneration, LedgerId};

        let mut table = crate::object_table::RuntimeObjectTable::default();
        table
            .insert(RuntimeObjectEntry {
                object_kind: AidlObjectKind::Lnb,
                object_id: AidlObjectId(55),
                generation: AidlObjectGeneration(7),
                ledger_id: LedgerId(55),
                ledger_generation: LedgerGeneration(7),
                owner: RuntimeOwnerRelation::Root,
                lifecycle: RuntimeObjectLifecycle::Live,
            })
            .expect("insert succeeds");
        table
            .begin_close_cascade(
                AidlObjectId(55),
                AidlObjectGeneration(7),
                CleanupStep::StopWorker,
            )
            .expect("begin close succeeds");
        table
            .mark_cleanup_failed_cascade(
                AidlObjectId(55),
                AidlObjectGeneration(7),
                CleanupStep::UnregisterRuntime,
            )
            .expect("mark failed succeeds");
        table
            .begin_close_cascade(
                AidlObjectId(55),
                AidlObjectGeneration(7),
                CleanupStep::UnregisterRuntime,
            )
            .expect("cleanup failed close can be retried");
        table
            .commit_close_cascade(AidlObjectId(55), AidlObjectGeneration(7))
            .expect("retry close commit succeeds");
    }

    fn configured_pes_filter_request() -> OpenFilterRequest {
        OpenFilterRequest {
            open_type: FilterOpenType::TsPes,
            buffer_size: 4096,
            callback_present: false,
        }
    }

    fn configured_pes_filter_config(pid: i32) -> FilterConfig {
        FilterConfig {
            open_type: FilterOpenType::TsPes,
            tpid: pid,
            kind: FilterConfigKind::TsPes(PesSettings {
                stream_id: 0xffff,
                raw: false,
            }),
        }
    }

    fn assert_product_profile_opens_filter_through_demux_use_case(open_type: FilterOpenType) {
        let runtime = Arc::new(Mutex::new(TunerServiceRuntime::new()));
        let demux_entry = {
            let mut guard = runtime.lock().unwrap();
            guard
                .root_open_txn()
                .open_demux_root_object(AidlMethodCall::PublicApi {
                    object: AidlObjectKind::Tuner,
                    api: AidlApi::TunerOpenDemux,
                })
                .unwrap()
        };

        ObjectMethodUseCase::execute_after_live(
            &runtime,
            demux_entry.object_id(),
            demux_entry.generation(),
            AidlObjectKind::Demux,
            || -> Result<_, HalError> {
                let request = OpenFilterRequest {
                    open_type,
                    buffer_size: 4096,
                    callback_present: false,
                };
                Ok((
                    AidlMethodCall::DemuxOpenFilter(RuntimeExecutableRequest::OpenFilter(
                        request.clone(),
                    )),
                    request,
                ))
            },
            |runtime, dispatch, request| {
                runtime
                    .child_open_txn()
                    .open_filter_child_runtime_for_demux_object(
                        demux_entry.object_id(),
                        demux_entry.generation(),
                        &request,
                        dispatch,
                    )
            },
        )
        .expect("published filter capability must open through the public use-case path");
    }

    #[test]
    fn attach_dvr_filter_maps_non_record_filter_to_invalid_argument() {
        let mut runtime = TunerServiceRuntime::new();
        let demux = runtime.allocate_demux_runtime().unwrap();
        let filter = runtime.allocate_filter_runtime(demux.id.0).unwrap();
        runtime
            .register_demux_filter_runtime(
                demux.id.0,
                filter.id.0,
                &OpenFilterRequest {
                    open_type: FilterOpenType::TsRaw,
                    buffer_size: 4096,
                    callback_present: false,
                },
            )
            .unwrap();

        let dvr = runtime.allocate_dvr_runtime(demux.id.0).unwrap();
        runtime
            .register_demux_dvr_runtime(
                demux.id.0,
                dvr.id.0,
                &OpenDvrRequest {
                    kind: DvrOpenKind::Record,
                    buffer_size: 8192,
                },
                true,
            )
            .unwrap();
        runtime
            .configure_dvr_runtime_request(
                dvr.id.0,
                DvrConfigureRequest {
                    kind: DvrConfigureKind::Record,
                    status_mask: 0,
                    low_threshold_bytes: 0,
                    high_threshold_bytes: 0,
                    data_format: DvrDataFormat::Ts,
                    packet_size: 188,
                },
            )
            .unwrap();

        let error = runtime
            .attach_dvr_filter(dvr.id.0, filter.id.0)
            .unwrap_err();
        assert!(matches!(error, HalError::InvalidArgument { .. }));
    }

    #[test]
    fn product_profile_opens_ts_audio_through_demux_open_filter_use_case() {
        assert_product_profile_opens_filter_through_demux_use_case(FilterOpenType::TsAudio);
    }

    #[test]
    fn product_profile_opens_ts_video_through_demux_open_filter_use_case() {
        assert_product_profile_opens_filter_through_demux_use_case(FilterOpenType::TsVideo);
    }

    #[test]
    fn record_filter_data_size_delay_hint_is_accepted() {
        let mut runtime = TunerServiceRuntime::new();
        let demux = runtime.allocate_demux_runtime().unwrap();
        let filter = runtime.allocate_filter_runtime(demux.id.0).unwrap();
        runtime
            .register_demux_filter_runtime(
                demux.id.0,
                filter.id.0,
                &OpenFilterRequest {
                    open_type: FilterOpenType::TsRecord,
                    buffer_size: 4096,
                    callback_present: false,
                },
            )
            .unwrap();

        runtime
            .set_filter_delay_hint_request(
                filter.id.0,
                FilterDelayHintRequest {
                    kind: FilterDelayHintKind::DataSizeDelayBytes,
                    value: 188,
                },
            )
            .unwrap();
    }

    #[test]
    fn raw_and_record_filters_share_the_published_ts_filter_capacity() {
        let mut capability_snapshot = CapabilitySnapshot::product_default();
        capability_snapshot.num_ts_filter = 1;
        let runtime = Arc::new(Mutex::new(
            TunerServiceRuntime::from_capability_snapshot_for_test(capability_snapshot),
        ));
        let demux_entry = {
            let mut guard = runtime.lock().unwrap();
            guard
                .root_open_txn()
                .open_demux_root_object(AidlMethodCall::PublicApi {
                    object: AidlObjectKind::Tuner,
                    api: AidlApi::TunerOpenDemux,
                })
                .unwrap()
        };
        let open_filter = |open_type| {
            ObjectMethodUseCase::execute_after_live(
                &runtime,
                demux_entry.object_id(),
                demux_entry.generation(),
                AidlObjectKind::Demux,
                || -> Result<_, HalError> {
                    let request = OpenFilterRequest {
                        open_type,
                        buffer_size: 4096,
                        callback_present: false,
                    };
                    Ok((
                        AidlMethodCall::DemuxOpenFilter(RuntimeExecutableRequest::OpenFilter(
                            request.clone(),
                        )),
                        request,
                    ))
                },
                |runtime, dispatch, request| {
                    runtime
                        .child_open_txn()
                        .open_filter_child_runtime_for_demux_object(
                            demux_entry.object_id(),
                            demux_entry.generation(),
                            &request,
                            dispatch,
                        )
                },
            )
        };
        open_filter(FilterOpenType::TsRaw).unwrap();
        let error = open_filter(FilterOpenType::TsRecord).unwrap_err();
        assert!(matches!(
            error,
            ObjectMethodUseCaseBuildError::Runtime(HalError::Unsupported(_))
        ));
    }

    #[test]
    fn playback_dvr_filter_link_is_typed_unavailable() {
        let mut runtime = TunerServiceRuntime::new();
        let demux = runtime.allocate_demux_runtime().unwrap();
        let filter = runtime.allocate_filter_runtime(demux.id.0).unwrap();
        runtime
            .register_demux_filter_runtime(
                demux.id.0,
                filter.id.0,
                &OpenFilterRequest {
                    open_type: FilterOpenType::TsRecord,
                    buffer_size: 4096,
                    callback_present: false,
                },
            )
            .unwrap();
        let dvr = runtime.allocate_dvr_runtime(demux.id.0).unwrap();
        runtime
            .register_demux_dvr_runtime(
                demux.id.0,
                dvr.id.0,
                &OpenDvrRequest {
                    kind: DvrOpenKind::Playback,
                    buffer_size: 8192,
                },
                true,
            )
            .unwrap();
        runtime
            .configure_dvr_runtime_request(
                dvr.id.0,
                DvrConfigureRequest {
                    kind: DvrConfigureKind::Playback,
                    status_mask: 0,
                    low_threshold_bytes: 0,
                    high_threshold_bytes: 0,
                    data_format: DvrDataFormat::Ts,
                    packet_size: 188,
                },
            )
            .unwrap();

        assert!(matches!(
            runtime.attach_dvr_filter(dvr.id.0, filter.id.0),
            Err(HalError::Unsupported(_))
        ));
        assert!(matches!(
            runtime.detach_dvr_filter(dvr.id.0, filter.id.0),
            Err(HalError::Unsupported(_))
        ));
    }

    fn scrambled_payload_packet(pid: u16) -> [u8; maleicacid_tuner_hal2_common::TS_PACKET_SIZE] {
        let mut packet = [0xffu8; maleicacid_tuner_hal2_common::TS_PACKET_SIZE];
        packet[0] = 0x47;
        packet[1] = ((pid >> 8) as u8) & 0x1f;
        packet[2] = pid as u8;
        packet[3] = 0x90;
        packet[4] = 0x00;
        packet
    }

    fn sample_multi2_key(byte: u8) -> Multi2KeyMaterial {
        let mut system_key = [0u8; 32];
        for (i, value) in system_key.iter_mut().enumerate() {
            *value = byte.wrapping_add(i as u8);
        }
        let mut iv = [0u8; 8];
        for (i, value) in iv.iter_mut().enumerate() {
            *value = 0xa0u8.wrapping_add(byte).wrapping_add(i as u8);
        }
        let mut data_key = [0u8; 8];
        for (i, value) in data_key.iter_mut().enumerate() {
            *value = 0x40u8.wrapping_add(byte).wrapping_add((i * 3) as u8);
        }
        Multi2KeyMaterial::new(system_key, iv, data_key)
    }

    fn encrypted_scrambled_payload_packet(
        pid: u16,
        key_slot: &DescramblerKeySlot,
    ) -> [u8; maleicacid_tuner_hal2_common::TS_PACKET_SIZE] {
        let mut packet = scrambled_payload_packet(pid);
        packet[3] = 0x10;
        packet[4..12].copy_from_slice(&[0x00, 0x00, 0x01, 0xe0, 0x00, 0x03, 0x80, 0x00]);
        let even_key = key_slot
            .key_for(maleicacid_tuner_hal2_descrambler::KeyParity::Even)
            .expect("test key slot must contain an even key");
        multi2_encrypt_payload(&mut packet[4..], even_key);
        packet[3] = 0x90;
        packet
    }

    #[test]
    fn descrambler_key_clear_without_key_keeps_bound_demux_and_pid_claims() {
        let mut runtime = TunerServiceRuntime::new();
        let demux = runtime.allocate_demux_runtime().unwrap();
        let filter = runtime.allocate_filter_runtime(demux.id.0).unwrap();
        runtime
            .register_demux_filter_runtime(
                demux.id.0,
                filter.id.0,
                &configured_pes_filter_request(),
            )
            .unwrap();
        runtime
            .configure_filter_runtime_request(filter.id.0, configured_pes_filter_config(200))
            .unwrap();

        let descrambler = runtime.allocate_descrambler_runtime().unwrap();
        runtime
            .set_descrambler_demux_source(descrambler.id.0, demux.id.0)
            .unwrap();
        runtime
            .add_descrambler_pid_non_null_source(descrambler.id.0, 200, filter.id.0)
            .unwrap();

        runtime
            .set_descrambler_key_token(descrambler.id.0, &[0x00])
            .unwrap();
        let session = runtime
            .registry()
            .descrambler_runtime(descrambler.id)
            .unwrap();
        assert_eq!(session.demux_binding(), Some((demux.id.0, 1)));
        let claim_sets = runtime
            .registry()
            .resolved_descrambler_claims_for_demux(demux.id.0, 1);
        assert_eq!(claim_sets.len(), 1);
        let (claims, key_slot) = claim_sets.into_iter().next().unwrap().into_parts();
        assert!(key_slot.is_none());
        assert_eq!(claims.len(), 1);
        assert!(!session.is_closed());
    }

    #[test]
    fn descrambler_failed_source_call_consumes_the_one_shot_binding_attempt() {
        let mut runtime = TunerServiceRuntime::new();
        let demux = runtime.allocate_demux_runtime().unwrap();
        let descrambler = runtime.allocate_descrambler_runtime().unwrap();

        let first = runtime
            .set_descrambler_demux_source(descrambler.id.0, 99_999)
            .unwrap_err();
        assert!(matches!(first, HalError::InvalidArgument { .. }));
        let second = runtime
            .set_descrambler_demux_source(descrambler.id.0, demux.id.0)
            .unwrap_err();
        assert!(matches!(second, HalError::InvalidState { .. }));
        assert_eq!(
            runtime
                .registry()
                .descrambler_runtime(descrambler.id)
                .unwrap()
                .demux_binding(),
            None
        );
        assert_eq!(
            runtime
                .registry()
                .descrambler_runtime(descrambler.id)
                .unwrap()
                .source_call_failure(),
            Some(crate::descrambler_session::DescramblerSourceCallFailure::InvalidDemuxId)
        );
    }

    #[test]
    fn descrambler_non_void_key_requires_a_live_bound_demux() {
        let mut runtime = TunerServiceRuntime::new();
        let descrambler = runtime.allocate_descrambler_runtime().unwrap();
        let token_bytes = vec![0x40; 8];
        let token = DescramblerKeyToken::try_from_bytes(token_bytes.clone()).unwrap();
        runtime
            .register_descrambler_key_slot(token.clone(), DescramblerKeySlot::empty())
            .unwrap();

        let error = runtime
            .set_descrambler_key_token(descrambler.id.0, &token_bytes)
            .unwrap_err();
        assert!(matches!(error, HalError::InvalidState { .. }));
        assert_eq!(
            runtime
                .registry_mut_for_test()
                .descrambler_key_table_mut()
                .refcount_for_test(&token),
            Some(0)
        );
        assert!(runtime.descrambler_diagnostics().iter().any(|record| {
            descrambler_set_key_diagnostic_matches(
                record,
                descrambler.id.0,
                DescramblerDiagnosticKind::SessionClosed,
            )
        }));
    }

    #[test]
    fn descrambler_clear_key_token_release_failure_quarantines_and_fails_api() {
        let mut runtime = TunerServiceRuntime::new();
        let demux = runtime.allocate_demux_runtime().unwrap();
        let descrambler = runtime.allocate_descrambler_runtime().unwrap();
        let token_bytes = vec![0x41; 8];
        let token = DescramblerKeyToken::try_from_bytes(token_bytes.clone()).unwrap();
        let key_slot = DescramblerKeySlot::empty()
            .try_with_even(sample_multi2_key(8))
            .unwrap();

        runtime
            .register_descrambler_key_slot(token.clone(), key_slot)
            .unwrap();
        runtime
            .set_descrambler_demux_source(descrambler.id.0, demux.id.0)
            .unwrap();
        runtime
            .set_descrambler_key_token(descrambler.id.0, &token_bytes)
            .unwrap();
        runtime
            .registry_mut_for_test()
            .descrambler_key_table_mut()
            .release(&token)
            .unwrap();

        let error = runtime
            .set_descrambler_key_token(descrambler.id.0, &[0x00])
            .unwrap_err();
        assert!(matches!(error, HalError::Internal { .. }));
        let session = runtime
            .registry()
            .descrambler_runtime(descrambler.id)
            .unwrap();
        assert!(!session.has_key());
        assert!(session.is_quarantined());
        assert!(runtime.descrambler_diagnostics().iter().any(|record| {
            descrambler_set_key_diagnostic_matches(
                record,
                descrambler.id.0,
                DescramblerDiagnosticKind::KeyTokenReleaseFailed,
            )
        }));
    }

    #[test]
    fn descrambler_replace_key_release_failure_quarantines_and_fails_api() {
        let mut runtime = TunerServiceRuntime::new();
        let demux = runtime.allocate_demux_runtime().unwrap();
        let descrambler = runtime.allocate_descrambler_runtime().unwrap();
        let old_token_bytes = vec![0x42; 8];
        let old_token = DescramblerKeyToken::try_from_bytes(old_token_bytes.clone()).unwrap();
        let old_key_slot = DescramblerKeySlot::empty()
            .try_with_even(sample_multi2_key(9))
            .unwrap();
        let new_token_bytes = vec![0x43; 8];
        let new_token = DescramblerKeyToken::try_from_bytes(new_token_bytes.clone()).unwrap();
        let new_key_slot = DescramblerKeySlot::empty()
            .try_with_even(sample_multi2_key(10))
            .unwrap();

        runtime
            .register_descrambler_key_slot(old_token.clone(), old_key_slot)
            .unwrap();
        runtime
            .register_descrambler_key_slot(new_token.clone(), new_key_slot)
            .unwrap();
        runtime
            .set_descrambler_demux_source(descrambler.id.0, demux.id.0)
            .unwrap();
        runtime
            .set_descrambler_key_token(descrambler.id.0, &old_token_bytes)
            .unwrap();
        runtime
            .registry_mut_for_test()
            .descrambler_key_table_mut()
            .release(&old_token)
            .unwrap();

        let error = runtime
            .set_descrambler_key_token(descrambler.id.0, &new_token_bytes)
            .unwrap_err();
        assert!(matches!(error, HalError::Internal { .. }));

        let session = runtime
            .registry()
            .descrambler_runtime(descrambler.id)
            .unwrap();
        assert_eq!(session.key_token(), Some(&new_token));
        assert!(session.is_quarantined());
        assert!(runtime.descrambler_diagnostics().iter().any(|record| {
            descrambler_set_key_diagnostic_matches(
                record,
                descrambler.id.0,
                DescramblerDiagnosticKind::KeyTokenReleaseFailed,
            )
        }));
    }

    #[test]
    fn descrambler_add_pid_rejects_source_filter_from_other_demux() {
        let mut runtime = TunerServiceRuntime::new();
        let owner_demux = runtime.allocate_demux_runtime().unwrap();
        let other_demux = runtime.allocate_demux_runtime().unwrap();
        let filter = runtime.allocate_filter_runtime(other_demux.id.0).unwrap();
        runtime
            .register_demux_filter_runtime(
                other_demux.id.0,
                filter.id.0,
                &configured_pes_filter_request(),
            )
            .unwrap();
        runtime
            .configure_filter_runtime_request(filter.id.0, configured_pes_filter_config(200))
            .unwrap();

        let descrambler = runtime.allocate_descrambler_runtime().unwrap();
        runtime
            .set_descrambler_demux_source(descrambler.id.0, owner_demux.id.0)
            .unwrap();
        let err = runtime
            .add_descrambler_pid_non_null_source(descrambler.id.0, 200, filter.id.0)
            .unwrap_err();
        assert!(matches!(
            err,
            maleicacid_tuner_hal2_common::HalError::InvalidArgument { .. }
        ));
        assert!(runtime.descrambler_diagnostics().iter().any(|record| {
            descrambler_pid_claim_with_demux_matches(
                record,
                DescramblerDiagnosticPhase::AddPid,
                descrambler.id.0,
                owner_demux.id.0,
                200,
                filter.id.0,
            )
        }));
    }

    #[test]
    fn descrambler_add_pid_rejects_pid_claimed_by_other_session() {
        let mut runtime = TunerServiceRuntime::new();
        let demux = runtime.allocate_demux_runtime().unwrap();
        let filter = runtime.allocate_filter_runtime(demux.id.0).unwrap();
        runtime
            .register_demux_filter_runtime(
                demux.id.0,
                filter.id.0,
                &configured_pes_filter_request(),
            )
            .unwrap();
        runtime
            .configure_filter_runtime_request(filter.id.0, configured_pes_filter_config(200))
            .unwrap();

        let first = runtime.allocate_descrambler_runtime().unwrap();
        let second = runtime.allocate_descrambler_runtime().unwrap();
        runtime
            .set_descrambler_demux_source(first.id.0, demux.id.0)
            .unwrap();
        runtime
            .set_descrambler_demux_source(second.id.0, demux.id.0)
            .unwrap();
        runtime
            .add_descrambler_pid_non_null_source(first.id.0, 200, filter.id.0)
            .unwrap();

        let err = runtime
            .add_descrambler_pid_non_null_source(second.id.0, 200, filter.id.0)
            .unwrap_err();
        assert!(matches!(
            err,
            maleicacid_tuner_hal2_common::HalError::InvalidState { .. }
        ));
    }

    #[test]
    fn descrambler_set_key_token_records_cas_unavailable_diagnostic() {
        let mut runtime = TunerServiceRuntime::new();
        let demux = runtime.allocate_demux_runtime().unwrap();
        let descrambler = runtime.allocate_descrambler_runtime().unwrap();
        runtime
            .set_descrambler_demux_source(descrambler.id.0, demux.id.0)
            .unwrap();

        let err = runtime
            .set_descrambler_key_token(descrambler.id.0, &[1, 2, 3, 4, 5, 6, 7, 8])
            .unwrap_err();

        assert!(matches!(
            err,
            maleicacid_tuner_hal2_common::HalError::InvalidState { .. }
        ));
        assert!(runtime.descrambler_diagnostics().iter().any(|record| {
            descrambler_set_key_diagnostic_matches(
                record,
                descrambler.id.0,
                DescramblerDiagnosticKind::CasTokenProducerUnavailable,
            )
        }));
    }

    #[test]
    fn descrambler_set_key_token_rejects_empty_and_invalid_length_tokens() {
        let mut runtime = TunerServiceRuntime::new();
        let descrambler = runtime.allocate_descrambler_runtime().unwrap();

        let empty_err = runtime
            .set_descrambler_key_token(descrambler.id.0, &[])
            .unwrap_err();
        assert!(matches!(
            empty_err,
            maleicacid_tuner_hal2_common::HalError::InvalidArgument { .. }
        ));
        assert!(runtime.descrambler_diagnostics().iter().any(|record| {
            descrambler_set_key_diagnostic_matches(
                record,
                descrambler.id.0,
                DescramblerDiagnosticKind::KeyTokenEmpty,
            )
        }));

        let invalid_len_err = runtime
            .set_descrambler_key_token(
                descrambler.id.0,
                &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17],
            )
            .unwrap_err();
        assert!(matches!(
            invalid_len_err,
            maleicacid_tuner_hal2_common::HalError::InvalidArgument { .. }
        ));
        assert!(runtime.descrambler_diagnostics().iter().any(|record| {
            descrambler_set_key_diagnostic_matches(
                record,
                descrambler.id.0,
                DescramblerDiagnosticKind::KeyTokenInvalidLength,
            )
        }));
    }

    #[test]
    fn descrambler_set_key_token_rejects_expired_tokens() {
        let mut runtime = TunerServiceRuntime::new();
        let demux = runtime.allocate_demux_runtime().unwrap();
        let descrambler = runtime.allocate_descrambler_runtime().unwrap();
        let token_bytes = vec![9, 9, 9, 9, 9, 9, 9, 9];
        let token = DescramblerKeyToken::try_from_bytes(token_bytes.clone()).unwrap();
        let key_slot = DescramblerKeySlot::empty()
            .try_with_even(sample_multi2_key(3))
            .unwrap();

        runtime
            .register_descrambler_key_slot(token.clone(), key_slot)
            .unwrap();
        runtime
            .set_descrambler_demux_source(descrambler.id.0, demux.id.0)
            .unwrap();
        runtime
            .registry_mut_for_test()
            .descrambler_key_table_mut()
            .expire_test_key(&token);

        let err = runtime
            .set_descrambler_key_token(descrambler.id.0, &token_bytes)
            .unwrap_err();
        assert!(matches!(
            err,
            maleicacid_tuner_hal2_common::HalError::InvalidState { .. }
        ));
        assert!(runtime.descrambler_diagnostics().iter().any(|record| {
            descrambler_set_key_diagnostic_matches(
                record,
                descrambler.id.0,
                DescramblerDiagnosticKind::KeyTokenExpired,
            )
        }));
    }

    #[test]
    fn descrambler_packet_policy_records_keyless_scrambled_diagnostics() {
        let mut runtime = TunerServiceRuntime::new();
        runtime.boot_from_probe_results([available(
            1_000_000,
            FrontendBackendKind::Px4CharDevice,
            FrontendSystem::IsdbT,
            "/dev/px4video0",
            None,
        )]);
        let demux = runtime.allocate_demux_runtime().unwrap();
        runtime
            .set_demux_frontend_data_source(demux.id.0, 1_000_000)
            .unwrap();
        let filter = runtime.allocate_filter_runtime(demux.id.0).unwrap();
        runtime
            .register_demux_filter_runtime(
                demux.id.0,
                filter.id.0,
                &configured_pes_filter_request(),
            )
            .unwrap();
        runtime
            .configure_filter_runtime_request(filter.id.0, configured_pes_filter_config(200))
            .unwrap();
        let descrambler = runtime.allocate_descrambler_runtime().unwrap();
        runtime
            .set_descrambler_demux_source(descrambler.id.0, demux.id.0)
            .unwrap();
        runtime
            .add_descrambler_pid_non_null_source(descrambler.id.0, 200, filter.id.0)
            .unwrap();

        runtime
            .push_frontend_ts_packet_to_bound_demuxes(1_000_000, &scrambled_payload_packet(200))
            .unwrap();

        assert!(runtime.descrambler_diagnostics().iter().any(|record| {
            descrambler_packet_policy_matches(
                record,
                demux.id.0,
                200,
                DescramblerDiagnosticKind::PacketScrambledWithoutKey,
            )
        }));
        assert!(runtime.descrambler_diagnostics().iter().any(|record| {
            descrambler_packet_policy_matches(
                record,
                demux.id.0,
                200,
                DescramblerDiagnosticKind::PacketAssemblySuppressed,
            )
        }));
    }

    #[test]
    fn descrambler_packet_path_records_source_filter_generation_mismatch() {
        let mut runtime = TunerServiceRuntime::new();
        runtime.boot_from_probe_results([available(
            1_000_000,
            FrontendBackendKind::Px4CharDevice,
            FrontendSystem::IsdbT,
            "/dev/px4video0",
            None,
        )]);
        let demux = runtime.allocate_demux_runtime().unwrap();
        runtime
            .set_demux_frontend_data_source(demux.id.0, 1_000_000)
            .unwrap();
        let filter = runtime.allocate_filter_runtime(demux.id.0).unwrap();
        runtime
            .register_demux_filter_runtime(
                demux.id.0,
                filter.id.0,
                &configured_pes_filter_request(),
            )
            .unwrap();
        runtime
            .configure_filter_runtime_request(filter.id.0, configured_pes_filter_config(200))
            .unwrap();
        let descrambler = runtime.allocate_descrambler_runtime().unwrap();
        runtime
            .set_descrambler_demux_source(descrambler.id.0, demux.id.0)
            .unwrap();
        runtime
            .add_descrambler_pid_non_null_source(descrambler.id.0, 200, filter.id.0)
            .unwrap();
        runtime
            .configure_filter_runtime_request(filter.id.0, configured_pes_filter_config(200))
            .unwrap();

        runtime
            .push_frontend_ts_packet_to_bound_demuxes(1_000_000, &scrambled_payload_packet(200))
            .unwrap();

        assert!(runtime.descrambler_diagnostics().iter().any(|record| {
            descrambler_source_filter_validation_matches(
                record,
                demux.id.0,
                200,
                filter.id.0,
                DescramblerDiagnosticKind::PacketSourceFilterGenerationMismatch,
            )
        }));
    }

    #[test]
    fn descrambler_packet_path_records_source_filter_validation_failure() {
        let mut runtime = TunerServiceRuntime::new();
        runtime.boot_from_probe_results([available(
            1_000_000,
            FrontendBackendKind::Px4CharDevice,
            FrontendSystem::IsdbT,
            "/dev/px4video0",
            None,
        )]);
        let demux = runtime.allocate_demux_runtime().unwrap();
        runtime
            .set_demux_frontend_data_source(demux.id.0, 1_000_000)
            .unwrap();
        let filter = runtime.allocate_filter_runtime(demux.id.0).unwrap();
        runtime
            .register_demux_filter_runtime(
                demux.id.0,
                filter.id.0,
                &configured_pes_filter_request(),
            )
            .unwrap();
        runtime
            .configure_filter_runtime_request(filter.id.0, configured_pes_filter_config(200))
            .unwrap();
        let descrambler = runtime.allocate_descrambler_runtime().unwrap();
        runtime
            .set_descrambler_demux_source(descrambler.id.0, demux.id.0)
            .unwrap();
        runtime
            .add_descrambler_pid_non_null_source(descrambler.id.0, 200, filter.id.0)
            .unwrap();
        runtime
            .configure_filter_runtime_request(filter.id.0, configured_pes_filter_config(201))
            .unwrap();

        runtime
            .push_frontend_ts_packet_to_bound_demuxes(1_000_000, &scrambled_payload_packet(200))
            .unwrap();

        assert!(runtime.descrambler_diagnostics().iter().any(|record| {
            descrambler_source_filter_validation_matches(
                record,
                demux.id.0,
                200,
                filter.id.0,
                DescramblerDiagnosticKind::PacketSourceFilterInvalid,
            )
        }));
    }

    #[test]
    fn descrambler_success_feeds_descrambled_packet_to_demux_pipeline() {
        let mut runtime = TunerServiceRuntime::new();
        runtime.boot_from_probe_results([available(
            1_000_000,
            FrontendBackendKind::Px4CharDevice,
            FrontendSystem::IsdbT,
            "/dev/px4video0",
            None,
        )]);
        let demux = runtime.allocate_demux_runtime().unwrap();
        runtime
            .set_demux_frontend_data_source(demux.id.0, 1_000_000)
            .unwrap();
        let filter = runtime.allocate_filter_runtime(demux.id.0).unwrap();
        runtime
            .register_demux_filter_runtime(
                demux.id.0,
                filter.id.0,
                &configured_pes_filter_request(),
            )
            .unwrap();
        runtime
            .configure_filter_runtime_request(filter.id.0, configured_pes_filter_config(200))
            .unwrap();
        runtime.start_filter_runtime(filter.id.0).unwrap();

        let key_slot = DescramblerKeySlot::empty()
            .try_with_even(sample_multi2_key(1))
            .unwrap();
        let token_bytes = vec![0x10; 8];
        let token = DescramblerKeyToken::try_from_bytes(token_bytes.clone()).unwrap();
        runtime
            .register_descrambler_key_slot(token.clone(), key_slot.clone())
            .unwrap();
        let descrambler = runtime.allocate_descrambler_runtime().unwrap();
        runtime
            .set_descrambler_demux_source(descrambler.id.0, demux.id.0)
            .unwrap();
        runtime
            .add_descrambler_pid_non_null_source(descrambler.id.0, 200, filter.id.0)
            .unwrap();
        runtime
            .set_descrambler_key_token(descrambler.id.0, &token_bytes)
            .unwrap();

        let reports = runtime
            .push_frontend_ts_packet_to_bound_demuxes(
                1_000_000,
                &encrypted_scrambled_payload_packet(200, &key_slot),
            )
            .unwrap();
        let report = reports.first().expect("bound demux report exists");

        assert!(report
            .delivery_actions
            .contains(&PipelineDeliveryAction::PesPayload {
                filter_id: filter.id.0
            }));
        assert!(!report
            .assembly_suppression_reasons
            .contains(&PipelineAssemblySuppressionReason::KeylessScrambledWithoutDescrambler));
        assert!(runtime.descrambler_diagnostics().iter().any(|record| {
            descrambler_packet_policy_matches(
                record,
                demux.id.0,
                200,
                DescramblerDiagnosticKind::PacketDescrambled,
            )
        }));
    }

    #[test]
    fn demux_owner_loss_cleans_bound_descrambler_session() {
        let mut runtime = TunerServiceRuntime::new();
        let demux = runtime.allocate_demux_runtime().unwrap();
        let filter = runtime.allocate_filter_runtime(demux.id.0).unwrap();
        runtime
            .register_demux_filter_runtime(
                demux.id.0,
                filter.id.0,
                &configured_pes_filter_request(),
            )
            .unwrap();
        runtime
            .configure_filter_runtime_request(filter.id.0, configured_pes_filter_config(200))
            .unwrap();
        let descrambler = runtime.allocate_descrambler_runtime().unwrap();
        runtime
            .set_descrambler_demux_source(descrambler.id.0, demux.id.0)
            .unwrap();
        runtime
            .add_descrambler_pid_non_null_source(descrambler.id.0, 200, filter.id.0)
            .unwrap();

        assert!(runtime
            .unregister_demux_runtime(demux.id.0)
            .unwrap()
            .is_some());

        let session = runtime
            .registry()
            .descrambler_runtime(descrambler.id)
            .unwrap();
        assert!(session.is_closed());
        assert_eq!(session.demux_binding(), None);
        assert!(!session.has_key());
    }

    #[test]
    fn descrambler_remove_pid_rejects_stale_source_filter_generation() {
        let mut runtime = TunerServiceRuntime::new();
        let demux = runtime.allocate_demux_runtime().unwrap();
        let filter = runtime.allocate_filter_runtime(demux.id.0).unwrap();
        runtime
            .register_demux_filter_runtime(
                demux.id.0,
                filter.id.0,
                &configured_pes_filter_request(),
            )
            .unwrap();
        runtime
            .configure_filter_runtime_request(filter.id.0, configured_pes_filter_config(200))
            .unwrap();

        let descrambler = runtime.allocate_descrambler_runtime().unwrap();
        runtime
            .set_descrambler_demux_source(descrambler.id.0, demux.id.0)
            .unwrap();
        runtime
            .add_descrambler_pid_non_null_source(descrambler.id.0, 200, filter.id.0)
            .unwrap();
        runtime
            .configure_filter_runtime_request(filter.id.0, configured_pes_filter_config(200))
            .unwrap();

        let err = runtime
            .remove_descrambler_pid_non_null_source(descrambler.id.0, 200, filter.id.0)
            .unwrap_err();
        assert!(matches!(
            err,
            maleicacid_tuner_hal2_common::HalError::InvalidState { .. }
        ));
    }
}

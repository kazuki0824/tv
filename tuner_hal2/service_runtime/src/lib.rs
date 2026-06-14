pub mod boot;
pub mod callback_registry;
pub mod capability_profile;
pub mod command_dispatch;
pub mod diagnostics;
pub mod dispatch;
pub mod frontend_request_txn;
pub mod frontend_worker_txn;
pub mod lnb_apply_txn;
pub mod lnb_backend_adapter;
pub mod lnb_lifecycle_txn;
pub mod object_table;
pub mod registry;
pub mod runtime_handlers;
pub mod runtime_result;
pub mod transaction_registry;

pub use boot::{
    start_frontend_demux_live_pump_from_reader, FrontendDemuxPacketSink, FrontendProbeOutcome,
    ServiceBootOutcome, TunerServiceRuntime,
};
pub use callback_registry::{
    CallbackHealthState, RuntimeCallbackRegistration, RuntimeCallbackRegistry,
};
pub use capability_profile::{
    configure_ip_cid_result, configure_monitor_event_result, failure_domain, feature_declared,
    hal_generates_japanese_scan_plan, open_failed, scan_candidate_owner, transport_declared,
    ProfileFeature, RuntimeFailureDomain, ScanCandidateOwner, TransportCapability,
};
pub use command_dispatch::{
    RuntimeCommandDispatchError, RuntimeCommandDispatchPlan, RuntimeCommandDispatcher,
};
pub use diagnostics::{
    CapabilitySuppressionReason, DescramblerDiagnosticKind, DescramblerDiagnosticPhase,
    DescramblerDiagnosticRecord, StartupDiagnosticKind, StartupDiagnosticPhase,
    StartupDiagnosticRecord,
};
pub use dispatch::{dispatch_target_for, ServiceRuntimeDispatchTarget};
pub use object_table::{
    RuntimeObjectEntry, RuntimeObjectLifecycle, RuntimeObjectTable, RuntimeObjectTableError,
    RuntimeOwnerRelation,
};
pub use registry::{
    DemuxRegistryEntry, DemuxRuntimeId, FrontendRegistryEntry, FrontendRuntimeId, LnbRegistryEntry,
    LnbRegistryProfile, LnbRuntimeId, RegistryCommitError, RuntimeRegistry, RuntimeRegistryKind,
};
pub use runtime_handlers::{
    all_runtime_transactions_are_classified, runtime_handler_coverage_for, RuntimeDispatchHandler,
};
pub use runtime_result::{
    RuntimeHandlerCoverage, RuntimeHandlerError, RuntimeHandlerResult, RuntimeHandlerSuccess,
};
pub use transaction_registry::{
    every_aidl_transaction_has_runtime_spec, runtime_transaction_specs, transaction_spec_for,
    RuntimeDispatchTarget, RuntimeTransactionSpec, RUNTIME_TRANSACTION_SPECS,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceState {
    Booting,
    Ready,
    Degraded,
    Closing,
    Failed,
}

#[cfg(test)]
mod tests {
    use super::*;
    use maleicacid_tuner_hal2_common::{FrontendBackendKind, FrontendSystem};
    use maleicacid_tuner_hal2_demux::{
        FilterConfig, FilterConfigKind, FilterOpenType, OpenFilterRequest, PesSettings,
    };
    use maleicacid_tuner_hal2_domain_request::{RuntimeTransactionName, AIDL_TRANSACTION_TABLE};
    use std::path::PathBuf;

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
        FrontendProbeOutcome::Available {
            id: FrontendRuntimeId(id),
            backend,
            system,
            path: path(path_name),
            lnb_profile,
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
            runtime.diagnostics()[0].kind,
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
        assert!(runtime.diagnostics().is_empty());
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
            .prepare_frontend_worker_generation(
                1_000_000,
                maleicacid_tuner_hal2_device::FrontendWorkerKind::Tune,
            )
            .unwrap();
        runtime
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
            frontend.state(),
            maleicacid_tuner_hal2_device::FrontendRuntimeState::Tuning { generation }
        );
        assert!(frontend.live_reader_descriptor().is_some());

        runtime
            .clear_frontend_live_reader_descriptor_and_idle(1_000_000)
            .unwrap();
        let frontend = runtime
            .registry()
            .frontend_runtime(FrontendRuntimeId(1_000_000))
            .unwrap();
        assert_eq!(
            frontend.state(),
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
            .prepare_frontend_worker_generation(
                1_000_000,
                maleicacid_tuner_hal2_device::FrontendWorkerKind::Scan,
            )
            .unwrap();
        runtime
            .install_frontend_live_reader_descriptor_for_generation(
                1_000_000,
                maleicacid_tuner_hal2_device::FrontendWorkerKind::Scan,
                generation,
            )
            .unwrap();
        runtime
            .record_frontend_scan_cancelled(
                1_000_000,
                generation,
                maleicacid_tuner_hal2_device::FrontendWorkerCancelReason::StopRequested,
            )
            .unwrap();
        let events = runtime.frontend_terminal_events(1_000_000).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].generation, generation);
        assert_eq!(
            events[0].kind,
            maleicacid_tuner_hal2_device::FrontendTerminalEventKind::ScanCancelled,
        );

        runtime
            .clear_frontend_live_reader_descriptor_and_idle(1_000_000)
            .unwrap();
        let frontend = runtime
            .registry()
            .frontend_runtime(FrontendRuntimeId(1_000_000))
            .unwrap();
        assert_eq!(
            frontend.state(),
            maleicacid_tuner_hal2_device::FrontendRuntimeState::Idle
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
            .prepare_frontend_worker_generation(
                2_000_000,
                maleicacid_tuner_hal2_device::FrontendWorkerKind::Tune,
            )
            .unwrap();
        runtime
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
        assert_eq!(runtime.registry().lnb_count(), 1);
        assert_eq!(runtime.lnb_ids(), vec![1_020_001]);
        assert!(runtime
            .lnb_id_by_name("maleicacid-lnb-px4-px4video0-unit-0")
            .is_some());
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
        let lnb = runtime.lnb_for_frontend_id(2_000_001).unwrap();
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
            .any(|record| record.kind == StartupDiagnosticKind::DuplicateFrontendId));
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
        }

        let mut packet = [0xffu8; maleicacid_tuner_hal2_common::TS_PACKET_SIZE];
        packet[0] = 0x47;
        packet[1] = 0x40;
        packet[2] = 0x00;
        packet[3] = 0x10;
        let reader = Box::new(std::io::Cursor::new(packet.to_vec()));
        let owner = crate::start_frontend_demux_live_pump_from_reader(
            Arc::clone(&runtime),
            1_000_000,
            reader,
        )
        .unwrap();
        let report = owner.join_after_stop().unwrap();
        assert_eq!(report.packets_delivered, 1);
    }

    #[test]
    fn every_binder_adapter_transaction_has_service_runtime_dispatch_target() {
        for plan in AIDL_TRANSACTION_TABLE {
            assert!(dispatch_target_for(plan.transaction).is_some());
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
            .find(|plan| plan.transaction == RuntimeTransactionName::FrontendTuneTxnApply)
            .expect("frontend tune transaction exists");
        let plan =
            RuntimeCommandDispatcher::plan(command_plan, None).expect("dispatch target exists");
        assert_eq!(
            plan.command_plan.transaction,
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

        let mut table = RuntimeObjectTable::default();
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

        let mut table = RuntimeObjectTable::default();
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

        let mut table = RuntimeObjectTable::default();
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
    fn all_runtime_transactions_have_handler_coverage_classification() {
        assert!(all_runtime_transactions_are_classified());
        for plan in AIDL_TRANSACTION_TABLE {
            let coverage = runtime_handler_coverage_for(plan.transaction);
            assert!(matches!(
                coverage,
                RuntimeHandlerCoverage::Connected
                    | RuntimeHandlerCoverage::NotConnected
                    | RuntimeHandlerCoverage::UnsupportedByDesign
            ));
        }
    }

    #[test]
    fn unconnected_handler_is_typed_error_not_success() {
        use maleicacid_tuner_hal2_domain_request::{
            AidlObjectGeneration, AidlObjectId, AidlObjectKind,
        };
        use maleicacid_tuner_hal2_resource_ledger::{LedgerGeneration, LedgerId};

        let command_plan = AIDL_TRANSACTION_TABLE
            .iter()
            .copied()
            .find(|plan| plan.transaction == RuntimeTransactionName::FrontendStopScanTxn)
            .expect("frontend stop scan transaction exists");
        let dispatch_plan =
            RuntimeCommandDispatcher::plan(command_plan, None).expect("dispatch target exists");
        let mut table = RuntimeObjectTable::default();
        table
            .insert(RuntimeObjectEntry {
                object_kind: AidlObjectKind::Frontend,
                object_id: AidlObjectId(1),
                generation: AidlObjectGeneration(1),
                ledger_id: LedgerId(1),
                ledger_generation: LedgerGeneration(1),
                owner: RuntimeOwnerRelation::Root,
                lifecycle: RuntimeObjectLifecycle::Live,
            })
            .expect("insert succeeds");
        let err = RuntimeDispatchHandler::dispatch(
            &dispatch_plan,
            &table,
            AidlObjectId(1),
            AidlObjectGeneration(1),
        )
        .expect_err("handler classification does not fake success for unconnected transactions");
        assert!(matches!(
            err,
            RuntimeHandlerError::NotConnected {
                transaction: RuntimeTransactionName::FrontendStopScanTxn,
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

        let mut table = RuntimeObjectTable::default();
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
            .begin_close(
                AidlObjectId(44),
                AidlObjectGeneration(3),
                CleanupStep::StopWorker,
            )
            .expect("begin close succeeds");
        table
            .commit_close(AidlObjectId(44), AidlObjectGeneration(3))
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

        let mut table = RuntimeObjectTable::default();
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

        let mut table = RuntimeObjectTable::default();
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

        let mut table = RuntimeObjectTable::default();
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
            .begin_close(
                AidlObjectId(55),
                AidlObjectGeneration(7),
                CleanupStep::StopWorker,
            )
            .expect("begin close succeeds");
        table
            .mark_cleanup_failed(
                AidlObjectId(55),
                AidlObjectGeneration(7),
                CleanupStep::UnregisterRuntime,
            )
            .expect("mark failed succeeds");
        table
            .begin_close(
                AidlObjectId(55),
                AidlObjectGeneration(7),
                CleanupStep::UnregisterRuntime,
            )
            .expect("cleanup failed close can be retried");
        table
            .commit_close(AidlObjectId(55), AidlObjectGeneration(7))
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
                stream_id: 0,
                raw: false,
            }),
        }
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
            .unwrap()
            .session();
        assert_eq!(session.demux_id(), Some(demux.id.0));
        assert_eq!(session.demux_generation(), Some(1));
        assert_eq!(session.key_slot(), None);
        assert_eq!(session.pid_claims().len(), 1);
        assert!(!session.is_closed());
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
            record.kind == DescramblerDiagnosticKind::PidClaimRejected
                && record.phase == DescramblerDiagnosticPhase::AddPid
                && record.descrambler_id == Some(descrambler.id.0)
                && record.demux_id == Some(owner_demux.id.0)
                && record.pid == Some(200)
                && record.filter_id == Some(filter.id.0)
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
        let descrambler = runtime.allocate_descrambler_runtime().unwrap();

        let err = runtime
            .set_descrambler_key_token(descrambler.id.0, &[1, 2, 3, 4, 5, 6, 7, 8])
            .unwrap_err();

        assert!(matches!(
            err,
            maleicacid_tuner_hal2_common::HalError::InvalidState { .. }
        ));
        assert!(runtime.descrambler_diagnostics().iter().any(|record| {
            record.kind == DescramblerDiagnosticKind::CasTokenProducerUnavailable
                && record.phase == DescramblerDiagnosticPhase::SetKeyToken
                && record.descrambler_id == Some(descrambler.id.0)
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
            record.kind == DescramblerDiagnosticKind::PacketScrambledWithoutKey
                && record.phase == DescramblerDiagnosticPhase::PacketPipeline
                && record.demux_id == Some(demux.id.0)
                && record.pid == Some(200)
        }));
        assert!(runtime.descrambler_diagnostics().iter().any(|record| {
            record.kind == DescramblerDiagnosticKind::PacketAssemblySuppressed
                && record.phase == DescramblerDiagnosticPhase::PacketPipeline
                && record.demux_id == Some(demux.id.0)
                && record.pid == Some(200)
        }));
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

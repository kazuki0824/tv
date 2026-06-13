pub mod frontend;
pub mod demux;
pub mod filter;
pub mod dvr;
pub mod descrambler;
pub mod lnb;
pub mod status;
pub mod aidl_method;
pub mod domain_request;

pub use status::{AidlFailureSource, AidlStatusMapper, ApiStatusPrecedence, DomainResult, StatusPrecedenceStep, TunerStatusCode};
pub use aidl_method::{AidlInputField, AidlInputSnapshot, AidlMethodAdapter, AidlMethodCall, AidlMethodPlan};
pub use domain_request::domain_request_from_snapshot;
pub use maleicacid_tuner_hal2_domain_request::{AidlApi, AidlDomainRequest, AidlObjectGeneration, AidlObjectId, AidlObjectKind, CommandPlan, DomainProfileSupport, DomainRequestField, DomainValueValidation, RuntimeExecutableRequest, RuntimeTransactionName, AIDL_TRANSACTION_TABLE};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DomainCommand {
    UnsupportedPublicApi { object: AidlObjectKind, api: AidlApi, request: Option<AidlDomainRequest> },
    Frontend(frontend::FrontendCommand),
    Demux(demux::DemuxCommand),
    Filter(filter::FilterCommand),
    Dvr(dvr::DvrCommand),
    Descrambler(descrambler::DescramblerCommand),
    Lnb(lnb::LnbCommand),
}

impl DomainCommand {
    pub fn runtime_executable_request(&self) -> Option<RuntimeExecutableRequest> {
        match self {
            DomainCommand::UnsupportedPublicApi { request: Some(request), .. } => Some(request.clone().into_runtime_executable_request()),
            DomainCommand::UnsupportedPublicApi { request: None, .. } => None,
            DomainCommand::Frontend(frontend::FrontendCommand::SetCallback(request)) => {
                Some(request.clone().into_runtime_executable_request())
            }
            DomainCommand::Demux(demux::DemuxCommand::SetFrontendDataSource(request)
                | demux::DemuxCommand::OpenFilter(request)
                | demux::DemuxCommand::OpenDvr(request)) => {
                Some(request.clone().into_runtime_executable_request())
            }
            DomainCommand::Filter(filter::FilterCommand::Configure(request)
                | filter::FilterCommand::ConfigureAvStreamType(request)
                | filter::FilterCommand::ReleaseAvHandle(request)
                | filter::FilterCommand::SetDataSource(request)
                | filter::FilterCommand::SetDelayHint(request)) => {
                Some(request.clone().into_runtime_executable_request())
            }
            DomainCommand::Dvr(dvr::DvrCommand::Configure(request)
                | dvr::DvrCommand::AttachFilter(request)
                | dvr::DvrCommand::DetachFilter(request)) => {
                Some(request.clone().into_runtime_executable_request())
            }
            DomainCommand::Lnb(lnb::LnbCommand::SetCallback(request)
                | lnb::LnbCommand::SetVoltage(request)
                | lnb::LnbCommand::SetTone(request)
                | lnb::LnbCommand::SetSatellitePosition(request)) => {
                Some(request.clone().into_runtime_executable_request())
            }
            DomainCommand::Frontend(_)
            | DomainCommand::Demux(demux::DemuxCommand::Close)
            | DomainCommand::Filter(filter::FilterCommand::GetQueueDesc
                | filter::FilterCommand::GetId
                | filter::FilterCommand::GetId64Bit
                | filter::FilterCommand::GetAvSharedHandle
                | filter::FilterCommand::Start
                | filter::FilterCommand::Stop
                | filter::FilterCommand::Flush
                | filter::FilterCommand::Close)
            | DomainCommand::Dvr(dvr::DvrCommand::GetQueueDesc
                | dvr::DvrCommand::Start
                | dvr::DvrCommand::Stop
                | dvr::DvrCommand::Flush
                | dvr::DvrCommand::Close
                | dvr::DvrCommand::SetStatusCheckIntervalHint(_))
            | DomainCommand::Descrambler(_)
            | DomainCommand::Lnb(lnb::LnbCommand::SendDiseqc(_) | lnb::LnbCommand::Close) => None,
        }
    }

    pub fn plan(&self) -> CommandPlan {
        match self {
            DomainCommand::UnsupportedPublicApi { object, api, .. } => CommandPlan {
                object: *object,
                api: *api,
                transaction: match object {
                    AidlObjectKind::Tuner => RuntimeTransactionName::TunerUnsupportedPublicApiTxn,
                    AidlObjectKind::Frontend => RuntimeTransactionName::FrontendUnsupportedPublicApiTxn,
                    AidlObjectKind::Demux => RuntimeTransactionName::DemuxUnsupportedPublicApiTxn,
                    _ => RuntimeTransactionName::TunerUnsupportedPublicApiTxn,
                },
            },
            DomainCommand::Frontend(command) => command.plan(),
            DomainCommand::Demux(command) => command.plan(),
            DomainCommand::Filter(command) => command.plan(),
            DomainCommand::Dvr(command) => command.plan(),
            DomainCommand::Descrambler(command) => command.plan(),
            DomainCommand::Lnb(command) => command.plan(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use maleicacid_tuner_hal2_common::{FrontendStreamIdKind, FrontendSystem, FrontendTuneRequest};

    fn request() -> FrontendTuneRequest {
        FrontendTuneRequest {
            system: FrontendSystem::IsdbT,
            frequency: 473_142_857,
            end_frequency: None,
            stream_id: None,
            stream_id_kind: Some(FrontendStreamIdKind::AbsoluteStreamId),
            bandwidth_hz: Some(6_000_000),
            symbol_rate: None,
        }
    }

    #[test]
    fn frontend_tune_command_maps_to_frontend_tune_transaction() {
        let command = DomainCommand::Frontend(frontend::FrontendCommand::Tune(request()));
        assert_eq!(command.plan().transaction, RuntimeTransactionName::FrontendTuneTxnApply);
    }

    #[test]
    fn transaction_table_covers_lnb_close() {
        assert!(AIDL_TRANSACTION_TABLE.contains(&CommandPlan {
            object: AidlObjectKind::Lnb,
            api: AidlApi::LnbClose,
            transaction: RuntimeTransactionName::LnbLifecycleTxnClose,
        }));
    }

    #[test]
    fn aidl_method_adapter_creates_domain_command_without_string_transaction() {
        let plan = aidl_method::AidlMethodAdapter::frontend_tune(request());
        assert!(matches!(plan.command, DomainCommand::Frontend(frontend::FrontendCommand::Tune(_))));
        assert_eq!(plan.command_plan.transaction, RuntimeTransactionName::FrontendTuneTxnApply);
    }

    #[test]
    fn all_aidl_method_kinds_have_command_plan_entries() {
        for method in aidl_method::all_aidl_method_kinds_for_coverage(request()) {
            let plan = aidl_method::AidlMethodAdapter::plan(method);
            assert!(AIDL_TRANSACTION_TABLE.contains(&plan.command_plan));
        }
    }
    #[test]
    fn filter_configure_command_carries_domain_request_not_snapshot_only() {
        let snapshot = aidl_method::AidlInputSnapshot::from_fields(
            "DemuxFilterSettings",
            vec![aidl_method::AidlInputField::new("top_variant", "ts"), aidl_method::AidlInputField::new("ts.tpid", "256")],
        );
        let command = aidl_method::AidlMethodCall::FilterConfigure(snapshot).into_domain_command();
        match command {
            DomainCommand::Filter(filter::FilterCommand::Configure(request)) => {
                assert_eq!(request.profile_support(), DomainProfileSupport::Supported);
                assert!(request.validate_supported_values().is_ok());
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn filter_configure_unsupported_variant_is_domain_request_unavailable_profile() {
        let snapshot = aidl_method::AidlInputSnapshot::from_fields(
            "DemuxFilterSettings",
            vec![aidl_method::AidlInputField::new("top_variant", "alp")],
        );
        let command = aidl_method::AidlMethodCall::FilterConfigure(snapshot).into_domain_command();
        match command {
            DomainCommand::Filter(filter::FilterCommand::Configure(request)) => {
                assert_eq!(request.profile_support(), DomainProfileSupport::UnsupportedRecordThenUnavailable);
            }
            _ => panic!("unexpected command"),
        }
    }

}

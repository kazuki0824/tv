pub mod aidl_filter_config;
pub mod aidl_frontend_settings;
pub mod aidl_method;
pub mod demux;
pub mod descrambler;
pub mod dvr;
pub mod filter;
pub mod frontend;
pub mod lnb;
pub mod status;

pub use aidl_frontend_settings::{
    aidl_frontend_settings_to_request, aidl_scan_type_to_mode,
};
pub use aidl_filter_config::{
    build_filter_summary_for_open_type, build_open_filter_request, build_section_condition,
    build_section_condition_kind, filter_main_type_supported, filter_open_type,
    normalize_pes_stream_id, validate_record_index_settings, validate_ts_pid,
};
pub use aidl_method::{
    build_dvr_configure_request, build_dvr_open_request, build_filter_av_stream_type_request,
    build_filter_delay_hint_request, build_lnb_satellite_position_request, build_lnb_tone_request,
    build_lnb_voltage_request, AidlMethodAdapter, AidlMethodCall, AidlMethodPlan,
};
pub use maleicacid_tuner_hal2_domain_request::{
    AidlApi, AidlDomainRequest, AidlObjectGeneration, AidlObjectId, AidlObjectKind, CommandPlan,
    DemuxSetFrontendDataSourceRequest, DomainProfileSupport, DvrConfigureKind, DvrConfigureRequest,
    DvrFilterLinkRequest, DvrOpenKind, FilterAvStreamKind, FilterAvStreamTypeRequest,
    FilterDelayHintKind, FilterDelayHintRequest, FilterReleaseAvHandleRequest,
    FilterSetDataSourceRequest, LnbSetSatellitePositionRequest, LnbToneRequest, LnbVoltageRequest,
    OpenDvrRequest, RuntimeExecutableRequest, RuntimeTransactionName, AIDL_TRANSACTION_TABLE,
};
pub use status::{
    AidlFailureSource, AidlStatusMapper, ApiStatusPrecedence, DomainResult, StatusPrecedenceStep,
    TunerStatusCode,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DomainCommand {
    PublicApi {
        object: AidlObjectKind,
        api: AidlApi,
    },
    UnsupportedPublicApi {
        object: AidlObjectKind,
        api: AidlApi,
        request: Option<AidlDomainRequest>,
    },
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
            DomainCommand::UnsupportedPublicApi {
                request: Some(request),
                ..
            } => Some(request.clone()),
            DomainCommand::PublicApi { .. }
            | DomainCommand::UnsupportedPublicApi { request: None, .. } => None,
            DomainCommand::Frontend(frontend::FrontendCommand::SetCallback(request)) => {
                Some(request.clone())
            }
            DomainCommand::Demux(
                demux::DemuxCommand::SetFrontendDataSource(request)
                | demux::DemuxCommand::OpenFilter(request)
                | demux::DemuxCommand::OpenDvr(request),
            ) => Some(request.clone()),
            DomainCommand::Filter(
                filter::FilterCommand::Configure(request)
                | filter::FilterCommand::ConfigureAvStreamType(request)
                | filter::FilterCommand::ReleaseAvHandle(request)
                | filter::FilterCommand::SetDataSource(request)
                | filter::FilterCommand::SetDelayHint(request),
            ) => Some(request.clone()),
            DomainCommand::Dvr(
                dvr::DvrCommand::Configure(request)
                | dvr::DvrCommand::AttachFilter(request)
                | dvr::DvrCommand::DetachFilter(request),
            ) => Some(request.clone()),
            DomainCommand::Lnb(
                lnb::LnbCommand::SetCallback(request)
                | lnb::LnbCommand::SetVoltage(request)
                | lnb::LnbCommand::SetTone(request)
                | lnb::LnbCommand::SetSatellitePosition(request),
            ) => Some(request.clone()),
            DomainCommand::Frontend(_)
            | DomainCommand::Demux(demux::DemuxCommand::Close)
            | DomainCommand::Filter(
                filter::FilterCommand::GetQueueDesc
                | filter::FilterCommand::GetId
                | filter::FilterCommand::GetId64Bit
                | filter::FilterCommand::GetAvSharedHandle
                | filter::FilterCommand::Start
                | filter::FilterCommand::Stop
                | filter::FilterCommand::Flush
                | filter::FilterCommand::Close,
            )
            | DomainCommand::Dvr(
                dvr::DvrCommand::GetQueueDesc
                | dvr::DvrCommand::Start
                | dvr::DvrCommand::Stop
                | dvr::DvrCommand::Flush
                | dvr::DvrCommand::Close
                | dvr::DvrCommand::SetStatusCheckIntervalHint(_),
            )
            | DomainCommand::Descrambler(_)
            | DomainCommand::Lnb(lnb::LnbCommand::SendDiseqc(_) | lnb::LnbCommand::Close) => None,
        }
    }

    pub fn plan(&self) -> CommandPlan {
        match self {
            DomainCommand::PublicApi { object, api } => CommandPlan {
                object: *object,
                api: *api,
                transaction: match object {
                    AidlObjectKind::Tuner => RuntimeTransactionName::TunerPublicApiTxn,
                    AidlObjectKind::Frontend => RuntimeTransactionName::FrontendPublicApiTxn,
                    AidlObjectKind::Demux => RuntimeTransactionName::DemuxPublicApiTxn,
                    _ => RuntimeTransactionName::TunerPublicApiTxn,
                },
            },
            DomainCommand::UnsupportedPublicApi { object, api, .. } => CommandPlan {
                object: *object,
                api: *api,
                transaction: match object {
                    AidlObjectKind::Tuner => RuntimeTransactionName::TunerUnsupportedPublicApiTxn,
                    AidlObjectKind::Frontend => {
                        RuntimeTransactionName::FrontendUnsupportedPublicApiTxn
                    }
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
        assert_eq!(
            command.plan().transaction,
            RuntimeTransactionName::FrontendTuneTxnApply
        );
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
    fn aidl_method_adapter_creates_domain_command_without_intermediate_string_layer() {
        let plan = aidl_method::AidlMethodAdapter::frontend_tune(request());
        assert!(matches!(
            plan.command,
            DomainCommand::Frontend(frontend::FrontendCommand::Tune(_))
        ));
        assert_eq!(
            plan.command_plan.transaction,
            RuntimeTransactionName::FrontendTuneTxnApply
        );
    }

    #[test]
    fn all_aidl_method_kinds_have_command_plan_entries() {
        for method in aidl_method::all_aidl_method_kinds_for_coverage(request()) {
            let plan = aidl_method::AidlMethodAdapter::plan(method);
            assert!(AIDL_TRANSACTION_TABLE.contains(&plan.command_plan));
        }
    }
}

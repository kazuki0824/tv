use crate::boot::TunerServiceRuntime;
use crate::method_dispatch::plan_object_method_dispatch;
use crate::registry::{FrontendRegistryEntry, LnbRegistryProfile};
use maleicacid_tuner_hal2_binder_adapter::{AidlMethodAdapter, AidlMethodCall};
use maleicacid_tuner_hal2_common::HalError;
use maleicacid_tuner_hal2_common::{FrontendBackendKind, FrontendSystem};
use maleicacid_tuner_hal2_domain_request::{AidlApi, AidlObjectKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RootFrontendInfoSnapshot {
    pub id: i32,
    pub backend: FrontendBackendKind,
    pub system: FrontendSystem,
    pub lnb_profile: Option<LnbRegistryProfile>,
}

impl From<FrontendRegistryEntry> for RootFrontendInfoSnapshot {
    fn from(entry: FrontendRegistryEntry) -> Self {
        Self {
            id: entry.id.0,
            backend: entry.backend,
            system: entry.system,
            lnb_profile: entry.lnb_profile,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RootDemuxCapabilitiesSnapshot {
    pub num_demux: i32,
    pub num_record: i32,
    pub num_playback: i32,
    pub num_ts_filter: i32,
    pub num_section_filter: i32,
    pub num_audio_filter: i32,
    pub num_video_filter: i32,
    pub num_pes_filter: i32,
    pub num_pcr_filter: i32,
    pub num_bytes_in_section_filter: i64,
    pub filter_caps: i32,
    pub link_caps: Vec<i32>,
    pub has_time_filter: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RootDemuxInfoSnapshot {
    pub filter_types: i32,
}

const ROOT_MAX_LIVE_DEMUXES: i32 = 8;
const ROOT_DEMUX_MAX_TS_FILTERS: i32 = 32;
const ROOT_DEMUX_MAX_SECTION_FILTERS: i32 = 8;
const ROOT_DEMUX_MAX_AUDIO_FILTERS: i32 = 4;
const ROOT_DEMUX_MAX_VIDEO_FILTERS: i32 = 4;
const ROOT_DEMUX_MAX_PES_FILTERS: i32 = 8;
const ROOT_DEMUX_MAX_PCR_FILTERS: i32 = 4;
const ROOT_MAX_SECTION_FILTER_BYTES: i64 = 16;
const ROOT_SUPPORTED_DEMUX_FILTER_CAPS: i32 = 1;

fn root_demux_capabilities_snapshot() -> RootDemuxCapabilitiesSnapshot {
    RootDemuxCapabilitiesSnapshot {
        num_demux: ROOT_MAX_LIVE_DEMUXES,
        num_record: ROOT_MAX_LIVE_DEMUXES,
        num_playback: ROOT_MAX_LIVE_DEMUXES,
        num_ts_filter: ROOT_DEMUX_MAX_TS_FILTERS,
        num_section_filter: ROOT_DEMUX_MAX_SECTION_FILTERS,
        num_audio_filter: ROOT_DEMUX_MAX_AUDIO_FILTERS,
        num_video_filter: ROOT_DEMUX_MAX_VIDEO_FILTERS,
        num_pes_filter: ROOT_DEMUX_MAX_PES_FILTERS,
        num_pcr_filter: ROOT_DEMUX_MAX_PCR_FILTERS,
        num_bytes_in_section_filter: ROOT_MAX_SECTION_FILTER_BYTES,
        filter_caps: ROOT_SUPPORTED_DEMUX_FILTER_CAPS,
        link_caps: vec![ROOT_SUPPORTED_DEMUX_FILTER_CAPS, 0, 0, 0, 0],
        has_time_filter: false,
    }
}

fn root_demux_info_snapshot() -> RootDemuxInfoSnapshot {
    RootDemuxInfoSnapshot {
        filter_types: ROOT_SUPPORTED_DEMUX_FILTER_CAPS,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RootQueryRequest {
    FrontendIds,
    FrontendInfo { frontend_id: i32 },
    LnbIds,
    DemuxIds,
    DemuxInfo { demux_id: i32 },
    DemuxCapabilities,
    MaxNumberOfFrontends { frontend_system: FrontendSystem },
    LnaSupported,
}

impl RootQueryRequest {
    fn method(&self) -> AidlMethodCall {
        match self {
            Self::FrontendIds => public_root_api(AidlApi::TunerGetFrontendIds),
            Self::FrontendInfo { .. } => public_root_api(AidlApi::TunerGetFrontendInfo),
            Self::LnbIds => public_root_api(AidlApi::TunerGetLnbIds),
            Self::DemuxIds => public_root_api(AidlApi::TunerGetDemuxIds),
            Self::DemuxInfo { .. } => public_root_api(AidlApi::TunerGetDemuxInfo),
            Self::DemuxCapabilities => public_root_api(AidlApi::TunerGetDemuxCaps),
            Self::MaxNumberOfFrontends { .. } => {
                public_root_api(AidlApi::TunerGetMaxNumberOfFrontends)
            }
            Self::LnaSupported => public_root_api(AidlApi::TunerIsLnaSupported),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum RootQueryResponse {
    FrontendIds(Vec<i32>),
    FrontendInfo(RootFrontendInfoSnapshot),
    LnbIds(Vec<i32>),
    DemuxIds(Vec<i32>),
    DemuxInfo(RootDemuxInfoSnapshot),
    DemuxCapabilities(RootDemuxCapabilitiesSnapshot),
    MaxNumberOfFrontends(i32),
    LnaSupported(bool),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RootCommandRequest {
    SetLna {
        enabled: bool,
    },
    SetMaxNumberOfFrontends {
        frontend_system: FrontendSystem,
        max_number: i32,
    },
}

impl RootCommandRequest {
    fn method(&self) -> AidlMethodCall {
        match self {
            Self::SetLna { .. } => unsupported_root_api(AidlApi::TunerSetLna),
            Self::SetMaxNumberOfFrontends { .. } => {
                public_root_api(AidlApi::TunerSetMaxNumberOfFrontends)
            }
        }
    }
}

fn public_root_api(api: AidlApi) -> AidlMethodCall {
    AidlMethodCall::PublicApi {
        object: AidlObjectKind::Tuner,
        api,
    }
}

fn unsupported_root_api(api: AidlApi) -> AidlMethodCall {
    AidlMethodCall::UnsupportedPublicApi {
        object: AidlObjectKind::Tuner,
        api,
    }
}

fn preflight_root_method_dispatch(
    runtime: &mut TunerServiceRuntime,
    method: AidlMethodCall,
) -> Result<(), HalError> {
    let method_plan = AidlMethodAdapter::plan(method)?;
    plan_object_method_dispatch(
        runtime,
        method_plan.command_plan,
        method_plan.command.runtime_executable_request(),
    )
}

impl TunerServiceRuntime {
    pub fn execute_root_query(
        &mut self,
        request: RootQueryRequest,
    ) -> Result<RootQueryResponse, HalError> {
        preflight_root_method_dispatch(self, request.method())?;
        let query = self.query();
        match request {
            RootQueryRequest::FrontendIds => {
                Ok(RootQueryResponse::FrontendIds(query.frontend_ids()))
            }
            RootQueryRequest::FrontendInfo { frontend_id } => query
                .frontend_entry(frontend_id)
                .map(RootFrontendInfoSnapshot::from)
                .map(RootQueryResponse::FrontendInfo)
                .ok_or(HalError::Unsupported("frontend id is not available")),
            RootQueryRequest::LnbIds => Ok(RootQueryResponse::LnbIds(query.lnb_ids())),
            RootQueryRequest::DemuxIds => Ok(RootQueryResponse::DemuxIds(query.demux_ids())),
            RootQueryRequest::DemuxInfo { demux_id } => {
                if query.has_demux_id(demux_id) {
                    Ok(RootQueryResponse::DemuxInfo(root_demux_info_snapshot()))
                } else {
                    Err(HalError::Unsupported("demux id is not available"))
                }
            }
            RootQueryRequest::DemuxCapabilities => Ok(RootQueryResponse::DemuxCapabilities(
                root_demux_capabilities_snapshot(),
            )),
            RootQueryRequest::MaxNumberOfFrontends {
                frontend_system: _frontend_system,
            } => Ok(RootQueryResponse::MaxNumberOfFrontends(0)),
            RootQueryRequest::LnaSupported => Ok(RootQueryResponse::LnaSupported(false)),
        }
    }

    pub fn execute_root_command(&mut self, request: RootCommandRequest) -> Result<(), HalError> {
        preflight_root_method_dispatch(self, request.method())?;
        match request {
            RootCommandRequest::SetLna { enabled: _enabled } => {
                Err(HalError::Unsupported("LNA is unsupported"))
            }
            RootCommandRequest::SetMaxNumberOfFrontends {
                frontend_system: _frontend_system,
                max_number,
            } => {
                if max_number == 0 {
                    Ok(())
                } else {
                    Err(HalError::Unsupported(
                        "frontend max override is unavailable without probed frontend",
                    ))
                }
            }
        }
    }
}

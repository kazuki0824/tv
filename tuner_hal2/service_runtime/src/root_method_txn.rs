use crate::boot::TunerServiceRuntime;
use crate::method_dispatch::plan_object_method_dispatch;
use crate::registry::{
    FrontendCapabilitySnapshot, FrontendRegistryEntry, LnbRegistryProfile,
};
use maleicacid_tuner_hal2_binder_adapter::{AidlMethodAdapter, AidlMethodCall};
use maleicacid_tuner_hal2_common::{FrontendBackendKind, FrontendSystem};
use maleicacid_tuner_hal2_common::{HalError, HalInvalidArgumentKind};
use maleicacid_tuner_hal2_domain_request::{AidlApi, AidlObjectKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RootFrontendInfoSnapshot {
    pub id: i32,
    pub backend: FrontendBackendKind,
    pub system: FrontendSystem,
    pub lnb_profile: Option<LnbRegistryProfile>,
    pub capability: FrontendCapabilitySnapshot,
}

impl From<FrontendRegistryEntry> for RootFrontendInfoSnapshot {
    fn from(entry: FrontendRegistryEntry) -> Self {
        Self {
            id: entry.id.0,
            backend: entry.backend,
            system: entry.system,
            lnb_profile: entry.lnb_profile,
            capability: entry.capability,
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

const ROOT_MAX_SECTION_FILTER_BYTES: i64 = 16;
pub(crate) fn published_demux_ids(
    snapshot: crate::CapabilitySnapshot,
) -> Result<Vec<i32>, maleicacid_tuner_hal2_common::HalError> {
    snapshot.public_demux_ids()
}

pub(crate) fn is_public_demux_id(
    snapshot: crate::CapabilitySnapshot,
    demux_id: i32,
) -> Result<bool, maleicacid_tuner_hal2_common::HalError> {
    snapshot
        .public_demux_filter_types(demux_id)
        .map(|filter_types| filter_types.is_some())
}

fn root_demux_capabilities_snapshot(
    snapshot: crate::CapabilitySnapshot,
) -> Result<RootDemuxCapabilitiesSnapshot, maleicacid_tuner_hal2_common::HalError> {
    let public_demuxes = snapshot.public_demuxes()?;
    let num_demux = i32::try_from(public_demuxes.len()).map_err(|_| {
        maleicacid_tuner_hal2_common::HalError::internal(
            maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
            "published demux capability count overflow",
        )
    })?;
    let filter_caps = snapshot.public_demux_filter_caps()?;
    Ok(RootDemuxCapabilitiesSnapshot {
        num_demux,
        num_record: snapshot.num_record,
        num_playback: snapshot.num_playback,
        num_ts_filter: snapshot.num_ts_filter,
        num_section_filter: snapshot.num_section_filter,
        num_audio_filter: snapshot.num_audio_filter,
        num_video_filter: snapshot.num_video_filter,
        num_pes_filter: snapshot.num_pes_filter,
        num_pcr_filter: snapshot.num_pcr_filter,
        num_bytes_in_section_filter: ROOT_MAX_SECTION_FILTER_BYTES,
        filter_caps,
        link_caps: vec![filter_caps, 0, 0, 0, 0],
        has_time_filter: false,
    })
}

fn root_demux_info_snapshot(
    snapshot: crate::CapabilitySnapshot,
    demux_id: i32,
) -> Result<Option<RootDemuxInfoSnapshot>, maleicacid_tuner_hal2_common::HalError> {
    snapshot
        .public_demux_filter_types(demux_id)
        .map(|filter_types| filter_types.map(|filter_types| RootDemuxInfoSnapshot { filter_types }))
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
            RootQueryRequest::DemuxIds => Ok(RootQueryResponse::DemuxIds(
                published_demux_ids(self.capability_snapshot())?,
            )),
            RootQueryRequest::DemuxInfo { demux_id } => {
                root_demux_info_snapshot(self.capability_snapshot(), demux_id)?
                    .map(RootQueryResponse::DemuxInfo)
                    .ok_or_else(|| HalError::invalid_argument(
                        HalInvalidArgumentKind::NumericRange,
                        "demux id is not published by the capability snapshot",
                    ))
            }
            RootQueryRequest::DemuxCapabilities => root_demux_capabilities_snapshot(
                self.capability_snapshot(),
            )
            .map(RootQueryResponse::DemuxCapabilities),
            RootQueryRequest::MaxNumberOfFrontends { frontend_system } => {
                Ok(RootQueryResponse::MaxNumberOfFrontends(
                    self.current_max_number_of_frontends(frontend_system),
                ))
            }
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
                frontend_system,
                max_number,
            } => {
                if max_number < 0 {
                    return Err(HalError::invalid_argument(
                        HalInvalidArgumentKind::NumericRange,
                        "frontend max number must be non-negative",
                    ));
                }
                let default_max = self.default_max_number_of_frontends(frontend_system);
                if max_number <= default_max {
                    self.set_current_max_number_of_frontends(frontend_system, max_number);
                    Ok(())
                } else {
                    Err(HalError::invalid_argument(
                        HalInvalidArgumentKind::NumericRange,
                        "frontend max number exceeds available frontend count for this type",
                    ))
                }
            }
        }
    }
}

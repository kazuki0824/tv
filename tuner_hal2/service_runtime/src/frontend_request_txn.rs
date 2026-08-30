use crate::registry::{FrontendRegistryEntry, SatellitePowerTopology};
use crate::TunerServiceRuntime;
use maleicacid_tuner_hal2_common::{
    is_japan_bs_if_frequency_hz, is_japan_cs110_if_frequency_hz,
    is_japan_isdbt_frequency_contract_hz, FrontendBackendKind,
    FrontendIsdbtPartialReceptionRequirement, FrontendScanMode, FrontendSystem,
    FrontendTuneRequest, HalError, HalInternalKind, HalInvalidArgumentKind,
};

fn validate_frontend_request_against_entry(
    entry: &FrontendRegistryEntry,
    request: &FrontendTuneRequest,
) -> Result<(), HalError> {
    if entry.system != request.system {
        return Err(HalError::invalid_argument(
            HalInvalidArgumentKind::MissingDeliverySystem,
            format!(
                "requested frontend system {} does not match exported frontend {}",
                request.system.as_hint(),
                entry.system.as_hint()
            ),
        ));
    }

    match request.system {
        FrontendSystem::IsdbT => {
            if matches!(
                request.partial_reception,
                FrontendIsdbtPartialReceptionRequirement::Required(_)
            ) && entry.backend == FrontendBackendKind::LinuxDvb
            {
                return Err(HalError::unsupported_detail(
                    "isdbt.partialReceptionFlag",
                    "earth_pt1 does not expose current TMCC partial reception readback",
                ));
            }
            if !is_japan_isdbt_frequency_contract_hz(request.frequency) {
                return Err(HalError::invalid_argument(
                    HalInvalidArgumentKind::UnsupportedFrequency,
                    "ISDB-T frequency is outside Japan CATV C13..UHF62 contract range",
                ));
            }
            if request.stream_id.is_some() || request.stream_id_kind.is_some() {
                return Err(HalError::invalid_argument(
                    HalInvalidArgumentKind::UnsupportedStreamSelector,
                    "ISDB-T tune must not carry ISDB-S stream selector",
                ));
            }
        }
        FrontendSystem::IsdbS => {
            if request.partial_reception != FrontendIsdbtPartialReceptionRequirement::Unspecified {
                return Err(HalError::invalid_argument(
                    HalInvalidArgumentKind::NumericRange,
                    "ISDB-S tune must not carry an ISDB-T partial reception requirement",
                ));
            }
            let is_bs = is_japan_bs_if_frequency_hz(request.frequency);
            let is_cs110 = is_japan_cs110_if_frequency_hz(request.frequency);
            if !is_bs && !is_cs110 {
                return Err(HalError::invalid_argument(
                    HalInvalidArgumentKind::UnsupportedFrequency,
                    "ISDB-S frequency must be a Japan BS/CS110 IF center frequency",
                ));
            }
            if is_cs110 && (request.stream_id.is_some() || request.stream_id_kind.is_some()) {
                return Err(HalError::invalid_argument(
                    HalInvalidArgumentKind::UnsupportedStreamSelector,
                    "CS110 tune must not carry TSID or relative stream selector",
                ));
            }
            if let Some(symbol_rate) = request.symbol_rate {
                let symbol_rate = i32::try_from(symbol_rate).map_err(|_| {
                    HalError::invalid_argument(
                        HalInvalidArgumentKind::UnsupportedSymbolRate,
                        "ISDB-S symbol rate does not fit the advertised capability domain",
                    )
                })?;
                let scalar = entry.capability.scalar;
                if symbol_rate < scalar.min_symbol_rate || symbol_rate > scalar.max_symbol_rate {
                    return Err(HalError::invalid_argument(
                        HalInvalidArgumentKind::UnsupportedSymbolRate,
                        "ISDB-S symbol rate is outside the advertised frontend range",
                    ));
                }
            }
        }
        FrontendSystem::IsdbS3 | FrontendSystem::DvbS => {
            return Err(HalError::Unsupported(
                "frontend system is outside the r51 product scope",
            ));
        }
    }
    Ok(())
}

fn validate_frontend_lnb_candidate(
    runtime: &TunerServiceRuntime,
    entry: &FrontendRegistryEntry,
    request: &FrontendTuneRequest,
) -> Result<(), HalError> {
    if !matches!(request.system, FrontendSystem::IsdbS) {
        return Ok(());
    }
    if entry.satellite_power_topology == SatellitePowerTopology::ExternalOrShared {
        return Ok(());
    }
    if entry.satellite_power_topology != SatellitePowerTopology::InternalFixed15V {
        return Err(HalError::Unsupported(
            "ISDB-S frontend does not have a verified power topology",
        ));
    }
    let lnb = runtime.query().lnb_for_frontend_id(entry.id.0);
    match (entry.lnb_profile, lnb) {
        (Some(expected_profile), Some(lnb_entry)) if lnb_entry.profile == expected_profile => {
            Ok(())
        }
        (Some(_), Some(_)) => Err(HalError::internal(
            HalInternalKind::InvariantViolation,
            "frontend/LNB profile mismatch in runtime registry",
        )),
        _ => Err(HalError::Unsupported(
            "ISDB-S frontend does not have a registered LNB candidate",
        )),
    }
}

fn validate_backend_tune_preflight(
    entry: &FrontendRegistryEntry,
    request: &FrontendTuneRequest,
) -> Result<(), HalError> {
    match entry.backend {
        FrontendBackendKind::Px4CharDevice => {
            let _mapped = maleicacid_tuner_hal2_device::px4::map_tune_request_to_px4(request)?;
        }
        FrontendBackendKind::LinuxDvb => {
            let normalized =
                maleicacid_tuner_hal2_device::dvb::normalized_tune_request_from_common(request)?;
            let pairs = maleicacid_tuner_hal2_device::dvb::tune_property_pairs(&normalized)?;
            let _dtv_properties = pairs.to_dtv_properties();
        }
    }
    Ok(())
}

impl TunerServiceRuntime {
    pub fn validate_frontend_request_for_id(
        &self,
        frontend_id: i32,
        request: &FrontendTuneRequest,
    ) -> Result<FrontendRegistryEntry, HalError> {
        let entry = self
            .frontend_entry(frontend_id)
            .ok_or(HalError::Unsupported(
                "frontend runtime entry is not available",
            ))?;
        validate_frontend_request_against_entry(&entry, request)?;
        validate_frontend_lnb_candidate(self, &entry, request)?;
        Ok(entry)
    }

    pub fn backend_scan_candidates_for_entry(
        &self,
        entry: &FrontendRegistryEntry,
        request: &FrontendTuneRequest,
        scan_mode: FrontendScanMode,
    ) -> Result<Vec<FrontendTuneRequest>, HalError> {
        let candidates = match entry.backend {
            FrontendBackendKind::Px4CharDevice => {
                maleicacid_tuner_hal2_device::px4::px4_scan_requests(request)?
            }
            FrontendBackendKind::LinuxDvb => {
                maleicacid_tuner_hal2_device::dvb::dvb_scan_requests(request, scan_mode)?
            }
        };
        for candidate in candidates.iter() {
            validate_backend_tune_preflight(entry, candidate)?;
        }
        Ok(candidates)
    }

    pub fn scan_candidates_for_frontend_entry(
        &self,
        entry: &FrontendRegistryEntry,
        request: &FrontendTuneRequest,
        scan_mode: FrontendScanMode,
    ) -> Result<Vec<FrontendTuneRequest>, HalError> {
        self.backend_scan_candidates_for_entry(entry, request, scan_mode)
    }
}

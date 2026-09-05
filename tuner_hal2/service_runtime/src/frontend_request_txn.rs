use crate::registry::{FrontendRegistryEntry, SatellitePowerTopology};
use crate::TunerServiceRuntime;
use maleicacid_tuner_hal2_binder_adapter::FrontendRequestedSetting;
use maleicacid_tuner_hal2_common::{
    is_japan_isdbt_frequency_contract_hz, FrontendBackendKind,
    FrontendIsdbtPartialReceptionRequirement, FrontendScanMode, FrontendStreamIdKind,
    FrontendSystem, FrontendTuneRequest, HalError, HalInternalKind, HalInvalidArgumentKind,
};

fn validate_frontend_request_semantics(request: &FrontendTuneRequest) -> Result<(), HalError> {
    if request.system == FrontendSystem::IsdbT && request.isdbt_layer_settings.len() > 3 {
        return Err(HalError::invalid_argument(
            HalInvalidArgumentKind::NumericRange,
            "ISDB-T tune must not request more than the three physical hierarchical layers A/B/C",
        ));
    }
    Ok(())
}

fn validate_frontend_requested_settings_against_product_profile(
    requested_settings: &[FrontendRequestedSetting],
) -> Result<(), HalError> {
    for setting in requested_settings {
        let unsupported = match setting {
            FrontendRequestedSetting::IsdbtBandwidthAuto
            | FrontendRequestedSetting::IsdbtModeAuto
            | FrontendRequestedSetting::IsdbtGuardIntervalAuto
            | FrontendRequestedSetting::IsdbtLayerModulationAuto { .. }
            | FrontendRequestedSetting::IsdbtLayerCoderateAuto { .. }
            | FrontendRequestedSetting::IsdbtLayerTimeInterleaveAuto { .. }
            | FrontendRequestedSetting::IsdbsModulationAuto
            | FrontendRequestedSetting::IsdbsCoderateAuto => None,
            FrontendRequestedSetting::IsdbtExplicitBandwidth {
                bandwidth_hz: 6_000_000,
            } => None,
            FrontendRequestedSetting::IsdbtExplicitBandwidth { .. } => Some((
                "isdbt.bandwidth",
                "known ISDB-T bandwidth is not supported by this product profile",
            )),
            FrontendRequestedSetting::IsdbtExplicitMode { .. } => {
                Some(("isdbt.mode", "explicit ISDB-T mode is not supported"))
            }
            FrontendRequestedSetting::IsdbtExplicitInversion { .. } => Some((
                "isdbt.inversion",
                "explicit ISDB-T spectral inversion is not supported",
            )),
            FrontendRequestedSetting::IsdbtExplicitGuardInterval { .. } => Some((
                "isdbt.guardInterval",
                "explicit ISDB-T guard interval is not supported",
            )),
            FrontendRequestedSetting::IsdbtServiceAreaId { .. } => Some((
                "isdbt.serviceAreaId",
                "explicit ISDB-T serviceAreaId is not supported",
            )),
            FrontendRequestedSetting::IsdbtPartialReceptionAuto => Some((
                "isdbt.partialReceptionFlag",
                "ISDB-T partial reception AUTO is not supported",
            )),
            FrontendRequestedSetting::IsdbtLayerModulation { .. } => Some((
                "isdbt.layer.modulation",
                "explicit ISDB-T layer modulation is not supported",
            )),
            FrontendRequestedSetting::IsdbtLayerCoderate { .. } => Some((
                "isdbt.layer.coderate",
                "explicit ISDB-T layer coderate is not supported",
            )),
            FrontendRequestedSetting::IsdbtLayerTimeInterleave { .. } => Some((
                "isdbt.layer.timeInterleave",
                "explicit ISDB-T layer time interleave is not supported",
            )),
            FrontendRequestedSetting::IsdbtExplicitSegmentCount { .. } => Some((
                "isdbt.layer.numOfSegment",
                "explicit ISDB-T segment count is not supported",
            )),
            FrontendRequestedSetting::IsdbsExplicitModulation { .. } => Some((
                "isdbs.modulation",
                "explicit ISDB-S modulation is not supported",
            )),
            FrontendRequestedSetting::IsdbsExplicitCoderate { .. } => Some((
                "isdbs.coderate",
                "explicit ISDB-S coderate is not supported",
            )),
            FrontendRequestedSetting::IsdbsExplicitRolloff { .. } => {
                Some(("isdbs.rolloff", "explicit ISDB-S rolloff is not supported"))
            }
        };
        if let Some((feature, detail)) = unsupported {
            return Err(HalError::unsupported_detail(feature, detail));
        }
    }
    Ok(())
}

fn validate_scan_mode_against_product_profile(scan_mode: FrontendScanMode) -> Result<(), HalError> {
    if scan_mode == FrontendScanMode::Blind {
        return Err(HalError::unsupported_detail(
            "frontend.scan.blind",
            "blind scan is not supported by the current product profile",
        ));
    }
    Ok(())
}

fn validate_isdbs_selector_invalid_arguments(
    request: &FrontendTuneRequest,
    is_bs: bool,
    is_cs110: bool,
) -> Result<(), HalError> {
    if is_cs110 && (request.stream_id.is_some() || request.stream_id_kind.is_some()) {
        return Err(HalError::invalid_argument(
            HalInvalidArgumentKind::UnsupportedStreamSelector,
            "CS110 tune must not carry TSID or relative stream selector",
        ));
    }
    if !is_bs {
        return Ok(());
    }
    let Some(stream_id) = request.stream_id else {
        return Ok(());
    };
    if stream_id > 65_534 {
        return Err(HalError::invalid_argument(
            HalInvalidArgumentKind::InvalidStreamIdRange,
            "ISDB-S STREAM_ID must be in 0..=65534 after AOSP INVALID_STREAM_ID normalization",
        ));
    }
    if matches!(
        request.stream_id_kind,
        Some(FrontendStreamIdKind::RelativeStreamNumber)
    ) && stream_id > 7
    {
        return Err(HalError::invalid_argument(
            HalInvalidArgumentKind::InvalidStreamIdRange,
            "ISDB-S RELATIVE_STREAM_NUMBER must be in 0..=7",
        ));
    }
    Ok(())
}

fn validate_frontend_request_invalid_arguments_against_entry(
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

    validate_frontend_request_semantics(request)?;

    match request.system {
        FrontendSystem::IsdbT => {
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
            let is_bs = maleicacid_tuner_hal2_device::px4::normalize_japan_bs_if_frequency_hz(
                request.frequency,
            )
            .is_some();
            let is_cs110 =
                maleicacid_tuner_hal2_device::px4::normalize_japan_cs110_if_frequency_hz(
                    request.frequency,
                )
                .is_some();
            if !is_bs && !is_cs110 {
                return Err(HalError::invalid_argument(
                    HalInvalidArgumentKind::UnsupportedFrequency,
                    "ISDB-S frequency cannot be normalized unambiguously to the Japan BS/CS110 raster",
                ));
            }
            validate_isdbs_selector_invalid_arguments(request, is_bs, is_cs110)?;
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
        FrontendSystem::IsdbS3 | FrontendSystem::DvbS => {}
    }
    Ok(())
}

fn validate_frontend_request_availability_against_entry(
    entry: &FrontendRegistryEntry,
    request: &FrontendTuneRequest,
) -> Result<(), HalError> {
    match request.system {
        FrontendSystem::IsdbT => {
            if matches!(
                (request.partial_reception, entry.backend),
                (
                    FrontendIsdbtPartialReceptionRequirement::Required(_),
                    FrontendBackendKind::LinuxDvb,
                )
            ) {
                return Err(HalError::unsupported_detail(
                    "isdbt.partialReceptionFlag",
                    "earth_pt1 does not expose current TMCC partial reception readback",
                ));
            }
        }
        FrontendSystem::IsdbS => {
            let is_bs = maleicacid_tuner_hal2_device::px4::normalize_japan_bs_if_frequency_hz(
                request.frequency,
            )
            .is_some();
            if is_bs {
                match (entry.backend, request.stream_id, request.stream_id_kind) {
                    (
                        FrontendBackendKind::Px4CharDevice,
                        Some(0..=11),
                        Some(FrontendStreamIdKind::AbsoluteStreamId) | None,
                    ) => {
                        return Err(HalError::unsupported_detail(
                            "isdbs.streamId",
                            "px4 legacy slot ABI cannot distinguish absolute STREAM_ID 0..=11 from relative stream numbers",
                        ));
                    }
                    (
                        FrontendBackendKind::LinuxDvb,
                        Some(_),
                        Some(FrontendStreamIdKind::RelativeStreamNumber),
                    ) => {
                        return Err(HalError::unsupported_detail(
                            "isdbs.relativeStreamNumber",
                            "earth_pt1/Linux DVB does not implement AOSP relative stream-number selection",
                        ));
                    }
                    _ => {}
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

fn validate_dynamic_isdbs_stream_id_scan_availability(
    entry: &FrontendRegistryEntry,
    request: &FrontendTuneRequest,
    scan_mode: Option<FrontendScanMode>,
) -> Result<(), HalError> {
    if scan_mode.is_none()
        || entry.backend != FrontendBackendKind::LinuxDvb
        || request.system != FrontendSystem::IsdbS
        || request.stream_id.is_some()
        || maleicacid_tuner_hal2_device::px4::normalize_japan_bs_if_frequency_hz(request.frequency)
            .is_none()
    {
        return Ok(());
    }
    Err(HalError::unsupported_detail(
        "frontend.scan.inputStreamIds",
        "Linux DVB does not expose authoritative BS TMCC TSID enumeration; use explicit absolute STREAM_ID tune candidates",
    ))
}

fn validate_frontend_begin_contract(
    entry: &FrontendRegistryEntry,
    request: &FrontendTuneRequest,
    requested_settings: &[FrontendRequestedSetting],
    scan_mode: Option<FrontendScanMode>,
) -> Result<(), HalError> {
    // 正規の優先順位として、malformed/semanticな`INVALID_ARGUMENT`判定をすべて完了してから、
    // 構文上有効だが製品/profileで利用不可な要求を判定する。
    validate_frontend_request_invalid_arguments_against_entry(entry, request)?;
    validate_frontend_requested_settings_against_product_profile(requested_settings)?;
    validate_dynamic_isdbs_stream_id_scan_availability(entry, request, scan_mode)?;
    validate_frontend_request_availability_against_entry(entry, request)?;
    if let Some(scan_mode) = scan_mode {
        validate_scan_mode_against_product_profile(scan_mode)?;
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
    pub fn validate_frontend_begin_request_for_id(
        &self,
        frontend_id: i32,
        request: &FrontendTuneRequest,
        requested_settings: &[FrontendRequestedSetting],
        scan_mode: Option<FrontendScanMode>,
    ) -> Result<FrontendRegistryEntry, HalError> {
        let entry = self
            .frontend_entry(frontend_id)
            .ok_or(HalError::Unsupported(
                "frontend runtime entry is not available",
            ))?;
        validate_frontend_begin_contract(&entry, request, requested_settings, scan_mode)?;
        validate_frontend_lnb_candidate(self, &entry, request)?;
        Ok(entry)
    }

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
        validate_frontend_request_invalid_arguments_against_entry(&entry, request)?;
        validate_frontend_request_availability_against_entry(&entry, request)?;
        validate_frontend_lnb_candidate(self, &entry, request)?;
        Ok(entry)
    }

    pub fn backend_scan_candidates_for_entry(
        &self,
        entry: &FrontendRegistryEntry,
        request: &FrontendTuneRequest,
        scan_mode: FrontendScanMode,
    ) -> Result<Vec<FrontendTuneRequest>, HalError> {
        validate_scan_mode_against_product_profile(scan_mode)?;
        let candidates = match entry.backend {
            FrontendBackendKind::Px4CharDevice => {
                maleicacid_tuner_hal2_device::px4::px4_scan_requests(request)?
            }
            FrontendBackendKind::LinuxDvb => {
                maleicacid_tuner_hal2_device::dvb::dvb_scan_requests(request, scan_mode)?
            }
        };
        for candidate in &candidates {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{
        FrontendCapabilitySnapshot, FrontendRuntimeId, FrontendScalarCapability,
    };
    use std::path::PathBuf;

    fn entry(
        backend: FrontendBackendKind,
        system: FrontendSystem,
        min_symbol_rate: i32,
        max_symbol_rate: i32,
    ) -> FrontendRegistryEntry {
        FrontendRegistryEntry {
            id: FrontendRuntimeId(1),
            backend,
            system,
            device_path: PathBuf::from("/dev/frontend-test"),
            capability: FrontendCapabilitySnapshot {
                scalar: FrontendScalarCapability {
                    min_frequency_hz: if system == FrontendSystem::IsdbT {
                        111_142_857
                    } else {
                        1_049_480_000
                    },
                    max_frequency_hz: if system == FrontendSystem::IsdbT {
                        767_142_857
                    } else {
                        2_053_000_000
                    },
                    min_symbol_rate,
                    max_symbol_rate,
                    acquire_range_hz: 0,
                },
                exclusive_group_id: match backend {
                    FrontendBackendKind::Px4CharDevice => 0x1000_0001,
                    FrontendBackendKind::LinuxDvb => 0x2000_0001,
                },
                isdbt_segment: None,
            },
            lnb_profile: None,
            satellite_power_topology: SatellitePowerTopology::ExternalOrShared,
        }
    }

    fn isdbs_entry(
        backend: FrontendBackendKind,
        min_symbol_rate: i32,
        max_symbol_rate: i32,
    ) -> FrontendRegistryEntry {
        entry(
            backend,
            FrontendSystem::IsdbS,
            min_symbol_rate,
            max_symbol_rate,
        )
    }

    fn isdbs_request(symbol_rate: Option<u32>) -> FrontendTuneRequest {
        FrontendTuneRequest {
            system: FrontendSystem::IsdbS,
            frequency: 1_049_480_000,
            end_frequency: None,
            stream_id: None,
            stream_id_kind: None,
            bandwidth_hz: None,
            symbol_rate,
            isdbt_layer_settings: Vec::new(),
            partial_reception: FrontendIsdbtPartialReceptionRequirement::Unspecified,
        }
    }

    fn isdbt_request_with_layer_count(layer_count: usize) -> FrontendTuneRequest {
        FrontendTuneRequest {
            system: FrontendSystem::IsdbT,
            frequency: 473_142_857,
            end_frequency: None,
            stream_id: None,
            stream_id_kind: None,
            bandwidth_hz: Some(6_000_000),
            symbol_rate: None,
            isdbt_layer_settings: vec![
                maleicacid_tuner_hal2_common::FrontendIsdbtLayerSetting {
                    num_of_segment:
                        maleicacid_tuner_hal2_common::FrontendIsdbtSegmentRequest::Unspecified,
                };
                layer_count
            ],
            partial_reception: FrontendIsdbtPartialReceptionRequirement::Unspecified,
        }
    }

    #[test]
    fn selectorless_bs_dynamic_stream_id_scan_is_px4_only() {
        let linux = isdbs_entry(FrontendBackendKind::LinuxDvb, 1, 100_000_000);
        let px4 = isdbs_entry(FrontendBackendKind::Px4CharDevice, 1, 100_000_000);
        let seed = isdbs_request(None);

        assert!(validate_dynamic_isdbs_stream_id_scan_availability(
            &linux,
            &seed,
            Some(FrontendScanMode::Auto),
        )
        .is_err());
        assert!(validate_dynamic_isdbs_stream_id_scan_availability(
            &px4,
            &seed,
            Some(FrontendScanMode::Auto),
        )
        .is_ok());

        let mut explicit = isdbs_request(None);
        explicit.stream_id = Some(16_400);
        explicit.stream_id_kind = Some(FrontendStreamIdKind::AbsoluteStreamId);
        assert!(validate_dynamic_isdbs_stream_id_scan_availability(
            &linux,
            &explicit,
            Some(FrontendScanMode::Auto),
        )
        .is_ok());
    }

    #[test]
    fn isdbt_physical_layer_cardinality_is_validated_after_aidl_conversion() {
        for layer_count in 0..=3 {
            assert!(
                validate_frontend_request_semantics(&isdbt_request_with_layer_count(layer_count))
                    .is_ok()
            );
        }
        for layer_count in 4..=5 {
            let error =
                validate_frontend_request_semantics(&isdbt_request_with_layer_count(layer_count))
                    .unwrap_err();
            assert_eq!(
                error.invalid_argument_kind(),
                Some(HalInvalidArgumentKind::NumericRange)
            );
        }
    }

    #[test]
    fn isdbt_auto_observations_are_supported_by_product_profile() {
        assert!(
            validate_frontend_requested_settings_against_product_profile(&[
                FrontendRequestedSetting::IsdbtBandwidthAuto,
                FrontendRequestedSetting::IsdbtModeAuto,
                FrontendRequestedSetting::IsdbtGuardIntervalAuto,
                FrontendRequestedSetting::IsdbtLayerModulationAuto { layer_index: 0 },
                FrontendRequestedSetting::IsdbtLayerCoderateAuto { layer_index: 0 },
                FrontendRequestedSetting::IsdbtLayerTimeInterleaveAuto { layer_index: 0 },
            ])
            .is_ok()
        );
    }

    #[test]
    fn semantic_invalid_precedes_product_unavailable() {
        let request = isdbt_request_with_layer_count(4);
        let error = validate_frontend_begin_contract(
            &entry(
                FrontendBackendKind::Px4CharDevice,
                FrontendSystem::IsdbT,
                0,
                0,
            ),
            &request,
            &[FrontendRequestedSetting::IsdbtExplicitMode { value: 2 }],
            None,
        )
        .unwrap_err();
        assert_eq!(
            error.invalid_argument_kind(),
            Some(HalInvalidArgumentKind::NumericRange)
        );
    }

    #[test]
    fn invalid_frequency_precedes_product_unavailable() {
        let mut request = isdbt_request_with_layer_count(1);
        request.frequency = 1;
        let error = validate_frontend_begin_contract(
            &entry(
                FrontendBackendKind::Px4CharDevice,
                FrontendSystem::IsdbT,
                0,
                0,
            ),
            &request,
            &[FrontendRequestedSetting::IsdbtExplicitMode { value: 2 }],
            None,
        )
        .unwrap_err();
        assert_eq!(
            error.invalid_argument_kind(),
            Some(HalInvalidArgumentKind::UnsupportedFrequency)
        );
    }

    #[test]
    fn invalid_cs110_selector_precedes_product_unavailable() {
        let mut request = isdbs_request(Some(28_860_000));
        request.frequency = 1_613_000_000;
        request.stream_id = Some(1);
        request.stream_id_kind = Some(FrontendStreamIdKind::AbsoluteStreamId);
        let error = validate_frontend_begin_contract(
            &isdbs_entry(FrontendBackendKind::Px4CharDevice, 28_860_000, 28_860_000),
            &request,
            &[FrontendRequestedSetting::IsdbsExplicitRolloff { value: 1 }],
            None,
        )
        .unwrap_err();
        assert_eq!(
            error.invalid_argument_kind(),
            Some(HalInvalidArgumentKind::UnsupportedStreamSelector)
        );
    }

    #[test]
    fn invalid_bs_relative_selector_precedes_product_unavailable() {
        let mut request = isdbs_request(Some(28_860_000));
        request.stream_id = Some(8);
        request.stream_id_kind = Some(FrontendStreamIdKind::RelativeStreamNumber);
        let error = validate_frontend_begin_contract(
            &isdbs_entry(FrontendBackendKind::Px4CharDevice, 28_860_000, 28_860_000),
            &request,
            &[FrontendRequestedSetting::IsdbsExplicitRolloff { value: 1 }],
            None,
        )
        .unwrap_err();
        assert_eq!(
            error.invalid_argument_kind(),
            Some(HalInvalidArgumentKind::InvalidStreamIdRange)
        );
    }

    #[test]
    fn px4_bs_absolute_tsid_0_through_11_is_canonical_unavailable() {
        for stream_id in [0, 11] {
            let mut request = isdbs_request(Some(28_860_000));
            request.stream_id = Some(stream_id);
            request.stream_id_kind = Some(FrontendStreamIdKind::AbsoluteStreamId);
            assert!(matches!(
                validate_frontend_begin_contract(
                    &isdbs_entry(FrontendBackendKind::Px4CharDevice, 28_860_000, 28_860_000,),
                    &request,
                    &[],
                    None,
                ),
                Err(HalError::UnsupportedDetail {
                    feature: "isdbs.streamId",
                    ..
                })
            ));
        }
    }

    #[test]
    fn linux_dvb_bs_relative_selector_is_canonical_unavailable() {
        for stream_id in [0, 7] {
            let mut request = isdbs_request(Some(28_860_000));
            request.stream_id = Some(stream_id);
            request.stream_id_kind = Some(FrontendStreamIdKind::RelativeStreamNumber);
            assert!(matches!(
                validate_frontend_begin_contract(
                    &isdbs_entry(FrontendBackendKind::LinuxDvb, 28_860_000, 28_860_000),
                    &request,
                    &[],
                    None,
                ),
                Err(HalError::UnsupportedDetail {
                    feature: "isdbs.relativeStreamNumber",
                    ..
                })
            ));
        }
    }

    #[test]
    fn supported_bs_selector_boundaries_pass_canonical_preflight() {
        let mut px4_relative = isdbs_request(Some(28_860_000));
        px4_relative.stream_id = Some(7);
        px4_relative.stream_id_kind = Some(FrontendStreamIdKind::RelativeStreamNumber);
        assert!(validate_frontend_begin_contract(
            &isdbs_entry(FrontendBackendKind::Px4CharDevice, 28_860_000, 28_860_000),
            &px4_relative,
            &[],
            None,
        )
        .is_ok());

        let mut px4_absolute = isdbs_request(Some(28_860_000));
        px4_absolute.stream_id = Some(12);
        px4_absolute.stream_id_kind = Some(FrontendStreamIdKind::AbsoluteStreamId);
        assert!(validate_frontend_begin_contract(
            &isdbs_entry(FrontendBackendKind::Px4CharDevice, 28_860_000, 28_860_000),
            &px4_absolute,
            &[],
            None,
        )
        .is_ok());

        let mut dvb_absolute = isdbs_request(Some(28_860_000));
        dvb_absolute.stream_id = Some(0);
        dvb_absolute.stream_id_kind = Some(FrontendStreamIdKind::AbsoluteStreamId);
        assert!(validate_frontend_begin_contract(
            &isdbs_entry(FrontendBackendKind::LinuxDvb, 28_860_000, 28_860_000),
            &dvb_absolute,
            &[],
            None,
        )
        .is_ok());
    }

    #[test]
    fn isdbs_auto_constraints_are_supported_by_product_profile() {
        assert!(
            validate_frontend_requested_settings_against_product_profile(&[
                FrontendRequestedSetting::IsdbsModulationAuto,
                FrontendRequestedSetting::IsdbsCoderateAuto,
            ])
            .is_ok()
        );
    }

    #[test]
    fn explicit_partial_reception_is_available_only_for_px4() {
        for required in [false, true] {
            let mut request = isdbt_request_with_layer_count(1);
            request.partial_reception =
                FrontendIsdbtPartialReceptionRequirement::Required(required);

            assert!(validate_frontend_begin_contract(
                &entry(
                    FrontendBackendKind::Px4CharDevice,
                    FrontendSystem::IsdbT,
                    0,
                    0,
                ),
                &request,
                &[],
                None,
            )
            .is_ok());
            assert!(matches!(
                validate_frontend_begin_contract(
                    &entry(
                        FrontendBackendKind::LinuxDvb,
                        FrontendSystem::IsdbT,
                        0,
                        0,
                    ),
                    &request,
                    &[],
                    None,
                ),
                Err(HalError::UnsupportedDetail {
                    feature: "isdbt.partialReceptionFlag",
                    ..
                })
            ));
        }
    }

    #[test]
    fn blind_scan_is_rejected_by_product_profile_before_backend_dispatch() {
        assert!(validate_scan_mode_against_product_profile(FrontendScanMode::Auto).is_ok());
        assert!(matches!(
            validate_scan_mode_against_product_profile(FrontendScanMode::Blind),
            Err(HalError::UnsupportedDetail {
                feature: "frontend.scan.blind",
                ..
            })
        ));
    }

    #[test]
    fn px4_symbol_rate_acceptance_matches_fixed_advertised_capability() {
        let entry = isdbs_entry(FrontendBackendKind::Px4CharDevice, 28_860_000, 28_860_000);
        assert!(validate_frontend_request_invalid_arguments_against_entry(
            &entry,
            &isdbs_request(None)
        )
        .is_ok());
        assert!(validate_frontend_request_invalid_arguments_against_entry(
            &entry,
            &isdbs_request(Some(28_860_000))
        )
        .is_ok());
        assert!(matches!(
            validate_frontend_request_invalid_arguments_against_entry(
                &entry,
                &isdbs_request(Some(28_859_999))
            ),
            Err(HalError::InvalidArgument { .. })
        ));
    }

    #[test]
    fn dvb_symbol_rate_acceptance_matches_fixed_earth_pt1_capability() {
        let entry = isdbs_entry(FrontendBackendKind::LinuxDvb, 28_860_000, 28_860_000);
        for symbol_rate in [None, Some(28_860_000)] {
            assert!(validate_frontend_request_invalid_arguments_against_entry(
                &entry,
                &isdbs_request(symbol_rate)
            )
            .is_ok());
        }
        for symbol_rate in [28_859_999, 28_860_001] {
            let error = validate_frontend_request_invalid_arguments_against_entry(
                &entry,
                &isdbs_request(Some(symbol_rate)),
            )
            .unwrap_err();
            assert_eq!(
                error.invalid_argument_kind(),
                Some(HalInvalidArgumentKind::UnsupportedSymbolRate)
            );
        }
    }
}

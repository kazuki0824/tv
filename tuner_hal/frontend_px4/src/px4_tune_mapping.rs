use crate::{PTX_ISDB_S_SYSTEM, PTX_ISDB_T_SYSTEM};
use maleicacid_tuner_hal_common::{
    FrontendStreamIdKind, FrontendSystem, FrontendTuneRequest, HalError,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Px4TuneRequest {
    pub system_code: u32,
    pub freq_no: i32,
    pub slot: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Px4SatBand {
    Bs,
    Cs110,
}

const PX4_ISDBT_UHF_FREQ_NO_MIN: i32 = 63;
const PX4_ISDBT_UHF_FREQ_NO_MAX: i32 = 112;
const PX4_ISDBT_UHF_BASE_KHZ: i32 = 95_143;
const PX4_ISDBT_UHF_STEP_KHZ: i32 = 6_000;
const PX4_ISDBT_CATV_BASE_KHZ: i32 = 93_143;
const PX4_ISDBT_CATV_CH12_EXTRA_KHZ: i32 = 2_000;
const PX4_ISDBT_CATV_RANGES: &[(i32, i32)] = &[(3, 12), (22, 62)];
const PX4_FREQ_TOLERANCE_KHZ: i32 = 500;

const PX4_BS_BASE_IF_HZ: u64 = 1_049_480_000;
const PX4_BS_STEP_HZ: u64 = 38_360_000;
const PX4_BS_FREQ_NO_MIN: i32 = 0;
const PX4_BS_FREQ_NO_MAX: i32 = 11;
const PX4_BS_SLOT_MIN: u16 = 0;
const PX4_BS_SLOT_MAX: u16 = 7;

const PX4_CS_BASE_IF_HZ: u64 = 1_613_000_000;
const PX4_CS_STEP_HZ: u64 = 40_000_000;
const PX4_CS_FREQ_NO_MIN: i32 = 12;
const PX4_CS_FREQ_NO_MAX: i32 = 23;

fn hz_to_nearest_khz(hz: u64) -> Result<i32, HalError> {
    let rounded = (hz + 500) / 1_000;
    i32::try_from(rounded)
        .map_err(|_| HalError::InvalidArgument(format!("frequency too large: {hz}")))
}

fn inverse_linear_index(freq_khz: i32, base_khz: i32, step_khz: i32) -> (i32, i32) {
    let delta = freq_khz - base_khz;
    let index = if delta >= 0 {
        (delta + step_khz / 2) / step_khz
    } else {
        (delta - step_khz / 2) / step_khz
    };
    let canonical = base_khz + index * step_khz;
    (index, freq_khz - canonical)
}

fn checked_direct_freq_no_with_tolerance(
    freq_khz: i32,
    base_khz: i32,
    step_khz: i32,
    first_freq_no: i32,
    last_freq_no: i32,
    tolerance_khz: i32,
) -> Option<(i32, i32)> {
    let (freq_no, residual) = inverse_linear_index(freq_khz, base_khz, step_khz);
    if !(first_freq_no..=last_freq_no).contains(&freq_no) {
        return None;
    }
    (residual.abs() <= tolerance_khz).then_some((freq_no, residual))
}

fn checked_direct_freq_no(
    freq_khz: i32,
    base_khz: i32,
    step_khz: i32,
    first_freq_no: i32,
    last_freq_no: i32,
) -> Option<(i32, i32)> {
    checked_direct_freq_no_with_tolerance(
        freq_khz,
        base_khz,
        step_khz,
        first_freq_no,
        last_freq_no,
        PX4_FREQ_TOLERANCE_KHZ,
    )
}

fn is_exact_japan_cs110_if_frequency_hz(if_hz: u64) -> bool {
    if if_hz < PX4_CS_BASE_IF_HZ {
        return false;
    }
    let delta = if_hz - PX4_CS_BASE_IF_HZ;
    if delta % PX4_CS_STEP_HZ != 0 {
        return false;
    }
    let freq_no = PX4_CS_FREQ_NO_MIN + i32::try_from(delta / PX4_CS_STEP_HZ).unwrap_or(i32::MAX);
    (PX4_CS_FREQ_NO_MIN..=PX4_CS_FREQ_NO_MAX).contains(&freq_no)
}

pub fn map_isdbt_frequency_to_px4(freq_hz: u64) -> Result<Px4TuneRequest, HalError> {
    let freq_khz = hz_to_nearest_khz(freq_hz)?;
    if let Some((freq_no, addfreq_khz)) = checked_direct_freq_no(
        freq_khz,
        PX4_ISDBT_UHF_BASE_KHZ,
        PX4_ISDBT_UHF_STEP_KHZ,
        PX4_ISDBT_UHF_FREQ_NO_MIN,
        PX4_ISDBT_UHF_FREQ_NO_MAX,
    ) {
        return Ok(Px4TuneRequest {
            system_code: PTX_ISDB_T_SYSTEM,
            freq_no,
            slot: addfreq_khz,
        });
    }

    for &(first, last) in PX4_ISDBT_CATV_RANGES {
        for freq_no in first..=last {
            let mut canonical = PX4_ISDBT_CATV_BASE_KHZ + freq_no * PX4_ISDBT_UHF_STEP_KHZ;
            if freq_no == 12 {
                canonical += PX4_ISDBT_CATV_CH12_EXTRA_KHZ;
            }
            let residual = freq_khz - canonical;
            if residual.abs() <= PX4_FREQ_TOLERANCE_KHZ {
                return Ok(Px4TuneRequest {
                    system_code: PTX_ISDB_T_SYSTEM,
                    freq_no,
                    slot: residual,
                });
            }
        }
    }
    Err(HalError::InvalidArgument(format!(
        "px4 ISDB-T周波数が日本向けUHF/CATV写像許容範囲内にありません: {freq_hz}"
    )))
}

pub fn map_bs_if_frequency_to_px4_freq_no(if_hz: u64) -> Result<i32, HalError> {
    if if_hz < PX4_BS_BASE_IF_HZ {
        return Err(HalError::InvalidArgument(format!(
            "px4 BS IF周波数は非対応です: {if_hz}"
        )));
    }
    let delta = if_hz - PX4_BS_BASE_IF_HZ;
    if delta % PX4_BS_STEP_HZ != 0 {
        return Err(HalError::InvalidArgument(format!(
            "px4 BS IF周波数は非対応です: {if_hz}"
        )));
    }
    let freq_no = PX4_BS_FREQ_NO_MIN
        + i32::try_from(delta / PX4_BS_STEP_HZ).map_err(|_| {
            HalError::InvalidArgument(format!("px4 BS IF周波数は非対応です: {if_hz}"))
        })?;
    if (PX4_BS_FREQ_NO_MIN..=PX4_BS_FREQ_NO_MAX).contains(&freq_no) {
        Ok(freq_no)
    } else {
        Err(HalError::InvalidArgument(format!(
            "px4 BS IF周波数は非対応です: {if_hz}"
        )))
    }
}

pub fn map_cs110_if_frequency_to_px4_freq_no(if_hz: u64) -> Result<i32, HalError> {
    if if_hz < PX4_CS_BASE_IF_HZ {
        return Err(HalError::InvalidArgument(format!(
            "px4 110CS IF周波数は非対応です: {if_hz}"
        )));
    }
    let delta = if_hz - PX4_CS_BASE_IF_HZ;
    if delta % PX4_CS_STEP_HZ != 0 {
        return Err(HalError::InvalidArgument(format!(
            "px4 110CS IF周波数は非対応です: {if_hz}"
        )));
    }
    let freq_no = PX4_CS_FREQ_NO_MIN
        + i32::try_from(delta / PX4_CS_STEP_HZ).map_err(|_| {
            HalError::InvalidArgument(format!("px4 110CS IF周波数は非対応です: {if_hz}"))
        })?;
    if (PX4_CS_FREQ_NO_MIN..=PX4_CS_FREQ_NO_MAX).contains(&freq_no) {
        Ok(freq_no)
    } else {
        Err(HalError::InvalidArgument(format!(
            "px4 110CS IF周波数は非対応です: {if_hz}"
        )))
    }
}

pub fn map_relative_stream_number_to_px4_slot(
    relative_stream_number: u16,
    band: Px4SatBand,
) -> Result<i32, HalError> {
    match band {
        Px4SatBand::Bs if (PX4_BS_SLOT_MIN..=PX4_BS_SLOT_MAX).contains(&relative_stream_number) => {
            Ok(i32::from(relative_stream_number))
        }
        Px4SatBand::Bs => Err(HalError::InvalidArgument(format!(
            "px4 BS相対ストリーム番号が範囲外です: {relative_stream_number}"
        ))),
        Px4SatBand::Cs110 => Err(HalError::InvalidArgument(
            "CS110はTSIDまたは相対ストリーム番号によるフロントエンド選択を使いません".to_string(),
        )),
    }
}

pub fn reportable_bs_tsid_for_scan(
    _if_hz: u64,
    raw_stream_id: u32,
    stream_id_kind: Option<FrontendStreamIdKind>,
) -> Option<u16> {
    match stream_id_kind {
        Some(FrontendStreamIdKind::RelativeStreamNumber) => None,
        Some(FrontendStreamIdKind::AbsoluteStreamId) | None if raw_stream_id >= 12 => {
            u16::try_from(raw_stream_id).ok()
        }
        Some(FrontendStreamIdKind::AbsoluteStreamId) | None => None,
    }
}

// この直渡し経路は、本プロジェクトで採用する px4_drv feat/android-ddk 系を対象にする。
// 同系ではBS slot >= 8が拒否されず、PTX_SET_CHANNEL.slotがdemod stream_idとして渡される。
// 公開develop相当driverとの互換目的で、TSIDから相対スロットへの変換表をここへ戻してはならない。
fn map_absolute_stream_id_to_px4_slot(stream_id: u16, band: Px4SatBand) -> Result<i32, HalError> {
    match band {
        Px4SatBand::Bs if stream_id >= 12 => Ok(i32::from(stream_id)),
        Px4SatBand::Bs => Err(HalError::InvalidArgument(format!(
            "px4 BS STREAM_ID は絶対TSIDでなければならず、相対ストリーム番号は使えません: {stream_id}"
        ))),
        Px4SatBand::Cs110 => Err(HalError::InvalidArgument(format!(
            "CS110のTSIDフロントエンド選択は方針上非対応です: 0x{stream_id:04x}"
        ))),
    }
}

fn validate_backend_bandwidth(request: &FrontendTuneRequest) -> Result<(), HalError> {
    match request.system {
        FrontendSystem::IsdbT => match request.bandwidth_hz {
            None | Some(6_000_000) => Ok(()),
            Some(other) => Err(HalError::InvalidArgument(format!(
                "r51のpx4 ISDB-Tは6MHz帯域幅だけを受け付けます。指定値: {other}Hz"
            ))),
        },
        FrontendSystem::IsdbS => match request.bandwidth_hz {
            None => Ok(()),
            Some(other) => Err(HalError::InvalidArgument(format!(
                "r51のpx4 ISDB-Sはbandwidth_hzを受け付けません。指定値: {other}Hz"
            ))),
        },
        FrontendSystem::IsdbS3 | FrontendSystem::DvbS => Ok(()),
    }
}

pub fn map_tune_request_to_px4(request: &FrontendTuneRequest) -> Result<Px4TuneRequest, HalError> {
    if request.symbol_rate.is_some() {
        return Err(HalError::InvalidArgument(
            "r51のpx4バックエンド契約では明示symbol_rateを受け付けません".to_string(),
        ));
    }
    validate_backend_bandwidth(request)?;
    match request.system {
        FrontendSystem::IsdbT => map_isdbt_frequency_to_px4(request.frequency),
        FrontendSystem::IsdbS => {
            let band = if is_exact_japan_cs110_if_frequency_hz(request.frequency) {
                Px4SatBand::Cs110
            } else {
                Px4SatBand::Bs
            };
            let freq_no = match band {
                Px4SatBand::Bs => map_bs_if_frequency_to_px4_freq_no(request.frequency)?,
                Px4SatBand::Cs110 => map_cs110_if_frequency_to_px4_freq_no(request.frequency)?,
            };
            let slot = match band {
                Px4SatBand::Cs110 => {
                    if request.stream_id.is_some() {
                        return Err(HalError::InvalidArgument("CS110フロントエンド選局にTSIDまたは相対ストリーム番号セレクタを載せてはなりません".to_string()));
                    }
                    0
                }
                Px4SatBand::Bs => {
                    let raw_stream_id = request.stream_id.ok_or_else(|| {
                        HalError::InvalidArgument("px4 BS選局にはTSIDまたは相対ストリーム番号が必要です。HAL側のscan展開は提供しません".to_string())
                    })?;
                    let stream_id = u16::try_from(raw_stream_id).map_err(|_| {
                        HalError::InvalidArgument(format!(
                            "stream_id が範囲外です: {raw_stream_id}"
                        ))
                    })?;
                    match request.stream_id_kind {
                        Some(FrontendStreamIdKind::RelativeStreamNumber) => {
                            map_relative_stream_number_to_px4_slot(stream_id, band)?
                        }
                        Some(FrontendStreamIdKind::AbsoluteStreamId) | None => {
                            map_absolute_stream_id_to_px4_slot(stream_id, band)?
                        }
                    }
                }
            };
            Ok(Px4TuneRequest {
                system_code: PTX_ISDB_S_SYSTEM,
                freq_no,
                slot,
            })
        }
        FrontendSystem::IsdbS3 | FrontendSystem::DvbS => Err(HalError::Unsupported(
            "px4バックエンドはISDB-T/ISDB-Sのみ対象です",
        )),
    }
}

pub fn px4_scan_requests(base: &FrontendTuneRequest) -> Result<Vec<FrontendTuneRequest>, HalError> {
    if base.end_frequency.unwrap_or(base.frequency) != base.frequency {
        return Err(HalError::Unsupported("px4バックエンドは日本向けscan表を生成しません。TISが明示選局候補を渡す必要があります"));
    }
    let _ = map_tune_request_to_px4(base)?;
    Ok(vec![base.clone()])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bs_request(tsid: u32) -> FrontendTuneRequest {
        FrontendTuneRequest {
            system: FrontendSystem::IsdbS,
            frequency: 1_049_480_000,
            end_frequency: None,
            stream_id: Some(tsid),
            stream_id_kind: Some(FrontendStreamIdKind::AbsoluteStreamId),
            bandwidth_hz: None,
            symbol_rate: None,
        }
    }

    #[test]
    fn rejects_internal_symbol_rate_contract_violation() {
        let request = FrontendTuneRequest {
            symbol_rate: Some(28_860_000),
            ..bs_request(0x4010)
        };
        let err = map_tune_request_to_px4(&request).unwrap_err().to_string();
        assert!(err.contains("symbol_rate"), "{err}");
    }

    #[test]
    fn maps_uhf_frequency_to_px4_channel() {
        let mapped = map_isdbt_frequency_to_px4(557_142_857).unwrap();
        assert_eq!(mapped.system_code, PTX_ISDB_T_SYSTEM);
        assert_eq!(mapped.freq_no, 77);
        assert_eq!(mapped.slot.abs() <= PX4_FREQ_TOLERANCE_KHZ, true);
    }

    #[test]
    fn maps_uhf_band_edges_with_direct_freq_no_formula() {
        let ch13 = map_isdbt_frequency_to_px4(473_142_857).unwrap();
        assert_eq!(ch13.freq_no, 63);
        assert_eq!(ch13.slot, 0);

        let ch62 = map_isdbt_frequency_to_px4(767_142_857).unwrap();
        assert_eq!(ch62.freq_no, 112);
        assert_eq!(ch62.slot, 0);
    }

    #[test]
    fn maps_bs_and_cs110_carrier_edges_with_direct_formula() {
        assert_eq!(
            map_bs_if_frequency_to_px4_freq_no(1_049_480_000).unwrap(),
            0
        );
        assert_eq!(
            map_bs_if_frequency_to_px4_freq_no(1_471_440_000).unwrap(),
            11
        );
        assert_eq!(
            map_cs110_if_frequency_to_px4_freq_no(1_613_000_000).unwrap(),
            12
        );
        assert_eq!(
            map_cs110_if_frequency_to_px4_freq_no(2_053_000_000).unwrap(),
            23
        );
    }

    #[test]
    fn maps_bs_tsid_to_direct_slot_inside_px4_backend() {
        let mapped = map_tune_request_to_px4(&bs_request(0x4011)).unwrap();
        assert_eq!(mapped.system_code, PTX_ISDB_S_SYSTEM);
        assert_eq!(mapped.freq_no, 0);
        assert_eq!(mapped.slot, 0x4011);
    }

    #[test]
    fn accepts_px4_bs_relative_stream_number_candidates() {
        let request = FrontendTuneRequest {
            stream_id: Some(2),
            stream_id_kind: Some(FrontendStreamIdKind::RelativeStreamNumber),
            ..bs_request(0x4010)
        };
        let mapped = map_tune_request_to_px4(&request).unwrap();
        assert_eq!(mapped.system_code, PTX_ISDB_S_SYSTEM);
        assert_eq!(mapped.freq_no, 0);
        assert_eq!(mapped.slot, 2);
    }

    #[test]
    fn reports_only_absolute_tsid_for_scan() {
        assert_eq!(
            reportable_bs_tsid_for_scan(
                1_049_480_000,
                0,
                Some(FrontendStreamIdKind::RelativeStreamNumber)
            ),
            None
        );
        assert_eq!(
            reportable_bs_tsid_for_scan(
                1_049_480_000,
                3,
                Some(FrontendStreamIdKind::AbsoluteStreamId)
            ),
            None
        );
        assert_eq!(
            reportable_bs_tsid_for_scan(
                1_049_480_000,
                0x4011,
                Some(FrontendStreamIdKind::AbsoluteStreamId)
            ),
            Some(0x4011)
        );
    }

    #[test]
    fn rejects_relative_like_value_in_absolute_stream_id_path() {
        let request = FrontendTuneRequest {
            stream_id: Some(3),
            stream_id_kind: Some(FrontendStreamIdKind::AbsoluteStreamId),
            ..bs_request(0x4010)
        };
        let err = map_tune_request_to_px4(&request).unwrap_err().to_string();
        assert!(err.contains("absolute TSID"), "{err}");
    }

    #[test]
    fn rejects_bs_frequency_without_stream_selector() {
        let request = FrontendTuneRequest {
            stream_id: None,
            stream_id_kind: None,
            ..bs_request(0x4010)
        };
        assert!(map_tune_request_to_px4(&request).is_err());
    }

    #[test]
    fn cs110_frequency_only_maps_to_fixed_zero_slot() {
        let request = FrontendTuneRequest {
            system: FrontendSystem::IsdbS,
            frequency: 1_613_000_000,
            end_frequency: None,
            stream_id: None,
            stream_id_kind: None,
            bandwidth_hz: None,
            symbol_rate: None,
        };
        let mapped = map_tune_request_to_px4(&request).unwrap();
        assert_eq!(mapped.system_code, PTX_ISDB_S_SYSTEM);
        assert_eq!(mapped.freq_no, 12);
        assert_eq!(mapped.slot, 0);
    }

    #[test]
    fn cs110_rejects_tsid_frontend_selection() {
        let request = FrontendTuneRequest {
            system: FrontendSystem::IsdbS,
            frequency: 1_613_000_000,
            end_frequency: None,
            stream_id: Some(0x6020),
            stream_id_kind: Some(FrontendStreamIdKind::AbsoluteStreamId),
            bandwidth_hz: None,
            symbol_rate: None,
        };
        let err = map_tune_request_to_px4(&request).unwrap_err().to_string();
        assert!(
            err.contains("CS110のTSIDフロントエンド選択は方針上非対応です"),
            "{err}"
        );
    }

    #[test]
    fn cs110_rejects_relative_frontend_selection() {
        let request = FrontendTuneRequest {
            system: FrontendSystem::IsdbS,
            frequency: 1_613_000_000,
            end_frequency: None,
            stream_id: Some(0),
            stream_id_kind: Some(FrontendStreamIdKind::RelativeStreamNumber),
            bandwidth_hz: None,
            symbol_rate: None,
        };
        let err = map_tune_request_to_px4(&request).unwrap_err().to_string();
        assert!(
            err.contains("CS110のTSIDフロントエンド選択は方針上非対応です"),
            "{err}"
        );
    }

    #[test]
    fn explicit_vts_profile_requests_expand_to_one_px4_scan_candidate() {
        let isdbt = FrontendTuneRequest {
            system: FrontendSystem::IsdbT,
            frequency: 557_142_857,
            end_frequency: None,
            stream_id: None,
            stream_id_kind: None,
            bandwidth_hz: Some(6_000_000),
            symbol_rate: None,
        };
        let bs = FrontendTuneRequest {
            end_frequency: Some(1_049_480_000),
            ..bs_request(0x4010)
        };
        let cs110 = FrontendTuneRequest {
            system: FrontendSystem::IsdbS,
            frequency: 1_613_000_000,
            end_frequency: None,
            stream_id: None,
            stream_id_kind: None,
            bandwidth_hz: None,
            symbol_rate: None,
        };
        assert_eq!(px4_scan_requests(&isdbt).unwrap(), vec![isdbt]);
        assert_eq!(px4_scan_requests(&bs).unwrap(), vec![bs]);
        assert_eq!(px4_scan_requests(&cs110).unwrap(), vec![cs110]);
    }

    #[test]
    fn isdbs_satellite_frequency_validation_is_exact_when_acquire_range_is_zero() {
        assert!(map_bs_if_frequency_to_px4_freq_no(1_049_480_000).is_ok());
        assert!(map_bs_if_frequency_to_px4_freq_no(1_049_480_001).is_err());
        assert!(map_bs_if_frequency_to_px4_freq_no(1_049_979_999).is_err());
        assert!(map_cs110_if_frequency_to_px4_freq_no(1_613_000_000).is_ok());
        assert!(map_cs110_if_frequency_to_px4_freq_no(1_613_000_001).is_err());
        assert!(map_cs110_if_frequency_to_px4_freq_no(1_613_499_999).is_err());
    }

    #[test]
    fn bs_tsid_is_not_validated_against_backend_local_table() {
        let request = FrontendTuneRequest {
            stream_id: Some(0x4973),
            stream_id_kind: Some(FrontendStreamIdKind::AbsoluteStreamId),
            ..bs_request(0x4010)
        };
        let mapped = map_tune_request_to_px4(&request).unwrap();
        assert_eq!(mapped.slot, 0x4973);
    }

    #[test]
    fn range_scan_generation_is_not_supported() {
        let request = FrontendTuneRequest {
            end_frequency: Some(2_053_000_000),
            ..bs_request(0x4010)
        };
        assert!(px4_scan_requests(&request).is_err());
    }
}

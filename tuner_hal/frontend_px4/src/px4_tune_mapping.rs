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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Px4BsTsidEntry {
    if_frequency_hz: u32,
    relative_stream_number: u16,
    tsid: u16,
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

// BS 専用。CS110 は TSID による frontend 選局を行わない。
const PX4_BS_TSID_TABLE: &[Px4BsTsidEntry] = &[
    Px4BsTsidEntry { if_frequency_hz: 1_049_480_000, relative_stream_number: 0, tsid: 0x4010 },
    Px4BsTsidEntry { if_frequency_hz: 1_049_480_000, relative_stream_number: 1, tsid: 0x4011 },
    Px4BsTsidEntry { if_frequency_hz: 1_049_480_000, relative_stream_number: 2, tsid: 0x4012 },
    Px4BsTsidEntry { if_frequency_hz: 1_087_840_000, relative_stream_number: 0, tsid: 0x4030 },
    Px4BsTsidEntry { if_frequency_hz: 1_087_840_000, relative_stream_number: 1, tsid: 0x4631 },
    Px4BsTsidEntry { if_frequency_hz: 1_087_840_000, relative_stream_number: 2, tsid: 0x4632 },
    Px4BsTsidEntry { if_frequency_hz: 1_126_200_000, relative_stream_number: 0, tsid: 0x4450 },
    Px4BsTsidEntry { if_frequency_hz: 1_126_200_000, relative_stream_number: 1, tsid: 0x4451 },
    Px4BsTsidEntry { if_frequency_hz: 1_202_920_000, relative_stream_number: 0, tsid: 0x4090 },
    Px4BsTsidEntry { if_frequency_hz: 1_202_920_000, relative_stream_number: 1, tsid: 0x4092 },
    Px4BsTsidEntry { if_frequency_hz: 1_279_640_000, relative_stream_number: 0, tsid: 0x40d0 },
    Px4BsTsidEntry { if_frequency_hz: 1_279_640_000, relative_stream_number: 1, tsid: 0x40d1 },
    Px4BsTsidEntry { if_frequency_hz: 1_279_640_000, relative_stream_number: 2, tsid: 0x46d2 },
    Px4BsTsidEntry { if_frequency_hz: 1_318_000_000, relative_stream_number: 0, tsid: 0x40f1 },
    Px4BsTsidEntry { if_frequency_hz: 1_318_000_000, relative_stream_number: 1, tsid: 0x40f2 },
    Px4BsTsidEntry { if_frequency_hz: 1_318_000_000, relative_stream_number: 2, tsid: 0x48f3 },
    Px4BsTsidEntry { if_frequency_hz: 1_394_720_000, relative_stream_number: 0, tsid: 0x4730 },
    Px4BsTsidEntry { if_frequency_hz: 1_394_720_000, relative_stream_number: 1, tsid: 0x4731 },
    Px4BsTsidEntry { if_frequency_hz: 1_394_720_000, relative_stream_number: 2, tsid: 0x4732 },
    Px4BsTsidEntry { if_frequency_hz: 1_394_720_000, relative_stream_number: 3, tsid: 0x4733 },
    Px4BsTsidEntry { if_frequency_hz: 1_433_080_000, relative_stream_number: 0, tsid: 0x4750 },
    Px4BsTsidEntry { if_frequency_hz: 1_433_080_000, relative_stream_number: 1, tsid: 0x4751 },
    Px4BsTsidEntry { if_frequency_hz: 1_433_080_000, relative_stream_number: 2, tsid: 0x4752 },
    Px4BsTsidEntry { if_frequency_hz: 1_471_440_000, relative_stream_number: 0, tsid: 0x4770 },
    Px4BsTsidEntry { if_frequency_hz: 1_471_440_000, relative_stream_number: 1, tsid: 0x4971 },
    Px4BsTsidEntry { if_frequency_hz: 1_471_440_000, relative_stream_number: 2, tsid: 0x4972 },
];

/// px4 ローカル対応表は TIS の BS TSID 単一情報源と一致しなければならない。
pub fn px4_bs_tsid_contract_entries() -> Vec<(u32, u16)> {
    PX4_BS_TSID_TABLE
        .iter()
        .map(|entry| (entry.if_frequency_hz, entry.tsid))
        .collect()
}

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
        "px4 ISDB-T frequency is not in the Japanese UHF/CATV mapping tolerance: {freq_hz}"
    )))
}

pub fn map_bs_if_frequency_to_px4_freq_no(if_hz: u64) -> Result<i32, HalError> {
    if if_hz < PX4_BS_BASE_IF_HZ {
        return Err(HalError::InvalidArgument(format!(
            "px4 BS IF frequency is not supported: {if_hz}"
        )));
    }
    let delta = if_hz - PX4_BS_BASE_IF_HZ;
    if delta % PX4_BS_STEP_HZ != 0 {
        return Err(HalError::InvalidArgument(format!(
            "px4 BS IF frequency is not supported: {if_hz}"
        )));
    }
    let freq_no = PX4_BS_FREQ_NO_MIN
        + i32::try_from(delta / PX4_BS_STEP_HZ).map_err(|_| {
            HalError::InvalidArgument(format!("px4 BS IF frequency is not supported: {if_hz}"))
        })?;
    if (PX4_BS_FREQ_NO_MIN..=PX4_BS_FREQ_NO_MAX).contains(&freq_no) {
        Ok(freq_no)
    } else {
        Err(HalError::InvalidArgument(format!(
            "px4 BS IF frequency is not supported: {if_hz}"
        )))
    }
}

pub fn map_cs110_if_frequency_to_px4_freq_no(if_hz: u64) -> Result<i32, HalError> {
    if if_hz < PX4_CS_BASE_IF_HZ {
        return Err(HalError::InvalidArgument(format!(
            "px4 110CS IF frequency is not supported: {if_hz}"
        )));
    }
    let delta = if_hz - PX4_CS_BASE_IF_HZ;
    if delta % PX4_CS_STEP_HZ != 0 {
        return Err(HalError::InvalidArgument(format!(
            "px4 110CS IF frequency is not supported: {if_hz}"
        )));
    }
    let freq_no = PX4_CS_FREQ_NO_MIN
        + i32::try_from(delta / PX4_CS_STEP_HZ).map_err(|_| {
            HalError::InvalidArgument(format!("px4 110CS IF frequency is not supported: {if_hz}"))
        })?;
    if (PX4_CS_FREQ_NO_MIN..=PX4_CS_FREQ_NO_MAX).contains(&freq_no) {
        Ok(freq_no)
    } else {
        Err(HalError::InvalidArgument(format!(
            "px4 110CS IF frequency is not supported: {if_hz}"
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
            "px4 BS relative stream number out of range: {relative_stream_number}"
        ))),
        Px4SatBand::Cs110 => Err(HalError::InvalidArgument(
            "CS110 does not use TSID or relative stream-number frontend selection".to_string(),
        )),
    }
}

fn frequency_matches(a_hz: u64, b_hz: u32) -> bool {
    a_hz == u64::from(b_hz)
}

pub fn map_tsid_to_px4_relative_stream_number(if_hz: u64, tsid: u16) -> Option<u16> {
    PX4_BS_TSID_TABLE
        .iter()
        .find(|entry| entry.tsid == tsid && frequency_matches(if_hz, entry.if_frequency_hz))
        .map(|entry| entry.relative_stream_number)
}

pub fn map_bs_relative_stream_number_to_tsid(
    if_hz: u64,
    relative_stream_number: u16,
) -> Option<u16> {
    PX4_BS_TSID_TABLE
        .iter()
        .find(|entry| {
            entry.relative_stream_number == relative_stream_number
                && frequency_matches(if_hz, entry.if_frequency_hz)
        })
        .map(|entry| entry.tsid)
}

pub fn reportable_bs_tsid_for_scan(
    if_hz: u64,
    raw_stream_id: u32,
    stream_id_kind: Option<FrontendStreamIdKind>,
) -> Option<u16> {
    let value = u16::try_from(raw_stream_id).ok()?;
    match stream_id_kind {
        Some(FrontendStreamIdKind::RelativeStreamNumber) => {
            map_bs_relative_stream_number_to_tsid(if_hz, value)
        }
        Some(FrontendStreamIdKind::AbsoluteStreamId) | None => PX4_BS_TSID_TABLE
            .iter()
            .find(|entry| entry.tsid == value && frequency_matches(if_hz, entry.if_frequency_hz))
            .map(|entry| entry.tsid),
    }
}

fn map_absolute_stream_id_to_px4_slot(
    if_hz: u64,
    stream_id: u16,
    band: Px4SatBand,
) -> Result<i32, HalError> {
    match band {
        Px4SatBand::Bs => {
            let Some(relative) = map_tsid_to_px4_relative_stream_number(if_hz, stream_id) else {
                return Err(HalError::InvalidArgument(format!(
                    "px4 BS TSID is not in the backend-local TSID table: 0x{stream_id:04x}"
                )));
            };
            map_relative_stream_number_to_px4_slot(relative, band)
        }
        Px4SatBand::Cs110 => Err(HalError::InvalidArgument(format!(
            "CS110 TSID frontend selection is not supported by policy: 0x{stream_id:04x}"
        ))),
    }
}

fn validate_backend_bandwidth(request: &FrontendTuneRequest) -> Result<(), HalError> {
    match request.system {
        FrontendSystem::IsdbT => match request.bandwidth_hz {
            None | Some(6_000_000) => Ok(()),
            Some(other) => Err(HalError::InvalidArgument(format!(
                "r51 px4 ISDB-T accepts only 6MHz bandwidth; got {other}Hz"
            ))),
        },
        FrontendSystem::IsdbS => match request.bandwidth_hz {
            None => Ok(()),
            Some(other) => Err(HalError::InvalidArgument(format!(
                "r51 px4 ISDB-S does not accept bandwidth_hz; got {other}Hz"
            ))),
        },
        FrontendSystem::IsdbS3 | FrontendSystem::DvbS => Ok(()),
    }
}

pub fn map_tune_request_to_px4(request: &FrontendTuneRequest) -> Result<Px4TuneRequest, HalError> {
    if request.symbol_rate.is_some() {
        return Err(HalError::InvalidArgument(
            "r51 px4 backend contract does not accept explicit symbol_rate".to_string(),
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
                        return Err(HalError::InvalidArgument("CS110 frontend tune must not carry TSID or relative stream-number selector".to_string()));
                    }
                    0
                }
                Px4SatBand::Bs => {
                    let raw_stream_id = request.stream_id.ok_or_else(|| {
                        HalError::InvalidArgument("px4 BS tune requires TSID or relative stream number; HAL scan expansion is not provided".to_string())
                    })?;
                    let stream_id = u16::try_from(raw_stream_id).map_err(|_| {
                        HalError::InvalidArgument(format!(
                            "stream_id out of range: {raw_stream_id}"
                        ))
                    })?;
                    match request.stream_id_kind {
                        Some(FrontendStreamIdKind::RelativeStreamNumber) => {
                            map_relative_stream_number_to_px4_slot(stream_id, band)?
                        }
                        Some(FrontendStreamIdKind::AbsoluteStreamId) | None => {
                            map_absolute_stream_id_to_px4_slot(request.frequency, stream_id, band)?
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
            "px4 backend は ISDB-T/ISDB-S のみ対象です",
        )),
    }
}

pub fn px4_scan_requests(base: &FrontendTuneRequest) -> Result<Vec<FrontendTuneRequest>, HalError> {
    if base.end_frequency.unwrap_or(base.frequency) != base.frequency {
        return Err(HalError::Unsupported("px4 backend no longer generates Japanese scan tables; TIS must submit explicit tune candidates"));
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

    fn parse_tis_bs_tsid_entries_from_scan_plan() -> Vec<(u32, u16)> {
        let source = include_str!("../../../tis/src/com/maleicacid/tvinput/tis/ScanPlan.kt");
        let marker = "BsTsidEntry(";
        let mut entries = Vec::new();
        let mut rest = source;
        while let Some(marker_index) = rest.find(marker) {
            let tail = &rest[marker_index + marker.len()..];
            let Some(end_index) = tail.find(')') else {
                break;
            };
            let fields = tail[..end_index]
                .split(',')
                .map(|field| field.trim())
                .collect::<Vec<_>>();
            assert!(
                fields.len() >= 2,
                "TIS ScanPlan.kt の BsTsidEntry field 数が不足しています: {}",
                &tail[..end_index]
            );
            if fields[0].starts_with("val ") {
                rest = &tail[end_index + 1..];
                continue;
            }
            let frequency = fields[0]
                .trim_end_matches('L')
                .replace('_', "")
                .parse::<u32>()
                .expect("TIS ScanPlan.kt の BS frequency を解釈できません");
            let tsid = fields[1]
                .replace('_', "")
                .parse::<u16>()
                .expect("TIS ScanPlan.kt の BS TSID を解釈できません");
            entries.push((frequency, tsid));
            rest = &tail[end_index + 1..];
        }
        entries
    }

    #[test]
    fn px4_bs_tsid_table_matches_tis_bs_ssot_source() {
        let tis_entries = parse_tis_bs_tsid_entries_from_scan_plan();
        assert!(
            !tis_entries.is_empty(),
            "TIS ScanPlan.kt の BS TSID 表を読めませんでした"
        );
        assert_eq!(px4_bs_tsid_contract_entries(), tis_entries);
    }

    #[test]
    fn maps_bs_tsid_to_relative_slot_inside_px4_backend() {
        let mapped = map_tune_request_to_px4(&bs_request(0x4011)).unwrap();
        assert_eq!(mapped.system_code, PTX_ISDB_S_SYSTEM);
        assert_eq!(mapped.freq_no, 0);
        assert_eq!(mapped.slot, 1);
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
    fn maps_bs_relative_stream_number_to_reportable_tsid() {
        assert_eq!(
            reportable_bs_tsid_for_scan(
                1_049_480_000,
                0,
                Some(FrontendStreamIdKind::RelativeStreamNumber)
            ),
            Some(0x4010)
        );
        assert_eq!(
            reportable_bs_tsid_for_scan(
                1_049_480_000,
                3,
                Some(FrontendStreamIdKind::AbsoluteStreamId)
            ),
            None
        );
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
            err.contains("CS110 frontend tune must not carry TSID"),
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
            err.contains("CS110 frontend tune must not carry TSID"),
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
    fn bs_tsid_frequency_pair_is_exact() {
        assert_eq!(
            map_tsid_to_px4_relative_stream_number(1_471_440_000, 0x4972),
            Some(2)
        );
        assert_eq!(
            map_tsid_to_px4_relative_stream_number(1_471_440_000, 0x4973),
            None
        );
        assert_eq!(
            map_tsid_to_px4_relative_stream_number(1_471_440_001, 0x4972),
            None
        );
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

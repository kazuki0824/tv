use std::collections::{BTreeMap, BTreeSet};
use std::fs::OpenOptions;
use std::os::fd::AsRawFd;
use std::path::PathBuf;

use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::ITuner::BnTuner;
use binder::BinderFeatures;
use maleicacid_tuner_hal2_common::os_abi::{last_errno, raw_ioctl_ptr};
use maleicacid_tuner_hal2_common::{
    japan_isdbt_frequency_contract_range_hz, FrontendBackendKind, FrontendSystem, HalError,
    HalErrorDetail, ANDROID15_TRM_RESOURCE_ID_MAX, TUNER_SERVICE_NAME,
};
use maleicacid_tuner_hal2_device::dvb::{
    DtvProperties, DtvProperty, DtvPropertyBuffer, DtvPropertyUnion, DvbFrontendInfo,
    DTV_ENUM_DELSYS, FE_GET_INFO, FE_GET_PROPERTY, SYS_ISDBS, SYS_ISDBT,
};
use maleicacid_tuner_hal2_service_runtime::{
    CapabilitySuppressionReason, FrontendCapabilitySnapshot, FrontendProbeOutcome,
    FrontendRuntimeId, FrontendScalarCapability, IsdbtSegmentCapability, LnbRegistryProfile,
    SatellitePowerTopology, TunerServiceRuntime,
};

use crate::tuner_service::TunerAidlService;

const PX4_PROBE_PREFIXES: &[&str] = &[
    "px4video",
    "pxmlt5video",
    "pxmlt8video",
    "isdb6014video",
    "isdb2056video",
    "pxm1urvideo",
    "pxs1urvideo",
    "isdbt2071video",
];

fn px4_device_family_code(device_name: &str) -> i32 {
    if device_name.starts_with("px4video") {
        return 1;
    }
    if device_name.starts_with("pxmlt5video") {
        return 2;
    }
    if device_name.starts_with("pxmlt8video") {
        return 3;
    }
    if device_name.starts_with("isdb6014video") {
        return 4;
    }
    if device_name.starts_with("isdb2056video") {
        return 5;
    }
    if device_name.starts_with("pxm1urvideo") {
        return 6;
    }
    if device_name.starts_with("pxs1urvideo") {
        return 7;
    }
    if device_name.starts_with("isdbt2071video") {
        return 8;
    }
    0
}

fn px4_frontend_systems(unit: i32, device_name: &str) -> Vec<FrontendSystem> {
    if unit < 0 {
        return Vec::new();
    }
    if device_name.starts_with("px4video") {
        return match unit.rem_euclid(4) {
            0 | 1 => vec![FrontendSystem::IsdbS],
            2 | 3 => vec![FrontendSystem::IsdbT],
            _ => unreachable!("rem_euclid(4) must stay within 0..=3"),
        };
    }
    if device_name.starts_with("pxmlt5video")
        || device_name.starts_with("pxmlt8video")
        || device_name.starts_with("isdb6014video")
        || device_name.starts_with("isdb2056video")
        || device_name.starts_with("pxm1urvideo")
    {
        return vec![FrontendSystem::IsdbT, FrontendSystem::IsdbS];
    }
    if device_name.starts_with("pxs1urvideo") || device_name.starts_with("isdbt2071video") {
        return vec![FrontendSystem::IsdbT];
    }
    Vec::new()
}

fn px4_lnb_profile_from_device_name(device_name: &str) -> LnbRegistryProfile {
    if device_name.starts_with("px4video") {
        LnbRegistryProfile::Px4Device15VOnly
    } else {
        LnbRegistryProfile::NoPower
    }
}

fn probe_lnb_profile_for_frontend(
    backend: FrontendBackendKind,
    system: FrontendSystem,
    path: &std::path::Path,
    device_name: Option<&str>,
) -> Option<LnbRegistryProfile> {
    if !matches!(system, FrontendSystem::IsdbS) {
        return None;
    }
    Some(match backend {
        FrontendBackendKind::Px4CharDevice => {
            let name = device_name
                .or_else(|| path.file_name().and_then(|value| value.to_str()))
                .unwrap_or("");
            px4_lnb_profile_from_device_name(name)
        }
        FrontendBackendKind::LinuxDvb => LnbRegistryProfile::EarthPt1FixedLnb,
    })
}

fn probe_satellite_power_topology(
    system: FrontendSystem,
    profile: Option<LnbRegistryProfile>,
) -> SatellitePowerTopology {
    if system != FrontendSystem::IsdbS {
        return SatellitePowerTopology::UnknownOrDisabled;
    }
    match profile {
        Some(LnbRegistryProfile::Px4Device15VOnly | LnbRegistryProfile::EarthPt1FixedLnb) => {
            SatellitePowerTopology::InternalFixed15V
        }
        Some(LnbRegistryProfile::NoPower) => SatellitePowerTopology::ExternalOrShared,
        None => SatellitePowerTopology::UnknownOrDisabled,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DvbProbeVariant {
    system: FrontendSystem,
    capability: Option<FrontendCapabilitySnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DvbProbeCandidate {
    adapter: i32,
    frontend_index: i32,
    path: PathBuf,
    physical_device_identity: Option<PathBuf>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct DvbPhysicalExclusiveGroupKey {
    physical_device_identity: PathBuf,
    driver_stream_ordinal: u8,
}

const JAPAN_BS_FIRST_IF_HZ: i64 = 1_049_480_000;
const JAPAN_CS110_LAST_IF_HZ: i64 = 2_053_000_000;
const ISDBS_SYMBOL_RATE: i32 = 28_860_000;
const PX4_PHYSICAL_GROUP_TAG: i32 = 0x1000_0000;
const DVB_PHYSICAL_GROUP_TAG: i32 = 0x2000_0000;

#[derive(Debug, Default)]
struct FrontendIdAllocator {
    next: i32,
}

impl FrontendIdAllocator {
    fn allocate(&mut self) -> Option<FrontendRuntimeId> {
        if self.next > ANDROID15_TRM_RESOURCE_ID_MAX {
            return None;
        }
        let id = FrontendRuntimeId(self.next);
        self.next = self.next.checked_add(1)?;
        Some(id)
    }
}

fn px4_capability(
    unit: i32,
    device_name: &str,
    system: FrontendSystem,
) -> Option<FrontendCapabilitySnapshot> {
    let family = px4_device_family_code(device_name);
    if unit < 0 || family <= 0 || unit > 0x3fff || family > 0x03ff {
        return None;
    }
    let group_payload = family.checked_shl(14)?.checked_add(unit)?;
    let exclusive_group_id = PX4_PHYSICAL_GROUP_TAG.checked_add(group_payload)?;
    let (scalar, isdbt_segment) = match system {
        FrontendSystem::IsdbT => {
            let (min_frequency_hz, max_frequency_hz, _) = japan_isdbt_frequency_contract_range_hz();
            (
                FrontendScalarCapability {
                    min_frequency_hz: i64::try_from(min_frequency_hz).ok()?,
                    max_frequency_hz: i64::try_from(max_frequency_hz).ok()?,
                    min_symbol_rate: 0,
                    max_symbol_rate: 0,
                    acquire_range_hz: 0,
                },
                Some(IsdbtSegmentCapability {
                    // px4系の固定product profileはsegment数を指定せずに
                    // ISDB-T選局する。
                    is_segment_auto: true,
                    is_full_segment: true,
                }),
            )
        }
        FrontendSystem::IsdbS => (
            FrontendScalarCapability {
                min_frequency_hz: JAPAN_BS_FIRST_IF_HZ,
                max_frequency_hz: JAPAN_CS110_LAST_IF_HZ,
                min_symbol_rate: ISDBS_SYMBOL_RATE,
                max_symbol_rate: ISDBS_SYMBOL_RATE,
                acquire_range_hz: 0,
            },
            None,
        ),
        FrontendSystem::IsdbS3 | FrontendSystem::DvbS => return None,
    };
    Some(FrontendCapabilitySnapshot {
        scalar,
        exclusive_group_id,
        isdbt_segment,
    })
}

fn dvb_capability(
    info: &DvbFrontendInfo,
    system: FrontendSystem,
    exclusive_group_id: i32,
) -> Option<FrontendCapabilitySnapshot> {
    let frequency_scale = if matches!(system, FrontendSystem::IsdbS) {
        1_000_i64
    } else {
        1_i64
    };
    let probed_min = i64::from(info.frequency_min).checked_mul(frequency_scale)?;
    let probed_max = i64::from(info.frequency_max).checked_mul(frequency_scale)?;
    let (scalar, isdbt_segment) = match system {
        FrontendSystem::IsdbT => {
            let (contract_min, contract_max, _) = japan_isdbt_frequency_contract_range_hz();
            let contract_min = i64::try_from(contract_min).ok()?;
            let contract_max = i64::try_from(contract_max).ok()?;
            if probed_min > contract_min || probed_max < contract_max {
                return None;
            }
            (
                FrontendScalarCapability {
                    min_frequency_hz: contract_min,
                    max_frequency_hz: contract_max,
                    min_symbol_rate: 0,
                    max_symbol_rate: 0,
                    acquire_range_hz: 0,
                },
                Some(IsdbtSegmentCapability {
                    // earth-pt1 profileはsegment数を指定しない
                    // 固定ISDB-T選局経路を持つ。
                    is_segment_auto: true,
                    is_full_segment: true,
                }),
            )
        }
        FrontendSystem::IsdbS => {
            if probed_min > JAPAN_BS_FIRST_IF_HZ || probed_max < JAPAN_CS110_LAST_IF_HZ {
                return None;
            }
            (
                FrontendScalarCapability {
                    min_frequency_hz: JAPAN_BS_FIRST_IF_HZ,
                    max_frequency_hz: JAPAN_CS110_LAST_IF_HZ,
                    // Linux v6.6 tc90522は固定済みISDB-S moduleがこのrateで動作する一方、
                    // FE_GET_INFOのsymbol-rate metadataを0/0のまま返す。
                    min_symbol_rate: ISDBS_SYMBOL_RATE,
                    max_symbol_rate: ISDBS_SYMBOL_RATE,
                    acquire_range_hz: 0,
                },
                None,
            )
        }
        FrontendSystem::IsdbS3 | FrontendSystem::DvbS => return None,
    };
    Some(FrontendCapabilitySnapshot {
        scalar,
        exclusive_group_id,
        isdbt_segment,
    })
}

fn earth_pt1_verified_topology_keys(
    candidates: &[DvbProbeCandidate],
) -> BTreeMap<(i32, i32), DvbPhysicalExclusiveGroupKey> {
    const EARTH_PT1_INDEPENDENT_STREAMS: usize = 4;

    let mut candidates_by_device: BTreeMap<PathBuf, Vec<&DvbProbeCandidate>> = BTreeMap::new();
    for candidate in candidates {
        let Some(identity) = candidate.physical_device_identity.as_ref() else {
            continue;
        };
        candidates_by_device
            .entry(identity.clone())
            .or_default()
            .push(candidate);
    }

    let mut verified = BTreeMap::new();
    for (identity, mut device_candidates) in candidates_by_device {
        device_candidates.sort_by_key(|candidate| (candidate.adapter, candidate.frontend_index));
        let tuple_set = device_candidates
            .iter()
            .map(|candidate| (candidate.adapter, candidate.frontend_index))
            .collect::<BTreeSet<_>>();
        if device_candidates.len() != EARTH_PT1_INDEPENDENT_STREAMS
            || tuple_set.len() != EARTH_PT1_INDEPENDENT_STREAMS
            || device_candidates
                .iter()
                .any(|candidate| candidate.frontend_index != 0)
        {
            continue;
        }

        for (ordinal, candidate) in device_candidates.into_iter().enumerate() {
            let Ok(driver_stream_ordinal) = u8::try_from(ordinal) else {
                continue;
            };
            verified.insert(
                (candidate.adapter, candidate.frontend_index),
                DvbPhysicalExclusiveGroupKey {
                    physical_device_identity: identity.clone(),
                    driver_stream_ordinal,
                },
            );
        }
    }
    verified
}

fn dvb_exclusive_group_ids(
    topology_keys: &BTreeMap<(i32, i32), DvbPhysicalExclusiveGroupKey>,
) -> Option<BTreeMap<(i32, i32), i32>> {
    let unique_keys = topology_keys.values().cloned().collect::<BTreeSet<_>>();
    let mut id_by_key = BTreeMap::new();
    for (payload, key) in unique_keys.into_iter().enumerate() {
        let payload = i32::try_from(payload).ok()?;
        let group_id = DVB_PHYSICAL_GROUP_TAG.checked_add(payload)?;
        id_by_key.insert(key, group_id);
    }

    topology_keys
        .iter()
        .map(|(tuple, key)| Some((*tuple, *id_by_key.get(key)?)))
        .collect()
}

fn dvb_driver_basename(adapter: i32, frontend_index: i32) -> Option<String> {
    let link = PathBuf::from(format!(
        "/sys/class/dvb/dvb{adapter}.frontend{frontend_index}/device/driver"
    ));
    std::fs::read_link(link).ok().and_then(|path| {
        path.file_name()
            .map(|name| name.to_string_lossy().to_string())
    })
}

fn dvb_physical_device_identity(adapter: i32, frontend_index: i32) -> Option<PathBuf> {
    std::fs::canonicalize(format!(
        "/sys/class/dvb/dvb{adapter}.frontend{frontend_index}/device"
    ))
    .ok()
}

fn systems_from_dvb_delsys_buffer(buffer: DtvPropertyBuffer) -> Vec<FrontendSystem> {
    let buffer_data = buffer.read_data_unaligned();
    let count = usize::try_from(buffer.read_len_unaligned())
        .unwrap_or(0)
        .min(buffer_data.len());
    let mut systems = Vec::new();
    for delsys in &buffer_data[..count] {
        match u32::from(*delsys) {
            SYS_ISDBT => systems.push(FrontendSystem::IsdbT),
            SYS_ISDBS => systems.push(FrontendSystem::IsdbS),
            _ => {}
        }
    }
    systems.sort_by_key(|system| match system {
        FrontendSystem::IsdbT => 0,
        FrontendSystem::IsdbS => 1,
        FrontendSystem::IsdbS3 => 2,
        FrontendSystem::DvbS => 3,
    });
    systems.dedup();
    systems
}

fn probe_dvb_delivery_systems(
    path: &PathBuf,
) -> Result<(Vec<FrontendSystem>, DvbFrontendInfo), HalError> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|error| HalError::OpenFailed {
            path: path.clone(),
            detail: HalErrorDetail::new(format!(
                "open DVB frontend for delivery-system probe failed: {error}"
            )),
        })?;
    let fd = file.as_raw_fd();

    let mut info = DvbFrontendInfo {
        name: [0; 128],
        fe_type: 0,
        frequency_min: 0,
        frequency_max: 0,
        frequency_stepsize: 0,
        frequency_tolerance: 0,
        symbol_rate_min: 0,
        symbol_rate_max: 0,
        symbol_rate_tolerance: 0,
        notifier_delay: 0,
        caps: 0,
    };
    // 安全性: `fd` はopen済みDVB frontend fdであり、`info` は呼び出し中に書込み可能なFE_GET_INFO互換C layout構造体を指す。
    let info_rc = unsafe { raw_ioctl_ptr(fd, FE_GET_INFO, &mut info) };
    if info_rc != 0 {
        return Err(HalError::IoctlFailed {
            backend: "dvb",
            path: Some(path.clone()),
            op: "FE_GET_INFO",
            errno: last_errno(),
        });
    }

    let mut prop = DtvProperty {
        cmd: DTV_ENUM_DELSYS,
        reserved: [0; 3],
        u: DtvPropertyUnion {
            buffer: DtvPropertyBuffer {
                data: [0; 32],
                len: 0,
                reserved1: [0; 3],
                reserved2: core::ptr::null_mut(),
            },
        },
        result: 0,
    };
    let mut props = DtvProperties {
        num: 1,
        props: &mut prop,
    };
    // 安全性: `props` は初期化済みunion buffer variantを持つ可変DtvPropertyを指す。kernelはdelivery-system bufferをin-placeで書く。
    let rc = unsafe { raw_ioctl_ptr(fd, FE_GET_PROPERTY, &mut props) };
    if rc != 0 {
        return Err(HalError::IoctlFailed {
            backend: "dvb",
            path: Some(path.clone()),
            op: "DTV_ENUM_DELSYS",
            errno: last_errno(),
        });
    }
    Ok((
        systems_from_dvb_delsys_buffer(prop.read_buffer_unaligned()),
        info,
    ))
}

fn dvb_probe_variants(
    path: &PathBuf,
    exclusive_group_id: i32,
) -> Result<Vec<DvbProbeVariant>, HalError> {
    let (systems, info) = probe_dvb_delivery_systems(path)?;
    Ok(systems
        .into_iter()
        .map(|system| DvbProbeVariant {
            system,
            capability: dvb_capability(&info, system, exclusive_group_id),
        })
        .collect())
}

fn collect_px4_probe_candidates(
    mut path_exists: impl FnMut(&std::path::Path) -> bool,
) -> Vec<(i32, PathBuf, String)> {
    let mut candidates = Vec::new();
    for prefix in PX4_PROBE_PREFIXES {
        for unit in 0..=0x3fff_i32 {
            let name = format!("{prefix}{unit}");
            let path = PathBuf::from(format!("/dev/{name}"));
            if path_exists(&path) {
                candidates.push((unit, path, name));
            }
        }
    }
    candidates.sort_by(|a, b| (a.0, &a.2).cmp(&(b.0, &b.2)));
    candidates
}

fn probe_frontends() -> Vec<FrontendProbeOutcome> {
    let mut outcomes = Vec::new();
    let mut frontend_ids = FrontendIdAllocator::default();

    let px4_candidates = collect_px4_probe_candidates(std::path::Path::exists);
    for (unit, path, name) in px4_candidates {
        for system in px4_frontend_systems(unit, &name) {
            let Some(capability) = px4_capability(unit, &name, system) else {
                outcomes.push(FrontendProbeOutcome::CapabilitySuppressed {
                    backend: FrontendBackendKind::Px4CharDevice,
                    path: path.clone(),
                    reason: CapabilitySuppressionReason::InvalidCapabilityProfile,
                });
                continue;
            };
            let Some(frontend_id) = frontend_ids.allocate() else {
                outcomes.push(FrontendProbeOutcome::CapabilitySuppressed {
                    backend: FrontendBackendKind::Px4CharDevice,
                    path: path.clone(),
                    reason: CapabilitySuppressionReason::RuntimeCapacityExhausted,
                });
                continue;
            };
            let lnb_profile = probe_lnb_profile_for_frontend(
                FrontendBackendKind::Px4CharDevice,
                system,
                &path,
                Some(&name),
            );
            outcomes.push(FrontendProbeOutcome::Available {
                id: frontend_id,
                backend: FrontendBackendKind::Px4CharDevice,
                system,
                path: path.clone(),
                lnb_profile,
                satellite_power_topology: probe_satellite_power_topology(system, lnb_profile),
                capability,
            });
        }
    }

    let mut dvb_candidates = Vec::new();
    for adapter in 0..16 {
        for frontend_index in 0..16 {
            let path = PathBuf::from(format!(
                "/dev/dvb/adapter{adapter}/frontend{frontend_index}"
            ));
            if !path.exists() {
                continue;
            }
            if dvb_driver_basename(adapter, frontend_index).as_deref() != Some("earth-pt1") {
                outcomes.push(FrontendProbeOutcome::CapabilitySuppressed {
                    backend: FrontendBackendKind::LinuxDvb,
                    path,
                    reason: CapabilitySuppressionReason::DeviceFamilyDisabled,
                });
                continue;
            }
            dvb_candidates.push(DvbProbeCandidate {
                adapter,
                frontend_index,
                path,
                physical_device_identity: dvb_physical_device_identity(adapter, frontend_index),
            });
        }
    }

    let topology_keys = earth_pt1_verified_topology_keys(&dvb_candidates);
    let group_ids = dvb_exclusive_group_ids(&topology_keys).unwrap_or_default();
    for candidate in dvb_candidates {
        let DvbProbeCandidate {
            adapter,
            frontend_index,
            path,
            ..
        } = candidate;
        let Some(exclusive_group_id) = group_ids.get(&(adapter, frontend_index)).copied() else {
            outcomes.push(FrontendProbeOutcome::CapabilitySuppressed {
                backend: FrontendBackendKind::LinuxDvb,
                path,
                reason: CapabilitySuppressionReason::InvalidCapabilityProfile,
            });
            continue;
        };
        match dvb_probe_variants(&path, exclusive_group_id) {
            Ok(variants) => {
                if variants.is_empty() {
                    outcomes.push(FrontendProbeOutcome::CapabilitySuppressed {
                        backend: FrontendBackendKind::LinuxDvb,
                        path,
                        reason: CapabilitySuppressionReason::UnsupportedDeliverySystem,
                    });
                } else {
                    for variant in variants {
                        let Some(capability) = variant.capability else {
                            outcomes.push(FrontendProbeOutcome::CapabilitySuppressed {
                                backend: FrontendBackendKind::LinuxDvb,
                                path: path.clone(),
                                reason: CapabilitySuppressionReason::InvalidCapabilityProfile,
                            });
                            continue;
                        };
                        let lnb_profile = probe_lnb_profile_for_frontend(
                            FrontendBackendKind::LinuxDvb,
                            variant.system,
                            &path,
                            None,
                        );
                        let Some(frontend_id) = frontend_ids.allocate() else {
                            outcomes.push(FrontendProbeOutcome::CapabilitySuppressed {
                                backend: FrontendBackendKind::LinuxDvb,
                                path: path.clone(),
                                reason: CapabilitySuppressionReason::RuntimeCapacityExhausted,
                            });
                            continue;
                        };
                        outcomes.push(FrontendProbeOutcome::Available {
                            id: frontend_id,
                            backend: FrontendBackendKind::LinuxDvb,
                            system: variant.system,
                            path: path.clone(),
                            lnb_profile,
                            satellite_power_topology: probe_satellite_power_topology(
                                variant.system,
                                lnb_profile,
                            ),
                            capability,
                        });
                    }
                }
            }
            Err(error) => outcomes.push(FrontendProbeOutcome::DeviceOpenFailed {
                backend: FrontendBackendKind::LinuxDvb,
                path,
                error,
            }),
        }
    }

    outcomes
}

pub fn run_service() {
    binder::ProcessState::start_thread_pool();
    let runtime = match TunerServiceRuntime::try_new() {
        Ok(runtime) => runtime,
        Err(_) => std::process::exit(1),
    };
    let context = crate::service_context::AidlServiceContext::shared(runtime);
    if context
        .reset_runtime_from_probe_results(probe_frontends())
        .is_err()
    {
        std::process::exit(1);
    }
    let tuner = match TunerAidlService::from_context(context) {
        Ok(tuner) => tuner,
        Err(_) => std::process::exit(1),
    };
    let binder = BnTuner::new_binder(tuner, BinderFeatures::default());
    if binder::add_service(TUNER_SERVICE_NAME, binder.as_binder()).is_err() {
        std::process::exit(1);
    }
    binder::ProcessState::join_thread_pool();
}

#[cfg(test)]
mod tests {
    use super::*;
    use maleicacid_tuner_hal2_device::dvb::DtvPropertyBuffer;

    fn collect_prefixes_from_ueventd(text: &str) -> std::collections::BTreeSet<String> {
        text.lines()
            .filter_map(|line| line.split_whitespace().next())
            .filter_map(|path| path.strip_prefix("/dev/"))
            .filter_map(|name| name.split("[0-9]").next())
            .filter(|name| super::PX4_PROBE_PREFIXES.contains(name))
            .map(str::to_string)
            .collect()
    }

    fn collect_prefixes_from_file_contexts(text: &str) -> std::collections::BTreeSet<String> {
        text.lines()
            .filter_map(|line| line.split_whitespace().next())
            .filter_map(|path| path.strip_prefix("/dev/"))
            .filter_map(|name| name.split("[0-9]").next())
            .filter(|name| super::PX4_PROBE_PREFIXES.contains(name))
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn px4_probe_prefixes_match_ueventd_and_file_contexts() {
        let expected: std::collections::BTreeSet<String> = super::PX4_PROBE_PREFIXES
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        let ueventd = include_str!("../../config/ueventd.tuner_hal2.rc");
        let file_contexts = include_str!("../../sepolicy/file_contexts");
        assert_eq!(collect_prefixes_from_ueventd(ueventd), expected);
        assert_eq!(collect_prefixes_from_file_contexts(file_contexts), expected);
        for prefix in super::PX4_PROBE_PREFIXES {
            let ueventd_prefix = format!("/dev/{prefix}[0-9]*");
            let ueventd_line = ueventd
                .lines()
                .find(|line| line.starts_with(ueventd_prefix.as_str()))
                .unwrap();
            assert!(
                ueventd_line.ends_with("0660 media system"),
                "{ueventd_line}"
            );
            let fc_prefix = format!("/dev/{prefix}[0-9]+");
            let fc_line = file_contexts
                .lines()
                .find(|line| line.starts_with(fc_prefix.as_str()))
                .unwrap();
            assert!(
                fc_line.ends_with("u:object_r:px4_tuner_device:s0"),
                "{fc_line}"
            );
        }
    }

    #[test]
    fn dvb_frontend_id_bitpack_avoids_adapter_frontend_collision() {
        let adapter0_frontend5 = dvb_export_frontend_id(0, 5, FrontendSystem::IsdbT).unwrap();
        let adapter1_frontend0 = dvb_export_frontend_id(1, 0, FrontendSystem::IsdbT).unwrap();
        assert_ne!(adapter0_frontend5, adapter1_frontend0);
        assert_eq!(adapter0_frontend5, 2_000_000 + (5_i32 << 4));
        assert_eq!(adapter1_frontend0, 2_000_000 + (1_i32 << 12));
    }

    #[test]
    fn dvb_frontend_id_rejects_out_of_range_fields() {
        assert!(dvb_export_frontend_id(-1, 0, FrontendSystem::IsdbT).is_none());
        assert!(dvb_export_frontend_id(0, -1, FrontendSystem::IsdbT).is_none());
        assert!(dvb_export_frontend_id(256, 0, FrontendSystem::IsdbT).is_none());
        assert!(dvb_export_frontend_id(0, 256, FrontendSystem::IsdbT).is_none());
    }

    #[test]
    fn px4_probe_candidates_use_known_paths_without_directory_enumeration_or_single_node_fallback()
    {
        let present = BTreeSet::from([
            PathBuf::from("/dev/px4video3"),
            PathBuf::from("/dev/pxmlt8video7"),
        ]);
        let candidates = collect_px4_probe_candidates(|path| present.contains(path));

        assert_eq!(
            candidates,
            vec![
                (3, PathBuf::from("/dev/px4video3"), "px4video3".to_string(),),
                (
                    7,
                    PathBuf::from("/dev/pxmlt8video7"),
                    "pxmlt8video7".to_string(),
                ),
            ]
        );
        assert!(collect_px4_probe_candidates(|_| false).is_empty());
    }

    #[test]
    fn px4_frontend_systems_follow_driver_character_device_contract() {
        assert_eq!(
            px4_frontend_systems(0, "px4video0"),
            vec![FrontendSystem::IsdbS]
        );
        assert_eq!(
            px4_frontend_systems(1, "px4video1"),
            vec![FrontendSystem::IsdbS]
        );
        assert_eq!(
            px4_frontend_systems(2, "px4video2"),
            vec![FrontendSystem::IsdbT]
        );
        assert_eq!(
            px4_frontend_systems(3, "px4video3"),
            vec![FrontendSystem::IsdbT]
        );
        assert_eq!(
            px4_frontend_systems(4, "px4video4"),
            vec![FrontendSystem::IsdbS]
        );
        for name in [
            "pxmlt5video0",
            "pxmlt8video7",
            "isdb6014video0",
            "isdb2056video0",
            "pxm1urvideo0",
        ] {
            assert_eq!(
                px4_frontend_systems(0, name),
                vec![FrontendSystem::IsdbT, FrontendSystem::IsdbS]
            );
        }
        for name in ["pxs1urvideo0", "isdbt2071video0"] {
            assert_eq!(px4_frontend_systems(0, name), vec![FrontendSystem::IsdbT]);
        }
        assert!(px4_frontend_systems(0, "unknown0").is_empty());
    }

    #[test]
    fn frontend_id_allocator_stays_within_android15_resource_handle_domain() {
        let mut allocator = FrontendIdAllocator::default();
        for expected in 0..=ANDROID15_TRM_RESOURCE_ID_MAX {
            assert_eq!(allocator.allocate(), Some(FrontendRuntimeId(expected)));
        }
        assert_eq!(allocator.allocate(), None);
    }

    fn candidate(adapter: i32, device: &str) -> DvbProbeCandidate {
        DvbProbeCandidate {
            adapter,
            frontend_index: 0,
            path: PathBuf::from(format!("/dev/dvb/adapter{adapter}/frontend0")),
            physical_device_identity: Some(PathBuf::from(device)),
        }
    }

    fn earth_pt1_frontend_info() -> DvbFrontendInfo {
        DvbFrontendInfo {
            name: [0; 128],
            fe_type: 0,
            frequency_min: 950_000,
            frequency_max: 2_150_000,
            frequency_stepsize: 0,
            frequency_tolerance: 0,
            symbol_rate_min: 0,
            symbol_rate_max: 0,
            symbol_rate_tolerance: 0,
            notifier_delay: 0,
            caps: 0,
        }
    }

    #[test]
    fn earth_pt1_isdbs_uses_fixed_rate_when_fe_get_info_reports_zero_range() {
        let capability = dvb_capability(
            &earth_pt1_frontend_info(),
            FrontendSystem::IsdbS,
            DVB_PHYSICAL_GROUP_TAG,
        )
        .expect("the pinned earth-pt1 profile must publish ISDB-S");

        assert_eq!(capability.scalar.min_symbol_rate, ISDBS_SYMBOL_RATE);
        assert_eq!(capability.scalar.max_symbol_rate, ISDBS_SYMBOL_RATE);
    }

    #[test]
    fn earth_pt1_complete_profile_proves_four_independent_stream_groups() {
        let candidates = (0..4)
            .map(|adapter| candidate(adapter, "/sys/devices/pci0000:00/0000:03:00.0"))
            .collect::<Vec<_>>();
        let topology = earth_pt1_verified_topology_keys(&candidates);
        let group_ids = dvb_exclusive_group_ids(&topology).unwrap();

        assert_eq!(group_ids.len(), 4);
        assert_eq!(
            group_ids.values().copied().collect::<BTreeSet<_>>().len(),
            4
        );
        assert!(group_ids
            .values()
            .all(|group| group & 0xf000_0000 == DVB_PHYSICAL_GROUP_TAG));
        assert!(group_ids
            .values()
            .all(|group| group & 0xf000_0000 != PX4_PHYSICAL_GROUP_TAG));
    }

    #[test]
    fn topology_group_key_controls_sharing_independently_of_public_tuple() {
        let shared_key = DvbPhysicalExclusiveGroupKey {
            physical_device_identity: PathBuf::from("/sys/devices/shared"),
            driver_stream_ordinal: 0,
        };
        let independent_key = DvbPhysicalExclusiveGroupKey {
            physical_device_identity: PathBuf::from("/sys/devices/independent"),
            driver_stream_ordinal: 0,
        };
        let topology = BTreeMap::from([
            ((0, 0), shared_key.clone()),
            ((1, 0), shared_key),
            ((2, 0), independent_key),
        ]);
        let group_ids = dvb_exclusive_group_ids(&topology).unwrap();

        assert_eq!(group_ids[&(0, 0)], group_ids[&(1, 0)]);
        assert_ne!(group_ids[&(0, 0)], group_ids[&(2, 0)]);
    }

    #[test]
    fn incomplete_or_unknown_earth_pt1_topology_is_not_published() {
        let candidates = (0..3)
            .map(|adapter| candidate(adapter, "/sys/devices/pci0000:00/0000:03:00.0"))
            .collect::<Vec<_>>();
        let topology = earth_pt1_verified_topology_keys(&candidates);

        assert!(topology.is_empty());
        assert!(dvb_exclusive_group_ids(&topology).unwrap().is_empty());
    }

    #[test]
    fn delivery_system_buffer_keeps_only_product_supported_isdb_systems() {
        let mut data = [0u8; 32];
        data[0] = u8::try_from(SYS_ISDBS).unwrap_or(0);
        data[1] = 6; // SYS_DVBS2, ignored by this product profile.
        data[2] = u8::try_from(SYS_ISDBT).unwrap_or(0);
        data[3] = u8::try_from(SYS_ISDBS).unwrap_or(0);
        let systems = systems_from_dvb_delsys_buffer(DtvPropertyBuffer {
            data,
            len: 4,
            reserved1: [0; 3],
            reserved2: core::ptr::null_mut(),
        });
        assert_eq!(systems, vec![FrontendSystem::IsdbT, FrontendSystem::IsdbS]);
    }
}

use std::fs::OpenOptions;
use std::os::fd::AsRawFd;
use std::path::PathBuf;

use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::ITuner::BnTuner;
use binder::BinderFeatures;
use maleicacid_tuner_hal2_common::os_abi::{ioctl, last_errno};
use maleicacid_tuner_hal2_common::{
    japan_isdbt_frequency_contract_range_hz, FrontendBackendKind, FrontendSystem, HalError,
    HalErrorDetail, TUNER_SERVICE_NAME,
};
use maleicacid_tuner_hal2_device::dvb::{
    DtvProperties, DtvProperty, DtvPropertyBuffer, DtvPropertyUnion, DvbFrontendInfo,
    DTV_ENUM_DELSYS, FE_GET_INFO, FE_GET_PROPERTY, SYS_ISDBS, SYS_ISDBT,
};
use maleicacid_tuner_hal2_service_runtime::{
    CapabilitySuppressionReason, FrontendCapabilitySnapshot, FrontendProbeOutcome,
    FrontendRuntimeId, FrontendScalarCapability, IsdbtSegmentCapability, LnbRegistryProfile,
    TunerServiceRuntime,
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

fn px4_export_frontend_base_id(unit: i32, device_name: &str) -> Option<i32> {
    if unit < 0 {
        return None;
    }
    let family = px4_device_family_code(device_name);
    1_000_000i32
        .checked_add(family.checked_mul(10_000)?)
        .and_then(|base| base.checked_add(unit.checked_mul(10)?))
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

#[derive(Clone, Debug, Eq, PartialEq)]
struct DvbProbeVariant {
    id: i32,
    system: FrontendSystem,
    capability: Option<FrontendCapabilitySnapshot>,
}

const JAPAN_BS_FIRST_IF_HZ: i64 = 1_049_480_000;
const JAPAN_CS110_LAST_IF_HZ: i64 = 2_053_000_000;
const ISDBS_SYMBOL_RATE: i32 = 28_860_000;
const PX4_PHYSICAL_GROUP_TAG: i32 = 0x1000_0000;
const DVB_SHARED_PHYSICAL_GROUP_ID: i32 = 0x2000_0000;

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
            let (min_frequency_hz, max_frequency_hz, _) =
                japan_isdbt_frequency_contract_range_hz();
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
            if probed_min > JAPAN_BS_FIRST_IF_HZ
                || probed_max < JAPAN_CS110_LAST_IF_HZ
                || info.symbol_rate_min == 0
                || info.symbol_rate_min > info.symbol_rate_max
            {
                return None;
            }
            let min_symbol_rate = i32::try_from(info.symbol_rate_min).ok()?;
            let max_symbol_rate = i32::try_from(info.symbol_rate_max).ok()?;
            if min_symbol_rate > ISDBS_SYMBOL_RATE || max_symbol_rate < ISDBS_SYMBOL_RATE {
                return None;
            }
            (
                FrontendScalarCapability {
                    min_frequency_hz: JAPAN_BS_FIRST_IF_HZ,
                    max_frequency_hz: JAPAN_CS110_LAST_IF_HZ,
                    min_symbol_rate,
                    max_symbol_rate,
                    acquire_range_hz: 0,
                },
                None,
            )
        }
        FrontendSystem::IsdbS3 | FrontendSystem::DvbS => return None,
    };
    Some(FrontendCapabilitySnapshot {
        scalar,
        // earth-pt1の独立RF経路を識別するprobe証跡がないため、
        // 保守的に共有groupとする。
        exclusive_group_id: DVB_SHARED_PHYSICAL_GROUP_ID,
        isdbt_segment,
    })
}

fn dvb_export_frontend_id(
    adapter: i32,
    frontend_index: i32,
    system: FrontendSystem,
) -> Option<i32> {
    if !(0..=255).contains(&adapter) || !(0..=255).contains(&frontend_index) {
        return None;
    }
    let variant = match system {
        FrontendSystem::IsdbT => 0,
        FrontendSystem::IsdbS => 1,
        _ => return None,
    };
    2_000_000_i32
        .checked_add(adapter.checked_shl(12)?)
        .and_then(|base| base.checked_add(frontend_index.checked_shl(4)?))
        .and_then(|base| base.checked_add(variant))
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
    let info_rc = unsafe { ioctl(fd, FE_GET_INFO, &mut info) };
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
    let rc = unsafe { ioctl(fd, FE_GET_PROPERTY, &mut props) };
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
    adapter: i32,
    frontend_index: i32,
    path: &PathBuf,
) -> Result<Vec<DvbProbeVariant>, HalError> {
    let (systems, info) = probe_dvb_delivery_systems(path)?;
    let mut variants = Vec::new();
    for system in systems {
        if let Some(id) = dvb_export_frontend_id(adapter, frontend_index, system) {
            variants.push(DvbProbeVariant {
                id,
                system,
                capability: dvb_capability(&info, system),
            });
        }
    }
    Ok(variants)
}

fn probe_frontends() -> Vec<FrontendProbeOutcome> {
    let mut outcomes = Vec::new();

    let mut px4_candidates: Vec<(i32, PathBuf, String)> = Vec::new();
    if let Ok(dir) = std::fs::read_dir("/dev") {
        for entry in dir.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            for prefix in PX4_PROBE_PREFIXES {
                let Some(idx) = name.strip_prefix(prefix) else {
                    continue;
                };
                let Ok(index) = idx.parse::<i32>() else {
                    continue;
                };
                px4_candidates.push((index, entry.path(), name.to_string()));
            }
        }
    }
    if px4_candidates.is_empty() && PathBuf::from("/dev/px4video0").exists() {
        px4_candidates.push((0, PathBuf::from("/dev/px4video0"), "px4video0".to_string()));
    }
    px4_candidates.sort_by(|a, b| (a.0, &a.2).cmp(&(b.0, &b.2)));
    px4_candidates.dedup_by(|a, b| a.1 == b.1);
    for (unit, path, name) in px4_candidates {
        let Some(base_id) = px4_export_frontend_base_id(unit, &name) else {
            continue;
        };
        let Some(isdbt_capability) = px4_capability(unit, &name, FrontendSystem::IsdbT) else {
            outcomes.push(FrontendProbeOutcome::CapabilitySuppressed {
                backend: FrontendBackendKind::Px4CharDevice,
                path,
                reason: CapabilitySuppressionReason::InvalidCapabilityProfile,
            });
            continue;
        };
        outcomes.push(FrontendProbeOutcome::Available {
            id: FrontendRuntimeId(base_id),
            backend: FrontendBackendKind::Px4CharDevice,
            system: FrontendSystem::IsdbT,
            path: path.clone(),
            lnb_profile: probe_lnb_profile_for_frontend(
                FrontendBackendKind::Px4CharDevice,
                FrontendSystem::IsdbT,
                &path,
                Some(&name),
            ),
            capability: isdbt_capability,
        });
        if let Some(isdbs_id) = base_id.checked_add(1) {
            let Some(isdbs_capability) = px4_capability(unit, &name, FrontendSystem::IsdbS) else {
                outcomes.push(FrontendProbeOutcome::CapabilitySuppressed {
                    backend: FrontendBackendKind::Px4CharDevice,
                    path: path.clone(),
                    reason: CapabilitySuppressionReason::InvalidCapabilityProfile,
                });
                continue;
            };
            outcomes.push(FrontendProbeOutcome::Available {
                id: FrontendRuntimeId(isdbs_id),
                backend: FrontendBackendKind::Px4CharDevice,
                system: FrontendSystem::IsdbS,
                path: path.clone(),
                lnb_profile: probe_lnb_profile_for_frontend(
                    FrontendBackendKind::Px4CharDevice,
                    FrontendSystem::IsdbS,
                    &path,
                    Some(&name),
                ),
                capability: isdbs_capability,
            });
        }
    }

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
            match dvb_probe_variants(adapter, frontend_index, &path) {
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
                            outcomes.push(FrontendProbeOutcome::Available {
                                id: FrontendRuntimeId(variant.id),
                                backend: FrontendBackendKind::LinuxDvb,
                                system: variant.system,
                                path: path.clone(),
                                lnb_profile: probe_lnb_profile_for_frontend(
                                    FrontendBackendKind::LinuxDvb,
                                    variant.system,
                                    &path,
                                    None,
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

    #[test]
    fn dvb_export_ids_keep_isdb_t_and_isdb_s_as_distinct_variants() {
        assert_eq!(
            dvb_export_frontend_id(0, 0, FrontendSystem::IsdbT),
            Some(2_000_000)
        );
        assert_eq!(
            dvb_export_frontend_id(0, 0, FrontendSystem::IsdbS),
            Some(2_000_001)
        );
        assert_eq!(
            dvb_export_frontend_id(1, 2, FrontendSystem::IsdbT),
            Some(2_004_128)
        );
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

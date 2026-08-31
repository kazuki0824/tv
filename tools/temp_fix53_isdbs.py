from pathlib import Path


def replace_once(path, old, new):
    p = Path(path)
    s = p.read_text()
    if s.count(old) != 1:
        raise SystemExit(f"{path}: expected one anchor, got {s.count(old)}")
    p.write_text(s.replace(old, new, 1))

# Canonical Android-Hz -> fixed Japan BS/CS110 raster adapter. The acceptance
# radius is derived from half the adjacent-channel spacing, not from CTS or a
# guessed hardware acquisition tolerance. Thus every accepted Hz value maps to
# at most one channel center; the large BS/CS gap remains rejected.
p = Path("tuner_hal2/common/src/lib.rs")
s = p.read_text()
anchor = "pub const ARIB_TDT_SECTION_LENGTH: usize = 5;\n"
addition = anchor + r'''
pub const JAPAN_BS_FIRST_IF_HZ: u64 = 1_049_480_000;
pub const JAPAN_BS_IF_STEP_HZ: u64 = 38_360_000;
pub const JAPAN_BS_CHANNEL_COUNT: u64 = 12;
pub const JAPAN_CS110_FIRST_IF_HZ: u64 = 1_613_000_000;
pub const JAPAN_CS110_IF_STEP_HZ: u64 = 40_000_000;
pub const JAPAN_CS110_CHANNEL_COUNT: u64 = 12;

fn normalize_frequency_to_discrete_raster(
    frequency_hz: u64,
    first_hz: u64,
    step_hz: u64,
    count: u64,
) -> Option<u64> {
    if count == 0 || step_hz == 0 {
        return None;
    }
    let last = first_hz.checked_add(step_hz.checked_mul(count.checked_sub(1)?)?)?;
    let half_step = step_hz / 2;
    if frequency_hz < first_hz.saturating_sub(half_step)
        || frequency_hz > last.checked_add(half_step)?
    {
        return None;
    }
    let index = if frequency_hz <= first_hz {
        0
    } else {
        frequency_hz
            .checked_sub(first_hz)?
            .checked_add(half_step)?
            / step_hz
    };
    if index >= count {
        return None;
    }
    let center = first_hz.checked_add(step_hz.checked_mul(index)?)?;
    // For an exact half-step tie, the integer nearest-index rule above chooses
    // the upper channel. Every non-tie accepted value has one nearest center.
    (frequency_hz.abs_diff(center) <= half_step).then_some(center)
}

pub fn normalize_japan_bs_if_frequency_hz(frequency_hz: u64) -> Option<u64> {
    normalize_frequency_to_discrete_raster(
        frequency_hz,
        JAPAN_BS_FIRST_IF_HZ,
        JAPAN_BS_IF_STEP_HZ,
        JAPAN_BS_CHANNEL_COUNT,
    )
}

pub fn normalize_japan_cs110_if_frequency_hz(frequency_hz: u64) -> Option<u64> {
    normalize_frequency_to_discrete_raster(
        frequency_hz,
        JAPAN_CS110_FIRST_IF_HZ,
        JAPAN_CS110_IF_STEP_HZ,
        JAPAN_CS110_CHANNEL_COUNT,
    )
}
'''
if s.count(anchor) != 1:
    raise SystemExit("common constants anchor mismatch")
s = s.replace(anchor, addition, 1)
if "isdbs_android_hz_adapter_accepts_cts_nominal_frequency_without_special_case" not in s:
    s += r'''

#[cfg(test)]
mod isdbs_raster_adapter_tests {
    use super::*;

    #[test]
    fn isdbs_android_hz_adapter_accepts_cts_nominal_frequency_without_special_case() {
        assert_eq!(normalize_japan_bs_if_frequency_hz(1_050_000_000), Some(1_049_480_000));
    }

    #[test]
    fn isdbs_raster_adapter_is_bounded_by_half_adjacent_spacing_and_rejects_band_gap() {
        let bs_half = JAPAN_BS_IF_STEP_HZ / 2;
        assert_eq!(
            normalize_japan_bs_if_frequency_hz(JAPAN_BS_FIRST_IF_HZ + bs_half),
            Some(JAPAN_BS_FIRST_IF_HZ + JAPAN_BS_IF_STEP_HZ)
        );
        assert!(normalize_japan_bs_if_frequency_hz(JAPAN_BS_FIRST_IF_HZ - bs_half - 1).is_none());
        assert!(normalize_japan_bs_if_frequency_hz(1_550_000_000).is_none());
        assert!(normalize_japan_cs110_if_frequency_hz(1_550_000_000).is_none());
    }
}
'''
p.write_text(s)

# Service validation uses the same normalizer as backend projection.
replace_once(
    "tuner_hal2/service_runtime/src/frontend_request_txn.rs",
    """use maleicacid_tuner_hal2_common::{
    is_japan_bs_if_frequency_hz, is_japan_cs110_if_frequency_hz,
    is_japan_isdbt_frequency_contract_hz, FrontendBackendKind,
""",
    """use maleicacid_tuner_hal2_common::{
    is_japan_isdbt_frequency_contract_hz, normalize_japan_bs_if_frequency_hz,
    normalize_japan_cs110_if_frequency_hz, FrontendBackendKind,
""",
)
replace_once(
    "tuner_hal2/service_runtime/src/frontend_request_txn.rs",
    """            let is_bs = is_japan_bs_if_frequency_hz(request.frequency);
            let is_cs110 = is_japan_cs110_if_frequency_hz(request.frequency);
            if !is_bs && !is_cs110 {
                return Err(HalError::invalid_argument(
                    HalInvalidArgumentKind::UnsupportedFrequency,
                    "ISDB-S frequency must be a Japan BS/CS110 IF center frequency",
                ));
            }
""",
    """            let is_bs = normalize_japan_bs_if_frequency_hz(request.frequency).is_some();
            let is_cs110 = normalize_japan_cs110_if_frequency_hz(request.frequency).is_some();
            if !is_bs && !is_cs110 {
                return Err(HalError::invalid_argument(
                    HalInvalidArgumentKind::UnsupportedFrequency,
                    "ISDB-S frequency cannot be normalized unambiguously to the Japan BS/CS110 raster",
                ));
            }
""",
)

# px4 mapping consumes the canonical normalized center, then converts center->freq_no.
p = Path("tuner_hal2/device/src/px4/tune_mapping.rs")
s = p.read_text()
s = s.replace(
    "    FrontendStreamIdKind, FrontendSystem, FrontendTuneRequest, HalError, HalInvalidArgumentKind,\n",
    "    normalize_japan_bs_if_frequency_hz, normalize_japan_cs110_if_frequency_hz,\n    FrontendStreamIdKind, FrontendSystem, FrontendTuneRequest, HalError, HalInvalidArgumentKind,\n",
    1,
)
start = s.index("pub fn map_bs_if_frequency_to_px4_freq_no")
mid = s.index("pub fn map_cs110_if_frequency_to_px4_freq_no", start)
end = s.index("pub fn map_relative_stream_number_to_px4_slot", mid)
replacement = r'''pub fn map_bs_if_frequency_to_px4_freq_no(if_hz: u64) -> Result<i32, HalError> {
    let center = normalize_japan_bs_if_frequency_hz(if_hz).ok_or_else(|| {
        HalError::invalid_argument(
            HalInvalidArgumentKind::UnsupportedFrequency,
            "px4 BS IF frequency is outside the unambiguous Japan raster domain",
        )
    })?;
    let delta = center - PX4_BS_BASE_IF_HZ;
    let freq_no = PX4_BS_FREQ_NO_MIN
        + i32::try_from(delta / PX4_BS_STEP_HZ).map_err(|_| {
            HalError::invalid_argument(
                HalInvalidArgumentKind::UnsupportedFrequency,
                "px4 BS IF frequency is unsupported",
            )
        })?;
    (PX4_BS_FREQ_NO_MIN..=PX4_BS_FREQ_NO_MAX)
        .contains(&freq_no)
        .then_some(freq_no)
        .ok_or_else(|| {
            HalError::invalid_argument(
                HalInvalidArgumentKind::UnsupportedFrequency,
                "px4 BS IF frequency is unsupported",
            )
        })
}

pub fn map_cs110_if_frequency_to_px4_freq_no(if_hz: u64) -> Result<i32, HalError> {
    let center = normalize_japan_cs110_if_frequency_hz(if_hz).ok_or_else(|| {
        HalError::invalid_argument(
            HalInvalidArgumentKind::UnsupportedFrequency,
            "px4 CS110 IF frequency is outside the unambiguous Japan raster domain",
        )
    })?;
    let delta = center - PX4_CS_BASE_IF_HZ;
    let freq_no = PX4_CS_FREQ_NO_MIN
        + i32::try_from(delta / PX4_CS_STEP_HZ).map_err(|_| {
            HalError::invalid_argument(
                HalInvalidArgumentKind::UnsupportedFrequency,
                "px4 CS110 IF frequency is unsupported",
            )
        })?;
    (PX4_CS_FREQ_NO_MIN..=PX4_CS_FREQ_NO_MAX)
        .contains(&freq_no)
        .then_some(freq_no)
        .ok_or_else(|| {
            HalError::invalid_argument(
                HalInvalidArgumentKind::UnsupportedFrequency,
                "px4 CS110 IF frequency is unsupported",
            )
        })
}

'''
s = s[:start] + replacement + s[end:]
s = s.replace(
    "            let band = if is_japan_cs110_if_frequency_range_hz(request.frequency) {\n",
    "            let band = if normalize_japan_cs110_if_frequency_hz(request.frequency).is_some() {\n",
    1,
)
# old broad range helper is no longer needed.
old_helper = '''fn is_japan_cs110_if_frequency_range_hz(if_hz: u64) -> bool {
    let last =
        PX4_CS_BASE_IF_HZ + PX4_CS_STEP_HZ * ((PX4_CS_FREQ_NO_MAX - PX4_CS_FREQ_NO_MIN) as u64);
    (PX4_CS_BASE_IF_HZ..=last).contains(&if_hz)
}

'''
s = s.replace(old_helper, "", 1)
if "android_isdbs_frequency_is_normalized_to_px4_raster" not in s:
    s += r'''

#[cfg(test)]
mod android_isdbs_frequency_adapter_tests {
    use super::*;

    #[test]
    fn android_isdbs_frequency_is_normalized_to_px4_raster() {
        assert_eq!(map_bs_if_frequency_to_px4_freq_no(1_050_000_000), Ok(0));
        assert_eq!(map_bs_if_frequency_to_px4_freq_no(PX4_BS_BASE_IF_HZ + PX4_BS_STEP_HZ), Ok(1));
        assert_eq!(map_cs110_if_frequency_to_px4_freq_no(PX4_CS_BASE_IF_HZ), Ok(12));
        assert!(map_bs_if_frequency_to_px4_freq_no(1_550_000_000).is_err());
        assert!(map_cs110_if_frequency_to_px4_freq_no(1_550_000_000).is_err());
    }
}
'''
p.write_text(s)

# Repo SSOT: the scalar is a bounding envelope; discrete-raster acceptance is
# derived from the same immutable product profile, with no CTS-only exception.
p = Path("tuner_hal/DESIGN_JA.md")
s = p.read_text()
old = "| T-AOSP-44 | `FrontendInfo` scalar境界とtune validation | min/max frequency、symbol rate、acquire rangeが同一`CapabilitySnapshot`と受付範囲に一致 |"
new = "| T-AOSP-44 | `FrontendInfo` scalar境界とtune validation | min/max frequency、symbol rate、acquire rangeは同一`CapabilitySnapshot`を正本とする。固定離散raster backendのfrequencyはscalar envelopeに加えて同snapshotのraster profileを適用し、Android Hz要求を隣接center間の半間隔以内で一意なnearest centerへ正規化する。BS/CS間gap、range外、曖昧化できない値は副作用前に拒否し、CTS個別値の特例を置かない |"
if s.count(old) != 1:
    raise SystemExit(f"T-AOSP-44 anchor mismatch: {s.count(old)}")
s = s.replace(old, new, 1)
p.write_text(s)

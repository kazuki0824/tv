from pathlib import Path


def replace_once(path, old, new):
    p = Path(path)
    text = p.read_text()
    n = text.count(old)
    if n != 1:
        raise SystemExit(f"{path}: expected one anchor, got {n}")
    p.write_text(text.replace(old, new, 1))

# The Android-Hz -> px4 discrete-channel conversion belongs to the px4
# adapter. Remove the cross-crate common helper that made the host wrapper's
# same-lib-name Cargo topology ambiguous for clippy/test builds.
p = Path("tuner_hal2/common/src/lib.rs")
text = p.read_text()
start = text.index("pub const JAPAN_BS_FIRST_IF_HZ: u64 = 1_049_480_000;")
end = text.index("pub const MAX_ARIB_SECTION_TOTAL_BYTES", start)
text = text[:start] + text[end:]
marker = "#[cfg(test)]\nmod isdbs_raster_adapter_tests {"
start = text.index(marker)
text = text[:start].rstrip() + "\n"
p.write_text(text)

# Make px4 adapter the one physical owner of this normalization contract.
replace_once(
    "tuner_hal2/device/src/px4/tune_mapping.rs",
    """use maleicacid_tuner_hal2_common::{
    normalize_japan_bs_if_frequency_hz, normalize_japan_cs110_if_frequency_hz,
    FrontendStreamIdKind, FrontendSystem, FrontendTuneRequest, HalError, HalInvalidArgumentKind,
};
""",
    """use maleicacid_tuner_hal2_common::{
    FrontendStreamIdKind, FrontendSystem, FrontendTuneRequest, HalError, HalInvalidArgumentKind,
};
""",
)
replace_once(
    "tuner_hal2/device/src/px4/tune_mapping.rs",
    """const PX4_CS_BASE_IF_HZ: u64 = 1_613_000_000;
const PX4_CS_STEP_HZ: u64 = 40_000_000;
const PX4_CS_FREQ_NO_MIN: i32 = 12;
const PX4_CS_FREQ_NO_MAX: i32 = 23;

""",
    """const PX4_CS_BASE_IF_HZ: u64 = 1_613_000_000;
const PX4_CS_STEP_HZ: u64 = 40_000_000;
const PX4_CS_FREQ_NO_MIN: i32 = 12;
const PX4_CS_FREQ_NO_MAX: i32 = 23;

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
    if frequency_hz <= first_hz {
        return Some(first_hz);
    }
    if frequency_hz >= last {
        return Some(last);
    }
    let delta = frequency_hz.checked_sub(first_hz)?;
    let lower_index = delta / step_hz;
    let remainder = delta % step_hz;
    if step_hz % 2 == 0 && remainder == half_step {
        return None;
    }
    let index = lower_index + u64::from(remainder > half_step);
    if index >= count {
        return None;
    }
    first_hz.checked_add(step_hz.checked_mul(index)?)
}

pub fn normalize_japan_bs_if_frequency_hz(frequency_hz: u64) -> Option<u64> {
    normalize_frequency_to_discrete_raster(
        frequency_hz,
        PX4_BS_BASE_IF_HZ,
        PX4_BS_STEP_HZ,
        u64::try_from(PX4_BS_FREQ_NO_MAX - PX4_BS_FREQ_NO_MIN + 1).ok()?,
    )
}

pub fn normalize_japan_cs110_if_frequency_hz(frequency_hz: u64) -> Option<u64> {
    normalize_frequency_to_discrete_raster(
        frequency_hz,
        PX4_CS_BASE_IF_HZ,
        PX4_CS_STEP_HZ,
        u64::try_from(PX4_CS_FREQ_NO_MAX - PX4_CS_FREQ_NO_MIN + 1).ok()?,
    )
}

""",
)

replace_once(
    "tuner_hal2/device/src/px4/mod.rs",
    """    map_bs_if_frequency_to_px4_freq_no, map_cs110_if_frequency_to_px4_freq_no,
    map_isdbt_frequency_to_px4, map_tune_request_to_px4, px4_scan_requests,
    reportable_bs_tsid_for_scan, Px4SatBand, Px4TuneRequest,
""",
    """    map_bs_if_frequency_to_px4_freq_no, map_cs110_if_frequency_to_px4_freq_no,
    map_isdbt_frequency_to_px4, map_tune_request_to_px4, normalize_japan_bs_if_frequency_hz,
    normalize_japan_cs110_if_frequency_hz, px4_scan_requests, reportable_bs_tsid_for_scan,
    Px4SatBand, Px4TuneRequest,
""",
)

# Service validation consumes the same px4 adapter entry as backend preflight.
replace_once(
    "tuner_hal2/service_runtime/src/frontend_request_txn.rs",
    """use maleicacid_tuner_hal2_common::{
    is_japan_isdbt_frequency_contract_hz, normalize_japan_bs_if_frequency_hz,
    normalize_japan_cs110_if_frequency_hz, FrontendBackendKind,
""",
    """use maleicacid_tuner_hal2_common::{
    is_japan_isdbt_frequency_contract_hz, FrontendBackendKind,
""",
)
p = Path("tuner_hal2/service_runtime/src/frontend_request_txn.rs")
text = p.read_text()
text = text.replace(
    "normalize_japan_bs_if_frequency_hz(request.frequency)",
    "maleicacid_tuner_hal2_device::px4::normalize_japan_bs_if_frequency_hz(request.frequency)",
)
text = text.replace(
    "normalize_japan_cs110_if_frequency_hz(request.frequency)",
    "maleicacid_tuner_hal2_device::px4::normalize_japan_cs110_if_frequency_hz(request.frequency)",
)
p.write_text(text)

# doctest suppression was diagnostic-only; with the crate API dependency removed,
# retain the original host wrapper contract.
replace_once(
    "tuner_hal2/host_ci/device/Cargo.toml",
    """[lib]
name = "maleicacid_tuner_hal2_device"
path = "../../device/src/lib.rs"
doctest = false
""",
    """[lib]
name = "maleicacid_tuner_hal2_device"
path = "../../device/src/lib.rs"
""",
)

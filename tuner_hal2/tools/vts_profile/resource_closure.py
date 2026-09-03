from __future__ import annotations

import re
import subprocess
import tempfile
from pathlib import Path
from typing import Any

from .model import ProfileError

_REPO_ROOT = Path(__file__).resolve().parents[3]
DEFAULT_CAPABILITY_SOURCE = _REPO_ROOT / "tuner_hal2/service_runtime/src/capability_snapshot.rs"
DEFAULT_FILTER_CONFIG_SOURCE = _REPO_ROOT / "tuner_hal2/demux/src/config.rs"
DEFAULT_PES_SOURCE = _REPO_ROOT / "tuner_hal2/demux/src/parser/ts_core.rs"


def _pes_max_bytes(path: Path) -> int:
    try:
        text = path.read_text(encoding="utf-8")
    except OSError as exc:
        raise ProfileError(f"failed to read PES source {path}: {exc}") from exc
    match = re.search(r"pub const MAX_PES_BUFFER_BYTES:\s*usize\s*=\s*([^;]+);", text)
    if not match:
        raise ProfileError("MAX_PES_BUFFER_BYTES was not found")
    expression = match.group(1).replace("_", "").strip()
    factors = [part.strip() for part in expression.split("*")]
    if not factors or any(not factor.isdigit() for factor in factors):
        raise ProfileError(f"unsupported MAX_PES_BUFFER_BYTES expression: {expression}")
    value = 1
    for factor in factors:
        value *= int(factor)
    return value


def _i32_buffer(value: Any, name: str) -> int:
    value = int(value)
    if value <= 0 or value > 0x7FFF_FFFF:
        raise ProfileError(f"{name} must fit the positive AIDL i32 buffer-size domain")
    return value


def _read_filter_config_source(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except OSError as exc:
        raise ProfileError(f"failed to read filter config source {path}: {exc}") from exc


def _program(
    profile: dict[str, Any],
    capability_source: Path,
    pes_max: int,
    filter_config_source: Path = DEFAULT_FILTER_CONFIG_SOURCE,
) -> str:
    flows = profile["flows"]
    queues = profile["queues"]
    record = flows["record"]["enabled"]
    live = flows["clear_live"]["enabled"]
    lines: list[str] = []
    if record:
        filter_bytes = _i32_buffer(queues["record_filter_bytes"], "queues.record_filter_bytes")
        dvr_bytes = _i32_buffer(queues["record_dvr_bytes"], "queues.record_dvr_bytes")
        lines.extend([
            'require_capacity(snapshot.filter_capacity(FilterOpenType::TsRecord), 1, "TS record filter")?;',
            f'ledger.reserve_filter(snapshot, 1, FilterOpenType::TsRecord, {filter_bytes}).map_err(debug_error)?;',
            'require_capacity(snapshot.num_record, 1, "record DVR")?;',
            f'ledger.reserve_dvr(snapshot, 1, {dvr_bytes}).map_err(debug_error)?;',
        ])
    if live:
        audio_bytes = _i32_buffer(queues["audio_filter_bytes"], "queues.audio_filter_bytes")
        video_bytes = _i32_buffer(queues["video_filter_bytes"], "queues.video_filter_bytes")
        lines.extend([
            'require_capacity(snapshot.filter_capacity(FilterOpenType::TsAudio), 1, "audio filter")?;',
            f'ledger.reserve_filter(snapshot, 2, FilterOpenType::TsAudio, {audio_bytes}).map_err(debug_error)?;',
            'require_capacity(snapshot.filter_capacity(FilterOpenType::TsVideo), 1, "video filter")?;',
            f'ledger.reserve_filter(snapshot, 3, FilterOpenType::TsVideo, {video_bytes}).map_err(debug_error)?;',
        ])
    demux_demand = 1 if record or live else 0
    operations = "\n        ".join(lines)
    capability = str(capability_source.resolve()).replace("\\", "\\\\")
    filter_config_text = _read_filter_config_source(filter_config_source)
    return f'''extern crate self as maleicacid_tuner_hal2_common;
extern crate self as maleicacid_tuner_hal2_demux;

#[derive(Clone, Debug)]
pub struct HalError;
#[derive(Clone, Copy, Debug)]
pub enum HalInternalKind {{ InvariantViolation }}
#[derive(Clone, Copy, Debug)]
pub enum HalInvalidArgumentKind {{ NumericRange }}
impl HalError {{
    pub fn internal(_kind: HalInternalKind, _detail: impl Into<String>) -> Self {{ Self }}
    pub fn unsupported_detail(_domain: impl Into<String>, _detail: impl Into<String>) -> Self {{ Self }}
    pub fn invalid_argument(_kind: HalInvalidArgumentKind, _detail: impl Into<String>) -> Self {{ Self }}
    pub fn cleanup_failed(_domain: impl Into<String>, _detail: impl Into<String>) -> Self {{ Self }}
    pub fn out_of_memory(_domain: impl Into<String>, _detail: impl Into<String>) -> Self {{ Self }}
}}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct TransportStreamPid(i32);
impl TransportStreamPid {{
    pub fn validate_i32(value: i32) -> Result<Self, ()> {{
        if (0..=0x1fff).contains(&value) {{ Ok(Self(value)) }} else {{ Err(()) }}
    }}
    pub const fn to_i32_for_aidl_boundary(self) -> i32 {{ self.0 }}
}}

pub mod packet_pipeline {{
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum PipelineOpenKind {{ Raw, Pcr, Record, Section, Pes, Av, Other }}
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct FilterPipelineConfig {{
        pub tpid: Option<i32>,
        pub raw: bool,
        pub record_index: Option<crate::production_demux_config::RecordIndexSettings>,
    }}
}}

mod production_demux_config {{
{filter_config_text}
}}
pub use production_demux_config::FilterOpenType;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DvrKind {{ Record, Playback }}
pub const MAX_PES_BUFFER_BYTES: usize = {pes_max};

mod production {{ include!(r#"{capability}"#); }}
use production::{{CapacityLedger, CapabilitySnapshot}};

fn debug_error(error: HalError) -> String {{ format!("{{error:?}}") }}
fn require_capacity(limit: i32, demand: i32, name: &str) -> Result<(), String> {{
    if limit < demand {{ Err(format!("{{name}} demand {{demand}} exceeds capability {{limit}}")) }} else {{ Ok(()) }}
}}

fn run() -> Result<(), String> {{
    let snapshot = CapabilitySnapshot::product_default();
    snapshot.validate_dependency_closures().map_err(debug_error)?;
    let demux_count = snapshot.public_demuxes().map_err(debug_error)?.len();
    if {demux_demand}usize > demux_count {{
        return Err(format!("demux demand {demux_demand} exceeds capability {{demux_count}}"));
    }}
    let mut ledger = CapacityLedger::default();
    {operations}
    Ok(())
}}

fn main() {{
    if let Err(error) = run() {{
        eprintln!("{{error}}");
        std::process::exit(2);
    }}
}}
'''


def validate_resource_closure(
    profile: dict[str, Any],
    *,
    capability_source: Path = DEFAULT_CAPABILITY_SOURCE,
    filter_config_source: Path = DEFAULT_FILTER_CONFIG_SOURCE,
    pes_source: Path = DEFAULT_PES_SOURCE,
    rustc: str = "rustc",
) -> None:
    if not capability_source.is_file():
        raise ProfileError(f"capability source is missing: {capability_source}")
    if not filter_config_source.is_file():
        raise ProfileError(f"filter config source is missing: {filter_config_source}")
    pes_max = _pes_max_bytes(pes_source)
    with tempfile.TemporaryDirectory(prefix="tuner-hal2-vts-closure-") as directory:
        directory_path = Path(directory)
        source = directory_path / "main.rs"
        binary = directory_path / "closure-check"
        source.write_text(
            _program(profile, capability_source, pes_max, filter_config_source),
            encoding="utf-8",
        )
        compiled = subprocess.run(
            [rustc, "--edition=2021", "-O", str(source), "-o", str(binary)],
            check=False, capture_output=True, text=True,
        )
        if compiled.returncode != 0:
            detail = (compiled.stderr or compiled.stdout).strip()
            raise ProfileError(f"failed to compile canonical CapabilitySnapshot/CapacityLedger checker: {detail}")
        checked = subprocess.run([str(binary)], check=False, capture_output=True, text=True)
        if checked.returncode != 0:
            detail = (checked.stderr or checked.stdout).strip()
            raise ProfileError(f"VTS resource closure is not satisfiable: {detail}")

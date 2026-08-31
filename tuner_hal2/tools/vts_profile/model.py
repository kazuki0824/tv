from __future__ import annotations

import json
import re
from pathlib import Path
from typing import Any

SCHEMA_VERSION = 1
SUPPORTED_VTS_CONTRACT = "android14-aidl-v1"
DEFAULT_CAPABILITY_SOURCE = Path("tuner_hal2/service_runtime/src/capability_snapshot.rs")
FRONTEND_ID = {"ISDBT": "FE_ISDBT_0", "ISDBS": "FE_ISDBS_0"}

_TOP = {"schema_version", "target", "vts", "frontend", "region", "flows", "queues"}
_TARGET = {"hal", "product", "backend"}
_VTS = {"contract", "source_ref", "variant"}
_FRONTEND = {
    "type", "is_software_frontend", "frequency_hz", "physical_channel",
    "stream_id", "stream_id_type", "symbol_rate", "modulation", "coderate", "rolloff",
}
_REGION = {"query", "candidates"}
_CANDIDATE = {"delivery_system", "physical_channel", "frequency_hz", "label"}
_FLOWS = {"scan", "record", "clear_live"}
_RECORD = {"enabled", "pid"}
_CLEAR_LIVE = {"enabled", "audio_pid", "video_pid", "audio_stream_type", "video_stream_type"}
_QUEUES = {"record_filter_bytes", "record_dvr_bytes", "audio_filter_bytes", "video_filter_bytes"}


class ProfileError(ValueError):
    pass


def require_dict(value: Any, name: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise ProfileError(f"{name} must be an object")
    return value


def reject_unknown(value: dict[str, Any], allowed: set[str], name: str) -> None:
    unknown = sorted(set(value) - allowed)
    if unknown:
        raise ProfileError(f"{name} has unconsumed fields: {', '.join(unknown)}")


def positive_int(value: Any, name: str, *, allow_zero: bool = False) -> int:
    if isinstance(value, bool):
        raise ProfileError(f"{name} must be an integer")
    try:
        parsed = int(value)
    except (TypeError, ValueError) as exc:
        raise ProfileError(f"{name} must be an integer") from exc
    if parsed < 0 or (parsed == 0 and not allow_zero):
        raise ProfileError(f"{name} must be {'non-negative' if allow_zero else 'positive'}")
    return parsed


def validate_pid(value: Any, name: str) -> int:
    pid = positive_int(value, name, allow_zero=True)
    if pid > 0x1FFF:
        raise ProfileError(f"{name} must be in 0..8191")
    return pid


def load_profile(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise ProfileError(f"failed to read {path}: {exc}") from exc
    return require_dict(value, str(path))


def save_profile(path: Path, profile: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_name(path.name + ".tmp")
    tmp.write_text(json.dumps(profile, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    tmp.replace(path)


def validate_profile(profile: dict[str, Any], *, require_resolved: bool = False) -> None:
    reject_unknown(profile, _TOP, "profile")
    if profile.get("schema_version") != SCHEMA_VERSION:
        raise ProfileError(f"schema_version must be {SCHEMA_VERSION}")

    target = require_dict(profile.get("target"), "target")
    reject_unknown(target, _TARGET, "target")
    if target.get("hal") != "tuner_hal2":
        raise ProfileError("target.hal must be tuner_hal2")
    for key in ("product", "backend"):
        if not isinstance(target.get(key), str) or not target[key].strip():
            raise ProfileError(f"target.{key} is required")

    vts = require_dict(profile.get("vts"), "vts")
    reject_unknown(vts, _VTS, "vts")
    if vts.get("contract") != SUPPORTED_VTS_CONTRACT:
        raise ProfileError(f"vts.contract must be {SUPPORTED_VTS_CONTRACT}")
    if not isinstance(vts.get("source_ref"), str) or not vts["source_ref"].strip():
        raise ProfileError("vts.source_ref is required")
    variant = vts.get("variant", "")
    if not isinstance(variant, str) or (variant and not re.fullmatch(r"[A-Za-z0-9_.-]+", variant)):
        raise ProfileError("vts.variant contains unsupported characters")

    frontend = require_dict(profile.get("frontend"), "frontend")
    reject_unknown(frontend, _FRONTEND, "frontend")
    fe_type = frontend.get("type")
    if fe_type not in FRONTEND_ID:
        raise ProfileError("frontend.type must be ISDBT or ISDBS")
    if frontend.get("is_software_frontend") is not False:
        raise ProfileError("frontend.is_software_frontend must be false for tuner_hal2")
    if frontend.get("frequency_hz") is None:
        if require_resolved:
            raise ProfileError("frontend.frequency_hz is unresolved")
    else:
        positive_int(frontend["frequency_hz"], "frontend.frequency_hz")
    if frontend.get("physical_channel") is not None:
        positive_int(frontend["physical_channel"], "frontend.physical_channel")

    satellite = ("stream_id", "stream_id_type", "symbol_rate", "modulation", "coderate", "rolloff")
    if fe_type == "ISDBS":
        if require_resolved:
            for key in satellite:
                if frontend.get(key) in (None, ""):
                    raise ProfileError(f"frontend.{key} is required for ISDBS")
    else:
        leftovers = [key for key in satellite if key in frontend]
        if leftovers:
            raise ProfileError("ISDBT frontend has unconsumed satellite fields: " + ", ".join(leftovers))

    region = profile.get("region")
    if region is not None:
        region = require_dict(region, "region")
        reject_unknown(region, _REGION, "region")
        if not isinstance(region.get("query"), str) or not region["query"].strip():
            raise ProfileError("region.query must be a non-empty string")
        candidates = region.get("candidates", [])
        if not isinstance(candidates, list):
            raise ProfileError("region.candidates must be an array")
        for index, candidate in enumerate(candidates):
            candidate = require_dict(candidate, f"region.candidates[{index}]")
            reject_unknown(candidate, _CANDIDATE, f"region.candidates[{index}]")
            if candidate.get("delivery_system") not in FRONTEND_ID:
                raise ProfileError(f"region.candidates[{index}].delivery_system is unsupported")
            positive_int(candidate.get("frequency_hz"), f"region.candidates[{index}].frequency_hz")
            if candidate.get("physical_channel") is not None:
                positive_int(candidate["physical_channel"], f"region.candidates[{index}].physical_channel")
            if candidate.get("label") is not None and not isinstance(candidate["label"], str):
                raise ProfileError(f"region.candidates[{index}].label must be a string")
        if candidates and frontend.get("frequency_hz") is not None:
            frequencies = {int(item["frequency_hz"]) for item in candidates}
            if int(frontend["frequency_hz"]) not in frequencies:
                raise ProfileError("frontend.frequency_hz is not one of region.candidates")

    flows = require_dict(profile.get("flows"), "flows")
    reject_unknown(flows, _FLOWS, "flows")
    if not isinstance(flows.get("scan"), bool):
        raise ProfileError("flows.scan must be boolean")

    record = require_dict(flows.get("record"), "flows.record")
    reject_unknown(record, _RECORD, "flows.record")
    if not isinstance(record.get("enabled"), bool):
        raise ProfileError("flows.record.enabled must be boolean")
    if record["enabled"]:
        if record.get("pid") is None:
            if require_resolved:
                raise ProfileError("flows.record.pid is unresolved")
        else:
            validate_pid(record["pid"], "flows.record.pid")
    elif "pid" in record:
        raise ProfileError("disabled flows.record must not keep an unconsumed pid")

    live = require_dict(flows.get("clear_live"), "flows.clear_live")
    reject_unknown(live, _CLEAR_LIVE, "flows.clear_live")
    if not isinstance(live.get("enabled"), bool):
        raise ProfileError("flows.clear_live.enabled must be boolean")
    if live["enabled"]:
        for key in ("audio_pid", "video_pid"):
            if live.get(key) is None:
                if require_resolved:
                    raise ProfileError(f"flows.clear_live.{key} is unresolved")
            else:
                validate_pid(live[key], f"flows.clear_live.{key}")
        if require_resolved:
            for key in ("audio_stream_type", "video_stream_type"):
                if live.get(key) is None:
                    raise ProfileError(f"flows.clear_live.{key} is unresolved")
    else:
        leftovers = sorted(set(live) - {"enabled"})
        if leftovers:
            raise ProfileError("disabled flows.clear_live has unconsumed fields: " + ", ".join(leftovers))

    queues = require_dict(profile.get("queues"), "queues")
    reject_unknown(queues, _QUEUES, "queues")
    required: set[str] = set()
    if record["enabled"]:
        required |= {"record_filter_bytes", "record_dvr_bytes"}
    if live["enabled"]:
        required |= {"audio_filter_bytes", "video_filter_bytes"}
    missing = sorted(required - set(queues))
    extra = sorted(set(queues) - required)
    if missing:
        raise ProfileError("queues is missing: " + ", ".join(missing))
    if extra:
        raise ProfileError("queues has unconsumed fields: " + ", ".join(extra))
    for key in required:
        positive_int(queues[key], f"queues.{key}")


def parse_capability_source(path: Path) -> dict[str, int]:
    try:
        text = path.read_text(encoding="utf-8")
    except OSError as exc:
        raise ProfileError(f"failed to read capability source {path}: {exc}") from exc
    marker = "pub const fn product_default() -> Self"
    start = text.find(marker)
    end = text.find("pub const fn filter_capacity", start)
    if start < 0 or end < 0:
        raise ProfileError("CapabilitySnapshot::product_default was not found")
    body = text[start:end]
    keys = (
        "num_record", "num_playback", "num_ts_filter", "num_section_filter",
        "num_audio_filter", "num_video_filter", "num_pes_filter", "num_pcr_filter",
    )
    result: dict[str, int] = {}
    for key in keys:
        match = re.search(rf"\b{re.escape(key)}:\s*(\d+)\s*,", body)
        if not match:
            raise ProfileError(f"could not read {key} from CapabilitySnapshot::product_default")
        result[key] = int(match.group(1))
    budget = re.search(r"\bfmq_runtime_budget_bytes:\s*(\d+)\s*\*\s*MIB\s*,", body)
    if not budget:
        raise ProfileError("could not read fmq_runtime_budget_bytes from CapabilitySnapshot::product_default")
    result["fmq_runtime_budget_bytes"] = int(budget.group(1)) * 1024 * 1024
    return result


def validate_against_capability(profile: dict[str, Any], capability: dict[str, int]) -> None:
    flows = profile["flows"]
    if flows["record"]["enabled"]:
        if capability["num_record"] <= 0 or capability["num_ts_filter"] <= 0:
            raise ProfileError("record flow is not published by tuner_hal2 capability")
    if flows["clear_live"]["enabled"]:
        if capability["num_audio_filter"] <= 0 or capability["num_video_filter"] <= 0:
            raise ProfileError("clear_live requires published audio and video filters")
    claimed = 0
    queues = profile["queues"]
    if flows["record"]["enabled"]:
        claimed += int(queues["record_filter_bytes"]) + int(queues["record_dvr_bytes"])
    if flows["clear_live"]["enabled"]:
        claimed += int(queues["audio_filter_bytes"]) + int(queues["video_filter_bytes"])
    if claimed > capability["fmq_runtime_budget_bytes"]:
        raise ProfileError("configured VTS queues exceed tuner_hal2 FMQ runtime budget")

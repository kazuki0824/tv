from __future__ import annotations

import json
import re
from pathlib import Path
from typing import Any

SCHEMA_VERSION = 1
SUPPORTED_VTS_CONTRACT = "android14-aidl-v1"
RECORD_FILTER_FMQ_PROBE_VARIANT = "record-filter-fmq"
FRONTEND_ID = {"ISDBT": "FE_ISDBT_0", "ISDBS": "FE_ISDBS_0"}

_TOP = {"schema_version", "target", "vts", "frontend", "region", "service", "flows", "queues"}
_TARGET = {"hal", "product", "backend"}
_VTS = {"contract", "source_ref", "variant"}
_FRONTEND = {
    "type", "is_software_frontend", "frequency_hz", "physical_channel",
    "stream_id", "stream_id_type", "symbol_rate", "modulation", "coderate", "rolloff",
}
_REGION = {"query", "candidates"}
_CANDIDATE = {"delivery_system", "physical_channel", "frequency_hz", "label"}
_SERVICE = {"service_id"}
_FLOWS = {"scan", "record", "clear_live", "playback"}
_RECORD = {"enabled", "pid"}
_CLEAR_LIVE = {
    "enabled", "audio_pid", "video_pid", "audio_stream_type", "video_stream_type",
    "pcr_pid", "section_pid",
}
_PLAYBACK = {"enabled", "input_file_path"}
_QUEUES = {
    "record_filter_bytes", "record_dvr_bytes", "audio_filter_bytes", "video_filter_bytes",
    "pcr_filter_bytes", "section_filter_bytes", "playback_dvr_bytes",
}


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


def load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise ProfileError(f"failed to read {path}: {exc}") from exc
    return require_dict(value, str(path))


load_profile = load_json


def save_profile(path: Path, profile: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_name(path.name + ".tmp")
    tmp.write_text(json.dumps(profile, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    tmp.replace(path)


def _require_resolved_pid(container: dict[str, Any], key: str, prefix: str, require_resolved: bool) -> None:
    value = container.get(key)
    if value is None:
        if require_resolved:
            raise ProfileError(f"{prefix}.{key} is unresolved")
    else:
        validate_pid(value, f"{prefix}.{key}")


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
    freq = frontend.get("frequency_hz")
    if freq is None:
        if require_resolved:
            raise ProfileError("frontend.frequency_hz is unresolved")
    else:
        positive_int(freq, "frontend.frequency_hz")
    if frontend.get("physical_channel") is not None:
        positive_int(frontend["physical_channel"], "frontend.physical_channel")
    satellite = ("stream_id", "stream_id_type", "symbol_rate", "modulation", "coderate", "rolloff")
    if fe_type == "ISDBS":
        for key in satellite:
            if frontend.get(key) in (None, "") and require_resolved:
                raise ProfileError(f"frontend.{key} is required for ISDBS")
            if frontend.get(key) not in (None, ""):
                positive_int(frontend[key], f"frontend.{key}", allow_zero=True)
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
        for index, raw in enumerate(candidates):
            candidate = require_dict(raw, f"region.candidates[{index}]")
            reject_unknown(candidate, _CANDIDATE, f"region.candidates[{index}]")
            if candidate.get("delivery_system") not in FRONTEND_ID:
                raise ProfileError(f"region.candidates[{index}].delivery_system is unsupported")
            positive_int(candidate.get("frequency_hz"), f"region.candidates[{index}].frequency_hz")
            if candidate.get("physical_channel") is not None:
                positive_int(candidate["physical_channel"], f"region.candidates[{index}].physical_channel")
        if candidates and freq is not None:
            if int(freq) not in {int(item["frequency_hz"]) for item in candidates}:
                raise ProfileError("frontend.frequency_hz is not one of region.candidates")

    service = profile.get("service")
    if service is not None:
        service = require_dict(service, "service")
        reject_unknown(service, _SERVICE, "service")
        sid = positive_int(service.get("service_id"), "service.service_id")
        if sid > 0xFFFF:
            raise ProfileError("service.service_id must be in 1..65535")

    flows = require_dict(profile.get("flows"), "flows")
    reject_unknown(flows, _FLOWS, "flows")
    if not isinstance(flows.get("scan"), bool):
        raise ProfileError("flows.scan must be boolean")

    record = require_dict(flows.get("record"), "flows.record")
    reject_unknown(record, _RECORD, "flows.record")
    if not isinstance(record.get("enabled"), bool):
        raise ProfileError("flows.record.enabled must be boolean")
    if record["enabled"]:
        _require_resolved_pid(record, "pid", "flows.record", require_resolved)
    elif "pid" in record:
        raise ProfileError("disabled flows.record must not keep an unconsumed pid")

    live = require_dict(flows.get("clear_live"), "flows.clear_live")
    reject_unknown(live, _CLEAR_LIVE, "flows.clear_live")
    if not isinstance(live.get("enabled"), bool):
        raise ProfileError("flows.clear_live.enabled must be boolean")
    if live["enabled"]:
        for key in ("audio_pid", "video_pid", "pcr_pid", "section_pid"):
            _require_resolved_pid(live, key, "flows.clear_live", require_resolved)
        for key in ("audio_stream_type", "video_stream_type"):
            if live.get(key) is None:
                if require_resolved:
                    raise ProfileError(f"flows.clear_live.{key} is unresolved")
            else:
                positive_int(live[key], f"flows.clear_live.{key}", allow_zero=True)
    else:
        leftovers = sorted(set(live) - {"enabled"})
        if leftovers:
            raise ProfileError("disabled flows.clear_live has unconsumed fields: " + ", ".join(leftovers))

    playback = require_dict(flows.get("playback"), "flows.playback")
    reject_unknown(playback, _PLAYBACK, "flows.playback")
    if not isinstance(playback.get("enabled"), bool):
        raise ProfileError("flows.playback.enabled must be boolean")
    if playback["enabled"]:
        path = playback.get("input_file_path")
        if not isinstance(path, str) or not path.strip() or not path.startswith("/"):
            raise ProfileError("flows.playback.input_file_path must be an absolute device path")
    elif "input_file_path" in playback:
        raise ProfileError("disabled flows.playback must not keep an input_file_path")

    if variant == RECORD_FILTER_FMQ_PROBE_VARIANT:
        if not record["enabled"]:
            raise ProfileError("record-filter-fmq VTS variant requires flows.record.enabled=true")
        if live["enabled"] or playback["enabled"]:
            raise ProfileError("record-filter-fmq VTS variant must remain a RECORD-only descriptor probe")
    else:
        missing_coverage: list[str] = []
        if not flows["scan"]:
            missing_coverage.append("scan")
        if not record["enabled"]:
            missing_coverage.append("record")
        if not live["enabled"]:
            missing_coverage.append("clear_live(A/V+PCR+SECTION)")
        if not playback["enabled"]:
            missing_coverage.append("playback")
        if missing_coverage:
            raise ProfileError(
                "canonical VTS capability coverage is unreachable: " + ", ".join(missing_coverage)
            )

    queues = require_dict(profile.get("queues"), "queues")
    reject_unknown(queues, _QUEUES, "queues")
    required: set[str] = set()
    if record["enabled"]:
        required |= {"record_filter_bytes", "record_dvr_bytes"}
    if live["enabled"]:
        required |= {
            "audio_filter_bytes", "video_filter_bytes", "pcr_filter_bytes", "section_filter_bytes"
        }
    if playback["enabled"]:
        required.add("playback_dvr_bytes")
    missing = sorted(required - set(queues))
    extra = sorted(set(queues) - required)
    if missing:
        raise ProfileError("queues is missing: " + ", ".join(missing))
    if extra:
        raise ProfileError("queues has unconsumed fields: " + ", ".join(extra))
    for key in required:
        positive_int(queues[key], f"queues.{key}")

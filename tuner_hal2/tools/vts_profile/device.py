from __future__ import annotations
import json
import subprocess
from copy import deepcopy
from pathlib import Path
from typing import Any
from .model import ProfileError, save_profile, validate_pid, validate_profile

DEFAULT_HELPER = "/vendor/bin/maleicacid_tuner_hal2_vts_resolver"


def _frequencies(profile: dict[str, Any], candidate_index: int | None) -> list[int]:
    candidates = profile.get("region", {}).get("candidates", []) if profile.get("region") else []
    if candidate_index is not None:
        if candidate_index < 0 or candidate_index >= len(candidates):
            raise ProfileError("candidate index is outside region.candidates")
        return [int(candidates[candidate_index]["frequency_hz"])]
    if profile["frontend"].get("frequency_hz") is not None:
        return [int(profile["frontend"]["frequency_hz"])]
    if candidates:
        return [int(item["frequency_hz"]) for item in candidates]
    raise ProfileError("resolve-device requires a selected frequency or regional candidates")


def _helper_command(profile: dict[str, Any], frequency: int, timeout_ms: int, helper: str) -> list[str]:
    cmd = [helper, "--delivery-system", profile["frontend"]["type"], "--frequency-hz", str(frequency), "--timeout-ms", str(timeout_ms)]
    service = profile.get("service")
    if service is not None:
        cmd += ["--service-id", str(service["service_id"])]
    if profile["frontend"]["type"] == "ISDBS":
        for key in ("stream_id", "stream_id_type", "symbol_rate", "modulation", "coderate", "rolloff"):
            cmd += ["--" + key.replace("_", "-"), str(profile["frontend"][key])]
    return cmd


def _run_helper(adb: str, serial: str | None, command: list[str], timeout_ms: int) -> dict[str, Any]:
    argv = [adb]
    if serial:
        argv += ["-s", serial]
    argv += ["shell", *command]
    try:
        result = subprocess.run(
            argv, capture_output=True, text=True,
            timeout=max(5.0, timeout_ms / 1000.0 + 5.0),
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise ProfileError(f"device resolver invocation failed: {exc}") from exc
    if result.returncode != 0:
        raise ProfileError((result.stderr or result.stdout or "device resolver failed").strip())
    lines = [line.strip() for line in result.stdout.splitlines() if line.strip()]
    if not lines:
        raise ProfileError("device resolver returned no JSON")
    try:
        payload = json.loads(lines[-1])
    except json.JSONDecodeError as exc:
        raise ProfileError("device resolver returned malformed JSON") from exc
    if not isinstance(payload, dict):
        raise ProfileError("device resolver JSON must be an object")
    return payload


def _apply(profile: dict[str, Any], payload: dict[str, Any]) -> dict[str, Any]:
    updated = deepcopy(profile)
    frequency = int(payload.get("frequency_hz", 0))
    if frequency <= 0:
        raise ProfileError("device resolver returned invalid frequency")
    service_id = int(payload.get("service_id", 0))
    if not 1 <= service_id <= 0xFFFF:
        raise ProfileError("device resolver returned invalid service_id")
    elementary = payload.get("elementary_pids")
    if not isinstance(elementary, list) or not elementary:
        raise ProfileError("device resolver returned no elementary PIDs")
    elementary_pids = [validate_pid(pid, "device elementary pid") for pid in elementary]
    audio_pid = payload.get("audio_pid")
    video_pid = payload.get("video_pid")
    audio = validate_pid(audio_pid, "device audio pid") if audio_pid is not None else None
    video = validate_pid(video_pid, "device video pid") if video_pid is not None else None
    updated["frontend"]["frequency_hz"] = frequency
    updated["service"] = {"service_id": service_id}
    record = updated["flows"]["record"]
    if record["enabled"]:
        chosen = record.get("pid")
        if chosen is None:
            chosen = video if video is not None else audio if audio is not None else elementary_pids[0]
            record["pid"] = chosen
        if int(chosen) not in elementary_pids:
            raise ProfileError("configured record PID is not present in resolved PMT")
    live = updated["flows"]["clear_live"]
    if live["enabled"]:
        if audio is None or video is None:
            raise ProfileError("resolved service does not contain both audio and video")
        if live.get("audio_pid") is None:
            live["audio_pid"] = audio
        if live.get("video_pid") is None:
            live["video_pid"] = video
        if int(live["audio_pid"]) != audio or int(live["video_pid"]) != video:
            raise ProfileError("configured AV PIDs do not match resolved PMT")
    validate_profile(updated, require_resolved=True)
    return updated


def resolve_device(
    profile_path: Path, *, adb: str = "adb", serial: str | None = None,
    helper: str = DEFAULT_HELPER, timeout_ms: int = 5000,
    candidate_index: int | None = None,
) -> dict[str, Any]:
    from .model import load_profile
    original = load_profile(profile_path)
    validate_profile(original)
    successes: list[dict[str, Any]] = []
    errors: list[str] = []
    for frequency in _frequencies(original, candidate_index):
        try:
            payload = _run_helper(adb, serial, _helper_command(original, frequency, timeout_ms, helper), timeout_ms)
            if int(payload.get("frequency_hz", 0)) != frequency:
                raise ProfileError("device resolver frequency does not match requested candidate")
            successes.append(payload)
        except ProfileError as exc:
            errors.append(f"{frequency}: {exc}")
    if len(successes) != 1:
        if not successes:
            raise ProfileError("no candidate resolved successfully: " + "; ".join(errors))
        raise ProfileError("multiple candidates resolved successfully; select one explicitly")
    updated = _apply(original, successes[0])
    save_profile(profile_path, updated)
    return updated

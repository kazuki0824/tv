from __future__ import annotations

import json
import subprocess
from copy import deepcopy
from pathlib import Path
from typing import Any

from .model import ProfileError, save_profile, validate_pid, validate_profile

DEFAULT_REMOTE_AGENT = "/vendor/bin/maleicacid_tuner_hal2_vts_agent"
PUSHED_REMOTE_AGENT = "/data/local/tmp/maleicacid_tuner_hal2_vts_agent"
DEFAULT_SI_HOST = "maleicacid_arib_si_engine_vts_host"


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


def _adb_prefix(adb: str, serial: str | None) -> list[str]:
    argv = [adb]
    if serial:
        argv += ["-s", serial]
    return argv


def _run(argv: list[str], *, timeout: float) -> subprocess.CompletedProcess[str]:
    try:
        return subprocess.run(argv, capture_output=True, text=True, timeout=timeout)
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise ProfileError(f"command failed: {' '.join(argv)}: {exc}") from exc


def _prepare_agent(*, adb: str, serial: str | None, agent_binary: Path | None, remote_agent: str) -> tuple[str, bool]:
    if agent_binary is None:
        return remote_agent, False
    if not agent_binary.is_file():
        raise ProfileError(f"VTS device agent binary does not exist: {agent_binary}")
    prefix = _adb_prefix(adb, serial)
    pushed = _run(prefix + ["push", str(agent_binary), PUSHED_REMOTE_AGENT], timeout=30.0)
    if pushed.returncode != 0:
        raise ProfileError((pushed.stderr or pushed.stdout or "adb push failed").strip())
    chmod = _run(prefix + ["shell", "chmod", "0755", PUSHED_REMOTE_AGENT], timeout=10.0)
    if chmod.returncode != 0:
        raise ProfileError((chmod.stderr or chmod.stdout or "chmod failed").strip())
    return PUSHED_REMOTE_AGENT, True


def _cleanup_agent(*, adb: str, serial: str | None, pushed: bool) -> None:
    if pushed:
        _run(_adb_prefix(adb, serial) + ["shell", "rm", "-f", PUSHED_REMOTE_AGENT], timeout=10.0)


def _agent_command(profile: dict[str, Any], frequency: int, pid: int, table_id: int, timeout_ms: int, remote_agent: str) -> list[str]:
    command = [
        remote_agent,
        "--delivery-system", profile["frontend"]["type"],
        "--frequency-hz", str(frequency),
        "--pid", str(pid),
        "--table-id", str(table_id),
        "--timeout-ms", str(timeout_ms),
    ]
    if profile["frontend"]["type"] == "ISDBS":
        for key in ("stream_id", "stream_id_type", "symbol_rate", "modulation", "coderate", "rolloff"):
            command += ["--" + key.replace("_", "-"), str(profile["frontend"][key])]
    return command


def _run_agent_payload(profile: dict[str, Any], frequency: int, pid: int, table_id: int, *, adb: str, serial: str | None, remote_agent: str, timeout_ms: int) -> tuple[int, bytes]:
    result = _run(
        _adb_prefix(adb, serial) + ["shell", *_agent_command(profile, frequency, pid, table_id, timeout_ms, remote_agent)],
        timeout=max(10.0, timeout_ms / 1000.0 + 10.0),
    )
    if result.returncode != 0:
        raise ProfileError((result.stderr or result.stdout or "VTS device agent failed").strip())
    lines = [line.strip() for line in result.stdout.splitlines() if line.strip()]
    if not lines:
        raise ProfileError("VTS device agent returned no JSON")
    try:
        payload = json.loads(lines[-1])
    except json.JSONDecodeError as exc:
        raise ProfileError("VTS device agent returned malformed JSON") from exc
    if not isinstance(payload, dict):
        raise ProfileError("VTS device agent JSON must be an object")
    if int(payload.get("frequency_hz", 0)) != frequency:
        raise ProfileError("VTS device agent frequency does not match requested candidate")
    if int(payload.get("pid", -1)) != pid or int(payload.get("table_id", -1)) != table_id:
        raise ProfileError("VTS device agent returned payload for a different PID/table")
    raw_hex = payload.get("payload_hex")
    if not isinstance(raw_hex, str):
        raise ProfileError("VTS device agent did not return payload_hex")
    try:
        section_payload = bytes.fromhex(raw_hex)
    except ValueError as exc:
        raise ProfileError("VTS device agent returned invalid payload_hex") from exc
    if not section_payload or section_payload[0] != table_id:
        raise ProfileError("VTS device agent section payload has the wrong table_id")
    return pid, section_payload


def _si_query(host: str, sections: list[tuple[int, bytes]]) -> dict[str, Any]:
    argv = [host]
    for pid, section in sections:
        argv += ["--payload", f"{pid}:{section.hex()}"]
    result = _run(argv, timeout=10.0)
    if result.returncode != 0:
        raise ProfileError((result.stderr or "arib_si_engine_rs host adapter failed").strip())
    try:
        payload = json.loads(result.stdout)
    except json.JSONDecodeError as exc:
        raise ProfileError("arib_si_engine_rs host adapter returned malformed JSON") from exc
    if not isinstance(payload, dict):
        raise ProfileError("arib_si_engine_rs host adapter JSON must be an object")
    return payload


def _resolve_frequency(profile: dict[str, Any], frequency: int, *, adb: str, serial: str | None, remote_agent: str, timeout_ms: int, si_host: str) -> dict[str, Any]:
    pat = [_run_agent_payload(profile, frequency, 0x0000, 0x00, adb=adb, serial=serial, remote_agent=remote_agent, timeout_ms=timeout_ms)]
    pat_semantics = _si_query(si_host, pat)
    programs = pat_semantics.get("programs")
    if not isinstance(programs, list) or not programs:
        raise ProfileError("arib_si_engine_rs found no service program in PAT")
    requested = profile.get("service", {}).get("service_id") if profile.get("service") else None
    if requested is not None:
        programs = [p for p in programs if int(p.get("service_id", -1)) == int(requested)]
    if len(programs) != 1:
        ids = ",".join(str(p.get("service_id")) for p in programs)
        raise ProfileError(f"service selection is ambiguous ({ids}); specify service_id")
    program = programs[0]
    pmt_pid = validate_pid(program.get("pmt_pid"), "PAT PMT pid")
    pmt = [_run_agent_payload(profile, frequency, pmt_pid, 0x02, adb=adb, serial=serial, remote_agent=remote_agent, timeout_ms=timeout_ms)]
    semantics = _si_query(si_host, pat + pmt)
    pmts = semantics.get("pmts")
    if not isinstance(pmts, list):
        raise ProfileError("arib_si_engine_rs returned invalid PMT result")
    matches = [item for item in pmts if int(item.get("service_id", -1)) == int(program["service_id"]) and int(item.get("pmt_pid", -1)) == pmt_pid]
    if len(matches) != 1:
        raise ProfileError("arib_si_engine_rs did not produce one PMT for selected service")
    selected = dict(matches[0])
    selected["frequency_hz"] = frequency
    return selected


def _apply(profile: dict[str, Any], resolved: dict[str, Any]) -> dict[str, Any]:
    updated = deepcopy(profile)
    frequency = int(resolved.get("frequency_hz", 0))
    service_id = int(resolved.get("service_id", 0))
    validate_pid(resolved.get("pmt_pid"), "resolved PMT pid")
    streams = resolved.get("streams")
    if frequency <= 0 or not 1 <= service_id <= 0xFFFF:
        raise ProfileError("resolved frequency/service is invalid")
    if not isinstance(streams, list) or not streams:
        raise ProfileError("resolved PMT has no elementary streams")
    elementary_pids = sorted({validate_pid(stream.get("pid"), "resolved elementary pid") for stream in streams})
    updated["frontend"]["frequency_hz"] = frequency
    updated["service"] = {"service_id": service_id}
    record = updated["flows"]["record"]
    if record["enabled"]:
        chosen = record.get("pid")
        if chosen is None:
            record["pid"] = elementary_pids[0]
        elif validate_pid(chosen, "configured record pid") not in elementary_pids:
            raise ProfileError("configured record PID is not present in arib_si_engine_rs PMT result")
    if updated["flows"]["clear_live"]["enabled"]:
        raise ProfileError(
            "clear_live is not satisfiable by the current tuner_hal2 capability; "
            "when AV filters become a product capability, the host arib_si_engine_rs adapter must export its canonical AV component classification before resolve-device can populate AV PIDs"
        )
    validate_profile(updated, require_resolved=True)
    return updated


def resolve_device(profile_path: Path, *, adb: str = "adb", serial: str | None = None,
    agent_binary: Path | None = None, remote_agent: str = DEFAULT_REMOTE_AGENT,
    si_host: str = DEFAULT_SI_HOST, timeout_ms: int = 5000,
    candidate_index: int | None = None) -> dict[str, Any]:
    from .model import load_profile
    original = load_profile(profile_path)
    validate_profile(original)
    remote, pushed = _prepare_agent(adb=adb, serial=serial, agent_binary=agent_binary, remote_agent=remote_agent)
    try:
        successes: list[dict[str, Any]] = []
        errors: list[str] = []
        for frequency in _frequencies(original, candidate_index):
            try:
                successes.append(_resolve_frequency(original, frequency, adb=adb, serial=serial,
                    remote_agent=remote, timeout_ms=timeout_ms, si_host=si_host))
            except ProfileError as exc:
                errors.append(f"{frequency}: {exc}")
        if len(successes) != 1:
            if not successes:
                raise ProfileError("no candidate resolved successfully: " + "; ".join(errors))
            raise ProfileError("multiple candidates resolved successfully; select one explicitly")
        updated = _apply(original, successes[0])
        save_profile(profile_path, updated)
        return updated
    finally:
        _cleanup_agent(adb=adb, serial=serial, pushed=pushed)

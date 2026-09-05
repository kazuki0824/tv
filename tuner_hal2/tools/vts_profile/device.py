from __future__ import annotations

import json
import queue
import subprocess
import threading
from copy import deepcopy
from pathlib import Path
from typing import Any

from .model import ProfileError, save_profile, validate_pid, validate_profile

DEFAULT_REMOTE_AGENT = "/vendor/bin/maleicacid_tuner_hal2_vts_agent"
PUSHED_REMOTE_AGENT = "/data/local/tmp/maleicacid_tuner_hal2_vts_agent"
DEFAULT_SI_HOST = "maleicacid_arib_si_engine_vts_host"

# ISO/IEC 13818-1 PMT stream_type -> AIDL android.hardware.tv.tuner stream type.
# Keep this deliberately aligned with the product's TIS clear-playback support set.
_VIDEO_STREAM_TYPE_TO_AIDL = {
    0x02: 3,  # MPEG-2 video -> VideoStreamType.MPEG2
    0x1B: 5,  # AVC -> VideoStreamType.AVC
}
_AUDIO_STREAM_TYPE_TO_AIDL = {
    0x03: 3,  # MPEG-1 audio -> AudioStreamType.MPEG1
    0x04: 4,  # MPEG-2 audio -> AudioStreamType.MPEG2
    0x0F: 16, # AAC ADTS -> AudioStreamType.AAC_ADTS
}


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


def _prepare_agent(
    *, adb: str, serial: str | None, agent_binary: Path | None, remote_agent: str
) -> tuple[str, bool]:
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
        _run(prefix + ["shell", "rm", "-f", PUSHED_REMOTE_AGENT], timeout=10.0)
        raise ProfileError((chmod.stderr or chmod.stdout or "chmod failed").strip())
    return PUSHED_REMOTE_AGENT, True


def _cleanup_agent(*, adb: str, serial: str | None, pushed: bool) -> None:
    if pushed:
        _run(
            _adb_prefix(adb, serial) + ["shell", "rm", "-f", PUSHED_REMOTE_AGENT],
            timeout=10.0,
        )


def _agent_start_command(
    profile: dict[str, Any], frequency: int, timeout_ms: int, remote_agent: str
) -> list[str]:
    command = [
        remote_agent,
        "--delivery-system",
        profile["frontend"]["type"],
        "--frequency-hz",
        str(frequency),
        "--timeout-ms",
        str(timeout_ms),
    ]
    if profile["frontend"]["type"] == "ISDBS":
        for key in (
            "stream_id",
            "stream_id_type",
            "symbol_rate",
            "modulation",
            "coderate",
            "rolloff",
        ):
            command += ["--" + key.replace("_", "-"), str(profile["frontend"][key])]
    return command


class _AgentSession:
    def __init__(
        self,
        profile: dict[str, Any],
        frequency: int,
        *,
        adb: str,
        serial: str | None,
        remote_agent: str,
        timeout_ms: int,
    ) -> None:
        self._timeout = max(10.0, timeout_ms / 1000.0 + 10.0)
        argv = _adb_prefix(adb, serial) + [
            "shell",
            *_agent_start_command(profile, frequency, timeout_ms, remote_agent),
        ]
        try:
            self._process = subprocess.Popen(
                argv,
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                bufsize=1,
            )
        except OSError as exc:
            raise ProfileError(f"failed to start VTS device agent: {exc}") from exc
        if self._process.stdin is None or self._process.stdout is None or self._process.stderr is None:
            self._terminate()
            raise ProfileError("failed to create VTS device agent pipes")

        self._responses: queue.Queue[str] = queue.Queue()
        self._stderr_lines: list[str] = []
        self._stdout_thread = threading.Thread(target=self._read_stdout, daemon=True)
        self._stderr_thread = threading.Thread(target=self._read_stderr, daemon=True)
        self._stdout_thread.start()
        self._stderr_thread.start()
        try:
            ready = self._receive()
        except ProfileError:
            self._terminate()
            raise
        if ready.get("status") != "ready" or int(ready.get("frequency_hz", 0)) != frequency:
            self._terminate()
            raise ProfileError("VTS device agent did not establish the requested tune session")

    def _read_stdout(self) -> None:
        assert self._process.stdout is not None
        for line in self._process.stdout:
            stripped = line.strip()
            if stripped:
                self._responses.put(stripped)

    def _read_stderr(self) -> None:
        assert self._process.stderr is not None
        for line in self._process.stderr:
            stripped = line.strip()
            if stripped:
                self._stderr_lines.append(stripped)

    def _stderr(self) -> str:
        return "\n".join(self._stderr_lines[-20:])

    def _receive(self) -> dict[str, Any]:
        try:
            line = self._responses.get(timeout=self._timeout)
        except queue.Empty as exc:
            detail = self._stderr() or "no response"
            raise ProfileError(f"VTS device agent response timeout: {detail}") from exc
        try:
            payload = json.loads(line)
        except json.JSONDecodeError as exc:
            raise ProfileError("VTS device agent returned malformed JSON") from exc
        if not isinstance(payload, dict):
            raise ProfileError("VTS device agent JSON must be an object")
        if payload.get("status") == "error":
            raise ProfileError(str(payload.get("message") or "VTS device agent request failed"))
        return payload

    def _request(self, request: dict[str, Any]) -> dict[str, Any]:
        if self._process.poll() is not None:
            raise ProfileError(f"VTS device agent exited unexpectedly: {self._stderr()}")
        assert self._process.stdin is not None
        try:
            self._process.stdin.write(json.dumps(request, separators=(",", ":")) + "\n")
            self._process.stdin.flush()
        except (BrokenPipeError, OSError) as exc:
            raise ProfileError(f"failed to write VTS device agent request: {exc}") from exc
        return self._receive()

    def section(self, pid: int, table_id: int) -> tuple[int, bytes]:
        response = self._request({"op": "section", "pid": pid, "table_id": table_id})
        if response.get("status") != "ok":
            raise ProfileError("VTS device agent returned an unexpected response")
        if int(response.get("pid", -1)) != pid or int(response.get("table_id", -1)) != table_id:
            raise ProfileError("VTS device agent returned payload for a different PID/table")
        raw_hex = response.get("payload_hex")
        if not isinstance(raw_hex, str):
            raise ProfileError("VTS device agent did not return payload_hex")
        try:
            section_payload = bytes.fromhex(raw_hex)
        except ValueError as exc:
            raise ProfileError("VTS device agent returned invalid payload_hex") from exc
        if not section_payload or section_payload[0] != table_id:
            raise ProfileError("VTS device agent section payload has the wrong table_id")
        return pid, section_payload

    def close(self) -> None:
        if self._process.poll() is None:
            try:
                response = self._request({"op": "close"})
                if response.get("status") != "closed":
                    raise ProfileError("VTS device agent returned an unexpected close response")
                self._process.wait(timeout=2.0)
            except (ProfileError, subprocess.TimeoutExpired):
                pass
        self._terminate()

    def _terminate(self) -> None:
        if self._process.poll() is None:
            self._process.terminate()
            try:
                self._process.wait(timeout=2.0)
            except subprocess.TimeoutExpired:
                self._process.kill()
                self._process.wait(timeout=2.0)

    def __enter__(self) -> "_AgentSession":
        return self

    def __exit__(self, exc_type: Any, exc: Any, tb: Any) -> None:
        self.close()


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


def _pmt_pids(semantics: dict[str, Any]) -> list[int]:
    raw = semantics.get("pmt_pids")
    if not isinstance(raw, list) or not raw:
        raise ProfileError("arib_si_engine_rs found no PMT PID in PAT")
    pids = sorted({validate_pid(pid, "PAT PMT pid") for pid in raw})
    return pids


def _resolve_frequency(
    profile: dict[str, Any],
    frequency: int,
    *,
    adb: str,
    serial: str | None,
    remote_agent: str,
    timeout_ms: int,
    si_host: str,
) -> dict[str, Any]:
    with _AgentSession(
        profile,
        frequency,
        adb=adb,
        serial=serial,
        remote_agent=remote_agent,
        timeout_ms=timeout_ms,
    ) as session:
        pat = session.section(0x0000, 0x00)
        pmt_pids = _pmt_pids(_si_query(si_host, [pat]))
        pmts = [session.section(pid, 0x02) for pid in pmt_pids]
        sdt_actual = session.section(0x0011, 0x42)
        semantics = _si_query(si_host, [pat, *pmts, sdt_actual])

    services = semantics.get("services")
    if not isinstance(services, list) or not services:
        raise ProfileError("arib_si_engine_rs produced no service with a parsed PMT")
    requested = profile.get("service", {}).get("service_id") if profile.get("service") else None
    if requested is not None:
        services = [item for item in services if int(item.get("service_id", -1)) == int(requested)]
    if len(services) != 1:
        ids = ",".join(str(item.get("service_id")) for item in services)
        raise ProfileError(f"service selection is ambiguous ({ids}); specify service_id")
    selected = dict(services[0])
    selected["frequency_hz"] = frequency
    return selected


def _single_stream(
    streams: list[dict[str, Any]], mapping: dict[int, int], label: str
) -> tuple[dict[str, Any], int]:
    matches = [item for item in streams if int(item.get("stream_type", -1)) in mapping]
    if not matches:
        raise ProfileError(f"resolved PMT has no supported {label} elementary stream")
    selected = sorted(matches, key=lambda item: int(item.get("pid", 0x2000)))[0]
    raw_stream_type = int(selected["stream_type"])
    return selected, mapping[raw_stream_type]


def _apply(profile: dict[str, Any], resolved: dict[str, Any]) -> dict[str, Any]:
    updated = deepcopy(profile)
    frequency = int(resolved.get("frequency_hz", 0))
    service_id = int(resolved.get("service_id", 0))
    pmt_pid = validate_pid(resolved.get("pmt_pid"), "resolved PMT pid")
    pcr_pid = validate_pid(resolved.get("pcr_pid"), "resolved PCR pid")
    streams = resolved.get("streams")
    if frequency <= 0 or not 1 <= service_id <= 0xFFFF:
        raise ProfileError("resolved frequency/service is invalid")
    if not isinstance(streams, list) or not streams:
        raise ProfileError("resolved PMT has no elementary streams")
    elementary_pids = sorted(
        {validate_pid(stream.get("pid"), "resolved elementary pid") for stream in streams}
    )
    updated["frontend"]["frequency_hz"] = frequency
    updated["service"] = {"service_id": service_id}
    record = updated["flows"]["record"]
    if record["enabled"]:
        chosen = record.get("pid")
        if chosen is None:
            record["pid"] = elementary_pids[0]
        elif validate_pid(chosen, "configured record pid") not in elementary_pids:
            raise ProfileError("configured record PID is not present in arib_si_engine_rs PMT result")

    live = updated["flows"]["clear_live"]
    if live["enabled"]:
        typed_streams = [dict(item) for item in streams if isinstance(item, dict)]
        audio, audio_aidl_type = _single_stream(
            typed_streams, _AUDIO_STREAM_TYPE_TO_AIDL, "audio"
        )
        video, video_aidl_type = _single_stream(
            typed_streams, _VIDEO_STREAM_TYPE_TO_AIDL, "video"
        )
        live.update(
            {
                "audio_pid": validate_pid(audio.get("pid"), "resolved audio pid"),
                "video_pid": validate_pid(video.get("pid"), "resolved video pid"),
                "audio_stream_type": audio_aidl_type,
                "video_stream_type": video_aidl_type,
                "pcr_pid": pcr_pid,
                "section_pid": pmt_pid,
            }
        )
    validate_profile(updated, require_resolved=True)
    return updated


def resolve_device(
    profile_path: Path,
    *,
    adb: str = "adb",
    serial: str | None = None,
    agent_binary: Path | None = None,
    remote_agent: str = DEFAULT_REMOTE_AGENT,
    si_host: str = DEFAULT_SI_HOST,
    timeout_ms: int = 5000,
    candidate_index: int | None = None,
) -> dict[str, Any]:
    from .model import load_profile

    original = load_profile(profile_path)
    validate_profile(original)
    remote, pushed = _prepare_agent(
        adb=adb, serial=serial, agent_binary=agent_binary, remote_agent=remote_agent
    )
    try:
        successes: list[dict[str, Any]] = []
        errors: list[str] = []
        for frequency in _frequencies(original, candidate_index):
            try:
                successes.append(
                    _resolve_frequency(
                        original,
                        frequency,
                        adb=adb,
                        serial=serial,
                        remote_agent=remote,
                        timeout_ms=timeout_ms,
                        si_host=si_host,
                    )
                )
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

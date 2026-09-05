from __future__ import annotations

import subprocess
from pathlib import Path

from .compiler import validated_xml
from .model import ProfileError, load_profile
from .render import output_filename
from .resource_closure import (
    DEFAULT_CAPABILITY_SOURCE,
    DEFAULT_PES_SOURCE,
    DEFAULT_PLAYBACK_SOURCE,
)

DEFAULT_COMPILED_DIR = Path("out/vts")
VENDOR_CONFIG_DIR = "/vendor/etc"
VARIANT_PROPERTY = "ro.vendor.vts_tuner_configuration_variant"


def _adb_command(adb: str, serial: str | None, *args: str) -> list[str]:
    command = [adb]
    if serial:
        command.extend(["-s", serial])
    command.extend(args)
    return command


def _run_adb_text(adb: str, serial: str | None, *args: str) -> str:
    command = _adb_command(adb, serial, *args)
    try:
        result = subprocess.run(
            command,
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
    except OSError as exc:
        raise ProfileError(f"failed to execute adb: {exc}") from exc
    if result.returncode != 0:
        detail = (result.stderr or result.stdout).strip() or f"exit {result.returncode}"
        raise ProfileError(f"adb {' '.join(args)} failed: {detail}")
    return result.stdout.strip()


def _run_adb_bytes(adb: str, serial: str | None, *args: str) -> bytes:
    command = _adb_command(adb, serial, *args)
    try:
        result = subprocess.run(
            command,
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
    except OSError as exc:
        raise ProfileError(f"failed to execute adb: {exc}") from exc
    if result.returncode != 0:
        detail = (result.stderr or result.stdout).decode("utf-8", errors="replace").strip()
        if not detail:
            detail = f"exit {result.returncode}"
        raise ProfileError(f"adb {' '.join(args)} failed: {detail}")
    return result.stdout


def install_device(
    profile_path: Path,
    *,
    hardware_interfaces_root: Path,
    adb: str = "adb",
    serial: str | None = None,
    artifact: Path | None = None,
    capability_source: Path = DEFAULT_CAPABILITY_SOURCE,
    pes_source: Path = DEFAULT_PES_SOURCE,
    playback_source: Path = DEFAULT_PLAYBACK_SOURCE,
    rustc: str = "rustc",
    xmllint: str = "xmllint",
) -> str:
    profile = load_profile(profile_path)
    expected = validated_xml(
        profile,
        hardware_interfaces_root=hardware_interfaces_root,
        capability_source=capability_source,
        pes_source=pes_source,
        playback_source=playback_source,
        rustc=rustc,
        xmllint=xmllint,
    ).encode("utf-8")
    filename = output_filename(profile)
    artifact_path = artifact if artifact is not None else DEFAULT_COMPILED_DIR / filename
    if artifact_path.name != filename:
        raise ProfileError(
            f"compiled artifact filename must be {filename}, got {artifact_path.name}"
        )
    try:
        compiled = artifact_path.read_bytes()
    except OSError as exc:
        raise ProfileError(f"failed to read compiled VTS artifact {artifact_path}: {exc}") from exc
    if compiled != expected:
        raise ProfileError(
            "compiled VTS artifact does not match the current fully validated profile; "
            "run compile again before install-device"
        )

    expected_variant = str(profile["vts"].get("variant", ""))
    actual_variant = _run_adb_text(
        adb,
        serial,
        "shell",
        "getprop",
        VARIANT_PROPERTY,
    )
    if actual_variant != expected_variant:
        raise ProfileError(
            f"device {VARIANT_PROPERTY}={actual_variant!r} does not match "
            f"profile variant {expected_variant!r}; build/flash a matching product image"
        )

    _run_adb_text(adb, serial, "root")
    _run_adb_text(adb, serial, "wait-for-device")
    uid = _run_adb_text(adb, serial, "shell", "id", "-u")
    if uid != "0":
        raise ProfileError(
            "adb root did not produce uid 0; use the product build/flash installation path"
        )
    _run_adb_text(adb, serial, "remount")

    remote_path = f"{VENDOR_CONFIG_DIR}/{filename}"
    _run_adb_text(adb, serial, "push", str(artifact_path), remote_path)
    observed = _run_adb_bytes(adb, serial, "exec-out", "cat", remote_path)
    if observed != expected:
        raise ProfileError(f"device readback does not match compiled artifact at {remote_path}")
    return remote_path

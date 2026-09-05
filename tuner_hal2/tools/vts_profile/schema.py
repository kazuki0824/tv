from __future__ import annotations

import subprocess
import tempfile
from pathlib import Path
from xml.etree import ElementTree

from .model import ProfileError

XSD_RELATIVE_PATH = Path("tv/tuner/config/tuner_testing_dynamic_configuration.xsd")
AOSP_VALIDATOR_TARGET = "maleicacid_tuner_hal2_vts_config_validator"
SOONG_UI_RELATIVE_PATH = Path("build/soong/soong_ui.bash")


def _git_commit(root: Path, ref: str) -> str:
    try:
        result = subprocess.run(
            ["git", "-C", str(root), "rev-parse", f"{ref}^{{commit}}"],
            check=True,
            capture_output=True,
            text=True,
        )
    except (OSError, subprocess.CalledProcessError) as exc:
        raise ProfileError(f"cannot resolve AOSP VTS ref {ref!r} in {root}") from exc
    return result.stdout.strip()


def selected_xsd(hardware_interfaces_root: Path, source_ref: str) -> Path:
    root = hardware_interfaces_root.resolve()
    if _git_commit(root, "HEAD") != _git_commit(root, source_ref):
        raise ProfileError("hardware/interfaces checkout HEAD does not match profile vts.source_ref")
    xsd = root / XSD_RELATIVE_PATH
    if not xsd.is_file():
        raise ProfileError(f"AOSP Tuner VTS XSD not found: {xsd}")
    return xsd


def _run_aosp_build_command(aosp_root: Path, args: list[str], *, label: str) -> str:
    soong_ui = aosp_root / SOONG_UI_RELATIVE_PATH
    if not soong_ui.is_file():
        raise ProfileError(f"AOSP Soong entry point not found: {soong_ui}")
    try:
        result = subprocess.run(
            [str(soong_ui), *args],
            cwd=aosp_root,
            check=True,
            capture_output=True,
            text=True,
        )
    except (OSError, subprocess.CalledProcessError) as exc:
        detail = ""
        if isinstance(exc, subprocess.CalledProcessError):
            detail = (exc.stderr or exc.stdout or "").strip()
        raise ProfileError(label + (f": {detail}" if detail else "")) from exc
    return result.stdout.strip()


def _build_aosp_consumer(aosp_root: Path) -> Path:
    root = aosp_root.resolve()
    _run_aosp_build_command(
        root,
        ["--make-mode", AOSP_VALIDATOR_TARGET],
        label="failed to build AOSP xsdc Tuner config validator",
    )
    host_out = _run_aosp_build_command(
        root,
        ["--dumpvar-mode", "HOST_OUT_EXECUTABLES"],
        label="failed to resolve AOSP HOST_OUT_EXECUTABLES",
    )
    if not host_out:
        raise ProfileError("AOSP HOST_OUT_EXECUTABLES is empty")
    output = Path(host_out)
    if not output.is_absolute():
        output = root / output
    validator = output / AOSP_VALIDATOR_TARGET
    if not validator.is_file():
        raise ProfileError(f"AOSP xsdc Tuner config validator was not produced: {validator}")
    return validator


def validate_xml_with_aosp_consumer(
    xml: str,
    *,
    aosp_root: Path,
    hardware_interfaces_root: Path,
    source_ref: str,
) -> None:
    root = aosp_root.resolve()
    interfaces = hardware_interfaces_root.resolve()
    expected_interfaces = (root / "hardware/interfaces").resolve()
    if interfaces != expected_interfaces:
        raise ProfileError(
            "hardware/interfaces root must belong to the same AOSP tree used to build the validator"
        )

    selected_xsd(interfaces, source_ref)
    try:
        ElementTree.fromstring(xml)
    except ElementTree.ParseError as exc:
        raise ProfileError(f"generated VTS XML is not well-formed: {exc}") from exc

    validator = _build_aosp_consumer(root)
    with tempfile.NamedTemporaryFile("w", encoding="utf-8", suffix=".xml", delete=True) as tmp:
        tmp.write(xml)
        tmp.flush()
        try:
            result = subprocess.run(
                [str(validator), tmp.name],
                cwd=root,
                capture_output=True,
                text=True,
            )
        except OSError as exc:
            raise ProfileError(f"failed to execute AOSP xsdc Tuner config validator: {exc}") from exc
        if result.returncode != 0:
            detail = (result.stderr or result.stdout).strip()
            raise ProfileError(
                "generated VTS XML is rejected by the selected AOSP xsdc Tuner config consumer"
                + (f": {detail}" if detail else "")
            )

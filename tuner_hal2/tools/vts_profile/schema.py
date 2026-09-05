from __future__ import annotations
import subprocess
import tempfile
from pathlib import Path
from xml.etree import ElementTree
from .model import ProfileError

XSD_RELATIVE_PATH = Path("tv/tuner/config/tuner_testing_dynamic_configuration.xsd")


def _git_commit(root: Path, ref: str) -> str:
    try:
        result = subprocess.run(
            ["git", "-C", str(root), "rev-parse", f"{ref}^{{commit}}"],
            check=True, capture_output=True, text=True,
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


def validate_xml(xml: str, xsd: Path, *, xmllint: str = "xmllint") -> None:
    try:
        ElementTree.fromstring(xml)
    except ElementTree.ParseError as exc:
        raise ProfileError(f"generated VTS XML is not well-formed: {exc}") from exc
    with tempfile.NamedTemporaryFile("w", encoding="utf-8", suffix=".xml", delete=True) as tmp:
        tmp.write(xml)
        tmp.flush()
        try:
            result = subprocess.run(
                [xmllint, "--noout", "--schema", str(xsd), tmp.name],
                capture_output=True, text=True,
            )
        except OSError as exc:
            raise ProfileError(f"failed to execute XSD validator {xmllint!r}: {exc}") from exc
        if result.returncode != 0:
            detail = (result.stderr or result.stdout).strip()
            raise ProfileError(f"generated VTS XML does not satisfy selected AOSP XSD: {detail}")

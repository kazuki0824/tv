from __future__ import annotations

import subprocess
import tempfile
from pathlib import Path
from xml.etree import ElementTree

import xmlschema

from .model import ProfileError

XSD_RELATIVE_PATH = Path("tv/tuner/config/tuner_testing_dynamic_configuration.xsd")


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


def _validate_with_external_command(xml: str, xsd: Path, command: str) -> None:
    with tempfile.NamedTemporaryFile("w", encoding="utf-8", suffix=".xml", delete=True) as tmp:
        tmp.write(xml)
        tmp.flush()
        try:
            result = subprocess.run(
                [command, "--noout", "--schema", str(xsd), tmp.name],
                capture_output=True,
                text=True,
            )
        except OSError as exc:
            raise ProfileError(f"failed to execute XSD validator {command!r}: {exc}") from exc
        if result.returncode != 0:
            detail = (result.stderr or result.stdout).strip()
            raise ProfileError(f"generated VTS XML does not satisfy selected AOSP XSD: {detail}")


def validate_xml(xml: str, xsd: Path, *, xmllint: str = "xmllint") -> None:
    try:
        ElementTree.fromstring(xml)
    except ElementTree.ParseError as exc:
        raise ProfileError(f"generated VTS XML is not well-formed: {exc}") from exc

    # Historical callers expose --xmllint. The default path is intentionally no longer
    # libxml2: the Android 14 Tuner schema uses XSD 1.1 constructs that libxml2 rejects.
    # A non-default executable remains an explicit test/compatibility hook only.
    if xmllint != "xmllint":
        _validate_with_external_command(xml, xsd, xmllint)
        return

    try:
        schema = xmlschema.XMLSchema11(str(xsd))
        schema.validate(xml)
    except (xmlschema.XMLSchemaException, OSError) as exc:
        raise ProfileError(f"generated VTS XML does not satisfy selected AOSP XSD: {exc}") from exc

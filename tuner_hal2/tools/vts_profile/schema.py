from __future__ import annotations

import subprocess
import tempfile
from collections import defaultdict
from pathlib import Path
from xml.etree import ElementTree

from .model import ProfileError

XSD_RELATIVE_PATH = Path("tv/tuner/config/tuner_testing_dynamic_configuration.xsd")
_XS = "{http://www.w3.org/2001/XMLSchema}"


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


def _required_attributes(complex_type: ElementTree.Element | None) -> frozenset[str]:
    if complex_type is None:
        return frozenset()
    return frozenset(
        attr.attrib["name"]
        for attr in complex_type.findall(f"{_XS}attribute")
        if attr.attrib.get("use") == "required" and "name" in attr.attrib
    )


def _aosp_xsd_required_attribute_contracts(
    xsd_root: ElementTree.Element,
) -> dict[str, tuple[frozenset[str], ...]]:
    named_types = {
        item.attrib["name"]: item
        for item in xsd_root.findall(f".//{_XS}complexType")
        if "name" in item.attrib
    }
    contracts: dict[str, list[frozenset[str]]] = defaultdict(list)
    for element in xsd_root.findall(f".//{_XS}element"):
        name = element.attrib.get("name")
        if not name:
            continue
        type_name = element.attrib.get("type")
        if type_name:
            required = _required_attributes(named_types.get(type_name))
        else:
            required = _required_attributes(element.find(f"{_XS}complexType"))
        if required not in contracts[name]:
            contracts[name].append(required)
    return {name: tuple(values) for name, values in contracts.items()}


def _validate_aosp_xsd_config_contract(xml: str, xsd: Path) -> None:
    try:
        xsd_root = ElementTree.parse(xsd).getroot()
    except (OSError, ElementTree.ParseError) as exc:
        raise ProfileError(f"failed to read selected AOSP Tuner XSD: {exc}") from exc
    contracts = _aosp_xsd_required_attribute_contracts(xsd_root)
    try:
        xml_root = ElementTree.fromstring(xml)
    except ElementTree.ParseError as exc:
        raise ProfileError(f"generated VTS XML is not well-formed: {exc}") from exc

    for element in xml_root.iter():
        local_name = element.tag.rsplit("}", 1)[-1]
        candidates = contracts.get(local_name)
        if not candidates:
            raise ProfileError(
                f"generated VTS XML element {local_name!r} is not declared by selected AOSP XSD"
            )
        actual = frozenset(element.attrib)
        if any(required <= actual for required in candidates):
            continue
        missing_sets = [sorted(required - actual) for required in candidates]
        raise ProfileError(
            f"generated VTS XML element {local_name!r} misses required AOSP XSD attributes: "
            f"{missing_sets}"
        )


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
    # AOSP feeds this file to Soong xsd_config. It is XML-well-formed, but some
    # revisions are not legal W3C XSD and are rejected by generic validators.
    # The default host check therefore consumes the selected AOSP file itself and
    # verifies every emitted element against its owning type's required attributes.
    # A non-default executable remains an explicit compatibility/test hook.
    if xmllint != "xmllint":
        _validate_with_external_command(xml, xsd, xmllint)
        return
    _validate_aosp_xsd_config_contract(xml, xsd)

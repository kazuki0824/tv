from __future__ import annotations

from pathlib import Path
from typing import Any

from .model import validate_profile
from .render import render_xml
from .resource_closure import (
    DEFAULT_CAPABILITY_SOURCE,
    DEFAULT_PES_SOURCE,
    DEFAULT_PLAYBACK_SOURCE,
    validate_resource_closure,
)
from .schema import selected_xsd, validate_xml


def validated_xml(
    profile: dict[str, Any],
    *,
    hardware_interfaces_root: Path,
    capability_source: Path = DEFAULT_CAPABILITY_SOURCE,
    pes_source: Path = DEFAULT_PES_SOURCE,
    playback_source: Path = DEFAULT_PLAYBACK_SOURCE,
    rustc: str = "rustc",
    xmllint: str = "xmllint",
) -> str:
    validate_profile(profile, require_resolved=True)
    validate_resource_closure(
        profile,
        capability_source=capability_source,
        pes_source=pes_source,
        playback_source=playback_source,
        rustc=rustc,
    )
    xml = render_xml(profile)
    xsd = selected_xsd(hardware_interfaces_root, profile["vts"]["source_ref"])
    validate_xml(xml, xsd, xmllint=xmllint)
    return xml

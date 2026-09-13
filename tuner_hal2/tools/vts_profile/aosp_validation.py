from __future__ import annotations

import argparse
import sys
from pathlib import Path

from .model import ProfileError, load_profile, validate_profile
from .render import render_xml
from .schema import validate_xml_with_aosp_consumer


def validate_profile_xml(profile_path: Path, aosp_root: Path) -> None:
    profile = load_profile(profile_path)
    validate_profile(profile, require_resolved=True)
    source_ref = str(profile["vts"]["source_ref"])
    validate_xml_with_aosp_consumer(
        render_xml(profile),
        aosp_root=aosp_root,
        hardware_interfaces_root=aosp_root / "hardware/interfaces",
        source_ref=source_ref,
    )


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description=(
            "Validate a generated tuner_hal2 VTS profile with the exact AOSP xsdc consumer "
            "built from the selected source tree."
        )
    )
    parser.add_argument("profile", type=Path)
    parser.add_argument("--aosp-root", type=Path, required=True)
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        validate_profile_xml(args.profile, args.aosp_root)
    except ProfileError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2
    print("ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

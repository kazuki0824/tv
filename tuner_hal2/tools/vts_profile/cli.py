from __future__ import annotations

import argparse
import sys
from pathlib import Path

from .model import (
    DEFAULT_CAPABILITY_SOURCE,
    FRONTEND_ID,
    ProfileError,
    SCHEMA_VERSION,
    SUPPORTED_VTS_CONTRACT,
    load_profile,
    parse_capability_source,
    positive_int,
    save_profile,
    validate_against_capability,
    validate_profile,
)
from .region import resolve_region, select_candidate
from .render import output_filename, render_xml

DEFAULT_PROFILE = Path("tuner_hal2/config/vts_environment_profile.json")


def _value(args: argparse.Namespace, name: str, label: str, *, optional: bool = False) -> str:
    current = getattr(args, name)
    if current not in (None, ""):
        return str(current)
    if not args.non_interactive:
        entered = input(f"{label}: ").strip()
        if entered or optional:
            return entered
    if optional:
        return ""
    raise ProfileError(f"{name.replace('_', '-')} is required")


def _new_profile(args: argparse.Namespace) -> dict:
    backend = _value(args, "backend", "backend")
    product = _value(args, "product", "product")
    fe_type = _value(args, "delivery_system", "delivery system (ISDBT/ISDBS)").upper()
    source_ref = _value(args, "vts_source_ref", "selected AOSP Tuner VTS tag/commit")
    region = _value(args, "region", "region/postal/address (optional)", optional=True)
    frequency = _value(args, "frequency_hz", "frequency Hz (optional)", optional=True)
    record_enabled = _value(args, "record", "record flow (yes/no)").lower() in {"y", "yes", "true", "1"}
    record_pid = _value(args, "record_pid", "record PID (optional)", optional=True) if record_enabled else ""
    scan_enabled = _value(args, "scan", "scan flow (yes/no)").lower() in {"y", "yes", "true", "1"}

    queues: dict[str, int] = {}
    if record_enabled:
        queues["record_filter_bytes"] = positive_int(
            _value(args, "record_filter_bytes", "record filter buffer bytes"), "record_filter_bytes"
        )
        queues["record_dvr_bytes"] = positive_int(
            _value(args, "record_dvr_bytes", "record DVR buffer bytes"), "record_dvr_bytes"
        )

    profile: dict = {
        "schema_version": SCHEMA_VERSION,
        "target": {"hal": "tuner_hal2", "product": product, "backend": backend},
        "vts": {"contract": SUPPORTED_VTS_CONTRACT, "source_ref": source_ref, "variant": args.variant or ""},
        "frontend": {
            "type": fe_type,
            "is_software_frontend": False,
            "frequency_hz": int(frequency) if frequency else None,
        },
        "flows": {
            "scan": scan_enabled,
            "record": {"enabled": True, "pid": int(record_pid) if record_pid else None} if record_enabled else {"enabled": False},
            "clear_live": {"enabled": False},
        },
        "queues": queues,
    }
    if region:
        profile["region"] = {"query": region, "candidates": []}
    validate_profile(profile)
    return profile


def cmd_init(args: argparse.Namespace) -> int:
    profile = _new_profile(args)
    save_profile(Path(args.profile), profile)
    print(args.profile)
    return 0


def cmd_resolve_region(args: argparse.Namespace) -> int:
    path = Path(args.profile)
    profile = load_profile(path)
    validate_profile(profile)
    dataset = load_profile(Path(args.dataset))
    resolve_region(profile, dataset, args.select_index)
    save_profile(path, profile)
    for index, candidate in enumerate(profile["region"]["candidates"]):
        channel = candidate.get("physical_channel")
        print(f"[{index}] {candidate['frequency_hz']} Hz ch={channel if channel is not None else '-'} {candidate.get('label', '')}".rstrip())
    return 0


def cmd_select_candidate(args: argparse.Namespace) -> int:
    path = Path(args.profile)
    profile = load_profile(path)
    validate_profile(profile)
    select_candidate(profile, args.index)
    save_profile(path, profile)
    print(profile["frontend"]["frequency_hz"])
    return 0


def cmd_validate(args: argparse.Namespace) -> int:
    profile = load_profile(Path(args.profile))
    validate_profile(profile, require_resolved=args.resolved)
    validate_against_capability(profile, parse_capability_source(Path(args.capability_source)))
    print("ok")
    return 0


def cmd_compile(args: argparse.Namespace) -> int:
    profile = load_profile(Path(args.profile))
    if profile.get("vts", {}).get("source_ref") != args.selected_vts_source_ref:
        raise ProfileError("profile vts.source_ref does not match --selected-vts-source-ref")
    capability = parse_capability_source(Path(args.capability_source))
    xml = render_xml(profile, capability)
    output = Path(args.output) if args.output else Path(args.output_dir) / output_filename(profile)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(xml, encoding="utf-8")
    print(output)
    return 0


def cmd_resolve_device(_args: argparse.Namespace) -> int:
    raise ProfileError(
        "resolve-device needs the device-side public-AIDL resolver; "
        "init/resolve-region/select-candidate/validate/compile are hardware-independent"
    )


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Configure tuner_hal2 AOSP Tuner VTS without hardware.")
    sub = parser.add_subparsers(dest="command", required=True)

    init = sub.add_parser("init")
    init.add_argument("profile", nargs="?", default=str(DEFAULT_PROFILE))
    init.add_argument("--non-interactive", action="store_true")
    init.add_argument("--backend")
    init.add_argument("--product")
    init.add_argument("--delivery-system", choices=sorted(FRONTEND_ID))
    init.add_argument("--region")
    init.add_argument("--frequency-hz")
    init.add_argument("--record")
    init.add_argument("--record-pid")
    init.add_argument("--scan")
    init.add_argument("--vts-source-ref")
    init.add_argument("--record-filter-bytes")
    init.add_argument("--record-dvr-bytes")
    init.add_argument("--variant", default="")
    init.set_defaults(func=cmd_init)

    region = sub.add_parser("resolve-region")
    region.add_argument("profile")
    region.add_argument("--dataset", required=True)
    region.add_argument("--select-index", type=int)
    region.set_defaults(func=cmd_resolve_region)

    select = sub.add_parser("select-candidate")
    select.add_argument("profile")
    select.add_argument("index", type=int)
    select.set_defaults(func=cmd_select_candidate)

    validate = sub.add_parser("validate")
    validate.add_argument("profile")
    validate.add_argument("--resolved", action="store_true")
    validate.add_argument("--capability-source", default=str(DEFAULT_CAPABILITY_SOURCE))
    validate.set_defaults(func=cmd_validate)

    compile_cmd = sub.add_parser("compile")
    compile_cmd.add_argument("profile")
    compile_cmd.add_argument("--capability-source", default=str(DEFAULT_CAPABILITY_SOURCE))
    compile_cmd.add_argument("--selected-vts-source-ref", required=True)
    compile_cmd.add_argument("--output")
    compile_cmd.add_argument("--output-dir", default="out/vts")
    compile_cmd.set_defaults(func=cmd_compile)

    device = sub.add_parser("resolve-device")
    device.add_argument("profile")
    device.set_defaults(func=cmd_resolve_device)
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        return args.func(args)
    except ProfileError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2

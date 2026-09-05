from __future__ import annotations

import argparse
import os
import sys
from pathlib import Path

from .device import resolve_device
from .integration import write_product_artifacts
from .model import (
    FRONTEND_ID,
    ProfileError,
    RECORD_FILTER_FMQ_PROBE_VARIANT,
    SCHEMA_VERSION,
    SUPPORTED_VTS_CONTRACT,
    load_json,
    load_profile,
    positive_int,
    save_profile,
    validate_profile,
)
from .region import resolve_region, select_candidate
from .render import output_filename, render_xml
from .resource_closure import (
    DEFAULT_CAPABILITY_SOURCE,
    DEFAULT_PES_SOURCE,
    DEFAULT_PLAYBACK_SOURCE,
    validate_resource_closure,
)
from .schema import checkout_commit, selected_xsd, validate_xml

_REPO_ROOT = Path(__file__).resolve().parents[3]
DEFAULT_PROFILE = _REPO_ROOT / "tuner_hal2/config/vts_environment_profile.json"
DEFAULT_PRODUCT = "virtio_x86_64_tv_grub"
DEFAULT_BACKEND = "px4"
SUPPORTED_BACKENDS = ("px4", "earth_pt1")
DEFAULT_DELIVERY_SYSTEM = "ISDBT"
DEFAULT_RECORD_FILTER_BYTES = 16 * 1024 * 1024
DEFAULT_RECORD_DVR_BYTES = 4 * 1024 * 1024
DEFAULT_AV_FILTER_BYTES = 16 * 1024 * 1024
DEFAULT_PCR_FILTER_BYTES = 16 * 1024 * 1024
DEFAULT_SECTION_FILTER_BYTES = 16 * 1024 * 1024
DEFAULT_PLAYBACK_DVR_BYTES = 4 * 1024 * 1024
DEFAULT_PLAYBACK_INPUT_PATH = "/data/local/tmp/segment000000.ts"
DEFAULT_PLAYBACK_VIDEO_PID = 256
DEFAULT_PLAYBACK_AUDIO_PID = 257
DEFAULT_PLAYBACK_SECTION_PID = 257
DEFAULT_PLAYBACK_VIDEO_STREAM_TYPE = 2
DEFAULT_PLAYBACK_AUDIO_STREAM_TYPE = 2
DEFAULT_ISDBS_LNB_VOLTAGE = "NONE"
DEFAULT_ISDBS_LNB_TONE = "NONE"
DEFAULT_ISDBS_LNB_POSITION = "UNDEFINED"
_TRUE_VALUES = {"y", "yes", "true", "1"}
_FALSE_VALUES = {"n", "no", "false", "0"}


def _choice(
    args: argparse.Namespace,
    name: str,
    label: str,
    choices: tuple[str, ...],
    default: str,
) -> str:
    current = getattr(args, name, None)
    if current not in (None, ""):
        value = str(current)
        for choice in choices:
            if value.lower() == choice.lower():
                return choice
        raise ProfileError(f"{name.replace('_', '-')} must be one of: {', '.join(choices)}")
    if args.non_interactive:
        return default
    default_index = choices.index(default) + 1
    print(f"{label}:")
    for index, choice in enumerate(choices, start=1):
        suffix = " (default)" if choice == default else ""
        print(f"  {index}) {choice}{suffix}")
    while True:
        entered = input(f"select [{default_index}]: ").strip()
        if not entered:
            return default
        if entered.isdigit():
            index = int(entered)
            if 1 <= index <= len(choices):
                return choices[index - 1]
        for choice in choices:
            if entered.lower() == choice.lower():
                return choice
        print(f"invalid selection: {entered}", file=sys.stderr)


def _optional_value(args: argparse.Namespace, name: str, label: str) -> str:
    current = getattr(args, name, None)
    if current not in (None, ""):
        return str(current)
    if args.non_interactive:
        return ""
    return input(f"{label} (optional): ").strip()


def _boolean_option(value: object, name: str, *, default: bool) -> bool:
    if value in (None, ""):
        return default
    normalized = str(value).strip().lower()
    if normalized in _TRUE_VALUES:
        return True
    if normalized in _FALSE_VALUES:
        return False
    raise ProfileError(f"{name} must be yes/no")


def _product(args: argparse.Namespace) -> str:
    if getattr(args, "product", None) not in (None, ""):
        return str(args.product)
    target_device = os.environ.get("TARGET_DEVICE", "").strip()
    if target_device:
        return target_device
    target_product = os.environ.get("TARGET_PRODUCT", "").strip()
    if target_product:
        prefix = "lineage_"
        return target_product[len(prefix):] if target_product.startswith(prefix) else target_product
    return DEFAULT_PRODUCT


def _hardware_interfaces_root(args: argparse.Namespace) -> Path | None:
    explicit = getattr(args, "hardware_interfaces_root", None)
    if explicit:
        return Path(explicit).expanduser().resolve()
    android_top = os.environ.get("ANDROID_BUILD_TOP", "").strip()
    if android_top:
        candidate = Path(android_top).expanduser().resolve() / "hardware/interfaces"
        if candidate.is_dir():
            return candidate
    if (
        _REPO_ROOT.name == "tv"
        and _REPO_ROOT.parent.name == "maleicacid"
        and _REPO_ROOT.parent.parent.name == "vendor"
    ):
        candidate = _REPO_ROOT.parent.parent.parent / "hardware/interfaces"
        if candidate.is_dir():
            return candidate.resolve()
    return None


def _source_ref(args: argparse.Namespace) -> str:
    explicit = getattr(args, "vts_source_ref", None)
    if explicit not in (None, ""):
        return str(explicit)
    hardware_interfaces_root = _hardware_interfaces_root(args)
    if hardware_interfaces_root is None:
        raise ProfileError(
            "cannot locate hardware/interfaces checkout; initialize the Android build environment "
            "or pass --hardware-interfaces-root/--vts-source-ref"
        )
    return checkout_commit(hardware_interfaces_root)


def _new_profile(args: argparse.Namespace) -> dict:
    backend = _choice(
        args,
        "backend",
        "backend",
        SUPPORTED_BACKENDS,
        DEFAULT_BACKEND,
    )
    product = _product(args)
    fe_type = _choice(
        args,
        "delivery_system",
        "delivery system",
        tuple(FRONTEND_ID),
        DEFAULT_DELIVERY_SYSTEM,
    )
    source_ref = _source_ref(args)
    variant = getattr(args, "variant", "") or ""
    full_coverage = variant != RECORD_FILTER_FMQ_PROBE_VARIANT
    record_enabled = _boolean_option(getattr(args, "record", None), "record", default=True)
    scan_enabled = _boolean_option(getattr(args, "scan", None), "scan", default=True)
    if not full_coverage:
        scan_enabled = False

    region = _optional_value(args, "region", "region address/postal/latitude,longitude")
    frequency = _optional_value(args, "frequency_hz", "frequency Hz")
    service_id = _optional_value(args, "service_id", "service ID")
    record_pid = _optional_value(args, "record_pid", "record PID") if record_enabled else ""

    playback_dvr_bytes = getattr(args, "playback_dvr_bytes", DEFAULT_PLAYBACK_DVR_BYTES)
    playback_input_path = getattr(args, "playback_input_path", DEFAULT_PLAYBACK_INPUT_PATH)
    playback_audio_pid = getattr(args, "playback_audio_pid", DEFAULT_PLAYBACK_AUDIO_PID)
    playback_video_pid = getattr(args, "playback_video_pid", DEFAULT_PLAYBACK_VIDEO_PID)
    playback_section_pid = getattr(args, "playback_section_pid", DEFAULT_PLAYBACK_SECTION_PID)
    playback_audio_stream_type = getattr(
        args, "playback_audio_stream_type", DEFAULT_PLAYBACK_AUDIO_STREAM_TYPE
    )
    playback_video_stream_type = getattr(
        args, "playback_video_stream_type", DEFAULT_PLAYBACK_VIDEO_STREAM_TYPE
    )

    queues: dict[str, int] = {}
    if record_enabled:
        queues["record_filter_bytes"] = positive_int(
            args.record_filter_bytes if args.record_filter_bytes is not None else DEFAULT_RECORD_FILTER_BYTES,
            "record_filter_bytes",
        )
        queues["record_dvr_bytes"] = positive_int(
            args.record_dvr_bytes if args.record_dvr_bytes is not None else DEFAULT_RECORD_DVR_BYTES,
            "record_dvr_bytes",
        )
    if full_coverage:
        queues.update(
            {
                "audio_filter_bytes": DEFAULT_AV_FILTER_BYTES,
                "video_filter_bytes": DEFAULT_AV_FILTER_BYTES,
                "pcr_filter_bytes": DEFAULT_PCR_FILTER_BYTES,
                "section_filter_bytes": DEFAULT_SECTION_FILTER_BYTES,
                "playback_dvr_bytes": positive_int(playback_dvr_bytes, "playback_dvr_bytes"),
            }
        )

    profile: dict = {
        "schema_version": SCHEMA_VERSION,
        "target": {"hal": "tuner_hal2", "product": product, "backend": backend},
        "vts": {"contract": SUPPORTED_VTS_CONTRACT, "source_ref": source_ref, "variant": variant},
        "frontend": {"type": fe_type, "is_software_frontend": False, "frequency_hz": int(frequency) if frequency else None},
        "flows": {
            "scan": scan_enabled,
            "record": {"enabled": True, "pid": int(record_pid) if record_pid else None} if record_enabled else {"enabled": False},
            "clear_live": (
                {
                    "enabled": True,
                    "audio_pid": None,
                    "video_pid": None,
                    "audio_stream_type": None,
                    "video_stream_type": None,
                    "pcr_pid": None,
                    "section_pid": None,
                }
                if full_coverage
                else {"enabled": False}
            ),
            "playback": (
                {
                    "enabled": True,
                    "input_file_path": playback_input_path,
                    "audio_pid": int(playback_audio_pid),
                    "video_pid": int(playback_video_pid),
                    "section_pid": int(playback_section_pid),
                    "audio_stream_type": int(playback_audio_stream_type),
                    "video_stream_type": int(playback_video_stream_type),
                }
                if full_coverage
                else {"enabled": False}
            ),
        },
        "queues": queues,
    }
    if fe_type == "ISDBS" and full_coverage:
        profile["lnb"] = {
            "voltage": DEFAULT_ISDBS_LNB_VOLTAGE,
            "tone": DEFAULT_ISDBS_LNB_TONE,
            "position": DEFAULT_ISDBS_LNB_POSITION,
        }
    if region:
        profile["region"] = {"query": region, "candidates": []}
    if service_id:
        profile["service"] = {"service_id": int(service_id)}
    validate_profile(profile)
    return profile


def _print_region_candidates(profile: dict) -> None:
    for index, candidate in enumerate(profile["region"]["candidates"]):
        channel = candidate.get("physical_channel")
        print(
            f"[{index}] {candidate['frequency_hz']} Hz "
            f"ch={channel if channel is not None else '-'} {candidate.get('label', '')}".rstrip()
        )


def cmd_init(args: argparse.Namespace) -> int:
    path = Path(args.profile)
    profile = _new_profile(args)
    # initの入力はresolverとは独立したcheckpointとして先に保存する。
    save_profile(path, profile)
    print(args.profile)
    if (
        not args.non_interactive
        and profile["frontend"]["type"] == "ISDBT"
        and "region" in profile
    ):
        resolve_region(profile)
        save_profile(path, profile)
        _print_region_candidates(profile)
    return 0


def cmd_resolve_region(args: argparse.Namespace) -> int:
    path = Path(args.profile)
    profile = load_profile(path)
    validate_profile(profile)
    dataset = load_json(Path(args.dataset)) if args.dataset else None
    resolve_region(profile, dataset, args.select_index)
    save_profile(path, profile)
    _print_region_candidates(profile)
    return 0


def cmd_select_candidate(args: argparse.Namespace) -> int:
    path = Path(args.profile)
    profile = load_profile(path)
    validate_profile(profile)
    select_candidate(profile, args.index)
    save_profile(path, profile)
    print(profile["frontend"]["frequency_hz"])
    return 0


def _validate_closure(profile: dict, args: argparse.Namespace) -> None:
    validate_resource_closure(
        profile,
        capability_source=Path(args.capability_source),
        pes_source=Path(args.pes_source),
        playback_source=Path(args.playback_source),
        rustc=args.rustc,
    )


def cmd_validate(args: argparse.Namespace) -> int:
    profile = load_profile(Path(args.profile))
    validate_profile(profile, require_resolved=args.resolved)
    _validate_closure(profile, args)
    print("ok")
    return 0


def cmd_compile(args: argparse.Namespace) -> int:
    profile = load_profile(Path(args.profile))
    validate_profile(profile, require_resolved=True)
    _validate_closure(profile, args)
    xml = render_xml(profile)
    hardware_interfaces_root = _hardware_interfaces_root(args)
    if hardware_interfaces_root is None:
        raise ProfileError(
            "cannot locate hardware/interfaces checkout; initialize the Android build environment "
            "or pass --hardware-interfaces-root"
        )
    xsd = selected_xsd(hardware_interfaces_root, profile["vts"]["source_ref"])
    validate_xml(xml, xsd, xmllint=args.xmllint)
    if args.product_integration_dir:
        if args.output:
            raise ProfileError("--output cannot be combined with --product-integration-dir")
        output = write_product_artifacts(profile, xml, Path(args.product_integration_dir))
    else:
        output = Path(args.output) if args.output else Path(args.output_dir) / output_filename(profile)
        output.parent.mkdir(parents=True, exist_ok=True)
        tmp = output.with_name(output.name + ".tmp")
        tmp.write_text(xml, encoding="utf-8")
        tmp.replace(output)
    print(output)
    return 0


def _interactive_service_selector(services: list[dict]) -> int:
    print("service:")
    for index, service in enumerate(services, start=1):
        service_id = int(service["service_id"])
        pmt_pid = service.get("pmt_pid")
        streams = service.get("streams")
        stream_types = sorted(
            {
                int(item["stream_type"])
                for item in streams
                if isinstance(item, dict) and item.get("stream_type") is not None
            }
        ) if isinstance(streams, list) else []
        print(
            f"  {index}) service_id={service_id} "
            f"pmt_pid={pmt_pid if pmt_pid is not None else '-'} "
            f"stream_types={','.join(hex(value) for value in stream_types) or '-'}"
        )
    by_id = {int(item["service_id"]) for item in services}
    while True:
        try:
            entered = input("select: ").strip()
        except EOFError as exc:
            raise ProfileError(
                "service selection requires an interactive choice or --service-id"
            ) from exc
        if entered.isdigit():
            value = int(entered)
            if 1 <= value <= len(services):
                return int(services[value - 1]["service_id"])
            if value in by_id:
                return value
        print(f"invalid selection: {entered}", file=sys.stderr)


def cmd_resolve_device(args: argparse.Namespace) -> int:
    service_selector = (
        _interactive_service_selector
        if args.service_id is None and sys.stdin.isatty()
        else None
    )
    updated = resolve_device(
        Path(args.profile),
        adb=args.adb,
        serial=args.serial,
        agent_binary=Path(args.agent) if args.agent else None,
        remote_agent=args.remote_agent,
        si_host=args.si_host,
        timeout_ms=args.timeout_ms,
        candidate_index=args.candidate_index,
        service_id=args.service_id,
        service_selector=service_selector,
    )
    print(updated["frontend"]["frequency_hz"])
    return 0

def _add_profile_arg(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("profile", nargs="?", default=str(DEFAULT_PROFILE))


def _add_closure_args(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--capability-source", default=str(DEFAULT_CAPABILITY_SOURCE))
    parser.add_argument("--pes-source", default=str(DEFAULT_PES_SOURCE))
    parser.add_argument("--playback-source", default=str(DEFAULT_PLAYBACK_SOURCE))
    parser.add_argument("--rustc", default="rustc")


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Configure tuner_hal2 AOSP Tuner VTS.")
    sub = parser.add_subparsers(dest="command", required=True)
    init = sub.add_parser("init")
    _add_profile_arg(init)
    init.add_argument("--non-interactive", action="store_true")
    init.add_argument("--backend", choices=SUPPORTED_BACKENDS)
    init.add_argument("--product")
    init.add_argument("--delivery-system", choices=sorted(FRONTEND_ID))
    init.add_argument("--region")
    init.add_argument("--frequency-hz")
    init.add_argument("--service-id")
    init.add_argument("--record")
    init.add_argument("--record-pid")
    init.add_argument("--scan")
    init.add_argument(
        "--vts-source-ref",
        help="override the exact hardware/interfaces checkout commit detected from the Android tree",
    )
    init.add_argument(
        "--hardware-interfaces-root",
        help="hardware/interfaces checkout used to auto-detect the exact VTS source commit",
    )
    init.add_argument(
        "--record-filter-bytes",
        type=int,
        default=DEFAULT_RECORD_FILTER_BYTES,
        help=f"TS RECORD filter buffer bytes (default: {DEFAULT_RECORD_FILTER_BYTES})",
    )
    init.add_argument(
        "--record-dvr-bytes",
        type=int,
        default=DEFAULT_RECORD_DVR_BYTES,
        help=f"RECORD DVR buffer bytes (default: {DEFAULT_RECORD_DVR_BYTES})",
    )
    init.add_argument(
        "--playback-dvr-bytes",
        type=int,
        default=DEFAULT_PLAYBACK_DVR_BYTES,
        help=f"PLAYBACK DVR buffer bytes (default: {DEFAULT_PLAYBACK_DVR_BYTES})",
    )
    init.add_argument(
        "--playback-input-path",
        default=DEFAULT_PLAYBACK_INPUT_PATH,
        help=f"device-side TS input file for DVR playback (default: {DEFAULT_PLAYBACK_INPUT_PATH})",
    )
    init.add_argument("--playback-audio-pid", type=int, default=DEFAULT_PLAYBACK_AUDIO_PID)
    init.add_argument("--playback-video-pid", type=int, default=DEFAULT_PLAYBACK_VIDEO_PID)
    init.add_argument("--playback-section-pid", type=int, default=DEFAULT_PLAYBACK_SECTION_PID)
    init.add_argument(
        "--playback-audio-stream-type", type=int, default=DEFAULT_PLAYBACK_AUDIO_STREAM_TYPE
    )
    init.add_argument(
        "--playback-video-stream-type", type=int, default=DEFAULT_PLAYBACK_VIDEO_STREAM_TYPE
    )
    init.add_argument("--variant", default="")
    init.set_defaults(func=cmd_init)
    region = sub.add_parser("resolve-region")
    _add_profile_arg(region)
    region.add_argument(
        "--dataset",
        help=(
            "optional explicit region dataset; when omitted, ISDBT uses the "
            "repository snapshot after resolving the region input through coordinates"
        ),
    )
    region.add_argument("--select-index", type=int)
    region.set_defaults(func=cmd_resolve_region)
    select = sub.add_parser("select-candidate")
    _add_profile_arg(select)
    select.add_argument("index", type=int)
    select.set_defaults(func=cmd_select_candidate)
    validate = sub.add_parser("validate")
    _add_profile_arg(validate)
    validate.add_argument("--resolved", action="store_true")
    _add_closure_args(validate)
    validate.set_defaults(func=cmd_validate)
    compile_cmd = sub.add_parser("compile")
    _add_profile_arg(compile_cmd)
    _add_closure_args(compile_cmd)
    compile_cmd.add_argument("--hardware-interfaces-root")
    compile_cmd.add_argument("--xmllint", default="xmllint")
    compile_cmd.add_argument("--output")
    compile_cmd.add_argument("--output-dir", default="out/vts")
    compile_cmd.add_argument("--product-integration-dir")
    compile_cmd.set_defaults(func=cmd_compile)
    device = sub.add_parser("resolve-device")
    _add_profile_arg(device)
    device.add_argument("--adb", default="adb")
    device.add_argument("--serial")
    device.add_argument("--agent", help="local maleicacid_tuner_hal2_vts_agent to adb-push temporarily")
    device.add_argument("--remote-agent", default="/vendor/bin/maleicacid_tuner_hal2_vts_agent",
                        help="agent path in an explicit VTS/test image when --agent is omitted")
    device.add_argument("--si-host", default="maleicacid_arib_si_engine_vts_host")
    device.add_argument("--timeout-ms", type=int, default=5000)
    device.add_argument("--candidate-index", type=int)
    device.add_argument(
        "--service-id",
        type=int,
        help="explicit service selector for non-interactive or preselected resolution",
    )
    device.set_defaults(func=cmd_resolve_device)
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        return args.func(args)
    except ProfileError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2

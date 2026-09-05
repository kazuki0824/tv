from __future__ import annotations

import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from vts_profile.cli import _interactive_service_selector, build_parser
from vts_profile.device import (
    ServiceSelectionRequired,
    _select_service_for_flows,
    resolve_device,
)
from vts_profile.model import ProfileError


class VtsDeviceServiceSelectionTest(unittest.TestCase):
    def test_only_flow_compatible_service_is_selected_automatically(self) -> None:
        services = [{"service_id": 100}, {"service_id": 200}]

        def apply(_profile: dict, service: dict) -> dict:
            if service["service_id"] == 100:
                raise ProfileError("no supported video elementary stream")
            return {"resolved": service["service_id"]}

        with patch("vts_profile.device._apply", side_effect=apply):
            selected = _select_service_for_flows(
                {},
                services,
                473_142_857,
                requested_service_id=None,
                service_selector=None,
            )
        self.assertEqual(selected["service_id"], 200)
        self.assertEqual(selected["frequency_hz"], 473_142_857)

    def test_multiple_compatible_services_require_explicit_selection(self) -> None:
        services = [{"service_id": 200}, {"service_id": 100}]
        with patch("vts_profile.device._apply", return_value={"resolved": True}):
            with self.assertRaisesRegex(
                ServiceSelectionRequired,
                r"multiple services satisfy.*\(100,200\)",
            ):
                _select_service_for_flows(
                    {},
                    services,
                    473_142_857,
                    requested_service_id=None,
                    service_selector=None,
                )

    def test_interactive_selector_chooses_one_compatible_service(self) -> None:
        services = [{"service_id": 100}, {"service_id": 200}]
        seen: list[list[int]] = []

        def selector(candidates: list[dict]) -> int:
            seen.append([int(item["service_id"]) for item in candidates])
            return 200

        with patch("vts_profile.device._apply", return_value={"resolved": True}):
            selected = _select_service_for_flows(
                {},
                services,
                473_142_857,
                requested_service_id=None,
                service_selector=selector,
            )
        self.assertEqual(seen, [[100, 200]])
        self.assertEqual(selected["service_id"], 200)

    def test_explicit_service_id_selects_matching_service(self) -> None:
        services = [{"service_id": 100}, {"service_id": 200}]
        with patch("vts_profile.device._apply", return_value={"resolved": True}):
            selected = _select_service_for_flows(
                {},
                services,
                473_142_857,
                requested_service_id=200,
                service_selector=None,
            )
        self.assertEqual(selected["service_id"], 200)

    def test_ambiguity_does_not_fall_through_to_next_frequency(self) -> None:
        profile = {
            "frontend": {"frequency_hz": None},
            "region": {
                "candidates": [
                    {"frequency_hz": 473_142_857},
                    {"frequency_hz": 479_142_857},
                ]
            },
        }
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "profile.json"
            path.write_text("{}", encoding="utf-8")
            with (
                patch("vts_profile.model.load_profile", return_value=profile),
                patch("vts_profile.device.validate_profile"),
                patch("vts_profile.device._prepare_agent", return_value=("/vendor/bin/agent", False)),
                patch(
                    "vts_profile.device._resolve_frequency",
                    side_effect=ServiceSelectionRequired("choose service"),
                ) as resolve_frequency,
                patch("vts_profile.device._cleanup_agent"),
            ):
                with self.assertRaisesRegex(ServiceSelectionRequired, "choose service"):
                    resolve_device(path)
        self.assertEqual(resolve_frequency.call_count, 1)
        self.assertEqual(resolve_frequency.call_args.args[1], 473_142_857)

    def test_cli_interactive_selector_accepts_numbered_choice(self) -> None:
        services = [
            {"service_id": 100, "pmt_pid": 256, "streams": [{"stream_type": 0x1B}]},
            {"service_id": 200, "pmt_pid": 512, "streams": [{"stream_type": 0x02}]},
        ]
        with patch("builtins.input", return_value="2"):
            self.assertEqual(_interactive_service_selector(services), 200)

    def test_resolve_device_parser_accepts_service_id(self) -> None:
        args = build_parser().parse_args(["resolve-device", "--service-id", "200"])
        self.assertEqual(args.service_id, 200)


if __name__ == "__main__":
    unittest.main()

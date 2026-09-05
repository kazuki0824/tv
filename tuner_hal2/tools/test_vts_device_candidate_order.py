from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import call, patch

from vts_profile.device import resolve_device
from vts_profile.model import ProfileError


class VtsDeviceCandidateOrderTest(unittest.TestCase):
    def test_ranked_candidates_stop_at_first_success(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "profile.json"
            profile = {
                "frontend": {"frequency_hz": None},
                "region": {
                    "candidates": [
                        {"frequency_hz": 473142857},
                        {"frequency_hz": 479142857},
                        {"frequency_hz": 485142857},
                    ]
                },
            }
            path.write_text(json.dumps(profile), encoding="utf-8")
            resolved = {"frequency_hz": 479142857, "service_id": 1}
            updated = {"resolved": True}
            with (
                patch("vts_profile.device.validate_profile"),
                patch("vts_profile.device._prepare_agent", return_value=("/vendor/bin/agent", False)),
                patch(
                    "vts_profile.device._resolve_frequency",
                    side_effect=[ProfileError("no lock"), resolved],
                ) as resolve_frequency,
                patch("vts_profile.device._apply", return_value=updated),
                patch("vts_profile.device.save_profile") as save,
                patch("vts_profile.device._cleanup_agent"),
            ):
                self.assertEqual(resolve_device(path), updated)

            self.assertEqual(
                [item.args[1] for item in resolve_frequency.call_args_list],
                [473142857, 479142857],
            )
            save.assert_called_once_with(path, updated)

    def test_ranked_candidates_report_all_failures_when_none_resolve(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "profile.json"
            profile = {
                "frontend": {"frequency_hz": None},
                "region": {
                    "candidates": [
                        {"frequency_hz": 473142857},
                        {"frequency_hz": 479142857},
                    ]
                },
            }
            path.write_text(json.dumps(profile), encoding="utf-8")
            with (
                patch("vts_profile.device.validate_profile"),
                patch("vts_profile.device._prepare_agent", return_value=("/vendor/bin/agent", False)),
                patch(
                    "vts_profile.device._resolve_frequency",
                    side_effect=[ProfileError("first"), ProfileError("second")],
                ),
                patch("vts_profile.device._cleanup_agent"),
            ):
                with self.assertRaisesRegex(
                    ProfileError,
                    "473142857: first; 479142857: second",
                ):
                    resolve_device(path)


if __name__ == "__main__":
    unittest.main()

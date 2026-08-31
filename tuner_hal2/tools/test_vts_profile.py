from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from vts_profile.cli import _new_profile
from vts_profile.model import ProfileError, parse_capability_source, validate_against_capability, validate_profile
from vts_profile.region import resolve_region, select_candidate
from vts_profile.render import render_xml

CAPABILITY_SOURCE = """
impl CapabilitySnapshot {
    pub const fn product_default() -> Self {
        Self {
            num_record: 8,
            num_playback: 8,
            num_ts_filter: 32,
            num_section_filter: 8,
            num_audio_filter: 0,
            num_video_filter: 0,
            num_pes_filter: 4,
            num_pcr_filter: 4,
            fmq_runtime_budget_bytes: 256 * MIB,
        }
    }
    pub const fn filter_capacity(self) {}
}
"""


class VtsProfileTest(unittest.TestCase):
    def profile(self) -> dict:
        return {
            "schema_version": 1,
            "target": {"hal": "tuner_hal2", "product": "default", "backend": "px4"},
            "vts": {"contract": "android14-aidl-v1", "source_ref": "aosp-commit", "variant": ""},
            "frontend": {
                "type": "ISDBT",
                "is_software_frontend": False,
                "frequency_hz": 557142857,
                "physical_channel": 27,
            },
            "region": {"query": "test", "candidates": []},
            "flows": {
                "scan": True,
                "record": {"enabled": True, "pid": 272},
                "clear_live": {"enabled": False},
            },
            "queues": {"record_filter_bytes": 1048576, "record_dvr_bytes": 4194304},
        }

    def capability(self) -> dict[str, int]:
        return {
            "num_record": 8,
            "num_playback": 8,
            "num_ts_filter": 32,
            "num_section_filter": 8,
            "num_audio_filter": 0,
            "num_video_filter": 0,
            "num_pes_filter": 4,
            "num_pcr_filter": 4,
            "fmq_runtime_budget_bytes": 256 * 1024 * 1024,
        }

    def test_record_only_xml_is_generated_without_device(self) -> None:
        xml = render_xml(self.profile(), self.capability())
        self.assertIn('frequency="557142857"', xml)
        self.assertIn('pid="272"', xml)
        self.assertIn("<scan ", xml)
        self.assertIn("<dvrRecord ", xml)
        self.assertNotIn("clearLiveBroadcast", xml)

    def test_region_resolution_updates_same_profile(self) -> None:
        profile = self.profile()
        profile["frontend"].pop("physical_channel")
        profile["frontend"]["frequency_hz"] = None
        dataset = {
            "schema_version": 1,
            "dataset_version": "v1",
            "entries": [
                {"region": "test", "delivery_system": "ISDBT", "physical_channel": 27,
                 "frequency_hz": 557142857, "label": "A"},
            ],
        }
        resolve_region(profile, dataset)
        self.assertEqual(profile["frontend"]["frequency_hz"], 557142857)
        self.assertEqual(len(profile["region"]["candidates"]), 1)

    def test_select_candidate_uses_saved_candidates(self) -> None:
        profile = self.profile()
        profile["frontend"].pop("physical_channel")
        profile["frontend"]["frequency_hz"] = None
        profile["region"]["candidates"] = [
            {"delivery_system": "ISDBT", "physical_channel": 22, "frequency_hz": 527142857, "label": "A"},
            {"delivery_system": "ISDBT", "physical_channel": 27, "frequency_hz": 557142857, "label": "B"},
        ]
        select_candidate(profile, 1)
        self.assertEqual(profile["frontend"]["frequency_hz"], 557142857)
        self.assertEqual(profile["frontend"]["physical_channel"], 27)

    def test_clear_live_is_rejected_by_current_capability(self) -> None:
        profile = self.profile()
        profile["flows"]["clear_live"] = {
            "enabled": True,
            "audio_pid": 273,
            "video_pid": 272,
            "audio_stream_type": 2,
            "video_stream_type": 2,
        }
        profile["queues"].update({"audio_filter_bytes": 1048576, "video_filter_bytes": 1048576})
        with self.assertRaises(ProfileError):
            validate_against_capability(profile, self.capability())

    def test_unknown_profile_field_is_rejected(self) -> None:
        profile = self.profile()
        profile["unused"] = True
        with self.assertRaises(ProfileError):
            validate_profile(profile)

    def test_capability_is_read_from_rust_ssot(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "capability_snapshot.rs"
            path.write_text(CAPABILITY_SOURCE, encoding="utf-8")
            capability = parse_capability_source(path)
        self.assertEqual(capability["num_audio_filter"], 0)
        self.assertEqual(capability["fmq_runtime_budget_bytes"], 256 * 1024 * 1024)

    def test_noninteractive_init_requires_explicit_inputs(self) -> None:
        class Args:
            non_interactive = True
            backend = "px4"
            product = "default"
            delivery_system = "ISDBT"
            vts_source_ref = "aosp-commit"
            region = None
            frequency_hz = "557142857"
            record = "yes"
            record_pid = "272"
            scan = "yes"
            record_filter_bytes = "1048576"
            record_dvr_bytes = "4194304"
            variant = ""

        profile = _new_profile(Args())
        self.assertEqual(profile["vts"]["source_ref"], "aosp-commit")
        self.assertEqual(profile["queues"]["record_dvr_bytes"], 4194304)


if __name__ == "__main__":
    unittest.main()

from __future__ import annotations

import unittest
from types import SimpleNamespace

from vts_profile.cli import DEFAULT_RECORD_DVR_BYTES, DEFAULT_RECORD_FILTER_BYTES, _new_profile
from vts_profile.model import ProfileError
from vts_profile.region import resolve_region


class VtsRegionDefaultsTest(unittest.TestCase):
    def test_builtin_japan_isdbt_plan_needs_no_dataset(self) -> None:
        profile = {
            "schema_version": 1,
            "target": {"hal": "tuner_hal2", "product": "default", "backend": "px4"},
            "vts": {"contract": "android14-aidl-v1", "source_ref": "aosp-commit", "variant": ""},
            "frontend": {"type": "ISDBT", "is_software_frontend": False, "frequency_hz": None},
            "region": {"query": "大阪府大阪市中央区大阪城1-1", "candidates": []},
            "flows": {"scan": True, "record": {"enabled": False}, "clear_live": {"enabled": False}},
            "queues": {},
        }
        resolve_region(profile)
        candidates = profile["region"]["candidates"]
        self.assertEqual(len(candidates), 40)
        self.assertEqual(candidates[0]["physical_channel"], 13)
        self.assertEqual(candidates[0]["frequency_hz"], 473_142_857)
        self.assertEqual(candidates[-1]["physical_channel"], 52)
        self.assertEqual(candidates[-1]["frequency_hz"], 707_142_857)

    def test_builtin_plan_accepts_japanese_postal_code(self) -> None:
        profile = {
            "schema_version": 1,
            "target": {"hal": "tuner_hal2", "product": "default", "backend": "px4"},
            "vts": {"contract": "android14-aidl-v1", "source_ref": "aosp-commit", "variant": ""},
            "frontend": {"type": "ISDBT", "is_software_frontend": False, "frequency_hz": None},
            "region": {"query": "540-0002", "candidates": []},
            "flows": {"scan": True, "record": {"enabled": False}, "clear_live": {"enabled": False}},
            "queues": {},
        }
        resolve_region(profile)
        self.assertEqual(len(profile["region"]["candidates"]), 40)

    def test_builtin_plan_rejects_non_japanese_region_input(self) -> None:
        profile = {
            "schema_version": 1,
            "target": {"hal": "tuner_hal2", "product": "default", "backend": "px4"},
            "vts": {"contract": "android14-aidl-v1", "source_ref": "aosp-commit", "variant": ""},
            "frontend": {"type": "ISDBT", "is_software_frontend": False, "frequency_hz": None},
            "region": {"query": "not-a-japanese-address", "candidates": []},
            "flows": {"scan": True, "record": {"enabled": False}, "clear_live": {"enabled": False}},
            "queues": {},
        }
        with self.assertRaises(ProfileError):
            resolve_region(profile)

    def test_record_buffers_default_to_aosp_vts_sample_values(self) -> None:
        args = SimpleNamespace(
            non_interactive=True,
            backend="px4",
            product="default",
            delivery_system="ISDBT",
            vts_source_ref="aosp-commit",
            region="大阪府大阪市中央区大阪城1-1",
            frequency_hz=None,
            service_id=None,
            record="yes",
            record_pid=None,
            scan="yes",
            record_filter_bytes=None,
            record_dvr_bytes=None,
            variant="",
        )
        profile = _new_profile(args)
        self.assertEqual(profile["queues"]["record_filter_bytes"], DEFAULT_RECORD_FILTER_BYTES)
        self.assertEqual(profile["queues"]["record_dvr_bytes"], DEFAULT_RECORD_DVR_BYTES)
        self.assertEqual(DEFAULT_RECORD_FILTER_BYTES, 16 * 1024 * 1024)
        self.assertEqual(DEFAULT_RECORD_DVR_BYTES, 4 * 1024 * 1024)


if __name__ == "__main__":
    unittest.main()

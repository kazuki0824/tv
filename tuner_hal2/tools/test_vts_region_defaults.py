from __future__ import annotations

import json
import unittest
from pathlib import Path
from types import SimpleNamespace

from vts_profile.cli import DEFAULT_RECORD_DVR_BYTES, DEFAULT_RECORD_FILTER_BYTES, _new_profile
from vts_profile.model import ProfileError
from vts_profile.region import DEFAULT_REGION_DATASET, resolve_region


def _profile(region: str) -> dict:
    return {
        "schema_version": 1,
        "target": {"hal": "tuner_hal2", "product": "default", "backend": "px4"},
        "vts": {"contract": "android14-aidl-v1", "source_ref": "aosp-commit", "variant": ""},
        "frontend": {"type": "ISDBT", "is_software_frontend": False, "frequency_hz": None},
        "region": {"query": region, "candidates": []},
        "flows": {"scan": True, "record": {"enabled": False}, "clear_live": {"enabled": False}},
        "queues": {},
    }


class VtsRegionDefaultsTest(unittest.TestCase):
    def test_bundled_dataset_has_all_prefectures_and_current_channels_only(self) -> None:
        dataset = json.loads(DEFAULT_REGION_DATASET.read_text(encoding="utf-8"))
        self.assertEqual(dataset["schema_version"], 2)
        self.assertEqual(len(dataset["prefectures"]), 47)
        for prefecture in dataset["prefectures"].values():
            channels = prefecture["prefecture_channels"]
            self.assertTrue(channels)
            self.assertTrue(all(13 <= channel <= 52 for channel in channels))

    def test_osaka_address_resolves_osaka_city_candidates_not_nationwide_raster(self) -> None:
        profile = _profile("大阪府大阪市中央区大阪城1-1")
        resolve_region(profile)
        channels = [item["physical_channel"] for item in profile["region"]["candidates"]]
        self.assertEqual(channels, [13, 14, 15, 16, 17, 18, 24])
        self.assertLess(len(channels), 40)
        self.assertEqual(profile["region"]["candidates"][0]["frequency_hz"], 473_142_857)

    def test_address_changes_candidate_set(self) -> None:
        osaka = _profile("大阪府大阪市中央区大阪城1-1")
        tokyo = _profile("東京都八王子市元本郷町3-24-1")
        resolve_region(osaka)
        resolve_region(tokyo)
        osaka_channels = {item["physical_channel"] for item in osaka["region"]["candidates"]}
        tokyo_channels = {item["physical_channel"] for item in tokyo["region"]["candidates"]}
        self.assertNotEqual(osaka_channels, tokyo_channels)
        self.assertEqual(
            tokyo_channels,
            {14, 20, 21, 22, 23, 24, 25, 26, 27, 29, 31, 35, 36, 37, 39, 40, 41, 42, 43, 44, 47},
        )

    def test_prefecture_only_address_falls_back_to_prefecture_union_not_nationwide(self) -> None:
        profile = _profile("大阪府")
        dataset = json.loads(DEFAULT_REGION_DATASET.read_text(encoding="utf-8"))
        expected = dataset["prefectures"]["大阪府"]["prefecture_channels"]
        resolve_region(profile)
        channels = [item["physical_channel"] for item in profile["region"]["candidates"]]
        self.assertEqual(channels, expected)
        self.assertLess(len(channels), 40)

    def test_longest_area_match_is_used(self) -> None:
        dataset = {
            "schema_version": 2,
            "dataset_version": "fixture",
            "source": {"index_url": "fixture", "source_notice": "fixture"},
            "prefectures": {
                "大阪府": {
                    "source_url": "fixture",
                    "default_channels": [13],
                    "prefecture_channels": [13, 14, 15],
                    "areas": {"大阪市": [13, 14], "大阪市中央区": [15]},
                }
            },
        }
        profile = _profile("大阪府大阪市中央区大阪城1-1")
        resolve_region(profile, dataset)
        self.assertEqual(
            [item["physical_channel"] for item in profile["region"]["candidates"]],
            [15],
        )

    def test_region_without_prefecture_is_fail_closed_for_bundled_dataset(self) -> None:
        with self.assertRaises(ProfileError):
            resolve_region(_profile("大阪市中央区大阪城1-1"))

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

    def test_record_buffer_defaults_can_be_overridden(self) -> None:
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
            record_filter_bytes=1_048_576,
            record_dvr_bytes=2_097_152,
            variant="",
        )
        profile = _new_profile(args)
        self.assertEqual(profile["queues"]["record_filter_bytes"], 1_048_576)
        self.assertEqual(profile["queues"]["record_dvr_bytes"], 2_097_152)


if __name__ == "__main__":
    unittest.main()

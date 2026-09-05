from __future__ import annotations

import json
import unittest
from types import SimpleNamespace
from unittest.mock import patch

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
        "flows": {
            "scan": True,
            "record": {"enabled": True, "pid": None},
            "clear_live": {
                "enabled": True,
                "audio_pid": None,
                "video_pid": None,
                "audio_stream_type": None,
                "video_stream_type": None,
                "pcr_pid": None,
                "section_pid": None,
            },
            "playback": {
                "enabled": True,
                "input_file_path": "/data/local/tmp/segment000000.ts",
            },
        },
        "queues": {
            "record_filter_bytes": 16 * 1024 * 1024,
            "record_dvr_bytes": 4 * 1024 * 1024,
            "audio_filter_bytes": 16 * 1024 * 1024,
            "video_filter_bytes": 16 * 1024 * 1024,
            "pcr_filter_bytes": 16 * 1024 * 1024,
            "section_filter_bytes": 16 * 1024 * 1024,
            "playback_dvr_bytes": 4 * 1024 * 1024,
        },
    }


def _channels(profile: dict) -> list[int]:
    return [item["physical_channel"] for item in profile["region"]["candidates"]]


class VtsRegionDefaultsTest(unittest.TestCase):
    def test_bundled_dataset_has_all_prefectures_and_current_channels_only(self) -> None:
        dataset = json.loads(DEFAULT_REGION_DATASET.read_text(encoding="utf-8"))
        self.assertEqual(dataset["schema_version"], 2)
        self.assertNotIn("dataset_version", dataset)
        self.assertEqual(len(dataset["prefectures"]), 47)
        for prefecture in dataset["prefectures"].values():
            channels = prefecture["prefecture_channels"]
            self.assertTrue(channels)
            self.assertTrue(all(13 <= channel <= 52 for channel in channels))

    def test_address_is_geocoded_to_coordinate_before_area_lookup(self) -> None:
        profile = _profile("大阪府大阪市中央区大阪城1-1")
        with (
            patch("vts_profile.region._geocode_address", return_value=(34.6873, 135.5262)) as geocode,
            patch("vts_profile.region._coordinate_area", return_value=("大阪府", "大阪市中央区")) as area,
        ):
            resolve_region(profile)
        geocode.assert_called_once_with("大阪府大阪市中央区大阪城1-1")
        area.assert_called_once_with((34.6873, 135.5262))
        self.assertEqual(_channels(profile), [13, 14, 15, 16, 17, 18, 24])

    def test_plain_coordinate_uses_same_area_lookup(self) -> None:
        profile = _profile("34.6873,135.5262")
        with patch("vts_profile.region._coordinate_area", return_value=("大阪府", "大阪市中央区")):
            resolve_region(profile)
        self.assertEqual(_channels(profile), [13, 14, 15, 16, 17, 18, 24])

    def test_prefixed_coordinate_is_supported(self) -> None:
        profile = _profile("latlon:34.6873,135.5262")
        with patch("vts_profile.region._coordinate_area", return_value=("大阪府", "大阪市中央区")):
            resolve_region(profile)
        self.assertEqual(_channels(profile), [13, 14, 15, 16, 17, 18, 24])

    def test_postal_code_is_geocoded_then_uses_same_area_lookup(self) -> None:
        profile = _profile("540-0002")
        with (
            patch("vts_profile.region._postal_addresses", return_value={"5400002": {"大阪府大阪市中央区大阪城"}}),
            patch("vts_profile.region._geocode_address", return_value=(34.6873, 135.5262)),
            patch("vts_profile.region._coordinate_area", return_value=("大阪府", "大阪市中央区")),
        ):
            resolve_region(profile)
        self.assertEqual(_channels(profile), [13, 14, 15, 16, 17, 18, 24])

    def test_prefixed_postal_code_is_supported(self) -> None:
        profile = _profile("postal:5400002")
        with (
            patch("vts_profile.region._postal_addresses", return_value={"5400002": {"大阪府大阪市中央区大阪城"}}),
            patch("vts_profile.region._geocode_address", return_value=(34.6873, 135.5262)),
            patch("vts_profile.region._coordinate_area", return_value=("大阪府", "大阪市中央区")),
        ):
            resolve_region(profile)
        self.assertEqual(_channels(profile), [13, 14, 15, 16, 17, 18, 24])

    def test_prefecture_only_input_uses_prefecture_union(self) -> None:
        profile = _profile("大阪府")
        dataset = json.loads(DEFAULT_REGION_DATASET.read_text(encoding="utf-8"))
        expected = dataset["prefectures"]["大阪府"]["prefecture_channels"]
        resolve_region(profile)
        self.assertEqual(_channels(profile), expected)

    def test_longest_coverage_area_match_is_used(self) -> None:
        dataset = {
            "schema_version": 2,
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
        profile = _profile("34.6873,135.5262")
        with patch("vts_profile.region._coordinate_area", return_value=("大阪府", "大阪市中央区")):
            resolve_region(profile, dataset)
        self.assertEqual(_channels(profile), [15])

    def test_invalid_postal_code_is_fail_closed(self) -> None:
        with self.assertRaises(ProfileError):
            resolve_region(_profile("postal:123"))

    def test_invalid_coordinates_are_fail_closed(self) -> None:
        with self.assertRaises(ProfileError):
            resolve_region(_profile("latlon:91,135"))

    def test_record_buffers_default_to_aosp_vts_sample_values(self) -> None:
        args = SimpleNamespace(
            non_interactive=True, backend="px4", product="default", delivery_system="ISDBT",
            vts_source_ref="aosp-commit", region="大阪府大阪市中央区大阪城1-1", frequency_hz=None,
            service_id=None, record="yes", record_pid=None, scan="yes", record_filter_bytes=None,
            record_dvr_bytes=None, variant="",
        )
        profile = _new_profile(args)
        self.assertEqual(profile["queues"]["record_filter_bytes"], DEFAULT_RECORD_FILTER_BYTES)
        self.assertEqual(profile["queues"]["record_dvr_bytes"], DEFAULT_RECORD_DVR_BYTES)
        self.assertEqual(DEFAULT_RECORD_FILTER_BYTES, 16 * 1024 * 1024)
        self.assertEqual(DEFAULT_RECORD_DVR_BYTES, 4 * 1024 * 1024)

    def test_record_buffer_defaults_can_be_overridden(self) -> None:
        args = SimpleNamespace(
            non_interactive=True, backend="px4", product="default", delivery_system="ISDBT",
            vts_source_ref="aosp-commit", region="大阪府大阪市中央区大阪城1-1", frequency_hz=None,
            service_id=None, record="yes", record_pid=None, scan="yes", record_filter_bytes=1_048_576,
            record_dvr_bytes=2_097_152, variant="",
        )
        profile = _new_profile(args)
        self.assertEqual(profile["queues"]["record_filter_bytes"], 1_048_576)
        self.assertEqual(profile["queues"]["record_dvr_bytes"], 2_097_152)


if __name__ == "__main__":
    unittest.main()

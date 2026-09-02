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
        "flows": {"scan": True, "record": {"enabled": False}, "clear_live": {"enabled": False}},
        "queues": {},
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

    def test_osaka_address_resolves_osaka_city_candidates_not_nationwide_raster(self) -> None:
        profile = _profile("大阪府大阪市中央区大阪城1-1")
        resolve_region(profile)
        self.assertEqual(_channels(profile), [13, 14, 15, 16, 17, 18, 24])
        self.assertLess(len(_channels(profile)), 40)
        self.assertEqual(profile["region"]["candidates"][0]["frequency_hz"], 473_142_857)

    def test_address_changes_candidate_set(self) -> None:
        osaka = _profile("大阪府大阪市中央区大阪城1-1")
        tokyo = _profile("東京都八王子市元本郷町3-24-1")
        resolve_region(osaka)
        resolve_region(tokyo)
        self.assertNotEqual(set(_channels(osaka)), set(_channels(tokyo)))
        self.assertEqual(
            set(_channels(tokyo)),
            {14, 20, 21, 22, 23, 24, 25, 26, 27, 29, 31, 35, 36, 37, 39, 40, 41, 42, 43, 44, 47},
        )

    def test_postal_code_resolves_through_japan_post_address(self) -> None:
        profile = _profile("540-0002")
        with patch(
            "vts_profile.region._japan_post_lookups",
            return_value=({"5400002": {"大阪府大阪市中央区"}}, {"27128": "大阪府大阪市中央区"}),
        ):
            resolve_region(profile)
        self.assertEqual(_channels(profile), [13, 14, 15, 16, 17, 18, 24])

    def test_prefixed_postal_code_is_supported(self) -> None:
        profile = _profile("postal:5400002")
        with patch(
            "vts_profile.region._japan_post_lookups",
            return_value=({"5400002": {"大阪府大阪市中央区"}}, {"27128": "大阪府大阪市中央区"}),
        ):
            resolve_region(profile)
        self.assertEqual(_channels(profile), [13, 14, 15, 16, 17, 18, 24])

    def test_coordinates_resolve_via_gsi_municipality_then_channel_plan(self) -> None:
        profile = _profile("latlon:34.6873,135.5262")
        with (
            patch(
                "vts_profile.region._fetch_json",
                return_value={"results": {"muniCd": "27128", "lv01Nm": "大阪城"}},
            ),
            patch(
                "vts_profile.region._japan_post_lookups",
                return_value=({}, {"27128": "大阪府大阪市中央区"}),
            ),
        ):
            resolve_region(profile)
        self.assertEqual(_channels(profile), [13, 14, 15, 16, 17, 18, 24])

    def test_plain_coordinates_are_supported(self) -> None:
        profile = _profile("34.6873,135.5262")
        with (
            patch(
                "vts_profile.region._fetch_json",
                return_value={"results": {"muniCd": "27128", "lv01Nm": "大阪城"}},
            ),
            patch(
                "vts_profile.region._japan_post_lookups",
                return_value=({}, {"27128": "大阪府大阪市中央区"}),
            ),
        ):
            resolve_region(profile)
        self.assertEqual(_channels(profile), [13, 14, 15, 16, 17, 18, 24])

    def test_prefecture_only_address_falls_back_to_prefecture_union_not_nationwide(self) -> None:
        profile = _profile("大阪府")
        dataset = json.loads(DEFAULT_REGION_DATASET.read_text(encoding="utf-8"))
        expected = dataset["prefectures"]["大阪府"]["prefecture_channels"]
        resolve_region(profile)
        self.assertEqual(_channels(profile), expected)
        self.assertLess(len(_channels(profile)), 40)

    def test_longest_area_match_is_used(self) -> None:
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
        profile = _profile("大阪府大阪市中央区大阪城1-1")
        resolve_region(profile, dataset)
        self.assertEqual(_channels(profile), [15])

    def test_address_without_prefecture_is_geocoded_before_channel_lookup(self) -> None:
        profile = _profile("大阪市中央区大阪城1-1")
        with patch("vts_profile.region._geocoded_address", return_value="大阪府大阪市中央区大阪城"):
            resolve_region(profile)
        self.assertEqual(_channels(profile), [13, 14, 15, 16, 17, 18, 24])

    def test_ambiguous_address_geocoding_is_fail_closed(self) -> None:
        with patch("vts_profile.region._fetch_json_value", return_value=[{}, {}]):
            with self.assertRaises(ProfileError):
                resolve_region(_profile("中央区一丁目"))

    def test_invalid_postal_code_is_fail_closed(self) -> None:
        with self.assertRaises(ProfileError):
            resolve_region(_profile("postal:123"))

    def test_invalid_coordinates_are_fail_closed(self) -> None:
        with self.assertRaises(ProfileError):
            resolve_region(_profile("latlon:91,135"))

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

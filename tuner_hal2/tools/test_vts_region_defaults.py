from __future__ import annotations

import json
import unittest
from urllib.parse import parse_qs, urlparse
from unittest.mock import patch

from vts_profile.model import ProfileError
from vts_profile.region import (
    DEFAULT_REGION_DATASET,
    _canonicalize_address,
    _distance_km,
    _geocode_address,
    resolve_region,
)


def _profile(region: str) -> dict:
    return {
        "schema_version": 1,
        "target": {"hal": "tuner_hal2", "product": "default", "backend": "px4"},
        "vts": {"contract": "android14-aidl-v1", "source_ref": "aosp-commit", "variant": ""},
        "frontend": {"type": "ISDBT", "is_software_frontend": False, "frequency_hz": None},
        "region": {"query": region, "transmitter_candidate_count": 2, "candidates": []},
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
            "playback": {"enabled": True, "input_file_path": "/data/local/tmp/segment000000.ts"},
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


def _service(name: str, remote: int, channel: int, output_w: float | None) -> dict:
    return {
        "name": name,
        "remote_control_key_id": remote,
        "physical_channel": channel,
        "polarization": "水平",
        "output_text": "" if output_w is None else f"{output_w}W",
        "output_w": output_w,
    }


def _transmitter(
    transmitter_id: str,
    name: str,
    latitude: float | None,
    longitude: float | None,
    channel: int,
    output_w: float | None,
    *,
    coverage_texts: list[str] | None = None,
    prefecture: str = "神奈川県",
) -> dict:
    return {
        "id": transmitter_id,
        "prefecture": prefecture,
        "name": name,
        "source_url": f"https://ina4n.example/{transmitter_id}",
        "location_text": name,
        "latitude": latitude,
        "longitude": longitude,
        "coordinate_source": "fixture" if latitude is not None else None,
        "coverage_texts": list(coverage_texts or []),
        "services": [_service("NHK総合", 1, channel, output_w)],
    }


def _dataset(*transmitters: dict) -> dict:
    return {
        "schema_version": 3,
        "mode": "snapshot",
        "source": {"index_url": "https://ina4n.example/index", "source_notice": "INA4N fixture"},
        "transmitters": list(transmitters),
    }


class VtsRegionDefaultsTest(unittest.TestCase):
    def test_bundled_dataset_is_live_ina4n_descriptor(self) -> None:
        dataset = json.loads(DEFAULT_REGION_DATASET.read_text(encoding="utf-8"))
        self.assertEqual(dataset["schema_version"], 3)
        self.assertEqual(dataset["mode"], "live-ina4n")
        self.assertEqual(dataset["coordinate_overrides"], {})
        self.assertNotIn("prefecture_channels", json.dumps(dataset, ensure_ascii=False))

    def test_live_descriptor_loads_nationwide_transmitter_set(self) -> None:
        profile = _profile("鹿児島県枕崎市")
        live = {
            "schema_version": 3,
            "mode": "live-ina4n",
            "source": {"index_url": "https://ina4n.example/index", "source_notice": "INA4N fixture"},
            "coordinate_overrides": {},
        }
        transmitters = [
            _transmitter("makurazaki", "枕崎局", 31.27, 130.30, 20, 10.0, prefecture="鹿児島県"),
            _transmitter("border", "県外局", 31.28, 130.30, 21, 1.0, prefecture="熊本県"),
        ]
        with (
            patch("vts_profile.region._geocode_address", return_value=(31.27, 130.30)),
            patch("vts_profile.ina4n_dataset.load_all", return_value=tuple(transmitters)) as loader,
        ):
            resolve_region(profile, live)
        loader.assert_called_once_with()
        labels = [candidate["label"] for candidate in profile["region"]["candidates"]]
        self.assertTrue(any("枕崎局" in label for label in labels))
        self.assertTrue(any("県外局" in label for label in labels))

    def test_prefecture_only_input_is_rejected_as_too_coarse(self) -> None:
        with self.assertRaisesRegex(ProfileError, "prefecture-only region input is too coarse"):
            resolve_region(_profile("神奈川県"), _dataset(_transmitter("tx", "局", 35.0, 139.0, 20, 1.0)))

    def test_municipality_prefix_adds_prefecture(self) -> None:
        municipalities = {
            "14113": ("神奈川県", "横浜市緑区"),
            "14216": ("神奈川県", "座間市"),
        }
        with patch("vts_profile.region._municipalities", return_value=municipalities):
            self.assertEqual(_canonicalize_address("横浜市緑区長津田"), "神奈川県横浜市緑区長津田")
            self.assertEqual(_canonicalize_address("座間市相模が丘"), "神奈川県座間市相模が丘")

    def test_ambiguous_municipality_prefix_is_fail_closed(self) -> None:
        municipalities = {
            "13206": ("東京都", "府中市"),
            "34208": ("広島県", "府中市"),
        }
        with patch("vts_profile.region._municipalities", return_value=municipalities):
            with self.assertRaisesRegex(ProfileError, "municipality prefix is ambiguous"):
                _canonicalize_address("府中市本町")

    def test_geocoder_receives_canonical_address(self) -> None:
        municipalities = {"14113": ("神奈川県", "横浜市緑区")}
        response = [{"geometry": {"coordinates": [139.53, 35.51]}}]
        with (
            patch("vts_profile.region._municipalities", return_value=municipalities),
            patch("vts_profile.region._fetch_json_value", return_value=response) as fetch,
        ):
            self.assertEqual(_geocode_address("横浜市緑区"), (35.51, 139.53))
        query = parse_qs(urlparse(fetch.call_args.args[0]).query)["q"][0]
        self.assertEqual(query, "神奈川県横浜市緑区")

    def test_coverage_text_does_not_override_inverse_square_ranking(self) -> None:
        profile = _profile("35.51,139.53")
        dataset = _dataset(
            _transmitter("coverage", "coverage記載局", 35.60, 139.60, 20, 0.1, coverage_texts=["横浜市緑区"]),
            _transmitter("power", "高スコア局", 35.511, 139.53, 21, 1000.0),
        )
        resolve_region(profile, dataset)
        self.assertIn("高スコア局", profile["region"]["candidates"][0]["label"])
        self.assertNotIn("coverage", profile["region"]["candidates"][0]["label"])

    def test_inverse_square_ranking_crosses_prefecture_border(self) -> None:
        coordinate = (31.27, 130.30)
        dataset = _dataset(
            _transmitter("near-low", "近距離小出力", 31.28, 130.30, 20, 1.0, prefecture="鹿児島県"),
            _transmitter("far-high", "県外大出力", 31.45, 130.30, 21, 1000.0, prefecture="熊本県"),
        )
        first_score = 1.0 / max(_distance_km(coordinate, (31.28, 130.30)), 0.1) ** 2
        second_score = 1000.0 / max(_distance_km(coordinate, (31.45, 130.30)), 0.1) ** 2
        expected_first = "県外大出力" if second_score > first_score else "近距離小出力"
        profile = _profile("31.27,130.30")
        resolve_region(profile, dataset)
        self.assertIn(expected_first, profile["region"]["candidates"][0]["label"])

    def test_unknown_output_is_ranked_after_known_scores_by_distance(self) -> None:
        profile = _profile("31.27,130.30")
        dataset = _dataset(
            _transmitter("known", "既知出力局", 31.30, 130.30, 20, 1.0),
            _transmitter("unknown", "出力不明局", 31.271, 130.30, 21, None),
        )
        resolve_region(profile, dataset)
        labels = [item["label"] for item in profile["region"]["candidates"]]
        self.assertIn("既知出力局", labels[0])
        self.assertTrue(any("出力不明局" in label and "distance-no-output" in label for label in labels))

    def test_each_transmitter_emits_only_one_representative_channel(self) -> None:
        profile = _profile("35.0,139.0")
        profile["region"]["transmitter_candidate_count"] = 1
        transmitter = _transmitter("multi", "多波局", 35.01, 139.0, 20, 10.0)
        transmitter["services"] = [
            _service("低出力", 1, 20, 1.0),
            _service("高出力", 4, 21, 10.0),
            _service("同出力次点", 5, 22, 10.0),
        ]
        resolve_region(profile, _dataset(transmitter))
        candidates = profile["region"]["candidates"]
        self.assertEqual(len(candidates), 1)
        self.assertEqual(candidates[0]["physical_channel"], 21)
        self.assertIn("高出力", candidates[0]["label"])
        self.assertNotIn("fallback", candidates[0]["label"])

    def test_default_top_k_is_two_transmitter_probes(self) -> None:
        profile = _profile("35.0,139.0")
        profile["region"].pop("transmitter_candidate_count")
        dataset = _dataset(
            _transmitter("a", "A局", 35.001, 139.0, 20, 10.0),
            _transmitter("b", "B局", 35.002, 139.0, 21, 10.0),
            _transmitter("c", "C局", 35.003, 139.0, 22, 10.0),
        )
        resolve_region(profile, dataset)
        self.assertEqual(len(profile["region"]["candidates"]), 2)
        self.assertIn("A局", profile["region"]["candidates"][0]["label"])
        self.assertIn("B局", profile["region"]["candidates"][1]["label"])

    def test_explicit_top_k_one_limits_probe_count(self) -> None:
        profile = _profile("35.0,139.0")
        profile["region"]["transmitter_candidate_count"] = 1
        dataset = _dataset(
            _transmitter("a", "A局", 35.001, 139.0, 20, 10.0),
            _transmitter("b", "B局", 35.002, 139.0, 21, 10.0),
        )
        resolve_region(profile, dataset)
        self.assertEqual(len(profile["region"]["candidates"]), 1)
        self.assertIn("A局", profile["region"]["candidates"][0]["label"])

    def test_duplicate_frequency_does_not_replace_a_top_k_transmitter(self) -> None:
        profile = _profile("35.0,139.0")
        dataset = _dataset(
            _transmitter("a", "A局", 35.001, 139.0, 20, 10.0),
            _transmitter("b", "B局", 35.002, 139.0, 20, 9.0),
            _transmitter("c", "C局", 35.003, 139.0, 21, 8.0),
        )
        resolve_region(profile, dataset)
        candidates = profile["region"]["candidates"]
        self.assertEqual(len(candidates), 1)
        self.assertIn("A局", candidates[0]["label"])
        self.assertNotIn("C局", candidates[0]["label"])

    def test_non_natural_candidate_counts_are_rejected(self) -> None:
        for invalid in (0, -1, 1.5, "2", True):
            with self.subTest(invalid=invalid):
                profile = _profile("35.0,139.0")
                profile["region"]["transmitter_candidate_count"] = invalid
                with self.assertRaisesRegex(ProfileError, "must be a natural number"):
                    resolve_region(profile, _dataset(_transmitter("tx", "局", 35.0, 139.0, 20, 1.0)))

    def test_invalid_dataset_schema_is_fail_closed(self) -> None:
        profile = _profile("35.0,139.0")
        with self.assertRaisesRegex(ProfileError, "schema_version must be 3"):
            resolve_region(profile, {"schema_version": 1, "source": {}, "transmitters": []})

    def test_invalid_postal_code_is_fail_closed(self) -> None:
        with self.assertRaises(ProfileError):
            resolve_region(_profile("postal:123"), _dataset(_transmitter("tx", "局", 35.0, 139.0, 20, 1.0)))

    def test_invalid_coordinates_are_fail_closed(self) -> None:
        with self.assertRaises(ProfileError):
            resolve_region(_profile("latlon:91,135"), _dataset(_transmitter("tx", "局", 35.0, 139.0, 20, 1.0)))


if __name__ == "__main__":
    unittest.main()

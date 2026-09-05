from __future__ import annotations

import json
import unittest
from unittest.mock import patch

from vts_profile.model import ProfileError
from vts_profile.region import DEFAULT_REGION_DATASET, _distance_km, resolve_region


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


def _service(
    name: str,
    remote: int,
    channel: int,
    output_w: float | None,
    *,
    polarization: str | None = "水平",
) -> dict:
    return {
        "name": name,
        "remote_control_key_id": remote,
        "physical_channel": channel,
        "polarization": polarization,
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
    coverage_areas: list[str] | None = None,
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
        "coverage_texts": list(coverage_areas or []),
        "coverage_areas": list(coverage_areas or []),
        "services": [_service("NHK総合", 1, channel, output_w)],
    }


def _dataset(*transmitters: dict) -> dict:
    return {
        "schema_version": 3,
        "mode": "snapshot",
        "source": {
            "index_url": "https://ina4n.example/index",
            "source_notice": "INA4N fixture",
        },
        "transmitters": list(transmitters),
    }


class VtsRegionDefaultsTest(unittest.TestCase):
    def test_bundled_dataset_is_live_ina4n_descriptor(self) -> None:
        dataset = json.loads(DEFAULT_REGION_DATASET.read_text(encoding="utf-8"))
        self.assertEqual(dataset["schema_version"], 3)
        self.assertEqual(dataset["mode"], "live-ina4n")
        self.assertIn("INA4N", dataset["source"]["source_notice"])
        self.assertEqual(dataset["coordinate_overrides"], {})
        self.assertNotIn("prefecture_channels", json.dumps(dataset, ensure_ascii=False))
        self.assertNotIn("transmitters", dataset)

    def test_live_descriptor_loads_only_resolved_prefecture(self) -> None:
        profile = _profile("鹿児島県枕崎市")
        live = {
            "schema_version": 3,
            "mode": "live-ina4n",
            "source": {"index_url": "https://ina4n.example/index", "source_notice": "INA4N fixture"},
            "coordinate_overrides": {},
        }
        transmitters = [_transmitter("makurazaki", "枕崎局", 31.27, 130.30, 20, 10.0, prefecture="鹿児島県")]
        with (
            patch("vts_profile.region._geocode_address", return_value=(31.27, 130.30)),
            patch("vts_profile.region._coordinate_area", return_value=("鹿児島県", "枕崎市")),
            patch(
                "vts_profile.ina4n_dataset.load_prefecture_with_overrides",
                return_value=transmitters,
            ) as loader,
        ):
            resolve_region(profile, live)
        loader.assert_called_once_with("鹿児島県", {})
        self.assertIn("枕崎局", profile["region"]["candidates"][0]["label"])

    def test_prefecture_only_input_is_rejected_as_too_coarse(self) -> None:
        with self.assertRaisesRegex(ProfileError, "prefecture-only region input is too coarse"):
            resolve_region(
                _profile("神奈川県"),
                _dataset(_transmitter("tx", "局", 35.0, 139.0, 20, 1.0)),
            )

    def test_address_is_geocoded_then_coverage_evidence_wins(self) -> None:
        profile = _profile("兵庫県神戸市北区鈴蘭台")
        dataset = _dataset(
            _transmitter(
                "coverage",
                "鈴蘭台近傍局",
                34.75,
                135.15,
                21,
                0.1,
                coverage_areas=["神戸市"],
                prefecture="兵庫県",
            ),
            _transmitter(
                "power",
                "大出力局",
                34.80,
                135.20,
                22,
                1000.0,
                prefecture="大阪府",
            ),
        )
        with (
            patch("vts_profile.region._geocode_address", return_value=(34.73, 135.14)) as geocode,
            patch("vts_profile.region._coordinate_area", return_value=("兵庫県", "神戸市北区")),
        ):
            resolve_region(profile, dataset)
        geocode.assert_called_once_with("兵庫県神戸市北区鈴蘭台")
        first = profile["region"]["candidates"][0]["label"]
        self.assertIn("鈴蘭台近傍局", first)
        self.assertIn("coverage+inverse-square", first)

    def test_no_coverage_match_uses_inverse_square_order(self) -> None:
        coordinate = (31.27, 130.30)
        dataset = _dataset(
            _transmitter("near-low", "近距離小出力", 31.28, 130.30, 20, 1.0, prefecture="鹿児島県"),
            _transmitter("far-high", "遠距離大出力", 31.45, 130.30, 21, 1000.0, prefecture="熊本県"),
        )
        first_score = 1.0 / max(_distance_km(coordinate, (31.28, 130.30)), 0.1) ** 2
        second_score = 1000.0 / max(_distance_km(coordinate, (31.45, 130.30)), 0.1) ** 2
        expected_first = "遠距離大出力" if second_score > first_score else "近距離小出力"
        ranked = _profile("31.27,130.30")
        with patch("vts_profile.region._coordinate_area", return_value=("鹿児島県", "枕崎市")):
            resolve_region(ranked, dataset)
        self.assertIn(expected_first, ranked["region"]["candidates"][0]["label"])
        self.assertIn("inverse-square", ranked["region"]["candidates"][0]["label"])

    def test_unknown_output_is_retained_with_lower_priority_distance_basis(self) -> None:
        profile = _profile("31.27,130.30")
        dataset = _dataset(
            _transmitter("known", "既知出力局", 31.30, 130.30, 20, 1.0, prefecture="鹿児島県"),
            _transmitter("unknown", "出力不明局", 31.271, 130.30, 21, None, prefecture="鹿児島県"),
        )
        with patch("vts_profile.region._coordinate_area", return_value=("鹿児島県", "枕崎市")):
            resolve_region(profile, dataset)
        labels = [item["label"] for item in profile["region"]["candidates"]]
        self.assertIn("既知出力局", labels[0])
        self.assertTrue(any("出力不明局" in label and "distance-no-output" in label for label in labels))

    def test_unknown_coordinate_is_retained_when_coverage_matches(self) -> None:
        profile = _profile("34.73,135.14")
        dataset = _dataset(
            _transmitter(
                "unknown-coordinate",
                "座標不明局",
                None,
                None,
                21,
                1.0,
                coverage_areas=["神戸市"],
                prefecture="兵庫県",
            )
        )
        with patch("vts_profile.region._coordinate_area", return_value=("兵庫県", "神戸市北区")):
            resolve_region(profile, dataset)
        self.assertIn("coverage-no-coordinate", profile["region"]["candidates"][0]["label"])

    def test_one_probe_frequency_is_emitted_per_ranked_transmitter(self) -> None:
        profile = _profile("35.0,139.0")
        transmitter = _transmitter("multi", "多波局", 35.01, 139.0, 20, 10.0)
        transmitter["services"] = [
            _service("低出力", 1, 20, 1.0),
            _service("高出力", 4, 21, 10.0),
            _service("同出力優先順位後", 5, 22, 10.0),
        ]
        with patch("vts_profile.region._coordinate_area", return_value=("東京都", "千代田区")):
            resolve_region(profile, _dataset(transmitter))
        candidates = profile["region"]["candidates"]
        self.assertEqual(len(candidates), 1)
        self.assertEqual(candidates[0]["physical_channel"], 21)
        self.assertIn("高出力", candidates[0]["label"])

    def test_duplicate_frequency_from_different_transmitters_is_probed_once(self) -> None:
        profile = _profile("35.0,139.0")
        dataset = _dataset(
            _transmitter("a", "A局", 35.01, 139.0, 20, 10.0),
            _transmitter("b", "B局", 35.02, 139.0, 20, 5.0),
        )
        with patch("vts_profile.region._coordinate_area", return_value=("東京都", "千代田区")):
            resolve_region(profile, dataset)
        self.assertEqual(len(profile["region"]["candidates"]), 1)
        self.assertIn("A局", profile["region"]["candidates"][0]["label"])

    def test_invalid_dataset_schema_is_fail_closed(self) -> None:
        profile = _profile("35.0,139.0")
        with patch("vts_profile.region._coordinate_area", return_value=("東京都", "千代田区")):
            with self.assertRaisesRegex(ProfileError, "schema_version must be 3"):
                resolve_region(profile, {"schema_version": 1, "source": {}, "transmitters": []})

    def test_invalid_postal_code_is_fail_closed(self) -> None:
        with self.assertRaises(ProfileError):
            resolve_region(
                _profile("postal:123"),
                _dataset(_transmitter("tx", "局", 35.0, 139.0, 20, 1.0)),
            )

    def test_invalid_coordinates_are_fail_closed(self) -> None:
        with self.assertRaises(ProfileError):
            resolve_region(
                _profile("latlon:91,135"),
                _dataset(_transmitter("tx", "局", 35.0, 139.0, 20, 1.0)),
            )


if __name__ == "__main__":
    unittest.main()

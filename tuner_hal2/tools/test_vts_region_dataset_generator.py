from __future__ import annotations

import unittest
from unittest.mock import patch

from vts_profile import ina4n_dataset as generator


class VtsRegionDatasetGeneratorTest(unittest.TestCase):
    def test_prefecture_page_preserves_transmitter_and_coverage_relation(self) -> None:
        page_url = "https://ina4n.com/chideji/47tv/kanagawa/kanagawa-index.html"
        html = """
        <div>主なカバーエリア</div><div>平塚市の一部</div>
        <a href="2005/hiratsuka-d-tv.html">平塚局</a>
        <a href="kanagawa-index.html">神奈川</a>
        """
        refs = generator._prefecture_transmitter_refs("神奈川県", page_url, html)
        self.assertEqual(len(refs), 1)
        prefecture, url, record = refs[0]
        self.assertEqual(prefecture, "神奈川県")
        self.assertEqual(url, "https://ina4n.com/chideji/47tv/kanagawa/2005/hiratsuka-d-tv.html")
        self.assertEqual(record["index_name"], "平塚局")
        self.assertEqual(record["coverage_texts"], ["平塚市の一部"])

    def test_detail_page_preserves_location_services_power_and_polarization(self) -> None:
        url = "https://ina4n.com/chideji/47tv/kanagawa/2005/hiratsuka-d-tv.html"
        html = """
        <h1>平塚テレビ中継局</h1>
        <table>
          <tr><th>中継局の場所</th><td>
            <a href="https://map.yahoo.co.jp/pl?lat=35.320709122176&lon=139.31059345819">
              神奈川県平塚市万田字泡垂山970-66（湘南平）
            </a>
          </td></tr>
          <tr><th>放送局</th><th>リモコン</th><th>物理ch</th><th>偏波</th><th>出力</th></tr>
          <tr><td>NHK総合</td><td>1</td><td>19</td><td>垂直</td><td>100W</td></tr>
          <tr><td>日本テレビ</td><td>4</td><td>25</td><td>垂直</td><td>100W</td></tr>
        </table>
        """
        record = generator._build_transmitter(
            "神奈川県",
            url,
            {"index_name": "平塚局", "coverage_texts": ["平塚市の一部"]},
            html,
        )
        assert record is not None
        self.assertEqual(record["id"], "kanagawa/2005/hiratsuka-d-tv.html")
        self.assertEqual(record["coverage_areas"], ["平塚市"])
        self.assertAlmostEqual(record["latitude"], 35.320709122176)
        self.assertAlmostEqual(record["longitude"], 139.31059345819)
        self.assertEqual(record["coordinate_source"], "INA4N-map")
        self.assertEqual(record["services"][0]["physical_channel"], 19)
        self.assertEqual(record["services"][0]["polarization"], "垂直")
        self.assertEqual(record["services"][0]["output_w"], 100.0)

    def test_power_parser_supports_watts_and_kilowatts(self) -> None:
        self.assertEqual(generator._parse_power_w("0.1W"), 0.1)
        self.assertEqual(generator._parse_power_w("1KW"), 1000.0)
        self.assertEqual(generator._parse_power_w("10kW"), 10000.0)

    def test_missing_output_and_polarization_are_preserved_as_unknown(self) -> None:
        services = generator._services([["北海道放送", "1", "22", "", ""]])
        self.assertEqual(len(services), 1)
        self.assertIsNone(services[0]["polarization"])
        self.assertIsNone(services[0]["output_w"])

    def test_apab_coordinate_override_is_used_only_when_ina4n_map_is_missing(self) -> None:
        url = "https://ina4n.com/chideji/47tv/hokkaido/2006/sapporo-d-tv.html"
        transmitter_id = "hokkaido/2006/sapporo-d-tv.html"
        html = """
        <h1>札幌送信所</h1>
        <table>
          <tr><th>中継局の場所</th><td>北海道札幌市</td></tr>
          <tr><td>NHK総合</td><td>3</td><td>15</td><td>水平</td><td>3kW</td></tr>
        </table>
        """
        record = generator._build_transmitter(
            "北海道",
            url,
            {"index_name": "札幌局", "coverage_texts": ["札幌市の一部"]},
            html,
            {transmitter_id: {"source": "A-PAB", "latitude": 43.0, "longitude": 141.0}},
        )
        assert record is not None
        self.assertEqual(record["coordinate_source"], "A-PAB")

    def test_gsi_geocode_is_fallback_after_no_ina4n_or_apab_coordinate(self) -> None:
        url = "https://ina4n.com/chideji/47tv/hokkaido/2006/sapporo-d-tv.html"
        html = """
        <h1>札幌送信所</h1>
        <table>
          <tr><th>中継局の場所</th><td>北海道札幌市</td></tr>
          <tr><td>NHK総合</td><td>3</td><td>15</td><td>水平</td><td>3kW</td></tr>
        </table>
        """
        with patch("vts_profile.ina4n_dataset._geocode_location", return_value=(43.1, 141.2)) as geocode:
            record = generator._build_transmitter(
                "北海道",
                url,
                {"index_name": "札幌局", "coverage_texts": []},
                html,
            )
        assert record is not None
        geocode.assert_called_once_with("北海道札幌市")
        self.assertEqual(record["coordinate_source"], "GSI-from-INA4N-location")

    def test_missing_coordinate_does_not_delete_valid_transmitter(self) -> None:
        url = "https://ina4n.com/chideji/47tv/hokkaido/2010/erimosawamachi-d-tv.html"
        html = """
        <h1>えりも沢町中継局</h1>
        <table><tr><td>NHK総合</td><td>3</td><td>22</td><td></td><td></td></tr></table>
        """
        with patch("vts_profile.ina4n_dataset._geocode_location", return_value=None):
            record = generator._build_transmitter(
                "北海道",
                url,
                {"index_name": "えりも沢町", "coverage_texts": ["えりも町の一部"]},
                html,
            )
        assert record is not None
        self.assertIsNone(record["latitude"])
        self.assertEqual(record["services"][0]["physical_channel"], 22)

    def test_page_without_current_isdbt_service_is_skipped_not_global_failure(self) -> None:
        html = """
        <h1>旧局</h1>
        <table><tr><td>放送大学</td><td>12</td><td>-</td><td>水平</td><td>1W</td></tr></table>
        """
        self.assertIsNone(
            generator._build_transmitter(
                "神奈川県",
                "https://ina4n.com/chideji/47tv/kanagawa/2005/example-d-tv.html",
                {"index_name": "旧局", "coverage_texts": []},
                html,
            )
        )

    def test_nationwide_loader_flattens_all_prefecture_pages(self) -> None:
        prefectures = ("東京都", "神奈川県")
        links = {
            "東京都": "https://ina4n.example/tokyo-index.html",
            "神奈川県": "https://ina4n.example/kanagawa-index.html",
        }
        refs = {
            "東京都": [("東京都", "https://ina4n.example/tokyo.html", {"index_name": "東京", "coverage_texts": []})],
            "神奈川県": [("神奈川県", "https://ina4n.example/kanagawa.html", {"index_name": "神奈川", "coverage_texts": []})],
        }

        def prefecture_refs(prefecture: str, _url: str, _html: str):
            return refs[prefecture]

        def build(prefecture: str, detail_url: str, _record: dict, _html: str, _overrides=None):
            return {
                "id": detail_url,
                "prefecture": prefecture,
                "name": prefecture,
                "source_url": detail_url,
                "location_text": prefecture,
                "latitude": 35.0,
                "longitude": 139.0,
                "coordinate_source": "fixture",
                "coverage_texts": [],
                "coverage_areas": [],
                "services": [{
                    "name": "fixture",
                    "remote_control_key_id": 1,
                    "physical_channel": 20 if prefecture == "東京都" else 21,
                    "polarization": "水平",
                    "output_text": "1W",
                    "output_w": 1.0,
                }],
            }

        with (
            patch.object(generator, "PREFECTURE_NAMES", prefectures),
            patch("vts_profile.ina4n_dataset.frequency_links", return_value=links),
            patch("vts_profile.ina4n_dataset._fetch_text", return_value="fixture"),
            patch("vts_profile.ina4n_dataset._prefecture_transmitter_refs", side_effect=prefecture_refs),
            patch("vts_profile.ina4n_dataset._build_transmitter", side_effect=build),
        ):
            transmitters = generator.load_all_with_overrides(None)
        self.assertEqual({item["prefecture"] for item in transmitters}, {"東京都", "神奈川県"})
        self.assertEqual(len(transmitters), 2)

    def test_live_descriptor_contains_no_lossy_prefecture_channel_union(self) -> None:
        descriptor = generator.live_descriptor()
        self.assertEqual(descriptor["mode"], "live-ina4n")
        self.assertEqual(descriptor["coordinate_overrides"], {})
        self.assertNotIn("prefecture_channels", str(descriptor))
        self.assertNotIn("transmitters", descriptor)


if __name__ == "__main__":
    unittest.main()

from __future__ import annotations

import csv
import io
import json
import re
import urllib.parse
import urllib.request
import zipfile
from functools import lru_cache
from pathlib import Path
from typing import Any

from .model import FRONTEND_ID, ProfileError, load_json, positive_int, reject_unknown, require_dict, validate_profile

DEFAULT_REGION_DATASET = Path(__file__).resolve().parents[2] / "config/vts_channel_plan.japan.json"
ISDBT_FIRST_CHANNEL = 13
ISDBT_LAST_CHANNEL = 52
ISDBT_CHANNEL_13_HZ = 473_142_857
ISDBT_CHANNEL_STEP_HZ = 6_000_000
JAPAN_POST_KEN_ALL_URL = "https://www.post.japanpost.jp/zipcode/dl/kogaki/zip/ken_all.zip"
GSI_REVERSE_GEOCODER_URL = "https://mreversegeocoder.gsi.go.jp/reverse-geocoder/LonLatToAddress"
GSI_ADDRESS_SEARCH_URL = "https://msearch.gsi.go.jp/address-search/AddressSearch"
GSI_MUNICIPALITY_URL = "https://maps.gsi.go.jp/js/muni.js"
HTTP_USER_AGENT = "maleicacid-tuner-hal2-vts-region-resolver/1"
JAPAN_PREFECTURES = (
    "北海道", "青森県", "岩手県", "宮城県", "秋田県", "山形県", "福島県",
    "茨城県", "栃木県", "群馬県", "埼玉県", "千葉県", "東京都", "神奈川県",
    "新潟県", "富山県", "石川県", "福井県", "山梨県", "長野県", "岐阜県",
    "静岡県", "愛知県", "三重県", "滋賀県", "京都府", "大阪府", "兵庫県",
    "奈良県", "和歌山県", "鳥取県", "島根県", "岡山県", "広島県", "山口県",
    "徳島県", "香川県", "愛媛県", "高知県", "福岡県", "佐賀県", "長崎県",
    "熊本県", "大分県", "宮崎県", "鹿児島県", "沖縄県",
)


def _frequency_for_channel(channel: int) -> int:
    if channel < ISDBT_FIRST_CHANNEL or channel > ISDBT_LAST_CHANNEL:
        raise ProfileError(f"unsupported current ISDBT physical channel: {channel}")
    return ISDBT_CHANNEL_13_HZ + (channel - ISDBT_FIRST_CHANNEL) * ISDBT_CHANNEL_STEP_HZ


def _current_channel(value: Any, name: str) -> int:
    channel = positive_int(value, name)
    if not ISDBT_FIRST_CHANNEL <= channel <= ISDBT_LAST_CHANNEL:
        raise ProfileError(f"{name} must be in current ISDBT range 13..52")
    return channel


def _fetch_bytes(url: str) -> bytes:
    request = urllib.request.Request(url, headers={"User-Agent": HTTP_USER_AGENT})
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            return response.read()
    except OSError as exc:
        raise ProfileError(f"failed to fetch regional lookup data from {url}: {exc}") from exc


def _fetch_json_value(url: str) -> Any:
    raw = _fetch_bytes(url)
    try:
        return json.loads(raw.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise ProfileError(f"invalid JSON from regional lookup service {url}") from exc


def _fetch_json(url: str) -> dict[str, Any]:
    return require_dict(_fetch_json_value(url), "regional lookup response")


def _parse_latlon(query: str) -> tuple[float, float]:
    value = query.strip()
    if value.lower().startswith("latlon:"):
        value = value.split(":", 1)[1]
    parts = [part.strip() for part in value.split(",")]
    if len(parts) != 2:
        raise ProfileError("lat/lon region input must be 'latitude,longitude'")
    try:
        latitude = float(parts[0])
        longitude = float(parts[1])
    except ValueError as exc:
        raise ProfileError("lat/lon region input must contain numeric coordinates") from exc
    if not -90.0 <= latitude <= 90.0 or not -180.0 <= longitude <= 180.0:
        raise ProfileError("latitude/longitude is outside the valid coordinate range")
    return latitude, longitude


def _geocode_address(address: str) -> tuple[float, float]:
    url = GSI_ADDRESS_SEARCH_URL + "?" + urllib.parse.urlencode({"q": address})
    value = _fetch_json_value(url)
    if not isinstance(value, list) or len(value) != 1:
        raise ProfileError("GSI address search must resolve the address to exactly one location")
    feature = require_dict(value[0], "GSI address-search feature")
    geometry = require_dict(feature.get("geometry"), "GSI address-search geometry")
    coordinates = geometry.get("coordinates")
    if not isinstance(coordinates, list) or len(coordinates) < 2:
        raise ProfileError("GSI address search result has no coordinates")
    try:
        longitude = float(coordinates[0])
        latitude = float(coordinates[1])
    except (TypeError, ValueError) as exc:
        raise ProfileError("GSI address search returned invalid coordinates") from exc
    return latitude, longitude


@lru_cache(maxsize=1)
def _postal_addresses() -> dict[str, set[str]]:
    archive = _fetch_bytes(JAPAN_POST_KEN_ALL_URL)
    try:
        with zipfile.ZipFile(io.BytesIO(archive)) as zf:
            csv_names = [name for name in zf.namelist() if name.upper().endswith(".CSV")]
            if len(csv_names) != 1:
                raise ProfileError("Japan Post KEN_ALL archive must contain exactly one CSV")
            csv_text = zf.read(csv_names[0]).decode("cp932")
    except (zipfile.BadZipFile, KeyError, UnicodeDecodeError) as exc:
        raise ProfileError("failed to parse Japan Post KEN_ALL postal dataset") from exc

    result: dict[str, set[str]] = {}
    for row in csv.reader(io.StringIO(csv_text)):
        if len(row) < 9:
            continue
        postal_code = row[2].strip()
        prefecture = row[6].strip()
        municipality = row[7].strip()
        town = row[8].strip()
        if postal_code and prefecture and municipality:
            result.setdefault(postal_code, set()).add(prefecture + municipality + town)
    return result


def _postal_coordinates(query: str) -> list[tuple[float, float]]:
    postal_code = re.sub(r"[^0-9]", "", query)
    if not re.fullmatch(r"\d{7}", postal_code):
        raise ProfileError("postal region input must contain exactly seven digits")
    addresses = sorted(_postal_addresses().get(postal_code, ()))
    if not addresses:
        raise ProfileError(f"Japan Post dataset has no address for postal code {postal_code}")
    return [_geocode_address(address) for address in addresses]


@lru_cache(maxsize=1)
def _municipalities() -> dict[str, tuple[str, str]]:
    try:
        text = _fetch_bytes(GSI_MUNICIPALITY_URL).decode("utf-8")
    except UnicodeDecodeError as exc:
        raise ProfileError("invalid GSI municipality table encoding") from exc
    result: dict[str, tuple[str, str]] = {}
    pattern = re.compile(r"GSI\.MUNI_ARRAY\[\"(\d+)\"\]\s*=\s*'([^']+)';")
    for code, value in pattern.findall(text):
        parts = value.split(",", 3)
        if len(parts) != 4:
            continue
        prefecture = parts[1].strip()
        municipality = re.sub(r"[\s　]+", "", parts[3])
        result[code.zfill(5)] = (prefecture, municipality)
    if not result:
        raise ProfileError("GSI municipality table is empty")
    return result


def _coordinate_area(coordinate: tuple[float, float]) -> tuple[str, str]:
    latitude, longitude = coordinate
    url = GSI_REVERSE_GEOCODER_URL + "?" + urllib.parse.urlencode(
        {"lat": f"{latitude:.8f}", "lon": f"{longitude:.8f}"}
    )
    response = _fetch_json(url)
    results = require_dict(response.get("results"), "GSI reverse-geocoder results")
    raw_code = str(results.get("muniCd", "")).strip()
    if not re.fullmatch(r"\d{4,5}", raw_code):
        raise ProfileError("GSI reverse geocoder did not return a Japanese municipality code")
    code = raw_code.zfill(5)
    area = _municipalities().get(code)
    if area is None:
        raise ProfileError(f"GSI municipality table has no entry for code {code}")
    return area


def _region_coordinates(query: str) -> list[tuple[float, float]]:
    value = query.strip()
    if value.lower().startswith("postal:"):
        return _postal_coordinates(value.split(":", 1)[1])
    if re.fullmatch(r"\d{3}-?\d{4}", value):
        return _postal_coordinates(value)
    if value.lower().startswith("latlon:"):
        return [_parse_latlon(value)]
    if re.fullmatch(r"[+-]?\d+(?:\.\d+)?\s*,\s*[+-]?\d+(?:\.\d+)?", value):
        return [_parse_latlon(value)]
    return [_geocode_address(value)]


def _channels_for_area(prefecture: str, municipality: str, prefectures: dict[str, Any]) -> tuple[set[int], str]:
    prefecture_data = require_dict(prefectures.get(prefecture), f"dataset.prefectures[{prefecture!r}]")
    reject_unknown(
        prefecture_data,
        {"source_url", "default_channels", "prefecture_channels", "areas"},
        f"dataset.prefectures[{prefecture!r}]",
    )
    areas = require_dict(prefecture_data.get("areas"), f"dataset.prefectures[{prefecture!r}].areas")
    matching_keys = sorted(
        (key for key in areas if isinstance(key, str) and key and key in municipality),
        key=lambda key: (-len(key), key),
    )
    if matching_keys:
        max_length = len(matching_keys[0])
        selected_keys = [key for key in matching_keys if len(key) == max_length]
        channels = {
            _current_channel(channel, f"dataset area {key} channel")
            for key in selected_keys
            for channel in areas[key]
        }
        return channels, "/".join(selected_keys)
    channels = {
        _current_channel(channel, f"dataset prefecture {prefecture} channel")
        for channel in prefecture_data.get("prefecture_channels", [])
    }
    return channels, prefecture


def _snapshot_candidates(profile: dict[str, Any], dataset: dict[str, Any]) -> list[dict[str, Any]]:
    reject_unknown(dataset, {"schema_version", "source", "prefectures"}, "dataset")
    if dataset.get("schema_version") != 2:
        raise ProfileError("region dataset schema_version must be 2")
    source = require_dict(dataset.get("source"), "dataset.source")
    reject_unknown(source, {"index_url", "source_notice"}, "dataset.source")
    prefectures = require_dict(dataset.get("prefectures"), "dataset.prefectures")

    region = require_dict(profile.get("region"), "region")
    query = region.get("query")
    if not isinstance(query, str) or not query.strip():
        raise ProfileError("region.query is required for resolve-region")
    if profile["frontend"]["type"] != "ISDBT":
        raise ProfileError("regional channel dataset is only valid for ISDBT")

    if query.strip() in JAPAN_PREFECTURES:
        prefecture = query.strip()
        prefecture_data = require_dict(prefectures.get(prefecture), f"dataset.prefectures[{prefecture!r}]")
        channel_labels = {
            _current_channel(channel, f"dataset prefecture {prefecture} channel"): {prefecture}
            for channel in prefecture_data.get("prefecture_channels", [])
        }
    else:
        channel_labels: dict[int, set[str]] = {}
        for coordinate in _region_coordinates(query):
            prefecture, municipality = _coordinate_area(coordinate)
            channels, label = _channels_for_area(prefecture, municipality, prefectures)
            for channel in channels:
                channel_labels.setdefault(channel, set()).add(label)

    if not channel_labels:
        raise ProfileError(f"region dataset has no ISDBT channels for {query!r}")
    return [
        {
            "delivery_system": "ISDBT",
            "physical_channel": channel,
            "frequency_hz": _frequency_for_channel(channel),
            "label": f"{'/'.join(sorted(channel_labels[channel]))} ch{channel}",
        }
        for channel in sorted(channel_labels)
    ]


def resolve_region(
    profile: dict[str, Any],
    dataset: dict[str, Any] | None = None,
    select_index: int | None = None,
) -> None:
    region = require_dict(profile.get("region"), "region")
    query = region.get("query")
    if not isinstance(query, str) or not query.strip():
        raise ProfileError("region.query is required for resolve-region")

    if dataset is None:
        if not DEFAULT_REGION_DATASET.is_file():
            raise ProfileError(f"built-in region dataset is missing: {DEFAULT_REGION_DATASET}")
        dataset = load_json(DEFAULT_REGION_DATASET)

    if dataset.get("schema_version") != 2:
        raise ProfileError("dataset.schema_version must be 2")
    matches = _snapshot_candidates(profile, dataset)
    if not matches:
        raise ProfileError(f"no {profile['frontend']['type']} candidates found for region {query!r}")
    matches.sort(key=lambda item: (item["frequency_hz"], item.get("physical_channel") or 0, item["label"]))
    region["candidates"] = matches
    if select_index is not None:
        select_candidate(profile, select_index)
    elif len(matches) == 1:
        select_candidate(profile, 0)
    validate_profile(profile)


def select_candidate(profile: dict[str, Any], index: int) -> None:
    region = require_dict(profile.get("region"), "region")
    candidates = region.get("candidates")
    if not isinstance(candidates, list) or not candidates:
        raise ProfileError("region.candidates is empty; run resolve-region first")
    if index < 0 or index >= len(candidates):
        raise ProfileError("candidate index is outside region.candidates")
    selected = candidates[index]
    if selected.get("delivery_system") not in FRONTEND_ID:
        raise ProfileError("selected candidate has unsupported delivery system")
    profile["frontend"]["frequency_hz"] = int(selected["frequency_hz"])
    if selected.get("physical_channel") is not None:
        profile["frontend"]["physical_channel"] = int(selected["physical_channel"])
    validate_profile(profile)

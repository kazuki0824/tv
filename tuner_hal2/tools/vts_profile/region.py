from __future__ import annotations

import csv
import io
import json
import re
import urllib.parse
import urllib.request
import zipfile
from pathlib import Path
from typing import Any

from japanese_address_parser_py import Parser

from .model import FRONTEND_ID, ProfileError, load_json, positive_int, reject_unknown, require_dict, validate_profile

DEFAULT_REGION_DATASET = Path(__file__).resolve().parents[2] / "config/vts_channel_plan.japan.json"
ISDBT_FIRST_CHANNEL = 13
ISDBT_LAST_CHANNEL = 52
ISDBT_CHANNEL_13_HZ = 473_142_857
ISDBT_CHANNEL_STEP_HZ = 6_000_000
JAPAN_POST_KEN_ALL_URL = "https://www.post.japanpost.jp/zipcode/dl/kogaki/zip/ken_all.zip"
GSI_REVERSE_GEOCODER_URL = "https://mreversegeocoder.gsi.go.jp/reverse-geocoder/LonLatToAddress"
GSI_ADDRESS_SEARCH_URL = "https://msearch.gsi.go.jp/address-search/AddressSearch"
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


_ADDRESS_PARSER = Parser()

def _normalize_address(query: str) -> str:
    result = _ADDRESS_PARSER.parse(query)
    if result.error:
        raise ProfileError(f"failed to normalize Japanese address: {result.error}")
    address = result.address
    if not isinstance(address, dict):
        raise ProfileError("Japanese address parser returned no structured address")
    prefecture = address.get("prefecture")
    city = address.get("city")
    if not isinstance(prefecture, str) or not prefecture or not isinstance(city, str) or not city:
        raise ProfileError("Japanese address must resolve to a prefecture and municipality")
    town = address.get("town")
    return prefecture + city + (town if isinstance(town, str) else "")

def _frequency_for_channel(channel: int) -> int:
    if channel < ISDBT_FIRST_CHANNEL or channel > ISDBT_LAST_CHANNEL:
        raise ProfileError(f"unsupported current ISDBT physical channel: {channel}")
    return ISDBT_CHANNEL_13_HZ + (channel - ISDBT_FIRST_CHANNEL) * ISDBT_CHANNEL_STEP_HZ


def _prefecture_from_address(query: str) -> str:
    matches = [prefecture for prefecture in JAPAN_PREFECTURES if prefecture in query]
    if len(matches) != 1:
        raise ProfileError("resolved Japanese address must contain exactly one prefecture name")
    return matches[0]


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


def _japan_post_lookups() -> tuple[dict[str, set[str]], dict[str, str]]:
    archive = _fetch_bytes(JAPAN_POST_KEN_ALL_URL)
    try:
        with zipfile.ZipFile(io.BytesIO(archive)) as zf:
            csv_names = [name for name in zf.namelist() if name.upper().endswith(".CSV")]
            if len(csv_names) != 1:
                raise ProfileError("Japan Post KEN_ALL archive must contain exactly one CSV")
            csv_text = zf.read(csv_names[0]).decode("cp932")
    except (zipfile.BadZipFile, KeyError, UnicodeDecodeError) as exc:
        raise ProfileError("failed to parse Japan Post KEN_ALL postal dataset") from exc

    postal_to_addresses: dict[str, set[str]] = {}
    municipality_to_address: dict[str, str] = {}
    for row in csv.reader(io.StringIO(csv_text)):
        if len(row) < 9:
            continue
        municipality_code = row[0].strip()
        postal_code = row[2].strip()
        prefecture = row[6].strip()
        municipality = row[7].strip()
        if not municipality_code or not postal_code or not prefecture or not municipality:
            continue
        address = prefecture + municipality
        postal_to_addresses.setdefault(postal_code, set()).add(address)
        existing = municipality_to_address.get(municipality_code)
        if existing is None:
            municipality_to_address[municipality_code] = address
        elif existing != address:
            raise ProfileError(f"Japan Post municipality code {municipality_code} maps to multiple regions")
    return postal_to_addresses, municipality_to_address


def _postal_addresses(query: str) -> list[str]:
    postal_code = re.sub(r"[^0-9]", "", query)
    if not re.fullmatch(r"\d{7}", postal_code):
        raise ProfileError("postal region input must contain exactly seven digits")
    postal_to_addresses, _ = _japan_post_lookups()
    matches = sorted(postal_to_addresses.get(postal_code, ()))
    if not matches:
        raise ProfileError(f"Japan Post dataset has no address for postal code {postal_code}")
    return matches


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


def _coordinate_address(query: str) -> str:
    latitude, longitude = _parse_latlon(query)
    url = GSI_REVERSE_GEOCODER_URL + "?" + urllib.parse.urlencode(
        {"lat": f"{latitude:.8f}", "lon": f"{longitude:.8f}"}
    )
    response = _fetch_json(url)
    results = require_dict(response.get("results"), "GSI reverse-geocoder results")
    municipality_code = str(results.get("muniCd", "")).strip()
    if not re.fullmatch(r"\d{5}", municipality_code):
        raise ProfileError("GSI reverse geocoder did not return a Japanese municipality code")
    _, municipality_to_address = _japan_post_lookups()
    address = municipality_to_address.get(municipality_code)
    if address is None:
        raise ProfileError(f"Japan Post dataset has no municipality for GSI code {municipality_code}")
    town = results.get("lv01Nm")
    if isinstance(town, str) and town.strip():
        address += town.strip()
    return address


def _geocoded_address(query: str) -> str:
    url = GSI_ADDRESS_SEARCH_URL + "?" + urllib.parse.urlencode({"q": query})
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
    return _coordinate_address(f"{latitude},{longitude}")


def _resolved_region_addresses(query: str) -> list[str]:
    value = query.strip()
    if value.lower().startswith("postal:"):
        return _postal_addresses(value.split(":", 1)[1])
    if re.fullmatch(r"\d{3}-?\d{4}", value):
        return _postal_addresses(value)
    if value.lower().startswith("latlon:"):
        return [_coordinate_address(value)]
    if re.fullmatch(r"[+-]?\d+(?:\.\d+)?\s*,\s*[+-]?\d+(?:\.\d+)?", value):
        return [_coordinate_address(value)]
    if value in JAPAN_PREFECTURES:
        return [value]
    if any(prefecture in value for prefecture in JAPAN_PREFECTURES):
        return [_normalize_address(value)]
    return [_geocoded_address(value)]


def _channels_for_address(address: str, prefectures: dict[str, Any]) -> tuple[set[int], str]:
    prefecture = _prefecture_from_address(address)
    prefecture_data = require_dict(prefectures.get(prefecture), f"dataset.prefectures[{prefecture!r}]")
    reject_unknown(
        prefecture_data,
        {"source_url", "default_channels", "prefecture_channels", "areas"},
        f"dataset.prefectures[{prefecture!r}]",
    )
    areas = require_dict(prefecture_data.get("areas"), f"dataset.prefectures[{prefecture!r}].areas")
    matching_keys = sorted(
        (key for key in areas if isinstance(key, str) and key and key in address),
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
        raise ProfileError("built-in region dataset schema_version must be 2")
    source = require_dict(dataset.get("source"), "dataset.source")
    reject_unknown(source, {"index_url", "source_notice"}, "dataset.source")
    prefectures = require_dict(dataset.get("prefectures"), "dataset.prefectures")

    region = require_dict(profile.get("region"), "region")
    query = region.get("query")
    if not isinstance(query, str) or not query.strip():
        raise ProfileError("region.query is required for resolve-region")
    if profile["frontend"]["type"] != "ISDBT":
        raise ProfileError("built-in regional channel dataset is only valid for ISDBT")

    channel_labels: dict[int, set[str]] = {}
    for address in _resolved_region_addresses(query):
        channels, label = _channels_for_address(address, prefectures)
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


def _legacy_dataset_candidates(
    profile: dict[str, Any], dataset: dict[str, Any]
) -> list[dict[str, Any]]:
    reject_unknown(dataset, {"schema_version", "entries"}, "dataset")
    if dataset.get("schema_version") != 1:
        raise ProfileError("dataset.schema_version must be 1 or 2")
    entries = dataset.get("entries")
    if not isinstance(entries, list):
        raise ProfileError("dataset.entries must be an array")

    region = require_dict(profile.get("region"), "region")
    query = region.get("query")
    if not isinstance(query, str) or not query.strip():
        raise ProfileError("region.query is required for resolve-region")
    fe_type = profile["frontend"]["type"]

    matches: list[dict[str, Any]] = []
    for index, raw in enumerate(entries):
        entry = require_dict(raw, f"dataset.entries[{index}]")
        reject_unknown(
            entry,
            {"region", "delivery_system", "physical_channel", "frequency_hz", "label"},
            f"dataset.entries[{index}]",
        )
        if entry.get("region") != query or entry.get("delivery_system") != fe_type:
            continue
        matches.append(
            {
                "delivery_system": fe_type,
                "physical_channel": entry.get("physical_channel"),
                "frequency_hz": positive_int(
                    entry.get("frequency_hz"), f"dataset.entries[{index}].frequency_hz"
                ),
                "label": entry.get("label") or "",
            }
        )
    return matches


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

    schema_version = dataset.get("schema_version")
    if schema_version == 2:
        matches = _snapshot_candidates(profile, dataset)
    elif schema_version == 1:
        matches = _legacy_dataset_candidates(profile, dataset)
    else:
        raise ProfileError("dataset.schema_version must be 1 or 2")

    if not matches:
        raise ProfileError(f"no {profile['frontend']['type']} candidates found for region {query!r}")
    matches.sort(
        key=lambda item: (
            item["frequency_hz"],
            item.get("physical_channel") or 0,
            item["label"],
        )
    )
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

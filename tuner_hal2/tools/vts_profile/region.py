from __future__ import annotations

from pathlib import Path
from typing import Any

from .model import FRONTEND_ID, ProfileError, load_json, positive_int, reject_unknown, require_dict, validate_profile

DEFAULT_REGION_DATASET = Path(__file__).resolve().parents[2] / "config/vts_channel_plan.japan.json"
ISDBT_FIRST_CHANNEL = 13
ISDBT_LAST_CHANNEL = 52
ISDBT_CHANNEL_13_HZ = 473_142_857
ISDBT_CHANNEL_STEP_HZ = 6_000_000
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


def _prefecture_from_address(query: str) -> str:
    matches = [prefecture for prefecture in JAPAN_PREFECTURES if prefecture in query]
    if len(matches) != 1:
        raise ProfileError(
            "built-in ISDBT region dataset requires an address containing exactly one Japanese prefecture name"
        )
    return matches[0]


def _current_channel(value: Any, name: str) -> int:
    channel = positive_int(value, name)
    if not ISDBT_FIRST_CHANNEL <= channel <= ISDBT_LAST_CHANNEL:
        raise ProfileError(f"{name} must be in current ISDBT range 13..52")
    return channel


def _snapshot_candidates(profile: dict[str, Any], dataset: dict[str, Any]) -> list[dict[str, Any]]:
    reject_unknown(dataset, {"schema_version", "dataset_version", "source", "prefectures"}, "dataset")
    if dataset.get("schema_version") != 2:
        raise ProfileError("built-in region dataset schema_version must be 2")
    if not isinstance(dataset.get("dataset_version"), str) or not dataset["dataset_version"]:
        raise ProfileError("dataset.dataset_version is required")
    source = require_dict(dataset.get("source"), "dataset.source")
    reject_unknown(source, {"index_url", "source_notice"}, "dataset.source")
    prefectures = require_dict(dataset.get("prefectures"), "dataset.prefectures")

    region = require_dict(profile.get("region"), "region")
    query = region.get("query")
    if not isinstance(query, str) or not query.strip():
        raise ProfileError("region.query is required for resolve-region")
    if profile["frontend"]["type"] != "ISDBT":
        raise ProfileError("built-in regional channel dataset is only valid for ISDBT")

    prefecture = _prefecture_from_address(query)
    prefecture_data = require_dict(prefectures.get(prefecture), f"dataset.prefectures[{prefecture!r}]")
    reject_unknown(
        prefecture_data,
        {"source_url", "default_channels", "prefecture_channels", "areas"},
        f"dataset.prefectures[{prefecture!r}]",
    )
    areas = require_dict(prefecture_data.get("areas"), f"dataset.prefectures[{prefecture!r}].areas")

    matching_keys = sorted(
        (key for key in areas if isinstance(key, str) and key and key in query),
        key=lambda key: (-len(key), key),
    )
    if matching_keys:
        max_length = len(matching_keys[0])
        selected_keys = [key for key in matching_keys if len(key) == max_length]
        channels = sorted(
            {
                _current_channel(channel, f"dataset area {key} channel")
                for key in selected_keys
                for channel in areas[key]
            }
        )
        label_prefix = "/".join(selected_keys)
    else:
        channels = sorted(
            _current_channel(channel, f"dataset prefecture {prefecture} channel")
            for channel in prefecture_data.get("prefecture_channels", [])
        )
        label_prefix = prefecture

    if not channels:
        raise ProfileError(f"region dataset has no ISDBT channels for {query!r}")
    return [
        {
            "delivery_system": "ISDBT",
            "physical_channel": channel,
            "frequency_hz": _frequency_for_channel(channel),
            "label": f"{label_prefix} ch{channel}",
        }
        for channel in channels
    ]


def _legacy_dataset_candidates(
    profile: dict[str, Any], dataset: dict[str, Any]
) -> list[dict[str, Any]]:
    reject_unknown(dataset, {"schema_version", "dataset_version", "entries"}, "dataset")
    if dataset.get("schema_version") != 1:
        raise ProfileError("dataset.schema_version must be 1 or 2")
    if not isinstance(dataset.get("dataset_version"), str) or not dataset["dataset_version"]:
        raise ProfileError("dataset.dataset_version is required")
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

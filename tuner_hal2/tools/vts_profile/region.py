from __future__ import annotations

import re
from typing import Any

from .model import FRONTEND_ID, ProfileError, positive_int, reject_unknown, require_dict, validate_profile

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


def _validate_japan_region_query(query: str) -> None:
    compact = re.sub(r"[\s-]", "", query)
    if re.fullmatch(r"\d{7}", compact):
        return
    if any(prefecture in query for prefecture in JAPAN_PREFECTURES):
        return
    raise ProfileError(
        "builtin ISDBT region resolution requires a Japanese 7-digit postal code "
        "or an address containing a prefecture name"
    )


def _builtin_isdbt_candidates() -> list[dict[str, Any]]:
    return [
        {
            "delivery_system": "ISDBT",
            "physical_channel": channel,
            "frequency_hz": ISDBT_CHANNEL_13_HZ
            + (channel - ISDBT_FIRST_CHANNEL) * ISDBT_CHANNEL_STEP_HZ,
            "label": f"Japan UHF {channel}",
        }
        for channel in range(ISDBT_FIRST_CHANNEL, ISDBT_LAST_CHANNEL + 1)
    ]


def _dataset_candidates(
    profile: dict[str, Any], dataset: dict[str, Any]
) -> list[dict[str, Any]]:
    reject_unknown(dataset, {"schema_version", "dataset_version", "entries"}, "dataset")
    if dataset.get("schema_version") != 1:
        raise ProfileError("dataset.schema_version must be 1")
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
    fe_type = profile["frontend"]["type"]

    if dataset is None:
        if fe_type != "ISDBT":
            raise ProfileError(
                "automatic region resolution without a dataset is supported only for ISDBT"
            )
        _validate_japan_region_query(query)
        matches = _builtin_isdbt_candidates()
    else:
        matches = _dataset_candidates(profile, dataset)

    if not matches:
        raise ProfileError(f"no {fe_type} candidates found for region {query!r}")
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

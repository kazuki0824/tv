from __future__ import annotations

from typing import Any

from .model import FRONTEND_ID, ProfileError, positive_int, reject_unknown, require_dict, validate_profile

ISDBT_FIRST_CHANNEL = 13
ISDBT_LAST_CHANNEL = 52
ISDBT_CHANNEL_13_HZ = 473_142_857
ISDBT_CHANNEL_STEP_HZ = 6_000_000


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
        matches = _builtin_isdbt_candidates()
        region["dataset_version"] = "builtin-japan-isdbt-uhf-13-52-v1"
    else:
        matches = _dataset_candidates(profile, dataset)
        region["dataset_version"] = str(dataset["dataset_version"])

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

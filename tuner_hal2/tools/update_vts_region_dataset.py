from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any

from vts_profile.ina4n_dataset import generate_snapshot, live_descriptor


def _load_overrides(path: str | None) -> dict[str, Any] | None:
    if path is None:
        return None
    value = json.loads(Path(path).read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise SystemExit("coordinate override file must contain a JSON object")
    return value


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", required=True)
    parser.add_argument(
        "--mode",
        choices=("live", "snapshot"),
        default="live",
        help="write the lightweight live INA4N descriptor or a full transmitter snapshot",
    )
    parser.add_argument(
        "--coordinate-overrides",
        help="optional transmitter-id keyed A-PAB coordinate override JSON for snapshot mode",
    )
    args = parser.parse_args()

    if args.mode == "live":
        if args.coordinate_overrides:
            raise SystemExit("--coordinate-overrides is only valid with --mode snapshot")
        dataset = live_descriptor()
    else:
        dataset = generate_snapshot(_load_overrides(args.coordinate_overrides))

    output = Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(
        json.dumps(dataset, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

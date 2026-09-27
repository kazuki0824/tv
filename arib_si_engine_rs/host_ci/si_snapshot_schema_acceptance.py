#!/usr/bin/env python3
import json
import sys
from pathlib import Path

from jsonschema import Draft202012Validator
from referencing import Registry, Resource


def main() -> int:
    if len(sys.argv) != 2:
        raise SystemExit("usage: si_snapshot_schema_acceptance.py <repo-root>")
    repo_root = Path(sys.argv[1])
    schema_dir = repo_root / "arib_si_engine_rs" / "schema"
    schema = json.loads((schema_dir / "si_snapshot_v2.schema.json").read_text())
    resources = []
    for path in sorted(schema_dir.glob("*.schema.json")):
        document = json.loads(path.read_text())
        schema_id = document.get("$id")
        if schema_id:
            resources.append((schema_id, Resource.from_contents(document)))
    registry = Registry().with_resources(resources)
    Draft202012Validator.check_schema(schema)
    validator = Draft202012Validator(schema, registry=registry)

    payloads = json.load(sys.stdin)
    if not isinstance(payloads, list) or not all(isinstance(item, str) for item in payloads):
        raise SystemExit("stdin must be a JSON array of JSON strings")

    accepted = []
    for raw in payloads:
        try:
            value = json.loads(raw)
        except json.JSONDecodeError:
            accepted.append(False)
            continue
        accepted.append(validator.is_valid(value))

    json.dump(accepted, sys.stdout, separators=(",", ":"))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

from pathlib import Path
import json


def replace_once(path: str, old: str, new: str) -> None:
    p = Path(path)
    text = p.read_text()
    if text.count(old) != 1:
        raise SystemExit(f"expected exactly one match in {path}: {old!r}")
    p.write_text(text.replace(old, new, 1))


replace_once(
    "arib_si_engine_rs/src/lib.rs",
    "struct BulkSnapshot {\n    ingest_sequence: u64,",
    "struct BulkSnapshot {\n    schema_version: u32,\n    ingest_sequence: u64,",
)
replace_once(
    "arib_si_engine_rs/src/lib.rs",
    "serde_json::to_string(&BulkSnapshot {\n        ingest_sequence,",
    "serde_json::to_string(&BulkSnapshot {\n        schema_version: 1,\n        ingest_sequence,",
)
replace_once(
    "tis/src/com/maleicacid/tvinput/aribsi/NativeAribSiParser.kt",
    '        val root = JSONObject(raw.ifBlank { "{}" })\n        val serviceFacts = parseServiceSemanticFacts(root.optJSONArray("serviceSemanticFacts"))',
    '        val root = JSONObject(raw.ifBlank { "{}" })\n        check(root.optInt("schemaVersion", -1) == SI_SNAPSHOT_SCHEMA_VERSION) {\n            "未対応のSI snapshot schemaVersion=${root.optInt(\"schemaVersion\", -1)}"\n        }\n        val serviceFacts = parseServiceSemanticFacts(root.optJSONArray("serviceSemanticFacts"))',
)
replace_once(
    "tis/src/com/maleicacid/tvinput/aribsi/NativeAribSiParser.kt",
    "class NativeAribSiParser : AutoCloseable {",
    "class NativeAribSiParser : AutoCloseable {\n    private companion object {\n        const val SI_SNAPSHOT_SCHEMA_VERSION = 1\n    }",
)

schema = {
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "$id": "https://maleicacid.example/schema/si_snapshot_v1.schema.json",
    "title": "ARIB SI bulk snapshot v1",
    "type": "object",
    "additionalProperties": False,
    "required": [
        "schemaVersion",
        "collectionGeneration",
        "ingestSequence",
        "discoveryStage",
        "broadcastClock",
        "tableRequirements",
        "catCaMetadata",
        "malformedCaDescriptorDiagnostics",
        "malformedCaDescriptorCounts",
        "transportSemanticFacts",
        "events",
        "eitInstances",
        "serviceSemanticFacts",
        "parserDiagnostics",
    ],
    "properties": {
        "schemaVersion": {"const": 1},
        "collectionGeneration": {"type": "integer", "minimum": 0},
        "ingestSequence": {"type": "integer", "minimum": 0},
        "discoveryStage": {"type": "integer", "minimum": 0, "maximum": 2},
        "broadcastClock": {"type": ["object", "null"]},
        "tableRequirements": {"type": "array", "items": {"type": "object"}},
        "catCaMetadata": {"type": "array", "items": {"type": "object"}},
        "malformedCaDescriptorDiagnostics": {"type": "array", "items": {"type": "object"}},
        "malformedCaDescriptorCounts": {"type": "array", "items": {"type": "object"}},
        "transportSemanticFacts": {"type": "array", "items": {"type": "object"}},
        "events": {"type": "array", "items": {"type": "object"}},
        "eitInstances": {"type": "array", "items": {"type": "object"}},
        "serviceSemanticFacts": {"type": "array", "items": {"type": "object"}},
        "parserDiagnostics": {"type": "array", "items": {"type": "object"}},
    },
}
schema_path = Path("arib_si_engine_rs/schema/si_snapshot_v1.schema.json")
schema_path.write_text(json.dumps(schema, ensure_ascii=False, indent=2) + "\n")

fixture = {
    "schemaVersion": 1,
    "collectionGeneration": 0,
    "ingestSequence": 0,
    "discoveryStage": 0,
    "broadcastClock": None,
    "tableRequirements": [],
    "catCaMetadata": [],
    "malformedCaDescriptorDiagnostics": [],
    "malformedCaDescriptorCounts": [],
    "transportSemanticFacts": [],
    "events": [],
    "eitInstances": [],
    "serviceSemanticFacts": [],
    "parserDiagnostics": [],
}
fixture_path = Path("arib_si_engine_rs/testdata/si_snapshot_v1/minimal.json")
fixture_path.parent.mkdir(parents=True, exist_ok=True)
fixture_path.write_text(json.dumps(fixture, ensure_ascii=False, indent=2) + "\n")

replace_once(
    ".github/workflows/arib-si-engine-host-ci.yml",
    '          Draft202012Validator.check_schema(schema)\n          validator = Draft202012Validator(schema, registry=registry)',
    '          Draft202012Validator.check_schema(schema)\n          snapshot_schema = json.loads((schema_dir / "si_snapshot_v1.schema.json").read_text())\n          Draft202012Validator.check_schema(snapshot_schema)\n          snapshot_validator = Draft202012Validator(snapshot_schema, registry=registry)\n          snapshot_fixture_dir = Path("arib_si_engine_rs/testdata/si_snapshot_v1")\n          for path in sorted(snapshot_fixture_dir.glob("*.json")):\n              snapshot_validator.validate(json.loads(path.read_text()))\n              print(path)\n          validator = Draft202012Validator(schema, registry=registry)',
)

replace_once(
    "arib_si_engine_rs/DESIGN_JA.md",
    "JNIは同じ放送事実をbulkで渡す。TISの公開判断の唯一のownerはKotlin `EpgPublicationPolicy` / `EpgSectionPolicy`であり、具体契約は`../tis/DESIGN_JA.md`の「TIS / EPG 公開境界」を正とする。",
    "JNIは同じ放送事実をbulkで渡す。Rust→TISのbulk JSON境界は`schema/si_snapshot_v1.schema.json`を唯一のwire contractとし、`schemaVersion=1`を必須とする。互換性のない変更ではversionを更新し、TISは未対応versionを解釈しない。TISの公開判断の唯一のownerはKotlin `EpgPublicationPolicy` / `EpgSectionPolicy`であり、具体契約は`../tis/DESIGN_JA.md`の「TIS / EPG 公開境界」を正とする。",
)
replace_once(
    "tis/DESIGN_JA.md",
    "`AribSiEngine` 呼び出し側は複数 snapshot を合成してはならない。本番経路は以下の用途別bulk DTOを使う。engineから受け取るpolicy入力は`ServiceSemanticFacts`・event・EIT instanceの放送/受信事実であり、`ProgramPublishability`等のTIS product policyをRust側DTOに持たせない。",
    "`AribSiEngine` 呼び出し側は複数 snapshot を合成してはならない。本番経路は以下の用途別bulk DTOを使う。Rust→TISのbulk JSONは`../arib_si_engine_rs/schema/si_snapshot_v1.schema.json`を唯一のwire contractとし、TISは対応する`schemaVersion`だけを受理する。engineから受け取るpolicy入力は`ServiceSemanticFacts`・event・EIT instanceの放送/受信事実であり、`ProgramPublishability`等のTIS product policyをRust側DTOに持たせない。",
)

Path(".github/workflows/temporary-si-snapshot-contract.yml").unlink()
Path(".github/scripts/apply_si_snapshot_contract.py").unlink()

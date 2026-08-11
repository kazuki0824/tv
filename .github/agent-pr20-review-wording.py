from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    p = Path(path)
    text = p.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one match, got {count}: {old!r}")
    p.write_text(text.replace(old, new), encoding="utf-8")


replace_once(
    "tis/DESIGN_JA.md",
    "`RELATIVE`はdriver固有値になるため、TISの通常channelデータへ保存しない。",
    "`RELATIVE`はAOSP Tuner AIDLで合法なtune-time selector種別だが、本製品では永続channel tune identityとして採用しないため、TISの通常channelデータへ保存しない。",
)

replace_once(
    "tis/DESIGN_JA.md",
    "TV input ownershipはchannel rowのrequired fieldである`TvContract.Channels.COLUMN_INPUT_ID`を唯一のSSOTとする。",
    "channelとTvInputServiceの関連付けはchannel rowのrequired fieldである`TvContract.Channels.COLUMN_INPUT_ID`を唯一のSSOTとする。",
)

replace_once(
    "tis/DESIGN_JA.md",
    "`0x01`のdigital television serviceと`0x02`のdigital radio sound serviceだけに恒久固定する。",
    "`0x01`の`Digital television service`と`0x02`の`Digital audio service`だけに恒久固定する。",
)

changelog = Path("tis/CHANGELOG.md")
text = changelog.read_text(encoding="utf-8")
entry = """# r50ee99_review_wording_precision\n\n- AOSP frozen Tuner AIDLで`RELATIVE_STREAM_NUMBER`が合法なselector種別であることを明示し、永続channel tune identityでは採用しないという製品設計理由へ表現を修正した。\n- `Channels.COLUMN_INPUT_ID`の責務をTV input ownership一般ではなく、channelとTvInputServiceの関連付けのSSOTとして限定した。\n- ARIB STD-B10 5.13-E1 Part 2 Table 6-25に合わせ、`0x01`を`Digital television service`、`0x02`を`Digital audio service`と表記した。\n- schemaおよび実装コード変更なし。文言整合と`git diff --check`のみ確認し、Android/Soong build、Rust unit test、atest、CTS、VTS、実機確認は未実施。\n\n"""
if text.startswith("# r50ee99_review_wording_precision"):
    raise SystemExit("changelog entry already exists")
changelog.write_text(entry + text, encoding="utf-8")

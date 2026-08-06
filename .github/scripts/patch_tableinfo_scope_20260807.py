from pathlib import Path

path = Path("tuner_hal/DESIGN_JA.md")
text = path.read_text(encoding="utf-8")

old_contract = """- Android 14 `SectionSettings`の`repeat=false`がいうall sectionsは、ISO/IEC 13818-1／ARIBの拡張section構文で1個のtableを構成する`section_number=0..last_section_number`である。table idだけではsection番号空間を一意にできず、同じtable idでも`table_id_extension`、actual version、`current_next_indicator`が異なれば別のtable instanceである。本設計では`TableInstanceKey={input_origin_generation, filter_generation, PID, table_id, table_id_extension, actual_version, current_next_indicator}`を内部同一性とし、別instanceの同じsection番号を一つのtableへ混成しない。
"""
new_contract = """- Android 14 `SectionSettings`の`repeat=false`が明記するのは、`TableInfo`でtable IDとversionに基づくall sectionsを配送した後に停止することまでである。同一PID上で公開条件に一致する複数の`table_id_extension`、actual version、`current_next_indicator`のどれをone-shot対象にするか、および候補全体の有限終端はAOSP公開契約では規定されない。ISO/IEC 13818-1／ARIBの拡張section構文では、`section_number=0..last_section_number`の完結性は1個のtable instance内で成立し、同じtable IDでも`table_id_extension`、actual version、`current_next_indicator`が異なれば別のsection番号空間を持つ。本設計では`TableInstanceKey={input_origin_generation, filter_generation, PID, table_id, table_id_extension, actual_version, current_next_indicator}`を内部同一性とし、別instanceの同じsection番号を一つのtableへ混成しない。
"""

old_resolution = """- AOSP公開面は`table_id_extension`または全subtable集合の列挙・終端通知を持たない一方、`repeat=false`には有限な停止点が必要である。このため`TableInfo repeat=false`は、公開条件に一致して入力順で最初に受理した構造上完全なsectionが属する1個の`TableInstanceKey`を要求tableとして確定する。これは追加のcaller-visible filter条件ではなく、公開条件が選んだtable種別・versionから、規格上の1個のtableを決定するone-shot解決規則である。`version=-1`はtarget選択まではwildcardのまま維持し、選択後のactual versionは設定値の書換えではなくtable instance identityとして固定する。
"""
new_resolution = """- 本製品は、AOSP未規定の複数候補解決として、公開条件に一致して入力順で最初に受理した構造上完全なsectionが属する1個の`TableInstanceKey`をone-shot targetに選ぶ。first-instanceはAOSPの明文要求ではなく、有限なsnapshotを決定的に選択する製品内規則である。これはcaller-visible filter条件を追加するものでも、AOSPがall sectionsを1個のinstanceと定義したと主張するものでもない。`version=-1`はtarget選択時のwildcardであり、全actual versionを1回で配送する指定ではない。target確定後のactual version固定は設定値の書換えではなくtable instance identityである。全serviceのEIT等、複数instanceを包括的・継続的に取得するcallerは`repeat=true`を使用し、SI engineがinstance別の完成を管理した後に明示的に`stop()`する。
"""

for old, new, label in (
    (old_contract, new_contract, "AOSP/TS contract boundary"),
    (old_resolution, new_resolution, "product first-instance resolution"),
):
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"expected exactly one {label} paragraph, found {count}")
    text = text.replace(old, new, 1)

for required in (
    "候補全体の有限終端はAOSP公開契約では規定されない",
    "first-instanceはAOSPの明文要求ではなく",
    "複数instanceを包括的・継続的に取得するcallerは`repeat=true`を使用",
):
    if required not in text:
        raise SystemExit(f"missing required phrase: {required}")

for forbidden in (
    "Android 14 `SectionSettings`の`repeat=false`がいうall sectionsは、ISO/IEC 13818-1",
    "規格上の1個のtableを決定するone-shot解決規則である",
):
    if forbidden in text:
        raise SystemExit(f"stale overclaim remains: {forbidden}")

path.write_text(text, encoding="utf-8")

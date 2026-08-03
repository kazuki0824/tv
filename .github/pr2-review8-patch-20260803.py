from __future__ import annotations

import re
import sys
from pathlib import Path

root = Path(sys.argv[1])


def read(path: str) -> str:
    return (root / path).read_text(encoding="utf-8")


def write(path: str, text: str) -> None:
    (root / path).write_text(text, encoding="utf-8")


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise RuntimeError(f"{label}: expected exactly one match, got {count}")
    return text.replace(old, new, 1)


def replace_between(text: str, start: str, end: str, new: str, label: str) -> str:
    count = text.count(start)
    if count != 1:
        raise RuntimeError(f"{label}: expected one start marker, got {count}")
    i = text.find(start)
    j = text.find(end, i)
    if j < 0:
        raise RuntimeError(f"{label}: end marker not found")
    return text[:i] + new + text[j:]


hal_path = "tuner_hal/DESIGN_JA.md"
hal = read(hal_path)

hal = replace_between(
    hal,
    "- セクションフィルターの`repeat=false`は重複抑止ではなく",
    "- PES `streamId`は",
    """- セクションフィルターの`repeat=false`は重複抑止ではなく、同一`start()`世代内の配送停止条件である。`SectionBits`は最初に一致したsectionを1件配送した後に自動配送を停止する。\n- `TableInfo repeat=true`は対応する。AOSP公開条件であるtable idとversionだけで照合し、明示versionではそのversion、`version=-1`では全actual versionを対象として、条件に一致する構造上完全なsectionを継続配送する。callerが指定していないPID、table種別、`table_id_extension`、`last_section_number`、`ProductProfile`の私的一覧で対象を狭めない。\n- `TableInfo repeat=false`は、AOSP契約上、callerが指定したtable idとversionに基づくall sectionsを配送してから停止しなければならない。しかしAndroid 14 AIDLには総`table_id_extension`数、対象actual version集合、終了通知がなく、MPEG-TSの`last_section_number`が完結させるのは個々のtable instanceだけである。現行ARIB対象範囲にも、受理可能な全table IDについて未観測instanceの不存在を証明できる単一の規範的最大送出周期はない。このため、汎用的な有限完了を証明できない現行`ProductProfile`では当該組合せを対応済みと表明せず、`configure()`のvalidate段階で`UNAVAILABLE`を返す。既存設定、filter generation、queue、追跡状態を変更しない。\n- `TableInfo repeat=false`を、時間窓、最初に完成したcandidate、最初に観測したextension/version、非公開table一覧、再送一巡の推測で成功扱いにしてはならない。将来対応する場合は、公開条件だけから対象全集合と有限終端を証明できる入力構文またはAOSP側の終了情報、および複数extension/versionの適合試験を同一変更で追加する。\n- `TableInfo.version`は`-1`または`0..31`だけを受け付ける。`-1`はwildcardであり、runtimeの最初の観測値へ固定しない。範囲外は`INVALID_ARGUMENT`とする。\n""",
    "TableInfo contract",
)

hal = replace_between(
    hal,
    "| T-SEC-14 |",
    "| T-SEC-15 |",
    """| T-SEC-14 | `TableInfo repeat=false` | `UNAVAILABLE`、設定・generation・queue・追跡状態に副作用なし |\n| T-SEC-14a | `version=-1`かつ`TableInfo repeat=false` | wildcardを観測値へ固定せず、同じく`UNAVAILABLE`・副作用なし |\n| T-SEC-14b | 複数extension/versionが並行する`TableInfo repeat=true` | table id/version条件に一致する全sectionを継続配送し、first-winnerや時間窓で停止しない |\n| T-SEC-14c | VTS/product profile | 有限完了を証明する契約と試験が追加されるまで`TableInfo repeat=false`を成功scenarioへ入れない |\n""",
    "TableInfo tests",
)

hal = replace_between(
    hal,
    "| AT-001 | `IFrontend.tune()` / 再選局 |",
    "| AT-002 |",
    """| AT-001 | `IFrontend.tune()` / 再選局 | 表19のvalidate・prepare後、安定同一条件なら非破壊re-entry、その他は確定A・backend要求・確定B | 安定同一条件は`request_sequence`更新と`LOCKED`配送予約の確定時。full retuneは新generationを確定Bで公開した時 | re-entry判定前の失敗は旧状態を維持する。確定A後に新要求が拒否された場合は旧要求を自動再投入せず、backend停止・境界終端を確認できれば`Untuned`、結果不明は`FailedBackend`、境界不明は`FailedBoundary`、fence不成立は`Quarantined`へ進む | frontend、旧世代、失敗したdemux境界 | 表19の失敗分類に従う | 非破壊re-entryに確定A/Bを適用しない。破壊的commit後の旧session復元経路を設けない |\n""",
    "AT-001",
)

old_tune_row = "| `IFrontend.tune(settings)` | normalized tune settings が現在条件と同一でも、受理した公開呼出しは新transaction / generationへ進める | 公開transaction、旧generationのfencing、demux boundary、callback契約を省略してはならない。backend固有の同一設定書込みだけは、これらを維持できる場合に省略可 | 異なる条件も同じ公開transaction規則で旧tune停止、新generation、新tune投入、boundary resetを行う |"
new_tune_row = "| `IFrontend.tune(settings)` | `Locked`でnormalized settings、typed selector、LNB/power条件が同一かつbackendとstream boundaryがhealthy | 非破壊re-entryとし、`request_sequence`更新と現lockの`LOCKED`再通知だけを行う。stream generation、worker、backend要求、demux境界、AVを維持する | 条件不一致、旧tune未完了、または同値性・健全性を証明できない場合だけ表19のfull retuneへ進む |"
hal = replace_once(hal, old_tune_row, new_tune_row, "same-setting table row")

old_capability_paragraph = "- demux、型別filter、DVRの個数は、frontendと公開可能LNBの検出後、`ProductProfile`が列挙する完全な`RuntimeCapabilityVector`から選ぶ。各vectorは任意の非負整数を使用でき、2の冪へ丸めない。object数、worker、callback、reaper、cleanup、PES/AV/playback/FMQ byte予算をvector全体で一括予約し、候補間の列を混成しない。機能群ごとの縮退は他群の値を維持した完全vectorとして明示する。確定値は`CapabilitySnapshot`へ格納し、open/配送時の実領域はsnapshot残量から割り当てる。PES assemblerは全ての有効な明示stream IDとwildcardを同じ能力で扱い、宣言長ありPESと映像stream IDの長さ0 PESを`MAX_PES_BUFFER_BYTES`および`pesRuntimeBudgetBytes`内で保持する。Tuner VTSは別途起動前環境へ結び付け、入力元、PID、経路、queue容量、memory予算が定義されるまで`DESIGN_HOLD_VTS_ENVIRONMENT_UNDECLARED`とする。"
new_capability_paragraph = "- demux、型別filter、DVRの個数とbyte予算は、frontend/backend/電源、demux base、main type別filter/FMQ、PES、AV、playback/record DVR、worker/callback/reaper/cleanup共有枠の`CapabilityClosure`ごとに原子的に検証・予約する。各閉包の失敗は、その閉包を必要とする能力だけを非公開にし、依存しないfrontend、filter種別、DVR種別へ波及させない。選択済み閉包を合成した後、query/openの同一性、`numDemux`、`filterCaps`、用途別個数、全byte台帳の横断不変条件を一括検証し、変更不能な`CapabilitySnapshot`として確定する。PES assemblerは全ての有効な明示stream IDとwildcardを同じPES閉包で扱い、宣言長ありPESと映像stream IDの長さ0 PESを`MAX_PES_BUFFER_BYTES`および`pesRuntimeBudgetBytes`内で保持する。Tuner VTSは別途起動前環境へ結び付け、入力元、PID、経路、queue容量、memory予算が定義されるまで`DESIGN_HOLD_VTS_ENVIRONMENT_UNDECLARED`とする。"
hal = replace_once(hal, old_capability_paragraph, new_capability_paragraph, "capability boundary paragraph")

capability_section_pattern = re.compile(
    r"### `CapabilitySnapshot` の完全能力ベクトル\n.*?(?=\n### )",
    re.DOTALL,
)
capability_section = """### `CapabilitySnapshot` の依存閉包合成

`ProductProfile`は全能力を一個の候補vectorとして一括採否せず、次の`CapabilityClosure`ごとに優先順を持つ有限候補を宣言する。候補値は任意の非負整数とし、実資源を2の冪へ丸めない。

| 閉包 | 原子的に確定する内容 | 依存先 | 失敗時の縮退範囲 |
|---|---|---|---|
| frontend | backend、電源トポロジ、frontend object、tune/scan worker、callback、期限資源 | 機器probeと共有worker基盤 | 当該frontendだけを非公開 |
| demux base | demux object、入力境界、共通packet処理、基礎worker/cleanup枠 | 共有worker基盤 | demuxと配下能力だけを非公開 |
| filter main type / FMQ | main type別object数、FMQ byte、callback、assembler、配送worker | demux base、共有worker基盤 | 当該main typeだけを非公開 |
| PES | PES filter数、assembler、`pesRuntimeBudgetBytes` | section以外の対象filter閉包、demux base | PES能力だけを非公開 |
| AV | AV filter数、1 event、filter別未解放総量、runtime総量、allocator/handle台帳 | 対象filter閉包、demux base | AV能力だけを非公開 |
| DVR playback / record | 用途別object数、FMQ、処理中buffer、worker、callback | demux base、共有worker基盤 | 当該DVR用途だけを非公開 |
| shared runtime | worker、callback、reaper、cleanup authority、診断台帳の共有上限 | なし | 依存する閉包だけを候補から除外 |

各閉包は、必要な共有runtime claimを含む全依存資源を同一transactionで仮予約し、全て成功した候補だけを選ぶ。ある閉包の失敗を理由に、依存関係のない閉包を落としてはならない。共有枠が複数閉包で競合する場合は`ProductProfile`の固定優先順で候補を評価し、先に確定したclaimを後続候補が越えないようにする。候補間の数値を無制約に組み合わせるのではなく、各閉包自身の内部不変条件と明示した依存辺を保ったまま合成する。

全閉包の選択後、次を一括検証して変更不能な`CapabilitySnapshot`を確定する。

- `getFrontendIds()`、`getFrontendInfo()`、open受付が同じfrontend集合を参照する。
- `getDemuxIds()`、`getDemuxInfo()`、`getDemuxCaps()`、open受付が同じdemux集合と個数を参照する。
- `numDemux`、main type別`filterCaps`、PES/AV/DVR個数が、依存先demuxと共有runtime claimを越えない。
- FMQ、PES、AV、playback処理中buffer、callback、worker、reaper、cleanupの各台帳上限が、選択済み閉包の合計claim以上である。
- capability query、open、configure、start、配送の受付判定が、同じsnapshotと台帳残量だけを入力にする。

合成後の横断検証に失敗した場合はsnapshotを公開せず、全仮予約を逆順に返却する。サービス寿命中にsnapshotの個数または能力集合を部分更新しない。open/配送時の実領域はsnapshotの閉包別台帳残量から割り当てる。
"""
hal, count = capability_section_pattern.subn(capability_section, hal, count=1)
if count != 1:
    raise RuntimeError(f"capability section: expected one match, got {count}")

stale_hal = [
    "tableInfoCompletionWindowMs",
    "tableInfoTrackingBudgetBytes",
    "復元を含む2確定点",
    "設定の同異にかかわらず新規要求受理時は確定B",
    "完全な`RuntimeCapabilityVector`",
    "完全能力ベクトル",
    "vector全体で一括予約",
    "候補間の列を混成しない",
]
for stale in stale_hal:
    if stale in hal:
        raise RuntimeError(f"stale HAL text remains: {stale}")

required_hal = [
    "`TableInfo repeat=false`は、AOSP契約上",
    "validate段階で`UNAVAILABLE`",
    "非破壊re-entryに確定A/Bを適用しない",
    "`CapabilitySnapshot` の依存閉包合成",
    "`CapabilityClosure`",
]
for required in required_hal:
    if required not in hal:
        raise RuntimeError(f"required HAL text missing: {required}")

write(hal_path, hal)

hal2_path = "tuner_hal2/DESIGN_JA.md"
hal2 = read(hal2_path)
old_hal2_row = "| frontend tune/scan、再選局、終端deadline | 設計済み・実装未適用 | `service_runtime/`、`device/`、callback配送 | AOSP callback契約を満たす終端、旧session遮断、新要求失敗時restore、deadlineの試験が合格 |"
new_hal2_row = "| frontend tune/scan、再選局、終端deadline | 設計済み・実装未適用 | `service_runtime/`、`device/`、callback配送 | AOSP callback契約を満たす終端、安定同一条件の非破壊re-entry、full retuneでの旧session遮断、破壊的commit後に旧要求を再投入しないこと、旧TSが新demux/filter世代へ混入しないこと、原因別の`Untuned`／`FailedBackend`／`FailedBoundary`／`Quarantined`遷移、deadlineの試験が合格 |"
hal2 = replace_once(hal2, old_hal2_row, new_hal2_row, "tuner_hal2 retune completion row")
if "新要求失敗時restore" in hal2:
    raise RuntimeError("stale tuner_hal2 restore text remains")
write(hal2_path, hal2)

for path in (hal_path, hal2_path):
    text = read(path)
    if text.count("```") % 2 != 0:
        raise RuntimeError(f"unbalanced Markdown fences: {path}")

print("review8 patch applied")

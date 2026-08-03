from __future__ import annotations

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
    i = text.find(start)
    if i < 0:
        raise RuntimeError(f"{label}: start marker not found")
    j = text.find(end, i)
    if j < 0:
        raise RuntimeError(f"{label}: end marker not found")
    return text[:i] + new + text[j:]


def replace_fenced_block_containing(text: str, needle: str, new_block: str, label: str) -> str:
    p = text.find(needle)
    if p < 0:
        raise RuntimeError(f"{label}: needle not found")
    start = text.rfind("```mermaid", 0, p)
    if start < 0:
        raise RuntimeError(f"{label}: opening fence not found")
    end = text.find("```", p)
    if end < 0:
        raise RuntimeError(f"{label}: closing fence not found")
    end += 3
    return text[:start] + new_block + text[end:]


hal_path = "tuner_hal/DESIGN_JA.md"
hal = read(hal_path)

hal = replace_between(
    hal,
    "- `TableInfo repeat=false`は、AOSP公開条件であるtable idとversionを変更せず",
    "- PES `streamId`は",
    """- `TableInfo repeat=false`は、AOSP公開条件であるtable idとversionだけで照合する。配送済みkeyは`(actual_version, table_id_extension, section_number)`とし、明示versionではそのversionだけ、`version=-1`では全actual versionを対象にする。callerが指定していないversion、extension、PID、table種別または`ProductProfile`の私的一覧で候補を狭めない。\n- 構造上完全でCRC条件を満たす各sectionは、同一`start()`世代で同じkeyを最初に観測した時点で直ちに1回配送する。payloadを完了待ちbufferへ保持せず、候補ごとに`last_section_number`、受信済みsection番号bitmap、version、extension、最終新規key時刻だけを保持する。同じcandidateの`0..last_section_number`が揃えばcandidate完成とする。`last_section_number`が同じcandidate内で変化した場合は矛盾したcandidateとして完成扱いせず、型付き診断を残す。\n- 自動配送停止は、観測済みcandidateが全て完成し、かつ最後の新規matching keyから`ProductProfile.tableInfoCompletionWindowMs`の間、新しいversion、extension、section keyを観測しなかった時点とする。新規keyを受信した場合はwindowを最初から測り直す。`tableInfoCompletionWindowMs`は、現行ARIB TS profileでTableInfo条件に入り得る全table IDについて、適用するARIB伝送運用規定の最大送出反復間隔の最大値に、scheduler遅延と受信jitterの検証済み余裕を加えて導出する単一の正値とする。table ID別の非公開許可表や到着順によるwinner選択には使わない。profile検証では、この単一windowが対象ARIB profile全体を覆うことを証明する。\n- `version=-1`は停止までwildcardのまま維持し、window内に観測した複数actual versionのsectionをそれぞれ配送・完成管理する。観測済みcandidateが未完成の間はquiescence windowが経過しても成功停止せず、利用側の`flush()`、`stop()`または`close()`まで収集を継続する。規定反復間隔を満たさない入力から未観測candidateの不存在を推測しない。正常完了後は自動配送だけを停止し、filter objectの公開lifecycleはStartedのまま維持する。\n- `TableInfo repeat=false`の追跡状態は`RuntimeCapabilityVector.tableInfoTrackingBudgetBytes`から原子的にclaimする。bitmapまたはcandidate metadataの追加が予算を超える場合は`DemuxFilterStatus::OVERFLOW`と型付き診断を1回通知し、内部`table_info_overflow_latched`を立てて以後の自動配送を停止するが、成功完了とは扱わない。公開lifecycleはStartedのままとし、`flush()`だけが追跡状態とoverflow latchを破棄して新しい配送generationで収集を再開する。`stop()`は通常のStopped、`close()`は通常の閉鎖へ進む。overflow後に黙示的な再開、黙示的な成功停止、payload全保持を行わない。`repeat=true`はtable idとversion条件に一致する全sectionを継続配送する。\n- `TableInfo.version`は`-1`または`0..31`だけを受け付ける。`-1`はwildcardであり、runtimeの最初の観測値へ固定しない。範囲外は`INVALID_ARGUMENT`とする。\n""",
    "TableInfo contract",
)

hal = replace_between(
    hal,
    "| T-SEC-14 |",
    "| T-SEC-15 |",
    """| T-SEC-14 | 複数extensionが並行する`TableInfo repeat=false` | table id/version条件に一致する各固有section keyを到着時に1回配送し、first-winnerで破棄しない |\n| T-SEC-14a | `version=-1`で複数actual versionが並行 | wildcardを固定せず、versionごと・extensionごとに独立bitmapを持ち、全て配送する |\n| T-SEC-14b | 最大反復間隔の直前に新しいextensionまたはversionを受信 | completion windowを再開し、そのcandidateの完成前に停止しない |\n| T-SEC-14c | 全観測candidate完成後、単一のARIB由来completion window中に新規keyなし | 自動配送を停止し、公開lifecycleはStartedを維持する |\n| T-SEC-14d | candidate metadata予算枯渇 | OVERFLOWを1回通知してlatchし、`flush()`まで再開しない。payload全保持は行わない |\n| T-SEC-14e | overflow後の`flush()` | bitmap、metadata、latchを破棄し、新しい配送generationで収集を再開する |\n| T-SEC-14f | multi-subtable tableで`repeat=true` | table id/version条件に一致する全sectionを継続配送する |\n""",
    "TableInfo tests",
)

hal = replace_once(
    hal,
    "旧操作を正常に停止できた後、新しい`tune()`要求の受理だけが失敗した場合は、表19の復元用snapshotから旧要求を正確に1回復元する。",
    "旧操作を正常に停止した後、新しい`tune()`要求が拒否された場合は旧要求を自動再投入しない。旧generationは既に遮断され、旧demux境界も終端しているため、旧TSをcallerが新サービス向けに再構成したfilterへ流さず、表19の原因別失敗状態へ進む。",
    "retune summary",
)

hal = replace_once(
    hal,
    "validate には、settings型、周波数範囲、frontend capability、LNB候補を含める。prepare には、ワーカー生成準備、コールバック経路 準備可能性、バックエンドロールバック経路 準備可能性を含める。",
    "validateにはsettings型、周波数範囲、frontend capability、LNB候補を含める。prepareにはworker、callback、backend requestの局所的な受付可能性、必要資源、旧generationを遮断した後の失敗回収経路を含め、旧tuneを破壊する前に確認可能な条件を全て確定する。backendへ実要求を送らなければ判定できない拒否だけをcommit A後に残す。",
    "retune validate prepare",
)

hal = replace_between(
    hal,
    "| TN-005a | commit A |",
    "| TN-007a | commit B |",
    """| TN-005a | commit A | 旧generationへのcallback・queue・backend確定権限を遮断し、旧workerとbackendを停止して全対象demux境界を終端する。旧設定は診断snapshotとしてだけ保持し、再投入権限を持たせない | 全処理の成功時だけTN-006aへ進む | 復元しない |\n| TN-005b | commit A失敗 / backend停止不明 | backend停止結果を確定できない | `UNKNOWN_ERROR`を返し`FailedBackend`へ移し、新要求を送らない | 復元しない |\n| TN-005c | commit A失敗 / 境界不明・fence成立 | backend停止済みだがdemux境界の終端を確定できない | `UNKNOWN_ERROR`を返し`FailedBoundary`へ移し、新要求を送らない | 復元しない |\n| TN-005d | commit A失敗 / fence不成立 | 旧世代のcallback・queue・backend確定権限を遮断できない | `UNKNOWN_ERROR`を返し`Quarantined`へ移す | 復元しない |\n| TN-006a | backend request | 新しい選局要求をbackendへ正確に1回送り、受理された | TN-007aへ進む | 新要求へ移行中 |\n| TN-006b | backend request拒否 / backend停止・全境界終端を確認 | 新要求の準備物を解放し、旧要求を再投入しない | 新要求の原因別エラーを返し`Untuned`へ移る | 復元しない |\n| TN-006c | backend request結果不明 | 新旧いずれのbackend要求がactiveか確定できない | `UNKNOWN_ERROR`を返し`FailedBackend`へ移す | 復元しない |\n| TN-006d | backend停止済み・境界不明・fence成立 | 新要求を公開せず、不明なdemux境界を隔離する | `UNKNOWN_ERROR`を返し`FailedBoundary`へ移す | 復元しない |\n| TN-006e | fence不成立 | 旧世代または新要求の確定権限を遮断できない | `UNKNOWN_ERROR`を返し`Quarantined`へ移す | 復元しない |\n""",
    "retune failure rows",
)

new_mermaid = """```mermaid
flowchart TD
    A[設定とLNB候補を検証] -->|失敗| B[エラーを返し、旧tuneを維持]
    A --> C[worker・callback・backend受付可能性と失敗回収資源を準備]
    C -->|失敗| B
    C --> D{Lockedかつ設定・selector・給電条件が同一でbackendと境界がhealthyか}
    D -->|はい| E[request_sequenceを更新]
    E --> E2[現lock snapshotのLOCKEDを1回通知]
    E2 --> E3[stream generation・worker・backend・demux境界・AVを維持]
    D -->|いいえ| F[旧generationを遮断しbackendを停止]
    F --> G[旧demux境界を終端]
    G --> H[新しいtune要求を1回送信]
    H -->|送信成功| I[新generationを公開してworkerを有効化]
    H -->|拒否・backend停止と境界終端済み| J[旧要求を再投入せずUntuned]
    H -->|backend結果不明| M[FailedBackend]
    H -->|境界だけ不明・fence成立| N[FailedBoundary]
    H -->|fence不成立| O[Quarantined]
```"""
hal = replace_fenced_block_containing(
    hal,
    "設定の同異にかかわらず旧tuneを停止し旧世代を終端",
    new_mermaid,
    "retune mermaid",
)

hal = replace_between(
    hal,
    "現行ProductProfileのAV未解放payload予算は",
    "- 入力値不正は",
    """AV資源上限は全codec共通の固定byte値にしない。`ProductProfile`は対応するcodec、stream subtype、backendごとに`avMaxEventBytes`と`avMaxOutstandingEventsPerFilter`を持つ。`avMaxEventBytes`は、対応宣言するcodec/profileで成立し得る最大access unitまたはPES payload、HAL assembler上限、allocatorの連続・map可能上限、対象機器とdecoderでの最悪値測定を突き合わせ、正の有限値として導出する。対応codecの正当な最大sampleを収容できないprofileはAV能力を公開しない。allocator上限をcodec上限の代用にせず、単一event上限と未解放payload総量を分離する。\n\n各AV filterの集約上限`avPerFilterLiveBytes`は、当該filterが取り得る最大`avMaxEventBytes`と`avMaxOutstandingEventsPerFilter`のchecked積以上とする。`avRuntimeBudgetBytes`は、最終`CapabilitySnapshot`へ含める各AV能力閉包のfilter別集約上限をchecked加算して導出する。event-local allocationは、event上限、filter集約上限、runtime集約上限を別々にclaimし、`releaseAvHandle()`または後片付け完了時に同じ台帳へ返却する。\n\n構造上有効なeventがprofile導出済み`avMaxEventBytes`を超える場合、または一時的に集約claimできない場合は、handle/dataIdを公開する前に暫定allocationを解放し、`DemuxFilterStatus::OVERFLOW`と原因別診断を通知する。既に公開したhandleを暗黙解放せず、filter lifecycleを失敗へ移さない。容量が返却された後の後続eventは通常どおり再試行可能とし、固定8 MiB閾値だけを理由に対応codecの正当なsampleを恒久dropしない。\n\n公開能力は、サービス初期化時の機器probeと`ProductProfile`から、実際に同時予約が必要な依存閉包ごとに原子的に確定し、最後に1個の変更不能な`CapabilitySnapshot`へ合成する。依存閉包は少なくとも、(1) frontend/backend/電源/frontend worker・callback、(2) demux base/query/open、(3) main type別filterとFMQ、(4) PES assembler、(5) AV allocationとhandle台帳、(6) playback/record DVR、(7) cleanup/reaper共有枠に分ける。各閉包は必要な下位閉包と共有pool claimを同一transactionで予約し、失敗時はその閉包の仮予約だけを戻す。\n\n最終合成は依存順に行い、同じworker、callback、reaper、FMQ byte、AV/PES byteを二重計上しない。AV閉包の不足で無関係なfrontendまたはrecord DVR閉包を落とさず、共有するdemux baseや全体worker poolが不足する場合だけ、その依存先を使う閉包へ失敗を伝播する。`getDemuxIds()`、`getDemuxInfo()`、`getDemuxCaps()`、open受付、`numDemux`、`filterCaps`の横断不変条件は、合成後snapshotから一括導出する。能力広告と受付判定は同じsnapshotだけを参照し、別候補closureの列を実行時に混成しない。\n\nAV payloadは配送時、宣言長ありPESはheaderから必要量を確定した時点、長さ0映像PESは受信量増加時、FMQとplayback処理中bufferはconfigure時に実領域をclaimする。PES filterを非0で公開する場合は、PES閉包が`pesRuntimeBudgetBytes >= MAX_PES_BUFFER_BYTES * pesFilterCount`を満たし、有効な明示`streamId 0..255`とwildcard `0xFFFF`を同じ能力で受理する。ARIB字幕用`0xBD`はTISの利用設定でありHAL capabilityの部分集合ではない。VTSは別の起動前環境bindingとし、未定義中は固定path XMLをinstallせず成功を表明しない。\n\n""",
    "AV and capability contract",
)

required_hal = [
    "tableInfoCompletionWindowMs",
    "table_info_overflow_latched",
    "旧要求を再投入しない",
    "avMaxEventBytes",
    "依存閉包ごとに原子的に確定",
]
for value in required_hal:
    if value not in hal:
        raise RuntimeError(f"missing HAL requirement: {value}")

for stale in [
    "最初に完成したcandidateをwinner",
    "avPerFilterLiveBytes=8 MiB",
    "旧要求を正確に1回再投入",
    "設定の同異にかかわらず旧tuneを停止し旧世代を終端",
    "完全な`RuntimeCapabilityVector`を順に検証",
]:
    if stale in hal:
        raise RuntimeError(f"stale HAL text remains: {stale}")

write(hal_path, hal)

conv_path = "tuner_hal/CODE_CONVENTION.md"
conv = read(conv_path)
conv = replace_once(
    conv,
    "- AV共有メモリ が必要な AV経路 は、失敗時に該当 操作 だけ エラーにする\n- DVR再生 / 録画 を 対応宣言する場合は、ワーカー 失敗 / queue overflow を 状態 として返す",
    "- AV共有メモリ が必要な AV経路 は、失敗時に該当 操作 だけ エラーにする\n- AVの1event上限、filter別未解放総量、runtime総量を分離し、codec・allocator・実機証跡からProductProfileごとに導出する。全codec共通の固定byte値を能力契約にしない\n- capabilityは実際に同時予約が必要な依存閉包ごとに原子的に確定し、無関係な閉包の予約失敗を波及させない。最終snapshotの横断不変条件は合成後に一括検証する\n- DVR再生 / 録画 を 対応宣言する場合は、ワーカー 失敗 / queue overflow を 状態 として返す",
    "capability convention",
)
write(conv_path, conv)

tis_path = "tis/DESIGN_JA.md"
tis = read(tis_path)
tis = replace_between(
    tis,
    "TISはdecoder構成完了後かつAV filter開始前に変更不能な`TisPlaybackBudgetSnapshot`を作る。",
    "A/V同期方式は",
    """TISはdecoder構成完了後かつAV filter開始前に変更不能な`TisPlaybackBudgetSnapshot`を作る。snapshotはcodec、decoder実装、device profileごとに、`singleEventLimitBytes`、`pendingQueueBudgetBytes`、`pendingQueueMaxSamples`、`pendingQueueMaxDurationUs`、`decoderStartupDeadlineMs`、`steadyBackpressureDeadlineMs`を持つ。これらを全codec共通の8 MiB、4 sample、1000 msへ固定しない。\n\n`singleEventLimitBytes`は、対象codecの最大access unit・header収集条件、設定した`KEY_MAX_INPUT_SIZE`、実際に取得したdecoder input bufferまたはblock capacity、allocator上限、対象機器での最悪値測定から導出する。`pendingQueueBudgetBytes`、sample数、再生時間上限は、codecのheader収集、reorder depth、decoder起動中の入力保持、通常再生中の一時dequeue停止を含む最悪値に検証済み余裕を加えて導出する。sample数だけでqueue満杯を決めず、byte数とPTSから求める保留再生時間も同時に強制する。正の有限値を確定できないdecoder/profileは開始しない。\n\n必要なqueue領域とclaim台帳はplayback generation開始時に原子的に予約する。各eventはrange検証後、copy、map保持またはdecoder投入前に`dataLength`をsnapshot台帳へclaimし、いずれかのevent、byte、sample、duration上限を超える場合は原因別に`SAMPLE_TOO_LARGE`または`PENDING_QUEUE_FULL`を記録してHAL handleを解放する。claim済みbyte、sample、durationはdequeue、generation変更、stop、releaseで正確に返す。HALの`avPerFilterLiveBytes`または`avRuntimeBudgetBytes`をTISへ公開・複製・1event上限化しない。\n\nfirst frame前はcodec-specificな`decoderStartupDeadlineMs`を用い、必要なsequence header、SPS/PPS、audio config、reorder用入力を収集している間の一時queue増加を通常backpressure失敗へ写像しない。startup deadlineまでにdecoder入力可能状態またはfirst frameへ到達できず、queueのbyteまたはduration上限も解消しない場合だけplaybackを停止して`notifyVideoUnavailable()`へ進む。first frame後は別の`steadyBackpressureDeadlineMs`を用い、単発超過は当該sampleを解放して継続し、期限中にdequeue進行がなくqueue上限が継続する場合だけunavailableへ遷移する。audioだけの超過はvideo-only継続可否を既存規則で判定し、無条件にvideo unavailableへ写像しない。\n\n""",
    "TIS playback budget",
)
for stale in ["現行productのrequested input上限は8 MiB", "pendingQueueMaxSamples=4", "playbackBackpressureDeadlineMs=1000"]:
    if stale in tis:
        raise RuntimeError(f"stale TIS text remains: {stale}")
for value in ["pendingQueueMaxDurationUs", "decoderStartupDeadlineMs", "steadyBackpressureDeadlineMs"]:
    if value not in tis:
        raise RuntimeError(f"missing TIS requirement: {value}")
write(tis_path, tis)

for path in [hal_path, conv_path, tis_path]:
    text = read(path)
    if text.count("```") % 2 != 0:
        raise RuntimeError(f"unbalanced code fences: {path}")

print(hal_path)
print(conv_path)
print(tis_path)

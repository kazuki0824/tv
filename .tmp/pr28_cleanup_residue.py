from pathlib import Path
import re

p = Path('tuner_hal/DESIGN_JA.md')
s = p.read_text()

def must_sub(pattern, repl, label, flags=re.M|re.S):
    global s
    s2, n = re.subn(pattern, repl, s, flags=flags)
    if n == 0:
        raise SystemExit(f'missing {label}')
    s = s2

# Fix the PES capability row corrupted by the broad wildcard rewrite.
s = re.sub(
    r'^\| 2 \| PES \|.*$',
    '| 2 | PES | 有効なPES設定を一般に受理 | 明示`streamId=0..255`のPES能力を宣言する | `openFilter()`は成功。`configure()`は`streamId=0..255`を受理し、`0xFFFF` (`INVALID_STREAM_ID`) および256..65535は`INVALID_ARGUMENT` | 表1のFMQ対象状態に従う | 通常FMQ + `DemuxFilterPesEvent` | TISのARIB字幕経路は利用設定として`0xBD`を指定する。video/audio本体はAV filter経路を使用してよい |',
    s, flags=re.M)

pes = '''### PES stream IDと宣言長の境界

PES filterの`streamId`として受理する値は0..255だけである。`0xFFFF`はAOSP `INVALID_STREAM_ID`でありwildcardではないため`INVALID_ARGUMENT`、256..65535も`INVALID_ARGUMENT`として状態を変更しない。字幕利用側が`0xBD`を指定することはHAL能力の限定ではない。

先頭6 byteを検証後、宣言長ありPESではassemblerが`PES_packet_length + 6` byteだけをservice共通台帳からclaimする。映像`stream_id 0xE0..0xEF`の`PES_packet_length == 0`は、同一PIDの次PUSIまでを収集し、`MAX_PES_BUFFER_BYTES`を上限とする。起動前に`ProductProfile`を検証して`CapabilitySnapshot.pesRuntimeBudgetBytes`へ固定し、その値を`MAX_PES_BUFFER_BYTES * CapabilitySnapshot.pesFilterCount`以上とする。各filterは同時に1 assemblerだけを所有するため、公開個数上限まで最大保持量を同時に保証できる。

`PES_packet_length == 0`を許す映像`stream_id 0xE0..0xEF`は、同一PIDの次PUSIを完成境界とし、`MAX_PES_BUFFER_BYTES`を超えた時点で当該PESをoversizeとして破棄して次PUSIから再同期する。全filterの最大保持量は`pesRuntimeBudgetBytes`で予約し、filter間は各1 assemblerの固定上限で公平性を確保する。`flush()`、`stop()`、`close()`では未完PESを完成扱いせず、対応するper-filter PES parser ownerがstateを破棄してclaimを返す。stream/source boundaryからは`StreamBoundaryTxn`のtyped reset dispatchを受ける。

'''
must_sub(r'^### PES stream IDと宣言長の境界\n.*?(?=^## 失敗時状態・境界処理の設計固定)', pes, 'PES section')

# Replace the old TableInfo first-instance narrowing with the public predicate contract.
tableinfo = '''#### TableInfo / SectionBits repeat=false one-shot契約

`SectionBits`の`repeat=false`は公開predicateに最初にmatchしたsectionを1件だけ配送して停止する。

`TableInfo`の公開match predicateはTS filter settingsのPID、`tableId`、`version`だけである。`version=-1`はversion wildcardであり、`table_id_extension`、`current_next_indicator`その他の内部識別子をhidden eligibility filterとして使ってはならない。内部`TableInstanceKey`は異なるsection-number空間を混ぜないtracker分離のためだけに使う。

one-shot完了前に観測した公開match済みinstanceはactive setへ加入し、instanceごとに`section_number=0..last_section_number`を独立追跡する。現在active setに属する全matching instanceが完成した時点でone-shotを終了する。完了前に新しいmatching instanceを観測した場合はactive setへ追加する。private timer、table allowlist、最初のextension/current_nextだけをtargetとして固定する規則は設けない。short sectionは1-section instanceとして扱う。

tracker、delivery bitmap、section/PES/record-index parser stateは対応するper-filter parser ownerが所有し、`FilterProducerDrainGate.parser_state_generation`でfenceする。stream/source/filter boundaryでは`StreamBoundaryTxn`からtyped reset / invalidate dispatchを受け、境界前後のsectionを結合または配送しない。

'''
must_sub(r'- セクションフィルターの`?repeat=false`?.*?(?=^### PES assembler)', tableinfo + '\n', 'TableInfo legacy block')

# Replace the raw Section/PES event section wholesale.
raw = '''#### raw section / raw PES event 生成契約

Section/PES処理は外形抽出、設定されたCRC検査、typed event生成に必要な構文検証を独立段階として扱う。外形不完全、宣言長不成立、設定上限超過、境界不明は配送しない。

| filter設定 | FMQ payload | FMQ commit後の必須callback | `onFilterEvent()` |
|---|---|---|---|
| Section `raw=true` | 完全なsection bytes | `IFilterCallback.onFilterStatus(DATA_READY)` | `DemuxFilterSectionEvent`を生成しない |
| PES `raw=true` | 完全なPES bytes | `IFilterCallback.onFilterStatus(DATA_READY)` | `DemuxFilterPesEvent`を生成しない |
| Section `raw=false` | event契約に対応するsection data | event配送規則に従う | `DemuxFilterSectionEvent` |
| PES `raw=false` | event契約に対応するPES data | event配送規則に従う | `DemuxFilterPesEvent` |

raw=trueではFMQ payloadをcommitした後に`DATA_READY` status callbackを配送する。EventFlagはFMQ consumerを追加で起床させる同期手段であり、`onFilterStatus(DATA_READY)`の代替ではない。raw/nonraw切替前のcallback/eventを切替後として配送しないfenceは`FilterProducerDrainGate.filter_delivery_generation`を使う。

nonraw Section eventの`tableId`は実sectionのtable_id、long sectionでは実際の5-bit `version`と`section_number`、short sectionでは`version=0` / `sectionNum=0`とする。`dataLength`は対応する完全section byte数と一致させる。nonraw PES eventの`streamId`はparseした実stream_id 0..255、`dataLength`は対応する完全PES byte数、TS-only productの`mpuSequenceNumber`は0とする。推測値でmetadataを生成しない。

'''
must_sub(r'^#### raw section / raw PES event 生成契約\n.*?(?=^### )', raw, 'raw section/PES contract')

# Remove the later duplicate raw-section paragraph that still allowed EventFlag as a substitute.
s = re.sub(r'^raw sectionは、外形、設定されたCRC検査、意味検証を分ける契約に従う。.*?(?=\n\n)', '', s, flags=re.M|re.S)

# Add startId contract before FilterDelayHint if it is still absent.
if '### `DemuxFilterEvent.startId` 契約' not in s:
    startid = '''### `DemuxFilterEvent.startId` 契約

settingsを変更する有効な`configure()`は、commit後の`filter_delivery_generation`に対応するpending startIdをprepareする。同じsettingsの冪等`configure()`では新しいstartIdを発行しない。Filterを再startした後、最初のevent callbackはstartIdだけを含むcallbackとして正確に1回配送し、その後に通常eventを配送する。startId-only callbackに別eventを同梱しない。新規open Filterの最初のstartだけはAOSP予約値0を使用してよく、それ以外は再利用しないpositive idを使用する。stale `filter_delivery_generation`のpending startIdは配送しない。positive idを再利用なしに発行できない場合は既存`filter_delivery_generation` exhaustionの局所failure契約へ従い、新しい独立generation軸を追加しない。

'''
    if '### FilterDelayHint 契約' not in s:
        raise SystemExit('FilterDelayHint anchor missing')
    s = s.replace('### FilterDelayHint 契約', startid + '### FilterDelayHint 契約', 1)

# Fix DVR status row itself, not only the prose below it.
s = re.sub(
    r'^\| DVR-033 \| `setStatusCheckIntervalHint\(\)` 正常入力 \|.*$',
    '| DVR-033 | `setStatusCheckIntervalHint()` 正常入力 | D0R, D0P, D1, D2, D3, D4, D5, D6 | 成功 | 入力状態を維持 | positive msを後続data-status評価のtarget intervalとして適用し、0はproduct defaultへ戻す | `dvr_status_hint_set` | queue/lifecycleを変えず評価cadenceだけを更新 |',
    s, flags=re.M)

# Normalize generation/steady-state ownership across overflow table, PacketPipeline and record-index text.
s = re.sub(r'^\| section / PES / record-index parser/assembler generation \|.*\n', '', s, flags=re.M)
s = re.sub(
    r'^\| source filter origin / stream generation \|.*$',
    '| source relation generation | `checked_add(1)` | `SourceBoundaryTxn` | wrap / relation generation再利用 |',
    s, flags=re.M)
s = re.sub(
    r'`PacketPipeline` は、TS packet validation、source origin分類、record index input、およびcanonical ownerが確定したgeneration / continuity snapshotを参照するdata-path dispatchを正本として持つ。PID continuity / discontinuity、section / PES / record-index parser/assembler generationは0-S-3Bの`StreamBoundaryTxn`、`filter_delivery_generation` / `parser_state_generation`は`FilterProducerDrainGate`を唯一のmutation ownerとし、`PacketPipeline`自身がこれらを直接更新しない。',
    '`PacketPipeline`はTS packet validation、source origin分類、record index input、および入力origin/PIDごとのsteady-state continuity tableを所有する。section/PES/TableInfo/record-indexのassembler/tracker stateは対応するper-filter parser ownerが所有し、Filter側のparser fenceは`FilterProducerDrainGate.parser_state_generation`を使う。`StreamBoundaryTxn`は`stream_boundary_generation`と各ownerへのtyped reset / invalidate dispatchだけを所有し、steady-state continuity/parser stateを直接変更しない。',
    s)
s = s.replace(
    '`flush()`、`setDataSource()`、filter close、source unlink、stream boundaryに伴うparser / assembler stateとgenerationのmutationは0-S-3Bの`StreamBoundaryTxn`を唯一の正本とし、本節では再定義しない。',
    '`flush()`、`setDataSource()`、filter close、source unlink、stream boundaryでは`StreamBoundaryTxn`がtyped reset / invalidateをdispatchし、steady-state parser / assembler stateは対応するper-filter parser owner、Filter parser fenceは`FilterProducerDrainGate.parser_state_generation`が変更する。')
s = s.replace(
    'record-index parser carry / parser・assembler generationのmutationは`StreamBoundaryTxn`、event配送fenceは`FilterProducerDrainGate.filter_delivery_generation`だけを正本とし、独立した`record output generation`軸は設けない。',
    'record-index parser carryはrecord-index parser ownerが所有し、parser boundaryは`FilterProducerDrainGate.parser_state_generation`でfenceする。stream/source boundaryからのreset要求だけを`StreamBoundaryTxn`のtyped dispatchとして受ける。event配送fenceは`FilterProducerDrainGate.filter_delivery_generation`を使い、独立した`record output generation`軸は設けない。')
s = s.replace(
    '`DemuxFilterTsRecordEvent`は、`StreamBoundaryTxn`が確定したcurrent record-index parser/assembler generationと`FilterProducerDrainGate`のcurrent delivery generationに属し、',
    '`DemuxFilterTsRecordEvent`は、`FilterProducerDrainGate`のcurrent `parser_state_generation`とcurrent `filter_delivery_generation`に属し、')

# Fix resource lifetime rows to keep all eight columns.
s = re.sub(
    r'^\| RL-002 \|.*$',
    '| RL-002 | `frontend_operation_generation` | `FrontendRuntime`。発行・fenceはcanonical `frontend tune/scan` | tune/scan operation開始時 | operation終端 / replacement時にfence | fence結果を確定できない場合 | 当該generationを再利用せずcanonical frontend failure契約へ接続 | backend/worker/callback用に並行するfrontend generation軸を新設しない |',
    s, flags=re.M)
s = re.sub(
    r'^\| RL-003 \|.*$',
    '| RL-003 | `stream_boundary_generation` | `StreamBoundaryTxn` | stream boundary prepare時 | boundary replacement / object cleanup時に失効 | 発行・fence結果を確定できない場合 | 当該generationを再利用せず`StreamBoundaryTxn`の局所failure契約へ接続 | steady-state continuity/parser generationをこの資源へ吸収しない |',
    s, flags=re.M)

p.write_text(s)

# Final hard checks.
final = p.read_text()
for bad in [
    '明示`streamId 0..255`とwildcard',
    'wildcard `0xFFFF`のPES能力',
    '`DATA_READY`またはEventFlag',
    'first-instance解決',
    '1個の`TableInstanceKey`をone-shot target',
    'section / PES / record-index parser/assembler generation | `checked_add(1)` | `StreamBoundaryTxn`',
    'record-index parser carry / parser・assembler generationのmutationは`StreamBoundaryTxn`',
    'hint 値だけ保存',
]:
    if bad in final:
        raise SystemExit('residue: ' + bad)
for good in [
    '### `DemuxFilterEvent.startId` 契約',
    '#### TableInfo / SectionBits repeat=false one-shot契約',
    '`IFilterCallback.onFilterStatus(DATA_READY)`',
    '`streamId`として受理する値は0..255だけ',
    'current `parser_state_generation`',
]:
    if good not in final:
        raise SystemExit('missing: ' + good)

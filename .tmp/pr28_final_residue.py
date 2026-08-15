from pathlib import Path
import re

p = Path('tuner_hal/DESIGN_JA.md')
s = p.read_text()

# startId: use the existing Filter delivery generation; do not create a parallel generation axis.
if '### `DemuxFilterEvent.startId` 契約' not in s:
    anchor = '### FilterDelayHint 契約'
    if anchor not in s:
        raise SystemExit('FilterDelayHint anchor missing')
    block = '''### `DemuxFilterEvent.startId` 契約

settingsを変更する有効な`configure()`は、commit後の`filter_delivery_generation`に対応するpending startIdをprepareする。同じsettingsの冪等`configure()`では新しいstartIdを発行しない。Filterを再startした後、最初のevent callbackはstartIdだけを含むcallbackとして正確に1回配送し、その後に通常eventを配送する。startId-only callbackに別eventを同梱しない。新規open Filterの最初のstartだけはAOSP予約値0を使用してよく、それ以外は再利用しないpositive idを使用する。stale `filter_delivery_generation`のpending startIdは配送しない。positive idを再利用なしに発行できない場合は既存`filter_delivery_generation` exhaustionの局所failure契約へ従い、新しい独立generation軸を追加しない。

'''
    s = s.replace(anchor, block + anchor, 1)

# raw Section/PES: DATA_READY callback is mandatory after FMQ commit; EventFlag is only an additional wake.
if '#### raw section / raw PES event 生成契約' not in s:
    anchor = '### PES assembler の異常系状態表'
    if anchor not in s:
        raise SystemExit('PES assembler anchor missing')
    block = '''#### raw section / raw PES event 生成契約

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
    s = s.replace(anchor, block + anchor, 1)

s = s.replace(
    '| T-SEC-5a | `isCheckCrc=false` + CRC bad + 構文正常 | CRCを配送条件にせず、rawはFMQ配送と`DATA_READY`またはEventFlag通知、non-rawは型付きevent規則に従う |',
    '| T-SEC-5a | `isCheckCrc=false` + CRC bad + 構文正常 | CRCを配送条件にせず、rawはFMQ commit後に`IFilterCallback.onFilterStatus(DATA_READY)`を配送する。EventFlagは追加wakeだけに使い、non-rawは型付きevent規則に従う |')

# PES configuration: 0xFFFF is INVALID_STREAM_ID, never a wildcard or a valid explicit value.
s = s.replace(
    'PES filterは、外形検証の後に`stream_id`で通常optional-header構文とspecial syntaxを分岐する。明示`streamId 0..255`または`0xFFFF`は`INVALID_STREAM_ID`として拒否する値 `0xFFFF`の有効な設定を受理し、ヘッダーが複数TSパケットに分割される場合にも対応する。',
    'PES filterは、外形検証の後に`stream_id`で通常optional-header構文とspecial syntaxを分岐する。設定で受理する明示`streamId`は0..255だけとし、`0xFFFF` (`INVALID_STREAM_ID`) と256..65535は`INVALID_ARGUMENT`で拒否する。受信するPES packetの`stream_id`は8 bit値として構文分岐し、ヘッダーが複数TSパケットに分割される場合にも対応する。')
s = s.replace(
    '| FILTER_PES | サービス全体 | 4 | `CapabilitySnapshot`の値 | 0 | demux当たり1 | 有効な明示`streamId 0..255`と`0xFFFF`は`INVALID_STREAM_ID`として拒否する値 `0xFFFF`を同じPES capabilityで扱う。宣言長ありPESは宣言長+6 byteをPES実行時台帳からclaimし、映像`0xE0..0xEF`の長さ0 PESは`MAX_PES_BUFFER_BYTES`と同台帳の上限内で組み立てる。stream ID別の非公開capabilityを設けない。 |',
    '| FILTER_PES | サービス全体 | 4 | `CapabilitySnapshot`の値 | 0 | demux当たり1 | 有効な明示`streamId 0..255`を同じPES capabilityで扱う。`0xFFFF` (`INVALID_STREAM_ID`) と256..65535は`INVALID_ARGUMENT`で拒否する。宣言長ありPESは宣言長+6 byteをPES実行時台帳からclaimし、映像`0xE0..0xEF`の長さ0 PESは`MAX_PES_BUFFER_BYTES`と同台帳の上限内で組み立てる。stream ID別の非公開capabilityを設けない。 |')

# Steady-state continuity/parser ownership. StreamBoundaryTxn only dispatches typed boundary resets.
repls = {
    '境界前後のsection / PES等を連結しない。assembler / generationのmutationは`StreamBoundaryTxn`参照':
        '境界前後のsection / PES等を連結しない。steady-state continuityは`PacketPipeline`、parser/assembler stateは各per-filter parser owner、Filter parser fenceは`FilterProducerDrainGate.parser_state_generation`を正本とし、`StreamBoundaryTxn`はtyped reset / invalidate dispatchだけを行う',
    'continuity / parser / assemblerの内部mutationは`StreamBoundaryTxn`参照とする。':
        'steady-state continuityは`PacketPipeline`、parser / assembler stateは各per-filter parser owner、Filter parser fenceは`FilterProducerDrainGate.parser_state_generation`を正本とする。stream/source boundaryでは`StreamBoundaryTxn`からtyped reset / invalidate dispatchだけを受ける。',
    'continuity / assemblerのmutationは`StreamBoundaryTxn`参照とする。':
        'steady-state continuityは`PacketPipeline`、parser / assembler stateは各per-filter parser ownerを正本とし、境界時は`StreamBoundaryTxn`のtyped reset / invalidate dispatchを受ける。',
    'continuity / assembler / partial-stateのmutationは`StreamBoundaryTxn`参照とする。':
        'steady-state continuityは`PacketPipeline`、parser / assembler / partial stateは各per-filter parser ownerを正本とし、境界時は`StreamBoundaryTxn`のtyped reset / invalidate dispatchを受ける。',
    'raw/recordへ保持し、境界前の意味解析結果を後続へ連結しない。内部boundary mutationは`StreamBoundaryTxn`参照':
        'raw/recordへ保持し、境界前の意味解析結果を後続へ連結しない。continuityは`PacketPipeline`、parser stateは各per-filter parser ownerを更新し、`StreamBoundaryTxn`はtyped reset dispatchだけを行う',
    '境界前後の意味解析結果を連結しない。continuity / assembler mutationは`StreamBoundaryTxn`参照':
        '境界前後の意味解析結果を連結しない。continuityは`PacketPipeline`、parser stateは各per-filter parser ownerを更新し、`StreamBoundaryTxn`はtyped reset dispatchだけを行う',
}
for old, new in repls.items():
    s = s.replace(old, new)

# Remove duplicate Frontend close sentence.
dup = '`IFrontend.close()` は、scan / tune worker、live pump、frontend backend、callback registration、demux relation、frontend leaseを当該frontend固有のcleanup対象とする。`IFrontend.close()` は、scan / tune worker、live pump、frontend backend、callback registration、demux relation、frontend leaseを当該frontend固有のcleanup対象とする。'
s = s.replace(dup, '`IFrontend.close()` は、scan / tune worker、live pump、frontend backend、callback registration、demux relation、frontend leaseを当該frontend固有のcleanup対象とする。')

# Public DESIGN keeps the single-open product invariant; concrete Rust/fd/poll mechanism belongs to CODE_CONVENTION.
s = s.replace(
    'px4 backend は control fd を一度だけ open し、ライブ TS reader はその `File` を `try_clone()` / fd duplicate 相当で複製して使う。TS pump は nonblocking fd と `poll()` の組み合わせで動かし、reader 作成のために同じ chardev path を再 open しない。これにより、px4_drv の single-open 制約下でも tune 後に ライブ TS、section、AV、record/DVR経路 へ packet を流せることを保証する。',
    'px4 backendは同一device nodeを二重openせず、1回のbackend openからcontrol経路とlive TS readerを派生させる。二重open回避の具体的Rust API、fd複製、nonblocking / poll方式は`tuner_hal/CODE_CONVENTION.md`を正本とする。single-open制約下でもtune後にlive TS、section、AV、record/DVR経路へpacketを流せることを公開設計上の不変条件とする。')

# Packet-origin state is owned by the steady-state owners, not by StreamBoundaryTxn.
s = s.replace(
    'source filter 由来の TS packet は frontend 由来の TS packet と同じ packet pipeline を通る。ただし origin namespace は frontend と source filter で分離し、assembler generation、carry state、flush state を相互に消してはならない。',
    'source filter由来のTS packetはfrontend由来のTS packetと同じpacket pipelineを通る。ただしorigin namespaceはfrontendとsource filterで分離し、`PacketPipeline`のper-origin continuity stateと各per-filter parser ownerのcarry/tracker stateを別origin間で共有・相互resetしない。stream/source boundaryのreset要求だけを`StreamBoundaryTxn`のtyped dispatchとして受ける。')

# TableInfo active-set tracking must not retain the old one-target fixed allocation model.
s = re.sub(
    r'^\| FILTER_SECTION \|.*$',
    '| FILTER_SECTION | サービス全体 | 8 | `CapabilitySnapshot`の値 | 0 | なし | FMQ容量に加え、TableInfo `repeat=false`では公開predicateへmatchしたactive instanceごとのmetadataとsection bitmapをSECTION closureのparser/tracker runtime byte budgetからclaimする。内部extension/current_nextでmatching instanceを除外しない。runtime budget不足では既存trackerを壊さず`OVERFLOW` statusとtyped diagnosticを通知する。 |',
    s, flags=re.M)
s = re.sub(
    r'^\| filter main type / FMQ \|.*$',
    '| filter main type / FMQ | main type別object数、FMQ byte、callback、assembler、配送worker。SECTIONではTableInfo/SectionBits parser・tracker用runtime byte budgetを含む | demux base、共有worker基盤 | 当該main typeだけを非公開 |',
    s, flags=re.M)

# Tests must reflect the cumulative Record Filter output lifetime byteNumber contract.
s = re.sub(
    r'^\| T-AOSP-49 \|.*$',
    '| T-AOSP-49 | RECORD index settings/event | request mask/typeを無損失検証し、event mask、`pts`、`firstMbInSlice`をcurrent parser/delivery fenceに一致させる。`byteNumber`はFilter output lifetime先頭からRecord DVRへcommit済みの累積`record_output_byte_offset`とし、flush/reconfigure/source/stream boundaryで0へ戻さない |',
    s, flags=re.M)

# Evidence wording: distinguish the English evidence revision from the current Japanese revision.
s = s.replace(
    'ISDB-Tの列挙値域は、ARIB公式英語版STD-B31 2.2-E1本文の2.3、3.8、3.9、3.11.1、3.14.2、3.15.6.5〜3.15.6.7に従う。',
    'ISDB-Tの列挙値域は、今回精読したARIB公式英訳STD-B31 2.2-E1本文の2.3、3.8、3.9、3.11.1、3.14.2、3.15.6.5〜3.15.6.7を証拠本文とする。現行日本語版2.3との差分は「ARIB規範本文との静的照合」の`差分未証明`管理に従う。')
s = s.replace(
    'ISDB-T設定値の規格上の妥当性は、ARIB公式英語版STD-B31 2.2-E1本文の2.3、3.8、3.9、3.11.1、3.14.2、3.15.6.5〜3.15.6.7に従う。',
    'ISDB-T設定値の規格上の妥当性は、今回精読したARIB公式英訳STD-B31 2.2-E1本文の2.3、3.8、3.9、3.11.1、3.14.2、3.15.6.5〜3.15.6.7を証拠本文とし、現行日本語版2.3との差分未証明を別管理する。')

p.write_text(s)

final = p.read_text()
bad = [
    '`DATA_READY`またはEventFlag',
    'first-instance',
    '1個の`TableInstanceKey`をone-shot target',
    '`0xFFFF`は`INVALID_STREAM_ID`として拒否する値',
    'assembler / generationのmutationは`StreamBoundaryTxn`',
    'continuity / parser / assemblerの内部mutationは`StreamBoundaryTxn`',
    'continuity / assemblerのmutationは`StreamBoundaryTxn`',
    'continuity / assembler / partial-stateのmutationは`StreamBoundaryTxn`',
    '`File` を `try_clone()`',
    '`IFrontend.close()` は、scan / tune worker、live pump、frontend backend、callback registration、demux relation、frontend leaseを当該frontend固有のcleanup対象とする。`IFrontend.close()`',
]
for x in bad:
    if x in final:
        raise SystemExit('residue remains: ' + x)
required = [
    '### `DemuxFilterEvent.startId` 契約',
    '#### raw section / raw PES event 生成契約',
    'IFilterCallback.onFilterStatus(DATA_READY)',
    '#### TableInfo / SectionBits repeat=false one-shot契約',
    '### TS PID 共通値域契約',
    '### SectionBits bit照合契約',
    '### TS live `MediaEvent` field契約',
    'record_output_byte_offset',
    'stream_boundary_generation',
    'frontend_operation_generation',
    '差分未証明',
]
for x in required:
    if x not in final:
        raise SystemExit('required contract missing: ' + x)

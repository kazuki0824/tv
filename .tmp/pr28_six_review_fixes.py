from pathlib import Path
import re

p = Path('tuner_hal/DESIGN_JA.md')
s = p.read_text()


def replace_line(prefix: str, new: str) -> None:
    global s
    lines = s.splitlines()
    hits = [i for i, line in enumerate(lines) if line.startswith(prefix)]
    assert len(hits) == 1, (prefix, len(hits))
    lines[hits[0]] = new
    s = '\n'.join(lines) + ('\n' if s.endswith('\n') else '')


def sub_once(pattern: str, repl: str, flags: int = 0) -> None:
    global s
    s2, n = re.subn(pattern, repl, s, count=1, flags=flags)
    assert n == 1, (pattern, n)
    s = s2


replace_line('| `StreamBoundaryTxn` |', '| `StreamBoundaryTxn` | `tuner_hal2/DESIGN_JA.md` の同名論理契約行 | typed stream-boundary use-case、上位transactionからの`prepare()` | stream generation、continuity、section/PES/record-index parser/assembler boundary、prepared invalidation dispatch | relation table、Filter/DVR queue内部、A/V sync/PCR内部、callback、descrambler | validate → owned generationの次値を`checked_add()`でprepare → prepare `PreparedStreamBoundary` → commit / abort | abortでは旧generation維持。owned generationを発行できない場合はwrap / saturating reuseせず、当該stream/filter boundaryを`Quarantined`として新boundaryを開始しない。commit不明時だけ対象streamをfail/quarantine | service_runtime packet/boundary use-case、上位relation transaction | API/worker/helperのparser/generation直接変更 | standalone commit、prepared abort/commit、stale generation、generation exhaustionで非再利用・局所quarantine、relation composite atomicity |')
replace_line('| `WorkerRuntime` / `WorkerHandle` |', '| `WorkerRuntime` / `WorkerHandle` | `tuner_hal2/DESIGN_JA.md` の同名論理契約行 | 各domain worker ownerのspawn/stop/wake/join/reaper | owner generation / signal generation、stop signal、JoinHandle、fence、reaper handoff mechanism、有界`ReaperSupervisor` work queue、retry schedule / coalesce state、typed worker terminal result（`Normal` / `StopRequested` / `RuntimeFailure` / `PanicOrJoinFailure`） | domain start/stop state、backend semantic failure、queue payload | owner handle slot prepare → 次generationを`checked_add()`でprepare → fenced worker spawn / handle bind → signal stop → wake/cancel → observe/join または one-shot reaper handoff → handoff済みworkを既存retry scheduleで外部API再呼出しなしに自律再試行 → worker実終了と依存cleanup完了後にlease/slot release | handle slot準備失敗ではspawnしない。owner/signal generationを発行できない場合はwrap / saturating reuseや存在しない次generationでのreplacement spawnを行わず、現generationをfenceして停止・回収し、影響するowner/generation/resourceだけを`Quarantined`とする。取消generationごとのstop/wake通知は各1回までとし、終了済みworkerは直ちに回収する。failureはtyped reportしleaseを早期再利用しない。`ReaperSupervisor`へのenqueueおよび早期再開要求は `(owner, generation, dependency resource)` ごとにcoalesceし、同一未完workを重複実行しない。外部APIの再呼出しがなくても有界work queueがretry scheduleに従って進行する。terminal budgetは`cleanupRetryScheduleMs=[0,10,100,1000]`後1000 ms間隔、`cleanupTerminalDeadlineMs=30000`、`workerIoDeadlineMs=2000`、`workerReaperDeadlineMs=10000`。deadline到達後もowner generation無効化で副作用を遮断できる場合は対象owner/generation/resourceだけを`Quarantined`とする。無効化後もservice-global stateを変更可能、遮断不能なservice-wide exclusive resourceを保持、owner/generation/resource tokenで遮断不能、または同一資源のreplacement/restartと競合可能というtyped evidenceがある場合だけ`ServiceCritical`とする | domain worker owner、cleanup/reaper | generic `WorkerLifecycleProtocol`の追加、AIDLからの直接join | handle-slot failureでno-spawn、generation exhaustionでwrap/reuse/replacementなし、typed terminal result、stop/wake一回性、generation fence、join/one-shot reaper、外部API再呼出しなしの自律retry、早期再開要求のcoalesce、panic、no early reuse、deadline branch、local quarantine対ServiceCritical判定 |')
replace_line('| `FilterProducerDrainGate` |', '| `FilterProducerDrainGate` | `tuner_hal2/DESIGN_JA.md` の同名論理契約行 | Filter/SharedFilter producer、`QueueCleanupTxn`からのtyped drain request | `Open`/`Draining`/`Closed`、`filter_delivery_generation`、`parser_state_generation`、`admitted_producer_count`、bounded pending event queue、`FilterProducerPermit(g)` | FMQ内容、DVR token/epoch、flush全体のorchestration | `Open`でadmit/permit発行 → producer commit/finishでpermit解放 → drain開始を`Draining`へ線形化し新規admit拒否 → admitted producerとpending eventを排出 → 次generationを`checked_add()`でprepareしてgeneration/parser stateを確定し`Open`へ戻す、またはcloseで`Closed` | panic/returnでもpermit解放。generationを発行できない場合はwrap / saturating reuseせず対象Filterを`Quarantined`として`Open`へ戻さない。drain中は旧generationのproducer/eventを新generationへ確定せず、その他の遮断不能failureだけFilter fail。`QueueCleanupTxn`はtyped入口の結果だけを集約 | data producer、`QueueCleanupTxn` | Binder callback/IO/joinをpermit内に保持、`QueueCleanupTxn`/API/workerがgate内部stateを直接変更、DVR stateの吸収 | Open/Draining/Closed遷移、flush中の新規permit拒否、全permit/pending event排出、generation/parser更新、generation exhaustionで非再利用・quarantine、panic/drop、commit前失敗時の旧state維持、共通orchestratorからのdrain |')
replace_line('| `LnbControlTxn` |', '| `LnbControlTxn` | `tuner_hal2/DESIGN_JA.md` の同名論理契約行 | `setVoltage()` / `setTone()` / `setSatellitePosition()` | operation lock、candidate、backend apply結果、LnbRegistry commit、failure state、LNB state generation | DiSEqC transient send、callback、endpoint lease | validate → lock → old snapshot → 次generationを`checked_add()`でcandidateへprepare → backend apply → registry commit | generationを発行できない場合はbackend apply前に拒否し、wrap / saturating reuseせず対象LNBを`Quarantined`とする。`Rejected`はregistry不変。backend反映成功後のregistry commit失敗ではbackend rollback applyを行わずLNBを失敗状態とし、当該操作および以後の公開control APIを`UNKNOWN_ERROR`とする。backend反映結果自体が不明な場合はLNBをfail/quarantine。backend / registry failureでは要求状態、backend apply結果、最後に確認できた機器状態、registry errorをtyped diagnosticとして保持する。成功時だけgeneration更新 | LNB object use-case | 3 APIの個別state machine、DiSEqCの吸収 | 3操作、invalid/unavailable、generation exhaustion、backend rejected/indeterminate、registry failure、close race |')
replace_line('| `PcrClockAnchorStore` |', '| `PcrClockAnchorStore` | `tuner_hal2/DESIGN_JA.md` の同名論理契約行 | PCR観測、stream/filter boundaryからprepared invalidation | generation-scoped `PcrClockAnchor { raw_pcr_base_33, unwrapped_pcr_90k, monotonic_base_ns, generation }` とその観測・無効化状態 | A/V sync ID relation、stream generation本体 | current generationの最初の有効PCR観測でanchorを生成 → 同一generationかつ`discontinuity_indicator`なしの後続PCRは33-bit baseを前向きにunwrapして観測monotonic時刻へanchor更新。`discontinuity_indicator`、前向きwrapとして解釈不能なPCR逆行、PCR PID/source filterの置換・再設定・`flush()`・`stop()`・`close()`、demux input generation変更、frontend retune・`stopTune()`・`close()`、playback sourceの`flush()`/resetはprepared invalidation → outer commit / abort | stale generationを拒否する。`now_monotonic_ns < monotonic_base_ns`の時計異常ではcurrent anchorを無効化する。prepared invalidationのabortでは旧anchor維持、commit後は旧generation anchorを再利用せず、新しい有効PCR観測までanchorなしとする | PCR data path、StreamBoundary/filter lifecycle | API/StreamBoundaryによる内部直接変更 | 初回PCR observe、同一generation update、33-bit wrap、時計逆行、discontinuity/PCR逆行、flush/stop/close/input-gen/retune/playback flush/reset invalidation、stale generation、prepared abort/commit |')

replace_line('| packet pipeline |', '| packet pipeline | `PacketPipeline` | `soft_demux` | packet validation・origin分類・data-path dispatchを担当し、continuity / generation stateを各canonical ownerを迂回して更新しない |')

sub_once(r'### 表14\. 寿命ID・世代ID・token 規則\n.*?\n### 表15\. backend state model', '''### 表14. 寿命ID・世代ID・token 規則

寿命ID、世代ID、token ID に `saturating_add()` を使って固定値で継続してはならない。次値は `checked_add()` 等の検査付き発行で準備し、上限到達時の状態遷移・隔離範囲・再利用可否は次表のcanonical ownerだけを正本とする。本表ではoverflow state machineを再定義しない。

| 対象 | 発行規則 | overflow契約の正本 | 禁止事項 |
|---|---|---|---|
| filter delivery / parser state generation | `checked_add(1)` | `FilterProducerDrainGate` | wrap / `saturating_add()` / generation再利用 |
| section / PES / record-index parser/assembler generation | `checked_add(1)` | `StreamBoundaryTxn` | wrap / `saturating_add()` / stale generation継続 |
| source filter origin / stream generation | `checked_add(1)` | `StreamBoundaryTxn` | wrap / origin generation再利用 |
| worker owner / signal generation | `checked_add(1)` | `WorkerRuntime` / `WorkerHandle` | wrap / 固定化 / 存在しない次generationでのreplacement spawn |
| LNB state generation | `checked_add(1)` | `LnbControlTxn` | wrap / `saturating_add()` / generation再利用 |
| AV `avDataId` | 正数だけを検査付きで発行 | 表1-C-AVH / AV割り当て契約 | 0 / 負数発行、wrap、再利用 |

`OpaqueKeyToken`、`TokenEntryId`、`ResolvedKeyMaterial`、CASの有効性は、別の型と別の存続期間で管理する。


### 表15. backend state model''', flags=re.S)

sub_once(r'### 表18\. source filter origin / downstream 状態所有契約\n.*?\n#### 表18-B\. source filter boundary 補足表', '''### 表18. source filter origin / downstream 状態所有契約

Tuner HAL は AOSP Tuner HAL の filter linkage 構造のうち、capability と本表で固定した範囲だけを受理する。

本製品の source filter linkage は、raw TS packet を下流 raw TS / record 系へ配送する範囲だけを正式対応とする。section payload / PES payload / AV payload / record payload を別filterへ直接再投入する linkage は対応しない。

AOSP `DemuxCapabilities.linkCaps` は main type 粒度であり、VTS は広告された main type pair について subtype `UNDEFINED` の filter 接続を生成し得る。そのため本製品は、実際に成功させない main type pair を `linkCaps` に広告しない。TS→TS main type linkage を広告する場合は、VTS が生成する `UNDEFINED` subtype source / sink の `setDataSource()` 接続と demux input 復帰を成功対象に含める。

source relation mutationは`SourceBoundaryTxn`、stream generation / continuity / section・PES・record-index parser/assembler boundaryは`StreamBoundaryTxn`、Filter producer drainは`FilterProducerDrainGate` / `QueueCleanupTxn`を唯一の正本とする。本表は入力origin、linkage可否、caller-visibleなdata-path結果だけを定義し、generation更新、continuity、parser/assembler mutation、relation prepare/commit/rollbackを再定義しない。

| 番号 | 事象 | 入力origin | caller-visible / data-path結果 | 内部mutation正本 |
|---:|---|---|---|---|
| SF-001 | frontend input TS | `TsInputOrigin::Frontend` | frontend入力として処理し、別origin由来dataと混成しない | `StreamBoundaryTxn` |
| SF-002 | DVR playback input TS | `TsInputOrigin::PlaybackDvr(dvr_id, queue_identity, queue_epoch)` | playback入力として処理し、別origin由来dataと混成しない | `QueueEpochProtocol` / `StreamBoundaryTxn` |
| SF-003 | source filter raw TS delivery | `TsInputOrigin::SourceFilter(filter_id, generation)` | 接続済みdownstreamだけへraw TSを配送し、未接続downstreamへは配送しない | `SourceBoundaryTxn` / `StreamBoundaryTxn` |
| SF-004 | source filter `flush()` / reconfigure | `TsInputOrigin::SourceFilter(filter_id, generation)` | 境界前の未配送data/eventを境界後のdataとして配送せず、公開relationは各API契約どおり維持する | `FilterProducerDrainGate` / `QueueCleanupTxn` / `StreamBoundaryTxn` |
| SF-006 | source filter close / unlink | `TsInputOrigin::SourceFilter(filter_id, generation)` | close/unlink後は当該source由来の新規配送を行わず、downstreamはsource lostを観測する | `ObjectCloseTxn` / `SourceBoundaryTxn` / `StreamBoundaryTxn` |

| source filter 出力 | downstream | 対応 | caller-visibleな配送結果 | 非対応時 |
|---|---|---:|---|---|
| raw TS packet | raw TS filter | 可 | 同一TS packet viewを配送する | - |
| raw TS packet | record filter | 可 | Record DVR/filter公開契約に従ってraw TSを配送する | - |
| raw TS packet | section / PES / AV filter | 不可 | raw TS再parse chainを作らない | `UNAVAILABLE` |
| section / PES / AV / record payload | 任意downstream | 不可 | payloadをsourceとして直接再配送しない | `UNAVAILABLE` |
| ペイロードなしfilter | 任意downstream | 不可 | source/sinkとして接続しない | `INVALID_ARGUMENT` |

recordのデータ経路とイベント経路は分離する。


#### 表18-B. source filter boundary 補足表''', flags=re.S)

sub_once(r'`PacketPipeline` は、次を正本として持つ。\n\n```text\nTS packet validation\nsource origin\nPID continuity\ndiscontinuity\nsection generation\nPES generation\nfilter delivery generation\nflush generation\nrecord index input\n```\n\nsource origin は次の名前空間で分離する。\n\n\| origin \| 意味 \|\n\|---\|---\|\n\| Frontend \| backend live TS \|\n\| PlaybackDvr \| playback DVR input \|\n\| SourceFilter\(filter_id, generation\) \| source filterからのraw TS再投入 \|\n\nFrontend と SourceFilter を同じ continuity / generation 名前空間に入れてはならない。', '''`PacketPipeline` は、TS packet validation、source origin分類、record index input、およびcanonical ownerが確定したgeneration / continuity snapshotを参照するdata-path dispatchを正本として持つ。PID continuity / discontinuity、section / PES / record-index parser/assembler generationは0-S-3Bの`StreamBoundaryTxn`、`filter_delivery_generation` / `parser_state_generation`は`FilterProducerDrainGate`を唯一のmutation ownerとし、`PacketPipeline`自身がこれらを直接更新しない。

source origin は次の名前空間で分離する。

| origin | 意味 |
|---|---|
| Frontend | backend live TS |
| PlaybackDvr | playback DVR input |
| SourceFilter(filter_id, generation) | source filterからのraw TS再投入 |

Frontend / PlaybackDvr / SourceFilter のoriginを混同せず、continuity / generationの内部mutationは`StreamBoundaryTxn` / `FilterProducerDrainGate`のcanonical contractに従う。''')

old = '所有者消滅では待機を伴わない後片付けを開始し、残りは回収機構へ委ねる。'
new = 'owner lossで発生するobject/domain固有cleanup対象とcaller-visibleな結果は各状態表に従う。cleanup開始authority、待機 / 非待機の実行方式、handoff / reaperは0-S-3Bの`ObjectCloseTxn`を唯一の正本とし、本節では再定義しない。'
assert s.count(old) == 1, s.count(old)
s = s.replace(old, new, 1)

sub_once(r'同一demuxに属する稼働中のPCRフィルターを示す有効なA/V同期IDについては、PCR未観測でも`getAvSyncTime\(\)`を成功させ、`Tuner\.INVALID_TIMESTAMP`を返す。.*?値0を未観測時の特別値として公開してはならない。', '''同一demuxに属する稼働中のPCRフィルターを示す有効なA/V同期IDについて、0-S-3Bの`PcrClockAnchorStore`に当該generationの有効anchorがない場合は`getAvSyncTime()`を成功させ、`Tuner.INVALID_TIMESTAMP`を返す。anchorの初回生成、後続PCRによる33-bit unwrap / 更新、discontinuity・PCR逆行・filter/source/stream/frontend/playback境界による無効化、stale generation拒否、時計逆行時の無効化は`PcrClockAnchorStore`だけを正本とし、本節ではmutationを再定義しない。

有効anchorがある場合のcaller-visibleな時刻計算は、`current_90k = (unwrapped_pcr_90k + floor((now_monotonic_ns - monotonic_base_ns) * 90000 / 1000000000)) mod 2^33`とし、PCR到着間隔中もmonotonic clockで進行させる。計算は符号なしオーバーフローを起こさない拡張精度で行う。`PcrClockAnchorStore`がanchorを無効と判定した場合は`Tuner.INVALID_TIMESTAMP`を返す。別demuxのID、PCR以外のフィルターID、閉鎖済みID、不明なIDには`INVALID_ARGUMENT`を返す。値0を未観測時の特別値として公開してはならない。''', flags=re.S)

old = 'PCRとmonotonic clockの対応付け、90 kHzへの整数変換、33-bit wrap、anchor破棄条件は直前の`PcrClockAnchor`契約を唯一の正本とする。'
new = 'PCR anchorの観測・33-bit wrap・更新・無効化条件は0-S-3Bの`PcrClockAnchorStore`を唯一のmutation正本とし、90 kHzへのcaller-visibleな整数変換は直前の`getAvSyncTime()`契約を正とする。'
assert s.count(old) == 1, s.count(old)
s = s.replace(old, new, 1)

sub_once(r'### 表20\. counter / generation overflow 契約\n.*?\n## ワーカー abnormal exit と scan terminal state の固定方針', '''### 表20. counter / generation overflow 契約

寿命ID / generationのwrap・saturating reuse禁止、次値発行、上限到達時の状態遷移・隔離範囲は表14から各0-S-3B canonical ownerへ参照する。本節ではsection/PES/source/filter/worker/LNB generationのoverflow state machineを再定義しない。特にworker generation上限で、発行不能な「新しい世代番号」のreplacement workerを生成する経路は設けない。

診断counterは `saturating_add()` を許可する。ただし、上限到達時は `diagnostic_counter_saturated` を記録し、本体データ経路を停止しない。diagnostic counter overflowをfilter / DVR / demux / frontendのruntime failureに昇格してはならず、診断counterをbusiness APIの成功/失敗判定に使ってはならない。

| 分類 | 対象 | 加算規則 | overflow時 | データ経路への波及 | 禁止事項 |
|---|---|---|---|---|---|
| 診断counter | malformed packet count | `saturating_add(1)` | saturated flag 記録 | なし | wrap、panic、データ経路停止 |
| 診断counter | drop count | `saturating_add(1)` | saturated flag 記録 | なし | wrap、panic、データ経路停止 |
| 診断counter | ioctl error count | `saturating_add(1)` | saturated flag 記録 | なし | wrap、panic、データ経路停止 |
| 診断counter | queue clear failure count | `saturating_add(1)` | saturated flag 記録 | なし | wrap、panic、データ経路停止 |
| debug統計 | dump用累計 | `saturating_add(1)` | saturated表示 | なし | 成功/失敗判定に使う |

| 表示項目 | 値 |
|---|---|
| `counter_value` | `u64::MAX` |
| `counter_saturated` | `true` |
| `last_increment_result` | `Saturated` |

診断counterのsaturation/dropは、diagnostic取得APIを除く全business APIの戻り値を変更しない。例外は設けない。

| 表示項目 | 値 |
|---|---|
| 本体状態 | 維持 |
| 追加診断 | `diagnostic_counter_saturated:<counter_name>` |


## ワーカー abnormal exit と scan terminal state の固定方針''', flags=re.S)

p.write_text(s)

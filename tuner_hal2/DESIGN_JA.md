# Tuner HAL2 実装構造設計

## 本書の責務

本書は、`tuner_hal2`における論理責務の分割、依存方向、AIDL境界とドメイン処理の接続、現在の実装位置との対応を定義する。

公開AIDLの状態、戻り値、能力値、資源寿命、確定点、巻き戻し、後片付け、ワーカー、キュー、section/PES/TS処理は`../tuner_hal/DESIGN_JA.md`を正とする。PSI/SI表固有の意味解釈は`../arib_si_engine_rs/DESIGN_JA.md`を正とする。本書はこれらの契約を再定義せず、`tuner_hal2`の論理責務へ対応付ける。

物理ファイル名、module名、type名、関数名はAOSP公開契約またはARIB規範ではない。ただし、`../tuner_hal/DESIGN_JA.md`の`共通部品の定義条件`を満たすため、状態・寿命・失敗時遷移を所有する単一実装正本と許可entry pointは、本書の`共通transaction / use-caseの規範実装アンカー`で規範的な追跡アンカーとして固定する。責務を変えないrename、split、mergeだけでは公開設計変更にならないが、同一変更でアンカーを更新し、移動前後に複数正本を残してはならない。

本PRで追加・変更する契約は、`tuner_hal2`へ適用する目標設計であり、現行実装済みの事実を表すものではない。実装、設定、`Android.bp`、VTS用XML、単体試験が同じ契約へ追従し、`../タスク完了判定の実施方法.md`による横断確認が完了するまでは「設計済み・実装未適用」とする。公開能力を有効にできるのは、対応する実装入口、状態遷移・異常系試験、製品設定またはVTS設定がそろった機能だけである。移行は公開API単位を最小ゲートとし、依存するAPI、台帳、worker、設定を含む適用単位の完了条件も同時に満たす。未移行APIまたは未完了の依存閉包を新設計の成功能力として広告してはならない。

## 責務の一方向参照

| 正本 | 所有する内容 | 他文書での扱い |
|---|---|---|
| `tuner_hal/DESIGN_JA.md` | AOSP公開契約、VTSと能力公開、TS伝送構文、Table ID別section長、公開状態、寿命、失敗時遷移 | `tuner_hal2`は実装責務へ接続するだけとし、同じ表を持たない |
| `arib_si_engine_rs/DESIGN_JA.md` | PSI/SI表固有の意味解析と意味オブジェクト | Tuner HAL公開状態または伝送長を定義しない |
| `tuner_hal2/DESIGN_JA.md` | 実装内の論理責務、依存方向、現在位置との対応 | 公開契約の値や状態を上書きしない |
| `tuner_hal2/CODE_CONVENTION.md` | 実装規約、禁止構造、静的検査観点 | 状態遷移または戻り値を定義しない |

### 現行適用状態: nullable Binder境界

公開契約の意味・戻り値・状態遷移は`../tuner_hal/DESIGN_JA.md`の「nullable Binder 境界」を正とし、本節は現在の実装適用状態だけを追跡する。

Android 14 official AIDLから生成される現行Rust traitは、`IFilter.setDataSource()`、`IDescrambler.addPid()` / `removePid()`、`IFrontend.setCallback()`、`ILnb.setCallback()`のBinder interface引数をnon-null `Strong<dyn ...>`として受けるため、現在のRust実装にはNULLを受信するend-to-end経路がない。`future_work/r51/android14_aidl_rust_nullable_filter_boundary_blocker.md`は、公開AIDLを改変せずに現行Rust backendでNULLを受け取れない実装阻害だけを追跡する残課題であり、契約SSOTではない。同残課題が解消されるまでは、上記NULL経路を実装済み、VTS接続済み、またはAOSP契約達成済みと表明しない。 この阻害を理由に`setDataSource(NULL)`または`IDescrambler.addPid()` / `removePid()`のNULL経路を実装対象から除外してはならない。

依存はAIDL境界からドメイン処理へ向かう。下位層がAIDL objectまたはBinder statusを保持してはならない。

```mermaid
flowchart TD
    A[AIDL境界] --> B[サービス調停]
    B --> C[ドメイントランザクション]
    C --> D[機器・demux・FMQ]
    C --> E[資源台帳]
```

## 論理コンポーネント

| 論理責務 | 入力 | 所有するもの | 所有しないもの |
|---|---|---|---|
| AIDL境界 | AIDL引数、callback、object handle | AIDL値の外形検証、typed requestへの変換、Binder statusへの変換 | ドメイン状態、backend、rollback方針 |
| サービス調停 | typed request、root/object識別子 | object所有関係、世代の再検証、操作の振り分け、単一lock snapshot | packet解析、driver固有I/O |
| ドメイントランザクション | 検証済みrequest、予約済み資源 | 確定点、補償操作、局所隔離、状態変更 | Binder表現、AIDL callback実体 |
| 機器適合 | frontend/LNB要求 | device probe、driver固有設定、実状態の確認 | 公開能力の捏造、上位状態の直接変更 |
| demux処理 | 入力元とTS packet | 入力元世代、continuity、section/PES assembler、配送候補 | PSI/SI意味解析、公開object寿命 |
| FMQ・callback配送 | 確定済みpayload/event | queueへの確定、EventFlag、callback配送結果 | backend状態の巻き戻し |
| 資源台帳 | 予約・確定・解放要求 | object数、FMQ、PES、AV、DVR、descrambler、workerの使用権 | 公開能力値の独自算出 |
| 後片付け管理 | 閉鎖、所有者消滅、失敗した解放 | 未完手順、再試行権限、隔離資源 | 通常操作への復帰判断 |

## 公開メソッドの接続規則

静的inventory／capability参照メソッドは、サービス調停が同一lock内で変更不能な`CapabilitySnapshot`から応答snapshotを作り、AIDL境界が応答へ変換する。動的な`IFrontend.getStatus()`／`getFrontendStatusReadiness()`は`../tuner_hal/DESIGN_JA.md`の世代付き`FrontendStatusSnapshot`契約を正とし、現行製品ではtune/scan workerまたはbackend監視ownerがbounded backend I/O完了後に更新した値を読む。参照呼出し自身は状態変更、後片付け、ワーカー停止、callback配送を行わない。AOSPはqueryごとの同期backend readを必須にしていないため、現在のsnapshot方式を維持する。将来、特定statusをbounded同期readへ変更する場合は、対象status、I/O上限、失敗写像、generation再検証、snapshot更新との排他を公開状態表へ追加してから有効化する。

更新系メソッドは次の責務分担を守る。

1. AIDL境界は、対象objectとAPI種別を特定するための外形だけを読み取り、失敗し得るdomain入力変換は行わない。
2. サービス調停が同一排他区間で呼出対象objectの生存、呼出対象自身の登録owner、object generation、kind、依存generationを検証する。呼出対象lifecycle/generation不整合と引数値不正の優先順位および公開statusは`../tuner_hal/DESIGN_JA.md`の該当状態表をそのまま適用し、本書では値を再定義しない。
3. 呼出対象の生存検証後に、AIDL境界のtag、列挙値、nullable入力、値域をtyped requestへ変換する。引数として別objectを受けるAPIでは、引数objectの生存/generationとowner/demux/kind/互換関係を別段階で検証し、公開statusと状態不変条件は`../tuner_hal/DESIGN_JA.md`を正とする。呼出対象objectのowner検証と引数objectのownership検証を同じ判定へ丸めない。
4. サービス調停がrequestと依存関係を再検証し、一回限りの実行権限を発行する。
5. 資源台帳が失敗し得る予約を行う。
6. ドメイントランザクションが外部副作用を実行し、commit pointでdomain状態を確定する。commit前失敗は予約と準備物を逆順に戻し、commit後の後片付け失敗は`../tuner_hal/DESIGN_JA.md`の`CleanupPending`または隔離へ接続する。
7. AIDL境界は確定結果だけをBinder応答へ変換する。

失敗時の戻り値、補償操作、`CleanupPending`、隔離条件は`../tuner_hal/DESIGN_JA.md`に従う。AIDL境界、サービス調停、機器適合が独自の状態表を持ってはならない。

### 契約正本と実装入口の対応

公開transactionのphase、確定点、失敗処理は`../tuner_hal/DESIGN_JA.md`の「公開transactionのphase・確定点・失敗処理契約」を正とする。本節が規範として所有するのは、その契約を強制する実装ownerと呼び出し禁止入口だけである。

| 契約 | 実装所有者 | 禁止入口 |
|---|---|---|
| object method | サービス調停のobject method use-case | AIDL methodからbackend、registry、低水準dispatchを直接呼ばない |
| root/child open | サービス調停のopen use-case | AIDL helperでledger IDを再解釈しない |
| public close / owner loss / Drop leak | 一回限りのcleanup authorityを持つclose use-case | AIDL、Drop、Reaperが同時にcleanup authorityを持たない |
| descrambler key/session | descrambler transaction use-case。`setKeyToken()`のkey/session更新は`DescramblerKeyTxn`、closeまたはdemux無効化時のPID/key/pool cleanupは`DescramblerSessionCleanupTxn`だけが所有する | AIDL層またはdescrambler crateからkey tableを直接変更しない。close/invalidate経路でPID、key、pool帰属を個別releaseしない |
| source boundary | `SourceBoundaryTxn`がsource/sink関係のvalidate・prepare・commit・rollbackを所有し、データ経路境界は`StreamBoundaryTxn`へ一回だけ要求する | filter wrapperまたはAPI別use-caseから接続表・origin generation・assembler・queueを個別変更しない |
| frontend tune/scan | frontend session transactionはfrontend/backend lifecycle、対象demux集合、公開状態、戻り値と処理順序だけを所有し、demux内部の境界変更は`StreamBoundaryTxn`へ委譲する | worker、backend adapter、callback層がfrontend公開状態を直接確定しない。`tune()` / `stopTune()` use-caseがassembler、FMQ、AV、record queue、stream generationを個別変更しない |
| Filter / DVR flush | 公開`flush()` use-caseは状態表からcleanup対象と公開結果を確定し、実際のqueue cleanupと失敗集約は`QueueCleanupTxn`だけが所有する | API別use-caseがFMQ pointer、queue内容、parser generation、`queue_epoch`を直接更新しない |
| DVR playback consume | playback workerはworker lifecycle、待機、停止とtyped outcome処理だけを所有し、FMQ read・TS parse・backend inject・消費確定は`PlaybackConsumeTxn`だけが所有する | playback workerまたは別helperでread/parse/inject/consume状態機械を再実装しない |
| callback artifact | callback registry use-case | demux/device/resource ledgerへBinder callback実体を渡さない |
| worker終端 | worker ownerと後片付け管理は停止順序と分類結果からの状態写像だけを所有し、stop/wake/join/EventFlag/Reaper/backend制御の失敗種別は`WorkerFailureClassifier`だけが分類する | worker自身がowner objectをunregisterしない。API別・worker別に同じ失敗種別を文字列、errno、個別分岐で再判定しない |

#### 共通transaction / use-caseの規範実装アンカー

次表は`../tuner_hal/DESIGN_JA.md`の`共通部品の定義条件`に対する`実装正本`、`公開入口`、`呼び出し許可層`、`呼び出し禁止層`を固定する。記載したmodule/file/typeは外部APIではなく、現在の単一実装正本を特定する規範的トレーサビリティアンカーである。責務不変のrename、split、mergeでは同一変更で本表を更新し、旧アンカーと新アンカーを同時に正本として残してはならない。`../tuner_hal/DESIGN_JA.md`のAPI別表や後続散文に`reset`、破棄、世代更新、queue clear、key release、worker失敗等の具体的効果が記載されていても、それは要求される公開意味・対象・順序・結果を表し、同じ状態変更の第二の実装正本をAPI別use-caseへ与えるものではない。共通部品適用表が所有者を指定する状態変更は、本表のアンカーだけが確定する。

| 契約 | 状態・寿命・失敗時遷移の単一実装正本 | 許可entry point | 禁止する迂回 |
|---|---|---|---|
| object method | `service_runtime/src/object_method_txn.rs`の`ObjectMethodTxnPlan`、`ObjectMethodDispatchProof`、`ObjectMethodExecutionToken`。validation/dispatchの補助正本は同moduleからだけ呼ぶ`method_validation.rs`と`method_dispatch.rs` | `aidl_service/src/object_runtime/mod.rs`の`execute_*_use_case*`、`plan_unavailable_object_method_use_case()`、`execute_object_query_use_case()`。domain側は`TunerServiceRuntime::*_for_object`が`ObjectMethodExecutionToken`を一回消費する | 個別AIDL methodによる先行runtime query、`AidlMethodAdapter::plan()`の直接実行、dispatch proofの生成・再利用、backend/registryの直接変更 |
| root open | `service_runtime/src/root_object_ops.rs`。登録後失敗の補償正本は`service_runtime/src/open_rollback.rs` | `aidl_service/src/tuner_service.rs`のroot AIDL methodからroot object use-caseを呼び、返された`RuntimeObjectEntry`からtyped Binder objectを生成する。生成後失敗はservice_runtime rollback入口へ返す | AIDL層でruntime allocation、object table登録、rollback順序、status写像を組み立てる |
| child open | `service_runtime/src/boot/demux_filter_dvr_txn.rs`の`DemuxFilterDvrTxn<'a>`。公開use-case façadeは`service_runtime/src/demux_filter_dvr_ops.rs` | `aidl_service/src/child_object_open.rs`の`open_filter_child_for_owner_object_with_request_builder()`および`open_dvr_child_for_owner_object_with_request_builder()` | `openFilter()`/`openDvr()`ごとのallocation・callback cleanup・rollback複製、`RuntimeObjectEntry.ledger_id`の再解釈 |
| public close / owner loss / Drop leak | `service_runtime/src/object_close_txn.rs`のtyped artifact/domain/runtime cleanup command、`CleanupExecutionReport`接続、close finalization | `aidl_service/src/object_runtime/mod.rs`の`close_object_after_close_preflight()`および`drop_leak_object()`。runtime unregisterは`TunerServiceRuntime::unregister_public_runtime_for_closed_aidl_entry()`だけを使用する | AIDL、Drop、worker、Reaperによるcleanup authorityの重複保持、runtime unregister前の`Closed` commit、個別objectでのclose state machine複製 |
| descrambler key/session | `service_runtime/src/boot/descrambler_txn.rs`の`DescramblerTxn<'a>`内で、`DescramblerKeyTxn`をkey ref取得・置換・releaseとsession key変更の唯一の更新transaction、`DescramblerSessionCleanupTxn`をclose/demux無効化時のPID claim・key ref・pool session帰属の唯一のcleanup transactionとする。session stateは`service_runtime/src/descrambler_session.rs`、key token/slot/refcountは`service_runtime/src/descrambler_key_table.rs`だけが所有する | `service_runtime/src/descrambler_ops.rs`の`TunerServiceRuntime::*_for_object` use-case。AIDL methodはobject method façade経由で`ObjectMethodExecutionToken`を渡す。初回`setDemuxSource()` bindingと通常の`addPid()` / `removePid()`は公開domain transactionのまま維持する | AIDL層または`descrambler` crateからsession/key tableを直接変更する、`setKeyToken()` callerがkey refcountとsession keyを別々に更新する、close/invalidate callerがPID/key/pool cleanupを個別所有する、1件のcleanup失敗で後続claimを未試行のまま終了する |
| source relation | `demux/src/runtime/source_boundary.rs`の`SourceBoundaryTxn`。service-level owner/generation調停は`service_runtime/src/boot/demux_filter_dvr_txn.rs` | `service_runtime/src/demux_filter_dvr_ops.rs`のobject-handle based source use-caseをobject method façadeから呼ぶ。`SourceBoundaryTxn`はsource/sink関係のcommit時に必要なdata-path boundaryを`StreamBoundaryTxn`へ一回だけ要求する | filter wrapperから接続graph、owner demux、queue generation、assemblerを個別に変更する、source relationとstream stateを別々のAPI固有commitで確定する |
| stream boundary / packet ingress | `../tuner_hal/DESIGN_JA.md`の論理契約`StreamBoundaryTxn`の実装アンカーは`demux/src/runtime/generation_boundary.rs`の`GenerationBoundaryTxn`とし、packet ingressは`service_runtime/src/boot/packet_txn.rs`の`PacketTxn<'a>`、packet pipelineは`demux/src/parser/packet_pipeline.rs`が所有する。`GenerationBoundaryTxn`は`StreamBoundaryTxn`の実装名であり、別の境界状態正本を意味しない | `service_runtime/src/packet_ops.rs`のtyped packet/source-boundary use-case。`FrontendTxn`、`SourceBoundaryTxn`、flush/closeの上位transactionは対象と順序を固定したうえで同じtyped boundary入口を一回だけ呼ぶ | AIDL層、backend adapter、filter callback、`FrontendTxn`、source APIがcontinuity、assembler、FMQ/AV/record境界、stream generationを個別に直接変更する |
| Filter / DVR queue cleanup | `service_runtime/src/queue_cleanup_txn.rs`の`QueueCleanupTxn`を公開`flush()` cleanup orchestrationと失敗集約の単一正本とする。Filter/SharedFilterの生産許可排出は`FilterProducerDrainGate`、DVR queue transactionとepochは`QueueEpochProtocol`を下位protocol正本として維持し、`QueueCleanupTxn`はその状態を二重所有しない | Filter/DVR object method use-caseは状態表からcleanup対象を確定し、`QueueCleanupTxn`へtyped cleanup requestを一回渡す | API別use-caseがFMQ pointer、queue内容、pending event、parser generation、`queue_epoch`を直接変更する、または下位protocolを飛び越えてcleanup結果を成功に丸める |
| DVR playback consume | `service_runtime/src/playback_consume_txn.rs`の`PlaybackConsumeTxn`をFMQ read、processing buffer、TS parse、backend partial accept/retry、injection cursor、消費確定の単一状態機械とする | playback workerは開始/停止・待機・起床を所有し、1回のconsume stepごとに`PlaybackConsumeTxn`を呼び、typed outcomeだけを処理する | playback worker、FMQ helper、packet helperがread/parse/inject/consume状態機械を再実装する、注入結果を確定する前にread済み入力を一律消費済みにする |
| frontend tune/scan | `service_runtime/src/boot/frontend_txn.rs`の`FrontendTxn<'a>`。public use-case façadeは`service_runtime/src/frontend_ops.rs` | `aidl_service/src/tuner_service/frontend_methods.rs`からobject method façadeを経由して`TunerServiceRuntime`のfrontend object use-caseを呼ぶ。full retune、`stopTune()`、scan切替でdemux境界が必要な場合は`StreamBoundaryTxn`のtyped入口だけを使用する | worker、device backend、callback delivery層によるfrontend公開状態・generation・rollback状態の直接確定、`FrontendTxn`自身によるassembler、FMQ、AV、record queue、demux stream generationの直接変更 |
| callback artifact | callback registry stateは`service_runtime/src/callback_registry.rs`の`RuntimeCallbackRegistry`、AIDL callback実体は`aidl_service/src/callback_store.rs`だけが所有し、domain commit/rollbackは対象の`service_runtime/src/*_ops.rs` use-caseが所有する | `aidl_service/src/object_runtime/mod.rs`のcallback registration façadeでlive/dispatch preflight後にartifact bridgeを実行し、service_runtime finish use-caseへ結果を返す | callback実体をdemux/device/resource ledgerへ渡す、AIDL層でruntime registrationとdomain commitのrollback方針を持つ |
| worker failure classification | `service_runtime/src/worker_failure_classifier.rs`の`WorkerFailureClassifier`をstop/wake/join/EventFlag/Reaper/backend制御の失敗種別をtyped resultへ分類する単一正本とする | 各worker ownerと後片付け管理は生の失敗をclassifierへ渡し、typed resultだけをAPI別状態写像、retry、cleanup、診断へ使用する | `reason.contains(...)`、文字列、API固有のerrno分岐、worker種別ごとの同型分類で失敗種別を決める |
| worker終端 | frontend workerの停止・回収・cleanup resultは`service_runtime/src/frontend_worker_txn.rs`、worker slot/generationは`device/src/runtime/frontend_worker.rs`の`FrontendWorkerRegistry`が所有する | `service_runtime/src/frontend_ops.rs`および`service_runtime/src/boot/frontend_txn.rs`のworker lifecycle use-case。close/owner-lossは`ObjectCloseTxn`からtyped cleanup commandとして接続する。失敗種別は`WorkerFailureClassifier`のtyped resultだけを受ける | worker自身によるowner unregister/lease返却、回収完了前のresource再利用、AIDL層によるjoin/reaper方針の決定、worker終了use-caseによる失敗種別の再分類 |

##### 共通部品とAPI固有手順の合成規則

- `SourceBoundaryTxn`はsource/sink関係のvalidate・prepare・commit・rollbackを所有し、data-path boundaryが必要な場合だけ`StreamBoundaryTxn`へ一回委譲する。`setDataSource()`、source置換、source flush/reconfigure/closeのAPI別記述は要求される効果・順序・公開結果を定めるものであり、関係表とorigin generation/assembler/queueをAPI側で別々に確定する根拠にしない。
- `FrontendTxn`はfrontend/backend lifecycle、対象demux集合、公開状態、AIDL結果と処理順序を所有する。full retune、`stopTune()`、scan切替で必要なdemux内部のorigin、generation、assembler、FMQ、AV shared、record queueの変更は`StreamBoundaryTxn`だけが確定する。
- `QueueCleanupTxn`はFilter/DVR公開`flush()`のcleanup orchestrationと失敗集約を所有する。`FilterProducerDrainGate`はFilter/SharedFilterの生産許可排出、`QueueEpochProtocol`はDVR queue transactionとepochを所有する下位protocolであり、公開APIまたは`QueueCleanupTxn`がそれらの内部状態を第二の正本として保持しない。
- `PlaybackConsumeTxn`は`../tuner_hal/DESIGN_JA.md`のplayback consumer commit表、processing buffer、injection cursor、retry契約を実現する状態機械正本である。playback workerはworker lifecycleとtyped outcome処理だけを行い、同じread/parse/inject/consume遷移を個別に持たない。
- `DescramblerKeyTxn`は`setKeyToken()`のkey ref取得・置換・releaseとsession key変更を所有する。`DescramblerSessionCleanupTxn`はclose/demux無効化時のPID claim、key ref、pool session帰属のcleanupと失敗集約を所有する。初回`setDemuxSource()` bindingおよび通常の`addPid()` / `removePid()`は各公開domain transactionを維持し、このcleanup transactionへ統合しない。
- `WorkerFailureClassifier`は失敗種別の分類だけを所有する。各API、worker owner、ワーカー終了契約は停止順序、retry/cleanup、typed resultからの状態遷移を所有してよいが、失敗種別そのものを別規則で再分類しない。

### ルートobject

`openFrontendById()`、`openDemux()`、`openDemuxById()`、`openDescrambler()`、`openLnbById()`、`openLnbByName()`は、同じroot open責務を使う。各APIのAIDL入出力を混成せず、入力IDを持つAPIでは公開IDを検証し、`openDemux()`と`openLnbByName()`ではobjectとout IDを同一確定点で公開する。`openDescrambler()`はdemux未結合のobject/session枠だけを生成し、demux IDと復号poolの選択は一回限りの`setDemuxSource()`へ委ねる。使用権予約、runtime登録、typed Binder object生成、失敗時の解放を一つの操作として扱い、objectを返した後に登録を巻き戻さない。

`getFrontendIds()`、`getFrontendInfo()`、`getLnbIds()`、`getDemuxIds()`、`getDemuxInfo()`、`getDemuxCaps()`、`getMaxNumberOfFrontends()`、`isLnaSupported()`は、起動時に確定した能力snapshotと現在の使用上限から応答する。照会中にprobeまたは能力の再選択を行わない。

### 子objectと関連付け

Filter、DVR、TimeFilterなどの子objectは、親demuxの生存、所有者、世代、能力、資源予約を確認してから登録する。Descramblerはrootで未結合objectを生成した後、`setDemuxSource()`で親demuxの生存、世代、能力、復号poolを検証し、一回だけ原子的に結合する。対応しないTimeFilterは`tuner_hal`の契約どおりobjectを生成しない。

`IFilter.setDataSource()`、DVR接続、descramblerのPID登録、frontendとLNBまたはCI CAMの接続は、両objectの所有者と世代を同じsnapshotで検証する。片側だけを確定した状態を通常状態として公開しない。

### 入力処理

TS入力は、frontend、playback DVR、許可されたsource filterの入力元を別の世代空間で保持する。packet validation、continuity、section/PES組み立て、filter照合までをdemux責務とし、PSI/SI意味解析を呼ばない。

queueへの書き込み権限は世代付きとし、`flush()`、再設定、停止、再選局、入力元変更、閉鎖で旧世代を失効させる。配送済みAV領域など、クライアントが保持する資源の寿命はqueue世代と分離する。

## 現在実装との追跡索引

次表は規範実装アンカーを探すための補助索引であり、transaction正本または公開契約を置き換えない。

| 論理責務 | 現在の主な位置 |
|---|---|
| AIDL境界 | `aidl_service/`、`binder_adapter/` |
| サービス調停 | `service_runtime/` |
| typed request | `domain_request/` |
| frontend/LNB backend | `device/`、`lnb/` |
| demuxとpacket処理 | `demux/` |
| descrambler | `descrambler/` |
| FMQ | `fmq/`、`fmq_shim/` |
| 資源台帳 | `resource_ledger/` |
| 共通の値型 | `common/` |
| Android公開設定 | `manifest/`、`init/`、`sepolicy/`、`config/` |

## 適用状態と移行完了条件

本PRの文書変更だけでは、次の項目を実装済みまたは試験済みと扱わない。各行は依存するAPI・台帳・worker・設定を含む不変条件閉包として移行し、完了条件を満たした行だけを製品profileで有効にする。

| 適用単位 | 現在状態 | 実装追跡先 | 移行完了条件 |
|---|---|---|---|
| 公開object methodの検証順序とtransaction境界 | 設計済み・実装未適用 | `aidl_service/`、`service_runtime/`、`domain_request/` | 対象APIの入口が規範phase orderへ一本化され、状態不正と入力不正の優先順位、commit前後失敗、rollbackの試験が合格 |
| 共通transaction経路の一本化 | 設計済み・実装未適用 | `service_runtime/`、`demux/`、descrambler/worker owner | `SourceBoundaryTxn` / `StreamBoundaryTxn` / `QueueCleanupTxn` / `PlaybackConsumeTxn` / `DescramblerKeyTxn` / `DescramblerSessionCleanupTxn` / `WorkerFailureClassifier`の各対象経路が規範anchorへ一本化され、API/worker側に同じ状態機械・失敗分類・cleanup ownershipが残らないことを静的確認と異常系試験で確認 |
| frontend tune/scan、再選局、終端deadline | 設計済み・実装未適用 | `service_runtime/`、`device/`、callback配送 | AOSP callback契約を満たす終端、`scan(K)→LOCKED(g1)→scan(K)→END(g2)`で2回目のbackend探索・LOCKED再配送がないこと、異なるscan request・stopScan・tune・closeで継続状態が失効すること、安定同一条件の非破壊re-entry、full retuneでの旧session遮断、破壊的commit後に旧要求を再投入しないこと、旧TSが新demux/filter世代へ混入しないこと、原因別の`Untuned`／`FailedBackend`／`FailedBoundary`／`Quarantined`遷移、deadlineの試験が合格 |
| Filter/DVR/AV/PESとFMQの資源契約 | 設計済み・実装未適用 | `demux/`、`fmq/`、`fmq_shim/`、`resource_ledger/` | `CapabilitySnapshot`からの予約、event-local/shared AV、processing buffer、overflow、close解放の試験が合格 |
| 自律cleanupとworker回収 | 設計済み・実装未適用 | `service_runtime/`、各worker owner、`resource_ledger/` | owner操作なしで再試行が進み、期限後の隔離またはservice-critical遷移とlease非再利用を試験で確認 |
| query snapshotとbackend適合 | 設計済み・実装未適用 | `service_runtime/`、`device/`、`config/` | queryがbackend I/Oを行わず、世代付きcacheの更新・失効とmanifest/probeによるbackend選択を試験で確認 |
| VTS/product profile | 設計保留 | `config/`、VTS XML、製品設定 | `VTS-ENV-01`から`06`の実測値を確定し、対応XML一式を静的選択して対象VTSを合格 |

## 構造上の禁止事項

- AIDL methodごとにclose、queue、rollback、quarantineの状態機械を複製しない。
- `../tuner_hal/DESIGN_JA.md`の共通部品適用表が所有者を指定した処理について、API別use-case、worker、helperが同じ状態変更、cleanup、失敗分類を個別再実装しない。
- `tuner_hal`で定義した公開戻り値を`service_runtime`またはbackendで別の値へ読み替えない。
- AIDL objectまたはcallback実体をdemux、device、resource ledgerへ渡さない。
- 静的inventory／capability queryからcleanup、worker操作、backend I/Oを開始しない。動的frontend status queryは現行製品では世代付き`FrontendStatusSnapshot`だけを読み、query呼出しを契機にbackend I/Oを開始しない。
- file名またはtype名をAOSP公開契約、ARIB根拠、公開状態遷移の値そのものとして扱わない。
- `共通transaction / use-caseの規範実装アンカー`以外の物理配置表を状態遷移の正本として扱わない。
- 規範実装アンカーのrename、split、merge時に旧アンカーを残したまま新アンカーを追加し、複数のtransaction正本を作らない。
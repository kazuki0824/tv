# Tuner HAL2 実装構造設計

## 本書の責務

本書は、`tuner_hal2`における論理責務の分割、依存方向、AIDL境界とドメイン処理の接続、現在の実装位置との対応を定義する。

公開AIDLの状態、戻り値、能力値、資源寿命、確定点、巻き戻し、後片付け、ワーカー、キュー、section/PES/TS処理は`../tuner_hal/DESIGN_JA.md`を正とする。PSI/SI表固有の意味解釈は`../arib_si_engine_rs/DESIGN_JA.md`を正とする。本書はこれらの契約を再定義せず、`tuner_hal2`の論理責務へ対応付ける。

物理ファイル名、module名、type名、関数名はAOSP公開契約またはARIB規範ではない。ただし、`../tuner_hal/DESIGN_JA.md`の`共通部品の定義条件`を満たすため、状態・寿命・失敗時遷移を所有する単一実装正本と許可entry pointは、本書の`共通transaction / use-caseの規範実装アンカー`で規範的な追跡アンカーとして固定する。責務を変えないrename、split、mergeだけでは公開設計変更にならないが、同一変更でアンカーを更新し、移動前後に複数正本を残してはならない。

本PRで追加・変更する契約は、`tuner_hal2`へ適用する目標設計であり、現行実装済みの事実を表すものではない。実装、設定、`Android.bp`、VTS用XML、単体試験が同じ契約へ追従し、`../タスク完了判定の実施方法.md`による横断確認が完了するまでは「設計済み・実装未適用」とする。公開能力を有効にできるのは、対応する実装入口、状態遷移・異常系試験、製品設定またはVTS設定がそろった機能だけである。移行は公開API単位を最小ゲートとし、依存するAPI、台帳、worker、設定を含む適用単位の完了条件も同時に満たす。未移行APIまたは未完了の依存閉包を新設計の成功能力として広告してはならない。

## 責務の一方向参照

| 正本 | 所有する内容 | 他文書での扱い |
|---|---|---|
| `tuner_hal/DESIGN_JA.md` | AOSP公開契約、VTSと能力公開、TS伝送構文、Table ID別section長、公開状態、寿命、失敗時遷移、共通部品の論理契約 | `tuner_hal2`は実装責務へ接続するだけとし、同じ状態表を持たない |
| `arib_si_engine_rs/DESIGN_JA.md` | PSI/SI表固有の意味解析と意味オブジェクト | Tuner HAL公開状態または伝送長を定義しない |
| `tuner_hal2/DESIGN_JA.md` | 実装内の論理責務、依存方向、現在位置との対応、規範実装アンカー | 公開契約の値や状態を上書きしない |
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
| FMQ・callback配送 | 確定済みpayload/event | queueへの確定、data通知EventFlag、callback配送結果 | backend状態の巻き戻し、worker制御失敗分類 |
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

公開transactionのphase、確定点、失敗処理は`../tuner_hal/DESIGN_JA.md`の「公開transactionのphase・確定点・失敗処理契約」と「0-S-3B. 共通部品の規範定義」を正とする。本節が規範として所有するのは、その契約を強制する実装ownerと呼び出し禁止入口だけである。

| 契約 | 実装所有者 | 禁止入口 |
|---|---|---|
| object method | サービス調停のobject method use-case | AIDL methodからbackend、registry、低水準dispatchを直接呼ばない |
| root/child open | サービス調停のopen use-case | AIDL helperでledger IDを再解釈しない |
| public close / owner loss / Drop | `ObjectCloseTxn`だけがcleanup実行authorityを持つ | AIDL、Drop、Reaper、個別objectが別々にcleanup authorityを持たない |
| descrambler key | `DescramblerKeyTxn` | callerがkey refcountとsession keyを別々に更新しない |
| descrambler PID | `DescramblerPidTxn` | `addPid()` / `removePid()` callerがbackend apply、PID claim、補償、quarantine判定を個別実装しない |
| descrambler session cleanup | `DescramblerSessionCleanupTxn` | close/invalidate callerがPID、key、pool帰属を個別releaseしない |
| Filter source relation | `SourceBoundaryTxn` | filter wrapperまたはAPI別use-caseが接続graphを直接確定しない |
| Demux frontend source relation | `DemuxFrontendSourceTxn` | `IDemux.setFrontendDataSource()` callerがrelationとstream boundaryを別々の確定点で公開しない |
| stream boundary | `StreamBoundaryTxn` | relation、queue token、A/V sync map、PCR store内部、callback artifactを所有しない |
| packet ingress / pipeline | `PacketTxn` / `PacketPipeline` | 通常packet ingress/parse/filter dispatchを`StreamBoundaryTxn`へ吸収しない。AIDL/backend adapter/filter callbackがpipeline状態を直接変更しない |
| frontend tune/scan | `FrontendTxn` | worker、backend adapter、callback層がfrontend公開状態/generation/rollbackを直接確定しない。demux内部boundaryを`FrontendTxn`へ吸収しない |
| Record DVR / Filter relation | `RecordDvrFilterRelationTxn` | DVR側とFilter側がrelation shadow copyを別commitしない |
| LNB persistent control | `LnbControlTxn` | 3つのpersistent control APIで同じbackend+registry transactionを複製しない |
| callback registration | service_runtime側`CallbackRegistrationUseCase`がruntime/domain prepare・composite commit・rollback policyを所有し、AIDL façadeはBinder artifactのprepare/releaseだけを担当 | AIDL façadeがdomain state/rollback policyを所有すること、Binder artifact・runtime registry・domain callback stateを片側だけcommitすること |
| post-commit callback failure | `PostCommitCallbackFailureTxn`が`WorkerFailureClassifier`で分類済みのtyped callback failureを受け、delivery outcome / callback health / diagnosticへの写像だけを所有する | failure categoryの再分類、commit済みdomain stateのrollback、API別同型handler |
| Filter / DVR flush cleanup orchestration | `QueueCleanupTxn`。Filter/DVR固有stateを所有せず、typed下位protocol呼出しと失敗集約だけを共通化 | API別orchestration複製、下位protocol内部stateの直接変更 |
| DVR playback consume | `PlaybackConsumeTxn` | playback workerがread/parse/inject/consume状態機械を再実装しない |
| A/V sync relation | `AvSyncRegistry` | `media_filter_id -> hw_sync_id`のmany-to-one relationと、保持する場合の`hw_sync_id -> Set<media_filter_id>` reverse indexを片側だけ直接変更しない。injective / bijectiveを仮定しない |
| PCR clock anchor | `PcrClockAnchorStore` | APIまたは`StreamBoundaryTxn`がanchor内部を直接変更しない |
| worker lifecycle mechanism | `WorkerRuntime` / `WorkerHandle` | 別のgeneric worker lifecycle transaction/protocolを重ねない |
| worker failure classification | `WorkerFailureClassifier`。stop/wake/join/EventFlag/Reaper/backend-control/callback等の生の失敗を共通typed分類し、分類結果だけを返す | 停止順序、retry/cleanup、公開状態遷移の所有、API別再分類 |

#### 共通transaction / use-caseの規範実装アンカー

次表は`../tuner_hal/DESIGN_JA.md`の`共通部品の定義条件`に対する`実装正本`、`公開入口`、`呼び出し許可層`、`呼び出し禁止層`を固定する。既存アンカーは責務変更がない限り維持し、新しい論理契約だけを既存の責務層へ追加する。記載した新規type名・新規pathは目標設計の単一正本名であり、現時点の実装済み事実を意味しない。

| 契約 | 状態・寿命・失敗時遷移の単一実装正本 | 許可entry point | 禁止する迂回 |
|---|---|---|---|
| object method | `service_runtime/src/object_method_txn.rs`の`ObjectMethodTxnPlan`、`ObjectMethodDispatchProof`、`ObjectMethodExecutionToken`。validation/dispatchの補助正本は同moduleからだけ呼ぶ`method_validation.rs`と`method_dispatch.rs` | `aidl_service/src/object_runtime/mod.rs`の`execute_*_use_case*`、`plan_unavailable_object_method_use_case()`、`execute_object_query_use_case()`。domain側は`TunerServiceRuntime::*_for_object`が`ObjectMethodExecutionToken`を一回消費する | 個別AIDL methodによる先行runtime query、`AidlMethodAdapter::plan()`の直接実行、dispatch proofの生成・再利用、backend/registryの直接変更 |
| root open | `service_runtime/src/root_object_ops.rs`。登録後失敗の補償正本は`service_runtime/src/open_rollback.rs` | `aidl_service/src/tuner_service.rs`のroot AIDL methodからroot object use-caseを呼び、返された`RuntimeObjectEntry`からtyped Binder objectを生成する。生成後失敗はservice_runtime rollback入口へ返す | AIDL層でruntime allocation、object table登録、rollback順序、status写像を組み立てる |
| child open | `service_runtime/src/boot/demux_filter_dvr_txn.rs`の`DemuxFilterDvrTxn<'a>`。公開use-case façadeは`service_runtime/src/demux_filter_dvr_ops.rs` | `aidl_service/src/child_object_open.rs`の`open_filter_child_for_owner_object_with_request_builder()`および`open_dvr_child_for_owner_object_with_request_builder()` | `openFilter()`/`openDvr()`ごとのallocation・callback cleanup・rollback複製、`RuntimeObjectEntry.ledger_id`の再解釈 |
| `ObjectCloseTxn` | `service_runtime/src/object_close_txn.rs::ObjectCloseTxn`。`begin_close`がlogical close確定・新規通常操作遮断・`CloseCleanupAuthority`取得を単一atomic commitとして所有し、typed artifact/domain/runtime cleanup command、`CleanupExecutionReport`接続、close finalizationを同typeが所有する | `aidl_service/src/object_runtime/mod.rs`のpublic close、owner-loss/Drop入口、service shutdown/reaper retryはいずれも同じ`begin_close`を使用する。取得済みauthorityの未完分だけを回収機構へ一度移管する | `DropLeakTxn`等の別cleanup authority、logical closeとauthority取得の分離commit、AIDL/Drop/worker/Reaperによるauthority重複、個別objectでのclose state machine複製 |
| `DescramblerKeyTxn` / `DescramblerPidTxn` / `DescramblerSessionCleanupTxn` | 既存`service_runtime/src/boot/descrambler_txn.rs`内の独立type。session stateは`service_runtime/src/descrambler_session.rs`、key token/slot/refcountは`service_runtime/src/descrambler_key_table.rs`だけが所有する | 通常key/PID操作は`service_runtime/src/descrambler_ops.rs`のobject use-case。session cleanupはdescrambler close時に`ObjectCloseTxn` typed cleanup commandから、demux invalidation時にdemux invalidation ownerから直接typed cleanup requestとして`DescramblerSessionCleanupTxn`へ入る | key/PID/cleanupのstate machineを相互に吸収する、AIDL層またはdescrambler crateから台帳を直接変更する、demux invalidationをpublic close authorityへ変換する、close/invalidate callerがPID/key/pool cleanupを個別所有する |
| `SourceBoundaryTxn` | 既存`demux/src/runtime/source_boundary.rs` | `service_runtime/src/demux_filter_dvr_ops.rs`のFilter source use-case、およびsource Filter close/unlink時に`ObjectCloseTxn`/cleanupから渡されるtyped relation mutation | demux/frontend relationをこのtransactionへ吸収する、filter wrapper/cleanup callerからgraph/stream stateを直接変更する |
| `DemuxFrontendSourceTxn` | `service_runtime/src/demux_filter_dvr_ops.rs::DemuxFrontendSourceTxn` | `IDemux.setFrontendDataSource()` object use-case、およびFrontend/Demux close時に`ObjectCloseTxn`/cleanupから渡されるtyped unbind mutation | relation recordと`StreamBoundaryTxn`を別commitで公開する、cleanup callerがrelationを直接編集する、`SourceBoundaryTxn`へ吸収する |
| `StreamBoundaryTxn` | 既存`demux/src/runtime/generation_boundary.rs::GenerationBoundaryTxn`。`GenerationBoundaryTxn`は`StreamBoundaryTxn`の実装名であり別正本ではない | `service_runtime/src/packet_ops.rs`のtyped boundary use-case。上位transactionは`prepare`で`PreparedStreamBoundary`を得て、同一owner排他区間で`commit`または`abort`する | relation table、Filter/DVR queue内部、A/V sync map、PCR store内部、callback artifact、descrambler key/PIDを直接変更する |
| packet ingress / pipeline | `service_runtime/src/boot/packet_txn.rs::PacketTxn<'a>`がpacket ingress transaction、`demux/src/parser/packet_pipeline.rs::PacketPipeline`が通常packet parse/filter pipelineの単一アンカー | `service_runtime/src/packet_ops.rs`のtyped packet ingress use-case。必要な境界変更だけを`StreamBoundaryTxn`のtyped入口へ委譲する | 通常packet ingress/pipelineを`StreamBoundaryTxn`へ吸収する、AIDL層/backend adapter/filter callbackがPacketTxn/PacketPipelineを迂回してcontinuity・assembler・delivery stateを直接変更する |
| `RecordDvrFilterRelationTxn` | `service_runtime/src/demux_filter_dvr_ops.rs::RecordDvrFilterRelationTxn` | Record DVR `attachFilter()` / `detachFilter()`、Filter/DVR close、demux cleanupからのtyped relation mutation | 両objectのrelation shadow copyを別commitする、close側がrelation tableを直接編集する |
| `LnbControlTxn` | `service_runtime/src/lnb_control_txn.rs::LnbControlTxn` | `ILnb.setVoltage()` / `setTone()` / `setSatellitePosition()`のobject use-case | `sendDiseqcMessage()`をpersistent state transactionへ吸収する、3 APIでlock/apply/commit/failure transitionを複製する |
| `CallbackRegistrationUseCase` | service_runtime側`service_runtime/src/callback_registry.rs::CallbackRegistrationUseCase`がregistration orchestrationとrollback policyを所有し、prepared runtime mutationは同moduleの`RuntimeCallbackRegistry`、domain logical stateは対象`service_runtime/src/*_ops.rs`のprepared mutationが所有する。Binder artifact本体は既存`aidl_service/src/callback_store.rs`だけが所有する | `IFrontend.setCallback()` / `ILnb.setCallback()`等ではAIDL façadeがlive/dispatch preflight後にBinder artifactを非公開prepareし、そのhandleをservice_runtime ownerへ渡す。service_runtime composite commit後、AIDL façadeは旧artifact cleanup/releaseだけを行う | AIDL façadeがdomain state・runtime registry・rollback policyを所有する、Binder callback実体をLNB/demux/device/resource ledgerへ保持する、artifact/runtime/domainを片側だけcommitする |
| `PostCommitCallbackFailureTxn` | `service_runtime/src/post_commit_callback_failure_txn.rs::PostCommitCallbackFailureTxn`。分類済みtyped callback failureからdelivery outcome、callback health、必須診断への写像だけを所有し、failure category分類・commit済みdomain state・worker lifecycle・rollbackは所有しない | domain commitを完了したFrontend/Filter/DVR等のcompletion use-caseが`WorkerFailureClassifier`の分類済みtyped callback failureを一回だけ渡す | failure categoryの再分類、文字列/errno分類、domain commitのrollback、APIごとのcallback health/診断更新再実装 |
| `FilterProducerDrainGate` | 既存`demux/src/runtime/queue_runtime.rs`。`Open`/`Draining`/`Closed`、`filter_delivery_generation`、`parser_state_generation`、`admitted_producer_count`、bounded pending event queue、`FilterProducerPermit(g)`の単一正本 | Filter/SharedFilter data pathのtyped producer admission/finishと、`QueueCleanupTxn`からのtyped drain/flush requestだけを入口とする | 公開API、worker、`QueueCleanupTxn`がgate内部状態を直接変更する、permit/drainを飛び越えてFMQ writeまたはpending event追加を確定する、DVR stateを吸収する |
| `QueueEpochProtocol` | 既存`demux/src/runtime/queue_runtime.rs`。`Open(g)`/`Draining(g)`/`Closed`、一回限りのread/write transaction token、受付中transaction数、`queue_epoch`の単一正本とし、`PlaybackQueueBacking.queue_identity`は参照するが所有しない | DVR data pathの`beginRead`/`beginWrite`/`commit`/`cancel`相当のtyped入口と、`QueueCleanupTxn`からのtyped flush requestだけを入口とする | 公開API、worker、`QueueCleanupTxn`が`queue_epoch`、queue pointer、token stateを直接変更する、または`PlaybackQueueBacking.queue_identity`の所有を二重化する、Filter stateを吸収する |
| `QueueCleanupTxn` | 既存`service_runtime/src/queue_cleanup_txn.rs::QueueCleanupTxn`。Filter/DVR固有stateはそれぞれ`FilterProducerDrainGate` / `QueueEpochProtocol`が所有する | Filter/DVR `flush()` object use-case | 下位protocol内部stateを直接変更する、API別に同じorchestration/failure aggregationを再実装する |
| `PlaybackConsumeTxn` | 既存`service_runtime/src/playback_consume_txn.rs` | playback workerから1 consume stepごとのtyped入口 | worker/FMQ helper/packet helperがread/parse/inject/consume遷移を再実装する |
| frontend tune/scan | `service_runtime/src/boot/frontend_txn.rs::FrontendTxn<'a>`。public use-case façadeは`service_runtime/src/frontend_ops.rs` | `aidl_service/src/tuner_service/frontend_methods.rs`からobject method façadeを経由してfrontend object use-caseを呼ぶ。full retune、`stopTune()`、scan切替でdemux境界が必要な場合は`StreamBoundaryTxn`のtyped入口だけを使用する | worker、device backend、callback delivery層によるfrontend公開状態・generation・rollback状態の直接確定、`FrontendTxn`自身によるassembler、FMQ、AV、record queue、demux stream generationの直接変更 |
| `AvSyncRegistry` | `demux/src/runtime/av_sync_registry.rs::AvSyncRegistry`。正方向は`media_filter_id -> hw_sync_id`のmany-to-one relationとし、reverse indexを物理保持する場合は`hw_sync_id -> Set<media_filter_id>`を同ownerで管理する。injective / bijectiveは要求しない | filter configure/unregister/close、demux closeからのprepared relation mutation。1 filterのunregisterは同一`hw_sync_id`を共有する他filterのrelationを維持する | API、filter wrapper、`StreamBoundaryTxn`がrelation/reverse indexを直接更新する、reverse indexを単一filterへ縮退する、PCR anchorを同ownerへ吸収する |
| `PcrClockAnchorStore` | `demux/src/runtime/pcr_clock_anchor.rs::PcrClockAnchorStore` | PCR観測と`StreamBoundaryTxn`からのprepared invalidation | APIまたは`StreamBoundaryTxn`がanchor内部を直接更新する、A/V sync relationを同ownerへ吸収する |
| `WorkerRuntime` / `WorkerHandle` | `service_runtime/src/worker_runtime.rs::{WorkerRuntime, WorkerHandle}` | 各domain worker ownerのspawn/stop/wake/join/reaper入口 | `WorkerLifecycleProtocol`等の別generic lifecycle ownerを追加する、domain start/stop state machineを共通runtimeへ吸収する |
| `WorkerFailureClassifier` | 既存`service_runtime/src/worker_failure_classifier.rs` | worker owner / cleanup manager / callback・backend失敗を扱うownerからtyped failureを入力し、分類結果だけを返す | classifierが停止順序、retry/cleanup、quarantine、公開/domain state transitionを直接変更する、owner側が文字列/errnoで再分類する |
| frontend worker終端 | 既存`service_runtime/src/frontend_worker_txn.rs`、worker slot/generationは`device/src/runtime/frontend_worker.rs`の`FrontendWorkerRegistry` | `service_runtime/src/frontend_ops.rs`および`service_runtime/src/boot/frontend_txn.rs`。close/owner-lossは`ObjectCloseTxn`からtyped cleanup commandとして接続し、失敗種別は`WorkerFailureClassifier`のtyped resultだけを受ける | worker自身によるowner unregister/lease返却、回収完了前のresource再利用、AIDL層によるjoin/reaper方針の決定、worker終了use-caseによる失敗種別の再分類、generic worker mechanismやclassifierの責務の再実装 |

##### 共通部品とAPI固有手順の合成規則

- `SourceBoundaryTxn`はFilter source/sink relationだけを所有する。demux/frontend relationは`DemuxFrontendSourceTxn`が所有する。
- relation transactionがstream data boundaryを必要とする場合、`StreamBoundaryTxn.prepare()`で変更不能な`PreparedStreamBoundary`を取得し、relation prepareとともに同じ上位transactionのcommit対象へ含める。pre-commit failureでは両方をabortし、旧relationと旧stream generationを維持する。relationだけまたはboundaryだけを先に公開してはならない。
- `StreamBoundaryTxn`はstream generation、continuity、section/PES/record-index parser/assembler boundaryとprepared invalidation dispatchだけを所有する。Filter/DVR queue内部、relation table、A/V sync map、PCR anchor内部、callback artifact、descrambler key/PIDを所有しない。
- callback登録ではAIDL façadeはBinder artifactのprepare/releaseだけを担当し、service_runtime側`CallbackRegistrationUseCase`がruntime registry mutationとdomain callback stateをprepareしてcomposite commit/rollback policyを所有する。prepared artifact handleはcomposite commitで採用し、AIDL façadeがdomain stateまたはrollback方針を持たない。`ILnb.setCallback()`専用のBinder artifact ownerを新設しない。
- `DescramblerPidTxn`は通常の`addPid()` / `removePid()`だけを所有し、key mutationとsession cleanupを吸収しない。
- Record DVR/Filter relationは`RecordDvrFilterRelationTxn`の一つのrelation stateを正本とし、DVR/Filter側の集合は同commitから導出する。
- `WorkerRuntime` / `WorkerHandle`はstop predicate、wake/cancel、generation fence、join、reaper handoffのmechanismだけを共通化し、Frontend/Filter/DVR/Playbackのdomain start/stop state machineを所有しない。
- `WorkerFailureClassifier`はstop/wake/join/EventFlag/Reaper/backend-control/callback等の失敗種別のtyped分類だけを共通化する。停止順序、retry/cleanup、quarantine、公開状態遷移は各worker owner/API側に残す。FMQ data通知EventFlagのpayload保持・再起床state machineはqueue runtimeが所有し、classifierはそのfailure categoryだけを返す。
- `PostCommitCallbackFailureTxn`はAPI名ではなくdomain commitとの相対時点で適用する。commit後だけを対象とし、domain stateをrollbackせずcallback health/diagnosticだけを更新する。
- Filter/DVR `flush()`は`QueueCleanupTxn`を共通orchestratorとして使用し、cleanup対象調停と失敗集約だけを共通化する。Filter固有stateは`FilterProducerDrainGate`、DVR固有stateは`QueueEpochProtocol`が独立して所有し、`QueueCleanupTxn`はtyped入口だけを使用する。
- `AvSyncRegistry`と`PcrClockAnchorStore`はprepared mutation/invalidation tokenを上位transactionへ返し、外側のfilter lifecycleまたは`StreamBoundaryTxn`のcommitと同じ排他区間で確定する。pre-commit failureではtokenをabortし、片側だけ更新しない。
- top-level cleanup / rollback use-caseは`CleanupExecutionReport` / `SharedCleanupDiagnostics`と共通failure-composition helperを通し、API別・worker別にfirst-error aggregation、primary/cleanup precedence、文字列detail合成を再実装しない。これらはcleanup結果の共通表現と合成部品であり、新しいlifecycle transaction ownerを意味しない。

最低試験は、`PostCommitCallbackFailureTxn`についてFilter/DVR双方でcommit済みstartを維持してrollbackしないこと、`FilterProducerDrainGate`についてflush中の新規permit拒否・全permit排出・commit前失敗時の状態不変、`QueueEpochProtocol`についてtokenの一回消費・flush drain後だけのepoch更新・flush commit前失敗時のqueue/epoch不変を固定する。

### ルートobject

`openFrontendById()`、`openDemux()`、`openDemuxById()`、`openDescrambler()`、`openLnbById()`、`openLnbByName()`は、同じroot open責務を使う。各APIのAIDL入出力を混成せず、入力IDを持つAPIでは公開IDを検証し、`openDemux()`と`openLnbByName()`ではobjectとout IDを同一確定点で公開する。`openDescrambler()`はdemux未結合のobject/session枠だけを生成し、demux IDと復号poolの選択は一回限りの`setDemuxSource()`へ委ねる。使用権予約、runtime登録、typed Binder object生成、失敗時の解放を一つの操作として扱い、objectを返した後に登録を巻き戻さない。

`getFrontendIds()`、`getFrontendInfo()`、`getLnbIds()`、`getDemuxIds()`、`getDemuxInfo()`、`getDemuxCaps()`、`getMaxNumberOfFrontends()`、`isLnaSupported()`は、起動時に確定した能力snapshotと現在の使用上限から応答する。照会中にprobeまたは能力の再選択を行わない。

### 子objectと関連付け

Filter、DVR、TimeFilterなどの子objectは、親demuxの生存、所有者、世代、能力、資源予約を確認してから登録する。Descramblerはrootで未結合objectを生成した後、`setDemuxSource()`で親demuxの生存、世代、能力、復号poolを検証し、一回だけ原子的に結合する。対応しないTimeFilterは`tuner_hal`の契約どおりobjectを生成しない。

`IFilter.setDataSource()`は`SourceBoundaryTxn`、`IDemux.setFrontendDataSource()`は`DemuxFrontendSourceTxn`、Record DVR接続は`RecordDvrFilterRelationTxn`、descrambler PID登録は`DescramblerPidTxn`を通す。frontendとLNBまたはCI CAMの接続は、両objectの所有者と世代を同じsnapshotで検証する。片側だけを確定した状態を通常状態として公開しない。

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

公開契約または実装構造の移行状態を追跡する既存表は、実装作業の判定に用いる。本PRで定義する共通化境界そのものの設計完了条件は`../tuner_hal/DESIGN_JA.md`の共通部品定義と各論理契約の自己整合性だけで判定し、実装追従、静的確認、異常系試験の実施状況をこの設計PRの完了条件へ混在させない。

| 適用単位 | 現在状態 | 実装追跡先 | 移行完了条件 |
|---|---|---|---|
| 公開object methodの検証順序とtransaction境界 | 設計済み・実装未適用 | `aidl_service/`、`service_runtime/`、`domain_request/` | 対象APIの入口が規範phase orderへ一本化され、状態不正と入力不正の優先順位、commit前後失敗、rollbackの試験が合格 |
| frontend tune/scan、再選局、終端deadline | 設計済み・実装未適用 | `service_runtime/`、`device/`、callback配送 | AOSP callback契約を満たす終端、`scan(K)→LOCKED(g1)→scan(K)→END(g2)`で2回目のbackend探索・LOCKED再配送がないこと、異なるscan request・`stopScan()`・`tune()`・`close()`で継続状態が失効すること、安定同一条件の非破壊re-entry、full retuneでの旧session遮断、破壊的commit後に旧要求を再投入しないこと、旧TSが新demux/filter世代へ混入しないこと、原因別の`Untuned`／`FailedBackend`／`FailedBoundary`／`Quarantined`遷移、deadlineの試験が合格 |
| Filter/DVR/AV/PESとFMQの資源契約 | 設計済み・実装未適用 | `demux/`、`fmq/`、`fmq_shim/`、`resource_ledger/` | `CapabilitySnapshot`からの予約、event-local/shared AV、processing buffer、overflow、close解放の試験が合格 |
| 自律cleanupとworker回収 | 設計済み・実装未適用 | `service_runtime/`、各worker owner、`resource_ledger/` | owner操作なしで再試行が進み、期限後の隔離またはservice-critical遷移とlease非再利用を試験で確認 |
| query snapshotとbackend適合 | 設計済み・実装未適用 | `service_runtime/`、`device/`、`config/` | queryがbackend I/Oを行わず、世代付きcacheの更新・失効とmanifest/probeによるbackend選択を試験で確認 |
| VTS/product profile | 設計保留 | `config/`、VTS XML、製品設定 | `VTS-ENV-01`から`06`の実測値を確定し、対応XML一式を静的選択して対象VTSを合格 |

## 構造上の禁止事項

- AIDL methodごとにclose、queue、rollback、quarantineの状態機械を複製しない。
- `../tuner_hal/DESIGN_JA.md`の共通部品適用表が所有者を指定した処理について、API別use-case、worker、helperが同じ状態変更、cleanup、失敗分類を個別再実装しない。
- `DropLeakTxn`を`ObjectCloseTxn`と並ぶcleanup authorityとして置かない。
- Demux frontend relationをFilter用`SourceBoundaryTxn`へ吸収しない。
- relation transactionと`StreamBoundaryTxn`を別々の公開commitにしない。
- Filter/DVR `flush()`のcleanup orchestrationと失敗集約をAPI別に複製せず、`QueueCleanupTxn`のtyped入口を使用する。
- `WorkerLifecycleProtocol`等を`WorkerRuntime` / `WorkerHandle`と並ぶgeneric lifecycle ownerとして置かない。
- worker owner/APIがstop/wake/join/EventFlag/Reaper/backend-control/callback等の同型失敗分類を個別実装せず、`WorkerFailureClassifier`のtyped結果を使用する。
- LNB Binder callback実体をLNB domain/AIDL objectに直接保持しない。
- DVR側とFilter側がRecord relationを別々にcommitしない。
- A/V sync relation / reverse indexまたはPCR anchorを複数ownerが直接変更しない。
- `tuner_hal`で定義した公開戻り値を`service_runtime`またはbackendで別の値へ読み替えない。
- AIDL objectまたはcallback実体をdemux、device、resource ledgerへ渡さない。
- 静的inventory／capability queryからcleanup、worker操作、backend I/Oを開始しない。動的frontend status queryは現行製品では世代付き`FrontendStatusSnapshot`だけを読む。ただし、`../tuner_hal/DESIGN_JA.md`の公開状態表で対象status、bounded I/O上限、失敗写像、generation再検証、snapshot更新との排他を明示したstatusはbounded synchronous readへ変更できる。
- file名またはtype名をAOSP公開契約、ARIB根拠、公開状態遷移の値そのものとして扱わない。
- `共通transaction / use-caseの規範実装アンカー`以外の物理配置表を状態遷移の正本として扱わない。
- 規範実装アンカーのrename、split、merge時に旧アンカーを残したまま新アンカーを追加し、複数のtransaction正本を作らない。

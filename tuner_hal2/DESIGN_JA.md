# tuner_hal2 DESIGN_JA.md

この文書は、tv直下 `開発規則.md` が許可した `tuner_hal2` 固有の構造差分だけを記載する。既存 `tuner_hal/DESIGN_JA.md` と同じ公開契約、状態遷移、戻り値、資源寿命、`WorkerExit` / `WorkerFailureClassifier` / `ScanSessionTxn` の論理契約は再定義しない。

## 1. worker構造差分

`tuner_hal2` では、frontend単位のworker slotを `FrontendWorkerRegistry` が所有する。これは既存契約名である `WorkerExit` を置き換える正本ではなく、tuner_hal2内部でfrontend workerを探すためのslot所有構造である。

| 境界 | tuner_hal2の実体 | 既存契約との関係 |
|---|---|---|
| worker種別 | `FrontendWorkerKind::{Tune, Scan}` | `WorkerOwnerKind` のfrontend系所有者をtuner_hal2内で分けるための構造差分 |
| 停止要求 | `FrontendWorkerCancelReason` | `WorkerStopReason` へ写像するtuner_hal2内部入力 |
| 停止操作結果 | `FrontendWorkerStopOutcome` | stop要求APIの戻り値。終了分類の正本ではない |
| 終了分類 | `WorkerExit` | 既存契約名をそのまま使う |
| 失敗分類 | `WorkerFailureClassifier` | 既存契約名をそのまま使う |

## 2. ScanSession構造差分

`tuner_hal2` では、既存 `ScanSessionTxn` 論理契約に対応する内部状態正本として `FrontendScanSession` を置く。

| 境界 | tuner_hal2の実体 | 既存契約との関係 |
|---|---|---|
| session owner | `FrontendScanSession` | active session、generation、fingerprintをfrontend runtime内で所有する構造差分 |
| 候補進行 | `current_candidate()` / `advance_after_candidate()` | scan candidate進行をsession状態へ閉じる |
| 置換 | `SupersededByNewRequest` terminal化 | 旧scan停止後に新generationを開始する契約をtuner_hal2のworker slotへ接続する |
| 停止 | `StopRequested` terminal化 | `stopScan()` 由来の停止理由をsessionへ残す |
| 終端理由 | `FrontendScanTerminalReason` | `END` / cancel / backend失敗 / callback失敗 / panicをScanSession内で区別する構造差分 |

## 3. live path構造差分

`tuner_hal2` では、device側descriptor、pump owner、packet sinkを分ける。

| 境界 | tuner_hal2の実体 | 既存契約との関係 |
|---|---|---|
| descriptor | `FrontendLiveReaderDescriptor` | 読取元fd/pathの説明だけを持つ |
| pump owner | `FrontendLivePumpOwner` | thread handle、cancel、join、reportを所有する |
| packet sink | `FrontendLivePacketSink` | demux側配送先を抽象化する |
| stop結果 | `FrontendLivePumpReport` | stop/join後のpacket数、malformed byte、cancel/EOFを返す |

## 4. demux依存境界

`tuner_hal2` のfrontend runtimeは、demux runtimeを所有しない。bound demux quarantine、demux unbind、attached demux stop notification、demux sinkの実体はdemux側runtimeの責務であり、frontend側では構造境界だけを持つ。

## 5. AIDL object lifecycle 構造差分

本節は既存 `tuner_hal/DESIGN_JA.md` の close / cleanup failed / Drop leak / quarantine 契約を再定義しない。`tuner_hal2` 内で、その既存契約に対応する実体名を固定するための構造差分だけを記載する。

| 既存契約上の境界 | tuner_hal2の実体 | 既存契約との関係 |
|---|---|---|
| public close lifecycle | `aidl_service::object_runtime::close_object()` と `RuntimeObjectTable::{begin_close_cascade, mark_cleanup_failed_cascade, commit_close_cascade}` | public close の object table 遷移と runtime unregister の入口 |
| Drop leak terminalization | `aidl_service::object_runtime::drop_leak_object()` と `RuntimeObjectTable::quarantine_cascade()` | Drop は通常 cleanup の代替にせず、live object と descendant を quarantine へ落とす入口 |
| 単一 object quarantine 遷移 | `RuntimeObjectTable` 内の private helper | 外部から直接呼ばせない。object lifecycle の公開入口は cascade 経路へ統一する |
| callback実体 cleanup | `aidl_service::callback_store` と `RuntimeCallbackRegistry` | callback object実体の保持・削除はAIDL層が正本。backend trait に callback cleanup を持たせない |
| runtime unregister | `TunerServiceRuntime::unregister_public_runtime_for_closed_aidl_entry()` | close / Drop leak の object table 終端後にだけ呼ぶ派生処理 |

禁止する構造差分:

- AIDL object 種別ごとに Drop cleanup 処理をコピー実装しない。
- 単一 object quarantine を外部公開入口として残さない。
- Drop 経路で public close と同じ通常 cleanup を実行しない。
- callback store cleanup を LNB backend / profile backend / device backend の責務へ戻さない。
- close / Drop leak の runtime unregister を object table 終端前に実行しない。

## 6. AIDL / service_runtime のファイル分割境界

本節は巨大制御層を再発させないための tuner_hal2 固有の配置規則である。`include!` による見かけ上の分割は禁止し、Rust の通常 `mod` による module 分割だけを使う。

### 6.1 AIDL 層

`aidl_service/src/tuner_service.rs` は root `ITuner` service、root object open/query だけを所有する。AIDL object lookup、unsupported planning、source filter handle 検証などの service-level helper は `aidl_service/src/tuner_service/support.rs` へ分ける。child object の公開 AIDL trait 実装は `aidl_service/src/tuner_service/*_methods.rs` へ分ける。

| ファイル | 所有する実装 | 禁止事項 |
|---|---|---|
| `aidl_service/src/tuner_service.rs` | `TunerAidlService`、`ITuner`、root open/query | child trait 実装と service-level helper を戻さない |
| `aidl_service/src/tuner_service/support.rs` | AIDL object lookup、local binder downcast、unsupported planning、source filter owner/public id helper | AIDL trait 実装、runtime状態遷移、Binder status helper再定義を置かない |
| `aidl_service/src/tuner_service/frontend_methods.rs` | `impl IFrontend for FrontendAidlObject` | runtime registry の直接所有を増やさない |
| `aidl_service/src/tuner_service/demux_methods.rs` | `impl IDemux for DemuxAidlObject` | filter/DVR/descrambler 状態遷移を直接所有しない |
| `aidl_service/src/tuner_service/filter_methods.rs` | `impl IFilter for FilterAidlObject` | callback/FMQ/AV cleanup failure を空消費しない |
| `aidl_service/src/tuner_service/dvr_methods.rs` | `impl IDvr for DvrAidlObject` | FMQ/EventFlag commit 条件を局所実装しない |
| `aidl_service/src/tuner_service/descrambler_methods.rs` | `impl IDescrambler for DescramblerAidlObject` | token / PID lifetime を AIDL 層で所有しない |
| `aidl_service/src/tuner_service/lnb_methods.rs` | `impl ILnb for LnbAidlObject` | LNB backend safe-state apply を Drop 経路へ戻さない |

AIDL method body は `ensure_open()`、typed method planning、AIDL input の domain request 変換、service_runtime use-case 呼び出し、`error_bridge` による Binder status 変換だけを行う。runtime registry / object table / callback registry の状態遷移を AIDL method body へ新規追加する場合は、対応する service_runtime use-case function を先に追加する。

AIDL 層から `service_runtime::frontend_worker_txn` や `service_runtime::boot/*_txn.rs` を直接 import しない。frontend tune / scan / stop / close の worker 境界は service_runtime の public frontend use-case façade を呼ぶ。AIDL object handle から public runtime id / owner relation を解決する処理は service_runtime query façade を通し、AIDL helper から `RuntimeObjectTable` を直接参照しない。

通常の supported public API planning は `AidlMethodCall::PublicApi` を使う。unsupported-by-design API の戻り値生成だけ `AidlMethodCall::UnsupportedPublicApi` を使う。query / open / 状態取得系の supported API を unsupported planning に流用しない。

### 6.2 service_runtime 層

#### 6.2.1 `boot.rs` の責務

`service_runtime/src/boot.rs` は `TunerServiceRuntime` 定義、boot/probe、object table / callback registry / diagnostic accessor、command dispatch を所有する。`service_runtime/src/boot.rs` に通常 operation を追加してはならない。`TunerServiceRuntime` の field は private のままとし、operation module へ渡す目的で `pub(crate)` 化しない。

#### 6.2.2 top-level `*_ops.rs` の責務

公開 operation wrapper は top-level `service_runtime/src/*_ops.rs` へ置く。top-level `*_ops.rs` は `TunerServiceRuntime` の private field に直接触れず、`boot` child module の domain transaction context または read-only query wrapper を呼ぶだけにする。

| ファイル | 所有する公開 wrapper | 呼び出す context |
|---|---|---|
| `service_runtime/src/frontend_ops.rs` | frontend runtime / scan / live reader / worker lifecycle | `FrontendTxn<'_>` |
| `service_runtime/src/demux_filter_dvr_ops.rs` | demux/filter/DVR allocation/register/configure/start/stop/flush/source/DVR | `DemuxFilterDvrTxn<'_>` |
| `service_runtime/src/descrambler_ops.rs` | descrambler allocation/demux/key/PID/unregister/owner-loss cleanup | `DescramblerTxn<'_>` |
| `service_runtime/src/packet_ops.rs` | packet ingress / demux binding | `PacketTxn<'_>` |
| `service_runtime/src/lnb_ops.rs` | LNB binding / apply / lifecycle / callback / drop leak | `LnbTxn<'_>` |

#### 6.2.3 `boot/*_txn.rs` の責務

状態変更 transaction は `service_runtime/src/boot/*_txn.rs` へ置く。`boot/*_txn.rs` は domain transaction context を定義し、registry / frontend worker / diagnostics / key table / stream boundary などの private field 操作を所有する。top-level `*_ops.rs` は平坦な `transact_*` helper を直接呼ばず、domain transaction context の method を呼ぶ。

| ファイル | 所有する transaction context | 所有する状態変更 |
|---|---|---|
| `service_runtime/src/boot/frontend_txn.rs` | `FrontendTxn<'a>` | frontend runtime / scan / live reader / worker lifecycle の状態変更 |
| `service_runtime/src/boot/demux_filter_dvr_txn.rs` | `DemuxFilterDvrTxn<'a>` | demux/filter/DVR allocation/register/configure/start/stop/flush/source/DVR の状態変更 |
| `service_runtime/src/boot/descrambler_txn.rs` | `DescramblerTxn<'a>` | descrambler allocation/demux/key/PID/unregister/owner-loss cleanup の状態変更 |
| `service_runtime/src/boot/packet_txn.rs` | `PacketTxn<'a>` | frontend TS packet ingress、demux source boundary、descrambler packet policy、packet diagnostics の状態変更 |
| `service_runtime/src/boot/lnb_txn.rs` | `LnbTxn<'a>` | LNB binding、runtime state apply、callback registration commit、lifecycle close、drop leak recording の状態変更 |

`transact_*` helper は `boot/*_txn.rs` 内の実装補助として扱う。top-level `service_runtime/src/*_ops.rs` から `transact_*` を直接呼んではならない。`query_api.rs` から mutating transaction context または `transact_*` を呼んではならない。

#### 6.2.4 `query_api.rs` の責務

状態を変更しない参照系 API は `service_runtime/src/boot/query_api.rs` へ置く。`query_api.rs` は `RuntimeQuery<'a>` を定義し、read-only query は `RuntimeQuery<'a>` の method として実装する。`RuntimeQuery<'a>` は必要な read-only source だけを immutable reference として保持する。現行実装では runtime registry と object table の immutable reference だけを保持する。将来 frontend worker 等の read-only source を参照する場合も immutable reference に限定し、状態変更、cleanup、rollback、quarantine、worker stop/start を行わない。AIDL object handle から public runtime id / owner relation を解決する read-only query はここへ置く。

`TunerServiceRuntime` に残る参照系 method は `self.query()` で `RuntimeQuery<'_>` を生成し、その method へ委譲する wrapper とする。

LNB profile/backend policy は `ServiceRuntimeLnbProfileAdapter` が `LnbBackendOps` へ適合させる。これは実 backend I/O ではなく、service_runtime の registry/profile 状態を domain transaction へ渡す adapter である。

#### 6.2.5 禁止事項

- `service_runtime/src/boot.rs` に通常 operation を追加しない。
- `TunerServiceRuntime` field を operation module へ渡す目的で `pub` / `pub(crate)` 化しない。
- top-level `service_runtime/src/*_ops.rs` から `registry` / `frontend_workers` / `callback_registry` / `object_table` / `diagnostics` / `descrambler_diagnostics` を直接参照しない。
- top-level `service_runtime/src/*_ops.rs` から平坦な `transact_*` helper を直接呼ばない。
- AIDL 層から `service_runtime::frontend_worker_txn`、`service_runtime::boot/*_txn.rs`、`RuntimeObjectTable` を直接参照しない。
- `query_api.rs` から mutating transaction context または `transact_*` を呼ばない。
- `RuntimeQuery<'a>` に mutable reference を持たせない。
- `TunerServiceRuntime::registry_mut()` を呼んでよい production code は `service_runtime/src/boot/*_txn.rs` の domain transaction implementation に限る。top-level `service_runtime/src/*_ops.rs`、AIDL 層、device/demux/descrambler/lnb domain crate、`query_api.rs` から `registry_mut()` を呼んではならない。registry 変更は `boot/*_txn.rs` の domain transaction context に閉じる。
- `#[path]` / `include!` / `include_str!` を使わない。
- production code の file split module では `use super::*;` を使わない。親 module から必要な型・関数を使う場合は `use super::{...};` で明示する。

## 7. Drop leak / callback cleanup の共通部品境界

Drop 経路は public close の代替ではない。全 AIDL object の `Drop` は `drop_leak_object()` を呼ぶだけにする。object 種別固有の追加記録が必要な場合も、`Drop` 実装へ個別手順を書かず、`DropLeakDomainAction` と service_runtime 側 domain hook に閉じ込める。

callback cleanup は次の規則に従う。

- public close / rollback では `clear_owner_callback_registration()` を使い、callback store cleanup 失敗を `RuntimeCallbackRegistry` の unhealthy と Binder error へ接続する。
- Drop leak では `drop_leak_object()` が callback store clear、runtime callback registry clear/unhealthy、object table quarantine、runtime unregister をまとめて扱う。
- `best_effort` 名の callback cleanup helper を追加しない。
- callback cleanup failure を空消費しない。
- LNB だけを例外にして Drop cleanup 手順をコピー実装しない。LNB固有の drop leak 記録は `DropLeakDomainAction::RecordLnbDropLeak` でだけ表現する。

## 8. Frontend worker poison / stop outcome 境界

`FrontendWorkerContext::cancel_reason()` は lock poison を `None` や正常終了へ丸めない。cancel reason lock poison は `HalError::Internal(InvariantViolation)` と `WorkerExit::RuntimeFailure(Signal)` に写像する。

`FrontendWorkerRegistry::request_stop()` / `request_stop_and_join()` は、cancel reason lock に書けなかった場合、cancel flag だけを立てて成功扱いしない。`FrontendWorkerStopOutcome::StopRequestFailed` を返し、所有側は対象 frontend / scan session / live data の状態を未停止または failed として扱う。

## 9. AIDL status bridge の所有者

AIDL status 変換は次に固定する。

| 層 | 責務 |
|---|---|
| `binder_adapter::status` / `AidlStatusMapper` | `HalError` / domain failure から `TunerStatusCode` への純粋写像 |
| `aidl_service::error_bridge` | `TunerStatusCode` / `HalError` から `binder::Status` への唯一の変換点 |

禁止事項:

- `Status::new_service_specific_error()` を `aidl_service::error_bridge` 以外で直接呼ばない。
- `status_from_hal_error` / `status_from_tuner_status` / `service_error` 相当の helper を `tuner_service.rs`、object wrapper、child open helper、runtime helper へ再定義しない。
- `android.hardware.tv.tuner::Result` の整数値を、Binder status 生成目的で `error_bridge` 以外へ拡散しない。例外は `error_bridge::service_error()` の呼び出し引数として渡す場合だけである。


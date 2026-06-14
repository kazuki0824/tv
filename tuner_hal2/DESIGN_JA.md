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
| Drop leak terminalization | `aidl_service::object_runtime::quarantine_live_aidl_object_after_drop_leak()` と `RuntimeObjectTable::quarantine_cascade()` | Drop は通常 cleanup の代替にせず、live object と descendant を quarantine へ落とす入口 |
| 単一 object quarantine 遷移 | `RuntimeObjectTable` 内の private helper | 外部から直接呼ばせない。object lifecycle の公開入口は cascade 経路へ統一する |
| callback実体 cleanup | `aidl_service::callback_store` と `RuntimeCallbackRegistry` | callback object実体の保持・削除はAIDL層が正本。backend trait に callback cleanup を持たせない |
| runtime unregister | `TunerServiceRuntime::unregister_public_runtime_for_closed_aidl_entry()` | close / Drop leak の object table 終端後にだけ呼ぶ派生処理 |

禁止する構造差分:

- AIDL object 種別ごとに Drop cleanup 処理をコピー実装しない。
- 単一 object quarantine を外部公開入口として残さない。
- Drop 経路で public close と同じ通常 cleanup を実行しない。
- callback store cleanup を LNB backend / profile backend / device backend の責務へ戻さない。
- close / Drop leak の runtime unregister を object table 終端前に実行しない。

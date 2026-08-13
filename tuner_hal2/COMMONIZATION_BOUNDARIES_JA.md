# tuner_hal2 共通化境界・実装構造正本

## 1. 目的と優先順位

この文書は、`tuner_hal/COMMONIZATION_BOUNDARIES_JA.md` で定義した共通化境界を `tuner_hal2` の実装構造へ写像するための **規範正本**である。

対象は state owner、transaction/protocol の責務、module anchor、許可された呼出し経路、禁止された直接更新である。

本書と `tuner_hal2/DESIGN_JA.md` の既存記述が、CMB-01〜CMB-11 に関して異なる owner、異なる transaction 粒度、または異なる failure authority を示す場合は **本書を優先する**。既存設計書中の `QueueCleanupTxn`、広義の `WorkerFailureClassifier`、`DropLeakTxn` 相当の記述は、本書の境界に従って読み替える。

ここに記す module path は実装構造上の設計 target であり、現在のソースコードに存在するという主張ではない。本設計の作成・レビューでは実装ソースを前提にしない。

## 2. 共通化粒度の規則

共通 transaction は次の 4 要素が一致する範囲に限定する。

- state owner
- commit boundary
- rollback authority
- failure semantics

phase の形だけが同じ操作は transaction として統合しない。lock、artifact store、result composition、wake/join/reaper、typed error conversion 等、mechanism だけが同じ場合は protocol/helper/facade として共有する。

## 3. 規範 module anchor

| 論理契約 | 規範 module anchor | 所有するもの | 所有しないもの |
|---|---|---|---|
| `ObjectCloseTxn` | `service_runtime/src/object_close_txn.rs` | public close / owner loss / Drop cleanup の one-shot authority、cleanup plan 実行、全step継続、pending/retry、result report、reaper handoff | 各 object 固有の cleanup 項目・公開 status 意味 |
| `SourceBoundaryTxn` | `domain/src/source_boundary_txn.rs` | Filter source relation の validate/prepare/commit/rollback | demux/frontend relation、parser/assembler state |
| `DemuxFrontendSourceTxn` | `domain/src/demux_frontend_source_txn.rs` | Demux<->Frontend relation の validate/prepare/commit/rollback、relation record | parser/assembler/continuity state |
| `StreamBoundaryTxn` | `domain/src/stream_boundary_txn.rs` | stream generation、parser/assembler/continuity boundary、typed invalidation dispatch | source relation ownership、A/V sync map、PCR anchor 内部 |
| `LnbControlTxn` | `domain/src/lnb_control_txn.rs` | persistent LNB control command の lock/candidate/backend apply/registry commit/failure transition | DiSEqC message の transient send、callback artifact |
| callback artifact facade/store | `aidl_service/src/callback_store.rs` + callback registration facade | Binder strong ref、death recipient、replace/release | domain callback identity/generation |
| `DescramblerKeyTxn` | `domain/src/descrambler_key_txn.rs` | key apply/replace/refcount/rollback | PID relation mutation |
| `DescramblerPidTxn` | `domain/src/descrambler_pid_txn.rs` | PID add/remove backend apply、relation commit、compensation、quarantine decision | key lifecycle、session teardown |
| `DescramblerSessionCleanupTxn` | `domain/src/descrambler_session_cleanup_txn.rs` | session teardown、demux/key/PID cleanup coordination | normal key/PID mutation |
| `RecordDvrFilterRelationTxn` | `domain/src/record_dvr_filter_relation_txn.rs` | Record DVR<->Filter attach/detach/unlink relation、冪等性、両側 lifecycle validation | DVR/Filter 本体 lifecycle state machine |
| `WorkerLifecycleProtocol` | `service_runtime/src/worker_lifecycle.rs` | stop predicate、wake/cancel、generation fence、join、reaper handoff | domain start/stop state transition、backend failure classification |
| `WorkerFailureClassifier` | `service_runtime/src/worker_failure_classifier.rs` | worker infrastructure failure の typed classification | backend/device error、callback delivery error |
| `PostCommitCallbackFailureTxn` | `service_runtime/src/post_commit_callback_failure_txn.rs` | commit 後 callback delivery failure 時の health/diagnostic update | domain commit/rollback |
| `FilterFlushTxn` | `domain/src/filter_flush_txn.rs` | Filter flush eligibility と orchestration、producer drain、Filter queue/event/parser/AV pending cleanup | DVR queue epoch/token state |
| `DvrFlushTxn` | `domain/src/dvr_flush_txn.rs` | Record/Playback flush eligibility と orchestration、FMQ token/queue、DVR parser/stats、queue epoch commit | Filter producer/callback state |
| flush result helpers | `domain/src/cleanup_result.rs` 等 | result aggregation、checked cleanup/result composition | Filter/DVR の transaction commit authority |
| `AvSyncRegistry` | `domain/src/av_sync_registry.rs` | `filter_id <-> hw_id` 双方向 relation | PCR clock anchor |
| `PcrClockAnchorStore` | `domain/src/pcr_clock_anchor_store.rs` | generation-scoped PCR anchor | stream boundary generation 自体 |
| `PlaybackConsumeTxn` | 既存規範 anchor | Playback read/inject consume state machine | flush orchestration |
| `FilterProducerDrainGate` | 既存規範 anchor | Filter producer drain protocol | DVR queue semantics |
| `QueueEpochProtocol` | 既存規範 anchor | DVR playback queue epoch protocol | Filter flush semantics |

新規 module anchor は、既存 source の存在を仮定せず、設計上の責務配置を一意化するための名前である。

## 4. CMB-01 Object close / Drop

### 許可される入口

- AIDL public `close()` facade
- owner loss handling
- Drop fallback
- service shutdown / reaper retry

### 規則

各 object は `CleanupPlan` 相当の typed plan を `ObjectCloseTxn` へ渡す。plan は object 固有の cleanup command と順序を表すが、step runner そのものを object ごとに持たない。

`ObjectCloseTxn` は一度だけ close authority を取得し、途中 step が失敗しても残 cleanup を続行し、結果を `CleanupExecutionReport` 相当へ集約する。未完了 resource は pending cleanup として同じ authority へ残す。

Drop は別 transaction を作らず、origin=`OwnerLoss/Drop` を付けて `ObjectCloseTxn` へ委譲する。`DropLeakTxn` は規範契約名として使用しない。

## 5. CMB-02 Demux frontend source

`DemuxFrontendSourceTxn` は Filter 用 `SourceBoundaryTxn` から独立させる。両者は relation state owner と failure semantics が異なるため、同じ transaction に統合しない。

処理順は概念上次とする。

1. demux/frontend lifecycle と generation を検証する。
2. new relation を prepare する。
3. data-path boundary が必要なら `StreamBoundaryTxn` に typed request を発行する。
4. relation record を commit する。
5. commit 前失敗では relation prepare を rollback する。

`StreamBoundaryTxn` は relation record を書かず、`DemuxFrontendSourceTxn` は parser/assembler generation を直接書かない。

## 6. CMB-03 LNB callback artifact

LNB Binder callback object の strong reference / death handling は callback store/facade のみが所有する。

`LnbRegistry` / LNB domain state に保持してよいのは logical callback id、generation、registration state のみである。replace/remove/close は callback facade を経由する。

`LnbHal` 等の AIDL object が Binder callback artifact の独立 owner になってはならない。

## 7. CMB-04 LNB persistent controls

`LnbControlTxn` は `Voltage`、`Tone`、`SatellitePosition` の typed command を受ける。

共通 state machine:

`acquire operation authority -> read old registry state -> construct candidate -> backend apply -> registry commit -> classify/transition failure`

backend apply 後の commit 不成立を含む failure semantics は一か所に置く。

`sendDiseqcMessage()` は persistent candidate/registry commit を共有しないため、この transaction の command variant にしない。

## 8. CMB-05 Descrambler PID

`DescramblerPidTxn` は `Add(pid, filter)` / `Remove(pid, filter)` の typed operation を受ける。

所有する処理:

- lifecycle/demux ownership validation
- backend packet-path apply/prepare
- PID relation ledger commit
- compensation rollback
- compensation 不成立時の quarantine decision

`DescramblerKeyTxn` と state owner が異なるため統合しない。

## 9. CMB-06 Record DVR / Filter relation

Record DVR<->Filter relation table は `RecordDvrFilterRelationTxn` が単一 owner となる。

attach/detach、Filter close、DVR close、Demux cleanup はすべて typed mutation request で同 owner を通す。

relation の両側 shadow copy を独立 commit してはならない。必要な派生集合や Record DVR の filter union は relation commit と同じ transaction boundary から再計算/更新する。

## 10. CMB-07 Worker lifecycle と failure classification

### WorkerLifecycleProtocol

共通化するのは次の mechanism に限定する。

- owner generation fence
- stop predicate/token
- wake/cancel primitive
- join-from-owner
- join 不能時の reaper handoff

Frontend/Filter/DVR/Playback の domain state transition は各 domain transaction が所有する。

### WorkerFailureClassifier

分類対象は worker infrastructure failure のみ:

- `WorkerPanic`
- `JoinFailure`
- `StopWakeFailure`
- `EventFlagWakeFailure`
- reaper/termination infrastructure failure

backend adapter/device I/O/control failureは backend/domain typed error のまま保持する。callback failure は callback layer に残す。

## 11. CMB-08 Post-commit callback failure

`PostCommitCallbackFailureTxn` は API 名ではなく **commit relative timing** で適用を決める。

適用例:

- Filter/DVR `start()` commit 後 callback
- Frontend tune commit 後 callback
- DVR status/state callback の commit 後 delivery

この transaction は domain state を rollback せず、callback health、diagnostic、必要な callback suppression state のみを更新する。

commit 前 failure は各 domain transaction の通常 failure path で扱う。

## 12. CMB-09 A/V sync / PCR

### AvSyncRegistry

`filter_id -> hw_id` と `hw_id -> filter_id` を一体 state として所有する。insert/delete は両方向 map の同一 commit で行う。

Filter unregister/reconfigure/close、Demux close なども registry operation として削除する。

### PcrClockAnchorStore

PCR anchor は filter generation を key に所有する。anchor invalidation を要求できるのは stream boundary event 等だが、内部 map/state の mutation authority は store 自身に限る。

`StreamBoundaryTxn` は typed `InvalidatePcrAnchor{filter_id,generation,reason}` 相当を発行し、store 内部を直接触らない。

## 13. CMB-10 Flush transaction の分離

旧共有 `QueueCleanupTxn` を transaction-level owner として使用しない。

### FilterFlushTxn

Filter 固有の以下を所有する。

- Filter flush の lifecycle eligibility
- `FilterProducerDrainGate` による producer drain
- Filter FMQ/queue/event/pending callback/AV pending state cleanup
- Filter parser generation/reset
- Filter flush commit/failure semantics

### DvrFlushTxn

DVR 固有の以下を所有する。

- record/playback ごとの flush eligibility
- outstanding FMQ read/write token の扱い
- DVR queue cleanup
- playback/record assembler/parser reset
- `QueueEpochProtocol` の epoch commit
- DVR statistics/readback state の境界処理

両者は low-level queue helper、cleanup result type、error composition helper を共有してよいが、transaction-level commit/rollback authority は共有しない。

## 14. CMB-11 WorkerFailureClassifier の禁止入力

次を `WorkerFailureClassifier` へ入力してはならない。

- backend tune/control/apply error
- device read/write/ioctl error
- FMQ protocol/domain validation error
- callback delivery error
- descrambler/LNB/domain transaction failure

これらは発生 layer の typed error が原因を保持し、必要な公開 status mapping は該当 domain transaction/facade が行う。

## 15. 依存方向

許可される依存は概念上次とする。

```text
AIDL facade
  -> domain transaction / relation transaction
  -> protocol/helper/facade
  -> backend adapter / registry / artifact store
```

特例として、relation transaction が stream data boundary を必要とする場合は typed request で `StreamBoundaryTxn` を呼べる。ただし相互に state を直接変更しない。

`ObjectCloseTxn` は object 固有 `CleanupPlan` を実行するが、各 cleanup command の domain owner を奪わない。たとえば Record DVR/Filter unlink は `RecordDvrFilterRelationTxn`、callback artifact release は callback facade を command として呼ぶ。

## 16. 禁止する実装構造

- public AIDL object に独立した close step runner を置くこと。
- `DropLeakTxn` を別 cleanup authority として置くこと。
- Demux frontend relation を Filter 用 `SourceBoundaryTxn` に吸収すること。
- `SourceBoundaryTxn` / `DemuxFrontendSourceTxn` が parser/assembler state を直接変更すること。
- LNB Binder callback object を LNB domain/AIDL object に直接保持すること。
- 3 LNB persistent control API に同じ backend+registry state machine を複製すること。
- descrambler add/remove PID に同じ compensation/quarantine state machine を複製すること。
- DVR側とFilter側が Record relation を別々にcommitすること。
- generic worker protocol が domain start/stop state machine を所有すること。
- `WorkerFailureClassifier` が backend/callback failure を分類すること。
- Filter/DVR flush を一つの `QueueCleanupTxn` で orchestration すること。
- A/V sync 双方向 map または PCR anchor を複数 owner が直接変更すること。

## 17. 設計整合条件

`tuner_hal/COMMONIZATION_BOUNDARIES_JA.md` と本書は、CMB-01〜CMB-11について次を一致させる。

1. logical owner 名
2. state ownership
3. commit / rollback authority
4. failure semantics
5. transaction と protocol/helper の境界
6. 禁止する直接 mutation 経路

既存 `DESIGN_JA.md` の API 別手順はこの責務配置に従う公開要求記述として解釈し、同じ状態を所有する別 transaction を新設してはならない。

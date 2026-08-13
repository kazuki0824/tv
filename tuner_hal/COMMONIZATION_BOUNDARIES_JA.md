# Tuner HAL 共通化境界契約

## 1. この文書の位置づけ

この文書は `tuner_hal/DESIGN_JA.md` に対する **共通化境界の規範正本**である。対象は、複数 API / lifecycle / worker / callback / relation にまたがる状態変更の owner、commit authority、rollback authority、cleanup authority、failure classification の境界に限る。

本書と `tuner_hal/DESIGN_JA.md` の既存記述が、下記 11 項目について異なる owner または異なる共通化粒度を示す場合は、**本書を優先する**。既存 API 節に残る手順列は、公開意味・対象・順序・戻り値を説明するための要求であり、状態変更を実行する第二の正本とは解釈しない。

公開 AIDL / VTS / CDD / ARIB 上の意味論は変更しない。本書は内部の責務分離のみを規定する。

## 2. 共通 transaction と共通 mechanism の判定規則

複数操作を同一の **transaction** として共通化してよいのは、少なくとも次の 4 条件が一致する場合だけである。

1. 状態 owner が同じである。
2. commit boundary が同じである。
3. rollback authority が同じである。
4. failure semantics が同じである。

`validate -> prepare -> external apply -> commit` のような phase 形状が似ているだけでは、同一 transaction にしてはならない。

lock、結果集約、callback artifact 保持、worker wake/join/reaper handoff、typed error composition のように **mechanism だけが共通**する場合は、protocol / helper / facade として共通化する。異なる domain state machine を 1 個の transaction に吸収してはならない。

## 3. 規範 owner 一覧

| 対象 | 規範 owner / protocol | 規範上の境界 |
|---|---|---|
| public `close()` / owner loss / Drop cleanup | `ObjectCloseTxn` | API/object は cleanup plan と公開結果を定義する。実行 authority、全 step 継続、未完了管理、retry/reaper handoff は `ObjectCloseTxn` が所有する。`DropLeakTxn` を独立 authority として設けない。 |
| Filter source relation | `SourceBoundaryTxn` | Filter の source linkage の validate/prepare/commit/rollback を所有する。 |
| Demux frontend source relation | `DemuxFrontendSourceTxn` | `IDemux.setFrontendDataSource()` の relation lifecycle を所有する。必要な data-path 境界は `StreamBoundaryTxn` に typed request を 1 回だけ委譲する。 |
| stream/parser/assembler continuity boundary | `StreamBoundaryTxn` | parser/assembler/generation/continuity の境界だけを所有し、relation owner にはならない。 |
| LNB persistent controls | `LnbControlTxn` | `setVoltage()` / `setTone()` / `setSatellitePosition()` の lock、candidate、backend apply、registry commit、failure transition を typed command で共通化する。 |
| LNB callback artifact | callback artifact facade/store | Binder callback の strong reference / death handling / replacement / release を所有する。LNB domain は logical callback identity/generation のみを所有する。 |
| descrambler key | `DescramblerKeyTxn` | key token の apply/replace/refcount/rollback を所有する。 |
| descrambler PID | `DescramblerPidTxn` | `addPid()` / `removePid()` の backend apply、PID relation commit、compensation、quarantine 判定を typed operation で所有する。 |
| descrambler session cleanup | `DescramblerSessionCleanupTxn` | demux/key/PID/session cleanup を所有する。 |
| Record DVR <-> Filter relation | `RecordDvrFilterRelationTxn` | attach/detach と両側 close からの unlink を同じ relation owner で commit する。 |
| worker lifecycle mechanism | `WorkerLifecycleProtocol` | stop predicate、wake/cancel、generation fence、join、reaper handoff を共通 mechanism として所有する。各 domain の start/stop state machine は所有しない。 |
| worker infrastructure failure classification | `WorkerFailureClassifier` | panic/join/wake/EventFlag/reaper 等、worker infrastructure failure だけを分類する。backend/device/callback failure は分類しない。 |
| post-commit callback delivery failure | `PostCommitCallbackFailureTxn` | domain commit 後の callback delivery failure について domain state を rollback せず、callback health/diagnostic を更新する。domain commit 自体は所有しない。 |
| Filter `flush()` | `FilterFlushTxn` | `FilterProducerDrainGate`、Filter queue/event/parser/AV pending state を対象とする Filter 固有 orchestration。 |
| DVR `flush()` | `DvrFlushTxn` | record/playback eligibility、FMQ token、queue contents、`QueueEpochProtocol`、DVR parser/statistics を対象とする DVR 固有 orchestration。 |
| A/V sync id relation | `AvSyncRegistry` | `filter_id <-> hw_id` 双方向 map を一体 commit / delete する。 |
| PCR clock anchor | `PcrClockAnchorStore` | filter generation に属する PCR anchor を所有する。各 stream boundary は typed invalidation request を送る。 |

## 4. 11 項目の修正契約

### CMB-01 public close / owner loss / Drop

`IFrontend.close()`、`IFilter.close()`、`IDvr.close()`、その他 close を持つ object の API 節は、必要な cleanup の種類・順序・公開結果を定義してよい。ただし、cleanup step の実行 authority、途中失敗後も残 step を継続する規則、未完了 cleanup の保存、retry、Drop からの handoff は `ObjectCloseTxn` の単一正本とする。

Drop は別の `DropLeakTxn` を開始しない。Drop/owner loss は `ObjectCloseTxn` の同じ cleanup authority に owner-loss origin を付けて投入する。

### CMB-02 demux frontend source transition

`IDemux.setFrontendDataSource()` は `DemuxFrontendSourceTxn` を唯一の relation transaction とする。

`DemuxFrontendSourceTxn` は次を所有する。

- current/new frontend relation の validation
- new relation の prepare
- old relation の release 準備
- relation commit / rollback
- relation commit と一貫した domain record 更新

parser/assembler/continuity の境界変更が必要な場合は `StreamBoundaryTxn` を typed boundary request として 1 回だけ呼ぶ。`DemuxFrontendSourceTxn` 自身が parser/assembler/generation を直接変更してはならず、`StreamBoundaryTxn` が frontend relation を直接変更してもならない。

### CMB-03 LNB callback artifact

`ILnb.setCallback()` で受け取る Binder callback 実体は LNB object に直接保持しない。strong reference、death recipient、replacement、release は callback artifact 共通 facade/store のみが所有する。

LNB domain が保持してよいのは logical callback identity、generation、登録有無など Binder 非依存の domain state だけである。

### CMB-04 LNB persistent control

`setVoltage()` / `setTone()` / `setSatellitePosition()` は `LnbControlTxn` に typed command を渡す。

共通 transaction の順序は、lock -> old state snapshot -> candidate -> backend apply -> registry commit である。backend apply 成功後に registry commit が失敗する等の異常系についても `LnbControlTxn` が単一の failure transition を所有する。

`sendDiseqcMessage()` は persistent registry commit を同じ形で持たないため `LnbControlTxn` へ吸収しない。

### CMB-05 descrambler PID mutation

`addPid()` / `removePid()` は `DescramblerPidTxn` に typed operation を渡す。backend packet-path apply、PID relation ledger commit、必要な compensation、compensation 不成立時の quarantine 判定は同 transaction が所有する。

key token は `DescramblerKeyTxn`、session cleanup は `DescramblerSessionCleanupTxn` の責務であり、PID transaction と統合しない。

### CMB-06 Record DVR / Filter relation

Record DVR と Filter の attach/detach relation は `RecordDvrFilterRelationTxn` が唯一の mutation authority である。

- attach は両 object の lifecycle/owner/demux/kind を検証し、relation を 1 commit で作る。
- duplicate attach の冪等性も同 transaction が判定する。
- detach は同 relation を 1 commit で除去する。
- Filter close / DVR close / demux cleanup からの unlink も同じ owner を通す。

両 object が独立に relation table を更新してはならない。

### CMB-07 worker lifecycle

`WorkerLifecycleProtocol` は domain 非依存の mechanism として、stop predicate、wake/cancel、owner generation fence、join、reaper handoff を定義する。

Frontend、Filter、DVR、Playback 等の start/stop/failed state machine はそれぞれの domain owner に残す。共通 protocol は domain state transition を決めない。

`WorkerFailureClassifier` は `WorkerPanic`、`JoinFailure`、wake/EventFlag/reaper 等の worker infrastructure failure だけを分類する。backend/device I/O error と callback delivery error をここへ流してはならない。

### CMB-08 post-commit callback failure

`PostCommitCallbackFailureTxn` は Filter/DVR `start()` 専用ではなく、**domain commit 後**に callback delivery が失敗する全経路で使用する。

Frontend tune の commit 後 callback、DVR status callback 等も同じ規則を使う。domain commit は維持し、callback health/diagnostic のみ更新する。

commit 前の callback preparation failure や domain transaction failure はこの transaction の対象ではない。

### CMB-09 A/V sync / PCR anchor

`AvSyncRegistry` が `filter_id -> hw_id` と `hw_id -> filter_id` を単一 commit boundary で所有する。filter unregister/reconfigure/close、demux close 等からの削除も registry API を通し、片側 map だけを直接変更しない。

`PcrClockAnchorStore` が PCR filter generation に属する anchor を所有する。flush/stop/close、input generation change、retune、playback flush 等の stream boundary は `StreamBoundaryTxn` から typed invalidation request を送る。各 API が anchor 内部を直接変更してはならない。

### CMB-10 Filter / DVR flush の分離

共有 `QueueCleanupTxn` を Filter と DVR の共通 transaction owner として扱わない。

- Filter は `FilterFlushTxn` が orchestration を所有し、`FilterProducerDrainGate` と Filter 固有 queue/event/parser/AV pending state を扱う。
- DVR は `DvrFlushTxn` が orchestration を所有し、record/playback eligibility、FMQ token、queue contents、`QueueEpochProtocol`、DVR parser/statistics を扱う。

結果集約、checked queue operation、cleanup error composition 等の mechanism は helper として共通化してよいが、両 state machine の commit/rollback を 1 transaction にしない。

### CMB-11 WorkerFailureClassifier の限定

`WorkerFailureClassifier` は worker infrastructure failure に限定する。

backend/device failure は backend adapter/domain typed error が原因を保持し、上位 domain transaction が公開状態へ写像する。callback failure は callback layer と、commit 後であれば `PostCommitCallbackFailureTxn` が扱う。

同 classifier が worker、backend、callback の意味領域を横断して一つの failure enum/state machine を所有してはならない。

## 5. 禁止事項

次を禁止する。

- API/worker/helper が、本書で owner を指定した state を直接更新し、第二の mutation SSOT を作ること。
- phase の形が似ているだけの異種 state machine を、共通 transaction に吸収すること。
- `SourceBoundaryTxn` に demux/frontend relation を持たせること。
- `StreamBoundaryTxn` に relation ownership を持たせること。
- `QueueCleanupTxn` を Filter/DVR 共通 flush transaction として再導入すること。
- `WorkerFailureClassifier` に backend/device/callback failure を再統合すること。
- `DropLeakTxn` を `ObjectCloseTxn` と並ぶ cleanup authority として再導入すること。

## 6. 設計完了条件

この共通化境界修正は、次をすべて満たしたとき設計上完了とする。

1. CMB-01〜CMB-11 の各状態変更について、単一 owner または意図的に分離された state machine が明示されている。
2. API 個別節は公開意味・対象・順序・戻り値を定義しても、第二の mutation SSOT を持たない。
3. 共通 transaction は状態 owner、commit boundary、rollback authority、failure semantics が一致する範囲を越えない。
4. `tuner_hal2/COMMONIZATION_BOUNDARIES_JA.md` の実装構造正本と owner 名・責務境界が一致する。
5. 公開 AIDL / VTS / CDD / ARIB の意味論を変更しない。

実装追従状況、静的確認、異常系試験の実施状況は、この設計変更そのものの完了条件には含めない。

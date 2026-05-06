# Tuner HAL コーディング規則

## 1. 目的

このドキュメントは、Tuner HAL に固有のコーディング規則を固定する。プロジェクト全体の Rust 規約は `../GLOBAL_CODE_CONVENTION.md` の Rust 節に従う。本書では、Tuner HAL の公開契約、service lifecycle、AIDL error mapping、device absence、worker、callback、mutex poison 時の振る舞いを定める。

## 2. 最上位方針

Tuner HAL は framework と vendor hardware の安定境界であり、VINTF、VTS、製品 runtime の対象である。Tuner HAL の public Binder method、worker、callback path は panic で終了してはならない。

次は panic ではなく、AIDL error、degraded state、diagnostics へ写像する。

```text
- client 入力不正
- 未対応機能
- target tuner device absent
- device node open / ioctl failure
- callback failure
- FMQ / EventFlag / native shim failure
- dma-buf allocation failure
- 内部状態不整合
- mutex poison
```

## 3. No-panic boundary

次の範囲は Tuner HAL の no-panic boundary とする。

```text
- ITuner / IFrontend / IDemux / IFilter / IDvr / IDescrambler / ILnb の Binder method
- frontend tune / scan worker
- demux pump worker
- filter callback worker
- DVR playback / record worker
- shared memory allocation path
- device file open / ioctl path
- FMQ / EventFlag / native shim call boundary
- HAL callback 送信 path
```

この範囲では、release build の通常経路で以下を使ってはならない。

```text
panic!
unwrap()
expect()
todo!
unimplemented!
unreachable!
assert! / assert_eq! / assert_ne!
```

許可される例外は次だけである。

```text
- `#[cfg(test)]` の unit test
- offline generator / build-time tool
- service registration 前の明示的 fatal configuration check
```

VINTF HAL instance として service 登録済みになった後は、panic で service process を落とさない。

## 4. AIDL error mapping

Tuner HAL の public API は、下表の基準で AIDL service-specific error へ写像する。

| 状態 | 返す error | 方針 |
|---|---|---|
| client 引数不正 | `INVALID_ARGUMENT` | PID 範囲外、unknown enum、CS110 stream selector 指定、BS relative stream number の backend 不一致など |
| lifecycle 不正 | `INVALID_STATE` | close 後操作、start 前 read、AV stream type 未設定など |
| 未対応機能 | `UNAVAILABLE` | 未対応 CI CAM、未対応 LNB、未対応 backend、未対応 relative stream number など |
| resource 不足 | `NO_MEMORY` または `UNKNOWN_ERROR` | dma-buf allocation failure、FMQ allocation failure など。既存 AIDL helper に合わせる |
| target tuner device absent | `UNAVAILABLE` | degraded boot 方針に従い、該当 frontend / resource を advertise しない、または open 時に unavailable |
| device node / ioctl transient failure | `UNKNOWN_ERROR` | device は存在するが ioctl が予期せず失敗した場合 |
| internal invariant failure | `UNKNOWN_ERROR` | panic せず diagnostics に記録し、必要なら object を degraded / closed にする |
| callback remote failure | method 自体は panic せず cleanup | remote object dead / binder error は log + cleanup にする |
| mutex poison | `UNKNOWN_ERROR` + fail-closed | 対象 object を degraded / closed にする |

低レベル error は public Binder method の各所で直接散在させず、`binder_service` 内の status helper または error mapping helper 経由で返す。

## 5. Startup / runtime failure model

### 5.1 Fatal としてよいもの

以下は service registration 前に検出し、fatal としてよい。

```text
- AIDL service registration failure
- VINTF instance 名不一致
- 必須 profile / static config の parse failure
- stable AIDL / service name / init 設定の自己矛盾
```

これらは HAL service として成立しないため、起動完了扱いにしない。

### 5.2 target tuner device absent は degraded boot

対象 device node / frontend が存在しない場合でも、HAL service 自体は起動する。ただし、存在しない frontend / demux / backend resource を capability として advertise してはならない。該当 resource への open / tune / scan は `UNAVAILABLE` を返す。

実装要件:

```text
- 起動時 device discovery の結果を capability に反映する
- device absent の frontend id を返さない
- device absent でも service process を panic で落とさない
- degraded reason を dumpsys、log、internal diagnostics に残す
- VTS profile で required frontend が必要な場合は、profile 側で presence を検査し、欠落時に明確に fail させる
```

### 5.3 runtime failure は object 単位で閉じる

runtime 中の個別失敗は service 全体を落とさず、影響範囲を object 単位に閉じる。

| failure | 方針 |
|---|---|
| individual frontend open failure | 該当 frontend を unavailable / failed 状態にする |
| tune ioctl failure | 該当 tune / scan を error 完了にする。service process は継続する |
| demux pump failure | 該当 demux を failed / degraded にし、関連 filter / DVR を停止または error 状態へ遷移させる |
| dma-buf allocation failure | 該当 AV filter operation failure に限定する。非AV filter や service 全体へ波及させない |
| callback remote dead | 該当 callback 登録を cleanup する。panic しない |

## 6. Mutex poison は fail-closed

Tuner HAL における mutex poison recovery は fail-closed とする。`PoisonError::into_inner()` で通常復旧して処理継続してはならない。

poisoned lock に触れた public method / worker は、`UNKNOWN_ERROR`、`INVALID_STATE`、`UNAVAILABLE` のいずれかを返し、対象 object を degraded / closed にする。

| object | poison 時の扱い |
|---|---|
| ITuner global registry | affected resource registry を degraded にし、新規 open を制限する。service process は継続する |
| Frontend state | 該当 frontend を failed / degraded にする。以後の tune / scan は `INVALID_STATE` または `UNAVAILABLE` |
| Demux state | 該当 demux を failed にする。関連 filter / DVR は停止または closed 扱い |
| Filter state | 該当 filter を failed / closed にする。以後の start / read / flush は `INVALID_STATE` |
| DVR state | 該当 DVR を failed / closed にする。playback / record worker を停止する |
| Shared memory / FMQ state | 該当 object を closed にする。stale handle を registry から外す |

診断要件:

```text
- poisoned mutex 名を log に出す
- poison count を diagnostics に積む
- fail-closed により閉じた object id / type を diagnostics に残す
```

## 7. Lock / callback policy

Tuner HAL は以下を設計規則として固定する。

```text
- HAL internal lock を保持したまま framework callback を呼ばない
- HAL internal lock を保持したまま Binder method を再入呼び出ししない
- callback payload は lock 内で copy / snapshot 化し、lock 解放後に callback する
- callback failure は log + cleanup とする。panic しない
- worker は lock を長時間保持して blocking I/O / wait / callback を行わない
```

## 8. Worker error policy

worker thread は、外側で error を diagnostics と object state に反映できる構造にする。

推奨構造:

```rust
fn frontend_worker_main(...) -> Result<(), HalWorkerError>;
fn demux_worker_main(...) -> Result<(), HalWorkerError>;
fn filter_callback_worker_main(...) -> Result<(), HalWorkerError>;
fn dvr_playback_worker_main(...) -> Result<(), HalWorkerError>;
fn dvr_record_worker_main(...) -> Result<(), HalWorkerError>;
```

worker 異常時は次を行う。

```text
- worker_error_count を増やす
- catch_unwind を使う場合は worker_panic_count を増やす
- affected frontend / demux / filter / DVR を failed / degraded にする
- 次の public method で UNKNOWN_ERROR / INVALID_STATE を返す
- silent stop させない
```

worker thread が終了した場合、何も通知せず data path が止まる状態は禁止する。

## 9. Capability / advertise policy

capability は実体と一致させる。

```text
- 存在しない frontend を advertise しない
- 実装していない CI CAM / descrambler / LNB 機能を成功扱いしない
- dma-buf が確保できないことを理由に非AV filter capability を落とさない
- AV shared memory が必要な AV path は、失敗時に該当 operation だけ error にする
- DVR playback / record を claim する場合は、worker failure / queue overflow を status として返す
```

## 10. 完了判定

この規則に準拠していることは、次で確認する。

```text
1. release HAL path に panic / unwrap / expect / assert 系が残っていない
2. public Binder method の error mapping が本書と実装 helper で一致している
3. device discovery 結果が capability advertise に反映されている
4. device absent で service process が panic しない
5. callback は lock 解放後に実行される
6. worker death / error が diagnostics と object state に反映される
7. callback failure / ioctl failure / dma-buf failure が service process death にならない
8. mutex poison は fail-closed になる
```


## 追加固定規約

- framework callback の戻り値は必ず検査する。`let _ = callback...` による binder callback result の破棄を禁止する。
- callback failure は、対象 callback registration cleanup、対象 object の failed / closed 遷移、diagnostic log 記録を一体で行う。
- worker が lock failure、registry inconsistency、record 不在、callback failure で終了する場合、silent stop にせず abnormal worker stop として対象 object を failed / closed state に遷移させる。
- worker の停止待ちは stop signal で wake できる wait primitive を使う。DVR callback worker は client 指定 interval の `thread::sleep` によって close / Drop / shutdown をブロックしてはならない。
- device node 不在、open 不可、permission 不足は `UNAVAILABLE` とする。device が存在する状態での runtime ioctl failure / TS read failure / pump failure は `UNKNOWN_ERROR` とする。
- client invalid input は `INVALID_ARGUMENT` とする。CS110 stream selector 指定、unknown monitor bit、負値または `default_max` 超過の `setMaxNumberOfFrontends()` は `INVALID_ARGUMENT` に固定する。
- product runtime に degraded frontend entry variant / generator / helper を置かない。device absent は service 起動継続 + diagnostics record + frontend 非広告で扱う。
- 一時レビュー用 Markdown を release archive に同梱しない。恒久設計は `DESIGN_JA.md`、実装規約は `CODE_CONVENTION.md`、変更履歴は `CHANGELOG.md` に統合する。

## WorkerExit / scan terminal reason

release HAL path の worker は、panic を thread join 成功扱いにしてはならない。worker wrapper は `WorkerExit` を返し、`Normal` / `StopRequested` / `RuntimeFailure` / `PanicOrJoinFailure` を区別する。既存コードの読み替え互換として `Cancelled` / `Error` / `Panic` の alias を一時的に許容するが、正式な意味はそれぞれ `StopRequested` / `RuntimeFailure` / `PanicOrJoinFailure` である。`catch_unwind()` で捕捉した panic は `WorkerExit::PanicOrJoinFailure`、worker body が検出した runtime fatal は `WorkerExit::RuntimeFailure` として diagnostics に反映する。

`frontend_live_pump`、`frontend_tune_worker`、`frontend_scan_worker` の abnormal exit は affected frontend runtime の `record_runtime_failure()` に残し、live data path と linked demux/filter/DVR を fail-closed にする。

scan session は terminal reason を持つ。`Running`、`Completed`、`Cancelled`、`FailedBackend`、`FailedCallback`、`FailedPanic` を区別し、backend error や panic を normal completion として扱わない。

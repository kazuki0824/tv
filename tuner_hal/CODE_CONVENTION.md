# Tuner HAL コーディング規則

## 1. 目的

このドキュメントは、Tuner HAL に固有のコーディング規則を固定する。プロジェクト全体の Rust 規約は `../GLOBAL_CODE_CONVENTION.md` の Rust 節に従う。本書では、Tuner HAL の公開契約、serviceライフサイクル、AIDLエラー写像、デバイス不在、worker、callback、mutex poison 時の振る舞いを定める。

## 2. 最上位方針

Tuner HAL は framework と vendor hardware の安定境界であり、VINTF、VTS、製品実行時 の対象である。Tuner HAL の 公開Binderメソッド、worker、callback経路 は panic で終了してはならない。

次は panic ではなく、AIDLエラー、劣化状態、diagnostics へ写像する。

```text
- client 入力不正
- 未対応機能
- 対象tuner device不在
- device node open / ioctl失敗
- callback失敗
- FMQ / EventFlag / native shim失敗
- dma-buf確保失敗
- 内部状態不整合
- mutex poison
```

## 3. panic禁止境界

次の範囲は Tuner HAL の panic 禁止境界とする。

```text
- ITuner / IFrontend / IDemux / IFilter / IDvr / IDescrambler / ILnb の Binderメソッド
- frontend tune / scan worker
- demux pump worker
- filter callback worker
- DVR再生 / 録画 worker
- 共有メモリ確保経路
- device file open / ioctl経路
- FMQ / EventFlag / native shim呼び出し境界
- HAL callback送信経路
```

この範囲では、リリースビルド の通常経路で以下を使ってはならない。

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
- `#[cfg(test)]` の 単体テスト
- オフライン生成器 / ビルド時ツール
- service登録 前の明示的 致命的な設定確認
```

VINTF HAL instance として service 登録済みになった後は、panic で serviceプロセス を落とさない。

## 4. AIDLエラー写像

Tuner HAL の 公開API は、下表の基準で AIDL service固有エラー へ写像する。

| 状態 | 返すエラー | 方針 |
|---|---|---|
| クライアント引数不正 | `INVALID_ARGUMENT` | PID 範囲外、未知enum、CS110 stream selector 指定、BS relative stream number の backend 不一致など |
| lifecycle 不正 | `INVALID_STATE` | close 後操作、start 前 read、AV stream type 未設定など |
| 未対応機能 | `UNAVAILABLE` | 未対応 CI CAM、未対応 LNB、未対応 backend、未対応 relative stream number など |
| resource不足 | `NO_MEMORY` または `UNKNOWN_ERROR` | dma-buf確保失敗、FMQ 確保失敗 など。既存 AIDL補助関数 に合わせる |
| 対象tuner device不在 | `UNAVAILABLE` | 劣化起動 方針に従い、該当 frontend / resource を advertise しない、または open 時に unavailable |
| device node / ioctl 一時失敗 | `UNKNOWN_ERROR` | device は存在するが ioctl が予期せず失敗した場合 |
| 内部不変条件違反 | `UNKNOWN_ERROR` | panic せず diagnostics に記録し、必要なら object を degraded / closed にする |
| callback remote 失敗 | メソッド自体は panic せず cleanup | remote object dead / binder error はログ記録と cleanup にする |
| mutex poison | `UNKNOWN_ERROR` + 閉鎖側失敗 | 対象 object を degraded / closed にする |

低レベルエラー は 公開Binderメソッド の各所で直接散在させず、`binder_service` 内の 状態補助関数 または エラー写像補助関数 経由で返す。

## 5. 起動時 / 実行時失敗モデル

### 5.1 Fatal としてよいもの

以下は service登録 前に検出し、fatal としてよい。

```text
- AIDL service登録 失敗
- VINTF instance 名不一致
- 必須 profile / 静的設定 の 解析失敗
- stable AIDL / service名 / init 設定の自己矛盾
```

これらは HAL service として成立しないため、起動完了扱いにしない。

### 5.2 対象tuner device不在 は 劣化起動

対象 device node / frontend が存在しない場合でも、HAL service 自体は起動する。ただし、存在しない frontend / demux / backend resource を capability として advertise してはならない。該当 resource への open / tune / scan は `UNAVAILABLE` を返す。

実装要件:

```text
- 起動時 デバイス検出 の結果を capability に反映する
- device不在 の frontend ID を返さない
- device不在 でも serviceプロセス を panic で落とさない
- 劣化理由 を dumpsys、log、internal diagnostics に残す
- VTS profile で 必須frontend が必要な場合は、profile 側で 存在 を検査し、欠落時に明確に fail させる
```

### 5.3 実行時失敗 は object単位で閉じる

実行中の個別失敗は service 全体を落とさず、影響範囲を object単位に閉じる。

| 失敗 | 方針 |
|---|---|
| 個別frontend open失敗 | 該当 frontend を unavailable / failed 状態にする |
| tune ioctl失敗 | 該当 tune / scan を エラー完了にする。serviceプロセス は継続する |
| demux pump失敗 | 該当 demux を failed / degraded にし、関連 filter / DVR を停止または エラー状態へ遷移させる |
| dma-buf確保失敗 | 該当 AV filter 操作 失敗 に限定する。非AV filter や service 全体へ波及させない |
| callback remote dead | 該当 callback 登録を cleanup する。panic しない |

## 6. Mutex poison は 閉鎖側失敗

Tuner HAL における mutex poison recovery は 閉鎖側失敗 とする。`PoisonError::into_inner()` で通常復旧して処理継続してはならない。

poison済みロック に触れた 公開メソッド / worker は、`UNKNOWN_ERROR`、`INVALID_STATE`、`UNAVAILABLE` のいずれかを返し、対象 object を degraded / closed にする。

| object | poison 時の扱い |
|---|---|
| ITuner global registry | 影響を受けたresource registry を degraded にし、新規 open を制限する。serviceプロセス は継続する |
| Frontend 状態 | 該当 frontend を failed / degraded にする。以後の tune / scan は `INVALID_STATE` または `UNAVAILABLE` |
| Demux 状態 | 該当 demux を failed にする。関連 filter / DVR は停止または closed 扱い |
| Filter 状態 | 該当 filter を failed / closed にする。以後の start / read / flush は `INVALID_STATE` |
| DVR 状態 | 該当 DVR を failed / closed にする。playback / record worker を停止する |
| 共有メモリ / FMQ 状態 | 該当 object を closed にする。古いhandle を registry から外す |

診断要件:

```text
- poisoned mutex 名を log に出す
- poison回数 を diagnostics に積む
- 閉鎖側失敗 により閉じた object ID / type を diagnostics に残す
```

## 7. ロック / callback方針

Tuner HAL は以下を設計規則として固定する。

```text
- HAL内部ロック を保持したまま framework callback を呼ばない
- HAL内部ロック を保持したまま Binderメソッド を再入呼び出ししない
- callback payload は lock 内で copy / snapshot 化し、lock 解放後に callback する
- callback失敗 は log + cleanup とする。panic しない
- worker は lock を長時間保持して blocking I/O / wait / callback を行わない
```

## 8. Workerエラー方針

ワーカースレッド は、外側で エラーをdiagnosticsとobject状態 に反映できる構造にする。

推奨構造:

```rust
fn frontend_worker_main(...) -> Result<(), HalWorkerError>;
fn demux_worker_main(...) -> Result<(), HalWorkerError>;
fn filter_callback_worker_main(...) -> Result<(), HalWorkerError>;
fn dvr_playback_worker_main(...) -> Result<(), HalWorkerError>;
fn dvr_record_worker_main(...) -> Result<(), HalWorkerError>;
```

worker異常時は次を行う。

```text
- worker_error_count を増やす
- catch_unwind を使う場合は worker_panic_count を増やす
- affected frontend / demux / filter / DVR を failed / degraded にする
- 次の 公開メソッド で UNKNOWN_ERROR / INVALID_STATE を返す
- 無言停止 させない
```

ワーカースレッド が終了した場合、何も通知せず data path が止まる状態は禁止する。

## 9. Capability / advertise policy

capability は実体と一致させる。

```text
- 存在しない frontend を advertise しない
- 実装していない CI CAM / descrambler / LNB 機能を成功扱いしない
- dma-buf が確保できないことを理由に非AV filter capability を落とさない
- AV共有メモリ が必要な AV経路 は、失敗時に該当 操作 だけ エラーにする
- DVR再生 / 録画 を claim する場合は、worker 失敗 / queue overflow を status として返す
```

## 10. 完了判定

この規則に準拠していることは、次で確認する。

```text
1. リリースHAL経路 に panic / unwrap / expect / assert 系が残っていない
2. 公開Binderメソッド の エラー写像 が本書と実装 helper で一致している
3. デバイス検出 結果が capability広告 に反映されている
4. device不在 で serviceプロセス が panic しない
5. callback は lock 解放後に実行される
6. worker終了 / エラー が diagnostics と object 状態 に反映される
7. callback失敗 / ioctl 失敗 / dma-buf 失敗 が service プロセス終了 にならない
8. mutex poison は 閉鎖側失敗 になる
```


## 追加固定規約

- Tuner HAL の 単体テスト では、`include_str!("tuner_hal.rs")`、`include_str!("main.rs")` などで本体ソースを自己参照し、`contains()` / `find()` / `split()` / 正規表現で実装済み判定を行ってはならない。コード構造の存在確認は完了条件ではなく、テスト自身の文字列リテラルで自己充足し得るため禁止する。実装契約は公開API、helper、状態 machine、diagnostic、queue、callback、worker stop/join などを実際に呼び出すテストで固定する。`include_str!()` は VTS設定、rc、sepolicy、設計文書、別モジュールSSOTとの整合確認に限って補助的に使ってよい。
- framework callback の戻り値は必ず検査する。`let _ = callback...` による binder callback result の破棄を禁止する。
- callback失敗 は、対象 callback登録のcleanup、対象 object の failed / closed 遷移、診断log 記録を一体で行う。
- worker が ロック失敗、registry不整合、record 不在、callback失敗 で終了する場合、無言停止 にせず 異常worker停止 として対象 object を failed / closed 状態 に遷移させる。
- worker の停止待ちは 停止信号 で wake できる 待機primitive を使う。DVR callback worker は client 指定 interval の `thread::sleep` によって close / Drop / shutdown をブロックしてはならない。
- device node 不在、open 不可、permission 不足は `UNAVAILABLE` とする。device が存在する状態での 実行時ioctl失敗 / TS read 失敗 / pump 失敗 は `UNKNOWN_ERROR` とする。
- client不正入力 は `INVALID_ARGUMENT` とする。CS110 stream selector 指定、unknown monitor bit、負値または `default_max` 超過の `setMaxNumberOfFrontends()` は `INVALID_ARGUMENT` に固定する。
- product実行時 に 劣化frontend entry variant / generator / helper を置かない。device不在 は service 起動継続 + diagnostics記録 + frontend 非広告で扱う。
- 一時レビュー用 Markdown を リリースアーカイブ に同梱しない。恒久設計は `DESIGN_JA.md`、実装規約は `CODE_CONVENTION.md`、変更履歴は `CHANGELOG.md` に統合する。

## WorkerExit / scan 終了理由

リリースHAL経路 の worker は、panic を thread join成功扱いにしてはならない。worker wrapper は `WorkerExit` を返し、`Normal` / `StopRequested` / `RuntimeFailure` / `PanicOrJoinFailure` を区別する。既存コードの読み替え互換として `Cancelled` / `Error` / `Panic` の alias を一時的に許容するが、正式な意味はそれぞれ `StopRequested` / `RuntimeFailure` / `PanicOrJoinFailure` である。`catch_unwind()` で捕捉した panic は `WorkerExit::PanicOrJoinFailure`、worker body が検出した 実行時致命失敗 は `WorkerExit::RuntimeFailure` として diagnostics に反映する。

`frontend_live_pump`、`frontend_tune_worker`、`frontend_scan_worker` の 異常終了 は 影響を受けたfrontend runtime の `record_runtime_失敗()` に残し、live data path と linked demux/filter/DVR を 閉鎖側失敗 にする。

scan session は 終了理由 を持つ。`Running`、`Completed`、`Cancelled`、`FailedBackend`、`FailedCallback`、`FailedPanic` を区別し、backendエラー や panic を 正常完了 として扱わない。

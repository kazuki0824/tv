# Tuner HAL コーディング規則

## 1. 目的

このドキュメントは、Tuner HAL に固有のコーディング規則を固定する。プロジェクト全体の Rust 規約は `../GLOBAL_CODE_CONVENTION.md` の Rust 節に従う。本書では、Tuner HAL の公開契約、サービスライフサイクル、AIDLエラー写像、デバイス不在、ワーカー、コールバック、mutex汚染 時の振る舞いを定める。

## 2. 最上位方針

Tuner HAL は framework と vendor hardware の安定境界であり、VINTF、VTS、製品実行時 の対象である。Tuner HAL の 公開Binderメソッド、ワーカー、コールバック経路 は `panic` で終了してはならない。

次は `panic` ではなく、AIDLエラー、劣化状態、診断情報へ写像する。

```text
- client 入力不正
- 未対応機能
- 対象tuner device不在
- device node open / ioctl失敗
- コールバック失敗
- FMQ / EventFlag / ネイティブ薄層失敗
- dma-buf確保失敗
- 内部状態不整合
- mutex汚染
```

## 3. panic禁止境界

次の範囲は Tuner HAL の `panic` 禁止境界とする。

```text
- ITuner / IFrontend / IDemux / IFilter / IDvr / IDescrambler / ILnb の Binderメソッド
- frontend tune / scan ワーカー
- demux pump ワーカー
- filter コールバック ワーカー
- DVR再生 / 録画 ワーカー
- 共有メモリ確保経路
- device file open / ioctl経路
- FMQ / EventFlag / ネイティブ薄層呼び出し境界
- HAL コールバック送信経路
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
- サービス登録 前の明示的 致命的な設定確認
```

VINTF HAL instance として サービス登録済みになった後は、`panic` で サービスプロセス を落とさない。

## 4. AIDLエラー写像

Tuner HAL の 公開API は、下表の基準で AIDL サービス固有エラー へ写像する。

| 状態 | 返すエラー | 方針 |
|---|---|---|
| クライアント引数不正 | `INVALID_ARGUMENT` | PID 範囲外、未知enum、CS110 stream selector 指定、BS relative stream number の backend 不一致など |
| lifecycle 不正 | `INVALID_STATE` | close 後操作、start 前 read、AV stream type 未設定など |
| 未対応機能 | `UNAVAILABLE` | 未対応 CI CAM、未対応 LNB、未対応 backend、未対応 relative stream number など |
| resource不足 | `NO_MEMORY` または `UNKNOWN_ERROR` | dma-buf確保失敗、FMQ 確保失敗 など。既存 AIDL補助関数 に合わせる |
| 対象tuner device不在 | `UNAVAILABLE` | 劣化起動 方針に従い、該当 frontend / resource を advertise しない、または open 時に unavailable |
| device node / ioctl 一時失敗 | `UNKNOWN_ERROR` | device は存在するが ioctl が予期せず失敗した場合 |
| 内部不変条件違反 | `UNKNOWN_ERROR` | `panic` せず 診断情報に記録し、必要ならオブジェクトを 劣化 / 閉鎖済み にする |
| コールバック remote 失敗 | メソッド自体は `panic` せず cleanup | remote オブジェクト dead / binder error はログ記録と cleanup にする |
| mutex汚染 | `UNKNOWN_ERROR` + 閉鎖側失敗 | 対象オブジェクトを 劣化 / 閉鎖済み にする |

低レベルエラー は 公開Binderメソッド の各所で直接散在させず、`binder_service` 内の 状態補助関数 または エラー写像補助関数 経由で返す。

## 5. 起動時 / 実行時失敗モデル

### 5.1 Fatal としてよいもの

以下は サービス登録 前に検出し、fatal としてよい。

```text
- AIDL サービス登録 失敗
- VINTF instance 名不一致
- 必須 profile / 静的設定 の 解析失敗
- stable AIDL / サービス名 / init 設定の自己矛盾
```

これらは HAL サービスとして成立しないため、起動完了扱いにしない。

### 5.2 対象tuner device不在 は 劣化起動

対象 device node / frontend が存在しない場合でも、HAL サービス自体は起動する。ただし、存在しない frontend / demux / backend resource を capability として advertise してはならない。該当 resource への open / tune / scan は `UNAVAILABLE` を返す。

実装要件:

```text
- 起動時 デバイス検出 の結果を capability に反映する
- device不在 の frontend ID を返さない
- device不在 でも サービスプロセス を `panic` で落とさない
- 劣化理由 を dumpsys、ログ、internal 診断情報に残す
- VTS profile で 必須frontend が必要な場合は、profile 側で 存在 を検査し、欠落時に明確に fail させる
```

### 5.3 実行時失敗 は オブジェクト単位で閉じる

実行中の個別失敗は サービス 全体を落とさず、影響範囲を オブジェクト単位に閉じる。

| 失敗 | 方針 |
|---|---|
| 個別frontend open失敗 | 該当 frontend を unavailable / 失敗 状態にする |
| tune ioctl失敗 | 該当 tune / scan を エラー完了にする。サービスプロセス は継続する |
| demux pump失敗 | 該当 demux を 失敗 / 劣化 にし、関連 filter / DVR を停止または エラー状態へ遷移させる |
| dma-buf確保失敗 | 該当 AV filter 操作 失敗 に限定する。非AV filter や サービス 全体へ波及させない |
| コールバック remote dead | 該当 コールバック 登録を cleanup する。`panic` しない |

## 6. Mutex 汚染 は 閉鎖側失敗

Tuner HAL における mutex汚染 recovery は 閉鎖側失敗 とする。`PoisonError::into_inner()` で通常復旧して処理継続してはならない。

汚染済みロック に触れた 公開メソッド / ワーカー は、`UNKNOWN_ERROR`、`INVALID_STATE`、`UNAVAILABLE` のいずれかを返し、対象オブジェクトを 劣化 / 閉鎖済み にする。

| オブジェクト | 汚染 時の扱い |
|---|---|
| ITuner global registry | 影響を受けたresource registry を 劣化 にし、新規 open を制限する。サービスプロセス は継続する |
| Frontend 状態 | 該当 frontend を 失敗 / 劣化 にする。以後の tune / scan は `INVALID_STATE` または `UNAVAILABLE` |
| Demux 状態 | 該当 demux を 失敗 にする。関連 filter / DVR は停止または 閉鎖済み 扱い |
| Filter 状態 | 該当 filter を 失敗 / 閉鎖済み にする。以後の start / read / flush は `INVALID_STATE` |
| DVR 状態 | 該当 DVR を 失敗 / 閉鎖済み にする。playback / record ワーカーを停止する |
| 共有メモリ / FMQ 状態 | 該当オブジェクトを 閉鎖済み にする。古いhandle を registry から外す |

診断要件:

```text
- poisoned mutex 名を ログ に出す
- 汚染回数を 診断情報に積む
- 閉鎖側失敗 により閉じた オブジェクト ID / type を 診断情報に残す
```

## 7. ロック / コールバック方針

Tuner HAL は以下を設計規則として固定する。

```text
- HAL内部ロックを保持したまま framework コールバック を呼ばない
- HAL内部ロックを保持したまま Binderメソッド を再入呼び出ししない
- コールバックペイロードは ロック内で copy / snapshot 化し、ロック解放後に コールバック する
- コールバック失敗 は ログ + cleanup とする。`panic` しない
- ワーカー は ロックを長時間保持して blocking I/O / wait / コールバック を行わない
```

## 8. Workerエラー方針

ワーカースレッドは、外側で エラーを診断情報とオブジェクト状態に反映できる構造にする。

推奨構造:

```rust
fn frontend_worker_main(...) -> Result<(), HalWorkerError>;
fn demux_worker_main(...) -> Result<(), HalWorkerError>;
fn filter_callback_worker_main(...) -> Result<(), HalWorkerError>;
fn dvr_playback_worker_main(...) -> Result<(), HalWorkerError>;
fn dvr_record_worker_main(...) -> Result<(), HalWorkerError>;
```

ワーカー異常時は次を行う。

```text
- worker_error_count を増やす
- catch_unwind を使う場合は worker_panic_count を増やす
- affected frontend / demux / filter / DVR を 失敗 / 劣化 にする
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
- DVR再生 / 録画 を 対応宣言する場合は、ワーカー 失敗 / queue overflow を 状態 として返す
```

## 10. 完了判定

この規則に準拠していることは、次で確認する。

```text
1. リリースHAL経路 に `panic` / `unwrap` / expect / assert 系が残っていない
2. 公開Binderメソッド の エラー写像 が本書と実装 helper で一致している
3. デバイス検出 結果が capability広告 に反映されている
4. device不在 で サービスプロセス が `panic` しない
5. コールバック は ロック解放後に実行される
6. ワーカー終了 / エラー が 診断情報と オブジェクト 状態に反映される
7. コールバック失敗 / ioctl 失敗 / dma-buf 失敗 が サービス プロセス終了 にならない
8. mutex汚染 は 閉鎖側失敗 になる
```


## 追加固定規約

- Tuner HAL の 単体テスト では、`include_str!("tuner_hal.rs")`、`include_str!("main.rs")` などで本体ソースを自己参照し、`contains()` / `find()` / `split()` / 正規表現で実装済み判定を行ってはならない。コード構造の存在確認は完了条件ではなく、テスト自身の文字列リテラルで自己充足し得るため禁止する。実装契約は公開API、helper、状態 machine、診断、queue、コールバック、ワーカー stop/join などを実際に呼び出すテストで固定する。`include_str!()` は VTS設定、rc、sepolicy、設計文書、別モジュールSSOTとの整合確認に限って補助的に使ってよい。
- framework コールバック の戻り値は必ず検査する。`let _ = callback...` による binder コールバック result の破棄を禁止する。
- コールバック失敗 は、対象 コールバック登録のcleanup、対象 オブジェクト の 失敗 / 閉鎖済み 遷移、診断ログ 記録を一体で行う。
- ワーカー が ロック失敗、registry不整合、record 不在、コールバック失敗 で終了する場合、無言停止 にせず 異常ワーカー停止 として対象オブジェクトを 失敗 / 閉鎖済み 状態に遷移させる。
- ワーカー の停止待ちは 停止信号 で wake できる 待機primitive を使う。DVR コールバック ワーカー は client 指定 interval の `thread::sleep` によって close / Drop / shutdown をブロックしてはならない。
- device node 不在、open 不可、permission 不足は `UNAVAILABLE` とする。device が存在する状態での 実行時ioctl失敗 / TS read 失敗 / pump 失敗 は `UNKNOWN_ERROR` とする。
- client不正入力 は `INVALID_ARGUMENT` とする。CS110 stream selector 指定、unknown monitor bit、負値または `default_max` 超過の `setMaxNumberOfFrontends()` は `INVALID_ARGUMENT` に固定する。
- product実行時 に 劣化frontend entry variant / generator / helper を置かない。device不在 は サービス 起動継続 + 診断情報記録 + frontend 非広告で扱う。
- 一時レビュー用 Markdown を リリースアーカイブ に同梱しない。恒久設計は `DESIGN_JA.md`、実装規約は `CODE_CONVENTION.md`、変更履歴は `CHANGELOG.md` に統合する。

## WorkerExit / scan 終了理由

リリースHAL経路 の ワーカー は、`panic` を thread join成功扱いにしてはならない。ワーカー ラッパー は `WorkerExit` を返し、`Normal` / `StopRequested` / `RuntimeFailure` / `PanicOrJoinFailure` を区別する。既存コードの読み替え互換として `Cancelled` / `Error` / `Panic` の alias を一時的に許容するが、正式な意味はそれぞれ `StopRequested` / `RuntimeFailure` / `PanicOrJoinFailure` である。`catch_unwind()` で捕捉した `panic` は `WorkerExit::PanicOrJoinFailure`、ワーカー body が検出した 実行時致命失敗 は `WorkerExit::RuntimeFailure` として 診断情報に反映する。

`frontend_live_pump`、`frontend_tune_worker`、`frontend_scan_worker` の 異常終了 は 影響を受けたfrontend runtime の `record_runtime_失敗()` に残し、ライブ data path と linked demux/filter/DVR を 閉鎖側失敗 にする。

scan session は 終了理由 を持つ。`Running`、`Completed`、`Cancelled`、`FailedBackend`、`FailedCallback`、`FailedPanic` を区別し、backendエラー や `panic` を 正常完了 として扱わない。

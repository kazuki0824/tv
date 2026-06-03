# Tuner HAL コーディング規則

## 1. 目的

このドキュメントは、Tuner HAL に固有のコーディング規則を固定する。プロジェクト全体の Rust 規約は `../GLOBAL_CODE_CONVENTION.md` の Rust 節に従う。Tuner HAL の状態遷移、戻り値、資源寿命、閉鎖側失敗対象は `DESIGN_JA.md` の「Tuner HAL 状態遷移表SSOT」を正とし、本書ではその契約を実装で破らないための禁止構文、helper 使用規則、静的確認観点だけを定める。

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

Tuner HAL の 公開API は、`DESIGN_JA.md` の状態遷移表を正とし、実装 helper では下表の基準で AIDL サービス固有エラーへ写像する。

| 状態 | 返すエラー | 方針 |
|---|---|---|
| クライアント引数不正 | `INVALID_ARGUMENT` | PID 範囲外、未知enum、CS110 stream selector 指定、BS relative stream number の backend 不一致など |
| lifecycle 不正 | `INVALID_STATE` | close 後操作、start 前 read、AV stream type 未設定など |
| 未対応機能 | `UNAVAILABLE` | 未対応 CI CAM、未対応 LNB、未対応 backend、未対応 relative stream number など |
| resource不足 | `NO_MEMORY` または `UNKNOWN_ERROR` | dma-buf確保失敗、FMQ 確保失敗 など。既存 AIDL補助関数 に合わせる |
| 対象tuner device不在 | `UNAVAILABLE` | 劣化起動 方針に従い、該当 frontend / resource を advertise しない、または open 時に unavailable |
| device node / ioctl 一時失敗 | `UNKNOWN_ERROR` | device は存在するが ioctl が予期せず失敗した場合 |
| 内部不変条件違反 | `UNKNOWN_ERROR` | `panic` せず診断情報に記録する。次状態と閉鎖側失敗対象は `DESIGN_JA.md` を正とする |
| コールバック remote 失敗 | メソッド自体は `panic` せず cleanup | remote オブジェクト dead / binder error はログ記録と cleanup にする。後続処理停止条件は `DESIGN_JA.md` を正とする |
| mutex汚染 | `UNKNOWN_ERROR` + 閉鎖側失敗 | 対象、次状態、後続APIの戻り値は `DESIGN_JA.md` を正とする |

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

### 5.3 実行時失敗 は設計表へ写像する

実行中の個別失敗はサービス全体の `panic` にせず、検出箇所、診断名、補助関数、低レベルエラー種別を明示して `DESIGN_JA.md` の表7と表8へ写像する。本節は実装側の検出・写像規約だけを定め、対象オブジェクト、戻り値、次状態、閉鎖側失敗対象は再定義しない。

| 検出点 | 実装規約 |
|---|---|
| 個別frontend open失敗 | device不在、permission不足、backend open失敗を区別して診断へ記録し、`DESIGN_JA.md` の該当行へ写像する |
| tune ioctl失敗 | backend error を `panic` にせず、tune / scan の worker error として戻せる型へ変換する |
| demux pump失敗 | pump loop の戻り値と診断名を残し、無言停止にしない |
| dma-buf確保失敗 | 確保失敗を `NO_MEMORY` または `UNKNOWN_ERROR` へ写像し、非AV filter 経路へ誤波及させない |
| コールバック remote dead | Binder error を破棄せず、callback cleanup と診断記録を行う |

## 6. Mutex 汚染 は 閉鎖側失敗

Tuner HAL における mutex汚染 recovery は閉鎖側失敗とする。`PoisonError::into_inner()` で通常復旧して処理継続してはならない。汚染済みロックに触れた公開メソッド / ワーカーは、成功扱いにせず、対象、戻り値、次状態、閉鎖側失敗対象を `DESIGN_JA.md` の表7と表8へ写像する。

本節は `lock_or_fail_closed()` 系 helper の使用、通常復旧禁止、診断記録だけを定める。ITuner registry、Frontend、Demux、Filter、DVR、共有メモリ、FMQ の状態遷移は本書で再定義しない。

診断要件:

```text
- poisoned mutex 名を ログ に出す
- 汚染回数を 診断情報に積む
- 閉鎖側失敗 により閉じた オブジェクト ID / type を 診断情報に残す
```

## 7. ロック / コールバック方針

Tuner HAL は以下を実装規約として固定する。

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
- `DESIGN_JA.md` の表7と表8へ写像できる `WorkerExit` または domain error を返す
- 次の公開メソッドが設計表どおりのエラーを返せる状態へ接続する
- 無言停止させない
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

## 10. 実装規約の静的確認観点

本節は、実装規約違反をレビュー時に検出するための静的確認観点を示す。リリース完了条件、WP完了条件、atest/VTS合格条件は本書では定義しない。

```text
1. リリースHAL経路 に `panic` / `unwrap` / expect / assert 系が残っていない
2. 公開Binderメソッド の エラー写像 が本書と実装 helper で一致している
3. デバイス検出 結果が capability広告 に反映されている
4. device不在 で サービスプロセス が `panic` しない
5. コールバック は ロック解放後に実行される
6. ワーカー終了 / エラー が診断情報に反映され、`DESIGN_JA.md` の状態遷移へ写像される
7. コールバック失敗 / ioctl 失敗 / dma-buf 失敗 が サービス プロセス終了 にならない
8. mutex汚染 は 閉鎖側失敗 になる
```


## 追加固定規約

- Tuner HAL の 単体テスト では、`include_str!("tuner_hal.rs")`、`include_str!("main.rs")` などで本体ソースを自己参照し、`contains()` / `find()` / `split()` / 正規表現で実装済み判定を行ってはならない。コード構造の存在確認は完了条件ではなく、テスト自身の文字列リテラルで自己充足し得るため禁止する。実装契約は公開API、helper、状態 machine、診断、queue、コールバック、ワーカー stop/join などを実際に呼び出すテストで固定する。`include_str!()` は VTS設定、rc、sepolicy、設計文書、別モジュールSSOTとの整合確認に限って補助的に使ってよい。
- FMQ薄層の Rust 呼び出しは、書き込み成功、short write、overflow、native write failure、EventFlag wake failure を区別できる checked helper に集約する。`tuner_fmq_queue_write_checked()` を既存実装名として扱い、write失敗を 0 byte 成功や overflow に丸めない。
- framework コールバック の戻り値は必ず検査する。`let _ = callback...` による binder コールバック result の破棄を禁止する。失敗時の状態遷移、診断名、後続処理停止条件は `DESIGN_JA.md` の表7と表8を正とする。
- コールバック失敗は、単独のログ出力で終えてはならない。cleanup、診断記録、後続処理停止条件は `DESIGN_JA.md` の表7と表8へ写像する。
- ワーカーがロック失敗、registry不整合、record 不在、コールバック失敗で終了する場合、無言停止にしてはならない。`WorkerExit` の分類と公開戻り値は `DESIGN_JA.md` の表7と表8へ写像する。
- ワーカー の停止待ちは 停止信号 で wake できる 待機primitive を使う。DVR コールバック ワーカー は client 指定 interval の `thread::sleep` によって close / Drop / shutdown をブロックしてはならない。
- device node 不在、open 不可、permission 不足は `UNAVAILABLE` とする。device が存在する状態での 実行時ioctl失敗 / TS read 失敗 / pump 失敗 は `UNKNOWN_ERROR` とする。
- client不正入力 は `INVALID_ARGUMENT` とする。CS110 stream selector 指定、unknown monitor bit、負値または `default_max` 超過の `setMaxNumberOfFrontends()` は `INVALID_ARGUMENT` に固定する。
- product実行時 に 劣化frontend entry variant / generator / helper を置かない。device不在 は サービス 起動継続 + 診断情報記録 + frontend 非広告で扱う。
- 一時レビュー用 Markdown と一時変更履歴ファイルを リリースアーカイブ に同梱しない。恒久設計は `DESIGN_JA.md`、実装規約は `CODE_CONVENTION.md` に統合する。恒久的な変更履歴は `README_JA.md` が指定する `CHANGELOG.md` だけに記録し、複数の履歴ファイルを作らない。未公開リリース候補のため、後方互換目的の alias、互換 field、旧API は非公開化ではなく削除する.

## WorkerExit / scan 終了理由

リリースHAL経路のワーカーは、`panic` を thread join成功扱いにしてはならない。ワーカーラッパーは `WorkerExit` を返し、`Normal` / `StopRequested` / `RuntimeFailure` / `PanicOrJoinFailure` を区別する。後方互換目的の `Cancelled` / `Error` / `Panic` alias は残さない。`catch_unwind()` で捕捉した `panic` は `WorkerExit::PanicOrJoinFailure`、ワーカー body が検出した実行時致命失敗は `WorkerExit::RuntimeFailure` として診断情報に反映する。

scan worker は終了理由を表す型を持ち、`Running`、`Completed`、`Cancelled`、`FailedBackend`、`FailedCallback`、`FailedPanic` を区別できるようにする。各終了理由の設計上の意味、影響範囲、次状態は `DESIGN_JA.md` の表7・表8と scan terminal state 節を正とし、本書では再定義しない。


## 11. DESIGN_JA.md から移設した実装規約

本節は実装記法とhelper使用規約だけを固定する。Tuner HAL の状態遷移、戻り値、資源寿命、閉鎖側失敗対象は `DESIGN_JA.md` の「Tuner HAL 状態遷移表SSOT」を正とし、本節で再定義しない。

Tuner HAL の release runtime path は、public Binder method、ワーカースレッド、コールバック配送、frontend backend、demux/filter/DVR/descrambler/LNB runtime state の全てで no-`panic` 境界 とする。`unwrap()`、`expect()`、`panic!()`、`unreachable!()`、`todo!()`、`unimplemented!()`、`assert*()`、`dbg!()` を runtime invariant の表現として使わない。HAL サービス登録失敗は、`panic` ではなく明示ログと process exit で fail-fast する。

Target tuner device が存在しない、または権限・device node・driver probing に失敗する場合は劣化起動 とする。HAL サービス自体は登録するが、存在しない frontend / demux / backend resource を capability として advertise しない。`getFrontendIds()` は実在 probe できた frontend だけを返す。存在しない resource への `openFrontend*`、`tune`、`scan` などの public Binder method は `UNAVAILABLE` または対応する service-specific error を返し、サービス起動を `panic` で中断しない。

mutex汚染は recover-with-inner ではなく閉鎖側失敗とする。runtime オブジェクトの mutex lock に失敗した場合は操作成功扱いにせず、Binder method では `UNKNOWN_ERROR` / service-specific error、内部 HAL path では `HalError::Internal`、非同期ワーカーでは診断ログと `WorkerExit::RuntimeFailure` 相当へ写像する。対象、次状態、後続APIの戻り値は `DESIGN_JA.md` を正とし、本書では再定義しない。汚染後に破損可能な状態を継続利用しない。

Public Binder method の error mapping は、入力不正を `INVALID_ARGUMENT`、未対応機能を `UNAVAILABLE`、状態不整合を `INVALID_STATE`、汚染や内部整合性崩壊を `UNKNOWN_ERROR` または `HalError::Internal` 起点の service-specific error に固定する。存在しないオブジェクトを返却する API では AOSP Tuner HAL の該当契約に従い `NAME_NOT_FOUND` または同等の service-specific not-found error を使う。成功を返す場合は、対象 state mutation または query が汚染なしに完了していなければならない。

ワーカースレッドは `WorkerRuntime::spawn_owned()` または `WorkerRuntime::spawn_owned_with_exit_hook()` を通して生成し、entrypoint `panic` を worker runtime 内で捕捉して診断ログに残す。ワーカーの停止待ちは `WorkerHandle::request_stop()` / `WorkerHandle::wake()` / `WorkerHandle::join_from_owner()` に集約し、`panic` stop を黙殺しない。ワーカー内の mutex汚染や backend error は、通常停止と区別できる終了分類へ写像し、`panic` で HAL process を落とさない。所有 object の状態遷移は `DESIGN_JA.md` を正とする。

release HAL path の静的確認では、non-test runtime から `unwrap()` / `expect()` / `panic!()` / `unreachable!()` / `thread::spawn` 直接呼び出し / silent `join()` を禁止する。`#[cfg(test)]` と `tests/` 配下は対象外とする。`WorkerRuntime::spawn_owned*` と `WorkerHandle::join_from_owner()` は、ワーカー policy を実装する runtime ラッパーとして許可する。

## r50dz17: 共通部品経由の実装規約

Tuner HALのrelease HAL pathでは、次の直接実装を禁止する。

- `std::sync::Mutex::lock()` を公開AIDL実装、worker、runtime objectから直接呼ぶこと。同期処理は `hal_sync` へ集約する。
- `PoisonError::into_inner()` によりmutex汚染から通常復旧すること。
- lock失敗、wait失敗、join失敗、FMQ操作失敗を、正常停止、空queue、timeout、0 byte、no-op successへ丸めること。
- 各HAL objectが`JoinHandle`、`Condvar`、`AtomicBool`を直接組み合わせてworker制御すること。worker制御は `worker_runtime` へ集約する。
- open、close、configure、rollback、cleanupで、台帳更新とruntime登録を各APIが分散して手書きすること。状態変更は `lifecycle_txn` と `registry_ledger` へ集約する。
- Drop限定のbest-effort処理をpublic API主経路へ流用すること。
- HAL objectから`fmq_shim`を直接呼ぶこと。FMQ操作は `fmq_queue` へ集約する。
- binder_service内に新しいTS/PES/start-code parserを追加すること。TS/PES/record event処理は `packet_pipeline` と `record_index` へ集約する。

r50dz17では共通部品骨格の追加だけを行い、既存実行経路は変更しない。既存経路の置換は、以後の作業でテストを追加しながら行う。


### r50dz19: 共通部品化後の残存禁止事項

- `tuner_hal.rs` から `tuner_fmq_*` FFI symbol を直接参照してはならない。FMQ 接続は `fmq_queue.rs` だけに置く。
- `PoisonError::into_inner()` による通常復旧を追加してはならない。
- worker signal の lock / wait 失敗を `true`、`false`、timeout、normal wake へ丸めてはならない。
- FMQ fill 取得失敗を `0 byte` として返してはならない。
- record event 用の PES timestamp / start-code scanner を binder service 側へ再追加してはならない。追加する場合は `record_index` へ移す。


### r50dz20: lock guard破棄後再使用の禁止

- `drop(guard)` で明示破棄したlock guardを同一scopeで再使用してはならない。
- 台帳修復処理でguardを保持する必要がある場合は、scopeを分けずに同一guard上で修復を完了する。

## r50dz21: WP-04 補修後の追加禁止事項

- `current_fill_bytes()` は `usize` を直接返してはならない。失敗を `BinderResult<usize>` で返す。
- LNB 操作ロック台帳の取得で `expect()` / `unwrap()` を使ってはならない。
- `soft_demux/src/lib.rs` に `TsPacketView` を再定義してはならない。TS packet view は `packet_pipeline.rs` の定義を使う。
- malformed TS のみを読み取った DVR playback 入力を成功消費として扱ってはならない。

## r50dz22: WP-04 補修後の追加実装規約

- `worker_runtime.rs` 内でも release HAL path の `expect()` / `panic` を使わない。worker signal の lock / wait 失敗は `runtime_failure` として記録する。
- DVR callback wake で `Mutex::lock().expect()` を使わない。wake failure は公開経路では戻り値、best-effort 経路では診断ログで扱う。
- record event のために binder_service 側へ `TsPacketRecordView`、`StartCodeInfo`、`BitReader`、start-code scanner、PES timestamp decoder を置かない。
- TS packet view の拡張は `packet_pipeline.rs` で行う。
- record index parser の拡張は `record_index.rs` で行う。

### r50dz23: WP-04 補修後の残存禁止

- `tuner_hal.rs` に worker join 実装、worker handle wrapper、worker exit enum を再追加してはならない。worker制御は `worker_runtime.rs` に置く。
- LNB操作ロック台帳を `tuner_hal.rs` に再追加してはならない。LNB ID とロックの対応は `registry_ledger.rs` の `LnbLedger` で管理する。
- `configure_filter_with_summary_result()` では、失敗し得る採番・容量検証を状態変更後に置いてはならない。
- playback入力は、入力が全て破棄された場合に成功扱いとしてはならない。

## r50dz24: WP-04 補修後の残存禁止

- `tuner_hal.rs`から`fmq_queue_*`、`tuner_fmq_*`、`TunerFmqQueue`を直接参照してはならない。
- `tuner_hal.rs`内に`poisoned_lock_status`、`lock_mutex_status`、`lock_mutex_hal`、`lock_mutex_io`、`lock_mutex_option`の実装を置いてはならない。これらは`hal_sync`側へ置く。
- live pumpおよびDVR callback waitでmutex汚染・condvar wait失敗を`return`だけで正常扱いしてはならない。

- DVR callback worker用に`Arc<(Mutex<bool>, Condvar)>`の専用wake flagを追加してはならない。owner `WorkerHandle` / `ConcreteWorkerSignal` を使う。


## WP-04 直接 lock 検査の例外

`WP-04-2-5` の直接 `lock()` 検査で `#[cfg(test)]` 配下の単体テストだけが検出される場合、その検出は public API 主経路の残存とは扱わない。

例外条件は次に限定する。

- 対象は `#[cfg(test)]` 配下のテスト関数、テスト補助関数、テスト用 fixture だけである。
- production 経路、HAL object の public API、worker、FMQ、registry、descrambler session、stream boundary の本処理では `hal_sync` または対象共通部品を使う。
- テスト内の `lock().unwrap()` はテスト fixture の観測・準備専用であり、mutex 汚染を production 成功扱いへ丸める根拠にしてはならない。
- production ファイルで直接 `lock()` が必要になった場合は、この例外へ含めず、対象関数、理由、失敗時の扱いを個別に追記する。

## 12. 失敗領域の混同禁止

- callback未登録、callback Binder error、scan通知失敗を frontend backend failure として扱ってはならない。
- FMQ / AV shared backing の水位取得失敗を、queue破損またはdata path破損と同一視してはならない。
- backend ioctl/read/tune/stop失敗だけを backend failure として扱う。
- lifecycle違反、owner不一致、foreign object、closed object は対象APIの `INVALID_STATE` / `INVALID_ARGUMENT` に写像し、backend failureへ昇格させない。

## 13. public API transaction 実装規約

- public Binder method は、validate → prepare → commit の順に実装する。
- validate段階で公開状態を変更してはならない。
- prepare段階で旧queue、旧backing、旧binding、旧tokenを破棄してはならない。
- commit前に失敗した場合は、prepareで確保した資源だけをrollbackする。
- commit後に失敗した場合は、成功扱いで継続せず、対象objectをquarantineまたはFailedClosingへ移す。
- public API内で `let _ = cleanup...` により critical cleanup 失敗を握りつぶしてはならない。

## 14. 同一条件 no-op guard

- `setFrontendDataSource()` は、現在と同一frontend/generationなら stream boundary reset を呼ばない。
- `tune()` は、現在と同一 normalized tune settings なら backend stop、live pump停止、demux boundary reset を呼ばない。
- `configure()` は、現在設定と同一なら queue / AV backing / DVR backing を破棄しない。
- no-op guard は破壊的処理の前に置く。

## 15. best-effort 使用制限

- `best_effort` と名付けた関数を public API の主経路で使ってはならない。
- Drop / teardown 以外で `best_effort` を使う場合は、失敗が public API の戻り値・状態遷移に影響しない補助診断に限る。
- queue clear、registry unregister、backend stop、token release、worker join は best-effort で沈黙させない。
- 失敗を返せない場所では、診断名、対象ID、失敗段階を必ず記録する。

## 16. 寿命ID / generation / token 実装規約

- lifetime ID、generation、worker wake generation、token ID に `saturating_add()` を使ってはならない。
- wrap可能な `fetch_add()` を release HAL path に追加してはならない。
- 上限到達時は `checked_add()` 失敗として扱い、対象objectをquarantineまたは新規発行失敗にする。
- expired token は保持目的が明示されない限り table から削除する。
- 0、負値、予約値を通常発行IDとして使ってはならない。

## 17. backend診断分離

- DVB backend の失敗を px4 診断構造へ記録してはならない。
- px4 backend の失敗を DVB 診断構造へ記録してはならない。
- frontend共通処理から backend failure を記録する場合は、backend種別を引数として受け取り、対応する診断名前空間だけを更新する。

## 18. source filter 実装規約

- source filter downstream は `DESIGN_JA.md` の source filter downstream 契約表にある組み合わせだけを実装する。
- 未対応の downstream 組み合わせを成功 no-op にしてはならない。
- raw TS source を downstream へ渡す場合は、TEI、continuity、discontinuity、duplicate、flush generation の判定を通常入力と同じ経路で通す。
- section/PES/AV/record payload を別filterのsourceとして直接再配送する経路を追加してはならない。ただし `DESIGN_JA.md` で対応に変更した場合を除く。


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

## 4. AIDLエラー変換の集約規約

Tuner HALの公開AIDL戻り値、status precedence、次状態、資源変化、閉鎖側失敗対象は`DESIGN_JA.md`の「Tuner HAL 状態遷移表SSOT」だけを正本とする。本書は具体的な`android.hardware.tv.tuner.Result`値の対応表を持たず、低レベル失敗を正本の分類へ接続する実装規約だけを定める。

```text
- device、FMQ、共有メモリ、dma-buf、callback、workerの低レベル失敗は、原因を保持する型付きdomain errorへ変換する
- 容量不足、未対応、入力不正、lifecycle不正、backend内部障害をgeneric errorへ早期に丸めない
- 公開Binder statusへの最終変換は`binder_service`内の状態補助関数またはエラー変換補助関数へ集約する
- Binder method、worker、backend adapter、個別resource helperで公開Result値を直接選択しない
- helper側の分類が`DESIGN_JA.md`の公開契約と矛盾する場合は`DESIGN_JA.md`を正としてhelperを修正する
```

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

対象 device node / frontend が存在しない場合でも、HAL サービス自体は起動する。ただし、存在しない frontend / demux / backend resource を capability として advertise してはならない。該当resourceへの公開結果は`DESIGN_JA.md`の能力・状態表へ写像する。

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
| dma-buf確保失敗 | 容量不足と容量不足ではない内部障害を型付きdomain errorで区別し、公開結果は`DESIGN_JA.md`の該当行へ集約して、非AV filter経路へ誤波及させない |
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
- AVの1event上限、filter別未解放総量、runtime総量を分離し、codec・allocator・実機証跡からProductProfileごとに導出する。全codec共通の固定byte値を能力契約にしない
- capabilityは実際に同時予約が必要な依存閉包ごとに原子的に確定し、無関係な閉包の予約失敗を波及させない。最終snapshotの横断不変条件は合成後に一括検証する
- DVR再生 / 録画 を 対応宣言する場合は、ワーカー 失敗 / queue overflow を 状態 として返す
```

## 10. 実装規約の静的確認観点

本節は、実装規約違反をレビュー時に検出するための静的確認観点を示す。リリース完了条件、WP完了条件、atest/VTS合格条件は本書では定義しない。

```text
1. リリースHAL経路 に `panic` / `unwrap` / expect / assert 系が残っていない
2. 公開Binderメソッドの最終status変換が`DESIGN_JA.md`の公開契約と一致し、実装helperに別の公開写像表がない
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
- device node不在、open不可、permission不足と、device存在下の実行時ioctl/read/pump失敗を型付きdomain errorで区別し、公開結果は`DESIGN_JA.md`へ集約する。
- client入力不正は、CS110 stream selector指定、unknown monitor bit、負値または`default_max`超過の`setMaxNumberOfFrontends()`などの入力分類を保持したtyped validation errorとし、公開結果は`DESIGN_JA.md`へ集約する。
- product実行時 に 劣化frontend entry variant / generator / helper を置かない。device不在 は サービス 起動継続 + 診断情報記録 + frontend 非広告で扱う。
- 一時レビュー用 Markdown と一時変更履歴ファイルを リリースアーカイブ に同梱しない。恒久設計は `DESIGN_JA.md`、実装規約は `CODE_CONVENTION.md` に統合する。恒久的な変更履歴は `README_JA.md` が指定する `CHANGELOG.md` だけに記録し、複数の履歴ファイルを作らない。未公開リリース候補のため、後方互換目的の alias、互換 field、旧API は非公開化ではなく削除する.

## WorkerExit / scan 終了理由

リリースHAL経路のワーカーは、`panic` を thread join成功扱いにしてはならない。ワーカーラッパーは `WorkerExit` を返し、`Normal` / `StopRequested` / `RuntimeFailure` / `PanicOrJoinFailure` を区別する。後方互換目的の `Cancelled` / `Error` / `Panic` alias は残さない。`catch_unwind()` で捕捉した `panic` は `WorkerExit::PanicOrJoinFailure`、ワーカー body が検出した実行時致命失敗は `WorkerExit::RuntimeFailure` として診断情報に反映する。

scan worker は終了理由を表す型を持ち、`Running`、`Completed`、`Cancelled`、`FailedBackend`、`FailedCallback`、`FailedPanic` を区別できるようにする。各終了理由の設計上の意味、影響範囲、次状態は `DESIGN_JA.md` の表7・表8と scan terminal state 節を正とし、本書では再定義しない。


## 11. DESIGN_JA.md から移設した実装規約

本節は実装記法とhelper使用規約だけを固定する。Tuner HAL の状態遷移、戻り値、資源寿命、閉鎖側失敗対象は `DESIGN_JA.md` の「Tuner HAL 状態遷移表SSOT」を正とし、本節で再定義しない。

Tuner HAL の release runtime path は、public Binder method、ワーカースレッド、コールバック配送、frontend backend、demux/filter/DVR/descrambler/LNB runtime state の全てで no-`panic` 境界 とする。`unwrap()`、`expect()`、`panic!()`、`unreachable!()`、`todo!()`、`unimplemented!()`、`assert*()`、`dbg!()` を runtime invariant の表現として使わない。HAL サービス登録失敗は、`panic` ではなく明示ログと process exit で fail-fast する。

Target tuner device が存在しない、または権限・device node・driver probing に失敗する場合は劣化起動とする。HALサービス自体は登録するが、存在しないfrontend / demux / backend resourceをcapabilityとしてadvertiseしない。`getFrontendIds()`は実在probeできたfrontendだけを返す。存在しないresourceへの公開結果は`DESIGN_JA.md`の該当状態表へ写像し、サービス起動を`panic`で中断しない。

mutex汚染はrecover-with-innerではなく閉鎖側失敗とする。runtime objectのmutex lockに失敗した場合は操作成功扱いにせず、内部HAL pathでは型付きinternal failure、非同期workerでは診断ログと`WorkerExit::RuntimeFailure`相当へ写像する。公開結果、対象、次状態、後続APIの戻り値は`DESIGN_JA.md`を正とし、本書では再定義しない。汚染後に破損可能な状態を継続利用しない。

Public Binder methodの最終error mappingは`DESIGN_JA.md`だけを正本とし、本書では入力不正、未対応、lifecycle不整合、not-found、容量不足、内部障害のtyped domain分類を失わず`binder_service`の単一変換境界へ渡すことだけを固定する。成功を返す場合は、対象state mutationまたはqueryが汚染なしに完了していなければならない。

ワーカースレッドは `WorkerRuntime::spawn_owned()` または `WorkerRuntime::spawn_owned_with_exit_hook()` を通して生成し、entrypoint `panic` を worker runtime 内で捕捉して診断ログに残す。ワーカーの停止待ちは `WorkerHandle::request_stop()` / `WorkerHandle::wake()` / `WorkerHandle::join_from_owner()` に集約し、`panic` stop を黙殺しない。ワーカー内の mutex汚染や backend error は、通常停止と区別できる終了分類へ写像し、`panic` で HAL process を落とさない。所有 object の状態遷移は `DESIGN_JA.md` を正とする。

release HAL path の静的確認では、non-test runtime から `unwrap()` / `expect()` / `panic!()` / `unreachable!()` / `thread::spawn` 直接呼び出し / silent `join()` を禁止する。`#[cfg(test)]` と `tests/` 配下は対象外とする。`WorkerRuntime::spawn_owned*` と `WorkerHandle::join_from_owner()` は、ワーカー policy を実装する runtime ラッパーとして許可する。

## 12. 共通部品経由の実装規約

Tuner HAL の release HAL path では、次の直接実装を禁止する。

- `std::sync::Mutex::lock()` を公開AIDL実装、worker、runtime object から直接呼ぶこと。同期処理は `hal_sync` へ集約する。
- `PoisonError::into_inner()` により mutex汚染から通常復旧すること。
- lock失敗、wait失敗、join失敗、FMQ操作失敗を、正常停止、空queue、timeout、0 byte、no-op successへ丸めること。
- 各HAL objectが `JoinHandle`、`Condvar`、`AtomicBool` を直接組み合わせて worker 制御すること。worker制御は `worker_runtime` へ集約する。
- open、close、configure、rollback、cleanupで、台帳更新と runtime 登録を各APIが分散して手書きすること。状態変更は `lifecycle_txn` と `registry_ledger` へ集約する。
- Drop限定の best-effort 処理を public API 主経路へ流用すること。
- HAL object から `fmq_shim` を直接呼ぶこと。FMQ操作は `fmq_queue` へ集約する。
- binder_service 内に新しい TS/PES/start-code parser を追加すること。TS/PES/record event 処理は `packet_pipeline` と `record_index` へ集約する。

### 12.1 共通部品化後の残存禁止事項

- `tuner_hal.rs` から `tuner_fmq_*`、`fmq_queue_*`、`TunerFmqQueue` を直接参照してはならない。FMQ 接続は `fmq_queue.rs` に置く。
- `tuner_hal.rs` 内に `poisoned_lock_status`、`lock_mutex_status`、`lock_mutex_hal`、`lock_mutex_io`、`lock_mutex_option` の実装を置いてはならない。これらは `hal_sync` 側へ置く。
- worker signal の lock / wait 失敗を `true`、`false`、timeout、normal wake へ丸めてはならない。
- FMQ fill 取得失敗を `0 byte` として返してはならない。`current_fill_bytes()` は失敗を戻り値で表現する。
- DVR callback wake で `Mutex::lock().expect()` を使わない。wake failure は公開経路では戻り値、best-effort 経路では診断ログで扱う。
- record event 用の PES timestamp / start-code scanner を binder_service 側へ再追加してはならない。追加する場合は `record_index` へ移す。
- `tuner_hal.rs` に worker join 実装、worker handle wrapper、worker exit enum を再追加してはならない。worker制御は `worker_runtime.rs` に置く。
- DVR callback worker 用に `Arc<(Mutex<bool>, Condvar)>` の専用 wake flag を追加してはならない。owner `WorkerHandle` / `ConcreteWorkerSignal` を使う。
- LNB操作ロック台帳を `tuner_hal.rs` に再追加してはならない。LNB ID とロックの対応は `registry_ledger.rs` の `LnbLedger` で管理する。
- LNB 操作ロック台帳の取得で `expect()` / `unwrap()` を使ってはならない。
- `soft_demux/src/lib.rs` に `TsPacketView` を再定義してはならない。TS packet view は `packet_pipeline.rs` の定義を使う。
- record event のために binder_service 側へ `TsPacketRecordView`、`StartCodeInfo`、`BitReader`、start-code scanner、PES timestamp decoder を置かない。
- TS packet view の拡張は `packet_pipeline.rs` で行う。
- record index parser の拡張は `record_index.rs` で行う。
- malformed TS のみを読み取った DVR playback 入力を成功消費として扱ってはならない。
- `configure_filter_with_summary_result()` では、失敗し得る採番・容量検証を状態変更後に置いてはならない。
- playback入力は、入力が全て破棄された場合に成功扱いとしてはならない。
- `drop(guard)` で明示破棄した lock guard を同一 scope で再使用してはならない。台帳修復処理で guard を保持する必要がある場合は、同一 guard 上で修復を完了する。

### 12.2 直接 lock 検査の test 例外

直接 `lock()` 検査で `#[cfg(test)]` 配下の単体テストだけが検出される場合、その検出は public API 主経路の残存とは扱わない。

例外条件は次に限定する。

- 対象は `#[cfg(test)]` 配下のテスト関数、テスト補助関数、テスト用 fixture だけである。
- production 経路、HAL object の public API、worker、FMQ、registry、descrambler session、stream boundary の本処理では `hal_sync` または対象共通部品を使う。
- テスト内の `lock().unwrap()` はテスト fixture の観測・準備専用であり、mutex 汚染を production 成功扱いへ丸める根拠にしてはならない。
- production ファイルで直接 `lock()` が必要になった場合は、この例外へ含めず、対象関数、理由、失敗時の扱いを個別に追記する。

## 13. 失敗領域の混同禁止

- callback未登録、callback Binder error、scan通知失敗を frontend backend failure として扱ってはならない。
- FMQ / AV shared backing の水位取得失敗を、queue破損またはdata path破損と同一視してはならない。
- backend ioctl/read/tune/stop失敗だけを backend failure として扱う。
- lifecycle違反、owner不一致、foreign object、closed objectは対象APIのtyped validation/lifecycle failureとして保持し、backend failureへ昇格させない。公開結果のprecedenceは`DESIGN_JA.md`を正とする。

## 14. public API transaction 実装規約

- public Binder method は、validate → prepare → commit の順に実装する。
- validate段階で公開状態を変更してはならない。
- prepare段階で旧queue、旧backing、旧binding、旧tokenを破棄してはならない。
- commit前に失敗した場合は、prepareで確保した資源だけをrollbackする。
- commit後の副次処理が失敗した場合は、`DESIGN_JA.md`の当該API状態表が定める公開結果、次状態、後片付け方針をそのまま適用する。確定済み主処理を維持して型付き診断だけを残す場合、通常動作を継続する場合、`CleanupPending`として再試行権限を移す場合、実状態を確定できない対象だけを`Quarantined`へ移す場合を区別する。閉鎖操作も一律に`FailedClosing`へ丸めず、interface別close表に従う。実装規約側で公開状態または戻り値を再定義してはならない。
- public API内で `let _ = cleanup...` により critical cleanup 失敗を握りつぶしてはならない。

## 15. 同一条件の安全な非破壊最適化

- `setFrontendDataSource()` は、現在と同一frontend/generationなら stream boundary reset を呼ばない。
- 公開`IFrontend.tune()`は、前回tuneが未完了ならAOSP契約どおり旧tuneを停止・遮断して新要求を開始する。完了済み`Locked`で、normalized settings、typed selector、LNB/power条件、backend状態、stream boundaryの同値性と健全性をtransaction lock下の単一snapshotで証明できる場合は、request sequenceだけを更新し、stream generation、worker、backend要求、demux境界、AVを維持する非破壊re-entryを許可する。
- 非破壊re-entryでは現lock snapshotに対応する`LOCKED`を新request sequenceへlock外で1回配送する。条件不一致、旧tune未完了、scan中、Failed/cleanup中、callback終端未確定、同値性または健全性を証明できない場合はno-op guardへ入れず、`DESIGN_JA.md`のfull retune transactionへ進める。
- `configure()` は、現在設定と同一なら queue / AV backing / DVR backing を破棄しない。
- 同一条件の非破壊最適化は、各公開APIの状態遷移、generation、commit point、callback、stream boundary契約を変えない範囲で、破壊的処理の前にだけ適用する。

## 16. best-effort 使用制限

- `best_effort` と名付けた関数を public API の主経路で使ってはならない。
- Drop / teardown 以外で `best_effort` を使う場合は、失敗が public API の戻り値・状態遷移に影響しない補助診断に限る。
- queue clear、registry unregister、backend stop、token release、worker join は best-effort で沈黙させない。
- 失敗を返せない場所では、診断名、対象ID、失敗段階を必ず記録する。

## 17. 寿命ID / generation / token 実装規約

- lifetime ID、generation、worker wake generation、token ID に `saturating_add()` を使ってはならない。
- wrap可能な `fetch_add()` を release HAL path に追加してはならない。
- 上限到達時は `checked_add()` 失敗として扱い、対象objectをquarantineまたは新規発行失敗にする。
- expired token は保持目的が明示されない限り table から削除する。
- 0、負値、予約値を通常発行IDとして使ってはならない。

## 18. backend診断分離

- DVB backend の失敗を px4 診断構造へ記録してはならない。
- px4 backend の失敗を DVB 診断構造へ記録してはならない。
- frontend共通処理から backend failure を記録する場合は、backend種別を引数として受け取り、対応する診断名前空間だけを更新する。

## 19. source filter 実装規約

- source filter downstream は `DESIGN_JA.md` の source filter downstream 契約表にある組み合わせだけを実装する。
- 未対応の downstream 組み合わせを成功 no-op にしてはならない。
- raw TS source を downstream へ渡す場合は、TEI、continuity、discontinuity、duplicate、flush generation の判定を通常入力と同じ経路で通す。
- section/PES/AV/record payload を別filterのsourceとして直接再配送する経路を追加してはならない。ただし `DESIGN_JA.md` で対応に変更した場合を除く。


## 20. 旧 tuner_hal 参照実装固有の実装規約

本節は旧 `tuner_hal` 参照実装だけを拘束する。product default の `tuner_hal2` に適用する実装規約は `../tuner_hal2/CODE_CONVENTION.md`、実装owner / anchor / 許可entry pointは `../tuner_hal2/DESIGN_JA.md` を正とし、本節の具体type / helper名を `tuner_hal2` の規範として転用してはならない。両実装に共通化すべき規約が生じた場合も、両CODE_CONVENTIONへ同じ規範を重複定義せず、所有文書を一意にして他方から参照する。

公開AIDL意味ではなく旧 `tuner_hal` の実装方法に属する次の事項は本節を正本とする。

- scan END callbackを返すhelperの結果を`let _ =`等で破棄しない。配送失敗はtyped resultとして上位ownerへ返す。テスト専用helper/entryは`#[cfg(test)]`等のcompile-time gateでrelease経路から除外する。
- AV shared allocationの`active/reserved/free`、次ID/generation、diagnosticを一つの`AvSharedState`と一つのmutex配下で更新し、`clear_result()` / `release()` / `release_all()`は部分更新を残さない。
- px4 backendは同一device nodeを一度だけopenし、TS readerはcontrol `File`から`File::try_clone()` / fd duplicate相当で派生させる。readerはnonblocking + `poll()`で扱い、live reader取得のための再openを禁止する。
- Filter/DVR queue policyの具体実装は`filter_queue_model()` / `dvr_queue_model()`および`QueueOverflowPolicy`へ集約し、旧alias/boolean policyを再導入しない。
- MediaCas session ID bytesとinternal key resourceの登録entryは`register_from_cas_bridge()`へ集約する。`dump_descrambler_diagnostics_for_debug()`、`MALEICACID_TUNER_HAL_DESCRAMBLER_DIAGNOSTIC_FILE`等のdebug出力は公開AIDL契約を変更しない診断経路に限定し、debug file writeは5秒以内のbounded operationとする。

これらの名称・lock/API選択は旧 `tuner_hal` の実装規約であり、`DESIGN_JA.md`の公開状態・capability・戻り値・資源寿命、または`tuner_hal2`の実装owner/anchorを変更する根拠にはしない。

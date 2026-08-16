# 旧 Tuner HAL 参照実装コーディング規則

本書は旧 `tuner_hal` 参照実装だけを拘束する。プロジェクト全体の Rust 規約は `../GLOBAL_CODE_CONVENTION.md`、Tuner HAL の公開状態・戻り値・capability・資源寿命・transaction / cleanup / generation の論理契約は `DESIGN_JA.md`、product default の `tuner_hal2` に適用する現行実装規約は `../tuner_hal2/CODE_CONVENTION.md`、現行実装owner / anchor / 許可entry pointは `../tuner_hal2/DESIGN_JA.md` を正とする。

旧実装の具体type / helper / module名を `tuner_hal2` の規範として転用してはならない。両実装に共通する規則を本書と `tuner_hal2/CODE_CONVENTION.md` に重複定義せず、project-wide規則は `GLOBAL_CODE_CONVENTION.md`、公開・論理契約は `DESIGN_JA.md`、現行実装規約は `tuner_hal2/CODE_CONVENTION.md` に一意に置く。

## 20. 旧 tuner_hal 参照実装固有の実装規約

公開AIDL意味ではなく旧 `tuner_hal` の実装方法に属する次の事項は本節を正本とする。

- 旧`binder_service`では公開Binder statusへの最終変換を状態補助関数またはエラー変換補助関数へ集約し、Binder method、worker、backend adapter、個別resource helperが公開Result値を直接選択しない。
- 旧実装のruntime lockは`hal_sync`へ集約し、公開AIDL実装、worker、runtime objectから`std::sync::Mutex::lock()`を直接呼ばない。`PoisonError::into_inner()`で通常復旧しない。
- 旧実装のworker生成は`worker_runtime`へ集約し、各HAL objectが`JoinHandle`、`Condvar`、`AtomicBool`を組み合わせた独自worker lifecycleを持たない。`WorkerRuntime::spawn_owned()` / `spawn_owned_with_exit_hook()`と`WorkerHandle::request_stop()` / `wake()` / `join_from_owner()`を旧実装の正規入口とする。
- 旧実装のopen / close / configure / rollback / cleanupで台帳更新とruntime登録を各APIへ分散せず、`lifecycle_txn`と`registry_ledger`へ集約する。
- 旧HAL objectから`fmq_shim`を直接呼ばず、FMQ操作は`fmq_queue`へ集約する。lock / wait / join / FMQ失敗を正常停止、空queue、timeout、0 byte、no-op successへ丸めない。
- 旧FMQ薄層のchecked write入口は`tuner_fmq_queue_write_checked()`へ集約し、write success、short write、overflow、native write failure、EventFlag wake failureを区別する。
- 旧`binder_service`へTS/PES/start-code parserを追加せず、TS/PES/record event処理は`packet_pipeline`と`record_index`へ集約する。
- scan END callbackを返すhelperの結果を`let _ =`等で破棄しない。配送失敗はtyped resultとして上位ownerへ返す。テスト専用helper/entryは`#[cfg(test)]`等のcompile-time gateでrelease経路から除外する。
- 旧worker診断はruntime errorとpanic/join failureを別分類で記録し、`worker_error_count`と`worker_panic_count`相当を混同しない。旧scan workerの終了理由は`Running`、`Completed`、`Cancelled`、`FailedBackend`、`FailedCallback`、`FailedPanic`を区別し、後方互換aliasで丸めない。公開状態・影響範囲は`DESIGN_JA.md`を正とする。
- AV shared allocationの`active/reserved/free`、次ID/generation、diagnosticを一つの`AvSharedState`と一つのmutex配下で更新し、`clear_result()` / `release()` / `release_all()`は部分更新を残さない。
- px4 backendは同一device nodeを一度だけopenし、TS readerはcontrol `File`から`File::try_clone()` / fd duplicate相当で派生させる。readerはnonblocking + `poll()`で扱い、live reader取得のための再openを禁止する。
- Filter/DVR queue policyの具体実装は`filter_queue_model()` / `dvr_queue_model()`および`QueueOverflowPolicy`へ集約し、旧alias/boolean policyを再導入しない。
- MediaCas session ID bytesとinternal key resourceの登録entryは`register_from_cas_bridge()`へ集約する。`dump_descrambler_diagnostics_for_debug()`、`MALEICACID_TUNER_HAL_DESCRAMBLER_DIAGNOSTIC_FILE`等のdebug出力は公開AIDL契約を変更しない診断経路に限定し、debug file writeは5秒以内のbounded operationとする。
- `tuner_hal.rs`から`tuner_fmq_*`、`fmq_queue_*`、`TunerFmqQueue`を直接参照せず、FMQ接続は`fmq_queue.rs`に置く。
- `tuner_hal.rs`内に`poisoned_lock_status`、`lock_mutex_status`、`lock_mutex_hal`、`lock_mutex_io`、`lock_mutex_option`の実装を置かず、`hal_sync`側へ置く。
- worker signalのlock / wait失敗を`true`、`false`、timeout、normal wakeへ丸めない。
- FMQ fill取得失敗を`0 byte`として返さず、`current_fill_bytes()`は失敗を戻り値で表現する。
- DVR callback wakeで`Mutex::lock().expect()`を使わない。wake failureは公開経路では戻り値、best-effort経路では診断ログで扱う。
- record event用のPES timestamp / start-code scannerを`binder_service`側へ置かず、`record_index`へ置く。
- `tuner_hal.rs`にworker join実装、worker handle wrapper、worker exit enumを置かず、worker制御は`worker_runtime.rs`に置く。
- DVR callback worker用に`Arc<(Mutex<bool>, Condvar)>`の専用wake flagを追加せず、owner `WorkerHandle` / `ConcreteWorkerSignal`を使う。
- LNB操作ロック台帳を`tuner_hal.rs`に置かず、LNB IDとロックの対応は`registry_ledger.rs`の`LnbLedger`で管理する。LNB操作ロック台帳の取得で`expect()` / `unwrap()`を使わない。
- `soft_demux/src/lib.rs`に`TsPacketView`を再定義せず、TS packet viewは`packet_pipeline.rs`の定義を使う。TS packet viewの拡張も`packet_pipeline.rs`で行う。
- record eventのために`binder_service`側へ`TsPacketRecordView`、`StartCodeInfo`、`BitReader`、start-code scanner、PES timestamp decoderを置かず、record index parserの拡張は`record_index.rs`で行う。
- malformed TSのみを読み取ったDVR playback入力、または入力が全て破棄されたplayback入力を成功消費として扱わない。
- `configure_filter_with_summary_result()`では、失敗し得る採番・容量検証を状態変更後に置かない。
- `drop(guard)`で明示破棄したlock guardを同一scopeで再使用しない。台帳修復処理でguardを保持する必要がある場合は同一guard上で修復を完了する。
- 旧実装の直接`lock()`禁止の静的検査では、`#[cfg(test)]`配下のテスト関数、テスト補助、fixtureだけをproduction残存と数えない。productionのpublic API、worker、FMQ、registry、descrambler session、stream boundaryでは`hal_sync`または各旧正規部品を使う。テスト内`lock().unwrap()`をproductionのmutex汚染成功扱いの根拠にしない。

これらの名称・lock/API選択は旧 `tuner_hal` の実装規約であり、`DESIGN_JA.md`の公開状態・capability・戻り値・資源寿命、または`tuner_hal2`の実装owner/anchorを変更する根拠にはしない。

# 変更履歴

## r50dz86

- G1-13: source filter が closed または runtime failed の場合、`setDataSource()` 系の local filter 解決を `INVALID_STATE` に分離した。別 demux、非local filter、未open demux filter は `INVALID_ARGUMENT` のままとする。
- G1-12: LNB backend へ新状態を適用した後の registry commit 失敗で、旧状態 rollback を先に試行し、rollback 失敗時だけ voltage/tone/position を安全状態へ再投入する経路を追加した。
- G2-07: unbounded PES が 1MiB を超えた場合の即時 clear/drop を廃止し、chunk delivery と lifecycle boundary delivery に変更した。
- binder_service build gate: 未読 `queue_ring` 初期代入を削除し、未使用 FMQ FFI wrapper 群を削除した。
- 検証スクリプトは `m -k 0 -j"$JOBS"` を維持する。

## r50dz85

- r50dz84 の `m -k 0` 検証で、残件が binder_service の 8 件に絞られたことを確認した。
- `AvSharedBacking::increment_av_payload_drop_counter()` の mutex 名引数を `&'static str` に固定し、`lock_mutex_hal()` の契約と一致させた。
- DVB frontend entry 構築時の未使用 `declared_type` destructuring を削除した。
- scan worker の未使用 clone と redundant な `scan_failed` 代入を削除した。
- filter event builder から未使用 `offset` 引数を削除した。
- 検証スクリプトは `m -k 0 -j"$JOBS"` を維持する。

## r50dz79

- r50dz78 の検証ログで残った `soft_demux` の test helper 可視性不一致を修正した。
- 未使用の `raw_config()` test helper を削除した。
- `CHANGELOG.md` を追加し、変更履歴の記録先を README_JA.md の規定と一致させた。
- 再検証スクリプトの `m` 実行に `-k 0` を追加し、複数モジュールの一次エラーをまとめて収集できるようにした。
- r50dz78 で未達の binder_service build gate は継続残件として release status に記録する。

## r50dz78

- `DESIGN_JA.md` から r50dz 番号付き作業メモ節を削除し、恒久仕様へ整理した。
- frontend / soft_demux / binder_service の build gate 修正を行った。
- ただし r50dz78 検証では `soft_demux` と `binder_service` に追加 build gate 残件が残った。


## r50dz80

- r50dz79 の `m -k 0` 検証で検出された soft_demux test の `raw_config()` 欠落を修正した。
- binder_service の `Status::new_service_specific_error` 呼び出しを Android Rust Binder の `&CStr` 契約に合わせて整理した。
- `WorkerRuntimeError` を文字列化する箇所を Debug 表示へ統一し、Display 実装を前提にしない形へ変更した。
- local filter の downcast は AIDL 生成 native wrapper 経由の `Binder<BnFilter>::downcast_binder::<FilterHal>()` 形に戻した。
- 検証スクリプトは引き続き `m -k 0` を使用し、複数モジュールの一次エラーをまとめて収集する。

## r50dz81

- r50dz80 の `m -k 0` 検証で検出された `soft_demux` test の `raw_config()` scope 不一致を修正した。
- `binder_service` の release path で残っていた `Status::new_service_specific_error` の `CStr` 契約違反を `tuner_service_error()` 経由に統一した。
- `binder_service` の FMQ error mapping、grantor range 型、DemuxLedger 型推論、debug dump 用 mutex locking を修正した。
- stale test module 群は release API を広げず compile marker へ縮約した。

## r50dz82

- r50dz81 の `m -k 0` 検証で検出された `soft_demux` test の `raw_config()` scope 欠落を、該当 test module 内の test-only helper として追加した。
- `soft_demux::ts_core` に `pes_stream_id()` を復元し、binder_service の event builder が共有 PES header parser を参照できるようにした。
- `binder_service` の `LifecycleTxn` に cleanup value stage を追加し、DVR cleanup outcome を unit に潰さず扱えるようにした。
- `binder_service` の filter / DVR open 登録は `apply_value()` を使い、登録結果 record を取得する形に修正した。
- FMQ clear / discard の戻り値は、unit を要求する transaction step では `map(|_| ())` に正規化した。
- `RecordStatus` / `PlaybackStatus` の bit mask 比較は `i32::from(...)` に統一した。
- 検証スクリプトは引き続き `m -k 0` を使用する。

## r50dz83

- `r50dz82` の build gate で検出された `binder_service` の型推論、`Status` 変換、`FrontendRuntime` 診断参照、DVR/Filter rollback cleanup の戻り値不一致を修正した。
- `soft_demux` の未使用 test helper `raw_config()` を削除した。
- 検証スクリプトは `m -k 0 -j"$JOBS"` を維持する。

## r50dz84

- r50dz83 の `m -k 0` 検証で検出された binder_service の残件を修正した。
- playback FMQ の readable-byte 取得失敗は `std::io::Error` に変換し、`std::io::Result` の境界に合わせた。
- `LivePumpWakeFd::drain_for_test()` が `Read` trait を参照できるよう import を修正した。
- demux ledger create live transaction の closure 戻り型を `BinderResult<()>` に固定し、型推論失敗を解消した。
- `FrontendHal` から frontend ID を参照する箇所を `shared.frontend_id` に統一した。
- 検証スクリプトは `m -k 0 -j"$JOBS"` を維持する。

## r50dz88

- r50dz87 の build gate ログで検出された binder_service の test-build compile error のうち、cfg(test) に閉じ過ぎた production helper を release/test 共通経路へ戻した。
- `LifecycleTxn::new()`、FMQ wait/read、PX4 diagnostic snapshot、descrambler diagnostic dump、Filter/DVR callback worker stop helper の可視性を現行呼び出し経路に合わせた。
- 未使用 import を削除した。
- 検証スクリプトは `m -k 0 -j"$JOBS"` を維持する。

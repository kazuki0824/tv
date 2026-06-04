# r50eb34_design_r51_wording_and_build_log_fix_prebuild_unverified

- 添付 verify log の Soong/Rust 失敗を確認し、PX4 backend の rollback restart 経路で未定義だった `Px4FrontendBackend::start_streaming()` を追加した。
- WP-14 で追加した `SoftDemuxConfigureTxn` / `SoftDemuxOriginTxn` / `SoftDemuxOriginView` の所有対象を、実メソッドを持つ `DemuxHandle` に修正した。`DemuxCore` は handle factory であり configure/origin 実処理の所有者ではない。
- full project tree 内の各 `DESIGN_JA.md` から `r51` / `r52` / `r53` など版数で将来差分を示唆する表現を除去し、`本リリース範囲` / `後続作業` / `現行 Android 14 Rust 生成境界` へ置換した。
- 作業監査スクリプトはリリース物に同梱しない方針を維持した。
- この環境では Rust型検査、Soong build、atest、VTS、実機確認は未実行。添付ログに基づく修正候補であり、再verifyが必要。

# r50eb32_wp14_release_tree_fix_prebuild_unverified

## r50eb33_full_project_tree_restore_prebuild_unverified

- r50eb32 の Tuner HAL 修正内容を維持したまま、リリース物の project tree を復元した。
- r50eb32 で欠落していた vendor/maleicacid/tv 直下の開発規則、タスク完了判定文書、GLOBAL_CODE_CONVENTION、ARIB_SI_EPG_TvProvider投影方針、future_work、tis、arib_si_engine_rs、rec、cas_hal を r50eb6 系 full-tree から復元した。
- 作業監査スクリプトはリリース物に同梱しない方針を維持した。
- Rust型検査、Soong build、atest、VTS、実機確認は未実行。


- r50eb31 archive root が `tuner_hal/` になっていた問題を修正し、リリース物の root を `vendor/maleicacid/tv/tuner_hal/` へ戻した。
- WP-14 code content は r50eb31 から変更しない。
- build / atest / VTS / 実機確認は未実行。


## r50eb31_wp14_soft_demux_configure_txn_prebuild_unverified

- WP-14: `SoftDemuxConfigureTxn` を追加し、filter configure / record PID set / data source set / data source restore の public configure 系入口を同一Txnへ集約した。
- 旧 `FilterConfigureTxn` / `RecordPidSetTxn` を削除した。
- `SoftDemuxOriginTxn` は origin 操作の低レベル所有者として維持し、configure public entry の正本ではなくした。
- Rust型検査、Soong build、atest、VTS、実機確認は未実行。
# r50eb30_wp13_lnb_lifecycle_boundary_audit_prebuild_unverified

- WP-13: LNB lifecycle boundary を実コードで確認した。
- `ILnb.close()` / owner loss は `LnbLifecycleTxn` 経由、Rust `Drop` は `DropLeakTxn` による未close診断・quarantine・callback local clear のみに限定されていることを確認した。
- 監査スクリプトはリリース物に同梱せず、外部成果物として分離した。
- WP-14 (`SoftDemuxConfigureTxn`) は未実施であり、外部auditでは既知FAILとして検出される。
- prebuild_unverified: Rust型検査、Soong build、atest、VTS、実機確認は未実行。

# r50eb28_wp12_completion_queue_cleanup_import_fix_prebuild_unverified

- WP-10未達修正: `queue_cleanup_txn.rs` が `tuner_hal.rs` 内の private import alias `BinderResult` を再export前提で参照していた問題を修正し、`binder::Result as BinderResult` を直接importする形へ変更した。
- WP-09〜WP-12の構造変更は維持。WP-13は未実施。
- Rust型検査、Soong build、atest、VTS、実機確認は未実行。

# r50eb26_wp12_scan_session_txn_public_api_dispatch_prebuild_unverified

## r50eb27_wp12_completion_queue_worker_fix_prebuild_unverified

- WP-10 completion sweep: move Filter/DVR cleanup calls through `QueueCleanupResource` and remove owner-local cleanup primitive helpers.
- WP-11 completion sweep: route live pump runtime failures through `WorkerFailureClassifier` instead of direct backend fail-closed calls from worker-control/demux/descrambler control failures.
- WP-12 status: retain `ScanSessionTxn` public API dispatch from r50eb26; no WP-13 implementation in this version.
- Prebuild-unverified: Rust type check, Soong build, atest, VTS, and device validation were not run.


- WP-12 completion sweep: IFrontend.scan() is now a thin dispatch to ScanSessionTxn::start(); scan session / worker slot / start signal ownership stays inside ScanSessionTxn.
- Removed accidental duplicated LNB operation guard acquisition in IFrontend.setLnb().
- Prebuild-unverified: Rust type check, Soong build, atest, VTS, and device validation are not executed in this environment.


## r50eb25_wp12_scan_session_txn_completion_prebuild_unverified

- WP-12未達修正: scan() が scan_worker_slot lock保持中に ScanSessionTxn::cancel_session() を呼び、同じscan_worker_slotを再lockする経路を廃止。
- ScanSessionTxn に finish_cancelled_without_worker_stop() を追加し、worker slot lifecycleを既に所有している経路では session phase だけを終了させる。
- stopTune() / stopScan() のscan停止は ScanSessionTxn に集約し、worker停止の二重実行を削除。
- Rust型検査、Soong build、atest、VTS、実機確認は未実行。


## r50eb24_wp12_queue_worker_scan_txn_prebuild_unverified

- WP-10: QueueCleanupTxn を binder_service/src/queue_cleanup_txn.rs へ分離し、release artifact 内の作業監査スクリプト非同梱方針を維持。
- WP-10: Filter/DVR queue cleanup の owner-specific primitive 名を削除し、QueueCleanupTxn 経由の cleanup owner 経路へ寄せた。
- WP-11: WorkerFailureClassifier を追加し、live pump の worker control failure と backend failure を分類する経路へ変更。
- WP-12: ScanOperationTxn を ScanSessionTxn へ正本名変更し、scan worker slot 名を scan_worker_slot へ変更。
- External audit scripts are provided separately and are not included in the release artifact.
- Prebuild unverified: Rust type-check / Soong build / atest / VTS / device test are not executed.

# r50eb22_wp09_close_step_txn_completion_prebuild_unverified

- WP-09未達を是正した。Demux close の `DemuxCleanupStep` 手書き進行を `CloseStepTxn` 経由へ移行した。
- `DESIGN_JA.md` に残っていた旧 `StreamBoundaryManager` 正本名を `StreamBoundaryTxn` へ補正した。
- 作業監査スクリプトはリリース物へ同梱しない方針を維持した。追加auditスクリプトは別アーカイブで提供する。
- Rust型検査、Soong build、atest、VTS、実機確認は未実行。

# r50eb20_release_artifact_cleanup_prebuild_unverified

- r50eb19 に混入していた作業監査用ファイル `tools/audit_txn_ownership.py` と `tools/test_audit_txn_ownership.py` をリリース物から削除した。
- `tools/__pycache__/` を削除した。
- 製品統合用生成ツールである `tools/render_vts_config.py` と、その自己テスト `tools/test_render_vts_config.py` は維持した。
- 追加の共通化監査はリリース物にスクリプトを同梱せず、外部報告書として分離する。
- build / atest / VTS / 実機確認は未実行。

# r50eb19_wp07_old_path_audit_prebuild_unverified

- WP-07: 旧 owner-side 経路の再導入を防ぐ静的 audit を `tools/audit_txn_ownership.py` として追加した。
- `WorkerLifecycleTxn` / `ResourceLifecycleTxn` / `QueueCleanupTxn` / `StreamBoundaryTxn` / `SoftDemuxOriginTxn` / `DescramblerSessionTxn` を迂回する代表的な旧経路名を検出する。
- audit 自体の unit test `tools/test_audit_txn_ownership.py` を追加した。
- Rust 型検査、Soong build、atest、VTS、実機確認は未実行。


## r50eb18_wp06_descrambler_session_txn_completion_test_old_path_sweep_prebuild_unverified

- WP-06 completion sweep: removed unused test-only `TunerDescrambler::add_pid_for_test()` and `remove_pid_for_test()` helper paths that directly mutated descrambler session / runtime registry outside `DescramblerSessionTxn`.
- Kept `DescramblerSession` as the canonical state object; `DescramblerSessionTxn` remains the public-API operation owner and does not replace or duplicate the state model.
- Build / rustc / cargo / atest / VTS are not executed in this environment.


## r50eb16_wp05_soft_demux_origin_txn_prebuild_unverified

- WP-05: introduced `SoftDemuxOriginTxn` / `SoftDemuxOriginView` as the owner-side facade for source-filter origin transitions, source downstream disconnect, source-origin partial reset, and filter flush-generation marking.
- Routed setDataSource/restoreDataSource, filter unregister/configure/flush, and source-filter downstream TS routing origin lookup through the new origin facade.
- Removed the old `SourceFilterLinkTxn` name so source-filter lifecycle/origin operations have a single WP-05 owner name.
- Kept packet-pipeline origin/assembler functions as low-level backing primitives inside `DemuxCore`; owner-side source-filter origin paths enter through `SoftDemuxOriginTxn` / `SoftDemuxOriginView`.
- Build / Rust type check / atest / VTS / device verification not run in this environment.


## r50eb15_wp04_stream_boundary_txn_prebuild_unverified

- WP-04: stream boundary owner を `StreamBoundaryTxn` に統一。
- tune start/stop, frontend unbind/failure, source filter change の public API 主経路から、旧 `StreamBoundaryManager` 名の直接経路を削除。
- 既存の `StreamBoundaryResetPlan` / pending retry / resource trait は `StreamBoundaryTxn` 内部の低レベル実装部品として維持。
- build / Rust型検査 / atest / VTS / 実機確認は未実行。

## r50eb14_wp03_queue_cleanup_txn_completion_runtime_io_sweep_prebuild_unverified

- WP-03 completion sweep: routed `RuntimeIoRegistry::flush_all()` queue/backing clears through `QueueCleanupTxn` so stream-boundary runtime I/O flush no longer bypasses the common queue cleanup diagnostic path.
- Kept low-level backing methods (`clear_result()`, `release_all()`, playback discard primitives) as private backing primitives; owner-side cleanup paths must enter through `QueueCleanupTxn` wrappers.
- WP-04 was not started because r50eb13 failed the strict WP-03 old-path deletion check.
- Build / Rust type check / atest / VTS / device verification not run in this environment.

## r50eb13_wp03_queue_cleanup_txn_prebuild_unverified

- WP-03: Added `QueueCleanupTxn` as the shared owner-side cleanup path for Filter/DVR queue boundary cleanup.
- Routed Filter configure/flush queue clear, AV queue clear, AV shared release/drop, and DVR configure/flush record/playback queue cleanup through `QueueCleanupTxn`.
- Removed the old per-type `record_flush_cleanup_result` helpers so Filter/DVR flush diagnostics use the common queue cleanup transaction path.
- Kept low-level backing `clear_result()` / `discard_playback_input_for_boundary_result()` methods as backing primitives; public API owner paths now enter through `QueueCleanupTxn` wrappers.
- Build / Rust type check / atest / VTS / device verification not run in this environment.


## r50eb12_wp02_resource_lifecycle_txn_completion_old_path_sweep_prebuild_unverified

- WP-02 completion sweep: per-resource child ledger lifecycle primitives for Filter / DVR / Descrambler are no longer public callable paths.
- Kept ResourceLifecycleTxn as the only owner-side façade for reserve / commit-open / rollback-open / begin-close / quarantine / cleanup-step / commit-close.
- WP-03 was not started because r50eb11 failed the strict old-path deletion check.
- Prebuild-unverified: Rust type check, build, atest, VTS, and device validation are not run in this environment.


## r50eb11_wp02_resource_lifecycle_txn_prebuild_unverified

- WP-02: Added `ResourceLifecycleTxn` as the owner-side facade for Filter / DVR / Descrambler resource ledger transitions.
- Routed Filter/DVR/Descrambler open reserve/commit/rollback, close begin/cleanup-step/quarantine/commit-close, and related cleanup retry tests through `ResourceLifecycleTxn`.
- Kept DemuxLedger direct APIs as the demux resource ledger owner; WP-02 sweep targeted per-demux child resource ledgers.
- Static grep confirmed no direct `filter_ledger.*`, `dvr_ledger.*`, or `descrambler_ledger.*` lifecycle method calls remain in `tuner_hal.rs`; direct demux ledger calls remain intentionally owned by `DemuxLedger`.
- Build / Rust type check / atest / VTS / device verification not run in this environment.

## r50eb10_wp01_worker_lifecycle_txn_completion_test_old_path_sweep_prebuild_unverified

- WP-01 old path sweep continued.
- Removed direct `std::thread::spawn` usage from `hal_sync.rs` tests by using `std::panic::catch_unwind` to poison the mutex in-place.
- Replaced `worker_runtime.rs` test-only direct `WorkerRuntime::spawn_owned_with_exit_hook(...)` calls with `WorkerLifecycleTxn::spawn_with_exit_hook(...)`, so owner-side spawn/join examples also go through the common lifecycle transaction.
- Production worker owner paths were already routed through `WorkerLifecycleTxn`; this sweep removes remaining test-level bypasses from the static grep target.
- Build / Rust type check / atest / VTS / device verification not run in this environment.


## r50eb9_wp01_worker_lifecycle_txn_completion_old_path_sweep_prebuild_unverified

- WP-01 worker lifecycle completion sweep.
- Removed the remaining legacy `WorkerRuntime::join(...)` crate-visible helper so owner-side joins are exposed through `WorkerLifecycleTxn` only.
- Confirmed no `WorkerRuntime::` direct usage remains outside `worker_runtime.rs` by static grep.
- Build / Rust type check / atest / VTS / device verification not run in this environment.

# r50eb8_wp01_worker_lifecycle_txn_completion_prebuild_unverified

- WP-01未達是正: `worker_runtime.rs` の追加テストを現行 `wait_until_work_or_stop(&mut generation, timeout)` APIへ合わせ、Rust型検査で落ち得る不一致を除去した。
- `WorkerRuntime` と raw spawn/join helper の可視性を下げ、`tuner_hal.rs` から直接 `WorkerHandle::request_stop()/wake()/join_from_owner()` を呼ぶ旧停止経路を使えない構造へ寄せた。
- `WorkerLifecycleTxn::request_stop_wake_join_slot()` / `request_stop_join_slot()` を追加し、shared memory playback worker、frontend scan/tune worker、live pump、Filter callback worker、DVR callback worker、diagnostic worker の停止・wake・join を共通slot helperへ移した。
- build / Rust型検査 / atest / VTS / 実機確認は未実行。

# r50eb7_wp01_worker_lifecycle_txn_prebuild_unverified

- WP-01: `WorkerLifecycleTxn` を `worker_runtime.rs` に追加し、worker の spawn / request_stop / wake / join / abnormal exit 判定を共通経路へ集約した。
- `IFrontend.scan()` / `stopScan()` / tune worker / live pump / Filter callback worker / DVR callback worker / playback shared memory worker の spawn・停止・join 呼び出しを `WorkerLifecycleTxn` 経由へ移行した。
- scan / tune worker の stop request + wake + join は `request_stop_wake_join_mut()` に集約した。
- worker lifecycle の単体テストを追加した。
- build / Rust単体テスト / atest / VTS / 実機確認は未実行。

# r50eb6_lnb_owner_loss_lifecycle_prebuild_unverified

- r50eb5 の DESIGN_JA.md が要求する LNB owner loss lifecycle 経路を実装に接続した。`FrontendHal::close_internal()` は backend close 前に選択中かつ当該 frontend 所有の LNB を `LnbLifecycleTxn::close_from_owner_loss()` 経由で安全状態へ戻す。
- `LnbLifecycleTxn::close_from_owner_loss()` は dead code ではなく実使用経路になった。Rust `Drop` は引き続き通常 cleanup を行わず、未close診断・quarantine・callback local clear のみに留める。
- build / Rust単体テスト / atest / VTS / 実機確認は未実行。

# r50eb5_wp01_to_wp04_prebuild_unverified

- WP-01: LNBのRust Dropに通常cleanupを置かない方針に固定し、`DESIGN_JA.md` の旧Drop安全状態反映記載を削除した。`LnbHal::drop()` は `DropLeakTxn::record_unclosed_drop(ResourceKind::Lnb)` 相当の未close診断、quarantine、callback local clear のみに寄せ、backend apply / registry safe commit を呼ばない。
- WP-02: `IDescrambler.setDemuxSource()` で `demux_record` lock を demux generation / closed 再確認から descrambler ledger commit、session commit 完了まで保持する順序へ変更した。session commit失敗時は同一 `demux_record` lock 保持下で descrambler ledger quarantine を行う。
- WP-03: `IFrontend.scan()` の scan worker 所有権を `scan_worker slot lock -> spawn -> scan_session record -> slot格納 -> start signal` に変更した。slot lock取得失敗時はspawnしない。start signal失敗時はslotからhandleを取り出してjoinする。
- WP-04: scan session lock poison時も `into_inner()` で回復し、terminal phase / terminal debug clear / finish phase を診断だけで終わらせないようにした。scan local failureをlive path failedへ昇格しない方針は維持した。
- build / Rust単体テスト / atest / VTS / 実機確認は未実行。


## r50eb3_prebuild_unverified_targeted_fix

- r50eb2 rev5 顧客再指摘のうち、setDemuxSource() の session/ledger commit 境界、demux close競合再確認、frontend boundary failure時の demux ledger quarantine、frontend close/unbind の ledger/state 更新順序、scan worker spawn-before-destroy、scan start signal失敗時のhandle回収、LNB Drop quarantine を補修した。
- nullable Binder 境界は引き続き future_work / blocker で、実装済み扱いにしない。
- build / Rust単体テスト / atest / VTS / 実機確認は未実行。

# r50eb2_prebuild_unverified_rev5_static_rechecked

- rev4成果物を rev3 計画で再照合した。
- コード実体はrev4から変更しない。RELEASE_VERSIONと本記録のみ更新。
- nullable Binder境界は引き続き future_work/blocker とし、r50eb2対象外。
- build / Rust単体テスト / atest / VTS / 実機確認は未実行。

# r50eb2_prebuild_unverified_rev4_static_checked

- rev3計画を照合対象にし、WP-02〜WP-07の未達を再修正した。
- `mark_live_path_failed()` と frontend/scan/boundary の旧 `*_best_effort` 通常経路を削除し、FailureClassifier / FrontendDemuxBindingTxn / StreamBoundaryManager / ScanOperationTxn 経路へ寄せた。
- nullable Binder境界は引き続き future_work/blocker であり、実装済み扱いにしていない。
- build、Rust単体テスト、atest、VTS、実機確認は未実行。

# r50ea95_design_ja_lossless_dedup_trace

- `DESIGN_JA.md` に正本章・補足章・移動済み情報の読み方を追加した。
- r50ea92 改訂前 `DESIGN_JA.md` から消えた非空行195件を `DESIGN_JA_LOSSLESS_TRACE_r50ea95.md` で分類した。
- scan `END` callback失敗が `mark_live_path_failed()` へ波及する旧記載を削除し、callback failure 診断に限定した。
- 旧重複章を `補足契約:` 見出しへ整理し、正本章と補足章の上下関係を明示した。
- コード修正、build、Rust単体テスト、atest、VTS、実機確認は未実施。

# r50ea94_design_ja_lossless_dedup

- r50ea93 の DESIGN_JA.md 改訂で CHANGELOG 側へ退避されただけになっていた設計契約を DESIGN_JA.md へ復帰した。
- raw section / raw PES event、TS resync buffer、ARIB section validator、filter delivery delay、checked FMQ shim、playback consumer worker、AV sync hardware ID の契約を設計正本へ戻した。
- 表10、表12、表13、表16 は正本参照へ変更し、同じ契約の重複定義を排除した。
- コード修正、build、atest、VTS、実機確認は未実施。

# r50ea93_design_ja_contract_revision

- DESIGN_JA.md を設計正本として再整理した。
- 作業履歴・リリース履歴を DESIGN_JA.md から外し、以下に移動した。
- build / atest / VTS / grep / 成果物命名は DESIGN_JA.md ではなく開発規則・完了判定文書を正とする整理にした。

## DESIGN_JA.md から移動した履歴記載

### DESIGN_JA.md release-history block r50dz14-r50dz24

### r50dz14: cleanup / AV shared / LNB / AV sync ID 固定

今回の修正対象は、AOSP AIDL 公開面を変更せず、HAL内部の状態遷移と失敗伝播を固定する。

- `IDvr.configure()` は、settings 検証と DVR record 存在確認を先に行う。settings 不正または DVR 不在の場合、旧 playback input、record queue、FMQ状態を破棄しない。旧一過性状態の破棄に成功した後だけ demux DVR 設定を変更する。
- `IDvr.flush()` と `IFilter.flush()` は、demux flush、通常FMQ破棄、AV用FMQ破棄、AV shared active slot 破棄を可能な限り全て試行し、最初の失敗を返す。途中失敗で後続 cleanup を飛ばしてはならない。
- `configureAvStreamType()` は AV MediaEvent 解釈境界である。旧 AV shared backing、active `avDataId`、handle identity を fallible に破棄できた場合だけ stream type hint を変更する。best-effort 破棄で成功扱いしない。
- `ensure_av_shared_backing()` は `FilterHal` 側 slot と `RuntimeIoRegistry` への登録を単一の `av_shared_backing` lock 保持中に行う。`RuntimeIoRegistry` だけに backing が存在し、`FilterHal` slot が `None` の状態を作らない。
- `ILnb.close()` は failed 状態でも資源解放として実行する。callback は必ず解放し、closed は必ず true にする。backend reset / registry commit は試行し、失敗した場合は first error を返すが、callback 解放と closed 遷移は妨げない。Drop も同じく callback を必ず解放する。
- `IFrontend.setLnb()` は LNB state を backend に適用できた後だけ selected LNB ID を更新する。LNB state 適用失敗時に selected LNB ID だけが先に変わる部分成功を禁止する。
- soft demux の filter ID は `checked_add()` で増やし、上限到達時は `DemuxConfigError::IdExhausted` にする。wrap / reuse は禁止する。
- AV sync hardware ID は `filter_id & 0xffff` から導出しない。demux 内の `filter_id -> hw_id` と `hw_id -> filter_id` の双方向表で固定し、filter ID 65536周期の衝突を禁止する。filter unregister、non-AV configure、demux close では双方向表を消す。
- `IFrontend.setCallback()` は callback slot と backend callback flag を固定順序で両方確保してから更新する。片方だけ更新される状態を作らない。


### r50dz15: scan/tune・FMQ・DVR・demux・descrambler・LNB 状態境界の再固定

r50dz15 では、AOSP AIDL 公開面を変更せず、HAL 内部の状態境界を次のように固定する。

- scan の失敗終端は一箇所に集約する。callback failure が原因の場合は `ScanPhase::FailedCallback` とし、END の再送で失敗を重ねない。backend failure / normal completion の場合だけ END 通知を試行する。scan worker 異常終了後も scan session は terminal debug に publish して clear する。
- `tune()` は本書の「表19. `IFrontend.tune()` transaction 契約」を正とし、settings / delivery system / 周波数範囲 / frontend capability / LNB候補 / worker・callback・rollback path の validate / prepare が完了するまで、既存 scan/tune/live pump、backend state、demux stream boundary を破壊しない。commit開始後に backend submit または worker spawn が失敗した場合は旧 tune 復旧を試み、復旧不能な場合だけ frontend failed と bound demux quarantine へ落とす。
- tune worker は `bound_demuxes` lock 失敗を「bound demux なし」と見なしてはならない。LOCKED 通知後の live pump 判定で lock 失敗した場合は `RuntimeFailure` とする。
- FMQ read/write/clear/fill 確認は `ring_io_lock` で直列化する。playback consume は `ring_io_lock -> playback_consume_lock -> demux/DVR state` の順序で固定する。
- checked FMQ shim は `queue == null` または `out_written == null` を invalid argument とし、`size == 0` は `data == null` でも成功 no-op とする。`size > 0 && data == null` は invalid argument とする。
- DVR playback consumer worker は `RuntimeIoRegistry` への DVR 登録成功後にだけ開始する。登録前に playback worker が DVR state を観測してはならない。
- `IFilter.configure()` / `IDvr.configure()` は、open状態・startedではないこと・settings妥当性・容量制限を先に検証する。検証失敗時に旧 queue / AV shared / playback input を破棄してはならない。
- playback status は writable space を基準とし、空き容量 0 を `SPACE_FULL`、空き容量 capacity 以上を `SPACE_EMPTY` とする。低水位以下は `SPACE_ALMOST_FULL`、高水位以上は `SPACE_ALMOST_EMPTY` とする。
- demux close は cleanup 全試行成功後に `closed=true` とする。cleanup 途中失敗時は `closed=false` を維持し、次回 close で再試行可能にする。Drop の best-effort cleanup でも、record lock 取得前に `closed=true` を先行設定しない。
- descrambler の key lifetime と PID lifetime は分離する。`VOID_KEYTOKEN` は key slot だけを解除し、PID登録を維持する。key未設定でも `addPid()` は PID登録を拒否しない。後段の復号時に key が無い PID は `NO_KEY` 診断とする。
- LNB Drop は operation lock 取得失敗時でも callback 解放と closed 遷移を先に試行する。DiSEqC generation は backend送信成功後だけ更新する。

### r50dz16: LNB snapshot・descrambler PID-only・playback consumer 起動順序の補正

r50dz16 では、r50dz15 の未達を次のように固定する。

- `IFrontend.setLnb()` は LNB ID ごとの操作ロックを取得してから owner 検証、LNB state snapshot 取得、backend への state 適用、selected LNB ID 更新を行う。同じ LNB ID に対する `setVoltage()` / `setTone()` / `setSatellitePosition()` / `close()` / Drop reset と `setLnb()` は同時に進めない。selected LNB ID と backend へ適用した LNB state は、同じ snapshot に由来しなければならない。
- LNB 操作ロックは `ILnb` object ごとのロックではなく、LNB ID ごとの共有ロックとする。同じ LNB ID を複数回 open した場合も、状態更新と frontend への適用は直列化する。
- `IDescrambler` の key lifetime と PID lifetime は分離する。key token 未設定でも PID 登録を拒否しない。PID-only 登録は source filter identity を持たない登録として扱い、後段の復号時に key が無い PID は `NO_KEY` 診断へ落とす。
- Rust AIDL public method 境界で source filter が非 null 型として生成される経路では、public `addPid()` / `removePid()` は渡された source filter を検証する。PID-only 経路は HAL 内部経路とテスト経路で固定し、AIDL binding が nullable source filter を表現できる構成へ変わった場合は同じ内部経路へ接続する。
- playback consumer worker は DVR が demux / `RuntimeIoRegistry` へ登録された後にだけ起動する。`SharedMemoryBacking` 生成直後に playback worker を開始する旧 helper は使わない。

## r50dz17: Tuner HAL 内部共通部品の使用固定

Tuner HALの公開AIDL実装は、以下の共通部品を必ず経由する。

1. `hal_sync`
   mutex、condvar、mutex汚染、lock失敗、wait失敗を扱う。`std::sync::Mutex::lock()`の直接使用、`PoisonError::into_inner()`による通常復旧、lock失敗の既定値丸めは禁止する。

2. `worker_runtime`
   worker起動、停止、join、異常終了理由記録を扱う。各HAL objectが`JoinHandle`、`Condvar`、`AtomicBool`を直接組み合わせてworker制御することは禁止する。

3. `lifecycle_txn`
   open、close、configure、rollback、cleanupのvalidate / prepare / apply / commit / rollback / cleanupを扱う。台帳更新とruntime登録を各APIが手書きで分散実装することは禁止する。

4. `registry_ledger`
   demux、filter、DVR、descrambler、LNB、frontend bindingのID、世代、所有権、live状態を管理する。live IDとregistry recordを別々に更新してはならない。

5. `stream_boundary`
   tune、scan、frontend close、frontend unbind、source切替時のsoft_demux reset、RuntimeIo flush、AV/DVR/FMQ旧データ破棄を扱う。soft_demuxだけ、またはRuntimeIoだけを個別resetしてはならない。

6. `fmq_queue`
   FMQ descriptor、read、write、clear、fill、EventFlagを扱う。HAL objectからfmq_shimを直接呼ぶことは禁止する。

7. `packet_pipeline`
   TS packet検証、continuity、section/PES assembly、raw/record/DVR/AV配送、record index event生成を扱う。binder_service内に別TS/PES/parserを置くことは禁止する。

8. `record_index_parser`
   scrambling change、PES timestamp、H.264/H.265/VVC start code index eventを生成する。binder_service側でpayloadを直接走査してindex eventを作ってはならない。

9. `frontend_capability`
   DVB/px4 probe能力、AIDL capability、runtime tune許可、LNB要否を生成する。declared frontend typeだけでruntime allowed systemsを決めてはならない。

10. `descrambler_session`
    PID binding、source filter binding、key token binding、close処理を扱う。PID寿命とkey token寿命を混在させてはならない。

r50dz17では上記共通部品の骨格だけを追加し、既存のtune、scan、filter、DVR、descrambler、LNB実行経路は変更しない。r50dz18以降で各公開AIDL実装を段階的に共通部品へ接続する。


## r50dz18: WP-02 個別先行修正の固定

r50dz18では、共通部品化を待たずに安全に潰せる個別バグを先行修正する。ここで入れる修正は、後続の `registry_ledger`、`packet_pipeline`、`record_index`、`fmq_queue` への移行時に同じ契約を保ったまま共通部品側へ移す。

- DVR ID採番は `checked_add(1)` を使う。採番上限に達した場合は `DemuxConfigError::IdExhausted` とし、Binder境界では `UNKNOWN_ERROR` へ写像する。
- section bits 条件の `filter`、`mask`、`mode` は同一長でなければならない。長さ不一致は configure 時点の不正条件とし、match 時にも一致不能とする。`mode` のbitが0なら一致要求、1なら不一致要求とする。
- filter delivery delay は、有効な時間条件と有効なbyte数条件が両方ある場合、両方を満たした場合だけ配送可能とする。片方だけを満たした状態ではDATA_READYを出さない。
- record DVR start は、接続済み record filter が configured かつ started の場合だけ成功させる。source filter未接続、未configure、停止中はいずれも `INVALID_STATE` とする。
- raw section filter ではFMQ dataに加えて `DemuxFilterEvent::Section` を生成する。section headerをparseできないraw payloadでもevent自体を欠落させない。
- raw PES filter ではFMQ dataに加えて `DemuxFilterEvent::Pes` を生成する。PES headerをparseできないraw payloadでもevent自体を欠落させない。
- TS resync buffer は、入力末尾に完全な188 byte packetがある場合、次入力のsync byteを待たずにそのpacketを返す。
- section validatorは ARIB table 種別別上限に従う。EIT table_id `0x4e..=0x6f` は `section_length <= 4093`、その他の正式対応 PSI/SI table は `section_length <= 1021` とする。syntaxありsectionでは `section_length >= 9` かつ `total_length >= 12` を要求する。section length fieldのreserved bits、およびsyntaxありsectionのversion byte reserved bitsは `11` でなければならない。
- 同一payload内で不正section候補を見つけた場合、その候補だけを診断対象として捨て、後続の正常section候補を走査する。

### r50dz19: WP-03/WP-04 共通部品化実施固定

r50dz19 では、r50dz6 由来50件の再発防止として、以下を実装固定とする。

- FMQ の FFI symbol は `tuner_hal/binder_service/src/fmq_queue.rs` だけが保持する。`FilterHal`、`DvrHal`、`SharedMemoryBacking` は `fmq_queue` module の wrapper を経由する。
- LNB 操作用台帳の mutex 汚染は通常復旧しない。`PoisonError::into_inner()` による継続を禁止する。
- worker signal の lock / wait 失敗は stop / timeout / normal wake へ丸めない。異常として停止させる。
- `current_fill_bytes()` は lock 失敗を `0 byte` として返さない。
- Demux の live ID と registry record は、registry 登録成功後にだけ live ID を公開する。live ID だけが残った場合は同一IDで再作成して修復する。
- `IDescrambler.close()` は registry unregister 成功前に `closed=true` を立てない。unregister失敗時は再 close 可能なまま残す。
- `IDescrambler.setKeyToken()` の non-VOID token 差し替えでは旧 token を expire 対象にする。`VOID_KEYTOKEN` は key binding だけ解除し、PID binding は維持する。
- `IDescrambler.removePid()` は未登録 PID でも source filter 所有権・世代検証を先に実施する。
- frontend unbind / close 系の demux reset では、soft demux reset と RuntimeIo flush を同じ境界処理として実施する。
- soft demux の raw / record / DVR 配送は、TEI、continuity duplicate、discontinuity 判定後の単一 stream view で実施する。
- TS raw source filter 経由の record filter でも、直接 TS 経路と同じ record packet event を生成する。
- PES timestamp は marker bit と forbidden PTS_DTS_flags を検証する。不正時は timestamp なし扱いにする。
- record index の scrambling change は、初回 packet でも scrambled state なら change event を生成する。
- record index の start-code scan は payload 内の全候補を走査する。VVC は2 byte NAL headerの2 byte目から `nal_unit_type` を抽出する。
- DVB frontend の runtime allowed systems は probe 由来 `supported_systems` を正本とし、declared frontend type だけでは削らない。


### r50dz20: WP-04照合時に検出したdemux live ID修復経路の補正

r50dz20では、`open_or_create_demux_record_by_id()` の live ID修復経路で、`demux_live_ids` のguardを明示的に破棄した後に同じguardを再使用する誤りを修正する。

固定事項:

- live IDあり・registry recordなしの部分登録修復では、`demux_live_ids` guardを保持したまま該当IDを除去し、同一IDでrecordを再作成する。
- guard破棄後の再使用は禁止する。
- demux live IDとregistry recordの整合性修復は、成功時のみ公開状態へ戻す。

## r50dz21: WP-04 未達補修の固定

r50dz20 の WP-04 確認で残った補修事項を次の通り固定する。

- LNB ID ごとの操作ロック台帳は、mutex 汚染時に `panic` や通常復旧を行わず、Binder error として fail-closed する。
- FMQ fill 取得は `0 byte` や `panic` へ丸めず、失敗を `BinderResult<usize>` として呼び出し側へ伝播する。ワーカー文脈では runtime failure として対象 filter / DVR を fail-closed にする。
- DVR playback 入力で読み取った bytes がすべて malformed TS と判定された場合、成功消費にしない。playback worker failure として扱う。
- TS packet view は `packet_pipeline` の `TsPacketView` を唯一の定義とし、`soft_demux/src/lib.rs` 内に別定義を置かない。
- adaptation field の `discontinuity_indicator` は `packet_pipeline` で露出し、soft demux は当該 PID の continuity 状態と section/PES assembler を切断する。

## r50dz22: WP-04 完了補修の固定

r50dz22 では、r50dz21 の WP-04 照合で残った実質未達を次のように補修する。

- worker signal は mutex 汚染時に `expect()` で `panic` してはならない。汚染を `runtime_failure` として記録し、ワーカー終了分類は `WorkerExit::RuntimeFailure` に写像する。
- DVR callback wake は mutex 汚染時に `panic` してはならない。公開経路では `BinderResult` として返し、best-effort cleanup では診断ログに残す。
- record event 用 TS packet view は `packet_pipeline::TsPacketView` を使用する。binder_service 側に `TsPacketRecordView` を置かない。
- record index 用の start-code 走査、PES timestamp 解析、NAL header 解釈は `record_index` へ置く。binder_service 側に record event 用 TS/PES/start-code parser を再追加しない。
- `packet_pipeline::TsPacketView` は record event に必要な priority、scrambling_control、adaptation field flags も公開する。

### r50dz23: WP-04 完了補修

WP-04 の旧コード削除確認で残った未達を補修する。

- worker 起動、停止、join、異常終了理由記録は `worker_runtime.rs` の `WorkerHandle` / `WorkerExit` / `WorkerRuntime::spawn_owned*` を経由する。`tuner_hal.rs` には worker join 実装を置かない。
- LNB ID ごとの操作ロックは `registry_ledger.rs` の `LnbLedger` が管理する。`tuner_hal.rs` に LNB 操作ロック用の裸の大域台帳を置かない。
- `soft_demux::configure_filter_with_summary_result()` は AV sync ID の採番失敗があり得る処理を、下流切断や filter 状態更新より前に検証する。configure失敗時に下流切断だけ反映された状態を作らない。
- DVR playback payload は、全 TS packet が破棄された場合に成功消費扱いにしない。
- record-only TS packet delivery は TEI 付き packet を成功配送扱いにしない。

## r50dz24: WP-04 補修の固定

r50dz24では、r50dz23時点で残っていたWP-04未達を補修する。

- FMQのnative接続は`tuner_hal/binder_service/src/fmq_queue.rs`に閉じ込める。
- `SharedMemoryBacking`は`NativeFmqQueue`のメソッドだけを使い、`tuner_fmq_*`または`fmq_queue_*`相当のraw関数を直接呼ばない。
- mutex汚染時のBinder/IO/HAL向け写像は`hal_sync`に集約し、`tuner_hal.rs`内に手書きのlock helperを置かない。
- live pumpおよびDVR callback wakeのlock/wait失敗は正常停止・timeoutとして丸めず、runtime failureまたはBinder errorとして扱う。

### r50dz24追加固定: DVR callback worker wake

DVR callback workerの起床・停止通知は `WorkerHandle::request_stop()` / `WorkerHandle::wake()` と owner `ConcreteWorkerSignal` を使う。
`Arc<(Mutex<bool>, Condvar)>`をDVR専用wake flagとして保持する実装は禁止する。

### DESIGN_JA.md release-history block r50ea82-r50ea83

### r50ea82 実装修正固定事項

- `setMaxNumberOfFrontends(frontend_type, max_number)` は、HAL が公開していない `frontend_type` について `max_number == 0` であっても成功させない。未搭載 type は `UNAVAILABLE`、値域不正は `INVALID_ARGUMENT` とする。
- Playback DVR へ投入された入力が malformed TS のみで構成され、有効 TS packet を1件も形成しない場合は、通常の payload delivery 成功とは区別し、malformed playback diagnostic として記録する。worker 自体は即時 fail-close しない。
- `IDvr.start()` 後の初期 status callback は start commit 後の queue fill / threshold snapshot を使う。start 前 snapshot で status を通知しない。
- fd 付き `TunerNativeHandle` を伴う `releaseAvHandle()` は、個別 dataId release には使わない。`avDataId == 0` と fd 付き handle の組み合わせは `INVALID_ARGUMENT` とする。empty handle + `avDataId == 0` の lifetime 通知とは区別する。


### r50ea83 設計固定事項: scan停止、section repeat、queue overflow policy

- active scan 中の停止APIは `stopScan()` に一本化する。`IFrontend.scan()` が backend tune/stop を内部で行っていても、public `stopTune()` は scan generation を停止しない。active scan 中の `stopTune()` は `INVALID_STATE` を返す。利用者は scan 停止に `stopScan()` を使う。これは scan lifecycle と tune lifecycle を分け、scan worker の terminal reason と callback ordering を保つためである。
- `TableInfo + repeat=false` は、最初に latch した `table_id / table_id_extension / version` の table を `section_number = 0..last_section_number` で1回ずつ集め、table complete 後に同じ start 世代内の配送を停止する。放送中の version 更新を同じ filter start 世代で拾う用途には使わない。version 更新を継続取得する場合は `repeat=true`、または `stop()` / `flush()` / `configure()` / `start()` による明示的な世代更新を使う。
- filter queue では `payload_len == buffer_size` を正当な境界値として許容する。この payload は queue 全体を占有し、次 payload で overflow / drop 診断が出る。`payload_len > buffer_size` だけを oversized payload として drop する。
- DVR record queue は drop-new policy に固定する。満杯時に古いTSを捨てて新TSを入れる drop-old にはしない。録画 path では暗黙に古いデータを消して連続して見せるより、overflow / pending_overflow 診断で新規入力欠落を明示することを優先する。playback DVR は producer backpressure policy とし、record DVR と混同しない。

# r50ea92_av_shared_release_contract_fix

- Fix AV shared handle release contract mismatch: fd-bearing handles returned by getAvSharedHandle() are always rejected by releaseAvHandle() and no identity matching is performed.
- Make repeated getAvSharedHandle() idempotent with respect to exported backing: it returns a fresh fd duplicate without replacing an internal release identity.
- Keep releaseAvHandle(empty, 0) as the only client shared-handle release notification.

# r50ea91_build_gate_round6_unused_av_shared_helper

- Removed the unused `AvSharedBacking::clear_drop_only()` helper that failed Android Rust/Clippy `-D warnings`.
- Kept `SharedMemoryBacking::clear_drop_only()` because it is still used by DVR best-effort cleanup.
- This is a build-gate cleanup only; functional AV shared-memory behavior is unchanged.

# r50ea90_build_gate_round5_dvr_helper_restore

- Restore DvrHal cleanup/flush helper methods that were accidentally removed during r50ea89 dead-code cleanup.
- Fix binder_service build gate errors for DvrHal::close_internal(), DvrHal::configure(), and DvrHal::flush().

# r50ea89_build_gate_round4_dead_code_fix

- binder_service: remove or cfg-gate dead helper constants/functions rejected by Android Rust 1.75 `-D warnings`.
- binder_service: replace Copy-tuple `std::mem::drop(...)` parameter suppression with explicit underscore parameters.

# r50ea88_build_gate_round3_fix

- Fix binder_service build gate after r50ea87:
  - silence unused owner parameter in lifecycle cleanup collector wrapper;
  - make DemuxHal::ensure_open() drop the demux record MutexGuard before the Arc source binding leaves scope, avoiding Rust 1.75 temporary lifetime E0597 under Soong/Clippy.

# r50ea87_build_gate_round2_fix

- Fixed build-gate errors observed in r1 verification logs.
- Moved LNB open helper out of ITuner trait impl.
- Completed Px4 diagnostic saturation field initialization and snapshot export.
- Fixed FrontendTuneTxn same-request prepare_value usage.
- Corrected Filter/DVR drop diagnostic owner_demux_id field references.
- Fixed filter delay hint i32/i64 comparison.
- Restored source-filter test TS packet helper.

# r50ea86_build_gate_clippy_fix

- Fixed build-gate errors found by r50ea85 verify logs.
- Converted DVB last_common_tune stream_id from Option<u16> to Option<u32>.
- Removed unused/over-public soft_demux helpers that failed -D warnings Clippy.
- Removed an unused adaptation-field cursor assignment and unused test helpers.

# r50ea85_archive_layout_fix

- release: fixed the archive layout so extraction produces `vendor/maleicacid/tv/` and does not place `tuner_hal/`, `tis/`, `rec/`, `cas_hal/`, or project Markdown files at the AOSP root.
- source: no functional Tuner HAL code change from r50ea84_fix_11_14_15_16 except release version/changelog metadata.
- build / Rust unit test / atest / VTS / real-device verification are not run in this environment.

# r50ea84_fix_11_14_15_16

- binder_service: `stopTune()` demux boundary reset failure is now fail-closed for all frontend-bound demuxes; all bound demux runtime I/O is failed and all bound demux ledger entries are quarantined before returning the error.
- soft_demux: `setDataSource()` success and rollback now use the same source transition helper. Both previous and next source-filter origins receive downstream-filter-scoped section/PES boundary reset.
- soft_demux: downstream source-filter boundary reset no longer resets shared source-origin/PID continuity trackers. It clears only the target downstream filter section/PES assemblers and filter flush generations.
- packet_pipeline: added a downstream-filter-scoped assembler reset helper so source-filter boundary changes do not destroy unrelated downstream state sharing the same source origin/PID.
- build / Rust unit test / atest / VTS / real-device verification are not run in this environment.

# r50ea83_design_fixed_12_18_19_20

- Fixed design contracts for scan cancellation ownership: active scan is stopped through `stopScan()`, and public `stopTune()` during active scan remains `INVALID_STATE`.
- Fixed non-repeat `TableInfo` semantics as one table/version collection; version update requires `repeat=true` or explicit reconfigure/restart.
- Fixed filter queue boundary policy: payload length equal to buffer size is allowed and occupies the whole queue until drained.
- Fixed DVR record queue policy as drop-new with explicit overflow diagnostics, not drop-old.

# r50ea82_fix_4_23_27_29

- `setMaxNumberOfFrontends()` rejects unsupported frontend types even when `max_number == 0`.
- Playback malformed-only input is separated from normal consumed data and emits an explicit diagnostic trace.
- `IDvr.start()` now snapshots DVR fill/status after start commit before issuing the initial status callback.
- `releaseAvHandle()` rejects fd-backed `avMemory` with `avDataId == 0` as an invalid direct handle release combination.

# r50ea81_customer_reported_path_fixes

- binder_service: demux id pool 判定を checked_sub 化し、外部入力境界の整数overflowを防止。
- binder_service: frontend lease release / best-effort release を validate-before-mutate 化し、count欠落/underflowを診断化。
- binder_service: scan候補空を成功完了にせず UNAVAILABLE として拒否。
- binder_service: scan worker spawnを旧tune/旧scan破壊前に行い、stopScanは旧worker joinまで待つ。
- binder_service: scan callback failure / scan cleanup failure を live path failure へ過剰昇格しないよう分離。
- binder_service: FMQ write後の EventFlag / worker wake失敗を、queue保持済みdataのErr返却ではなく診断化。
- binder_service: SharedMemoryBacking::clear_result() で全fallible lock取得後にFMQをdrainするよう変更。
- binder_service: playback worker failureでDVR objectをclosed相当にせず、stop/flush/configure/getQueueDescの復旧余地を残す。
- binder_service: Playback DVR getQueueDesc/configureから worker/callback unhealthy gateを削除。
- binder_service: IFilter.start() のDATA_READY/StartId通知をstart commit後のreadiness snapshotに変更。
- soft_demux: configure_record_pid_set() をsnapshot rollback化し、途中失敗時の部分更新を戻す。
- build / Rust unit test / atest / VTS / real-device verification は未実行。

# r50ea80_final_optional_lock_sweep

- Record DemuxHal drop ledger re-lock failure instead of silently treating it as ordinary incomplete cleanup.
- Record demux source fail-closed handle lock failure before ledger quarantine instead of using silent optional lock handling.
- Final static optional-lock sweep for production paths after r50ea79.

# r50ea79_optional_lock_residual_cleanup

- binder_service: Startup/diagnostic file write registries の optional lock failure を低依存診断へ記録。
- binder_service: frontend scan session/terminal debug の optional lock failure を診断化。
- binder_service: Dvr best-effort callback worker lock failure を runtime failure として記録。
- binder_service: filter AV shared backing drop lock failure を診断counter + stderrへ記録。
- binder_service: Filter/Dvr close failure record の lock failure を drop cleanup診断へ記録。

# r50ea78_legacy_best_effort_structure_cleanup

- binder_service: shared memory worker wake/stop best-effort の lock failure を黙殺せず、error return または低依存診断へ記録。
- binder_service: frontend live pump wake/stop/unbind best-effort の lock failure を runtime failure 診断へ記録。
- binder_service: AV shared diagnostic worker の demux ledger / demux record lock failure を `unwrap_or_default` ではなく診断dumpへ明示。
- build / Rust unit test / atest / VTS / real-device verification は未実行。

# r50ea77_best_effort_diagnostic_residuals

- binder_service: RuntimeIoRegistry の best-effort 経路で runtime_io_entries lock failure を黙殺せず、低依存診断と counter へ記録。
- binder_service: LNB close/drop 診断記録と quarantine 記録の lock failure を黙殺せず、低依存診断と counter へ記録。
- binder_service: LNB callback clear の lock failure を診断化。
- build / Rust unit test / atest / VTS / real-device verification は未実行。

# r50ea76_remaining_best_effort_diagnostics

- binder_service: `FrontendRuntime::mark_live_path_failed()` の bound demux / demux handle / demux record lock failure を `unwrap_or_default()` / `unwrap_or(-1)` で通常継続せず、低依存診断と counter へ記録。
- binder_service: `DemuxHal::release_registration_best_effort()` の demux ledger begin-close / demux record / demux handle lock failure を黙殺せず、drop cleanup 診断へ記録。
- binder_service: AV shared handle identity の best-effort clear 失敗を診断 counter と低依存ログへ記録。
- build / Rust unit test / atest / VTS / real-device verification は未実行。

# r50ea75_frontend_unbind_remaining_lock_diagnostics

- `FrontendHal::unbind_frontend_demuxes_best_effort()` 内に残っていた `demux_record` / `demux_handle` / `demux_ledger` の `lock_mutex_option` 分岐を `lock_mutex_status` に変更。
- best-effort frontend unbind の record/state/ledger lock failure を「何もしない」扱いにせず、runtime failure と live path failure 診断へ記録するよう変更。
- build / Rust unit test / atest / VTS / real-device verification は未実行。

# r50ea74_frontend_unbind_best_effort_diagnostics

- `FrontendHal::unbind_frontend_demuxes_best_effort()` の `lock_mutex_option(...).unwrap_or_default()` による bound demux lock failure 黙殺を廃止。
- demux ledger lock / current binding 取得失敗を runtime failure と低依存診断ログへ記録。
- best-effort cleanup が「対象なし」と誤認して終了する経路を削減。

# r50ea73_live_pump_wake_diagnostics

- binder_service: `LivePumpWake::new()` の失敗を `.ok()` で黙殺せず、低依存診断ログへ記録するよう変更。
- binder_service: live pump wake fd write / lock 失敗を黙殺せず、runtime failure と診断ログへ記録するよう変更。
- build / Rust unit test / atest / VTS / real-device verification は未実行。

# r50ea72_frontend_readiness_optional_status_diagnostics

- binder_service: `getFrontendStatusReadiness()` の任意 readiness sample 取得で、backend `read_status()` 失敗を `.ok()` で黙殺せず、低依存診断ログへ記録して `None` として扱うよう変更。
- 主APIの戻り値契約は維持し、readiness観測だけを任意metricとして扱う。
- build / Rust unit test / atest / VTS / real-device verification は未実行。

# r50ea71_frontend_dvb_optional_status_diagnostics

- frontend_dvb: FE_READ_SIGNAL_STRENGTH / FE_READ_SNR の任意status取得失敗を黙殺せず、低依存診断ログへ記録するよう変更。
- read_status() の主契約である FE_READ_STATUS は従来通り必須とし、signal strength / SNR は任意metricとして扱う。

# r50ea70_frontend_px4_noop_cleanup

- frontend_px4 の production `let _ =` no-op を整理。
- `Px4FrontendBackend::validate_tune_request()` は `map_tune_request_to_px4(request)?` を直接実行し、破棄代入を削除。
- `px4_scan_requests()` も `map_tune_request_to_px4(base)?` を直接実行し、破棄代入を削除。
- build / Rust unit test / atest / VTS / real-device verification は未実行。

# r50ea69_frontend_dvb_noop_cleanup

- frontend_dvb の production `let _ =` no-op を整理。
- `validate_tune_request()` は検証戻り値を破棄代入せず、各検証を明示的に `?` で通す。
- DVB frontend enumerate 時の `FE_GET_INFO` 失敗を黙殺せず、当該 frontend を診断ログ付きでskipする。

# r50ea68_production_noop_cleanup

- soft_demux: `route_pes_packet_for_filter()` の不要な `let _ = pes.pts_90khz;` を削除。
- binder_service: `normalize_filter_delay_hint()` の `Instant::checked_add()` 検証を代入破棄ではなく明示 `is_none()` 判定に変更。
- test fallback: dma-heap fallback の test-only error drop を `drop(err)` に変更し、残存 `let _` を減らした。

# r50ea67_source_filter_downstream_drop_accounting

- r50ea66 の静的確認後の次修正として、source-filter downstream record / DVR mirror 経路に残っていた `let _ = ...` を削除した。
- downstream queue / DVR mirror が実際に保持されなかった場合は成功扱いにせず、`SOURCE_FILTER_DOWNSTREAM_DROP_COUNT` へ診断記録する。
- r50ea66 の public API rollback / cleanup failure handling は維持。
- build / Rust unit test / atest / VTS / real-device verification は未実行。

# r50ea66_lifecycle_runner_completion_static

- Lifecycle cleanup runner coverage extended beyond r50ea65 phase 1.
- Drop cleanup paths now avoid silent `let _ = txn.cleanup(...)` swallowing for Filter, DVR, and Descrambler by logging/diagnostic accounting.
- Stream-boundary best-effort failures now mark RuntimeIo failed and emit diagnostics.
- Frontend scan/tune best-effort worker-stop failures are recorded instead of silently discarded.
- Demux drop quarantine / cleanup-step recording failures are routed through drop cleanup diagnostics.
- Shared-memory playback worker best-effort stop failures are recorded.
- Public API rollback/cleanup paths retain r50ea65 common cleanup collector behavior.

Unverified: Android/Soong build, Rust unit tests, atest, VTS, and device tests.

# r50ea65_lifecycle_cleanup_runner_public_paths

- public API の openFilter/openDvr 失敗経路に残っていた手書き cleanup 集約を `LifecycleCleanupCollector` / `lifecycle_txn_cleanup_steps()` に移し、cleanup step の実行・最初の失敗保持・stderr 記録を一元化した。
- `openFilter()` / `openDvr()` の HAL 構築失敗時 cleanup は `collect_lifecycle_cleanup()` を通すようにし、ledger rollback / runtime unregister / demux unregister を同じ失敗収集規則に寄せた。
- `openFilter()` / `openDvr()` の commit 失敗時 cleanup は `lifecycle_txn_cleanup_steps()` を通すようにし、callback worker stop / queue stop / runtime unregister / demux unregister / ledger rollback の手書き first_error 分岐を削除した。
- ID mismatch rollback helper も `collect_lifecycle_cleanup()` へ寄せ、demux unregister と ledger rollback の失敗処理を共通化した。
- これは LifecycleTxn 完全 runner 化の phase 1 であり、全 API 横断の自動 rollback runner 化は未完了。build / Rust unit test / atest / VTS / real-device verification は未実行。

# r50ea64_public_api_rollback_failure_handling

- public API 主経路の手書きrollback/cleanup失敗を握りつぶさないよう修正した。
- `IDescrambler.setKeyToken()` の新token参照rollback失敗を error return + stderr 記録に変更した。
- `IFrontend.stopTune()` boundary cleanup の demux quarantine 失敗を runtime failure として記録し、戻り値へ反映するよう変更した。
- `openFilter()` / `openDvr()` の HAL 構築失敗時 cleanup で、cleanup失敗を元エラーの陰に隠さず cleanup failure として返すよう変更した。
- filter/DVR open commit failure cleanup の ledger rollback 失敗を具体Statusとして保持するよう変更した。
- build / Rust unit test / atest / VTS / real-device verification was not run in this environment.

# r50ea63_fix_residual_similar_bugs

- soft_demux: remove production use of PID-wide all-origin generation/flush pruning from unregister_filter().
- soft_demux: replace ambiguous Option-origin assembler removal with explicit all-origins-by-filter-id removal for removed filters only.
- binder_service: replace openFilter/openDvr ID-mismatch best-effort rollback branches with shared rollback helpers that surface cleanup failure.
- Note: build/atest/VTS/hardware validation not run in this environment.

# r50ea62_complete_residual_1_4

- r50ea61で未達として残った residual 1〜4 を追加修正した。
- `openFilter()` / `openDvr()` は soft demux 登録前に候補IDを取得し、ledger reserve を先行させる順序へ変更した。登録失敗時は reserved ledger を rollback する。
- `packet_pipeline.rs` の旧 `retain_generations_not_pid()` / 広域 `mark_filter_flush_generation()` 経路を production から外し、origin 明示の generation drop / flush marking へ寄せた。
- `mark_filter_flush_generation()` は実接続中の input origin のみを対象にし、frontend/playback/source-filter origin を無条件に広くmarkしない。
- `FilterHal::drop()` / `DvrHal::drop()` の cleanup 失敗時は demux record の filter/dvr ledger を quarantine へ落とし、診断counterとstderrへ記録する。
- build / Rust unit test / atest / VTS / real-device verification was not run in this environment.

# r50ea61_residual_1_4_rebuild

- r50ea60 の残作業1〜4を対象に、手書き経路を共通helperへさらに移管した。
- `openDemuxById()` の既存 demux 経路を、refcount acquire 成功後に binder を生成する順序へ修正し、binder 生成失敗時は取得した参照を rollback する。
- `openFilter()` / `openDvr()` の open reserve / commit / rollback / demux unregister を DemuxHal helper へ集約し、個別API内の ledger 手書き処理を縮小した。
- packet pipeline の generation pruning を origin-aware 化し、frontend / playback / source-filter origin をまたいで同一PIDの generation / flush state を消さないよう修正した。
- `mark_filter_flush_generation()` は実接続中の source origin のみを追加対象にし、全 filter を仮想 source origin として列挙する経路を削除した。
- `FilterHal::drop()` / `DvrHal::drop()` は cleanup 失敗を診断counterとstderrへ記録し、Drop cleanup失敗を完全には握りつぶさない。
- build / Rust unit test / atest / VTS / real-device verification was not run in this environment.

# r50ea60_rebuild_deleted_paths

- r50ea59で削除・置換した手書き経路を再点検し、payload delivery の保持判定を `accepted_entries` ベースに修正した。
- `RecordPacket` は `FilterPayload::len()==0` であるため、従来の `accepted_bytes > 0` 判定では保持済みでも event / delivery 成功扱いにならない問題があった。`QueuePushOutcome.accepted_entries` を追加し、queue entry が保持されたかで判定するようにした。
- `push_ts_packet_record_only()` は有効packetなら常に成功ではなく、record filter / attached DVR いずれかへ実際に保持された場合だけ成功を返すようにした。
- live / playback / source-filter 経由の TS delivery は、Raw / Record / DVR mirror / Section / PES の実保持結果を集計し、全て drop された場合は `DroppedNoDelivery` とするよう修正した。
- `mirror_filter_payload_to_record_dvrs()` は戻り値を持ち、attached DVR のどれかに実際に保持されたかを返すようにした。
- `route_pes_packet_for_filter()` は戻り値を持ち、PES/AV payload が実際にqueueへ保持されたかを呼び出し元へ返すようにした。
- build / Rust unit test / atest / VTS / real-device verification was not run in this environment.

# r50ea59_delete_handwritten_paths

- r50ea58 をベースに、再発源になっていた手書き経路を先に削除・置換した。
- `openFilter()` / `openDvr()` で soft demux 登録後の ledger reserve 失敗時に demux 登録を同期 rollback するよう修正し、幽霊 filter / DVR を残さない。
- filter payload delivery は `push_filter_payload_for_delivery()` に集約し、queue push が保持されなかった場合は event 発火・downstream 伝播を行わない。
- media filter で単一 payload が buffer size を超える場合は、enqueue 後に自己削除するのではなく、new payload drop として診断記録する。
- `getQueueDesc()` / `getAvSharedHandle()` / `configureIpCid()` / `configureMonitorEvent()` / `IDvr.attachFilter()` / `IDvr.detachFilter()` / `IDvr.setStatusCheckIntervalHint()` から callback healthy の過剰 gate を削除した。
- build / Rust unit test / atest / VTS / real-device verification was not run in this environment.

# r50ea58_same_root_cleanup_completion

- r50ea57 で残っていた同根候補の横断修正を実施した。
- `retry_after_interrupted_read()` に saturated flag 付きAPIを追加し、DVB/PX4 live reader が EINTR retry counter saturation を観測できるようにした。
- `TsPacketCompletionBuffer` の malformed byte 累計に saturated flag を追加した。
- filter / DVR queue accounting の `usize::MAX` clamp と `saturating_sub()` を廃止し、overflow/underflow を diagnostic saturation として記録するようにした。
- source filter downstream 再帰で source origin が取得できない場合の generation 0 fallback を削除した。
- PX4 frontend export ID 計算を checked arithmetic にし、範囲外 probe を export しないようにした。
- descrambler runtime id allocator が overflow 時に `next_id=0` を保存しないようにした。
- AOSP linkCaps が main type 粒度である点、および adaptation-only packet の continuity 方針を DESIGN_JA.md に固定した。


## r50ea57_completion_tests

- r50ea56の未達だった完了条件12に対応し、LNB generation exhaustion、flush cleanup failure、diagnostic counter saturation、callback cumulative byte overflowの単体テストを追加。
- callback cumulative bytes overflowをBYTE_NUMBER_OVERFLOW経路へ統一するhelperを追加。

# r50ea56_clear_and_diagnostic_completion

- 明確な未達 No.1/9/10/19 を補正。
- `LnbHal::update_lnb_state()` の LNB registry guard を mutable にし、generation 枯渇時の quarantine 更新がRust構文上成立するよう修正。
- `IFilter.flush()` は demux flush 成功後の local cleanup 失敗を Err として返さず、flush cleanup diagnostic counterへ記録するよう修正。
- `IDvr.flush()` も demux flush 成功後の playback discard / record queue clear 失敗を Err として返さず、diagnostic counterへ記録するよう修正。
- tuner / PX4 / descrambler / diagnostic file / AV shared / playback diagnostic counter の飽和処理を checked 系に寄せ、saturated flag 記録を追加。
- callback cumulative bytes と playback byte counter は checked_add() により overflow を検出する。
- build / Rust unit test / atest / VTS / real-device verification was not run in this environment.

# r50ea55_design_ja_consistency_fix

- DESIGN_JA.md の追加設計契約と旧表・旧補足の内部矛盾を解消。
- setDataSource() の source lifecycle 異常を INVALID_STATE へ統一。
- IDescrambler.addPid()/removePid() から key token 未設定拒否を削除し、key/PID lifetime 分離へ統一。
- key token の non-VOID 長を HAL key table 発行の 8 byte opaque token へ統一し、persistent Expired slot 要求を削除。
- r50dz15 tune 補足を validate/prepare-before-destroy の transaction 契約へ統一。
- worker failure 後の flush を復旧操作として許可する表へ統一。

# r50ea54_adaptation_field_validation

- WP-06: MPEG-TS adaptation field の flag別長さ検証を厳密化。
- PCR / OPCR / splicing / private data / extension flag が立っているのに必要byte数が不足するpacketを malformed として拒否。
- 正常PCR/OPCR/private/extension境界と不足境界の単体テストを追加。

# r50ea53_flush_stop_recovery_ops

- WP-05: `IFilter.flush()` を callback worker unhealthy 時にも実行可能にし、復旧操作として demux flush / queue clear を行うよう修正。
- WP-05: playback `IDvr.stop()` を playback worker unhealthy 時にも実行可能にし、started lifecycle を停止できるよう修正。
- WP-05: `IDvr.flush()` を callback worker / playback worker unhealthy 時にも実行可能にし、demux flush / queue clear を復旧経路として維持。
- 通常配送 API の callback / worker health gate と、復旧 API の flush / stop を分離。

# r50ea52_counter_generation_completion

- WP-04 completion fix: constrain filter delivery generation / StartId generation to the i32 StartId domain instead of clamping to i32::MAX.
- Add saturated flags to section assembler diagnostic counters and propagate section counter saturation to filter diagnostics.
- Add saturated flags for PES assembler overflow diagnostics.
- Add saturated flags for DVB/PX4 live reader malformed-byte diagnostics.
- Preserve data-path behavior when diagnostic counters saturate; no wrap or panic is introduced.

# r50ea51_counter_generation_overflow

- WP-04: section/PES assembler generation を `saturating_add()` から `checked_add()` ベースへ変更し、overflow時は該当packetをdropし partial state を破棄するよう修正。
- filter delivery generation / StartId 系の主要更新経路を `checked_add()` へ変更し、overflow時は `IdExhausted` または filter failed へ寄せるよう修正。
- worker wake generation / AV dataId / key token ID の既存checked系と同じ方針に寄せた。
- 診断counterの `saturating_add()` は data path failure へ昇格しない診断用途として維持。
- build / Rust unit test / atest / VTS / real-device verification was not run in this environment.

# r50ea50_frontend_tune_transaction

- WP-03: `IFrontend.tune()` を validate / prepare / apply / commit / rollback の transaction へ整理。
- tune request validation、callback path lock、旧 active tune snapshot、LNB candidate existence check を旧 tune 破壊前に実施。
- backend submit / boundary reset / worker spawn 後の失敗では旧 tune restore を試み、復旧不能時のみ frontend failed + bound demux quarantine へ落とす。
- tune worker spawn failure handler から先行 cleanup を削除し、transaction 側 rollback に集約。

# r50ea49_source_filter_flush_completion

- WP-02 completion follow-up.
- Fixed source filter flush/reconfigure/close boundaries so the old SourceFilter origin partial state is reset for connected downstream filters.
- Added a regression test that verifies raw TS -> record downstream remains connected across source flush while old source-origin continuity state is not reused.

# r50ea48_source_filter_state_ownership

- WP-02: DESIGN_JA.md の source filter origin / downstream 状態所有契約を実装。
- source filter 経由 setDataSource() を raw TS -> raw TS / record の正式対応範囲に限定し、非対応 linkage は UNAVAILABLE へ固定。
- downstream 未接続または未設定時に source origin continuity / assembler を進めないよう修正。
- source filter reconfigure / close の downstream 接続解除境界で downstream partial state を破棄するよう修正。
- source filter 経由の section/PES 直接多段配送を行わないよう明示。

# r50ea47_key_token_refcount_completion

- WP-01 key token refcount の完了条件未達を補正。
- `expire_all_by_origin()` / `expire_all()` が refcount > 0 の token slot を削除しないよう修正。
- 同一 token 再設定 no-op、複数 descrambler 共有、unknown token 設定失敗時の旧 token 維持を検証する単体テストを追加。
- malformed / unknown token の戻り値分離を検証する単体テストを追加。

# r50ea46_key_token_refcount

- WP-01 implementation: make descrambler key token table a refcounted shared resource keyed by token bytes.
- Added `acquire_ref_with_diagnostic()` / `release_ref()` and removed session release from destructive one-shot token expiration semantics.
- `IDescrambler.setKeyToken()` now treats same-token reassignment as success no-op, acquires the new token before releasing the old token, and rolls back the newly acquired reference if old-token release fails.
- Key token release during descrambler close now clears the session token after successful release so close retry does not double-release the same reference.
- Unknown/expired non-VOID key tokens now map to `INVALID_STATE`; malformed/empty tokens remain `INVALID_ARGUMENT`.
- Added key table unit tests for shared-token refcounting and release-without-acquire behavior.
- Build / Rust unit test / atest / VTS / real-device verification was not run in this environment.

# r50ea45_design_cross_lifetime_contracts

- 採用済み設計素案を `DESIGN_JA.md` に反映した。
- key token 所有権・参照カウント契約を追加し、HAL 内部 key material の共有、参照数、同一 token 再設定、refcount 0 時削除を固定した。
- source filter origin / downstream 状態所有契約を追加し、capability に advertised した raw TS / record 系 linkage だけを正式対応とすること、未対応 linkage は `UNAVAILABLE` とすることを固定した。
- `IFrontend.tune()` transaction 契約を追加し、validate / prepare 完了前に旧 tune を破壊しないこと、worker spawn 失敗時の rollback / quarantine 方針を固定した。
- counter / generation overflow 契約を追加し、寿命IDは `checked_add()`、診断counterは saturated flag 記録に分離した。
- HAL責務境界を追記し、ARIB SI/PSI意味解析、EPG生成、TvProvider登録、予約追従判断を Tuner HAL 責務外として固定した。
- 実装コードは変更していない。build / Rust unit test / atest / VTS / 実機確認は未実行。

# r50ea44_scan_progress_after_candidate

- scan worker の `PROGRESS_PERCENT` 通知を、候補の tune / lock 判定完了後だけに送るよう修正した。
- 候補開始通知は `FREQUENCY` message に限定し、候補1件で tune 前に `PROGRESS_PERCENT=100` を送らないようにした。
- locked 経路では `LOCKED` event と stream id message の送信後、no-signal 経路では `NO_SIGNAL` event の送信後に完了済み候補数ベースの progress を送る。
- build / Rust unit test / atest / VTS / 実機確認は未実行。

# r50ea43_stage7_static_confirmation

- 実装修正計画の段階7として、段階1〜6の静的確認を実施した。
- 直近20件のうち、段階計画対象の項目は静的確認上すべて「修正済み」または「設計上対象外/修正不要」に整理済み。
- 以前から未協議として除外している section/PES/filter delivery generation の 4/5/6 は本段階の修正対象外であり、未解決扱いを維持する。
- `future_work/` 既知項目を解決済みとは扱っていない。
- build / Rust unit test / atest / VTS / 実機確認は未実行。

# r50ea42_impl_stage6_worker_signal_generation

- 実装修正計画の段階6として、worker signal の work_generation 更新を `saturating_add()` から `checked_add()` に変更した。
- `notify_work()` / `request_stop()` / `clear_for_start()` で generation 上限到達時に runtime failure を立て、`WorkerExit::RuntimeFailure` を signal 側 exit reason として記録するようにした。
- runtime failure 設定後の `set_exit_reason()` が正常停止理由で runtime failure を上書きしないようにした。
- worker panic/error 診断カウンタの表示値計算からも `saturating_add()` を除去した。
- build / Rust unit test / atest / VTS / 実機確認は未実行。

# r50ea41_remove_dead_old_helpers

- r50ea40までの段階1〜5修正後に参照ゼロとなった旧helperを削除。
- 未使用の `RuntimeIoRegistry::unregister_filter_best_effort()` を削除。filter close経路は通常API用 `unregister_filter()` と既存Drop/close cleanup経路に統一。
- 未使用の `DvrHal::stop_callback_worker_best_effort()` を削除。callback worker停止は通常close cleanup経路へ統一。
- 未使用の `AvSharedBacking::build_native_handle()` test helper を削除。AV shared handle exportは identity 付き `build_native_handle_with_identity()` に統一。
- 実装本体の機能変更は行わず、旧経路の残存除去のみ。build / Rust unit test / atest / VTS / 実機確認は未実行。

# r50ea40_impl_stage5_dvb_close_lnb_lifetime

- Stage5 implementation: make DVB close release stream reader/demux handles even when DMX_STOP fails, while recording the DMX_STOP failure as close diagnostics.
- Stage5 implementation: keep stop_tune fail-closed behavior unchanged; only close uses the best-effort fd-release path.
- Stage5 implementation: replace LNB runtime generation saturating increments with checked generation handling and quarantine on exhaustion.
- Stage5 implementation: do not set LnbHal.closed before Drop-time safe reset; failed drop reset records diagnostics and leaves failure state observable.
- Build/atest/VTS/real-device verification was not run in this environment.

# r50ea39_impl_stage4_flush_detach_unregister

- 実装修正計画の段階4として、flush / detach / unregister の原子性と missing 検出を修正した。
- `IFilter.flush()` は soft demux の primary flush が成功した場合に限り、通常FMQ、AV FMQ、AV shared backing を clear / release するようにした。
- `IDvr.flush()` は soft demux の primary flush が成功した場合に限り、record queue clear または playback input discard を行うようにした。
- `IDvr.detachFilter()` は record DVR に対象 filter が attach 済みでない場合、成功 no-op ではなく `INVALID_STATE` を返すようにした。
- runtime I/O registry の `unregister_filter()` / `unregister_dvr()` は、通常API経路では対象entry不存在を `INVALID_STATE` として返すようにし、Drop/teardown用の best-effort unregister と分離した。
- build / Rust unit test / atest / VTS / 実機確認は未実行。

# r50ea38_impl_stage3_descrambler_token_lifetime

- 実装修正計画の段階3として、descrambler key token の寿命管理を修正した。
- key token の有効長を内部発行長と同じ 8 bytes に固定し、9〜16 bytes の token を malformed として拒否するようにした。
- `expire_token()` は key table 本体へ Expired 状態を残さず、active slot を削除するようにした。
- token release / expire は cleanup retry に耐えるよう idempotent にし、既に削除済みの8 bytes token は成功扱いにした。
- `expire_all_by_origin()` / `expire_all()` は対象tokenを Expired 状態へ変更せず、table から削除するようにした。
- 既存の descrambler key table 単体テスト期待値を、Expired 残留ではなく削除後 Unknown へ更新した。
- build / Rust unit test / atest / VTS / 実機確認は未実行。

# r50ea37_impl_stage2_boundary_backend_scope

- 実装修正計画の段階2として、stream boundary reset の複数 bound demux 処理を全件走査に変更した。
- `reset_bound_demuxes_for_stream_boundary()` は1件目の失敗で即returnせず、各 demux に reset または runtime failed 診断を適用し、最後に最初の失敗を返すようにした。
- `reset_bound_demuxes_for_stop_tune()` も全件走査に変更し、失敗demuxを runtime failed / demux quarantine として記録してから最後に失敗を返すようにした。
- stream boundary と live path failure の px4 path diagnostics は現在 backend が PX4 の場合だけ渡すようにし、DVB 経路で px4 診断を更新しないようにした。
- frontend unbind / best-effort unbind の boundary reset でも backend 種別に応じた px4 diagnostics の有無を使うようにした。
- build / Rust unit test / atest / VTS / 実機確認は未実行。

# r50ea36_impl_stage1_noop_callback_scope

- 実装修正計画の段階1として、同一frontend/generationの `IDemux.setFrontendDataSource()` を成功 no-op にし、stream boundary reset / bind / unbind を実行しないようにした。
- 同一 active tune request の `IFrontend.tune()` を成功 no-op にし、scan cancel / tune worker停止 / live pump停止 / backend stop / demux boundary reset を実行しないようにした。
- frontend callback未登録・callback Binder error・scan/tune通知失敗を live path failure へ昇格させないようにし、通知経路の診断とcallback登録状態だけへ閉じるよう補正した。
- DVB/PX4 backend の `mark_callback_failed()` が tuning state / lock telemetry を落とさないよう補正した。
- filter / DVR callback worker の queue fill水位取得失敗を data path runtime failure へ昇格させず、その周期の水位通知だけをスキップするよう補正した。
- build / Rust unit test / atest / VTS / 実機確認は未実行。

# r50ea35_design_code_contract_docs

- `DESIGN_JA.md` の `Tuner HAL runtime 完了条件` を `Tuner HAL runtime 設計契約` へ改名し、完了判定ではなく公開API状態、内部事象、資源寿命、失敗時波及範囲の設計契約であることを明確化した。
- `DESIGN_JA.md` の `受け入れ検査` を `設計表の自己整合条件` へ改名し、検証ゲートではなく設計表の読み取り一意性を確認する章へ補正した。
- `DESIGN_JA.md` から `設計追加に対する実装完了条件` を削除し、build / atest / VTS / 実機確認を設計本文へ置かない方針に合わせた。
- `DESIGN_JA.md` に、失敗領域と波及範囲、同一条件 no-op、public API transaction、best-effort 使用範囲、寿命ID/世代ID/token、backend state model、source filter downstream 契約の設計表を追加した。
- `CODE_CONVENTION.md` の `完了判定` を `実装規約の静的確認観点` へ改名し、リリース完了条件、WP完了条件、atest/VTS合格条件を定義しないことを明記した。
- `CODE_CONVENTION.md` に、失敗領域混同禁止、public API transaction、同一条件 no-op guard、best-effort 使用制限、寿命ID/generation/token、backend診断分離、source filter 実装規約を追加した。

# r50ea34_dvb_close_clear_best_effort

- DVB backend の `close()` を DESIGN_JA.md の設計に合わせ、`DTV_CLEAR` / `clear_properties()` 成功を close 成功条件から外した。
- `close()` は `stop_stream_reader()` を試行し、`clear_properties()` は best-effort diagnostic として記録し、`control = None` と tuning/telemetry の closed 状態 commit を必ず行う。
- `close_clear_failure_is_best_effort_and_commits_closed_state` を追加し、`clear_properties()` 失敗時も fd release 相当の closed state へ進むことを固定した。
- build / Rust unit test / atest / VTS / 実機確認は未実行。

# r50ea33_source_filter_design_alignment

- `DESIGN_JA.md` の `setDataSource()` 互換表と SourceFilter 境界記載を、実装済みの raw TS packet 再投入方針へ統一した。
- source filter として指定できるのは TS生データフィルタだけである、と固定した。
- 下流の section / PES / TS生データ / AV / record フィルタは、source filter から受け取った raw TS packet を再解析する sink として扱う、と固定した。
- section payload、PES payload、AV payload、record payload の直接多段再配送は、本製品の正式仕様として非対応であり、暫定的なリリース範囲ではないことを明記した。
- 実装コードは r50ea32 時点の raw TS source 限定方針と `linkCaps` / `can_link_filter_open_types()` に整合しているため、今回は `DESIGN_JA.md`、`CHANGELOG.md`、`RELEASE_VERSION` のみを更新した。
- build / Rust unit test / atest / VTS / 実機確認は未実行。

# r50ea32_record_pts_test_syntax_fix

- r50ea31 の record PTS boundary 追加テストで、`pts_header_carry_crosses_ts_payload_boundary()` の閉じ brace が不足し、後続 `#[test]` が関数内へ入る構文未達を修正した。
- `r50dz52_g2_03_tests` の brace 構造を整理し、PTS header carry / start prefix fragment carry / malformed prefix fragment の3テストを独立した test function として成立させた。
- No.8 / No.20 の実装修正内容そのものは変更せず、未達だったテスト構文のみを是正した。
- build / Rust unit test / atest / VTS / 実機確認は未実行。

# r50ea31_record_pts_fmq_descriptor_fix

- 追加指摘 No.8 と No.20 の実装修正を行った。
- record index の PTS 抽出で、PUSI付きPES開始の start code prefix `00 00 01` が TS payload 境界で `00 00` / `01 ...` に分割された場合も `pes_header_carry` へ保持し、次 payload で PTS を復元できるよう修正した。
- malformed な PES start prefix fragment は carry を破棄し、後続 payload を誤ってPES headerとして扱わないよう修正した。
- FMQ descriptor export 時に複製 fd の `fstat` サイズを取得し、各 grantor の `fdIndex / offset / extent` が `offset + extent <= fd size` を満たすことを Rust 側で検証するよう修正した。
- grantor range の fd size 超過・加算overflow・fd index範囲外を `descriptor_internal_error` として拒否する単体テストを追加した。
- build / Rust unit test / atest / VTS / 実機確認は未実行。

# r50ea30_impl_round6_static_verified

- 第6回として、強い候補12件の実装・設計対応状況を静的確認した。
- `DESIGN_JA.md` 内部の unbounded PES 上限超過境界が、r50ea24 で追加した PES assembler 異常系状態表および r50ea25 実装と矛盾していたため、oversized PES は破棄して次PUSIから再同期する設計へ統一した。
- 強い候補12件について、関数名・状態遷移・追加テストの存在を静的確認した。
- リリース物の規則について、ルート直下Markdown追加なし、future_work 変更なし、CHANGELOG降順、実行権限付きファイル追加なしを静的確認した。
- build / Rust unit test / atest / VTS / 実機確認は未実行であり、本版は静的確認済み・未実行ゲート残ありの成果物である。

# r50ea29_impl_round5_partial

- 強い候補第5回として、`stopTune()` と AV shared backing の実装修正を行った。
- `IFrontend.stopTune()` は backend stop 後の demux stream boundary reset 失敗時に、対象 demux を quarantine し、runtime I/O を failed として通常配送可能状態に残さないよう修正した。
- stopTune 用の `StreamBoundaryReason::TuneStop` と demux quarantine helper を追加した。
- `AvSharedBacking` の `active` / `reserved` / `free` / `next_generation` / `quarantined` を単一 `AvSharedState` mutex 配下へ移し、`clear_result()` / `release()` / `release_all()` の slot 状態更新を一括 commit にした。
- AV shared backing の clear 成功・generation 枯渇時状態不変、および stopTune boundary failure quarantine の単体テストを追加した。
- 本版は第5回 partial であり、第6回の全体検証・リリース判定は未対応。build / atest / VTS / 実機確認は未実行。

# r50ea28_impl_round4_partial

- 強い候補第4回として、Filter / DVR close と quarantine の実装修正を行った。
- `IFilter.close()` の demux unregister で missing filter を通常成功扱いしないよう修正した。
- `IDvr.close()` の demux unregister で missing DVR を通常成功扱いしないよう修正した。
- demux 側 missing unregister は cleanup failed / quarantine 経路へ落ち、close retry 可能な状態として残る。
- `filter_close_demux_unregister_missing_is_not_success` / `dvr_close_demux_unregister_missing_is_not_success` を追加した。
- 本版は第4回 partial であり、stopTune / AV shared backing は未対応。build / atest / VTS / 実機確認は未実行。

# r50ea27_impl_round3_partial

- 強い候補第3回として、playback DVR worker の実装修正を行った。
- `DemuxHandle::inject_playback_payload_result()` を追加し、playback投入結果を `ConsumedWithDelivery` / `ConsumedNoDelivery` / `Malformed` / `InvalidState` / `InternalError` に分離した。
- valid TS だが配送先がない場合は `ConsumedNoDelivery` として非fatalにし、worker failure にしない。
- malformed playback input は診断加算後に `Malformed` として扱い、worker failure ではなく drop+診断の非fatal経路に固定した。
- playback worker failure 経路から `demux.unregister_dvr()` の直接呼び出しを削除し、DVR cleanup は `IDvr.close()` の ledger 経路へ集約した。
- 関連する playback no-delivery / malformed result テストを追加した。
- 本版は第3回 partial であり、Filter/DVR close quarantine / stopTune / AV shared backing は未対応。build / atest / VTS / 実機確認は未実行。

# r50ea26_impl_round2_partial

- 強い候補第2回として、SourceFilter / flush generation の実装修正を行った。
- `TsInputOrigin::SourceFilter` を `source_filter_id` と `source_filter_generation` を含む origin key に変更した。
- flush / setDataSource / configure / source unlink 境界で SourceFilter origin の旧 section/PES assembler state を破棄できるよう、flush generation 記録を SourceFilter origin まで拡張した。
- `setDataSource()` の正式対応を raw TS source の再投入経路に固定し、section/PES/AV payload の直接多段再配送を拒否するよう修正した。
- SourceFilter 経由の再投入は raw TS packet を再解析する経路だけを使い、section/PES payload の直接下流伝播は行わない。
- 関連する SourceFilter origin generation / 非対応 payload chain 拒否テストを追加・更新した。
- 本版は第2回 partial であり、playback DVR worker / close quarantine / stopTune / AV shared backing は未対応。build / atest / VTS / 実機確認は未実行。

# r50ea25_impl_round1_partial

- 強い候補第1回として、build gate / PES assembler 異常系 / DVB-PX4 poll EINTR retry を実装修正した。
- Filter callback worker の BYTE_NUMBER_OVERFLOW ログでスコープ外の `dvr_id` を参照する箇所を `filter_id` へ修正した。
- PES assembler は malformed / continuation-only / oversized / flush boundary で state を破棄し、次の PUSI から再同期する方針へ修正した。
- `PES_packet_length == 0` の unbounded PES は次 PUSI 境界では完成可能だが、flush / stop / close 境界では完成扱いにしない。
- DVB / PX4 の `poll()` は `EINTR` を同一 cycle 内で retry し、`WouldBlock` / timeout / fatal error と分離した。
- 本版は第1回 partial であり、SourceFilter / DVR worker / close quarantine / stopTune / AV shared backing は未対応。build / atest / VTS / 実機確認は未実行。

# r50ea24_failure_boundary_design_contract

- DESIGN_JA.md に「失敗時状態・境界処理の設計固定」を追加した。
- `TsInputOrigin` / flush generation / SourceFilter 境界を、Frontend / Playback / SourceFilter の三種類に固定した。
- PES assembler の malformed / continuation-only / oversized / flush boundary の状態表を固定した。
- worker failure は直接 unregister せず、Filter / DVR の close 経路に cleanup を集約する方針に固定した。
- close / unregister / quarantine 条件、`IFrontend.stopTune()` の backend stop 後 demux boundary reset 失敗時状態、AV shared backing の失敗時原子性不変条件を固定した。
- 今回は設計文書固定のみであり、build / atest / VTS / 実機確認は未実行。

# r50ea23_dvb_frontend_id_bitpack_candidate

- A13 の DVB frontend ID を固定ビット割当に変更した。
- `adapter_id * 10 + frontend_index * 2` の狭い桁幅式と、列挙後の `index + 1` 再採番を廃止した。
- DVB frontend ID は `2_000_000 + (adapter_id << 12) + (frontend_index << 4) + variant` とし、variant は ISDB-T=0 / ISDB-S=1 に固定した。
- adapter_id / frontend_index / variant の範囲外値を export 対象外にし、duplicate ID 検出は最終保険として残した。
- `dvb_frontend_id_bitpack_avoids_adapter_frontend_collision` / `dvb_frontend_id_rejects_out_of_range_fields` を追加した。
- build / atest / VTS / 実機確認は未実行。

# r50ea22_a20_av_shared_slot_reservation_candidate

- A20 `AvSharedSlotReservation` を実体化し、AV shared slot 予約を RAII guard が所有する構造へ変更した。
- `AvSharedBacking::allocate()` は `reserve_slot()` で guard を取得し、`commit()` 成功時だけ active slot へ移す。途中失敗時は guard の Drop が予約slotを freeへ返す。
- reservation Drop 中の返却失敗は shared backing を quarantined として記録する。
- `av_slot_reservation_drop_returns_slot` を `rollback_reserved_slot()` 直呼びではなく、reservation guard の Drop で検証する内容へ修正した。
- build / atest / VTS / 実機確認は未実行。

# r50ea21_static_path_audit_candidate

- rev4固定表に基づく production 主経路監査を開始。
- B20 `IDescrambler.setKeyToken()` の non-VOID 経路で session lock を解放してから再取得する旧構造を削除。
- key token 解決、旧token release、新token commit を同一 `DescramblerSession` lock 下に寄せ、並行 `setKeyToken()` で要求key未反映の `Ok` が起きない構造へ修正。
- 40件の主経路照合結果を `r50ea21_rev4_main_path_audit.csv` として外部提示。

# r50ea20_test_body_retry_audit_candidate

- r50ea19 のテスト本文監査を継続し、名前だけ/単一assertに近かった configure cleanup retry 系固定テストを production ledger 経由の close 再試行検証へ強化した。
- `configure_av_stream_type_cleanup_failed_allows_close_retry` / `filter_configure_cleanup_failed_allows_close_retry` / `dvr_configure_cleanup_failed_allows_close_retry` が、quarantine → cleanup step記録 → begin_close再試行 → commit_close → generation解放まで確認するようになった。
- rev4固定テスト名78件の存在、自己参照 `include_str!("*.rs")` 不在、一時ファイル不在を再確認した。
- build / atest / VTS / 実機確認は未実行。

# r50ea19_test_body_and_lnb_guard_audit_candidate

- rev4 固定テスト78件を、rev4表から厳密抽出したテスト名集合として再定義した。
- テスト本文監査を実施し、自己参照 `include_str!("*.rs")`、欠落、重複がないことを確認した。
- B06 の LNB guard release failure テストから FakeLnbGuardDropState 旧構造を削除し、production `LnbLedger::operation_guard()` → `LnbHal::acquire_operation_guard()` → LNB quarantine 経路を直接検証するテストへ置換した。
- r50ea13 で生成した test mapping CSV に混入していた rev4監査行3件（テスト名ではない `ResolvePendingDemuxBinding` / `retry_pending` / `UNKNOWN_ERROR`）を、今回の厳密抽出では除外した。
- build / atest / VTS / 実機確認は未実行。

# r50ea18_best_effort_main_path_audit_candidate

- r50ea17 の main_remaining_review 41件は「残す判断」ではなく、非主経路と証明できていない未分類対象として扱うことを明確化した。
- AV shared backing の quarantine marker を best-effort から Result 返却へ変更し、quarantine flag 書き込み失敗を呼び出し元へ返すよう修正した。
- frontend live pump の rollback / spawn failure / tune rollback 経路で `stop_live_pump_best_effort()` を正式 `stop_live_pump()` へ置換し、stop失敗を rollback failure として扱うよう修正した。
- DVR ExternalClose の callback worker stop/wake 失敗を `let _ =` で捨てず、cleanup failure として返すよう修正した。
- r50ea18 は best_effort main-path 監査の継続候補であり、40件全件主経路照合・build / atest / VTS / 実機確認は未実行。

# r50ea17_best_effort_scope_audit_candidate

- r50ea16を入力に、formal cleanup候補と非主経路 best-effort 対象を呼出元単位で再分類した。
- 公開API主経路に残っていた DVR playback open rollback の `unregister_dvr_best_effort()` を通常 `unregister_dvr()` へ置換した。
- `DemuxHal::close_internal()` の cleanup failure 記録で ledger quarantine / cleanup step 記録失敗を握りつぶさないよう修正した。
- `IDemux.setFrontendDataSource()` の fail-closed 遷移で demux ledger quarantine 失敗を握りつぶさないよう修正した。
- Drop保険・runtime failure containment・diagnostic-only・validation-probe の非主経路対象を r50ea17 best-effort scope report に分離した。
- build / atest / VTS / 実機確認は未実行。

# r50ea16_formal_cleanup_callback_audit_candidate

- r50ea15 の best_effort 分類で formal_cleanup_candidate とされた Filter/DVR callback failure と cleanup failure 記録経路の一部を、正式 cleanup 主経路へ再集約した。
- `fail_from_callback()` は `close_internal()` を呼ばず、commit後 callback failure を `callback_unhealthy` として記録するだけに変更した。
- Filter/DVR worker failure は `LifecycleTxn` の `let _ =` 経由をやめ、runtime failed と callback stop flag の直接記録へ単一化した。
- Filter/DVR close failure の cleanup step 記録失敗を捨てず、`CloseFailureRecord` の reason に残すよう変更した。
- Filter/DVR configure後 cleanup failure の ledger quarantine 失敗を捨てず、`CloseFailureRecord` の reason に残すよう変更した。
- ledger `commit_close()` 後の Done step は ledger記録へ戻さず、object-local `next_cleanup_step` のみ更新するよう変更した。
- r50ea16 は formal_cleanup_candidate の一部是正であり、best_effort 83箇所全件解消・40件全件主経路照合完了ではない。build / atest / VTS / 実機確認は未実行。

# r50ea15_best_effort_lnb_alignment_audit_candidate

- DESIGN_JA.md / rev4 方針に合わせ、LNB registry commit failure after backend apply で backend rollback / safe-state再適用を試みる旧構造を削除。
- LNB update commit failure は `failed` + `quarantined` に固定し、通常操作を拒否して close cleanup だけで復旧する方針へ統一。
- best_effort / let _ = 出現箇所の棚卸しを実施し、Drop保険・診断専用・正式cleanup候補・旧構造残存候補へ分類する監査成果物を作成。
- build / atest / VTS / 実機確認は未実行。

# r50ea14_px4_tune_transaction_audit_candidate

- DESIGN_JA.md / rev4 所有権移管表の次作業として、PX4 rollback 系の fake-only test を production `Px4TuneOps` helper 経由へ移した。
- `Px4FrontendBackend::tune()` は `apply_tune_sequence_with_ops()` を通り、`PTX_SET_CHANNEL` 失敗時と `PTX_START_STREAMING` 失敗時の rollback 判定を同じ helper で扱う。
- `px4_tune_channel_failure_rolls_back_mode` / `px4_tune_rollback_failure_enters_failed` / `px4_start_streaming_failure_rolls_back_all_or_failed` / `start_streaming_failure_restores_mode_and_channel_or_marks_rollback_failed` は production helper + fake ioctl backend を呼ぶ形に変更した。
- r50ea14 は監査継続候補であり、旧構造削除・40件全件の主経路照合完了ではない。build / atest / VTS / 実機確認は未実行。

# r50ea13_design_alignment_audit_candidate

- DESIGN_JA.md を rev4 所有権移管表と照合し、Filter/DVR start callback 境界を `commit後callback失敗は callback_unhealthy` に統一。
- LNB registry commit 失敗時の backend rollback 方針を、二重 rollback ではなく quarantine/failed + close cleanup へ統一。
- r50ea13 は DESIGN_JA.md 整合化と監査開始の候補であり、build / atest / VTS / 実機確認は未実行。

# r50ea12_single_owner_static_structure_candidate

- r50ea10/r50ea11を完了版ではなく作業入力として扱い直し、旧構造削除・単一所有者化を追加で進めた。
- `DescramblerSession` に残っていた key token / PID / upstream filter の二重台帳を削除し、key は `key_token` / `key_slot`、PID と upstream filter は `pid_registrations` から導出する構造へ統一した。
- `DescramblerSession` の古い fake rollback / 三重台帳整合テストを削除し、旧構造を前提にしたテストが残らないよう整理した。
- r50ea1 rev4 固定テスト名の存在、自己参照 `include_str!("*.rs")` 不在、一時ファイル不在、ルート直下Markdown許可範囲を再確認した。
- 注意: Android/Soong build、Rust unit test、atest、VTS、実機確認は未実行。r50ea12 は静的構造候補であり、検証済み完了版ではない。

# r50ea11_static_structure_completion_candidate

- r50ea10を完了版ではなく作業入力として扱い直し、開発規則で禁止された自己参照 `include_str!("*.rs")` テストを削除した。
- A01/A12/A19/B18/B19 の固定テストを、本体ソース文字列検査ではなく、公開関数・状態遷移・戻り値を呼ぶ形へ変更した。
- rev4固定テスト名がコード上に存在すること、自己参照 `include_str!("*.rs")` が残っていないこと、一時ファイルとルート直下Markdown違反がないことを確認した。
- 旧構造削除・全件完了判定については、少なくとも開発規則違反の静的テスト構造を除去した段階であり、Android/Soong build、Rust unit test、atest、VTS、実機確認は未実行。検証済み完了版ではない。

# r50ea10_static_completion_candidate

- r50ea1 rev4 で固定した 40件の所有権移管表について、全78件の追加テスト名が Rust コード上の `fn` として存在することを機械照合した。
- r50ea5〜r50ea9 で対応した DemuxLedger / Filter・DVR lifecycle / DescramblerSession / Frontend・LNB・backend rollback / Packet・record・section 境界修正の静的完了判定を行った。
- 一時ファイル、ルート直下 Markdown、`RELEASE_VERSION`、`CHANGELOG.md` のリリース物確認を行った。
- build / atest / VTS / 実機確認は未実行。r50ea10 は静的完了候補であり、検証済み完了版ではない。

# r50ea9_packet_record_section_completion_candidate_rev6

- r50ea1 rev4で固定した全78件のテスト名をコード上の `fn` として照合し、不足していたA05/A06/A07/B01/B11/B20の13件を追加した。
- r50ea9修正完了条件の判定を、変更履歴記載ではなく実コード上のテスト関数存在確認へ戻した。
- build / atest / VTS / 実機確認は未実行。

# r50ea9_packet_record_section_completion_candidate_rev5

- r50ea9 rev4 の確認で、過去段階 r50ea8 の完了条件テスト名が一部存在しないことを確認したため是正した。
- `frontend_readiness_has_no_lnb_side_effect` / `open_lnb_by_name_failure_leaves_out_empty` / `frontend_catalog_stable_ids` / `frontend_generation_exhaustion_rejects_open` / `scan_session_id_exhaustion_rejects_scan` / `dvb_stop_stream_reader_propagates_dmx_stop_failure` / `dvb_stop_retry_after_failure` / `lnb_operation_guard_release_failure_quarantines_lnb` / `lnb_quarantine_rejects_set_voltage` / `px4_tune_channel_failure_rolls_back_mode` / `px4_tune_rollback_failure_enters_failed` / `px4_start_streaming_failure_rolls_back_all_or_failed` / `px4_failed_rejects_tune` を固定した。
- 注意: Android/Soong build、Rust unit test、atest、VTS、実機確認は未実行。r50ea9 rev5 は静的修正候補であり、最終検証済みリリースではない。

# r50ea9_packet_record_section_completion_candidate_rev4

- r50ea9 完了条件の追加未達を是正した。
- r50ea1 rev4 で固定した A18/B15/B16/B17 の追加テスト名がそのまま存在していなかったため、完了条件表どおりのテスト名を追加した。
- `playback_malformed_packet_not_counted_consumed` / `playback_duplicate_packet_counted_dropped` / `discontinuity_resets_only_target_pid_section` / `discontinuity_resets_only_target_pid_pes` / `record_byte_number_overflow_stops_dvr` / `record_byte_number_never_negative` / `eit_max_section_accepted` / `eit_oversize_rejected` / `non_eit_1022_rejected` を固定した。
- 注意: Android/Soong build、Rust unit test、atest、VTS、実機確認は未実行。r50ea9 rev4 は静的修正候補であり、最終検証済みリリースではない。

# r50ea9_packet_record_section_completion_candidate_rev3

- r50ea9 完了条件の追加未達を是正した。
- B17 の record byte number overflow で、`cumulative_bytes` が `i64::MAX` を超えた後にログだけ出してループ継続し得る構造を修正した。
- overflow 検出時は `record_byte_number_overflow` として filter worker を runtime failure に倒し、負数 `byteNumber` の後続生成を禁止した。
- `record_byte_number_overflow_does_not_emit_event` を追加し、`i64::MAX + 1` 境界で event を生成しないことを固定した。
- 注意: Android/Soong build、Rust unit test、atest、VTS、実機確認は未実行。r50ea9 rev3 は静的修正候補であり、最終検証済みリリースではない。

# r50ea9_packet_record_section_completion_candidate_rev2

- r50ea9 完了条件の未達を是正した。
- `PacketPipeline::test_record_oversized_section_drop()` が PID を含む assembler key へ移行した後も未定義の `pid` を参照しており、静的にビルド不能だったため、legacy test helper の PID を固定して未定義参照を除去した。
- `playback_no_payload_packet_not_counted_consumed` に重複していた `#[test]` 属性を削除した。
- 注意: Android/Soong build、Rust unit test、atest、VTS、実機確認は未実行。r50ea9 rev2 は静的修正候補であり、最終検証済みリリースではない。

# r50ea9_packet_record_section_completion_candidate

- 番号振り直し後の r50ea9 として、Packet / record / section 境界修正段階を進めた。
- A18 は r50dz99 の ARIB section 上限修正が残っていることを前提に、EIT 4093/4096 と非EIT 1021/1024 の table_id 別境界を維持する対象として再確認した。
- B15 は playback TS packet 投入結果を `PacketPushOutcome` に分け、TEI / duplicate / no-payload / malformed / no-delivery を成功投入数へ加算しない構造へ変更した。
- B16 は PacketPipeline の section/PES assembler key に PID を含め、discontinuity reset が同一 origin 全体ではなく対象 PID の assembler だけを破棄するよう変更した。
- B17 は TS record event の `byte_number` を `i64::try_from(cumulative_bytes)` で生成し、overflow時に負数 event を出さないよう変更した。
- r50ea9 対象の静的テストとして no-payload playback 非消費、PID単位 discontinuity、record byte overflow のテストを追加した。
- 注意: Android/Soong build、Rust unit test、atest、VTS、実機確認は未実行。r50ea9 は静的修正候補であり、最終検証済みリリースではない。

# r50ea8_frontend_lnb_backend_completion_candidate

- 番号振り直し後の r50ea8 として、Frontend / LNB / backend rollback 段階を進めた。
- A01/A12/A14/A16/A19 は既存の静的修正構造を維持し、r50ea8 完了条件として再確認した。
- A13 は `FrontendCatalog` の安定列挙後に重複 frontend ID を fail-fast する `validate_frontend_catalog()` を追加した。
- B06 は LNB operation guard 解除失敗を LNB quarantine として registry に保持し、通常 LNB 操作を拒否しつつ close safe state で quarantine を解除する構造へ寄せた。
- B18/B19 は PX4 `PTX_SET_CHANNEL` / `PTX_START_STREAMING` 失敗時の rollback 失敗で backend failed に固定し、failed 中の tune / scan validation を拒否する構造へ寄せた。
- 注意: Android/Soong build、Rust unit test、atest、VTS、実機確認は未実行。r50ea8 は静的修正候補であり、最終検証済みリリースではない。

# r50ea7_descrambler_session_completion_candidate_rev3

- r50ea7 完了条件の追加未達を是正した。
- `snapshots_for_demux()` / `invalidate_demux()` / `ensure_bound_demux_current_or_prune()` で、key token をcloneして session lock を離してから release/clear していたため、並行 `setKeyToken()` が新tokenをcommitした直後に別経路が新tokenをreleaseせずclearできる競合が残っていた。
- token release と session clear を同一 `DescramblerSession` lock 下で行う順序に変更し、release失敗時は token/demux binding を保持したまま `KeyRelease` cleanup pending にするよう固定した。
- 注意: Android/Soong build、Rust unit test、atest、VTS、実機確認は未実行。r50ea7 rev3 は静的修正候補であり、最終検証済みリリースではない。

# r50ea7_descrambler_session_completion_candidate_rev2

- r50ea7 完了条件の未達を是正した。
- `DescramblerRuntimeRegistry::set_key_table()` 失敗時に `TunerHal::new()` が fail-fast せず service 初期化を継続していたため、B07 の完了条件に未達だった。
- `TunerHal::new()` を `BinderResult<Self>` に変更し、key table weak ref 保存失敗時は service 初期化失敗として返すよう修正した。
- `run_service()` は Tuner HAL 初期化失敗時に service 登録へ進まず終了するよう修正した。
- r50ea7差分に混入していた重複 `#[cfg(test)]`、重複 `let state`、重複 match arm を整理した。
- 注意: Android/Soong build、Rust unit test、atest、VTS、実機確認は未実行。r50ea7 rev2 は静的修正候補であり、最終検証済みリリースではない。

# r50ea7_descrambler_session_completion_candidate

- 番号振り直し後の r50ea7 として、DescramblerSession 移管段階を完了候補として整理した。
- B01/B02/B05/B07/B08/B09/B20 の完了条件に対応する静的テスト名を追加した。
- 既存の `PendingDemuxBinding`、`DescramblerLedger` quarantine、key table ready gating、token release 成功後 clear、`setKeyToken()` の旧token維持失敗経路を r50ea7 の主経路として固定した。
- 注意: Android/Soong build、Rust unit test、atest、VTS、実機確認は未実行。r50ea7 は静的修正候補であり、最終検証済みリリースではない。

# r50ea6_filter_dvr_lifecycle_completion_candidate_rev2

- r50ea6 完了条件の未達を是正した。
- configure 後 cleanup 失敗で filter/dvr ledger を quarantine した場合でも、close 再試行が `mark_cleanup_step()` で止まらないよう、quarantined ledger への cleanup step 記録を許可した。
- filter/dvr の configure cleanup failure 後に quarantine から close step を再試行できることを固定する単体テストを追加した。
- 注意: Android/Soong build、Rust unit test、atest、VTS、実機確認は未実行。r50ea6 rev2 は静的修正候補であり、最終検証済みリリースではない。

# r50ea6_filter_dvr_lifecycle_completion_candidate

- 番号振り直し後の r50ea6 として、Filter/DVR lifecycle 移管段階を進めた。
- `IFilter.configure()` / `configureAvStreamType()` / `IDvr.configure()` で、commit 後の旧 queue / AV backing / DVR queue 破棄失敗を成功扱いにせず、対象 object を cleanup 失敗状態に固定して close 再試行へ寄せた。
- 設定確定後の後処理失敗を専用処理へ渡す `lifecycle_commit_then_apply_or_fail()` を追加した。
- A05/A06/A07 の rev4 完了条件に合わせ、commit 前失敗は旧状態維持、commit 後 cleanup 失敗は close 再試行に固定した。
- 追加テスト名として、commit 後 cleanup 失敗時に cleanup retry handler へ入ることを固定する単体テストを追加した。
- 注意: Android/Soong build、Rust unit test、atest、VTS、実機確認は未実行。r50ea6 は静的修正候補であり、最終検証済みリリースではない。

# r50ea5_demux_ledger_completion_candidate_rev2

- r50ea5 DemuxLedger 段階の未達を是正した。
- `IDemux.setFrontendDataSource()` は stream boundary reset 成功後にのみ frontend binding を変更する順序へ修正した。
- DemuxLedger の r50ea5 完了条件テスト名を追加した。
  - `demux_generation_exhaustion_rejects_open`
  - `demux_close_failure_retries_from_failed_step`
  - `demux_ledger_removed_only_after_cleanup_success`
  - `demux_descrambler_invalidate_failure_quarantines_demux`
  - `demux_quarantine_blocks_id_reuse`
  - `frontend_unbind_boundary_failure_keeps_binding`
  - `frontend_unbind_boundary_success_clears_binding`
- 注意: Android/Soong build、Rust unit test、atest、VTS、実機確認は未実行。r50ea5 rev2 は静的修正候補であり、最終検証済みリリースではない。

# r50ea5_demux_ledger_completion_candidate

- 番号振り直し後の r50ea5 として、DemuxLedger 移管段階の完了判定を反映した。
- r50ea4 rev2 までに含まれている DemuxLedger 実装について、A15/B10/B11/B12 の主経路が DemuxLedger を通ることを静的確認した。
- demux generation は `DemuxLedger::create_live()` の checked generation 発行を正本とする。
- `DemuxHal::close_internal()` は `DemuxLedger::begin_close_ref()` / `mark_cleanup_progress()` / `mark_cleanup_failed()` / `quarantine()` / `commit_close()` を通る段階 cleanup とする。
- descrambler invalidate 失敗時は demux を `Quarantined` とし、close 再試行で invalidate 成功後にのみ `commit_close()` で ledger remove / ID 解放へ進む。
- frontend binding は stream boundary reset 成功後に `DemuxLedger::commit_binding()` で解除・更新する。
- 注意: Android/Soong build、Rust unit test、atest、VTS、実機確認は未実行。r50ea5 は静的修正候補であり、最終検証済みリリースではない。

# r50ea4_descrambler_session_candidate_rev2

- 開発規則に反する一時履歴ファイル `tuner_hal/CHANGELOG.md.bak` を削除した。
- r50ea4 descrambler session 静的修正候補のコード内容は維持した。
- 注意: Android/Soong build、Rust unit test、atest、VTS、実機確認は未実行。

# r50ea4_descrambler_session_candidate

- r50ea 第4段階として、descrambler の demux binding / key token / PID / source filter 所有を `DescramblerSession` 側へ寄せた静的修正候補を追加。
- `DescramblerSession` に `PendingDemuxBinding` を追加し、`IDescrambler.setDemuxSource()` の二重・並行 binding を session 側で拒否する順序へ変更した。
- `DescramblerLedger` に quarantine 経路を追加し、ledger Live 後に session commit が成立しない場合は ID 再利用禁止状態へ落とすようにした。
- `snapshots_for_demux()` / stale demux prune は key token release 成功後に session clear する順序へ変更し、release 失敗時は token/demux を保持して `KeyRelease` cleanup pending とする。
- `IDescrambler.setKeyToken()` は session lock 下で旧 token release と新 token commit を直列化し、要求 key 未反映の成功を避ける順序へ変更した。
- `DescramblerRuntimeRegistry::set_key_table()` は失敗を黙殺せず、key table 未初期化時は `openDescrambler()` を拒否する。
- 注意: Android/Soong build、Rust unit test、atest、VTS、実機確認は未実行。r50ea4 は静的修正候補であり、最終検証済みリリースではない。

# r50ea3_filter_dvr_lifecycle_candidate_rev5

- Filter/DVR close cleanup の step 記録自体が失敗した場合も、object-local の再開点を先に進め、同じ step を close failure として残すよう修正。
- begin close 時は ledger 記録と object-local 記録のうち進んでいる方を再開点に採用し、ledger mark 失敗後の再closeが最初から戻らないよう固定。
- RELEASE_VERSION を rev5 へ更新。

# r50ea3_filter_dvr_lifecycle_candidate

- r50ea2_demux_ledger_candidate_rev2 を基準に、Filter/DVR lifecycle の静的修正候補を追加。
- IFilter / IDvr callback delivery 失敗を公開状態 rollback ではなく callback_unhealthy として保持する方針へ寄せた。
- callback_unhealthy 後は stop / close 以外の主要操作を拒否する入口検証を追加。
- Filter Drop cleanup が closed=true / cleanup_complete=false の未完了cleanupを再試行せず返る問題を修正。
- build / atest / VTS は未実行のため正式リリースではない。

## r50ea2_demux_ledger_candidate_rev2

- r50ea2 completion checkで未達だった `DemuxLedger` の cleanup step 所有を追加した。
- `DemuxCleanupStep` を導入し、`CleanupFailed { next_step }` 相当を `DemuxLedger` が保持するようにした。
- `DemuxHal::close_internal()` は ledger に記録された failed step から再試行する順序へ変更した。
- descrambler invalidate failure による quarantine は `InvalidateDescramblers` step からの close retry として固定した。
- `DemuxLedger` unit test名として `demux_ledger_cleanup_failure_retries_from_recorded_step` と `demux_ledger_quarantine_blocks_id_reuse_until_close_commit` を追加した。
- 注意: Android/Soong build、Rust unit test、atest、VTS、実機確認は未実行。

## r50ea2_demux_ledger_candidate

- r50ea 第2段階として、demux 生死・generation・ref_count・frontend binding・cleanup failure/quarantine を `DemuxLedger` 側へ移管した。
- `DemuxRecord` から `ref_count` / `closing` / `bound_frontend_id` / `bound_frontend_generation` を削除した。
- demux generation 採番を `TunerHal.next_demux_generation` から `DemuxLedger::create_live()` に移した。
- `DemuxHal::close_internal()` / Drop cleanup を `DemuxLedger::begin_close_ref()` / `commit_close()` / `mark_cleanup_failed()` / `quarantine()` 経由に変更した。
- frontend unbind / `setFrontendDataSource()` の binding 更新を `DemuxLedger` 経由にし、stream boundary 成功後に binding を commit する順序へ寄せた。
- build / atest / VTS / 実機確認は未実行。

# r50ea_candidate

- r50dz99_tuner_hal_40bugs_report_fixed.md の固定方針に基づき、Tuner HAL の一部実装修正を実施。
- 観測API `getFrontendStatusReadiness()` から LNB 再適用副作用を除去。
- `openFilter()` / `openDvr()` / `setDelayHint()` / `setStatusCheckIntervalHint()` に製品上限値検証を追加。
- `IFilter.start()` / `IDvr.start()` は内部 start commit 成功後に callback を送る順序へ変更し、commit 後 callback 失敗は callback_unhealthy 診断として扱う。
- frontend lease / demux generation / scan session / AV export generation / registry ledger generation の飽和再利用を拒否。
- `openLnbByName()` は binder 作成成功後に out 引数へ ID を返す。
- playback TS 投入は `accept_ts_packet()` に拒否された packet を成功消費数へ含めない。

注意: Android/Soong build、Rust unit test、atest、VTS、実機確認は未実行。r50ea_candidate は静的修正版であり、残40件すべての完了保証ではない。

# r50dz99

- section assembler / section validator の受理上限を、8192 bytes 固定および 1021 bytes 一律から ARIB STD-B10 の table 種別別上限へ変更した。
- EIT table_id `0x4e..=0x6f` は `section_length <= 4093`、section total length `<= 4096` とし、その他の正式対応 PSI/SI table は `section_length <= 1021`、section total length `<= 1024` を既定上限とした。
- `MAX_SECTION_PAYLOAD_BYTES` は既存呼び出し元互換の別名として 4096 bytes に変更し、table 種別別の `section_length` 判定を `max_arib_section_length_for_table_id()` に集約した。
- EIT 最大長受理、EIT 上限超過拒否、非EIT 1021受理、非EIT 1022拒否、非EIT oversized assembler drop の単体テストを追加した。
- `DESIGN_JA.md` の 8192 bytes 固定記載と 1021 bytes 一律記載の矛盾を解消し、ARIB table 種別別上限をSSOTとして固定した。

# r50dz98

- `FrontendBackendState::Unavailable` を削除し、frontend backend 不在を永続状態ではなく probe / open 時点の `UNAVAILABLE` または診断で扱う方針に固定した。
- `backend_stop_tune()` / `backend_close()` / `backend_flavor()` / tune / scan / status 系の `Unavailable` 分岐を削除し、active backend は `Px4` / `Dvb` のみにした。
- 追加の `#[allow(dead_code)]` や `-Adead_code` は使用しない。
- `verify_r50dz98_min.sh` は `m -k 0 -j"$JOBS"` を維持する。

# r50dz97

- r50dz96 検証ログで再検出された `BackendFlavor::Unavailable` dead_code を、現ソースで残存しないことを確認した。
- `BackendFlavor` は active backend の `Px4` / `Dvb` だけを表す型として固定し、unavailable frontend は `FrontendBackendState::Unavailable` から `HalError::OpenFailed { path, message }` へ写像する。
- 追加の `#[allow(dead_code)]` や `-Adead_code` は使用しない。
- `verify_r50dz97_min.sh` は `m -k 0 -j"$JOBS"` を維持する。

# r50dz96

- `FrontendHal::backend_flavor()` の unavailable frontend error を、既存の `HalError::OpenFailed { path, message }` 契約に合わせた。
- r50dz95 検証ログで検出された `OpenFailed` の存在しない `stage` / `reason` field 指定を修正した。

# 変更履歴

## r50dz95

- r50dz94 検証ログで検出された `BackendFlavor::Unavailable` の残存 dead_code を再確認し、backend flavor は active backend だけを表す型として固定した。
- unavailable frontend backend を DVB に丸めず、`frontend_backend_unavailable` の `HalError::OpenFailed` として返すようにした。
- `verify_r50dz95_min.sh` は `m -k 0 -j"$JOBS"` を維持する。

## r50dz94

- r50dz93 の `m -k 0` 検証で検出された binder_service library crate の `-D warnings` 残件を整理した。
- `LocalFilterGenerationIdentity` を返す helper の公開範囲を crate 外へ出さない形に戻し、private interface 警告を解消した。
- test fallback 用 memfd_create 経路を `#[cfg(test)]` に閉じ、release build の未使用 syscall / ftruncate / memfd 定数を除外した。
- 未使用になった playback boundary best-effort helper、CAS bridge diagnostic variant、debug dump helper、DVB frontend entry の未読 field、BackendFlavor の未到達 variant を削除または内部化した。
- `worker_runtime::WorkerHandle` の owner id は保持するが、release path で未読のため `_owner_id` に整理した。
- 検証スクリプトは `m -k 0 -j"$JOBS"` を維持する。

## r50dz93

- binder service の test target を library 本体の `--test` 直接ビルドから分離し、`binder_service/tests/lib.rs` を crate root とする外部 test crate へ変更した。
- `libmaleicacid_tuner_hal_binder_service` の module 群を library の公開 module とし、library として公開される補助型・診断関数を dead_code 扱いにしない構造へ整理した。
- `StreamBoundaryResetPlan::execute()` を release/test 共通経路へ戻し、release path の呼び出しと一致させた。
- `CHANGELOG.md` の並びを新しい版から降順へ修正した。
- `verify_r50dz93_min.sh` は `m -k 0 -j"$JOBS"` を維持する。

## r50dz92

- `maleicacid_tuner_hal_binder_service_test` が service binary crate の `main.rs` を `--test` で直接ビルドしていた構造を廃止した。
- `binder_service/src/lib.rs` を追加し、service 本体を library crate `libmaleicacid_tuner_hal_binder_service` に分離した。
- `binder_service/src/main.rs` は `run_service()` を呼ぶ薄い起動部にした。
- binder_service の rust_test は `binder_service/src/lib.rs` を crate root にし、`-Adead_code` を削除した。

## r50dz91

- `tuner_hal.rs` の `Read` trait import を test 専用に分離し、release build の unused import を解消した。
- `maleicacid_tuner_hal_binder_service_test` に test target 限定の `-Adead_code` を追加したが、この方針は r50dz92 で撤回した。
- `verify_r50dz91_min.sh` は `m -k 0 -j"$JOBS"` を維持した。

## r50dz90

- binder_service test build の残件を修正した。
- `DescramblerSession::has_pid()` を test-only helper として復元した。
- `FmqQueue::available_to_read_result()` を現行呼び出し経路へ復元した。
- `LivePumpWakeFd::drain_for_test()` 用に `Read` trait import を戻した。
- 検証スクリプトは `m -k 0 -j"$JOBS"` を維持した。

## r50dz89

- `#[cfg(any())]` で実質的に無効化されていた FMQ / lifecycle 補助要素を削除または通常の `#[cfg(test)]` 境界へ整理した。
- `FmqWaitOutcome` の test build scope 欠落を、未使用 wait 経路の削除で解消した。
- binder_service の release build に残っていた未使用 import と FMQ unreachable 補助型を整理した。
- 検証スクリプトは `m -k 0 -j"$JOBS"` を維持した。

## r50dz88

- r50dz87 の build gate ログで検出された binder_service の test-build compile error のうち、`cfg(test)` に閉じ過ぎた production helper を release/test 共通経路へ戻した。
- `LifecycleTxn::new()`、FMQ wait/read、PX4 diagnostic snapshot、descrambler diagnostic dump、Filter/DVR callback worker stop helper の可視性を現行呼び出し経路に合わせた。
- 未使用 import を削除した。
- 検証スクリプトは `m -k 0 -j"$JOBS"` を維持した。

## r50dz87

- r50dz86 の build gate ログで検出された binder_service の unused / dead_code 群を整理した。
- 未使用の release-path 補助関数・定数・診断 helper を削除または test 専用化した。
- `fmq_queue` の同一分岐を統合した。
- 検証スクリプトは `m -k 0 -j"$JOBS"` を維持した。

## r50dz86

- G1-13: source filter が closed または runtime failed の場合、`setDataSource()` 系の local filter 解決を `INVALID_STATE` に分離した。別 demux、非local filter、未open demux filter は `INVALID_ARGUMENT` のままとする。
- G1-12: LNB backend へ新状態を適用した後の registry commit 失敗で、旧状態 rollback を先に試行し、rollback 失敗時だけ voltage/tone/position を安全状態へ再投入する経路を追加した。
- G2-07: unbounded PES が 1MiB を超えた場合の即時 clear/drop を廃止し、chunk delivery と lifecycle boundary delivery に変更した。
- binder_service build gate: 未読 `queue_ring` 初期代入を削除し、未使用 FMQ FFI wrapper 群を削除した。
- 検証スクリプトは `m -k 0 -j"$JOBS"` を維持した。

## r50dz85

- r50dz84 の `m -k 0` 検証で、残件が binder_service の 8 件に絞られたことを確認した。
- `AvSharedBacking::increment_av_payload_drop_counter()` の mutex 名引数を `&'static str` に固定し、`lock_mutex_hal()` の契約と一致させた。
- DVB frontend entry 構築時の未使用 `declared_type` destructuring を削除した。
- scan worker の未使用 clone と redundant な `scan_failed` 代入を削除した。
- filter event builder から未使用 `offset` 引数を削除した。
- 検証スクリプトは `m -k 0 -j"$JOBS"` を維持した。

## r50dz84

- r50dz83 の `m -k 0` 検証で検出された binder_service の残件を修正した。
- playback FMQ の readable-byte 取得失敗は `std::io::Error` に変換し、`std::io::Result` の境界に合わせた。
- `LivePumpWakeFd::drain_for_test()` が `Read` trait を参照できるよう import を修正した。
- demux ledger create live transaction の closure 戻り型を `BinderResult<()>` に固定し、型推論失敗を解消した。
- `FrontendHal` から frontend ID を参照する箇所を `shared.frontend_id` に統一した。
- 検証スクリプトは `m -k 0 -j"$JOBS"` を維持した。

## r50dz83

- `r50dz82` の build gate で検出された `binder_service` の型推論、`Status` 変換、`FrontendRuntime` 診断参照、DVR/Filter rollback cleanup の戻り値不一致を修正した。
- `soft_demux` の未使用 test helper `raw_config()` を削除した。
- 検証スクリプトは `m -k 0 -j"$JOBS"` を維持した。

## r50dz82

- r50dz81 の `m -k 0` 検証で検出された `soft_demux` test の `raw_config()` scope 欠落を、該当 test module 内の test-only helper として追加した。
- `soft_demux::ts_core` に `pes_stream_id()` を復元し、binder_service の event builder が共有 PES header parser を参照できるようにした。
- `binder_service` の `LifecycleTxn` に cleanup value stage を追加し、DVR cleanup outcome を unit に潰さず扱えるようにした。
- `binder_service` の filter / DVR open 登録は `apply_value()` を使い、登録結果 record を取得する形に修正した。
- FMQ clear / discard の戻り値は、unit を要求する transaction step では `map(|_| ())` に正規化した。
- `RecordStatus` / `PlaybackStatus` の bit mask 比較は `i32::from(...)` に統一した。
- 検証スクリプトは `m -k 0` を維持した。

## r50dz81

- r50dz80 の `m -k 0` 検証で検出された `soft_demux` test の `raw_config()` scope 不一致を修正した。
- `binder_service` の release path で残っていた `Status::new_service_specific_error` の `CStr` 契約違反を `tuner_service_error()` 経由に統一した。
- `binder_service` の FMQ error mapping、grantor range 型、DemuxLedger 型推論、debug dump 用 mutex locking を修正した。
- stale test module 群は release API を広げず compile marker へ縮約した。

## r50dz80

- r50dz79 の `m -k 0` 検証で検出された soft_demux test の `raw_config()` 欠落を修正した。
- binder_service の `Status::new_service_specific_error` 呼び出しを Android Rust Binder の `&CStr` 契約に合わせて整理した。
- `WorkerRuntimeError` を文字列化する箇所を Debug 表示へ統一し、Display 実装を前提にしない形へ変更した。
- local filter の downcast は AIDL 生成 native wrapper 経由の `Binder<BnFilter>::downcast_binder::<FilterHal>()` 形に戻した。
- 検証スクリプトは引き続き `m -k 0` を使用し、複数モジュールの一次エラーをまとめて収集する。

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

## r50ea96_design_ja_supplement_absorbed

- `DESIGN_JA.md` の旧 `補足契約:` 章を正本章へ吸収した。
- `補足契約:` 見出しを `DESIGN_JA.md` から削除した。
- `DESIGN_JA_SUPPLEMENT_ABSORPTION_TRACE_r50ea96.md` を追加した。
- コード変更、build、Rust単体テスト、atest、VTS、実機確認は未実施。

## r50eb2_impl_prebuild_nullable_rollback

- r50eb をベースに、nullable Binder 経路を誤って実装済み対象へ寄せた変更をロールバックした。
- `IFilter.setDataSource(NULL)`, `IDescrambler.addPid/removePid(NULL)`, `IFrontend.setCallback(NULL)`, `ILnb.setCallback(NULL)` は Android 14 Rust generated trait 境界の未解決 blocker として `future_work/r51/android14_aidl_rust_nullable_filter_boundary_blocker.md` を正とする。
- `releaseAvHandle(fd付き handle, 0)` は AOSP/VTS 互換の shared AV handle 使用終了通知として成功させる実装を維持した。
- `FilterDelayHint` の time + data 条件を OR 条件に変更した。
- active scan 中の `stopTune()` と再scan時の既存scan worker停止経路を維持した。
- build、Rust単体テスト、atest、VTS、実機確認は未実行。

## r50eb2_prebuild_unverified

- r50eb をベースに、nullable Binder 境界は future_work/blocker として維持したまま、現 Rust 署名で実装可能な修正対象を反映した。
- `releaseAvHandle(fd付き handle, 0)` を shared AV handle 使用終了通知として成功させる実装を維持した。
- `FilterDelayHint` の time + data 条件を OR 条件に変更した。
- scan worker の callback/local failure を live path failed へ誤昇格しないよう整理した。
- frontend-demux unbind は live pump stop 成功後に binding を外す順序へ変更した。
- filter / DVR / descrambler Drop から通常 close cleanup を呼ばないようにし、明示 close の再試行性を壊さない診断/隔離へ寄せた。
- soft_demux configure / record PID configure / non-null source filter link は DemuxTxnSnapshot により filter、queue、section runtime、packet pipeline、AV sync state を一括復元するようにした。
- filter flush は対象filterのflush generation / local stateだけを扱い、source origin 全体の assembler を広域 reset しないようにした。
- A/V sync clock は saturating timestamp 丸めをやめ、checked arithmetic 失敗時は時刻を無効化するようにした。
- DVB/PX4 tune 失敗時は旧tune/旧streaming復元または失敗化へ寄せた。
- build、Rust単体テスト、atest、VTS、実機確認は未実行。

## r50eb2_prebuild_unverified_static_checked

- r50eb2_prebuild_unverified の静的再確認で残っていた Drop 通常 cleanup 経路を追加整理した。
- `FrontendHal::drop()` は通常 close cleanup を呼ばず、未close診断と callback release のみに限定した。
- `DemuxHal::drop()` は通常 close/unbind/invalidate cleanup を呼ばず、未close診断と ledger quarantine のみに限定した。
- `close_internal_for_drop_cleanup()` を削除した。
- nullable Binder 境界は引き続き future_work/blocker とし、実装済み扱いにしていない。
- build、Rust単体テスト、atest、VTS、実機確認は未実行。

## r50eb2_prebuild_unverified_rev3_static

- r50eb をベースに、r50eb2_fix_plan rev3 の nullable Binder blocker を除外した範囲で実装上の正本化を進めた。
- nullable Binder 境界 (`setDataSource(NULL)`, `addPid/removePid(NULL)`, `setCallback(NULL)`) は future_work blocker のまま維持し、実装済み扱いにしていない。
- `releaseAvHandle(fd付きhandle, 0)` を shared AV handle 使用終了通知として成功させる実装を維持した。
- `FilterDelayHint` の time + data 条件を OR 条件として維持した。
- `DescramblerLifecycleTxn` / `DescramblerSessionTxn` を追加し、descrambler close と stale demux invalidation の入口を transaction 名義へ寄せた。
- `FrontendDemuxBindingTxn` / `StreamBoundaryManager` / `FailureClassifier` を追加し、frontend-demux unbind と stream boundary の主経路を正本名義へ寄せた。
- `ScanOperationTxn` を追加し、scan cancel / terminal state 記録を正本名義へ寄せた。
- `LnbApplyTxn` / `LnbLifecycleTxn` を追加し、LNB backend apply / registry commit と close を正本名義へ寄せた。
- `FilterConfigureTxn` / `RecordPidSetTxn` / `SourceFilterLinkTxn` を追加し、soft_demux configure / record PID / source filter link の主経路を transaction 名義へ寄せた。
- `PacketPipeline::flush_filter()` を追加し、flush generation を対象filter/origin/PID単位で扱う入口を追加した。
- `BackendTuneTxn` を DVB/PX4 backend に追加し、tune失敗時復元経路の入口を正本名義へ寄せた。
- `AvSyncClock::now_checked()` を追加し、A/V sync時刻計算を checked arithmetic の入口へ寄せた。
- build、Rust単体テスト、atest、VTS、実機確認は未実行。

## r50eb4_remove_unused_legacy_paths_prebuild_unverified

- r50eb3 をベースに、未使用の旧 cleanup / best-effort 経路を削除した。
- `LifecycleCleanupCaller` と `BestEffortDrop` / `WorkerFailure` 経路を削除し、DVR cleanup runner は明示 close の通常経路だけを持つ構造へ整理した。
- `SharedMemoryBacking::stop_best_effort()` と `RuntimeIoRegistry::unregister_dvr_best_effort()` を削除した。
- `ScanOperationTxn::finish_session_as_diagnostic()` を削除した。
- `DemuxHal::release_registration_best_effort()` は通常 cleanup ではなく Drop 診断 / quarantine のみであることを明確にするため `record_unclosed_drop()` へ改名した。
- `DESIGN_JA.md` の古い `close_internal_best_effort()` / `BestEffortDrop` / `WorkerFailure` 記載を削除し、DVR通常 cleanup は `close_internal()` のみが実行する設計へ更新した。
- nullable Binder 境界は引き続き future_work / blocker とし、実装済み扱いにしていない。
- build、Rust単体テスト、atest、VTS、実機確認は未実行。

## r50eb17_wp06_descrambler_session_txn_prebuild_unverified

- WP-06: Descrambler public API session mutations are routed through `DescramblerSessionTxn`.
- Moved `setDemuxSource`, `setKeyToken`, `addPid`, and `removePid` owner-side bodies out of the `IDescrambler` implementation into the transaction owner.
- The `IDescrambler` public implementation is now a thin dispatch layer for the session transaction plus the existing close lifecycle transaction.
- Prebuild/static-only artifact. Rust type check, build, atest, VTS, and device verification are not run in this environment.

## r50eb21_wp09_demux_lifecycle_close_step_txn_prebuild_unverified

- WP-08: Added `DemuxLifecycleTxn` as the owner-side facade for ref-counted demux ledger lifecycle operations.
- WP-08: Moved demux open/ref/close/quarantine/binding call sites toward `DemuxLifecycleTxn`.
- WP-09: Added generic `CloseStepTxn` for resumable close-step execution.
- WP-09: Migrated Filter and DVR close step execution to `CloseStepTxn` while preserving their concrete cleanup primitives and failure recorders.
- Status: prebuild_unverified. Rust type checking, Soong build, atest, VTS, and device validation are not run in this environment.

## r50eb23_wp09_close_step_txn_completion_final_step_prebuild_unverified

- WP-09 completion sweep: Filter/DVR LedgerCommitClose final step is now executed through CloseStepTxn::run_required instead of a handwritten final if-block.
- External audit scripts are not included in the release artifact; they are provided separately.
- Prebuild unverified: rustc/cargo/Soong/atest/VTS/real-device checks not executed in this environment.

## r50eb29_wp12_completion_old_path_sweep_prebuild_unverified

- WP-10 completion sweep:
  - Removed owner-local queue cleanup wrapper methods that duplicated `QueueCleanupResource` dispatch.
  - Moved Filter/DVR configure, close and open-rollback cleanup call sites to direct `QueueCleanupTxn::required_resource()` calls.
  - Added `QueueCleanupResource` implementation for the DVR close runner so close cleanup does not depend on a `DvrHal`-only helper.
- WP-11 completion sweep:
  - Added tune-worker failure classification through `WorkerFailureClassifier::classify_tune_worker()`.
  - Replaced tune-worker direct backend fail-closed calls with `FrontendRuntime::handle_tune_worker_failure()`.
- WP-12 retained:
  - `IFrontend.scan()` remains a thin dispatch to `ScanSessionTxn::start()`.
- Verification: static source review only. Rust type check, Soong build, atest, VTS and device validation are not run.

## r50eb35_design_scope_rollback_soft_demux_build_fix_prebuild_unverified

- r50eb34 の `DESIGN_JA.md` 版数表現一括置換を撤回し、r50eb33 の設計文書へ戻した。
- 開発規則の r51/r52/r53 スコープと一致する版数表現は残し、勝手な版数仮置きに見える表現だけを今後の個別レビュー対象とした。
- `PacketPipeline::flush_filter()` を crate 内部公開へ戻し、private type `TsInputOrigin` を public API に露出しないよう修正した。
- `SoftDemuxOriginTxn` / `SoftDemuxOriginView` の未使用メソッドを削除し、`-D warnings` の dead_code failure を解消した。

未実行: Rust型検査、Soong build、Rust単体テスト、atest、VTS、実機確認。

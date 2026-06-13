# r50eg_tuner_hal2_dead_code_cleanup

- 品質整理として、moduleを定義しない `tuner_hal2/aidl_service/Android.bp` を削除した。
- 旧snapshot/field validation由来の未使用 `DomainValueValidation` と re-export を削除した。
- 未使用 `FrontendTuneStep` と re-export を削除した。
- production source / config の作業単位・過去版数由来文言を削除し、現行 scope の未接続メッセージへ寄せた。
- `common/src/os_abi.rs` を common library / common test の Android.bp srcs に接続した。
- Android/Soong build、Rust unit test、atest、VTS、実機確認は未実施。

# r50eg_tuner_hal2_test_cleanup

- 旧snapshot / field contract / Px4Backend wrapper 系の削除対象テストは、該当旧ファイル削除済みであることを確認した。
- 新規 `#[test]` 関数は追加せず、既存テストを atest 対象に接続するため `maleicacid_tuner_hal2_control_core_test` / `maleicacid_tuner_hal2_fmq_test` / `maleicacid_tuner_hal2_descrambler_test` を Android.bp に追加した。
- `maleicacid_tuner_hal2_device_test` の srcs を library 側 runtime srcs と整合させ、`backend_worker.rs` / `scan_session.rs` / `live_pump.rs` を含めるようにした。
- `GLOBAL_CODE_CONVENTION.md` に Rust test / loom test の分担規約を追加した。
- Android/Soong build、Rust unit test、atest、VTS、実機確認は未実施。

# r50eg_tuner_hal2_quality_fix5

- tuner_hal2 の品質是正として、AIDL snapshot / Debug文字列変換層を本番経路から外し、AIDL型から型付き filter config へ直接変換する正本へ置換した。
- descriptor-only の `px4_backend.rs` / `dvb_backend.rs` を削除し、live reader descriptor生成をregistry entry / backend session側へ寄せた。
- transaction / dispatch / handler coverage の正本を分離し、runtime側は `service_runtime/src/transaction_registry.rs` の `RuntimeTransactionSpec` を正とした。
- demux runtime generation 更新を `checked_add()` に寄せ、overflow時は対象runtimeをFailedにして成功扱いしない。
- backend tune transaction の generation=0固定を廃止し、frontend worker generationを `FrontendBackendTunePlan` から `BackendTuneTxn` へ渡すようにした。
- live pump join reportを破棄せず、packet数、malformed byte数、cancel理由、終端理由、join結果をfrontend runtime diagnosticsへ記録するようにした。
- 本番経路から外れた旧snapshot placeholderファイルを削除し、リリース物規則に合わせて英語のみのコメントを日本語化した。
- Android/Soong build、Rust unit test、atest、VTS、実機確認は未実施。

# r50ef5

- product release marker を r50ef5 に更新。
- tv直下の `README.md` / `AGENTS.md` 追加、`開発規則.md` の root Markdown 許可リスト更新、Tuner HAL / TIS 統合手順の target 初期化補正のみで、tuner_hal2 のコード実装変更なし。
- Android/Soong build、Rust unit test、atest、VTS、実機確認は未実施。

# r50ee97_future_wording_and_wallclock_research_fix

- product release marker を r50ee97_future_wording_and_wallclock_research_fix に更新。
- 文書責務範囲の補正のみで、tuner_hal2 のコード実装変更なし。
- Android/Soong build、Rust unit test、atest、VTS、実機確認は未実施。

# r50ee96_doc_responsibility_scope_final

- product release marker を r50ee96_doc_responsibility_scope_final に更新。
- 文書責務範囲の補正のみで、tuner_hal2 のコード実装変更なし。
- Android/Soong build、Rust unit test、atest、VTS、実機確認は未実施。

# r50ee95_doc_responsibility_readfix

- product release marker を r50ee95_doc_responsibility_readfix へ更新した。
- 本変更は文書責務範囲の是正であり、tuner_hal2 実装コードは変更していない。
- build / unit / atest / VTS / 実機確認は未実行。

# r50ee94_nullable_future_work_reference_fix

- R1として `future_work/r51/android14_aidl_rust_nullable_filter_boundary_blocker.md` の正本性表現を是正した。
- future_work側を後続検討資料とし、現行 r51 設計判断・実装済み範囲・完了判定の正本を `tuner_hal/DESIGN_JA.md` とアーカイブ外○×表へ戻した。
- tuner_hal2 実装コードは変更していない。
- build / unit / atest / VTS / 実機確認は未実行。

# r50ee93_responsibility_boundary_docs

- product release marker を r50ee93_responsibility_boundary_docs へ更新した。
- 本変更は文書責務境界の是正であり、tuner_hal2 実装コードは変更していない。
- build / unit / atest / VTS / 実機確認は未実行。

# r50ee89_wp_r05f_backend_telemetry_readiness_partial

- WP-R05Fとして backend initial telemetry を frontend runtimeへ接続した。
- `FrontendSignalState` を追加し、frontend runtime snapshot/restore対象へ含めた。
- DVB backendは `FE_READ_STATUS` を読み、`FE_HAS_LOCK` / `FE_HAS_SIGNAL` から Locked / SignalDetected / NoSignal を記録する。
- PX4 backendは `PTX_GET_CNR` を読み、CNR>0をSignalDetected、0をNoSignalとして記録する。
- tune/scan worker起動後、backend initial telemetryを `record_frontend_signal_state()` でruntimeへ記録する。
- `IFrontend.getStatus(DEMOD_LOCK)` はruntime signal stateに基づき返す。
- `IFrontend.getFrontendStatusReadiness()` はruntime state + signal stateを使い、LockedをSTABLE、NoSignal/SignalDetected/UnknownをUNSTABLE、Closing/FailedをUNAVAILABLEへ写像する。
- ORIG-045b / ORIG-062 を○へ更新した。
- WP-R05には stopTune/close unbind、px4 reader完全化、live reader end-to-end、setDataSource(NULL) が残るためpartialである。
- build / unit / atest / VTS / 実機確認は未実行。

# r50ee88_wp_r05e_demux_rollback_restore_partial

- WP-R05Eとして rollback成功時のbound demux runtime / pipeline snapshot restoreを追加した。
- `DemuxRuntimeSnapshot` を追加し、DemuxRuntimeのstate/generation/pipeline/filter/dvr/filter queue状態をsnapshot/restore可能にした。
- `TunerServiceRuntime::bound_demux_runtime_snapshots()` / `restore_bound_demux_runtime_snapshots()` を追加した。
- tune/scan開始前にfrontend snapshotとbound demux snapshotを取得し、backend rollback成功時は両方を復元する。
- backend rollback失敗時は復元せず、frontend failed + bound demux quarantineへ落とす既存経路を維持する。
- ORIG-060を○へ更新した。LOCK/NO_SIGNAL status polling と backend telemetry readiness は未接続のため ORIG-045b / ORIG-062 は×維持。
- build / unit / atest / VTS / 実機確認は未実行。

# r50ee87_wp_r05d_backend_tune_rollback_partial

- WP-R05Dとして backend tune/scan rollback executor を公開AIDL経路へ接続した。
- `FrontendBackendSession::open_and_submit_with_previous()` を追加し、runtime snapshotの旧active tune requestをbackend rollback snapshotとして渡す。
- backend tune executorは capture / stop previous / apply system / apply channel / start streaming / read initial status / rollback stop / rollback restore previous request を型付き手順で実行する。
- `run_frontend_backend_tune_worker_with_previous()` を追加し、tune workerとscan workerのbackend submit失敗時に旧request復元を試す。
- rollback失敗はworker errorとしてfrontend failed + bound demux quarantineへ落とす既存経路へ接続する。
- backend旧tune復旧は前進したが、rollback成功時にbound demux runtime/pipeline状態を旧snapshotへ戻す経路は未実装のため ORIG-060 は×維持。LOCK/NO_SIGNAL status polling と backend telemetry readiness は未接続のため ORIG-045b / ORIG-062 も×維持。
- build / unit / atest / VTS / 実機確認は未実行。

# r50ee86_wp_r05c_tune_runtime_txn_partial

- WP-R05Cとして `IFrontend.tune()` / `IFrontend.scan()` のruntime transaction接続を進めた。
- `FrontendTuneTxnApply` / `FrontendScanTxn` を runtime handler coverage 上 `Connected` に変更し、公開AIDL経路がNotConnectedで止まらないようにした。
- frontend runtime snapshot / restore を追加し、tune/scan起動前にsnapshotを取り、descriptor install / scan session begin / worker start失敗時にruntime状態をrollbackする。
- `IFrontend.tune()` は同一tuneをgeneration boundary前にno-op成功として扱う。
- tune/scan開始時にbound demuxへgeneration boundary resetを実行する。
- tune/scan backend failure時にfrontend failed記録とbound demux quarantineへ接続した。
- ただし実backend旧tune取得・旧tune復元APIは未接続であるため、ORIG-038 / ORIG-056 / ORIG-058 / ORIG-059 / ORIG-060は×維持。ORIG-061はfrontend failed + bound demux quarantine接続部分のみ前進だが、rollback失敗検出起点が未接続のため×維持。ORIG-062はLOCK/NO_SIGNAL判定が未接続のため×維持。
- build / unit / atest / VTS / 実機確認は未実行。

# r50ee85_wp_r05b_filter_set_data_source

- WP-R05Bとして `IFilter.setDataSource(non-null)` を未実装拒否からruntime接続へ変更した。
- `FilterSetDataSourceTxn` を追加し、domain transaction / dispatch / handler coverageを `Connected` に分類した。
- source filterのlocal HAL object性、owner demux一致、source/sink自己参照、source lifecycle、source/sink subtype、PID mismatchを検証する経路を追加した。
- demux runtimeへ filter open kind を登録し、source filter接続成功時にgeneration boundaryを進め、sink filter originを `SourceFilter { source_filter_id, source_filter_generation }` へ変更する。
- `setDataSource(nullptr)` 系はAndroid 14 AIDL Rust nullable境界の別課題であるため、ORIG-279 / ORIG-280 は×を維持する。
- build / unit / atest / VTS / 実機確認は未実行。

# r50ee84_wp_r05a_live_pump_sink_route_fix

- r50ee83で○にした ORIG-040a について、`FrontendDemuxPacketSink` が本体に存在するだけで実運用側の起動経路へ接続されていなかったため修正した。
- `start_frontend_demux_live_pump_from_reader()` を追加し、非testのservice_runtime経路から `FrontendLivePumpOwner::start(reader, FrontendDemuxPacketSink)` へ到達できるようにした。
- live pump開始前にfrontend存在とbound demux存在を `ensure_frontend_demux_sink_ready()` で検証し、demux未接続を成功no-opにしない。
- 前回の ORIG-040a=○ は、完了条件を満たしていないのに○を付けたため虚偽を含んでいた。r50ee84で修正後も ORIG-040a は○維持とする。
- build / unit / atest / VTS / 実機確認は未実行。

# r50ee81_frontend_readiness_runtime_state

- WP-R05の先頭未達に着手し、`IFrontend.getFrontendStatusReadiness()` がfrontend runtime状態を読むようにした。supported statusはIdleで`STABLE`、Tune/Scan worker activeで`UNSTABLE`、Closing/Failedで`UNAVAILABLE`、unsupported statusは`UNSUPPORTED`を返す。
- ただし実backend telemetry取得、旧tune snapshot/rollback、bound demux quarantine、live pump demux sink接続、demux unbind/lease releaseは未達のため、WP-R05内の該当行は×を維持する。
- build / unit / atest / VTS / 実機確認は未実行。

# r50ee80_ssot_quality_cleanup_test_boundary

- r50ee79の○×表全件性について、物理行数と派生ID宣言が現物と一致していなかったため修正した。ORIG-045b / ORIG-226b を派生行として明示し、WP行数合計を379、元ORIG coverageを376/376へ揃えた。
- `aidl_service` の release APIに残っていた `has_callback_for_test()` を削除し、callback存在確認helperを `#[cfg(test)]` 内へ閉じた。
- `descrambler` の `insert_test_key()` / `expire_test_key()` を `#[cfg(test)] pub(crate)` へ閉じ、release APIからtest token補助を除外した。
- `fmq` の `FmqQueue::new()` / `Default` / `test_fill` / native未生成fallbackを削除し、release経路ではnative queue生成済みの `FmqQueue` だけを扱う構造にした。
- `TunerAidlService::new_degraded()` を削除し、空runtime service生成helperをrelease APIから除外した。
- 未実装domainの実装は進めず、release/test境界と捨て対象の除去だけを行った。

# r50ee79_ssot_quality_cleanup

- ○×表の派生IDを明示し、ORIG-305 / ORIG-306 / ORIG-307 を復元して元ORIG-001〜ORIG-376のcoverageを376/376へ戻した。
- `aidl_service/src/input_snapshot.rs` の `DemuxFilterType` Debug文字列 `contains()` 分類を削除し、AIDL型fieldのmatchによる分類へ置換した。
- `control/src/lib.rs` の `WorkerFailureClassifier` unit structを削除し、`WorkerFailureDomain::runtime_failure_kind()`へ畳んだ。
- `service_runtime/src/capability_profile.rs` の `CapabilityProfilePolicy` unit structを削除し、profile判定をmodule関数へ畳んだ。

# r50ee78_ssot_quality_rework

- `tuner_hal2/DESIGN_JA.md` を、既存 `tuner_hal/DESIGN_JA.md` との差分構造だけを書く文書へ縮小した。完了判定、未達理由、実装規約、product統合手順、変更履歴は他文書へ重複配置しない。
- `CODE_CONVENTION.md` を実装規約だけに整理し、WorkerExit / WorkerFailureClassifier / ScanSessionTxn の契約再定義を削除した。
- `README_JA.md` を使い始め入口に限定し、設計責務表や統合手順を重複させない形へ戻した。
- アーカイブ外の○×表から過去経緯本文を削り、WP/完了条件/判定だけをSSOT化した。
- r50ee77でpartial名にした理由のうち、build/atest/VTS/実機確認をこちらが実行できないことを通常成果物否認理由にしないよう整理した。

# r50ee77_ssot_quality_rework_partial

- `tuner_hal2/DESIGN_JA.md` の記載範囲を tv 直下 `開発規則.md` に集約し、README/CODE_CONVENTION/DESIGN_JA 間の責務重複を削減した。
- `README_JA.md` は使い始め入口に限定し、設計責務表と統合手順の重複説明を削除した。
- `CODE_CONVENTION.md` は実装規約に限定し、WorkerExit/WorkerFailureClassifierの契約定義を再掲しない形へ直した。
- `FrontendLivePumpOwner` を追加し、live pump coreをthread/cancel/join/report所有へ進めた。ただしdemux sink接続は未達であり、WP-R05のdemux配送行は×維持。
- r50ee75の「tuner_hal2ではFrontendWorkerRegistry/FrontendWorkerStopOutcomeを正本とする」記載はr50ee76で撤回済みであり、現行契約ではWorkerExit/WorkerFailureClassifierの契約名を維持する。

# r50ee76_design_name_alignment

- `tuner_hal2/DESIGN_JA.md` を、既存 `tuner_hal/DESIGN_JA.md` と異なる、または既存文書に存在しない tuner_hal2 固有事項だけを書く方針へ改訂した。
- `WorkerExit` / `WorkerFailureClassifier` は旧`tuner_hal`の単なる参照概念ではなく、tuner_hal2でも契約名として維持する方針へ修正した。
- `FrontendWorkerStopOutcome` はworker終了分類の正本ではなく、停止要求APIの操作結果に限定した。`Completed` には `WorkerExit` を含めるよう実装を改めた。
- `control/src/lib.rs` に `WorkerFailureClassifier` / `WorkerFailureDomain` を追加し、自由文字列分類やモジュール固有の別名分類器を作らないようにした。
- r50ee69以降に追加された `backend_worker.rs` / `scan_session.rs` / `live_pump.rs` が `Android.bp` の `libmaleicacid_tuner_hal2_device` srcsに含まれていなかったため追加した。

# r50ee75_quality_boundary_fix

- `tuner_hal2/DESIGN_JA.md` を追加し、tuner_hal2固有の worker / ScanSession / live pump / 旧`tuner_hal`参照境界を同モジュール内の設計正本へ移した。
- 旧`tuner_hal/DESIGN_JA.md` 末尾に混入していた tuner_hal2 固有の品質再構築追補を削除した。
- `WorkerFailureClassifier` / `WorkerExit` は旧`tuner_hal`側の参照概念であり、tuner_hal2では `FrontendWorkerRegistry` / `FrontendWorkerStopOutcome` を正本とすることを固定した。
- リリース物規則に反する `tuner_hal2/COMPLETION_MATRIX.md` をアーカイブから除外し、○×表は配布補助成果物としてアーカイブ外に置く方針へ戻した。

# r50ee72_tune_failure_diagnostic

- WP-R05の先頭未達であるORIG-038に対し、backend tune worker failureをfrontend runtimeへ診断反映する経路を追加した。
- `FrontendRuntime`に`TuneFailed` terminal eventを追加し、active tune generationのworker失敗時に`Failed`状態へ落とす。
- `TunerServiceRuntime::mark_frontend_tune_worker_failed()`を追加し、AIDL tune workerからfailureを記録できるようにした。
- ただし旧backend session snapshot、rollback executor、bound demux quarantineは未実装のためORIG-038は×のまま維持する。

# r50ee70_scan_session_aidl_connect

- r50ee69で追加したScanSession正本を、AIDL `IFrontend.scan()` 成功経路へ接続した。
- `IFrontend.scan()` は既存Scan worker/ScanSessionをSupersededByNewRequestで停止し、新generationへlive reader descriptorとScanSessionをcommitしてからScan workerを起動する。
- Scan workerはcandidateごとにbackend sessionをopen/submit/stopし、candidate progressionとScanEnd terminal記録へ接続する。
- backend submit失敗時はScanSessionをbackend failedへ落とす。
- callback delivery、live TS pump、demux binding、旧tune rollbackは未接続のためWP-R05全体は未完了。

# r50ee68_quality_rework

- WP-R05品質是正。旧`tuner_hal`は丸ごと移植せず、`WorkerLifecycleTxn` / `WorkerExit` / `ScanSessionTxn` / `backend_submit_tune()` / live pump stop/join方針を設計参照として採用することを `tuner_hal/DESIGN_JA.md` に固定した。
- `run_frontend_backend_scan_worker()` と `FrontendBackendScanPlan` を削除した。これは最初に成功したcandidateをbackend tuneし、cancelまで待つだけでScanSession、candidate progression、scan END callback delivery、callback failure診断を持たず、scan実装として不適切だったためである。
- `IFrontend.scan()` はsettings変換、frontend境界validate、backend candidate生成までは行うが、ScanSession正本が未実装のため `UNAVAILABLE` を返す。tune-backed scan placeholderによる成功no-opを禁止する。
- `LiveReader` / `LiveReaderKind` を `FrontendLiveReaderDescriptor` / `FrontendLiveReaderDescriptorKind` へ改名した。descriptorだけをlive pump完了条件の根拠にしない。
- worker開始後のlive descriptor install失敗時に `request_stop()` 結果を `let _ =` で捨てず、stop/join結果を検査するようにした。
- ○×表をSSOTとし、全WP・完了条件Markdownは生成しない方針へ変更した。r50ee67で過大○だったORIG-039/041/049/050/051/052を×へ戻した。
- WP-R05は未完了。ScanSession正本、live pump、callback terminal delivery、rollback/quarantine、demux bindingは未達である。

# r50ee66_partial_wp_r05_scan_supersede

- WP-R05の未達解消を継続。前回の未達理由は一部「未実装宣言」に留まっていたため、先頭×行のうちscan supersedeに必要な実コードを追加した。
- `FrontendWorkerRegistry::request_stop_and_join()` を追加し、既存scan workerを `SupersededByNewRequest` reasonでcancelし、完了回収してから新scan workerを起動できるようにした。
- `IFrontend.scan()` は新scan開始前に既存scan workerを停止・回収し、live readerをclearしてから新generationのscan workerを起動するようにした。
- `stopTune()` / `stopScan()` はcancel要求だけでなくworker完了回収経路へ接続した。
- `FrontendRuntime` にterminal event generation filterを追加し、新generation開始後に旧generationのterminal eventを受理しない基礎を追加した。
- terminal eventの実callback配送、Cancelled/END診断、rollback/quarantine、demux bindingは未実装のためWP-R05全体は未完了。

# r50ee40_partial_wp_r04

- WP-R04の未達解消を継続。前回の課題群は外部ブロッカーではなく、主に未実装・未接続であるため追加実装した。
- `RuntimeRegistry` に filter / DVR / descrambler runtime ID台帳を追加し、各ID発行を `checked_add()` 化した。
- `IDemux.openFilter()` / `IDemux.openDvr()` を、demux owner検証 -> child runtime ID確保 -> AIDL child object table登録 -> wrapper生成失敗時rollbackへ進めた。
- `ITuner.openDescrambler()` を固定UNAVAILABLEから、descrambler runtime ID確保 -> AIDL object table登録 -> wrapper生成失敗時rollbackへ進めた。
- parent demux close時のcascade closeで、object table上のfilter / DVR childとそれぞれのruntime IDを解除する経路へ進めた。
- `boot_from_probe_results()` でtransient runtime registryとAIDL object tableを初期化し、probe再実行時の旧object混入を避けるようにした。
- LNB export/probe、filter/DVR/descramblerの実data path cleanup、worker ownership、FMQ/AV/callback worker cleanup、全domain generation overflowは未実装のためWP-R04全体は未完了。

# r50ee38_partial_wp_r04

- WP-R04の未達解消を継続。
- `openDemux()` を固定UNAVAILABLEから、runtime demux ID確保 -> AIDL object table登録 -> wrapper生成 -> wrapper生成失敗時runtime ID解除へ進めた。
- `getDemuxIds()` を固定空配列からruntime registry由来へ変更した。
- `openDemuxById()` をruntime registry上の既存demux ID検証後にAIDL objectを返す経路へ進めた。
- `RuntimeRegistry` に demux runtime ID台帳とchecked_add()によるID発行を追加し、ID枯渇時は失敗へ落とすようにした。
- Demux object close成功後にruntime registry上のdemux IDを解除する経路を追加した。
- `getLnbIds()` / `openLnbById()` はLNB registry由来へ寄せたが、LNB probe/export IDが未実装のため通常は空/UNAVAILABLEのままである。
- LNB name解決、demux配下filter/DVR/descrambler生成、実runtime cleanup、worker ownershipは未実装のためWP-R04全体は未完了。

# r50ee37_partial_wp_r04

- WP-R04の未達解消を継続。
- `AidlObjectGeneration` はstale handle / ABA防止の世代IDとして使う局所手法であり、AOSP AIDL標準API名ではないことを明確化。
- `RuntimeObjectTable` のchild object登録時にowner objectがliveであることを検証するよう変更し、owner missing / owner generation mismatch / owner kind mismatch / owner not liveをtyped error化した。
- public method lookupで対象object自身だけでなくowner chainがliveであることも検証するよう変更した。
- parent object close時にobject table上のdescendant child objectをClosing/CleanupFailed/Closedへ連動させるcascade lifecycle helperを追加した。
- AIDL close共通経路をsingle-object closeからobject table cascade closeへ変更した。
- これはobject table上の所有・世代・親子lifetimeの前進であり、実filter/DVR/descrambler runtime cleanupやworker cleanupはまだ未接続のためWP-R04全体は未完了。

# r50ee36_partial_wp_r04

- WP-R04のうち、外部ブロッカーではなく実装未達だった箇所へ追加対応。
- `TunerServiceRuntime` にAIDL object generation発行状態を追加し、AIDL child object登録時の固定 `AidlObjectGeneration(1)` を廃止した。
- AIDL object generation発行を `checked_add()` 化し、overflow時は `RuntimeObjectTableError::GenerationOverflow` として失敗させるようにした。
- `RuntimeObjectTableError::GenerationOverflow` のAIDL error mappingを追加した。
- `ResourceLedger::reserve()` のgeneration発行を `saturating_add()` から `checked_add()` へ変更し、wrap/saturating reuseを禁止した。
- `getFrontendIds()` をprobe済みfrontend registry由来のID返却へ変更した。
- `openFrontendById()` をprobe済みfrontend ID検証後にAIDL object tableへ登録し、wrapper生成失敗時は登録rollbackする経路へ進めた。
- demux / LNB / child object / worker / callback delivery / FMQ / AV / descrambler の本番runtime未接続は残るため、WP-R04全体は未完了。WP-R04の○×表は×を維持する。

# r50ee35_partial_wp_r04

- WP-R04の先頭未達に着手。
- `RuntimeObjectTable` に `RuntimeObjectLifecycle` を追加し、Live / Closing / CleanupFailed / Closed / Quarantined を区別するようにした。
- close時にobject table entryを即削除せず、begin close -> callback cleanup -> commit close の状態遷移を通すようにした。
- callback store cleanup失敗を握りつぶさず、objectをCleanupFailedへ落として `UNKNOWN_ERROR` を返すようにした。
- object registration後にAIDL object wrapper生成が失敗した場合、登録済みobjectをrollbackする補助経路を追加した。
- AIDL公開method内の `unreachable!()` を削除し、想定外成功時は `UNKNOWN_ERROR` を返すようにした。
- WP-R04全体は未完了。worker failure、全公開API transaction rollback、cascade cleanup、generation overflow、DropLeakTxn、表5/7/8/10/11/14/18/20の全面実装は未達。

# r50ee34

- WP-R03の○行を再確認し、r50ee33のORIG-024完了判定に虚偽があったことを記録。
- `IFilter.getId()` / `IFilter.getId64Bit()` が `AidlMethodCall` / `DomainCommand` / `CommandPlan` を通らず直接IDを返していた問題を修正。
- `FilterGetId` / `FilterGetId64Bit` の `AidlApi`、`RuntimeTransactionName`、`CommandPlan`、dispatch table、`FilterCommand`、`AidlMethodCall` を追加。
- `service_runtime` が `binder_adapter` に依存しない構造は維持し、公開AIDL methodのtyped command網羅を再確認。

# r50ee33

- WP-R03 / ORIG-024の未達に対応。
- `UnsupportedPublicApi` typed commandを追加し、unsupported/probe未接続系の公開AIDL methodを `AidlMethodCall` / `DomainCommand` / `CommandPlan` へ載せる構造へ変更。
- ITuner query系の空結果返却も `CommandPlan` 経由へ通し、直接returnのみの経路を削減。
- `status_unavailable()` / `StatusCode::NAME_NOT_FOUND` の直接返却経路を削除。
- WP-R03を全件○へ更新。

# r50ee32

- 品質問題対応として、`service_runtime` が `binder_adapter` の型正本へ依存していた責務逆転を是正。
- `AidlObjectKind` / `AidlObjectId` / `AidlObjectGeneration` / `AidlApi` / `RuntimeTransactionName` / `CommandPlan` / `AIDL_TRANSACTION_TABLE` の正本を `domain_request` crateへ移動し、`binder_adapter` は再exportする構造へ変更。
- `service_runtime` の dispatch / object table / callback registry / runtime handler / unit tests から `binder_adapter` 依存を削除。
- `TunerServiceRuntime::plan_domain_command_dispatch()` を削除し、`CommandPlan` と `RuntimeExecutableRequest` を受ける `plan_command_dispatch()` に置換。
- 不要な二重正本を残さず、runtime側の参照元を `domain_request` に統一。
- WP-R03のORIG-024は引き続き×。直接 `status_unavailable()` / `NAME_NOT_FOUND` を返す公開AIDL methodが残るため、全公開methodが型付き格納へ入る条件は未達。

# r50ee31

- WP-R03 の前回○を再確認。
- ORIG-024 は主要経路では `AidlMethodCall` / `DomainCommand` / `CommandPlan` / `RuntimeExecutableRequest` へ到達するが、`getStatus()`、`setLnb()`、`openTimeFilter()`、`getAvSyncHwId()`、`connectCiCam()` など直接 `status_unavailable()` を返す公開AIDL methodが残っていたため、全公開method条件としては未達と判定。
- r50ee30 の ORIG-024 ○は虚偽として扱い、○×表を×へ戻した。
- コード品質評価として、B案の責務分離は前進しているが、`service_runtime` が `binder_adapter` 型へ依存している点と、直接return経路が残る点を未達理由へ明記。

# r50ee30

- WP-R03 / ORIG-024 の未達解消を継続し、「ブロッカー」は存在しないことを明確化。残っていたのは判断待ちではなく実装順序上の未作成部品である。
- `DomainCommand::runtime_executable_request()` を追加し、typed domain request を runtime dispatch plan へ載せる境界を追加。
- `RuntimeCommandDispatchPlan` が `RuntimeExecutableRequest` を保持できるよう変更し、runtime handler側でも profile support / value validation を検査する前段を追加。
- `AidlMethodAdapter::plan()` の重複宣言を削除し、r50ee29の構文上の粗さを修正。
- ORIG-024は、AIDL method相当前段から `AidlMethodCall` / `DomainCommand` / `CommandPlan` / runtime executable request へ型付き格納する条件を満たすため○へ更新。

# r50ee29

- WP-R03 / ORIG-024 の未達解消を継続。
- ORIG-024 の前回説明は、状況と未達理由の分離が弱く、特に「なぜ今回完了できないか」の阻害課題としては不十分だったため、○×表の様式を是正。
- `DemuxOpenFilterRequest` / `DemuxOpenDvrRequest` を `domain_request` crateへ追加し、openFilter/openDvr の入力も runtime executable request 前段へ載せた。
- `DemuxFilterTypeDomain` を追加し、openFilter時の filter type を `ts/section/av/pes_data/record/unknown` のdomain分類として保持するよう変更。
- `ScIndexMaskRequest` を追加し、record `scIndexMask` を debug文字列だけでなく正規化可能なscalar maskとして保持する前段を追加。
- `binder_adapter/src/domain_request.rs` の未定義 `format_static` 依存を排除し、DVR field名を静的tableで解決するよう修正。
- ORIG-024は前進したが、Filter/DVR object生成transactionが未実装で、openFilter時のfilter typeをfilter object lifetimeへ永続化できず、configure(settings)との対応検証を実methodで保証できないため×を維持。

# r50ee28

- WP-R03 の前回○を再確認。ORIG-020/021/022/023/025/027 は現物上、完了条件を満たすことを確認。
- ORIG-232 は WP-R03 ではなく、runtime execution / rollback / callback / FMQ / EventFlag / cleanup failure の実発生点が揃った後に全公開method横断で検証すべき条件であるため、WP-R13へ再配置。
- ORIG-024 の残未達理由は、返答内で状況と実装完了を妨げている課題を説明する形式として成立していたが、表側の残理由も同じ粒度へ維持。
- 完了条件行数は376件のまま維持し、欠落・重複なしを確認。

# r50ee27

- WP-R03のORIG-024/ORIG-232未達解消を継続。
- `domain_request` crateに `RuntimeExecutableRequest` を追加し、`AidlDomainRequest` からruntime handlerが直接受け取る最終request enumへ変換できる前段を追加。
- `SectionBitsConditionRequest` をfilter/mask/modeの長さだけではなくbyte列本体を所有する型へ変更し、section conditionの本体所有先を `domain_request` 側へ固定。
- `aidl_service/src/input_snapshot.rs` が section condition のfilter/mask/mode byte列をhex fieldとしてsnapshot化するよう変更。
- `binder_adapter/src/domain_request.rs` がsection condition hex fieldをdecodeし、`SectionBitsConditionRequest` のbyte列本体へ渡すよう変更。
- `execute_object_aidl_method()` を `AidlDomainRequest` ではなく `RuntimeExecutableRequest` のprofile support / value validationを評価する形へ変更し、runtime実行用requestへの変換結果をexecutor前段の検査対象にした。
- `configureIpCid()` / `configureMonitorEvent(nonzero)` の個別validator直返しを減らし、`unavailable_after_method_plan()` 経由でstatus precedence resolverを通すよう変更。
- ORIG-024は最終request enumとsection condition本体所有先まで進んだが、DemuxFilterTypeとDemuxFilterSettingsの対応検証、record scIndexMask構造化、runtime handler本体への接続が未達のため×を維持。
- ORIG-232は個別早期returnを一部削減したが、runtime execution failure / rollback failure / callback・FMQ・EventFlag・cleanup failureの発火点が未実装であり、全公開methodのexecutor集約は未達のため×を維持。

# r50ee26

- B案を採用固定。純粋domain request型の正本を `binder_adapter` から分離し、専用crate `domain_request` へ移した。
- `domain_request/src/lib.rs` を追加し、`AidlDomainRequest` / `DemuxFilterDomainRequest` / `TsFilterRuntimeRequest` / `DvrRuntimeRequest` / `AvStreamDomainRequest` / `FilterDelayDomainRequest` / `DomainValueValidation` を収容した。
- `binder_adapter/src/domain_request.rs` はAIDL `AidlInputSnapshot` から `domain_request` crateのdomain request型へ変換するadapter層に限定した。
- `binder_adapter/src/aidl_method.rs` は `domain_request_from_snapshot()` を通じてdomain requestを作るよう変更し、domain型そのものの正本を持たない構造へ寄せた。
- `Android.bp` に `libmaleicacid_tuner_hal2_domain_request` を追加し、`binder_adapter` / `service_runtime` から参照可能にした。
- ORIG-024はB案のcrate分離まで進んだが、runtime handlerが受け取る最終request enumへの接続、section condition byte列本体、record scIndexMask構造化、DemuxFilterTypeとの対応検証が未完成のため×を維持。
- ORIG-232はB案の影響で前提構造は改善したが、runtime execution failure / rollback failure / callback・FMQ・EventFlag・cleanup failure / 個別早期returnのexecutor集約が未達のため×を維持。

# r50ee25

- WP-R03のORIG-024/ORIG-232未達解消を継続。
- `binder_adapter/src/domain_request.rs` の `AidlDomainRequest` を文字列field bag中心から、TS filter / DVR / AV stream / delay hint別のruntime request型へ分解。
- `DemuxFilterSettings::Ts` 配下の `Noinit` / `Section` / `Av` / `PesData` / `Record` を `TsFilterRuntimeRequest` として区別し、`TsPid`、section condition、DVR direction、AV stream kind、delay kindを型へ昇格。
- unsupported demux variantを `UnsupportedDemuxFilterRequest` として保持し、TS-only profile外を記録したうえで `UNAVAILABLE` に落とす前段を強化。
- `execute_object_aidl_method()` が `AidlDomainRequest` のprofile unsupportedとinput validation failureをfailure sourceとして集約するよう変更。
- ただしruntime実行用requestの全field完全変換、section condition byte列本体の所有先、rollback failure/runtime execution failureのexecutor集約は未達として残す。

# r50ee24

- WP-R03の先頭未達解消を続行。
- `binder_adapter/src/domain_request.rs` を追加し、AIDL入力snapshotを値域検証規則付きdomain request型へ変換する層を追加。
- `DomainCommand` の Demux / Filter / DVR / Frontend callback / LNB 適用系を `AidlDomainRequest` 保持へ変更し、runtimeがdebug snapshot文字列だけを再解釈する構造を縮小。
- `AidlFailureSource::ObjectLifetime` と `resolve_failure_source_by_precedence()` を追加し、共通executor骨格がobject lifetime / runtime dispatch failureをfailure sourceとして解決する形へ前進。
- `unavailable_after_method_plan()` を固定UNAVAILABLE返却から、`AidlFailureSource::RuntimeDispatch` をstatus precedence resolverへ渡す経路へ変更。
- ORIG-024 / ORIG-232は未達のまま維持し、残る阻害課題を○×表へ記載。

# r50ee23

- WP-R03の先頭未達解消を継続。
- 「TS/MMTP/IP/TLV/ALP variantを記録し、TS-only profile外をUNAVAILABLEへ落とす」責務を、variant/field保持はORIG-024、status優先順位はORIG-232として整理。
- Frontend/LNB callback `Strong` 保持をAIDL object本体から `callback_store` へ移し、AIDL object本体をhandle + shared runtime参照だけに戻した。これによりORIG-025を○へ戻した。
- `AidlFailureSource` と `AidlStatusMapper::resolve_failure_by_precedence()` を追加し、profile unsupportedをinput validationより先に解決できるstatus resolverを追加。
- `IDescrambler.addPid()` の重複関数宣言を削除。
- ORIG-024はruntime消費用domain request型への完全変換が未実装、ORIG-232は全公開methodのfailure source集約が未接続のため×のまま維持。

# r50ee22

- 前回着手WPであるWP-R03の○行を再確認。
- r50ee21の `ORIG-025` は、Frontend/LNB AIDL objectにcallback slotを保持しており「handleと共有runtime参照だけ」とは言えないため虚偽○として×へ戻した。
- `ORIG-027` は `IFrontend.tune()` / `scan()` と `IDescrambler.addPid()` / `removePid()` で入力変換がclose検査より先に走る経路があったため修正し、close状態検査を先行させた。
- WP-R03の残未達理由を、状況説明と実装完了を妨げる課題に分けて更新。

# r50ee21

- WP-R03の未達解消を継続。
- `aidl_service/src/aidl_v2_conversion_contract.rs` を追加し、Android 14 / LineageOS 21相当の `android.hardware.tv.tuner-V2` frozen AIDL生成hash、`DemuxFilterSettings` union tag、TS/DVR/AV/delay系field契約をコード上に固定。
- `input_snapshot.rs` をr50ee21のAIDL V2変換契約へ接続し、schema hashとfield contractをsnapshotへ添付。
- `AidlMethodCall::api()` と `AidlMethodPlan.api` を追加し、`execute_object_aidl_method()` をAPI別status precedence tableを通る共通executor骨格へ接続。
- WP-R03内に残していた `ORIG-026` / `ORIG-033` は再ソート不足だったため、Filter/DVR open transactionおよびcallback worker/failure接続の後続WPへ移動。
- r50ee20の残理由が浅かった `ORIG-026` / `ORIG-033` / `ORIG-232` は、状況説明と実装完了を妨げる課題を分けて再記載。

# r50ee20

- WP-R03のORIG-024未達を前進。
- `aidl_service/src/input_snapshot.rs` をAndroid 14 / LineageOS 21相当の `android.hardware.tv.tuner-V2` AIDL schema前提の構造化snapshotへ拡張。
- `DemuxFilterSettings::Ts` の `Noinit` / `Section` / `Av` / `PesData` / `Record`、`DvrSettings::Record` / `Playback`、`AvStreamType::Video` / `Audio`、`FilterDelayHint` の主要fieldをfield名付きで保持するよう変更。
- MMTP / IP / TLV / ALPはTS-only profile外としてvariantを記録し、`UNAVAILABLE`へ落とす前提を明示。
- ORIG-024は、unsupported variantの全field domain保存、invalid/unsupported precedence、実domain request型への完全変換が未完のため×のまま維持。

# r50ee19

- WP-R03の未達解消を継続。
- `aidl_service/src/input_snapshot.rs` を追加し、`DemuxFilterSettings` / `DvrSettings` / `AvStreamType` / `FilterDelayHint` / openFilter / openDvr のsnapshot生成を公開method本体から分離した。
- `IFilter.configure()` / `configureAvStreamType()` / `setDelayHint()`、`IDvr.configure()`、`IDemux.openFilter()` / `openDvr()` を専用snapshot生成関数へ接続し、公開method本体がdebug文字列を直接組み立てる経路を縮小した。
- `unavailable_after_method_plan()` をAPI別status precedence tableを参照する経路へ変更し、固定UNAVAILABLE返却でもtransaction tableのprecedence entry欠落を検出するようにした。
- ただしAIDL生成型の全variant・全fieldをdomain型へlosslessに落とす変換、Filter/DVR callbackのowner object寿命、callback配送worker/failure状態、全公開methodの共通executor化は未達のためWP-R03は×のまま維持する。

# r50ee18

- WP-R03の未達解消を継続。
- r50ee17の残未達理由が「何が未実装か」の説明に寄っていたため、○×表の理由欄を「今回実装完了を妨げている課題」へ書き直した。
- `aidl_service/src/callback_slot.rs` を追加し、Frontend/LNB callback の `Strong<dyn ...Callback>` をAIDL object shell内で保持するslotを追加。
- `FrontendAidlObject` / `LnbAidlObject` の `setCallback()` を、method plan後に callback slot と runtime callback registry へ登録する経路へ進めた。
- `close_object()` で runtime callback registry のowner entryを消すようにし、Frontend/LNB object close時はcallback slotもclearするようにした。
- Filter/DVR callbackは、openFilter/openDvrの実object生成・owner rollbackがWP-R04未達のため、まだ実callback配送経路へ保持できないものとして×に残した。

# r50ee17

- WP-R03の未達解消を継続。
- `IFilter.getQueueDesc()` / `IFilter.getAvSharedHandle()` / `IFilter.releaseAvHandle()` / `IDvr.getQueueDesc()` を `AidlMethodCall` / `DomainCommand` / `CommandPlan` / runtime dispatch table へ接続。
- `service_runtime/src/callback_registry.rs` を追加し、callback owner / generation / registration API / health state の台帳型を追加。
- ただし実callback配送worker、Strong callback保持、callback failure発生時の状態遷移接続は未実装のため、WP-R03のcallback系完了条件は×のまま維持。
- `DemuxFilterSettings` / `DvrSettings` / `AvStreamType` / `FilterDelayHint` のAIDL生成型全field lossless domain変換も未完了のため、ORIG-024は×のまま維持。

# r50ee16

- WP-R03の未達解消を継続。
- `AidlInputSnapshot` を単一debug文字列だけでなく `AidlInputField` の構造化field列を保持できる形へ拡張。
- `Demux.openFilter/openDvr`、`Demux.setFrontendDataSource`、`IFilter.configure/configureAvStreamType/setDataSource/setDelayHint`、`IDvr.configure/attachFilter/detachFilter`、`ILnb.set*` 系の入力snapshotを、既知scalar/handle有無についてfield名付きで保持する形へ更新。
- `AidlStatusMapper` にAPI別status precedence tableを追加し、少なくともtransaction table上の全APIが precedence entryを持つことを単体テストで固定。
- ただしAIDL生成型の全fieldを個別domain型へ写すlossless変換、callback実配送、callback failure状態遷移、全method実経路へのprecedence適用は未達のため、WP-R03は×のまま維持。

# r50ee15

- WP-R03の未達解消を継続。
- child AIDL methodのtyped command前段を拡張し、Demux openFilter/openDvr、Filter configure/configureAvStreamType/setDataSource/setDelayHint、DVR configure/attach/detach/status interval、Descrambler setDemuxSource、LNB callback/voltage/tone/position/DiSEqCの入力を `AidlMethodCall` / `DomainCommand` へ格納する経路を追加。
- ただしAIDL生成型を構造化したlossless変換、callback配送worker、callback failure状態、method別error precedenceは未達のため、WP-R03は×のまま維持。

# r50ee14

- WP-R03 の未達解消を継続。
- `IFrontend.tune()` / `scan()` が placeholder `FrontendTuneRequest` を使わず、AIDL `FrontendSettings` から ISDB-T / ISDB-S の公開入力を検証して `FrontendTuneRequest` へ変換する前段を追加。
- `IDescrambler.addPid()` / `removePid()` が placeholder PID 0 を使わず、AIDL `DemuxPid::TPid` を検証して typed command へ渡すよう修正。
- callback / FMQ / AV / data source / DemuxFilterSettings / DvrSettings の lossless DomainCommand 化は未達のまま、WP-R03 の該当欄を×で維持。

# r50ee13

- 先頭未達WPであるWP-R03に着手した。
- `aidl_service/src/object_runtime.rs` に `plan_object_aidl_method()` / `close_object_after_aidl_method_plan()` を追加し、AIDL object tableのclose状態検査後に `AidlMethodCall` / `DomainCommand` / `CommandPlan` / runtime dispatch target計画へ到達する共通前段を追加した。
- child AIDL object shellに `plan_method()` / `close_object_after_plan()` を追加した。
- `IFrontend.tune/stopTune/scan/stopScan/close`、`IDemux.openFilter/openDvr/close`、`IFilter.configure/configureIpCid/configureMonitorEvent/start/stop/flush/close`、`IDvr.configure/start/stop/flush/close`、`IDescrambler.setDemuxSource/setKeyToken/addPid/removePid/close`、`ILnb.setVoltage/setTone/setSatellitePosition/sendDiseqcMessage/close` から型付きmethod計画へ入るようにした。
- ただし `FrontendSettings` / `DemuxPid` 等のAIDL入力をlosslessにDomainCommandへ格納する変換、callback objectの実配送経路保持、callback failure状態接続、全公開methodのAPI別error precedenceは未達のため、WP-R03は×のまま維持する。
- 静的確認は実施した。Soong build / Rust unit test / atest / VTS はこの環境では未実行であり、最終gate側のWP-R13へ残す。

# r50ee12

- 先頭未達WPであるWP-R02に再着手した。
- `IFilter.configureIpCid()` / `IFilter.configureMonitorEvent()` のprofile境界を `FilterAidlObject` のmethod別前段へ移し、公開AIDL methodから直接通る経路として固定した。
- `configureIpCid()` は値が負数・0・正数のいずれでも、TS-only profile外のため `UNAVAILABLE` へ写像する。
- `configureMonitorEvent(0)` は成功、非0 mask は profile非宣言のため `UNAVAILABLE` へ写像する。
- unsupported API precedence を、IP CID値検証よりprofile unsupportedを先に評価する形で固定した。
- WP-R02内の残未達3行を○へ更新した。WP-R02内に未達は残さない。
- 静的確認は実施した。Soong build / Rust unit test / atest / VTS はこの環境では未実行であり、最終gate側のWP-R13へ残す。

# r50ee11

- WP-R02再確認で、r50ee10の○×表に虚偽が含まれていたことを記録した。
- r50ee10では「API別に `INVALID_ARGUMENT` / `INVALID_STATE` / `UNAVAILABLE` / `UNKNOWN_ERROR` をDESIGN_JA.mdと一致させる」を○にしていたが、実態は汎用 `HalError` kind 写像までであり、公開AIDL method別の入力検証・状態検証・unsupported precedence を実method経路へ接続していなかった。
- そのため同完了条件を×へ戻し、未達理由を○×表へ明記した。
- ○×表のWP順序を再ソートした。probe/capability、AIDL method別error接続、VTS/profile完全一致など、後続runtimeや最終gateを前提にする行をWP-R02から後続WPへ移した。
- 元の完了条件行は376件すべて保持し、欠落0・重複0を確認した。
- 実装コードは進めていない。

# r50ee10

- 再編成後の先頭未達WPであるWP-R02に着手した。
- `service_runtime/src/capability_profile.rs` を追加し、TS-only capability/profile、IP CID非対応、monitor event非0非対応、HAL-generated Japanese scan plan禁止、失敗領域分類を型として固定した。
- `HalError` に callback / FMQ / EventFlag / cleanup の失敗領域を追加し、device missing/open失敗は `UNAVAILABLE`、runtime ioctl/read・callback・FMQ・EventFlag・cleanup失敗は `UNKNOWN_ERROR` へ写像するよう `AidlStatusMapper` を修正した。
- `IFilter.configureIpCid()` は値の妥当性にかかわらず、TS-only profile外のため `UNAVAILABLE` へ写像する。
- `IFilter.configureMonitorEvent(0)` は成功、非0 mask は profile非宣言のため `UNAVAILABLE` へ写像する。
- frontend/demux/filter/DVR/descrambler/LNB capability のruntime/probe/profile完全一致、VTS profile生成、export frontend ID / physical group ID生成、linkCaps/AV shared/descrambler/LNB/statusCaps profile連動は後続WPが前提のためWP-R02内に×として残す。

# r50ee8

- WP-02再確認で、r50ee7の○×表に虚偽が含まれていたことを記録した。
- r50ee7では「AIDL method相当前段から `AidlMethodCall` / `DomainCommand` / `CommandPlan` へ型付き格納する」を○にしていたが、公開AIDL method本体はその経路を実際には呼ばず、`plan_method()` と単体テスト用経路に留まっていた。
- そのため、同完了条件を×へ戻し、未達理由を○×表に明記した。
- 実装コードは進めていない。WP-02総合判定は×のまま維持する。

# r50ee7

- 先頭未達WPであるWP-02に再着手した。
- child AIDL objectが `AidlObjectHandle` に加えて共有 `TunerServiceRuntime` 参照だけを持つようにし、runtime状態の複製は行わないまま、全child公開methodの先頭で object table の kind / generation / closed 状態を検査する経路を追加した。
- `aidl_service/src/object_runtime.rs` を追加し、close後・missing object・generation mismatch・kind mismatchを `INVALID_STATE` に写像する共通gateにした。
- `IFrontend` / `IDemux` / `IFilter` / `IDvr` / `IDescrambler` / `ILnb` の `close()` は、WP-02段階ではAIDL object table登録を解除するところまで実装した。
- callback object保持、probe/capability生成、open系owner/rollback、callback failure状態接続は後続WPのruntime / worker / delivery 実体化が前提のため、WP-02総合判定は×のまま維持する。

# r50ee6

- WP-01再確認で、r50ee5の○×表に虚偽が含まれていたことを記録した。
- r50ee5では「旧 `tuner_hal` の旧product統合用断片を残さない」を○にしていたが、旧 `tuner_hal/INTEGRATION.md` が残っていたため、この○は完了条件を満たしていなかった。
- 旧 `tuner_hal/INTEGRATION.md` を削除し、product統合手順のSSOTを `tuner_hal2/INTEGRATION.md` のみに固定した。
- `tuner_hal/README_JA.md` と `tuner_hal2/INTEGRATION.md` に、旧 `tuner_hal/INTEGRATION.md` を置かない方針を反映した。

# r50ee5

- product default Tuner HAL service を `tuner_hal2` だけに固定した。
- 旧 `tuner_hal` は参照用ソースとして残すが、product package、VINTF manifest、init rc、PRODUCT_PACKAGES、product integration には含めない方針へ変更した。
- `tuner_hal2/INTEGRATION.md`、`config/product_integration.mk`、`BoardConfigVendorSePolicy.mk`、`ueventd.tuner_hal2.rc` を追加した。
- 旧 `tuner_hal/config/product_integration.mk` は削除し、旧product統合用のconfig、VINTF/init、sepolicy、profile断片も削除した。
- `README_JA.md` と `開発規則.md` の既定service方針をr50ee5の固定方針へ更新した。
- WP-01のproduct統合切替・旧service排他未達を解消し、WP-01を○へ更新した。

# r50ee4

- 先頭未達WPであるWP-02に着手した。
- `android.hardware.tv.tuner-service.maleicacid2` の `rust_binary`、`aidl_service/src/main.rs`、`aidl_service/src/service_entry.rs`、init rc、VINTF fragment、sepolicy contextを追加した。
- `TunerAidlService` に `ITuner` AIDL trait実装本体を追加し、`FrontendAidlObject` / `DemuxAidlObject` / `FilterAidlObject` / `DvrAidlObject` / `DescramblerAidlObject` / `LnbAidlObject` にchild AIDL trait実装本体を追加した。
- 未実装runtimeへ到達する公開methodは成功扱いせず、`UNAVAILABLE` / `NAME_NOT_FOUND` へ落とす方針にした。
- callback保持、close後状態検査、probe/capability生成、open系rollback、callback failure状態接続は後続WP前提のため、WP-02総合判定は×のまま維持する。

# r50ee3

- r50ee2で `README_JA.md` にWP-01判定・未達理由を記載していたリリース物規則違反を修正。READMEは第三者が使い始めるために必要な現行情報だけに戻した。
- WP-01の○行を再確認し、`README_JA.md` / `CHANGELOG.md` の責務分離をWP-01完了条件へ追加した。
- r50ee2の説明にあった「WP-01の未達3行」は誤りであり、実際は未達5行だったため訂正した。
- 実装コードは進めていない。WP-01総合判定は×のまま維持する。

# r50ee2

- 先頭未達WPであるWP-01を再確認。
- `tuner_hal2` を `android.hardware.tv.tuner.ITuner/default` の既定serviceへ登録する条件は、WP-02の `rust_binary` / Binder登録entrypoint / `ITuner` trait実装が未達のため満たせないと固定。
- WP-01の未達5行は×のまま維持し、未達理由を○×表へ明記。
- 未完成AIDL serviceを製品統合へ出して二重登録や不可能なbuild pathを作らない方針を維持。

# CHANGELOG

## r50ee1

- r50ed16を基準に、`tuner_hal/DESIGN_JA.md` と `tuner_hal2` 配下の `#[test]` 単体テストの準拠関係を全件再監査した。
- DESIGN_JA.mdと矛盾する単体テストは検出されなかったため、単体テスト削除は行っていない。
- r50ed16の健全性根拠は、未接続runtime handlerを成功扱いせず `RuntimeHandlerError::NotConnected` として失敗させる点にある。
- r50ee1はテスト削除結果のリリースであり、公開AIDL実装、runtime実処理、worker、callback、VTS対応範囲を新規完了扱いにしない。

## r50ed16

- WP-R2を実施し、`service_runtime` に object table と runtime dispatch handler を追加した。
- `RuntimeObjectTable` は `BTreeMap<AidlObjectId, RuntimeObjectEntry>` を単一正本とし、object kind、ledger id、generation、owner relation を型付きで保持する。
- generation mismatch、missing object、owner/kind mismatch は `RuntimeObjectTableError` / `RuntimeHandlerError` として返し、成功扱いしない。
- `RuntimeHandlerCoverage` により全 `RuntimeTransactionName` を `Connected` / `NotConnected` / `UnsupportedByDesign` のいずれかへ分類する。
- r50ed16時点では実runtime handler接続はまだ行わず、未接続handlerは `RuntimeHandlerError::NotConnected` として型付き失敗に固定する。
- WP-R2はAIDL公開契約やARIB構文処理を新規定義せず、既存 `tuner_hal/DESIGN_JA.md` の境界に合わせる内部object table / dispatch handler実体化に限定する。

## r50ed15

- WP-R1を実施し、`aidl_service` の AIDL Binder object外側骨格を追加した。
- `TunerAidlService`、各 AIDL object wrapper、`AidlObjectHandle`、`CallbackBridge`、`NativeHandleBridge`、`AidlErrorBridge` を追加した。
- AIDL objectは `AidlObjectHandle` だけを保持し、runtime状態を複製しない構造にした。
- `aidl_service` は `device`、`demux`、`descrambler`、`lnb` の内部runtime型やdriver ABIへ直接依存しない。
- Android 14 Tuner AIDL生成crate、`binder_adapter`、`service_runtime` への依存をSoong定義へ追加した。
- WP-R1はAIDL公開契約やARIB構文処理を新規定義せず、既存 `tuner_hal/DESIGN_JA.md` と Android 14 Tuner AIDL を正とする外側骨格追加に限定する。

## r50ed14

- WP-10を実施し、AIDL公開メソッド相当前段を `binder_adapter/src/aidl_method.rs` として実体化した。
- `AidlMethodCall` / `AidlMethodAdapter` / `AidlMethodPlan` により、公開メソッド相当入力から `DomainCommand` と `CommandPlan` を型付き生成する。
- `service_runtime/src/command_dispatch.rs` を追加し、`DomainCommand` から `ServiceRuntimeDispatchTarget` までを型付きで接続する。
- `TunerServiceRuntime::plan_domain_command_dispatch()` を追加し、dispatch target欠落時は型付き診断へ落とす。
- WP-10はAIDL Binder service本体を新規実装せず、公開AIDL method相当前段から内部runtime dispatch targetまでの接続に限定する。

## r50ed13

- WP-09を実施し、`service_runtime` の boot / registry / startup diagnostics / dispatch table を実体化した。
- `StartupDiagnosticRecord` は `StartupDiagnosticKind` / `StartupDiagnosticPhase` / backend / path / typed error を保持し、起動診断に `Vec<String>` を使わない。
- probeで見つからないfrontend、open失敗、capability suppress、duplicate frontend idを診断へ残し、存在しないfrontendをruntime registryへ登録しない。
- `RuntimeRegistry` は export対象frontendだけを `FrontendRegistryEntry` として保持する。
- `SERVICE_RUNTIME_DISPATCH_TABLE` により `binder_adapter` の `RuntimeTransactionName` から service runtime dispatch target を追跡可能にした。
- `maleicacid_tuner_hal2_service_runtime_test` をSoong test moduleとして追加した。
- WP-09はAIDL公開API契約やARIB構文処理を新規定義せず、既存 `tuner_hal/DESIGN_JA.md` の境界に合わせるservice runtime内部の実体化に限定する。

## r50ed12

- WP-07を実施し、`lnb/src` の runtime / apply transaction / lifecycle transaction / operation ledgerを実体化した。
- `LnbApplyTxn` は backend apply と registry commit を同一取引で扱い、registry commit失敗時は通常状態へ戻さず `Failed` に落とす。
- `LnbLifecycleTxn` は public close / owner loss と Drop leak を分離し、Drop leakではbackend applyやcallback clearを実行しない。
- `LnbOperationLedger` はactive操作と解除失敗を型付きで保持し、自由文字列の操作失敗表を使わない。
- WP-08を実施し、`binder_adapter` の command / transaction / status mapper前段を実体化した。
- 各AIDL API相当のdomain commandは `CommandPlan` により呼び出すruntime transactionを型付きで追跡できる。
- `AidlStatusMapper` は `HalError` のkindから `TunerStatusCode` へ写像し、表示文字列には依存しない。
- `binder_adapter` は driver ABI、FMQ shim、device/demux/descrambler/lnb runtime crateへ直接依存しない構造にした。
- WP-07/08はAIDL公開API契約やARIB構文処理を新規定義せず、既存 `tuner_hal/DESIGN_JA.md` の境界に合わせる内部runtime / adapter前段の実体化に限定する。
## r50ed11

- WP-06を実施し、`descrambler/src/runtime` の session / key token / PID claim / cleanup transactionを実体化した。
- `DescramblerKeyToken` をBinder境界でだけ `Vec<u8>` から生成するnewtypeにし、sessionはtoken bytesではなく `DescramblerKeySlotId` を保持する。
- `DescramblerKeyTable` は unknown token と expired token を型付きで分ける。raw CWは保持・出力しない。
- `DescramblerSessionTxn` が demux binding、key replacement、PID claim、cleanupを所有し、unknown token時のrollbackとclose時の一括cleanupをテストで固定した。
- `DescramblerPidClaim` は source filter id と generation を型付きで保持し、NULL source filter経路は現行Android 14 Rust AIDL境界の実装対象外として型付き拒否する。
- WP-06はIDescrambler公開API契約を新規定義せず、既存 `tuner_hal/DESIGN_JA.md` の境界に合わせる内部runtime実体化に限定する。

## r50ed10

- WP-05を実施し、FMQ write と EventFlag wake を `FmqDeliveryTxn::commit_payload()` の一体commit条件にした。
- write済みbyte数が期待値と一致しない場合は `ShortWrite` として write段階失敗に固定し、wake成功だけで配送成功にしない。
- `demux/src/av` の `AvSharedBacking` を実体化し、slot、active/stale `dataId`、client handle release状態を保持するようにした。
- `releaseAvHandle()` 判定順序を `AvHandleReleaseTxn` に固定し、fd付きhandle + `dataId=0` と empty handle + `dataId=0` を client handle release通知として扱う。
- `dataId=0` release で active slot を全解放しないこと、active slot release 後に stale dataId として遅延releaseを吸収することをテストで固定した。
- WP-05はFMQ / AV内部runtimeの実体化であり、AIDL公開API契約やARIB構文処理を新規定義しない。

## r50ed9

- WP-04を実施し、`demux/src/runtime` の `DemuxRuntime` / `FilterRuntime` / `DvrRuntime` / boundary transactionを実体化した。
- `SourceBoundaryTxn` は存在しないqueueを新規生成して成功扱いにせず、`QueueMissing` と typed outcome で返す。
- `GenerationBoundaryTxn` は packet pipeline の assembler / continuity / resync を reset し、demux generation を進める。
- `FilterConfigureTxn` / `DvrConfigureTxn` は snapshot / rollback outcome を型付きで保持し、configure失敗時に内部設定だけ新状態になることを禁止した。
- loom利用準備として、`external/rust/android-crates-io` が `libloom` を供給する環境で使う任意Soong defaults `maleicacid_tuner_hal2_loom_test_defaults` を追加した。通常testへは適用していない。

## r50ed8

- WP-03を実施し、`device/src/runtime` の backend tune 取引を実体化した。
- `BackendTuneTxn::apply()` に capture / stop / apply system / apply channel / start streaming / read status / commit と rollback を実装した。
- rollback失敗は `BackendTuneRollbackStep` と `BackendTuneRollbackReport` で型付き保持する。
- `FrontendTuneTxn::apply()` で worker起動失敗時に backend tune を rollback し、backend tune済みだけが残る状態を禁止した。
- `Px4Backend` の live reader は control fd 複製を表す `LiveReaderKind::Px4DuplicatedControlFd` に固定し、同一 chardev path の二重openを構造上モデル化しない。
- `FrontendRuntime` は backend kind、generation、live reader、last error を所有するようにした。

## r50ed7

- `README_JA.md` から r50ed5 / r50ed6 固有の補足を削除し、現行構造の使い始め説明に限定した。
- `tuner_hal2` の設計正本は既存 `tuner_hal/DESIGN_JA.md` と tv 直下 `開発規則.md` であり、README は履歴・設計判断の正本ではないことを明確化した。

## r50ed6

- WP-02を実施し、`resource_ledger` を単一 `BTreeMap<LedgerId, LedgerEntry>` 正本へ実体化した。
- `reserve`、`commit_live`、`begin_close`、`advance_cleanup_step`、`mark_cleanup_failed`、`commit_close`、`rollback_open`、`quarantine` を型付き遷移として追加した。
- cleanup step を `CleanupStep` enum で保持し、文字列stepを使わない構造にした。
- `FrontendLedger` / `DemuxLedger` / `FilterLedger` / `DvrLedger` / `DescramblerLedger` / `LnbLedger` を type alias ではなく resource kind を保持するwrapperへ変更した。
- begin前commit、terminal後commit、二重rollback、cleanup step、wrapper kind を単体テストで固定した。

## r50ed5

- WP-01を改訂し、既存フォルダ固定を前提にしない階層構造へ変更した。
- `frontend_dvb` / `frontend_px4` のトップレベルcrateを廃止し、`device/src/dvb` / `device/src/px4` / `device/src/runtime` へ集約した。
- `soft_demux` のトップレベルcrateを廃止し、parser断片を `demux/src/parser`、新しいruntime骨格を `demux/src/runtime`、AV共有メモリ骨格を `demux/src/av` へ集約した。
- `descrambler` は `core` と `runtime` を同一module内に置き、`descrambler_runtime` というトップレベル横並びmoduleを作らない構造にした。
- `resource_ledger`、`binder_adapter`、`service_runtime`、`lnb` の新規骨格を追加した。
- `tuner_hal2/DESIGN_JA.md` は引き続き置かない。設計正本は既存 `tuner_hal/DESIGN_JA.md` と tv 直下 `開発規則.md` とする。

## r50ed4

- WP-00後の残骸確認として、test-only の空構造体を状態付きtest helperへ変更した。
- 新規実装計画は外部Markdown `tuner_hal2_static_dynamic_structure_plan_r50ed4.md` を正とし、`tuner_hal2/DESIGN_JA.md` は引き続き置かない。

## r50ed3 WP-00 hygiene

- copied fragment hygiene を実施した。
- `HalError::InvalidArgument(String)` / `InvalidState(String)` / `Internal(String)` を廃止し、型付き kind と表示用 detail に分離した。
- `frontend_dvb` / `frontend_px4` の tune mapping は `HalInvalidArgumentKind` を返すように変更した。
- px4 mapping のテストから `err.contains(...)` によるエラーメッセージ文字列依存を削除した。
- `HalError` の `message: String` 直接保持を `HalErrorDetail` へ寄せた。

## r50ed2 copy-complete-fix

- copy-complete 再確認で見つかった parser 断片内のコピー不要要素を削除した。
- `RecordIndexParser` を空構造体ではなく処理済みpacket数を保持する型へ変更した。
- record index の `TsRecordEventData` 重複フィールドを削除した。
- `packet_pipeline` の診断文字列 `code: &'static str` を `PipelineDiagnosticKind` へ型付き化した。
- PES assembler の drop reason を文字列から `PesDropReason` enum へ型付き化した。
- `TsPacketValidator` 参照を `TsPacketView::validate()` へ修正した。

## r50ed2 copy-complete

- `tuner_hal2/DESIGN_JA.md` を削除した。`tuner_hal2` は新しい設計正本を持たず、既存 `tuner_hal/DESIGN_JA.md` と tv 直下の `開発規則.md` を正とする。
- r50ed2で不足していた DVB explicit scan 境界と DVB tune property mapping 断片を追加した。
- common に POSIX poll/ioctl/read ABI、Japan frequency helper、FrontendScanMode を追加した。
- `Android.bp` に追加断片を反映した。

## r50ed2

- r50ed で未達だったコピー対象断片を追加した。
- `soft_demux` から section / PES / packet pipeline / record index parser 断片を追加した。ただし旧 `DemuxHandle` 制御層はコピーしていない。
- `frontend_dvb` に Linux DVB / earth_pt1 ioctl ABI 断片を追加した。
- `frontend_px4` に px4_drv ioctl ABI 断片を追加した。
- `fmq_shim` の native C++ shim と Rust側 `FmqQueue` wrapper 断片を追加した。Rust wrapper は `NoNativeQueue` と fill/clear の単一variant型を整理してから配置した。
- 旧 `binder_service/src/tuner_hal.rs`、薄いTxn、空分類器、文字列 `contains()` による EventFlag wake 失敗分類はコピーしていない。
- r50ed2 時点でも Binder service / AIDL HAL 実装は未達である。

## r50ed

- `tuner_hal` 隣に `tuner_hal2` を追加した。
- `tuner_hal` 全体のコピーは禁止し、必要な実ロジック断片だけを取り込む方針にした。
- `common`、`descrambler`、`frontend_px4` の再利用断片を追加した。
- 旧制御層をコピーせず、`control` crate に worker / lifecycle / FMQ delivery / stream boundary の型付き骨格を追加した。
- r50ed 時点では Binder service / AIDL HAL 実装は未達である。
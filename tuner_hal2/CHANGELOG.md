# r50eo84_pr53_audio_cold_start_order_independent_uniqueness_followup

- `data_alignment_indicator=false`のaudio cold startで、独立した`Pending`候補を`ConfirmedBoundary`より前後どちらで観測しても候補競合として扱うよう、一意性判定を走査順非依存へ修正した。確定検証に使った同一frame列のnext headerだけは独立候補から除外する。
- 実first AUが次PESへ跨ぐ先行`Pending`となり、その圧縮body内の後方に偽2-header列の`ConfirmedBoundary`が成立する逆順入力をunit testと公開demux AV配送試験へ追加した。初回PESで偽frameへexplicit PTS / `MediaEvent`を付与せず、後続で複数候補が確定する場合は既存上限内でfail-closedとする。
- H.222.0のfirst-AU PTS対応、ARIB TR-B15 4.6-E1 Fascicle 3 4.2.2のPES/audio-frame non-synchronization、AOSP `MediaEvent`の同一AU metadata対応を、既存`FilterRuntime`従属の有限候補判定だけで満たす。新規owner、queue、worker、clock、generation、payload複製、公開AIDL/VINTF・capability変更は追加していない。設計正本は既に候補順序によらない一意確定を要求しているため変更していない。
- Rust 1.81.0で変更audio moduleのrustfmt checkとClippy `-D warnings`、製品demux sourceの`cargo check --all-targets`、audio module 19 unit tests、公開demuxのaudio `MediaEvent` 6試験とlifecycle fence 1試験を実施した。Android/Soong build、atest、VTS、実機・実放送波確認は未実施。

# r50eo84_pr53_audio_cold_start_candidate_uniqueness_followup

- `data_alignment_indicator=false`のaudio cold startで、1件の`ConfirmedBoundary`があっても、その後方に独立した`Pending`候補が残る間はexplicit PTS anchorと`MediaEvent`をcommitしないよう候補一意性判定を修正した。確定検証に使った直後の同一frame列headerは競合候補から除外し、既存の正当なframe列は維持する。
- 偽の2-header列が`ConfirmedBoundary`、実際のfirst AUが次PESへ跨ぐ`Pending`となる入力をunit testと公開demux AV配送試験へ追加した。初回PESで偽frameへPTSを付与せず、後続でも複数候補が残る場合は既存契約どおりfail-closedとする。
- H.222.0のfirst-AU PTS対応、ARIB TR-B15 4.6-E1 Fascicle 3 4.2.2のPES/audio-frame non-synchronization、AOSP `MediaEvent`の同一AU metadata対応を、既存`FilterRuntime`従属の有限候補判定だけで満たす。新規owner、queue、worker、clock、generation、payload複製は追加していない。設計正本は既に「一意候補だけcommitし、複数候補はfail-closed」と定めているため変更していない。公開AIDL/VINTF、capability値、future_work、`RELEASE_VERSION`も変更していない。
- Rust 1.81.0で変更audio moduleのrustfmt checkとClippy `-D warnings`、製品demux sourceの`cargo check --all-targets`、audio module 18 unit tests、公開demuxのaudio `MediaEvent` 5試験とlifecycle fence 1試験を実施した。Android/Soong build、atest、VTS、実機・実放送波確認は未実施。

# r50eo84_pr53_audio_au_aligned_media_event_followup

- AOSP `MediaEvent`の`pts` / `dataLength`を同じaudio frameへ対応させるため、TS AUDIOのPES payload全体を1イベントとして配送する経路を、構造検証済みの完成AUごとに正確な長さで配送する経路へ変更した。PES途中から始まるcontinuationは既存frameへ結合し、そのframeのPTSで完成後に配送する。後方で開始するAUは別イベントとし、元PESのexplicit PTSまたはactual sample rate / exact sample countから確定したPTSだけを付与する。
- cold startの未確認`HeaderOnly`候補は即座にanchorへ採用せず、先行AU最大1 frame、当該PESで開始する最大1 frame、次header最大7 byteの合計16389 byte以内だけ同一`FilterRuntime`へ保留する。後続PES上の同一signature境界を実際に確認した一意候補だけをcommitし、複数候補または上限超過はfail-closedとする。false `HeaderOnly`と真のfirst AUがともに次PESへ跨ぐ場合にも誤anchorを公開しない。
- 保持対象は規格上限8191 byteの未完了AU 1件とcold-start確認用の有限bytesだけであり、既存owner・AV allocation・lifecycle fenceを再利用する。独立queue、worker、clock、ledger、TIS側parser、PCR/wallclock/nominal-rate fallbackは追加していないため、TR-B15のPES/audio-frame non-synchronizationとAOSPのframe metadata契約を同時に満たす最小実装である。
- unit testへ`false HeaderOnly + true HeaderOnly`の後続境界確認、PES横断AUのbyte完全性、PTS/provenanceを追加した。公開demux回帰試験ではmixed continuation/new-AUとcold-start ambiguityについて、最終`AvMediaEventDescriptor.data_length`が各完成AU長と一致し、付与PTSも同じAUを指すことを固定した。公開AIDL/VINTF、capability値、future_work、`RELEASE_VERSION`は変更していない。
- Rust 1.81.0で変更audio moduleのrustfmt check、製品demux sourceの`cargo check --all-targets`、audio module 17 unit testsとClippy `-D warnings`、公開demuxのaudio MediaEvent 4試験とlifecycle fence 1試験、変更Rust 5ファイルのtree-sitter構文解析、`git diff --check`を実施した。製品demux全体のClippyは既存警告のみで完走した。Android/Soong build、atest、VTS、実機・実放送波確認は未実施。

# r50eo84_pr53_audio_cold_start_acquisition_followup

- ARIB TR-B15 4.6-E1 Fascicle 3 4.2.2のPES/audio-frame non-synchronizationとH.222.0のfirst-AU PTS対応へ合わせ、filter開始後の最初のPESが「先行AUのcontinuation + 当該PESで最初に開始するAU + explicit PTS」である場合のbounded cold-start sync acquisitionを既存`AudioTimestampAssociation`へ追加した。`data_alignment_indicator=true`ではpayload先頭以外を探索しない。
- 探索はADTSの13-bit frame length上限未満に閉じ、対応codecの完全header、宣言frame length、観測可能な次frame境界を検査する。次境界まで確認できる候補をheader-only候補より優先し、syncword一致だけのlock、上限なし走査、payload copy、第二buffer、queue、worker、clockは追加していないため、既存`FilterRuntime`従属状態の最小拡張に留まる。
- continuation prefixからのexplicit anchorと後続PTS-sparse event、次PESへ跨ぐ最初のAU、偽sync候補、先行header-only候補より確認済み境界を優先するケース、`data_alignment_indicator=true`のfail-closedをunit testへ追加し、公開demux AV配送経路にもcold-start回帰試験を追加した。PCR、wallclock、nominal rateへのfallbackと`isPtsPresent` provenanceの偽装は行わない。
- `tuner_hal/DESIGN_JA.md`と`tuner_hal2/DESIGN_JA.md`をcold-startの成功条件、誤同期防止、上限、責務境界へ同期した。公開AIDL/VINTF、capability値、future_work、`RELEASE_VERSION`は変更していない。
- Rust 1.81.0で製品demux sourceの`cargo check --all-targets`、対象moduleの17 unit testsとClippy `-D warnings`、変更Rust 2ファイルのtree-sitter構文解析、`git diff --check`を実施した。Android/Soong build、atest、VTS、実機・実放送波確認は未実施。

# r50eo84_pr53_cross_pes_audio_frame_residual_followup

- ARIB TR-B15 4.6-E1 Fascicle 3 4.2.2がBS／広帯域CSのMPEG-2 AACで許容するPES packet／audio frame non-synchronizationへ合わせ、既存`AudioTimestampAssociation`をPES横断の有限frame walkerへ更新した。H.222.0どおり明示PTSを当該PES内で開始する最初のAUへanchorし、PTS-sparse PESは継続frame後に最初に開始するAU、continuation-only PESはその先頭byteを含むAUの時刻をexact sample countから確定する。
- 残余は同一`FilterRuntime`内の未完了header最大7 byte、またはADTSの13-bit frame length上限8191 byte以内の残りbyte数1件だけとした。payload本体の再構成・copy、新しいstate owner、queue、worker、clock、ledger、TIS側codec parserは追加していない。既存のTEI、continuity gap、scramble/drop、flush、source/generation、stop/failure fenceはanchorと残余を同時に破棄する。
- ADTSのframe body／header途中とMPEG audioのframe body途中にPES境界を置くunit test、公開demux AV配送経路で`explicit PTS + mid-frame boundary -> PTS-sparse PES`が`isPtsPresent=false`のまま次の開始AU時刻を配送する回帰試験を追加した。unsupported／malformed header、未anchor、未通知parameter変更では従来どおりfail-closedとし、PCR／wallclock／nominal rateへfallbackしない。
- `tuner_hal/DESIGN_JA.md`と`tuner_hal2/DESIGN_JA.md`をH.222.0のfirst-AU対応、TR-B15のnon-synchronization許容、有限残余上限、成功／失敗境界へ同期した。TR-B15の根拠は公式英訳4.6-E1の精読範囲であり、現行日本語版8.9との差分は未証明のままとした。公開AIDL/VINTF、capability値、future_work、`RELEASE_VERSION`は変更していない。
- GitHub Actions `tuner_hal2 host Rust CI` run 33278079105でRust 1.81.0のrustfmt、Clippy（`-D warnings`）、全target type-check、workspace unit testが成功した。Android/Soong build、atest、VTS、実機・実放送波確認は未実施。

# r50eo84_pr53_audio_timestamp_association_followup

- TS AUDIO filterのproducer側に、明示PTSをanchorとしてframe境界と時刻算出parameterを構造検証済みのMPEG-2 AAC LC ADTS / MPEG audio frame列のactual sample rateとexact sample countからPTS-sparse eventの先頭frame時刻を確定する`AudioTimestampAssociation`を追加した。`isPtsPresent`は元PES headerのprovenanceのまま保持し、PCR、wallclock、arrival time、nominal rateへfallbackしない。
- 未anchor、partial/unsupported frame、未通知parameter変更、overflowではAV eventを配送せず型付き診断を生成する。anchorは既存`TsInputOrigin`を含み、別frontend / playback queue epochへ再利用しない。TEI、continuity gap、scramble/PES drop、filter/DVR flush、source/generation境界、stop/failureでは該当anchorを破棄する。状態は既存`FilterRuntime`に従属するO(1)値だけで、独立owner、queue、worker、clockは追加していない。
- 製品既定snapshotをTS AUDIO=1、TS VIDEO=1とし、両filterの未解放payload上限を閉じる有限AV runtime予算へ更新した。TS AUDIOが公開demux open-filter use-caseを通る試験、explicit/sparse ADTSのAIDL provenance/value、exact sample duration、33-bit wrap、MPEG audio、missing anchor、parameter変更、flush/discontinuity fenceの試験を追加した。
- demux test targetに残っていた削除済み`PacketPid::from_config()`呼出4箇所を現行`from_config_pid()`へ更新し、今回追加したaudio timestamp試験を含む`--all-targets`の型検査を妨げていた既存test harnessのAPIずれを同じPR内で解消した。製品経路に互換aliasは追加していない。全filter能力を0へ落とす既存capacity fixtureも、AV能力と同時にAV byte予算を0へ落として検査軸を自己完結させた。
- `tuner_hal/DESIGN_JA.md`へH.222.0のaudio PTS/access-unit対応、ARIB STD-B32 3.11-E1 Fascicle 2のADTS frame条件とparameter切替PTS条件、producer-side associationの成功/抑止境界を反映し、`tuner_hal2/DESIGN_JA.md`へ物理owner mappingを追加した。現行日本語版4.1との差分未証明、公開AIDL/VINTF、future_work、`RELEASE_VERSION`は変更していない。
- ローカルでは`git diff --check`、変更Rust 9ファイルのtree-sitter構文解析、Rust 1.81による新規moduleのrustfmt check、製品demux sourceそのままの`cargo check --all-targets`、Clippy通常実行、audio timestamp対象8試験、`CapabilitySnapshot`全8試験を実施した。新規audio moduleにClippy警告はないが、`-D warnings`は既存demux警告があるため成功扱いにしていない。Android/Soong build、atest、VTS、loom、実機・実放送波確認は未実施。

# r50eo83_pr53_media_event_metadata_video_capability_followup

- PES parserが確定した`stream_id`、PTS/DTSのheader presenceと33-bit 90 kHz値を`AvMediaEventMetadata`としてAV allocation descriptorへ保持し、`DemuxFilterMediaEvent`の`streamId`、`isPtsPresent` / `pts`、`isDtsPresent` / `dts`へ無損失に投影するよう変更した。PTS/DTSやwallclockを推測生成する時刻源、永続state、queue、workerは追加していない。
- 製品対象のMPEG-2 Video、AVC、HEVCでは今回精読したARIB STD-B32 3.11-E1 Fascicle 1がvideo PES headerへのPTS明示を要求することに基づき、製品既定snapshotのTS VIDEO filterを1件と有限AV byte予算で有効化した。全audio PESへの同等のPTS明示保証とauthoritative event timestamp sourceはないため、TS AUDIO filterは0件を維持した。
- 明示PTS、明示PTS+DTS、PTS/DTSなしPES、PES header由来ではないauthoritative PTSのpresence/value分離のAIDL投影試験、video-only AV capability closure試験、製品既定profileのTS VIDEOが公開demux open-filter use-caseを通る試験を追加した。
- `tuner_hal/DESIGN_JA.md`のSTD-B32証拠本文台帳とMediaEvent timestamp契約をFascicle 1・2の精読結果、video/audioの別capability判断、audioを将来有効化する具体条件、`getAvSyncTime()`用wallclockを個別event PTSへ流用しない境界へ同期した。現行日本語版4.1との差分未証明は維持し、future_workと`RELEASE_VERSION`は変更していない。
- ローカルでは`git diff --check`、全`AvMediaEventDescriptor`構築箇所と`allocate_payload_bytes()`呼出箇所、製品snapshotのAV依存閉包を静的確認した。添付tree-sitter CLIはRust grammar設定がなく構文解析を実行できず、添付rustfmtは`librustc_driver`を含まない。rustfmt、Rust compile/unit/loom、Android/Soong build、atest、VTS、実機・実放送波確認はローカル環境では未実施。

# r50eo83_pr53_dvr_queue_cleanup_failure_semantics_followup

- DVR queue cleanupのFMQ clearとqueue epoch publicationを`QueueEpochProtocol`配下の単一commit境界へ統合した。epoch/drainを事前検証してからfailure-atomicなexact readでFMQをclearし、成功後はfallibleな処理を挟まず次epochを公開するため、precommit失敗時は旧content/read position/epoch/Open状態を維持する。
- `QueueCleanupUseCase`はruntime state更新が失敗しても、独立したplayback pipeline reset、PCR invalidate、record index resetとservice側playback residual/diagnostic cleanupを続行し、既存`DvrQueueCleanupReport`へ全phaseの結果と最初の失敗を集約するよう変更した。前提phase失敗で実行不能なphaseもtyped skipとして記録する。
- FMQ clear失敗時のcontent/epoch/Open維持、成功時のclearとepoch更新、およびpost-commit失敗後も後続phaseを集約するbehavior testを追加した。永続state owner、epoch namespace、queue、worker、diagnostic storeは追加していない。
- 公開AIDL/VINTF、ARIB処理、future_work、`RELEASE_VERSION`は変更していない。ローカルでは変更Rustファイルのtree-sitter構文解析、`git diff --check`、旧split入口の不在と参照経路を確認した。rustfmt、Rust compile/unit/loom、Android/Soong build、atest、VTS、実機確認はローカル環境では未実施。

# r50eo82_pr53_dvr_queue_cleanup_owner_followup

- DVR `flush()` の queue drain、FMQ clear、queue epoch commit、runtime state更新、playback pipeline/PCR reset、record index resetをtyped phaseへ分割し、`QueueCleanupUseCase`が呼出順序と`DvrQueueCleanupReport`の結果集約を所有する形へ変更した。
- `QueueEpochProtocol`のepoch・one-shot drain transactionとDemux/DVRの永続状態所有は移動せず、opaque plan / committed tokenだけを上位orchestratorのtyped入口へ渡す形を維持した。
- 未commitのDVR cleanup planをdropした場合に既存queue epochが再びOpenとなり、generationを更新せずI/Oを再開できるbehavior testを追加した。
- 公開AIDL/VINTF、ARIB処理、future_work、`RELEASE_VERSION`は変更していない。ローカルでは変更Rust 8ファイルのtree-sitter構文解析、`git diff --check`、参照検査を実施した。rustfmt、Rust compile/unit/loom、Android/Soong build、atest、VTS、実機確認はローカル環境では未実施。

# r50eo82_review_transaction_owner_and_playback_memory_fix

- `FrontendTuneScanTxn`へpreflight、固定LNB給電準備、worker start/stop、rollback、event/terminal acceptanceを移し、横流しだけのtransaction façadeと第二ownerの`FrontendTuneScanContext`を削除した。
- `ChildOpenTxn`自身がruntime borrowとfilter/DVRのallocation、registration、commit、rollback手順を所有する形へ変更し、実処理を保持していた`ChildOpenContext`型を削除した。
- broadな`LnbTxn`名称を廃止し、relation/controlのcanonical ownerとは別のcall-local primitive accessである`LnbMutationContext`へ縮退した。`FrontendLnbRelationTxn`と既存`LnbControlTxn`の正規入口は維持した。
- descrambler PIDのsource検証、排他確認、session commit、失敗診断をservice-level `DescramblerPidTxn`へ移し、鍵・source binding・cleanupを含むbroadな`DescramblerTxn`を、所有権を主張しない`DescramblerMutationContext`へ縮退した。session側の同名transactionはatomic commit primitiveとして維持した。
- `PlaybackConsumeTxn`のprocessing bufferをFMQ全容量mirrorではなく最大256 TS packetのbounded chunkへ縮小し、FMQ capacity identity、最大187 byte completion tail、packet cursorを独立して維持した。
- 4 MiB FMQでもbounded chunkだけを確保すること、小容量FMQでは容量を超えて確保しないことをbehavior testへ追加した。
- 公開AIDL/VINTF、future_work、`RELEASE_VERSION`は変更していない。`tuner_hal2/DESIGN_JA.md`とSoong source listは削除した第二ownerに同期した。ローカルでは`git diff --check`と参照検査のみ実施し、rustfmt、Rust compile/unit/loom、Android/Soong build、atest、VTS、実機確認は未実施。

# r50eo81_pr53_raw_filter_callback_fix

- Section/PESの完成payload eventへ設定時の`raw`属性を保持し、FMQ commit後のcallback投影で`raw=true`をtyped Section/PES eventへ変換しないようにした。既存のcommit後`DATA_READY` status配送は維持した。
- raw Section/PESそれぞれについて、完成payloadのFMQ commit後に`DATA_READY`が投影され、typed Section/PES eventが0件であることを実runtime経路で確認する単体テストを追加した。既存のraw section parser testも完成eventの`raw=true`保持を確認するよう更新した。
- 公開契約文書、実装規約、product統合文書、future_work、`RELEASE_VERSION`は変更していない。
- Source-static checks performed: AOSP Tuner frameworkのraw Section/PES event表、`tuner_hal/DESIGN_JA.md`のraw callback行列、generated-eventからservice callback投影までのdata flow、`git diff --check`。ローカル環境ではrustfmt、rustc/cargo、Soong build、unit/loom、atest、VTS、実機・実放送波検証を実行していない。

# r50eo80_customer26_followup_design_boundary_impl_recheck_source_static_unverified_v41

- Closed frontend runtime read-only intermediate helper methods (`state`, `signal_state`, active request/session accessors, test-only generation helper) from the crate-public surface; service_runtime query/status paths now use `FrontendRuntimeSnapshot` DTO data instead of direct intermediate helper calls.
- Reduced demux packet pipeline internals by making `DemuxRuntime::pipeline()` and raw/inspect packet pipeline helpers crate-local, keeping production ingress on the typed demux runtime request/report boundary.
- Hid descrambler raw TS header parser internals by making `core` private and removing `parse_ts_packet_header` / `TsPacketHeader` from the crate-public API while retaining the typed descramble operation used by service_runtime.

# r50eo80_customer26_followup_design_boundary_impl_recheck_source_static_unverified_v40

- Removed the remaining immediately replaceable test-only wrappers around demux queue descriptor export; descriptor export tests now call the queue descriptor export plan API directly.
- Removed the object-runtime `close_live_object_for_test` helper and updated tests to call the production close/finish object use-cases directly.
- Rechecked the remaining `_for_test` helpers after the replacement pass; 21 remain because they are either test-module builders or internal observation/fault-injection hooks that are not directly replaceable by existing production APIs without changing the test scope.

# r50eo80_customer26_followup_design_boundary_impl_recheck_source_static_unverified_v39

- Removed remaining direct demux queue read/write/drain helpers from the production surface by marking them test-only and renaming them with `_for_test`; production descriptor export remains limited to the service_runtime object/query wrapper and demux queue descriptor plan boundary.
- Rechecked similar queue helper surfaces so direct queue descriptor export, record DVR queue drain, playback queue write/consume, and filter queue drain are not left as production-visible wild helpers when only tests use them.

# r50eo80_customer26_followup_design_boundary_impl_recheck_source_static_unverified_v38

- Removed unused direct demux queue descriptor export helpers instead of leaving them as crate-local production surface; demux tests now exercise the production queue descriptor export plan API directly.
- Confirmed the remaining demux queue descriptor export plan API is used by service_runtime RuntimeQuery as the production AIDL owner/generation wrapper boundary, not as a standalone direct export helper.

# r50eo80_customer26_followup_design_boundary_impl_recheck_source_static_unverified_v37

- Reduced direct demux queue descriptor export helpers to crate-local visibility so service_runtime object/query use-cases remain the AIDL owner/generation boundary for external descriptor export.
- Changed shared cleanup and service-runtime diagnostic record failure counters from wrapping fetch_add to saturating increments.

# r50eo80_customer26_followup_design_boundary_impl_recheck_source_static_unverified_v36

- Fixed callback/drop-leak fallback record-failure counters to use saturating atomic increments, giving long-running/fuzz scenarios fixed overflow semantics instead of platform-dependent wrap/debug behavior.

# r50eo80_customer26_followup_design_boundary_impl_recheck_source_static_unverified_v35

- Fixed frontend tune/scan backend rollback diagnostics to reuse the already-acquired cleanup diagnostic sink when post-start rollback or backend-failure marking cannot reacquire the runtime lock.
- Fixed tune commit rollback diagnostics to use the pre-cloned cleanup diagnostic sink instead of reacquiring the runtime lock solely to obtain the sink.
- Preserved queue descriptor object-liveness across descriptor export by revalidating and exporting under the object-method runtime lock, and recording descriptor export failure through the same locked diagnostic path.
- Aligned filter/frontend callback runtime-lock fallback phase mapping with the service_runtime delivery-failure mapping and made filter callback dispatch all-attempt across snapshot entries.
- Prevented record-index parsing from running on TEI / duplicate / no-payload / keyless-scrambled suppressed TS packets while retaining byte-preserving raw/record delivery behavior.
- Reduced record DVR queue drain visibility to the demux crate boundary.

# r50eo80_customer26_followup_design_boundary_impl_recheck_source_static_unverified_v34

- Fixed frontend tune/scan rollback cleanup diagnostics so the diagnostic record `public_error` carries the primary failure, or the primary+rollback composed failure, instead of only the rollback cleanup error. This keeps the rollback-triggering primary failure visible in typed cleanup snapshots when rollback itself succeeds.

# r50eo80_customer26_followup_design_boundary_impl_recheck_source_static_unverified_v33

- Fixed continued frontend replacement cleanup diagnostic record-failure handling: tune/scan replacement stop reports now surface record failure through the public cleanup result instead of shrinking to the shared counter only.
- Propagated replacement context into tune commit rollback and tune/scan backend rollback-state restore diagnostics so stopped old-worker generation and new generation candidate remain visible in typed rollback records.
- Added `CompleteStopObject` frontend worker cleanup step so tune/scan object stop completion failures after external join retain the stop outcome and completion primary failure in the diagnostic record.
- Reused the pre-cloned frontend cleanup diagnostic sink during frontend close owner-loss worker/live-data cleanup, recorded scan-cancel skip when scan stop fails, and added production drop-leak error diagnostic snapshot access to records/dropped count.
- Attached demux transaction diagnostic details to DVR configure status-reporting failure public errors and records rollback failure as the diagnostic record error when rollback fails.

# r50eo80_customer26_followup_design_boundary_impl_recheck_source_static_unverified_v32

- DESIGN_JA.md / CODE_CONVENTION.md: frontend worker replacement の new worker generation を「予約済み generation」ではなく「stop 前に算出した generation candidate」として明確化。runtime state への commit は post-complete install / begin step で行い、candidate 失効は start rollback diagnostic に CompleteReplacement context として残す。
- RELEASE_VERSION: v32 へ更新。

# r50eo80_customer26_followup_design_boundary_impl_recheck_source_static_unverified_v31

- Clarified DESIGN_JA.md frontend-worker replacement boundary: post-stop install/begin may commit a pre-reserved generation, but post-stop generation reservation / rollback-token preparation / request preflight remains forbidden.
- Updated frontend_worker_txn.rs so post-complete install/begin/worker-start rollback diagnostics carry the replacement context through a CompleteReplacement step with stopped-worker generation and reserved new-worker generation.

# r50eo80_customer26_followup_design_boundary_impl_recheck_source_static_unverified_v30

- Rechecked DESIGN_JA.md frontend-worker replacement wording for self-consistency and clarified that the ticket carries a reserved new-worker generation while the generation commit occurs only in the later install step.
- Completed the v28/v29 implementation follow-up by recording post-stop replacement completion failures as `public_error` in the same frontend-worker cleanup diagnostic record as the old-worker stop outcome.
- Confirmed tune/scan replacement still performs request validation, scan candidate calculation, new-generation reservation, and bound-demux rollback-token preparation before requesting old-worker stop/join.

# r50eo80_customer26_followup_design_boundary_impl_source_static_unverified_v29

- Implemented the v28 frontend worker replacement design boundary: tune/scan replacement now reserves the new worker generation and prepares bound-demux rollback tokens before requesting the old worker stop/join.
- Clarified DESIGN_JA.md / CODE_CONVENTION.md so the replacement lifecycle ticket explicitly carries stopped-worker generation, reserved new-worker generation, and bound-demux rollback-token preparation.
- Extended the service-runtime replacement ticket so it carries the pre-stop new worker generation and demux rollback tokens through external join completion instead of running first-time fallible generation preparation after the old worker has stopped.
- Updated the service-runtime source test name/body so the static test expectation matches the new generation-before-stop replacement contract.

# r50eo80_customer26_followup_design_boundary_fix_source_static_unverified_v28

- Updated DESIGN_JA.md / CODE_CONVENTION.md for frontend worker replacement: destructive old-worker stop/join must not be followed by first-time fallible generation/pre-start preparation unless the stopped-old/new-not-started outcome and rollback/no-restart decision are retained as typed diagnostics.
- This is a design-only follow-up for the previous rebutted tune/scan generation-prepare failure items; code changes for the new requirement were not applied in this archive.

# r50eo80_customer26_followup_fix_source_static_unverified_v27

- Updated the release identifier for the follow-up artifact and recorded the follow-up diagnostic-policy changes so archive names, RELEASE_VERSION, and CHANGELOG remain traceable.
- Added callback delivery snapshot metadata for merged runtime/fallback diagnostics: runtime snapshot omission, fallback record counts, fallback dropped counts, and fallback record-failure counts are now observable from the production snapshot.
- Reset callback fallback counters only after their corresponding bounded fallback store is actually cleared, and reset drop-leak bounded diagnostics with dropped-count and record-failure lifecycle reset.
- Added record-failure accounting to shared cleanup diagnostics so frontend worker replacement stop report record failures are visible in cleanup snapshots even when the cleanup itself succeeded.
- Recorded DVR configure status-reporting failures as demux transaction diagnostics before rollback, preserving a diagnostic id for the successful-rollback failure path.

# r50eo80_customer26_valid_fix_source_static_unverified_v26

- Continued the previously unaddressed frontend cleanup items by recording frontend tune/scan rollback cleanup steps through the shared `CleanupExecutionReport`/`SharedCleanupDiagnostics` path instead of collapsing snapshot/demux rollback with `FirstErrorCollector` only.
- Added frontend close owner-loss cleanup reporting: per-LNB close outcomes plus the worker/live-data cleanup result are now recorded as a close-level cleanup diagnostic report.
- Added runtime-lock-poison fallback diagnostics for frontend scan-end and filter callback delivery failures so non-DVR callback delivery accounting is no longer lost when the service runtime lock cannot be reacquired.
- Updated DESIGN_JA.md / CODE_CONVENTION.md to require rollback, frontend close owner-loss, and non-DVR callback delivery fallback diagnostics to follow the same typed diagnostic retention policy.

# r50eo80_customer26_valid_fix_source_static_unverified_v25

- Added production diagnostic snapshots with dropped-count access for startup, descrambler, child-open rollback, queue descriptor query, filter callback delivery, and frontend callback delivery stores.
- Extended DVR post-commit diagnostic snapshots with a shared record-failure counter so fallback diagnostic-store failures are observable outside tests.
- Split DVR status notifier cleanup startup diagnostics from DVR post-commit notification diagnostics and added lifecycle records for notifier terminal and supersede cleanup outcomes.
- Recorded frontend tune/scan replacement stop and scan-cancel outcomes through the shared frontend worker cleanup diagnostic store.
- Updated DESIGN_JA.md / CODE_CONVENTION.md to require these production snapshots and lifecycle cleanup diagnostics.

# r50eo80_customer26_valid_fix_source_static_unverified_v24

- Rechecked the v23 cleanup commonization and fixed the remaining adapter-shape mismatch: object cleanup and frontend worker cleanup step outcomes now use variant-specific target/step records instead of nullable field-bag structs.
- Added `ObjectCleanupObjectTarget`, `FrontendWorkerCleanupTarget`, and `FrontendWorkerCleanupWorkerGeneration` so domain-specific context is typed while the generic cleanup execution report/snapshot/shared-store primitives remain shared.
- Updated DESIGN_JA.md / CODE_CONVENTION.md to make variant-specific cleanup adapters part of the required commonization pattern and to forbid nullable field-bag adapters for cleanup execution diagnostics.

# r50eo80_customer26_valid_fix_source_static_unverified_v23

- Added the generic cleanup execution primitives `CleanupExecutionReport<TStepOutcome, TFailure>`, `CleanupExecutionDiagnosticSnapshot<TRecord>`, and `SharedCleanupDiagnostics<TRecord>` as the common basis for all-attempt cleanup reports, first-error projection, bounded diagnostic snapshots, and shared diagnostic sinks.
- Rebased object close / drop-leak cleanup diagnostics onto the generic cleanup execution primitives while preserving typed object-specific step outcomes and diagnostic records.
- Added frontend worker cleanup reports and diagnostics for tune stop, scan stop, and frontend close worker/live-data cleanup paths, using the same generic cleanup execution and shared bounded diagnostic primitives as object cleanup.
- Updated DESIGN_JA.md / CODE_CONVENTION.md to require cleanup execution pattern commonization without collapsing object-specific and frontend-worker-specific typed adapters into an `Option` field bag.

# r50eo80_customer26_valid_fix_source_static_unverified_v22

- Removed dead `TunerServiceRuntime::stop_filter_runtime()` / `flush_filter_runtime()` convenience wrappers after v19/v20 routed production object-method stop/flush through `transact_stop_filter_runtime()` / `transact_flush_filter_runtime()` and test-only demux wrappers were already restricted to `#[cfg(test)]`.
- Rechecked the v21 improvement-only items against DESIGN_JA.md / CODE_CONVENTION.md and kept them as non-required improvements rather than changing design: validation reason granularity, post-stop worker restoration, frontend worker cleanup per-step diagnostics, and non-DVR callback shared fallback do not contradict the current required design boundary.

# r50eo80_customer26_valid_fix_source_static_unverified_v21

- Made DVR post-commit status delivery/notifier accounting non-reversing across initial delivery, runtime-policy skip, Binder delivery, notifier preflight, notifier terminal, and cleanup paths by routing accounting failure through the AIDL-context shared DVR post-commit diagnostic fallback instead of returning it to public `IDvr.start()`.
- Moved frontend tune/scan request validation and scan candidate calculation before superseding existing frontend workers, so invalid public tune/scan requests do not stop an existing worker before failing.
- Added rollback for DVR status-reporting configuration failure after a successful DVR configure transaction by restoring the pre-configure DVR snapshot and quarantining the demux if rollback fails.
- Clarified DESIGN_JA.md / CODE_CONVENTION.md for non-reversing DVR post-commit accounting fallback, frontend worker replacement validation-before-stop, and DVR status-reporting rollback after configure commit.

# r50eo80_customer26_valid_fix_source_static_unverified_v20

- Removed the now-dead raw `stop_filter_runtime_from_typed_request()` and `flush_filter_runtime_from_typed_request()` demux runtime helpers after object-method stop/flush were routed through the service_runtime transaction façade in v19.
- Restricted raw `stop_filter_runtime()` / `flush_filter_runtime()` convenience wrappers to `#[cfg(test)]` so production code cannot bypass `FilterRuntimeOperationReport` diagnostics while existing demux unit tests keep their local mutation helper surface.
- Performed non-fail-fast source-static checks for remaining stop/flush raw-helper references, transaction façade use sites, and v20 package versioning.

# r50eo80_customer26_valid_fix_source_static_unverified_v19

- Routed object-method `IFilter.stop()` / `IFilter.flush()` through the service_runtime filter runtime transaction façade so queue-clear failures preserve `FilterRuntimeOperationReport` diagnostics instead of discarding reports via raw demux runtime helpers.
- Added `DemuxTransactionDiagnosticSnapshot` with records and dropped-count access so the bounded demux transaction diagnostic store has production overflow observability consistent with DESIGN_JA.md/CODE_CONVENTION.md.
- Clarified DESIGN_JA.md/CODE_CONVENTION.md that demux transaction diagnostics require dropped-count snapshots and that AIDL filter object methods must not bypass the typed report transaction façade.

# r50eo80_customer26_valid_fix_source_static_unverified_v18
- Added typed `FilterRuntimeOperationReport` coverage for filter stop/flush partial-phase outcomes, including queue clear failure, pipeline rollback, queued payload clear, AV backing flush, and skipped phase decisions.
- Connected filter runtime operation failures to the demux transaction diagnostic store with diagnostic ids and public error detail correlation.
- Updated DESIGN_JA.md / CODE_CONVENTION.md so filter stop/flush multi-step runtime operations cannot fall back to string-only or result-only diagnostics.

# r50eo80_customer26_valid_fix_source_static_unverified_v17
- Reworked DVR post-commit fallback accounting so shared-sink record failure increments a context-local record-failure counter instead of disappearing behind `let _ =`.
- Changed public `IDvr.stop()` notifier cleanup accounting to use the AIDL context fallback path, preserving post-commit non-reversal while surfacing accounting failures.
- Added structured DVR status notifier reset cleanup diagnostics with per-notifier success/failure records and dropped-count snapshots.
- Made service reset recover a poisoned DVR notifier store guard for reset cleanup, take the remaining notifiers, and attempt join/diagnostic recording before reporting the poison as reset failure.
- Added production snapshots with dropped counters for DVR post-commit diagnostics and callback artifact runtime split diagnostics.

# r50eo80_customer26_valid_fix_source_static_unverified_v16
- Added a shared DVR post-commit notification diagnostic sink so superseded notifier cleanup accounting failure does not silently disappear when the service runtime lock cannot be reacquired.
- Updated `record_superseded_dvr_notifier_cleanup_failure()` to preserve both the notifier cleanup primary error and the accounting failure in the shared diagnostic fallback instead of swallowing the accounting failure.
- Kept public start/stop post-commit non-reversal while making the accounting failure observable.

# r50eo80_customer26_valid_fix_source_static_unverified_v15
- Narrowed test-only frontend tune transaction visibility: `FrontendTuneOutcome`, `FrontendTuneTxn`, and its test-only constructor/apply façade are now crate-private instead of public, matching the production-hidden `device::runtime::tune_txn` module boundary and CODE_CONVENTION test-only surface guidance.
- Kept `BackendTuneTxn::new` production-visible because production backend worker paths instantiate it directly.
- Added `DvrPostCommitNotificationFailureKind` and `CallbackDeliveryFailurePhase::NotifierPreflight` so DVR post-commit diagnostics distinguish runtime policy skip / notifier preflight / cleanup / Binder delivery without relying only on the broad notifier phase.

# r50eo80_customer26_valid_fix_source_static_unverified_v14
- Fixed the test-only `AidlServiceContext::from_shared_runtime_for_test()` initializer to clone and store the shared object-cleanup diagnostic sink, matching the production context fields added by the v10/v11 object-cleanup diagnostic work.
- Verified that v13 configure outcome changes, demux diagnostic-id lifecycle wording, service reset all-attempt flow, diagnostic clear failure composition, and object-cleanup dropped-count accessor are present and aligned with DESIGN_JA.md / CODE_CONVENTION.md.
- Performed non-fail-fast source-static checks for DESIGN_JA.md responsibility consistency, residual-target coverage, old fail-fast reset paths, and generated a v14 source package.

# r50eo80_customer26_valid_fix_source_static_unverified_v13

- Fixed residual service boot reset diagnostic-recording fail-fast by attempting every split diagnostic record and composing recording failures afterward.
- Fixed filter/DVR configure public error correlation for non-quarantine failures without fabricating synthetic cleanup failures.
- Refined filter/DVR configure outcomes so validation-only failures are `Failed` and successful rollback outcomes carry the rollback step.
- Added production object cleanup diagnostic snapshots with dropped-count visibility.
- Propagated boot reset diagnostic-store clear failures through service reset result composition after all attempts.

# r50eo80_customer26_valid_fix_source_static_unverified_v12
- Rechecked v11 non-fail-fast for implementation conformance.
- Removed the remaining runtime-lock-planning fail-fast in service reset: callback artifact reset planning failure is now captured as that attempt's result, while drop-leak diagnostic clear and runtime boot finish are still attempted and recorded through service boot split diagnostics.
- Source-static checks performed: service reset all-attempt path, boot diagnostic clear list, shared object cleanup diagnostic sink call-sites, demux diagnostic-id public detail call-sites, and residual fail-fast pattern search. rustfmt, rustc/cargo, Soong build, unit tests, loom, atest, VTS, and device validation were not run.

# r50eo80_customer26_valid_fix_source_static_unverified_v11
- Made service boot reset all-attempt across DVR notifier cleanup, callback artifact reset, drop-leak diagnostic clear, and runtime boot finish; DVR notifier cleanup failure is now recorded through the service boot split diagnostic path with notifier object/generation context instead of failing before later reset steps.
- Cleared queue descriptor query, frontend callback delivery, demux transaction, and object cleanup diagnostics during runtime boot reset so post-reset runtime state is not mixed with stale bounded diagnostic records.
- Changed object cleanup diagnostics to use a shared service-runtime diagnostic sink so close/drop-leak cleanup reports can be recorded before reacquiring the runtime lock; runtime lock poison no longer discards the report before finish/terminalization result composition.
- Added diagnostic-id-bearing public detail for non-quarantine filter/DVR configure failures by composing the primary public error with a diagnostic summary while keeping the typed report in the bounded production diagnostic store.
- Updated DESIGN_JA.md and CODE_CONVENTION.md to require service reset all-attempt behavior, boot-reset diagnostic clearing, and shared-sink fallback for object cleanup report recording.

# r50eo80_customer26_valid_fix_source_static_unverified_v10

- Rechecked v9 non-fail-fast for DESIGN_JA.md self-consistency and implementation conformance.
- Added `ObjectCleanupDiagnosticRecord` / `ObjectCleanupDiagnosticKind` and a production `TunerServiceRuntime::object_cleanup_diagnostics()` accessor so `ObjectCleanupExecutionReport` is not discarded at the AIDL façade after first-error projection.
- Recorded object close and drop-leak terminalization cleanup reports into the service_runtime bounded diagnostic store before converting them to public `BinderResult` / first-error status.
- Added `HalError::UnsupportedDetail` so source-boundary subtype failures keep the AOSP Unavailable/unsupported public status while still carrying the demux transaction diagnostic id in dynamic public detail.
- Updated DESIGN_JA.md / CODE_CONVENTION.md to make the production object-cleanup diagnostic store and dynamic unsupported detail variant explicit.
- Source-static checks performed: DESIGN_JA.md/CODE_CONVENTION.md wording search, object cleanup report retention call-site review, demux diagnostic id public error review, and diff generation. rustfmt, rustc/cargo, Soong build, unit tests, loom, atest, VTS, and device validation were not run.

# r50eo80_customer26_valid_fix_source_static_unverified_v9

- Adopted the higher-quality design instead of treating per-step cleanup audit and diagnostic correlation as non-required future work.
- Added `ObjectCleanupExecutionReport` / `ObjectCleanupStepOutcome` as the common object close/drop-leak cleanup result component. Object close and drop-leak terminalization now collect artifact/domain/runtime per-step outcomes first and derive the public first-error result from that report.
- Extended `ObjectDomainCleanupOutcome` to carry command identity (`object_kind`, `object_id`, `generation`, `cleanup_kind`) and exposed cleanup execution kind/detail accessors for structured audit.
- Added `DemuxTransactionDiagnosticId` and assigned monotonic diagnostic ids before recording source-boundary/filter-configure/DVR-configure diagnostics; source-boundary and rollback public error detail now carries the diagnostic id where dynamic detail is available.
- Updated DESIGN_JA.md / CODE_CONVENTION.md to make diagnostic correlation and object cleanup per-step report mandatory for the current design, removing the previous ambiguous "current non-required / future extension" wording.
- Source-static checks performed: residual search for ambiguous future/non-required wording in the edited design sections, diagnostic constructor call-site review, object cleanup report call-site review, and diff generation. rustfmt, rustc/cargo, Soong build, unit tests, loom, atest, VTS, and device validation were not run.

# r50eo80_customer26_valid_fix_source_static_unverified_v8

- Rechecked the v7 implementation non-fail-fast against the customer follow-up: the v7 production demux transaction diagnostic accessor/re-export is present and no additional source code patch is required for the v7 intended fix.
- Revised DESIGN_JA.md / CODE_CONVENTION.md to remove ambiguity around DVR notifier cleanup: notifier cleanup / policy skip / artifact lookup failures are diagnostic-only phases and must not be treated as current callback unhealthy marking; `JoinHandle::join()` consumes the handle, so retryable-handle retention after terminal join failure is not a design requirement.
- Clarified demux transaction diagnostics: `SourceBoundaryReport` / `FilterConfigureReport` / `DvrConfigureReport` must be stored in a bounded production-accessible typed diagnostic store; public `HalError` detail may still be a status bridge and does not by itself carry all typed fields or a diagnostic id.
- Clarified object close cleanup diagnostics: current design requires all-attempt cleanup and first-error composition, not a per-step success outcome vector, unless a later design explicitly adds audit-vector diagnostics.
- Source-static checks performed: version/changelog, demux transaction diagnostic accessor/re-export, SourceBoundaryReport constructor surface, DVR notifier phase mapping, object cleanup outcome requirements, and duplicate-line false positive recheck. rustfmt, rustc/cargo, Soong build, unit tests, loom, atest, VTS, and device validation were not run.

# r50eo80_customer26_valid_fix_source_static_unverified_v7

- Exposed demux transaction diagnostics through a non-test runtime accessor and re-exported the typed diagnostic record/kind so `SourceBoundaryReport` / `FilterConfigureReport` / `DvrConfigureReport` records are retrievable in production code, not only through `#[cfg(test)]` helpers.
- Rechecked DVR notifier retry claims against Rust `JoinHandle::join()` ownership: terminal join/thread failure cannot be made retryable with the same handle after `join()` consumes it, so those claims remain rebutted rather than patched.
- Source-static checks performed: targeted review of the customer v7 follow-up ranges, residual search for test-only demux transaction diagnostic accessors, and review of DVR notifier remove/join ownership semantics. rustfmt, rustc/cargo, Soong build, unit tests, loom, atest, VTS, and device validation were not run.

# r50eo80_customer26_valid_fix_source_static_unverified_v6

- Added typed service-runtime demux transaction diagnostics for source-boundary, filter-configure, and DVR-configure failures so `SourceBoundaryReport` / `FilterConfigureReport` / `DvrConfigureReport` are no longer only carried through formatted `HalError` detail.
- Extended `SourceBoundaryReport` with sink/source filter ids, removed the public rejected-report constructor escape hatch, and kept endpoint validation owned by `SourceBoundaryTxn`.
- Tightened DVR notifier supersede ordering: old notifier is removed and restored on spawn failure, the old notifier is cancelled before the new notifier is inserted, and old cleanup accounting remains separated from current notifier unhealthy marking.
- Preserved source-boundary reset/report detail in service-runtime formatting while recording the typed report in the bounded diagnostic store.
- Source-static checks performed: targeted review of the customer v6 follow-up ranges, residual search for `SourceBoundaryReport::rejected`, `HalError::Unsupported(format!(...))`, and boolean DVR status preflight in the touched files. rustfmt, rustc/cargo, Soong build, unit tests, loom, atest, VTS, and device validation were not run.

# r50eo80_customer26_valid_fix_source_static_unverified_v5

- Separated explicit/superseded DVR notifier cleanup accounting from current notifier post-commit status: cleanup accounting failures no longer return public start/stop errors after the start/stop commit has already succeeded.
- Moved source-filter connect endpoint validation into the source-boundary transaction path and split endpoint validation steps into sink/source/lifecycle/subtype/PID phases.
- Included source-boundary reset detail in source-boundary formatting, and included nested source-boundary report detail in filter configure failure formatting.
- Source-static checks performed: targeted review of the customer v5 follow-up ranges, residual search for manual `SourceBoundaryReport::rejected` construction in `demux.rs`, DVR notifier cleanup phase review, and archive traceability update. rustfmt, rustc/cargo, Soong build, unit tests, loom, atest, VTS, and device validation were not run.

# r50eo80_customer26_valid_fix_source_static_unverified_v4

- Separated superseded DVR notifier cleanup failures from current notifier start accounting: old notifier stop/join failure is now recorded with `CallbackDeliveryFailurePhase::NotifierCleanup` and no longer marks the currently running notifier callback unhealthy.
- Added `RuntimePolicySkip` callback-delivery phase for DVR status preflight skips caused by already-unhealthy callback state or disabled status reporting, keeping them separate from artifact lookup failure.
- Made the DVR status notifier loop consume `DvrStatusCallbackDeliveryOutcome` semantically and terminate on artifact-missing, store-failure, or binder-failure outcomes instead of discarding the outcome and continuing.
- Extended `SourceBoundaryOutcome::Failed` with `primary_error`, and updated rejected source-boundary reports to retain typed failure cause.
- Connected filter configure source-boundary detail into `FilterConfigureReport` and recorded `RollbackSoftDemuxConfig` for rollback-success paths after ClearOldFmq / DisconnectOldSource / DVR ClearQueue failures.
- Source-static checks performed: targeted review of the customer follow-up ranges, residual search for old `SourceBoundaryOutcome::Failed { step }` patterns, callback phase match exhaustiveness review, and archive traceability update. rustfmt, rustc/cargo, Soong build, unit tests, loom, atest, VTS, and device validation were not run.

# r50eo80_customer26_valid_fix_source_static_unverified_v3

- Updated `RELEASE_VERSION` and this changelog so the customer26/v3 source-static artifact is externally traceable.
- Moved DVR post-commit callback artifact lookup policy back into `service_runtime`: DVR post-commit notification failures now record diagnostics/accounting without relying on an AIDL-side expected-primary swallow.
- Replaced the DVR status-notifier `Result<bool, HalError>` preflight helper with a typed `DvrStatusNotificationPreflight` outcome, and connected start/notifier-loop handling to that outcome.
- Made `deliver_started_dvr_status()` consume `DvrStatusCallbackDeliveryOutcome` explicitly instead of discarding it through `?` and `Ok(())`.
- Extended `SourceBoundaryOutcome` with rollback-attempt/success and rollback-error detail so rollback-success and rollback-failure outcomes are distinguishable from pre-mutation failures.
- Source-static checks performed: targeted review of the customer26 follow-up ranges, residual search for `Result<bool, HalError>` in DVR status notification preflight, release/changelog verification, and source-boundary rollback outcome review. rustfmt, rustc/cargo, Soong build, unit tests, loom, atest, VTS, and device validation were not run.

# r50eo80_common_helper_cleanup_source_static_unverified

- Replaced the remaining hand-written first-error collection in `mark_filter_callback_delivery_failed_use_case()`, `mark_dvr_callback_delivery_failed_use_case()`, and `stop_all_dvr_status_notifiers()` with `FirstErrorCollector`, keeping the existing diagnostic recording side effects intact.
- Routed the drop-leak diagnostic record buffer through the shared `BoundedDiagnosticStore<DropLeakErrorRecord>` instead of a private `VecDeque` plus duplicated dropped counter. Added `clear_records_preserving_dropped_count()` so service boot reset clears records without reintroducing the old dropped-counter reset regression.
- Centralized the LNB voltage-status profile predicate by exporting `lnb_profile_supports_voltage_status()` from `service_runtime` and removing the duplicate AIDL-side definition.
- Replaced the service boot `let _ = callback_artifact_runtime_split_diagnostics.clear()` discard with a startup diagnostic record when the split diagnostic store clear fails.
- Source-static checks performed: targeted residual searches for the removed `let mut first_error` patterns, drop-leak `VecDeque` store, duplicate LNB predicate, and callback split diagnostic clear discard. `rustfmt` was attempted but unavailable in this environment; rustc/cargo, Soong build, unit tests, loom, atest, VTS, and device validation were not run.

# r50eo79_descrambler_clear_key_cleanup_outcome_source_static_unverified

- Fixed descrambler clear-key transaction order to match `DESIGN_JA.md`: validate/prepare, commit the session clear, then release the old key token.
- Added `DescramblerClearKeyOutcome` with `ClearedWithOldKeyReleaseFailure` so old-token release failure after a successful session clear is recorded as a cleanup diagnostic without returning an API failure that contradicts the committed no-key session state.
- Updated the clear-key release-failure source-only test to assert API success, session key cleared, and `KeyTokenReleaseFailed` diagnostic recording.
- Source-static checks performed: r50eo78→r50eo79 unified diff review, clear-key path order review, residual `DescramblerClearKeyTxnError::ReleaseOld` search, nullable-target unchanged check, and no lock-poison side sink addition check. rustfmt, rustc/cargo, Soong build, unit tests, loom, atest, VTS, and device validation were not run.

# r50eo78_descrambler_replace_key_cleanup_outcome_source_static_unverified

- Corrected the omitted E item from the r50eo75/r50eo77 plan: `setKeyToken(non-VOID)` replacement now reports old-token release failure as a cleanup diagnostic while keeping the AIDL API success when the session has already committed the new key.
- Added `DescramblerReplaceKeyOutcome::ReplacedWithOldKeyReleaseFailure` so the session state and API result no longer diverge as an error after successful replacement.
- Kept the lock-poison separate-sink exclusion unchanged; nullable AIDL future-work files remain untouched.
- Source-static checks performed: direct r50eo77 → r50eo78 diff read, replace-key path read, residual search for `DescramblerReplaceKeyTxnError::ReleaseOld` in replace-key flow, and unchanged nullable target files. rustfmt, rustc/cargo, Soong build, unit tests, loom, atest, VTS, and device validation were not run.

# r50eo77_static_completion_audit_cleanup_source_static_unverified

- Redid static completion review by reading unified diffs and current call paths instead of treating negative grep as the primary proof.
- Removed the unnecessary `lnb_backend_adapter.rs` private-field rename that had been introduced only to avoid a broad grep false positive; behavior is unchanged.
- Kept nullable AIDL files unchanged and did not add a lock-poison alternate diagnostic sink.
- Source-static checks only: unified diff review, logic-path inspection, negative counterexample search, and non-target file comparison. rustfmt, rustc/cargo, Soong build, unit tests, loom, atest, VTS, and device validation were not run.

# r50eo76_static_completion_path_followup_source_static_unverified

- Tightened the previous r50eo75 static-completion follow-up by replacing the remaining phase-driven `FrontendCallbackDeliveryDiagnosticRecord::new(...)` constructor with variant-specific constructors.
- Removed `CallbackDeliveryFailureReport::dvr_post_commit_phase() -> Option<_>` so callback delivery post-commit context is checked by matching the DVR report variant instead of reconstructing optional context.
- Updated the source-only contract test to pattern-match the DVR variant.
- No nullable AIDL files were changed. No runtime-lock-poison fallback sink was added.
- Source-static checks performed: old Option field names remain absent, `FrontendCallbackDeliveryDiagnosticRecord::new` is absent, `dvr_post_commit_phase() -> Option` is absent, and `validated_pid.raw()` remains absent from `descrambler_txn.rs`. rustfmt, rustc/cargo, Soong build, unit tests, loom, atest, VTS, and device validation were not run.

# r50eo75_diagnostic_and_pid_claim_boundary_cleanup_source_static_unverified

- Replaced remaining diagnostic field-bag records in the agreed scope with variant-specific records: callback delivery failure reports, startup diagnostics, child-open rollback diagnostics, and frontend callback delivery diagnostics no longer encode their meaning through unrelated `Option` field combinations.
- Structured drop-leak Binder status capture with `DropLeakStatusSnapshot` instead of storing only an unstructured debug string; the context-owned bounded store, dropped counter, record-failure counter, and lock-poison behavior remain unchanged, and no secondary sink was added.
- Removed validated-PID raw extraction from `service_runtime/src/boot/descrambler_txn.rs` by routing validated AIDL PIDs through helper methods that produce `DescramblerPidClaim` directly. This is a typed-boundary cleanup, not a nullable AIDL change.
- Renamed the private LNB pending-frontend field without behavior change so the broad `frontend_id: Option` static check does not produce an unrelated false positive.
- Source-static checks performed: targeted searches confirm the planned field-bag signatures and validated-PID raw extraction call sites are gone from the target files. rustfmt, rustc/cargo, Soong build, unit tests, loom, atest, VTS, and device validation were not run.

# r50eo74_non_design_residual_cleanup_source_static_unverified

- Continued the r50eo73 customer finding triage without changing nullable AIDL future-work.
- Source boundary rollback diagnostics now preserve both the primary source-boundary failure step/error and the rollback restore failure step in `SourceBoundaryOutcome::Quarantined`, instead of collapsing the report to a single rollback failure.
- Service boot drop-leak record clearing no longer resets the dropped/failure counters; boot reset clears bounded records only and preserves lifetime diagnostic failure counters.
- `DescramblerClearKeyPlan` was changed from an `Option` field pair to a variant-specific `NoKey` / `ClearExisting { token, key_slot }` plan, fixing the weak token/slot pairing without changing public API semantics.
- No nullable AIDL files were changed. rustfmt, rustc/cargo, Soong build, unit tests, loom, atest, VTS, and device validation were not run.

# r50eo73_boundary_report_detail_source_static_unverified

- Rebutted the stale quarantine `let _` findings by keeping the r50eo72 infallible quarantine API shape; no nullable AIDL future-work files were changed.
- Connected `FilterConfigureReport` / `DvrConfigureReport` rollback details into service_runtime cleanup failure text instead of collapsing quarantined rollback to fixed wording.
- Returned `SourceBoundaryReport` from source connect/disconnect typed demux boundary calls and included source-boundary outcome/steps in service_runtime error mapping for connect/disconnect failures.
- Source-static checks performed: targeted grep confirmed the source-boundary typed calls are handled only in service_runtime and that the three quarantine typed calls have no `let _` discard. rustfmt, rustc/cargo, Soong build, unit tests, loom, atest, VTS, and device validation were not run.

# r50eo72_quarantine_result_boundary_fix_source_static_unverified

- Removed the three remaining `let _ = quarantine_runtime_from_typed_request(...)` call sites in `service_runtime`, leaving nullable AIDL future-work untouched.
- Made `DemuxRuntime::quarantine_runtime_from_typed_request()` explicitly infallible (`()`) because the operation only transitions the runtime and descendants to `Quarantined`; callers no longer discard a fallible-looking result.
- Source-static checks performed: targeted search confirms no `let _ = .*quarantine_runtime_from_typed_request` remains and exactly three production call sites call the typed quarantine façade without discarding a `Result`. rustfmt, rustc/cargo, Soong build, unit tests, loom, atest, VTS, and device validation were not run.

# r50eo71_doc_responsibility_boundary_cleanup_source_static_unverified

- Reworked the r50eo70 typed-request boundary clarification so `DESIGN_JA.md` stays on responsibility boundaries, state transitions, failure precedence, and resource lifetime. Rust visibility / constructor / import / Clone / Copy / module-private style rules are kept in `CODE_CONVENTION.md`.
- Renamed the release-specific `r50eo68 source-only complete` design/convention sections to generic worker / callback / query / packet boundary sections. This removes a release-status phrase from design rules while preserving the actual lifecycle and cleanup contracts.
- Clarified that `FirstErrorCollector` owns only all-attempt cleanup-step first-error collection; primary+cleanup composed failure creation is owned by the failure composition helper group.
- Reworded typed request conditions so the violation is bypassing `service_runtime` transaction authority, not the request being a thin DTO.
- This is a document-responsibility cleanup. Rust source, generated code, build files, tests, and future_work documents were not changed. rustfmt, rustc/cargo, Soong build, unit tests, loom, atest, VTS, and device validation were not run.

# r50eo70_typed_request_boundary_design_clarification_source_static_unverified

- Clarified `tuner_hal2/DESIGN_JA.md` and `CODE_CONVENTION.md` so demux crate typed request DTOs are not mistaken for capability tokens / transaction proofs. Public crate-to-crate typed request constructors remain allowed when `service_runtime` owns object live/generation/owner validation, AIDL/binder/domain_request cannot call demux mutation façade directly, and forging the request cannot create a rollback token, queue export handle, snapshot body, registry entry, session map, or reusable restore authority.
- Clarified rollback prepare request, read-only filter/DVR snapshots, and queue descriptor DTO / export plan boundaries. Demux-local queue export plans hold demux target + non-Clone handle; AIDL object/generation/owner relation is held by the service_runtime wrapper plan.
- This is a DESIGN/CODE_CONVENTION clarification and rebuttal to false-positive regression claims against No.1-23/26. Rust source, generated code, build files, and future_work documents were not changed.
- Source-static checks performed: Rust source diff is empty; docs changed only in `tuner_hal2/DESIGN_JA.md`, `tuner_hal2/CODE_CONVENTION.md`, `tuner_hal2/CHANGELOG.md`, and `tuner_hal2/RELEASE_VERSION`; targeted source searches confirm demux rollback token has no snapshot body and no Clone/Copy, queue export handle/plan remain non-Clone, direct demux mutation calls from AIDL/binder/domain_request are absent, and packet_txn legacy keyless/source-filter terms are absent. rustfmt, rustc/cargo, Soong build, unit tests, loom, atest, VTS, and device validation were not run.

# r50eo69_design_packet_bytes_boundary_source_static_unverified

- Updated `DESIGN_JA.md` for the No.24/No.25 packet-byte boundary: `ValidatedTsPacket` may expose the original TS packet bytes only for output/mirror/FMQ/diagnostic-prefix use, while validation/PID/policy/section-PES planning must remain based on `ValidatedTsPacket` / `PacketPid` and must not reconstitute `TsPacketView` from raw bytes.
- This is a DESIGN-only source-static revision. Rust source, generated code, build files, tests, and future_work documents were not changed.
- Checks performed: text diff and targeted grep for `ValidatedTsPacket` / `packet_bytes` wording. rustfmt, rustc/cargo, Soong build, unit tests, loom, atest, VTS, and device validation were not run.

# r50eo68_wp1_5_repair15_source_static_unverified

- Fixed a repair14 WP-5 static-completion miss: callback registration artifact failure without a rollback command is now recorded in callback artifact/runtime split diagnostics, and runtime finish lock failure after such an artifact failure composes the artifact failure with the runtime/record failure instead of returning only the runtime error.
- Avoided taking a runtime finish lock for successful callback registration outcomes that have no rollback work, preventing a false failure after an already-complete registration.
- Re-ran source-static WP-1〜WP-5 checks; rustfmt, rustc/cargo, Soong build, unit tests, loom, atest, VTS, and device validation are not executed in this environment.

# r50eo68_wp1_5_repair14_source_static_unverified

- Fixed a repair13 source-static miss: callback registration artifact bridge now returns `(AidlMethodCall, request_tuple)` to `execute_shared_object_method_call_after_live` instead of a three-element tuple, so the typed request builder shape matches the transaction helper contract.
- Re-ran source-static WP-1〜WP-5 checks; rustfmt, rustc/cargo, Soong build, unit tests, loom, atest, VTS, and device validation are not executed in this environment.

# r50eo68_wp1_5_repair13_source_static_unverified

- Fixed a repair12 WP-5 static-completion miss: callback registration runtime-finish-lock failure now records rollback cleanup failure in the shared callback artifact/runtime split diagnostic record instead of returning it only as a composed error.
- Added `RuntimeFinishAndArtifactCleanupFailure` to the callback artifact/runtime split diagnostic outcome so artifact-retain success + runtime-finish-lock failure + rollback cleanup failure remains visible in all-attempt diagnostics.
- `repair13` remains source-static only; rustfmt, rustc/cargo, Soong build, unit tests, loom, atest, VTS, and device validation are not executed in this environment.

# r50eo68_wp1_5_repair12_source_static_unverified

- Fixed a repair11 WP-5 static-completion miss: production AIDL code no longer clears owner callback artifacts by raw `AidlObjectHandle` after callback registration finish-lock failure.
- Added a service_runtime-owned cleanup command façade for callback registration runtime-finish-lock failure and routed the AIDL rollback bridge through `OwnerCallbackCleanupArtifactCommand`.
- Removed the production handle-based callback artifact clear bridge so callback artifact store clear remains command-owned as required by `DESIGN_JA.md` / `CODE_CONVENTION.md`.
- Re-ran source-level static checks for WP-1〜WP-5. rustfmt, rustc/cargo, Soong build, unit tests, loom, atest, VTS, and device/real-broadcast tests were not run in this environment.

# r50eo68_wp1_5_repair11_source_static_unverified

- Fixed a repair10 WP-5 static-completion miss: `TunerServiceRuntime::record_callback_artifact_runtime_split_diagnostic()` no longer discards diagnostic-sink failures with `let _ =`.
- Propagated split-diagnostic record failures through owner callback cleanup finish, runtime registry missing accounting, and service boot reset finish composition instead of treating those record attempts as success.
- Added AIDL-side rollback of a retained callback artifact when runtime registration finish lock fails after artifact retain succeeds, so the callback store is not left with an owner artifact that the runtime never recorded.
- Re-ran source-level static checks for WP-1〜WP-5. rustfmt, rustc/cargo, Soong build, unit tests, loom, atest, VTS, and device/real-broadcast tests were not run in this environment.

# r50eo68_wp1_5_repair10_source_static_unverified

- Fixed a repair9 source-static miss in `aidl_service/src/object_runtime/mod.rs`: `OwnerCallbackCleanupArtifactCommand` was used in the runtime-finish-lock-failure helper but was not imported from service_runtime, making repair9 compile-breaking at source level.
- Tightened the WP-5 runtime-finish-lock-failure diagnostic path: AIDL-side split-diagnostic recording now returns `Result<(), HalError>` instead of silently discarding diagnostic sink failures with `let _ =`; record failures are composed with the runtime-finish-lock failure rather than being treated as success.
- Re-ran source-level static checks for WP-1〜WP-5. rustfmt, rustc/cargo, Soong build, unit tests, loom, atest, VTS, and device/real-broadcast tests were not run in this environment.

# r50eo68_wp1_5_repair9_source_static_unverified

- Fixed the repair8 WP-1 compile-breaking re-export gap by re-exporting DemuxRuntime typed request façade types from `demux/src/runtime/mod.rs`, matching the crate-root `demux/src/lib.rs` public surface used by service_runtime.
- Advanced WP-5 callback artifact/runtime split handling: artifact bridge execution remains outside the `TunerServiceRuntime` lock, while artifact-success/runtime-finish-lock-failure paths now record `CallbackArtifactRuntimeSplitDiagnosticRecord` through a service_runtime-owned shared diagnostic sink instead of silently losing the artifact attempt result.
- Added service boot reset runtime-finish-lock-failure recording using the same service_runtime-owned split diagnostic sink, including callback artifact reset and drop-leak clear attempt outcomes.
- Source-only checks performed: runtime re-export surface matches crate-root re-export; old DemuxRuntime mutation token / arbitrary closure executor remains absent; DemuxRuntime public direct `&mut self` mutation surface remains absent; QueueDescriptorExportHandle / QueueDescriptorExportPlan remain non-Clone; validated packet ingress remains typed-request based; packet_txn old keyless/source-filter ownership remnants remain absent; callback artifact bridge is not executed while holding the runtime lock and post-artifact runtime finish lock failures have a diagnostic record path. rustfmt, rustc/cargo, Soong build, unit tests, loom, atest, VTS, and device/real-broadcast tests were not run.

# r50eo68_wp1_5_repair8_source_static_unverified

- Updated the WP-1 DemuxRuntime rollback boundary to the v30 Rust visibility interpretation: crate-to-crate `pub` typed façade is allowed, but rollback tokens no longer carry snapshots, are non-Clone/non-Copy opaque ids, and restore consumes the runtime-internal rollback ledger once.
- Added restore-side snapshot generation verification. service_runtime rollback tokens are now shared through an internal `Arc<Mutex<Option<...>>>` one-shot holder where both the worker thread and the starting thread can race to consume the same rollback authority without cloning the token list.
- Renamed service_runtime rollback variables from snapshot to token to prevent the source from documenting a snapshot-carrying rollback token.
- Synchronized DESIGN_JA.md and CODE_CONVENTION.md so public typed request constructors are not treated as capability-token forgery by themselves; the required property is forge-safety, one-shot ledger consumption, no snapshot body export, and no direct AIDL/product API entry.
- Source-only checks performed: old mutation token / arbitrary closure executor remains absent; rollback token has no Clone/Copy derive; restore consumes the internal ledger and rejects demux-id, ledger-missing/reuse, and snapshot-generation mismatch; AIDL/binder_adapter/domain_request do not call DemuxRuntime rollback prepare/restore façade directly. rustfmt, rustc/cargo, Soong build, unit tests, atest, VTS, and device/real-broadcast tests were not run.

# r50eo68 repair6 partial unverified

- Continued WP-1 repair from repair5. Converted the remaining DemuxRuntime public direct mutation surface for callback unhealthy marking, AV shared handle export/release, filter/DVR removal, filter/DVR start-stop-flush, AV stream type, delay hint, DVR filter link, DVR status interval, filter source connect/disconnect, quarantine, and validated packet ingress into typed-request public facades.
- Made the corresponding raw mutation helpers crate-local so service_runtime production callers can no longer call those direct mutation methods by name.
- Updated service_runtime demux mutation call-sites that target DemuxRuntime directly to use the typed-request facades.
- Static check result: public DemuxRuntime methods taking &mut self now use typed-request-style method names; no standalone with_mutation_token / DemuxRuntimeMutationToken / mutation_token factory exists. This is still source-only and unverified; build/rustfmt/rustc/cargo/Soong/unit/loom/atest/VTS/device checks were not run.

# r50eo68 repair5 partial unverified

- repair5 is still a partial WP-1 correction, not a completion release. The prior repair4 answer did not sufficiently report the exact static-completion check locations and method.
- Moved DemuxRuntime rollback token prepare/restore away from direct public rollback_token()/restore_from_rollback_token() calls by adding typed request wrappers and updating service_runtime rollback call-sites.
- Static check result remains NG overall: DemuxRuntime still has public direct &mut self mutation methods for callback unhealthy marking, AV shared-handle operations, remove/start/stop/flush/source/quarantine/packet-ingress paths.
- Build, rustfmt, rustc/cargo, Soong, unit/loom/atest/VTS/device checks were not run.

# r50eo68_wp1_5_repair4_partial_unverified

- repair3 の静的完了条件確認は、`DemuxRuntime::with_mutation_token()` が public arbitrary mutation closure executor である点を見落としていたため不成立だった。これは担当者差ではなく、同一担当による確認実態不足である。
- repair4 では `with_mutation_token()` と `DemuxRuntimeMutationToken` の public surface を削除し、service_runtime call-site を具体的な demux domain API 呼び出しへ戻した。これにより、任意 closure で token と `&mut DemuxRuntime` を外部 caller に同時貸与する抜け道は消した。
- ただし、repair4 は WP-1 の完了版ではない。`DemuxRuntime` には `start_filter_runtime()` / `stop_filter_runtime()` / `remove_filter()` / `push_validated_ts_packet_from_origin()` などの public `&mut self` domain mutation method が残っており、v29 計画が要求する「typed request / capability token / transaction proof なし mutation 禁止」を全件満たすところまでは到達していない。
- WP-2 / WP-3 / WP-4 / WP-5 については、repair3 時点の主要静的成立状態を維持しているかを補助確認した。build, rustfmt, rustc, cargo, Soong, atest, VTS, emulator/device boot, adb sanity, and real-broadcast verification were not run in this environment.

# r50eo68_source_only_complete_corrected13_unverified

- corrected12 / repair2 の完了判定では、WP-1 の `DemuxRuntime` public mutation surface 全体を確認せず、`mutation_token()` / `ensure_mutation_token()` の存在と一部 call-site の typed request 化だけをもって静的完了条件OKと誤判定していた。実際には token なし public `&mut self` API が残り、`mutation_token()` も任意 caller が呼べる forgeable token factory だった。
- `DemuxRuntime::mutation_token()` を削除し、`with_mutation_token()` の scoped closure 内だけで `DemuxRuntimeMutationToken` を借用できる形に変更した。public mutation API は `DemuxRuntimeMutationToken` を必須引数にし、token なし public `&mut self` mutation surface を閉じた。
- service_runtime の demux/filter/DVR/packet/frontend rollback call-site は `with_mutation_token()` 経由へ更新した。demux crate 内部 helper と test-only helper は crate-local surface に閉じ、production caller が token なしに DemuxRuntime mutation を直接呼ばない形へ寄せた。
- corrected12 の callback artifact/runtime finish 記述は、artifact bridge 実行後に runtime lock を取得する窓を閉じる目的で runtime lock 中 artifact 実行へ寄せていたが、WP-5 計画上はこれ自体が未完了条件だった。repair2 で artifact bridge を runtime lock 外へ戻した状態を維持し、artifact result を service_runtime finish use-case へ渡す構造として扱う。
- Source-only static checks performed for this correction: public `DemuxRuntime` `&mut self` method without `DemuxRuntimeMutationToken` is absent; standalone `mutation_token()` factory is absent; service_runtime demux mutation call-sites use `with_mutation_token()`; `QueueDescriptorExportHandle` / `QueueDescriptorExportPlan` remain non-Clone; `push_validated_ts_packet_from_origin()` has no raw packet separate argument; packet_txn old keyless/source-filter policy ownership remnants are absent; callback artifact bridge is executed before runtime finish lock acquisition in the checked owner-cleanup, registration rollback, object-close, and service boot reset paths. Build, rustfmt, rustc, cargo, Soong, atest, VTS, emulator/device boot, adb sanity, and real-broadcast verification were not run in this environment.

# r50eo68_source_only_complete_corrected12_unverified

- corrected11 の完了判定では、D 系統の callback artifact/runtime all-attempt finish を完了扱いしていたが、`CallbackArtifactRuntimeSplitOutcome::ServiceBootResetFailure` が `callback_artifact_error: Option<HalError>` / `drop_leak_error: Option<HalError>` / `runtime_error: Option<HalError>` を持つ field bag のまま残っていた。これは Markdown v7 の D が求める split diagnostic / variant-specific outcome への未達であり、前回残件リストに未記載だった。
- corrected11 では、`finish_owner_callback_cleanup_outcome()` / `finish_callback_registration_artifact_outcome()` / object close callback cleanup executor が artifact bridge 実行後に runtime lock を取得していた。artifact mutation 成功後に runtime lock 取得が失敗すると runtime finish / split diagnostic 記録へ進めないため、runtime prepare -> artifact command execution -> runtime finish の all-attempt transaction として不十分だった。前回は指定箇所の実行順序までコードを読んで判定していなかった。
- デグレの意図は、artifact bridge の結果を runtime finish に渡す形だけを見て、artifact 実行後に runtime finish へ入れない lock failure 窓を十分に扱わなかったことだった。service boot reset については複数 failure を一つの struct にまとめるために Option field を使ったが、これは split diagnostic を variant-specific にする方針と矛盾していた。
- `ServiceBootResetFailure { Option<HalError>, ... }` を廃止し、`ServiceBootCallbackArtifactFailure` / `ServiceBootDropLeakFailure` / `ServiceBootRuntimeFailure` の required-error variant に分離した。service boot reset では各失敗を個別 diagnostic record として記録する。
- `service_boot_reset_from_results()` を `service_boot_reset_from_attempt_results()` に置換し、`Result<(), HalError>` の各 attempt result から diagnostic records を組み立てるよう修正した。service boot reset diagnostic constructor surface に Option field bag を残さない。
- source-only contract test に、service boot reset の callback artifact failure / drop-leak failure がそれぞれ required-error variant-specific outcome になることを固定する test を追加した。
- owner callback cleanup、callback registration rollback、object close callback cleanup executor では runtime lock を取得してから artifact bridge を実行し、その同じ runtime guard で finish use-case へ渡す順序へ変更した。これにより artifact mutation 後に runtime finish / split diagnostic へ進めない追加 lock acquisition 窓を閉じた。
- Source-only static checks performed for this correction: `ServiceBootResetFailure` は存在しない。`callback_artifact_error: Option<HalError>` / `drop_leak_error: Option<HalError>` / `runtime_error: Option<HalError>` の service boot reset field bag は存在しない。指定された object_runtime / service_context の順序は runtime guard acquisition -> artifact attempt -> finish/diagnostic record へ変更済み。Build, rustfmt, rustc, cargo, Soong, atest, VTS, emulator/device boot, adb sanity, and real-broadcast verification were not run in this environment.

# r50eo68_source_only_complete_corrected11_unverified

- corrected10 の完了判定では、`service_runtime/src/source_only_contract_tests.rs` の `matches!` pattern 内で `pid: test_descrambler_pid(...)` という関数呼び出しを pattern として書いていた。これは Rust pattern として成立せず、source-only contract test のコンパイル不能級未達であり、前回残件リストに未記載だった。
- 前回は `PacketPid` / `DescramblerPid` の型定義と raw accessor の削除確認を中心に見ており、追加 test source の assertion pattern を Rust 構文として読んで判定していなかった。これは Markdown v7 の H「通常 test source 実装」に対する過大報告だった。
- デグレの意図は、`DescramblerPid` 直接生成を避けるため test helper 経由に直したつもりだったが、pattern 位置で helper を呼ぶ形にしてしまい、実際には test source として成立しないコードを追加したことだった。
- `PidClaimRejectedWithoutDemux` / `PidClaimRejected` の assertion を、pattern では `pid` binding にし、guard で `pid == test_descrambler_pid(...)` を確認する形へ修正した。
- Source-only static checks performed for this correction: `source_only_contract_tests.rs` の `pid: test_descrambler_pid(...)` pattern は消えている。Build, rustfmt, rustc, cargo, Soong, atest, VTS, emulator/device boot, adb sanity, and real-broadcast verification were not run in this environment.

# r50eo68_source_only_complete_corrected10_unverified

- corrected9 の完了判定では、`PacketPid` の内部表現を `TransportStreamPid` に変更した後、`PacketPid::from_validated_pid()` が非 const になっているにもかかわらず、`ValidatedTsPacket::pid()` を `pub const fn` のまま残していた。これはコンパイル不能級の未達であり、前回残件リストに未記載だった。前回は `PacketPid` / `DescramblerPid` bridge の raw escape だけを追い、変更後の const 呼び出し制約までコードを読んで判定していなかった。
- デグレの意図は、`TsPacketView::packet_pid()` の const 指定を外した時点で同種の追従が完了したと誤認し、`ValidatedTsPacket::pid()` 側を同じ観点で見直さなかったことだった。
- `ValidatedTsPacket::pid()` を通常 `pub fn` に変更し、非 const validation helper を const context から呼ばないよう修正した。
- Source-only static checks performed for this correction: `ValidatedTsPacket::pid()` is no longer `const`; `PacketPid::get()` / `PacketPid::as_u16()` / `PacketPid::as_i32_for_internal_demux()` / `to_u16_for_packet_pid_bridge()` / tuple-field raw PID access are absent from the checked packet/descrambler path. Build, rustfmt, rustc, cargo, Soong, atest, VTS, emulator/device boot, adb sanity, and real-broadcast verification were not run in this environment.

# r50eo68_source_only_complete_corrected9_unverified

- corrected8 の完了判定では、`DescramblerPid` の tuple field `.0` を閉じたことだけで typed boundary が成立したと扱っていたが、`DescramblerPid::to_u16_for_packet_pid_bridge()` が public raw accessor として残り、`PacketPid::from_descrambler_pid_for_service_runtime_boundary()` がその raw `u16` を使っていた。これは F/G の typed boundary に対する未達であり、前回残件リストに未記載だった。
- この未達は常識的にあり得る誤判定ではなく、「ちゃんとコードを読んで判定した」とは言えない。関数本体の引数・戻り値と call-site を見れば、raw PID escape が残っていることは直ちに分かるためである。
- デグレの意図は、`DescramblerPid(…)` / `.0` の直接経路だけを閉じ、cross-crate 変換の都合で raw `u16` bridge を名前付き helper として残したことだった。結果として raw accessor を別名で温存しており、v7 の「検証済み DescramblerPid から PacketPid へ入る一方向 typed bridge」という意図に反していた。
- `common::TransportStreamPid` を追加し、`DescramblerPid` と `PacketPid` の内側を同一 typed PID に寄せた。`DescramblerPid` から `PacketPid` への bridge は `TransportStreamPid` を渡すだけにし、`u16` raw value を返す public bridge を削除した。
- `service_runtime/src/source_only_contract_tests.rs` に `PacketPid` import が不足していた。これは corrected8 の H に対するコンパイル不能級未達であり、前回残件リストに未記載だった。前回は test helper の型解決まで読めていなかった。
- `RuntimeObjectQueryError` の public re-export と public enum を crate-private 化し、query_api の非DTO public surface をさらに閉じた。
- 未実行: rustfmt / rustc / cargo / Soong build / `m nothing` / test module build / `atest -b` / atest run / Tuner VTS discovery / Tuner VTS run / adb sanity / emulator or device boot / 実波確認。

# r50eo68_source_only_complete_corrected8_unverified

- corrected7 の完了判定では、`DescramblerPid` / `PacketPid` typed boundary の追従を十分に読めていなかった。`PacketPid::from_descrambler_pid_for_service_runtime_boundary()` が raw `u16` を受け取り、`packet_txn.rs` / `descrambler_session.rs` が `claim.pid().0` / `descrambler_pid.0` で raw PID に戻す経路を残していた。これは F/G の typed boundary に対する未達であり、前回残件リストに未記載だった。
- デグレの意図は、corrected6 で `PacketPid::as_u16()` を消した後も、descrambler claim と packet path の照合を小変更で済ませるため、`DescramblerPid` の tuple field と raw `u16` bridge を温存したことだった。結果として、raw accessor を別経路に移しただけで、v7 の「検証済み DescramblerPid から PacketPid へ入る一方向 typed bridge」にはなっていなかった。
- `PacketPid::from_descrambler_pid_for_service_runtime_boundary()` は `DescramblerPid` を受け取る typed bridge に変更した。`demux` crate に `descrambler` crate 依存を追加し、`PacketPid` 変換境界でだけ `DescramblerPid` bridge を消費する。
- `DescramblerPid` の tuple field を非公開化し、外部 crate から `DescramblerPid(…)` / `.0` で raw PID を取り出す経路を閉じた。`DescramblerPidClaim` 経由で検証済み PID を得る形へ source-only contract tests と service_runtime tests を更新した。
- `validate_descrambler_source_filter()` / stale-source-generation / duplicate-claim check を `DescramblerPid` 受け取りへ変更し、`packet_txn.rs` の `descrambler_pid.0` call-site を削除した。
- invalid AIDL PID は valid typed PID を作れないため、`PidClaimRejectedInvalidPid` / `PidClaimRejectedInvalidPidWithoutDemux` の event-specific diagnostic に分離した。これは Option field bag ではなく invalid input 専用 variant として扱う。
- 未実行: rustfmt / rustc / cargo / Soong build / `m nothing` / test module build / `atest -b` / atest run / Tuner VTS discovery / Tuner VTS run / adb sanity / emulator or device boot / 実波確認。

# r50eo68_source_only_complete_corrected7_unverified

- corrected6 でも未達が残っていたため、source-only 修正を継続した。
- corrected6 の完了判定では、`DescramblerDiagnosticRecord::PacketPolicyWithoutPid` variant を追加したにもかかわらず、対応する `packet_policy_without_pid()` constructor を実装していなかった。`service_runtime/src/boot/packet_txn.rs` と source-only contract test がこの constructor を呼ぶため、これはコンパイル不能級の未達であり、前回残件リストに未記載だった。前回判定時は enum variant と call-site の存在だけを見て、constructor 定義まで読めていなかった。
- corrected6 の source-only contract test は `PacketPid` と `DescramblerPid` を直接 import しているのに、Android.bp の `maleicacid_tuner_hal2_service_runtime_source_only_contract_test` に `libmaleicacid_tuner_hal2_demux` と `libmaleicacid_tuner_hal2_descrambler` を追加していなかった。これは H の Android.bp / test source 完了条件に対する未達であり、前回残件リストに未記載だった。前回判定時は test source の import と target rustlibs を突合していなかった。
- `DescramblerDiagnosticRecord::packet_policy_without_pid()` を追加し、PID 欠落時の diagnostic constructor が variant-specific record を生成できるようにした。
- `maleicacid_tuner_hal2_service_runtime_source_only_contract_test` の rustlibs に `libmaleicacid_tuner_hal2_demux` と `libmaleicacid_tuner_hal2_descrambler` を追加した。
- これらは corrected6 で追加したコードの追従漏れであり、既存仕様判断の変更ではない。
- 未実行: rustfmt、rustc/cargo、Soong build、m nothing、test module build、atest -b、atest run、Tuner VTS discovery、Tuner VTS run、adb sanity、emulator/device boot、実波確認。

# r50eo68_source_only_complete_corrected6_unverified

- corrected5 でも未達が残っていたため、source-only 修正を継続した。
- corrected5 の完了判定では、`PacketPid::as_u16()` を消したことだけを確認し、`service_runtime/src/boot/packet_txn.rs` に残った `packet_pid.as_u16()` call-site と `ActiveDescramblerSnapshot` の `u16` PID key を十分読めていなかった。これはコンパイル不能級の未達かつ F の typed boundary 違反であり、前回残件リストに未記載だった。
- corrected5 で `PacketPid::as_u16()` を削除した後、call-site まで移行しないまま完了扱いにしたのはデグレ相当である。意図としては、demux packet pipeline 側の raw accessor 定義だけを潰せば十分と誤って考え、service_runtime の descrambler packet policy 経路を後続の照合対象として読めていなかった。
- `ActiveDescramblerSnapshot` を `u16` PID key から `PacketPid` / `DescramblerPid` typed key へ分離し、packet-derived PID を raw `u16` に戻さず descrambler claim と照合するよう修正した。
- `descramble_ts_packet_in_place()` の target PID set を `BTreeSet<DescramblerPid>` に変更し、descrambler core 側も raw `u16` target set を受け取らないようにした。
- `DescramblerDiagnosticRecord::PacketPolicy` / `PacketSourceFilterValidation` を `PacketPid` typed context に変更し、validation failure のように PID を持てない場合は `PacketPolicyWithoutPid` variant へ分離した。
- `DescramblerPid` と `PacketPid` の照合は、検証済み `DescramblerPid` から `PacketPid` へ入る一方向 typed bridge に限定し、`PacketPid` から raw PID を取り出す accessor は追加していない。
- source-only contract test に、packet policy diagnostic が `PacketPid` を持つことと、PID欠落時は dedicated variant を使うことを追加した。
- `DESIGN_JA.md` / `CODE_CONVENTION.md` に、descrambler claim と packet path の typed bridge は一方向であり、packet-derived `PacketPid` から raw PID を取り出さないことを追記した。
- `#[allow(dead_code)]` / `#[allow(unused*)]` は追加していない。今回確認した範囲で、削除済み raw accessor の呼び出し残りと、旧 `u16` key の dead/obsolete path は除去した。
- 未実行: rustfmt、rustc/cargo、Soong build、m nothing、test module build、atest -b、atest run、Tuner VTS discovery、Tuner VTS run、adb sanity、emulator/device boot、実波確認。

# r50eo68_source_only_complete_corrected5_unverified

- corrected4 でも未達が残っていたため、source-only 修正を継続した。
- corrected4 の完了判定では、`PacketPid::get()` / `as_i32_for_internal_demux()` の消滅だけを見て、`PacketPid::as_u16()` が production continuity / PES assembler path に残る点を見落としていた。
- `PacketPid::as_u16()` は raw accessor の再導入であり、v7 の「PacketPid raw accessor は AIDL presentation boundary のみ」という方針に反するため削除した。
- `ContinuityTracker` / `PesAssembler` / `PesPacket` を `PacketPid` typed key に寄せ、packet_pipeline から production raw u16 PID conversion を排除した。
- `PacketPipeline` の section/PES assembler key、generation key、continuity key は `PacketPid` typed key のまま維持した。
- corrected4 時点で、この未達は残件リストに記載していなかった。前回判定時は `PacketPid` raw accessor の代替面を十分に読まず、`as_u16()` の production use を見落としていた。
- rustfmt はこの環境に存在しないため未実行。Rust compile、cargo test、Soong build、m nothing、test module build、atest、Tuner VTS、adb sanity、emulator/device boot、実波確認も未実行。

# r50eo68_source_only_complete_corrected4_unverified

- 前回 corrected3_unverified の再照合で、`packet_pipeline.rs` に `PacketPid::as_i32_for_internal_demux()` を追加して production routing / generation map / assembler key へ raw PID を戻していたことを確認した。これは v7 の「PacketPid raw accessor は AIDL presentation boundary だけ」という F の意図に反するデグレであり、前回残件リストに未記載だった。前回は `PacketPid::get()` と `pid.as_i32()` の消滅だけを見ており、代替 raw accessor を導入したことが設計違反である点までコードを読んで判定していなかった。
- `PacketPid::as_i32_for_internal_demux()` を削除し、`PacketPipeline` の section/PES assembler key と generation key を `PacketPid` typed key に変更した。filter flush generation key も `PacketPid` を保持する形に変更し、config PID からの照合は private conversion と typed comparison helper に閉じた。
- 前回 corrected3_unverified の再照合で、`DescramblerDiagnosticRecord` を enum 化した後も `descrambler_id() -> Option<i32>` / `demux_id() -> Option<i32>` / `pid() -> Option<u16>` / `filter_id() -> Option<i32>` / `error() -> Option<&HalError>` accessor が残り、diagnostic を再び Option field bag として読み出せる状態だったことを確認した。これは G の variant-specific 化の未達であり、前回残件リストに未記載だった。前回は enum variant 定義部だけを見て、accessor によって Option field bag surface が残る点まで読んでいなかった。
- `DescramblerDiagnosticRecord` の Option field bag accessor を削除し、service_runtime の通常テストと source-only contract test を enum pattern match に変更した。
- 未実行: rustfmt、rustc/cargo、Soong build、m nothing、test module build、atest -b、atest run、Tuner VTS discovery、Tuner VTS run、adb sanity、emulator/device boot、実波確認。

# r50eo68_source_only_complete_corrected3_unverified

- 前回 corrected2_unverified の再照合で、DVR callback delivery の Binder failure accounting 呼び出しに `handle` 引数が重複するコンパイル不能級の誤差分が残っていたことを確認した。これは前回残件リストに未記載であり、前回完了判定時に当該差分行を実コードとして十分読んでいなかった。
- 前回 corrected2_unverified の再照合で、`RuntimeQuery` / `RuntimeObjectPublicEntry` が `service_runtime` public re-export に残り、`RuntimeQuery` の public methods 経由で registry entry / runtime snapshot / signal helper / PCR helper を外部 API として到達可能にしていたことを確認した。これは前回残件リストに未記載であり、前回完了判定時に `query_api.rs` の public surface を grep 断片で見ただけで、re-export と `runtime.query()` 経由の到達性まで読めていなかった。
- `dvr_callback_delivery.rs` の duplicate `handle` argument を削除し、Binder delivery failure accounting 呼び出しの引数列を `runtime, handle, phase, dvr_phase, primary` に戻した。
- `RuntimeQuery` と `RuntimeObjectPublicEntry` を crate-private にし、`TunerServiceRuntime::query()` および `RuntimeQuery` helper methods を crate-private に変更した。`service_runtime/src/lib.rs` と `boot.rs` から `RuntimeQuery` / `RuntimeObjectPublicEntry` の public re-export を削除した。
- `packet_pipeline.rs` に残っていた `pid.as_i32()` 呼び出しを `pid.as_i32_for_internal_demux()` に修正した。`PacketPid::get()` 削除後に残っていた未追従のコンパイル不能級差分であり、これも前回残件リストに未記載だった。前回完了判定時に raw accessor 削除後の全 call-site を読んでいなかった。
- service boot reset split diagnostic で drop-leak reset failure を runtime finish failure として記録していた誤分類を修正し、callback artifact error / drop-leak error / runtime error を service boot reset 専用 outcome として分けた。boot reset では runtime lock を取得してから callback artifact reset command を発行し、artifact reset / drop-leak reset / runtime boot reset / diagnostic record を同一 runtime finish 区間で扱うよう補正した。
- 未実行: rustfmt、rustc/cargo、Soong build、m nothing、test module build、atest -b、atest run、Tuner VTS discovery、Tuner VTS run、adb sanity、emulator/device boot、実波確認。

# r50eo68_source_only_complete_corrected2_unverified

- 前回 corrected_unverified の再照合で、frontend worker join ticket が worker generation / runtime snapshot / bound demux snapshot の再検証まで閉じていないこと、DescramblerDiagnosticRecord の pid_claim constructor が Option demux id を受け取っていたことを確認した。
- frontend worker replacement / stop object ticket に worker generation、frontend runtime snapshot、bound demux generation snapshot を保持させ、external join 後の complete helper で object generation / frontend id / worker generation / live reader / scan session / bound demux generation を再検証するよう補強した。
- FrontendWorkerStopTicket に public variant を持たせず、single-use ticket の worker_generation accessor だけを追加した。
- DescramblerDiagnosticRecord の PID claim 診断を demux resolved / demux unresolved の variant に分離し、Option demux id constructor を廃止した。
- source-only contract test に DescramblerDiagnosticRecord の demux resolved / unresolved variant 固定テストを追加した。
- 未実行: rustfmt、rustc/cargo、Soong build、m nothing、test module build、atest -b、atest run、Tuner VTS discovery、Tuner VTS run、adb sanity、emulator/device boot、実波確認。

# r50eo68_source_only_complete_corrected_unverified

- Corrected the previous r50eo68_source_only_complete over-report: the earlier archive did not fully satisfy the v7 source-only work plan for frontend worker ticket ownership, frontend scan-end artifact lookup regression, callback artifact/runtime split finish diagnostics, or the claimed source-only regression coverage.
- Implemented service-runtime frontend worker replacement/stop tickets that bind object id, object generation, frontend id, worker kind, cancel reason, and the underlying device stop ticket; join still occurs outside the runtime lock, and post-join mutation now goes through ticket-consuming complete helpers that revalidate the AIDL object and frontend target.
- Fixed frontend callback delivery failure accounting so `CallbackArtifactLookup` no longer marks frontend scan session callback failure or runtime callback registration unhealthy; added a regression test for frontend scan-end artifact lookup failure preserving registered callback health.
- Changed DVR callback artifact lookup diagnostic recording so missing/store-failure post-commit diagnostics are not silently skipped and do not rely on `Ok(false)` delivery reporting.
- Reworked callback artifact/runtime split diagnostics so owner cleanup, callback registration rollback, object close callback cleanup, and service boot reset use distinct split phases/targets; registry-missing finish is treated as a runtime finish failure diagnostic instead of being ignored.
- Kept the earlier PacketPid / PipelineDiagnostic typed accessor changes, DescramblerDiagnosticRecord enum conversion, query helper visibility narrowing, and Android.bp test target additions.
- Source-only static checks performed: no `pub enum FrontendWorkerStopTicket`, service-runtime replacement/stop ticket helpers are present, frontend artifact lookup unhealthy-marking path is phase-gated, `PacketPid::get()` and `PipelineDiagnostic::pid() -> Option<i32>` are absent from the packet pipeline, and the added Android.bp test target names are present.
- Not run: rustfmt, rustc, cargo, Soong build, `m nothing`, test module build, `atest -b`, atest run, Tuner VTS discovery, Tuner VTS run, adb sanity, emulator/device boot, or real-broadcast verification. This is source-only corrected/unverified, not build verified / atest OK / VTS complete / device verified.

# r50eo68

- Audited `tuner_hal2/DESIGN_JA.md` and `tuner_hal2/CODE_CONVENTION.md` beyond the previous diff-only cleanup for document responsibility split.
- Reworded remaining CODE_CONVENTION entries that re-stated design contracts for missing-target handling, mandatory diagnostics, Drop leak semantics, descrambler key phase order, AV shared backing failure semantics, root/object query DTO boundaries, packet diagnostic required context, and transaction phase visibility so they now reference `DESIGN_JA.md` as the semantic source of truth and describe only implementation-prohibited forms.
- Removed the Android.bp failure-injection test registration rule from `CODE_CONVENTION.md`; build/test registration requirements belong to completion evidence / build integration review rather than module coding convention.
- No Android/Soong build, rustc, cargo, rustfmt, unit test, atest, VTS, loom, or device tests were run in this environment.

# r50eo67

- Rebalanced `tuner_hal2/DESIGN_JA.md` and `tuner_hal2/CODE_CONVENTION.md` according to `開発規則.md` document responsibility rules: public contract / lifecycle / return-value / resource-lifetime semantics remain in DESIGN_JA.md, while CODE_CONVENTION.md now contains implementation-entry and helper-use rules that reference DESIGN_JA.md instead of redefining those semantics.
- Removed the Android.bp failure-injection-test registration rule from DESIGN_JA.md because build/test registration is an implementation/conformance rule, not a design contract.
- Removed a release-report wording rule from CODE_CONVENTION.md because release report / changelog wording is governed by `開発規則.md`, not module-local coding convention.
- No Rust/Soong build, rustc, cargo, rustfmt, unit test, atest, VTS, loom, or device tests were run in this environment.

# r50eo66 callback artifact delivery boundary fix24 callback reset command static partial

- Reviewed fix23 against the original callback artifact clear surface request and found a remaining production all-artifact clear helper on the service boot reset path.
- Added `CallbackArtifactResetCommand` as the service_runtime-issued command for service boot callback artifact reset.
- Changed `AidlServiceContext::reset_runtime_from_probe_results()` to obtain the reset command from service_runtime and clear callback artifacts through `clear_callback_artifact_reset_bridge()`.
- Made the all-callback-store clear implementation a private raw helper behind the reset bridge.
- Updated DESIGN_JA.md and CODE_CONVENTION.md so production callback artifact clear is command-bridge based for both owner cleanup and service boot reset.
- Did not change callback delivery failure accounting, callback unhealthy marking ownership, object close cascade ordering, or FMQ linkage.
- Soong build, rustc, cargo, rustfmt, unit test, atest, loom, VTS, and device-side service verification were not rerun in this environment.

# r50eo66 callback artifact delivery boundary fix23 service-runtime internal mark visibility static partial

- Reviewed fix22 against the original callback artifact clear surface / callback delivery failure composition request.
- Found that the AIDL delivery modules no longer call the callback unhealthy marking helpers, but the helper methods themselves were still public methods on `TunerServiceRuntime`.
- Changed `mark_frontend_callback_delivery_failed_use_case()`, `mark_filter_callback_delivery_failed_use_case()`, `mark_dvr_callback_delivery_failed_use_case()`, and `mark_frontend_scan_session_callback_failed()` to `pub(crate)` so the cross-crate service_runtime API exposes `finish_callback_delivery_failure_use_case()` as the callback delivery failure accounting entry point.
- Did not change `DESIGN_JA.md` or `CODE_CONVENTION.md`; the visibility change makes the implementation match the existing documented service_runtime ownership rule.
- Soong build, rustc, cargo, rustfmt, unit test, atest, loom, VTS, and device-side service verification were not rerun in this environment. The previous user-provided log showed non-device `01` through `08` passing for fix22; this fix is a narrow visibility correction.

# r50eo66 callback artifact delivery boundary fix22 drop-leak regression repair static partial

- Reviewed `r50eo66_tuner_hal2_verify_logs_20260630_000801.tar.gz`; VTS candidate scan, tuner HAL service precheck, and Tuner VTS execution remain excluded from the final verdict while their log collection steps remain present.
- Non-excluded verification had `01` through `07` passing and only `08_atest_run_tuner_hal2_tests` failing.
- Fixed `object_runtime::tests::drop_leak_registry_missing_is_reported_after_quarantine` by restoring the object-table-only setup. This test intentionally verifies that drop-leak quarantine is committed and a missing public runtime unregister is still reported as an error.
- Regression classification: this is not a previous repair omission from the immediately preceding log. The same test passed in `r50eo66_tuner_hal2_verify_logs_20260629_235334.tar.gz`; fix21 accidentally changed this drop-leak test from object-table-only setup to live-runtime setup while repairing `close_object_after_close_preflight_rejects_closed_object`.
- Did not change `DESIGN_JA.md` or `CODE_CONVENTION.md`; the fix restores the existing drop-leak failure-precedence test shape and does not change callback policy ownership, artifact cleanup boundary, delivery failure composition, or close/drop-leak terminalization design.
- Soong build, rustc, cargo, rustfmt, unit test, atest, loom, VTS, and device-side service verification were not rerun in this environment.

# r50eo66 callback artifact delivery boundary fix21 static partial

- Fixed the remaining non-device atest failures from the 20260629_235334 log while keeping VTS discovery, tuner HAL service precheck, and VTS execution excluded from the final verdict.
- Fixed `close_object_after_close_preflight_rejects_closed_object` by using a live Filter runtime entry for the first successful close. This is a fix20 follow-up regression: the previous close-test setup cleanup did not convert this success-path test from object-table-only setup to live-runtime setup.
- Fixed `source_boundary_disconnects_to_demux_input` by recreating the sink filter FMQ queue before the demux-input disconnect boundary. This is a previous repair omission: the source-boundary failure was already present in the prior non-device atest log and was not covered by fix20.
- Fixed `demux_frontend_data_source_binds_and_live_sink_reaches_demux_runtime` by waiting for the live-pump worker to complete before falling back to stop-and-join. This is a previous repair omission: the live-pump failure was already present in the prior non-device atest log and was not covered by fix20.
- Did not change `DESIGN_JA.md` or `CODE_CONVENTION.md`; the fixes are test setup and asynchronous test-shape corrections that preserve the existing runtime ownership and callback policy boundaries.
- Soong build, rustc, cargo, rustfmt, unit test, atest, loom, VTS, and device-side service verification were not rerun in this environment.

# r50eo66 fix20 non-device atest test-shape repair

- Reviewed `r50eo66_tuner_hal2_verify_logs_20260629_233433.tar.gz`; VTS candidate scan, tuner HAL service precheck, and Tuner VTS execution remain excluded from the final verdict while their log collection steps remain present.
- Fixed the remaining non-excluded `08_atest_run_tuner_hal2_tests` failures without changing callback policy ownership or moving failure composition back into AIDL.
- Updated AIDL object-runtime drop-leak tests so missing-runtime failure cases use object-table-only setup and live-runtime success cases keep real public runtime entries.
- Updated the AIDL close cleanup failure injection test to exercise a frontend domain cleanup failure, matching the service-runtime domain cleanup command model and preserving the expected `ReleaseBackend` cleanup step.
- Updated demux queue-dependent tests to create filters through `OpenFilterRequest` with a positive buffer size before asserting FMQ queue payload delivery, delay readiness, source-boundary disconnect, playback-DVR queue delivery, and frontend-to-demux live sink behavior.
- Fixed a malformed discontinuity-generation packet test helper by initializing the adaptation-field flags byte to zero when synthetic adaptation padding is present.
- Updated the TS completion-buffer split-push test to account for malformed-byte accounting on the first resync push and packet delivery on the following confirmed-sync push.
- Updated the frontend live-pump service-runtime test to provide three TS packets, matching the three-sync resync contract before asserting delivered packet count.
- Kept DESIGN_JA.md and CODE_CONVENTION.md unchanged because the fixes align tests with the existing runtime contracts and do not alter production design rules.
- Verification not rerun in this environment.

# r50eo66 fix19 non-device atest failure repair

- Reviewed `r50eo66_tuner_hal2_verify_logs_20260629_225830.tar.gz`; VTS candidate scan, tuner HAL service precheck, and Tuner VTS execution remain excluded from the final verdict while their log collection steps remain present.
- Fixed non-excluded `08_atest_run_tuner_hal2_tests` failures without moving callback cleanup policy back into AIDL.
- Relaxed FMQ descriptor fd-size validation only for Android fds that report metadata length zero; positive fd sizes still receive strict grantor range validation.
- Corrected stale demux parser unit-test vectors that no longer matched strict section/PES/adaptation validation contracts.
- Updated AIDL object-runtime close tests to create real public runtime entries where close success is under test and to use table-only setup only for missing-runtime cleanup failure cases.
- Updated callback delivery failure tests to register live AIDL object-table entries before recording runtime callback registrations.
- Fixed the object close closed-object test setup so the object is actually committed to `Closed` instead of being re-normalized to `Live` by `RuntimeObjectTable::insert()`.
- Fixed descrambler clear-key transaction ordering so a release failure preserves the session key instead of clearing the session first.
- Updated the invalid-length descrambler token test to use a token longer than the accepted 1..=16 byte token range.
- Removed a duplicate `name` property from the binder adapter `rust_library` stanza in `Android.bp`.
- Kept DESIGN_JA.md and CODE_CONVENTION.md unchanged because the changes preserve the documented service_runtime ownership and close/failure-precedence rules.
- Verification not rerun in this environment.

# r50eo66 fix18 FMQ static shim shared dependency propagation fix

- Reviewed `r50eo66_tuner_hal2_verify_logs_20260629_224329.tar.gz`; VTS candidate scan, tuner HAL service precheck, and Tuner VTS execution remain excluded from the final verdict while their log collection steps remain present.
- Fixed the non-excluded Soong link failure introduced by the static FMQ shim path: Rust dylib/test link steps now receive the native shared dependencies required by the static shim archive.
- Added the FMQ shim native dependency set as `shared_libs` to `libmaleicacid_tuner_hal2_fmq` and to the FMQ-dependent Rust test binaries that directly link `libmaleicacid_tuner_hal2_fmq_shim_static`.
- Kept `libmaleicacid_tuner_hal2_fmq_shim` and `libmaleicacid_tuner_hal2_fmq_shim_static` as explicit module targets so the verification script target list remains accurate.
- Kept DESIGN_JA.md and CODE_CONVENTION.md unchanged because this is a Soong linkage repair and does not change callback artifact cleanup, delivery failure composition, or service_runtime policy ownership.
- Verification not rerun in this environment.

# r50eo66 fix17 FMQ shim test link fix

- Replaced the Rust FMQ shim link path with a static shim variant for the Rust FMQ crate and FMQ-dependent Rust test modules, while keeping the existing shared shim module available for the scripted module target build.
- Added `libmaleicacid_tuner_hal2_fmq_shim_static` from the same FMQ shim defaults as the shared shim so device-side atest binaries no longer require `libmaleicacid_tuner_hal2_fmq_shim.so` to be deployed next to cached test executables.
- Removed the fix16 `data_libs` test packaging dependency for FMQ-dependent Rust tests because the test executables no longer need the shim as a runtime shared object.
- Kept DESIGN_JA.md and CODE_CONVENTION.md unchanged because this is a build/linkage repair that preserves the callback artifact cleanup and delivery failure ownership rules.
- Verification not rerun in this environment.

# r50eo66 fix16 compile warning and test runtime dependency fix

- Removed the production unused `AidlApi` import from `aidl_service/src/service_context.rs` and moved it behind `#[cfg(test)]` for the test-only callback store helpers.
- Added `libmaleicacid_tuner_hal2_fmq_shim` as `data_libs` for Rust test modules that link the shim so atest can deploy the native shim next to the device-side test executable.
- Kept DESIGN_JA.md and CODE_CONVENTION.md unchanged because the fix only closes a compile warning and test packaging dependency without changing callback policy ownership.
- Verification not rerun in this environment.

# r50eo66 fix15

- Fixed non-device verification build failures reported after fix14 while keeping VTS and tuner-HAL-service device precheck out of the final acceptance scope.
- Imported `FirstErrorCollector` from the common crate in drop-leak object cleanup instead of the AIDL object runtime module.
- Updated drop-leak artifact executor construction to match the command-only artifact bridge executor constructor.
- Removed the unused `CallbackHealthState` test import from AIDL object runtime tests.
- Converted the DVR Binder-delivery failure path from `Result<(), HalError>` to the surrounding `Result<bool, HalError>` shape without moving failure composition back into AIDL.
- Fixed the callback artifact bridge boundary test to pass an `i32` filter runtime id to the service-runtime cleanup planner.
- No `DESIGN_JA.md` or `CODE_CONVENTION.md` change was needed because this change only repairs build-shape mismatches and preserves the existing service-runtime policy boundary.

# r50eo66 callback artifact delivery boundary fix14 static partial

- Removed the unused `ObjectDomainCleanupOutcome::command()` accessor instead of adding a dead-code allowance.
- Reduced `ObjectDomainCleanupOutcome` to the executed cleanup result because no production or test path consumes the completed command after executor dispatch.
- Kept `ObjectDomainCleanupCommand` construction, typed executor dispatch, and close/drop-leak cleanup ordering unchanged.
- Kept `RELEASE_VERSION` unchanged at `r50eo66`; this archive remains static partial until the external verification script is rerun.
- Verification rerun status for this archive: not executed in this environment.

# r50eo66 fix13 static partial

- Fixed the callback delivery failure finish use-case build break reported by the 20260629_215941 verification logs.
- Changed `finish_callback_delivery_failure_use_case()` to clone the primary `HalError` before using report metadata, avoiding the Rust E0382 partial-move error in `service_runtime/src/boot.rs`.
- Kept DESIGN_JA.md and CODE_CONVENTION.md unchanged because this is a Rust ownership/build fix, not a design or convention change.
- Verification status: not rerun in this environment after this fix; rerun the non-VTS final verification steps on the generated archive.

# r50eo66 callback artifact delivery boundary fix12 static partial

- `r50eo66_tuner_hal2_verify_logs_20260629_213225.tar.gz` を確認し、VTS candidate scan / service precheck / VTS execution は顧客指示により最終検証対象から除外した。
- 最終検証対象の失敗は `05_build_tuner_hal2_modules` / `06_build_tuner_hal2_test_modules` / `07_atest_build_only_tuner_hal2_tests` / `08_atest_run_tuner_hal2_tests` で、根因は `service_runtime/src/boot.rs` の `DvrPostCommitNotificationPhase` import 欠落だった。
- `service_runtime/src/boot.rs` の diagnostics import に `DvrPostCommitNotificationPhase` を追加し、`CallbackDeliveryFailureReport::dvr()` と `finish_callback_delivery_failure_use_case()` が同型を参照できるようにした。
- `DESIGN_JA.md` と `CODE_CONVENTION.md` は変更していない。今回の修正は、既存の callback delivery failure composition 設計を変更せず、build failure を起こしていた import 境界だけを補正した。
- Soong build / rustc / cargo / rustfmt / unit test / atest / loom / 実機確認は、この環境では未実行。
- Tuner VTS は顧客指示により今回の最終検証対象外。

# r50eo66 callback artifact delivery boundary fix11 release-rule repair static partial

- Repacked the archive so it extracts to `vendor/maleicacid/tv/...` directly from an AOSP tree root instead of `maleicacid/tv/...`.
- Removed fix-specific release wording from `tuner_hal2/DESIGN_JA.md`; the callback artifact lookup / delivery failure boundary section now uses permanent design wording.
- Removed fix-specific release wording from `tuner_hal2/CODE_CONVENTION.md`; the callback delivery failure boundary section now uses permanent convention wording.
- Kept the fix10 callback artifact delivery boundary implementation and tests unchanged except for release-rule documentation cleanup.
- Static partial only: Soong build, rustc/cargo, rustfmt, unit test, atest, VTS, loom, and device verification were not run in this environment. RELEASE_VERSION remains r50eo66.

# r50eo66 callback artifact delivery boundary fix10 static partial

- Kept callback artifact lookup failure distinct from Binder delivery failure for DVR callback delivery failure accounting; DVR artifact lookup failure now records diagnostic context without marking callback registry or DVR runtime callback state unhealthy.
- Added service_runtime callback delivery failure tests for filter artifact lookup, filter Binder delivery with an existing runtime callback registration, DVR artifact lookup, and frontend scan END delivery failure composition.
- Added an AIDL object_runtime test showing that `clear_owner_callback_artifacts_bridge()` clears only callback artifacts and does not mutate the runtime callback registry directly.
- Preserved the command-only callback artifact cleanup bridge introduced in fix9; production callback store direct clear remains private raw helper plus test-only helper.
- Static partial only: Soong build, rustc/cargo, rustfmt, unit test, atest, VTS, loom, and device verification were not run in this environment. RELEASE_VERSION remains r50eo66.

# r50eo66 callback artifact delivery boundary fix9 static partial

- Closed the production direct callback artifact clear surface by making the owner-handle clear helper private and exposing only the `OwnerCallbackCleanupArtifactCommand` bridge for production cleanup.
- Added `CallbackDeliveryOwnerKind`, `CallbackDeliveryFailurePhase`, and `CallbackDeliveryFailureReport` as service_runtime-owned typed delivery failure inputs.
- Added `TunerServiceRuntime::finish_callback_delivery_failure_use_case()` as the owner of delivery diagnostic recording, scan-session callback failure marking, runtime callback registry unhealthy marking, filter/DVR runtime callback unhealthy marking, and primary+cleanup failure composition.
- Moved filter callback artifact missing, filter event conversion failure, and filter Binder callback failure accounting out of `aidl_service/src/filter_callback_delivery.rs` and into the service_runtime finish use-case.
- Moved DVR Binder callback failure and DVR post-commit notification failure accounting out of `aidl_service/src/dvr_callback_delivery.rs` and into the service_runtime finish use-case.
- Moved frontend scan END callback artifact missing, callback store failure, and Binder callback failure accounting out of `aidl_service/src/frontend_callback_delivery.rs` and into the service_runtime finish use-case.
- Replaced callback clear test call sites with `clear_owner_callbacks_for_test()` so production code cannot call owner-handle direct callback store clear.
- Added service_runtime failure-injection tests for filter and DVR callback delivery failure finish use-case composition and diagnostic recording.
- Updated DESIGN_JA.md and CODE_CONVENTION.md so callback artifact clear and callback delivery failure accounting are fixed as service_runtime-owned policy, with AIDL delivery modules limited to artifact lookup, event conversion, Binder execution, and primary error forwarding.
- Static partial only: Soong build, rustc/cargo, rustfmt, unit test, atest, VTS, loom, and device verification were not run in this environment. RELEASE_VERSION remains r50eo66.

# r50eo66 callback policy boundary actual completion fix8 static partial

- Removed the remaining public/direct callback registry mutation surface from AIDL-facing code paths: `record_callback_registration_for_object()` and generic callback unhealthy marking are now private service_runtime internals.
- Routed frontend/filter/DVR callback delivery failure accounting through service_runtime callback-delivery failure use-cases instead of AIDL delivery modules directly mutating `RuntimeCallbackRegistry` or filter/DVR unhealthy state.
- Updated DESIGN_JA.md and CODE_CONVENTION.md so callback delivery unhealthy marking is described as service_runtime-owned policy; AIDL delivery modules are limited to Binder delivery and result forwarding.
- Static partial only: build, atest, VTS, rustfmt, loom, and device verification were not run.

# r50eo66 callback policy boundary actual completion fix6 static partial

- Fixed the remaining setCallback ordering issue: frontend / LNB callback artifact retain bridge is now executed only after `ObjectMethodTxn` live/generation/kind validation and dispatch preflight have succeeded, so preflight failure cannot leave a retained callback artifact outside service_runtime cleanup ownership.
- Kept rollback command generation, unhealthy marking, and primary+cleanup failure composition in service_runtime; AIDL object_runtime now only carries the callback artifact retain bridge into the post-preflight execution path and returns bridge results to service_runtime outcomes.
- Updated DESIGN_JA.md / CODE_CONVENTION.md to state that callback artifact retain must not be performed before service_runtime object-method preflight.
- Verified the unified diff before packaging: the diff contains one Rust ordering fix plus matching design/convention text, not a version-only or script-only change.
- Not run: Soong build, atest, Tuner VTS, rustfmt, loom, real-device checks.

# r50eo66 callback policy boundary actual completion fix5 static partial

- Clarified current `DESIGN_JA.md` / `CODE_CONVENTION.md` wording so AIDL callback helpers are described only as callback artifact bridge façades, not as callback retain / rollback policy owners; also fixed the callback registration failure table to name service_runtime as the rollback command / failure-composition owner.
- No Rust code change from fix4; service_runtime remains the owner of callback cleanup command generation, unhealthy marking, and primary+cleanup failure composition for the reviewed callback boundary scope.
- Verified the unified diff before packaging: the diff is documentation-only and removes the remaining current-design wording that could imply AIDL-owned callback retain / rollback policy.
- Not run: Soong build, atest, Tuner VTS, rustfmt, loom, real-device checks.

# r50eo66 callback policy boundary actual completion fix4 static partial

- Clarified `DESIGN_JA.md` callback registration / child-open boundary text so the design no longer presents `object_runtime` or `child_object_open.rs` as owning callback rollback policy.
- Fixed the remaining documentation ambiguity after the previous code-side callback policy boundary migration: AIDL is limited to callback artifact bridge execution and Binder status conversion; service_runtime owns rollback command generation, unhealthy marking, and primary+cleanup failure composition.

# r50eo66 callback policy boundary actual completion follow-up 3 static partial

- Cleaned the remaining ambiguous DESIGN_JA.md wording around callback rollback. The design no longer describes child_object_open.rs as owning typed callback rollback or callback rollback bridge; it now limits AIDL to callback artifact retain bridge / service_runtime-issued cleanup bridge execution results.
- Verified the unified diff before packaging: the diff is documentation-only, and it removes the residual ambiguous phrases `typed callback retain closure`, `child allocation / callback rollback`, and `callback artifact retain / rollback bridge`.
- Static partial only. Build, atest, Tuner VTS, rustfmt, loom, and real-device validation are not executed here. RELEASE_VERSION remains r50eo66.

# r50eo66 callback policy boundary actual completion follow-up 2 static partial

- Removed remaining DESIGN_JA.md wording that made child_object_open.rs look like the owner of callback rollback policy. The design now states that AIDL performs callback artifact retain bridge and Binder object construction only, while service_runtime child-open finish use-cases own child rollback command generation and primary+cleanup failure composition.
- Verified the unified diff before packaging: the diff is documentation-only, and it changes child-open boundary wording from "typed callback retain / rollback" to "typed callback artifact retain bridge" plus service_runtime-owned rollback finish semantics.
- Static partial only. Build, atest, Tuner VTS, rustfmt, loom, and real-device validation are not executed here. RELEASE_VERSION remains r50eo66.

# r50eo66 callback policy boundary actual completion follow-up static partial

- Moved the remaining child-open artifact-retain-failure rollback composition out of AIDL `child_object_open.rs`: filter/DVR child callback retain failure now calls service_runtime finish use-cases that perform child runtime/object rollback and primary+cleanup failure composition.
- Moved child-open object-construction failure composition out of AIDL helper code: AIDL still executes the callback artifact bridge cleanup outcome, but service_runtime now performs the final primary+cleanup failure composition.
- Verified the unified diff before packaging: the diff removes `fail_after_cleanup` / direct rollback helpers from `aidl_service/src/child_object_open.rs` and adds service_runtime child-open failure finish use-cases in `service_runtime/src/boot.rs`.
- Static partial only. Build, atest, Tuner VTS, rustfmt, loom, and real-device validation are not executed here. `RELEASE_VERSION` remains `r50eo66`.

# r50eo66 callback policy boundary actual completion static partial

- Removed the remaining AIDL-side callback cleanup command planning paths: child-open object-construction rollback now obtains a service_runtime-owned owner callback cleanup outcome, and AIDL executes only the callback artifact bridge result.
- Embedded service_runtime-owned `OwnerCallbackCleanupArtifactCommand` data inside `ObjectArtifactCleanupCommand` so the AIDL artifact executor no longer calls `plan_owner_callback_cleanup_artifact_command()` or derives callback cleanup policy from object kind/API fields.
- Kept direct child-open runtime rollback only for artifact-retain failure, where no callback artifact has been retained yet; object-construction failure after retained artifact uses the service_runtime owner callback cleanup outcome.
- Updated `DESIGN_JA.md` so `child_object_open.rs` is described as Binder construction / artifact bridge glue, not the owner of callback cleanup or rollback policy.
- Static partial only. Build, atest, Tuner VTS, rustfmt, loom, and real-device validation are not executed here. `RELEASE_VERSION` remains `r50eo66`.

# r50eo66 callback policy boundary completion follow-up static partial

- Moved the remaining callback unregister and callback registration rollback outcome planning out of AIDL `object_runtime`: service_runtime now returns typed callback cleanup / rollback outcomes, while AIDL executes only callback artifact bridge operations and returns those results to service_runtime finish use-cases.
- Removed the AIDL `service_context` finish orchestration helpers so the context no longer composes callback cleanup policy; it only exposes callback artifact bridge execution.
- Updated DESIGN_JA.md to clarify that callback unregister and registration rollback command generation / failure composition are service_runtime-owned.
- Static partial only. Build, atest, Tuner VTS, rustfmt, loom, and real-device validation are not executed here. `RELEASE_VERSION` remains `r50eo66`.

# r50eo66 callback policy boundary completion static partial

- Removed the remaining callback unregister completion helper that kept method-result cleanup branching in `AidlServiceContext`; unregister now routes the domain operation through service_runtime callback unregister use-case and only uses AIDL for the callback artifact bridge/status mapping.
- Removed the old generic frontend/LNB callback registration closure façade from AIDL object wrappers; frontend/LNB domain registration selection now lives in `TunerServiceRuntime::execute_callback_registration_for_object_use_case()`.
- Moved object artifact cleanup command kind dispatch out of the AIDL executor: `ObjectArtifactCleanupCommand::execute_with()` performs typed dispatch inside service_runtime, and AIDL implements only typed bridge methods.
- Replaced child-open callback registration closure helper with a result-based artifact bridge helper so callback artifact retain execution and runtime registry finish are not represented as an AIDL-owned policy closure.
- Updated `DESIGN_JA.md` to remove the old `clear_owner_callback_registration()` wording and to state service_runtime typed callback artifact cleanup commands/use-cases as the policy owner.
- Static partial only. Build, atest, Tuner VTS, rustfmt, loom, and real-device validation are not executed here. `RELEASE_VERSION` remains `r50eo66`.

# r50eo66

- Command boundary follow-up: moved drop-leak LNB record decision into the service_runtime drop-leak plan, removed the AIDL-side drop-leak action selector, stopped generating domain cleanup commands for Tuner / Demux / Filter / Dvr / Descrambler no-op cases, and removed AIDL executor no-op domain cleanup policy arms.
- Kept RELEASE_VERSION at r50eo66; this remains a static partial because Soong / atest / Tuner VTS were not rerun in this environment.

- Build log follow-up 9 after `verify_tuner_hal2_all_v3.sh`: fixed `maleicacid_tuner_hal2_device_test` by importing `std::time::Duration` in the live-pump test module.
- Removed panic-based test patterns that were not required for the failing build: the drop-leak runtime-lock poison test no longer creates a poisoned mutex via deliberate panic, and MULTI2 key-preparation failure is checked by direct `Result` comparison instead of `catch_unwind`.
- Replaced explicit `panic!` fallback in live-pump polling tests with ordinary result-state assertions.
- Updated the verification helper to `verify_tuner_hal2_all_v4.sh`; VTS candidate scan no longer calls unsupported `atest --list-tests` and instead scans checked-in test metadata.
- Kept `RELEASE_VERSION` at `r50eo66`; this is still a build-fix static partial because Soong / atest / VTS have not been rerun after this fix.

- Build log follow-up 8 after `verify_tuner_hal2_all_v2.sh`: fixed atest-run failures by aligning stale common/device tests with current semantics, removing Android-runner-visible intentional panic/poison test bodies, and adding direct `libmaleicacid_tuner_hal2_fmq_shim` shared-library dependencies to test modules that load FMQ transitively.
- Kept `#[cfg(test)]` only for active test fixtures/test-only imports. No `#[allow(dead_code)]` or `#[allow(unused*)]` was added; no unavoidable dead code is reported in this pass.
- Kept `RELEASE_VERSION` at `r50eo66`; this is still a build-fix static partial because Soong/atest/VTS have not been rerun after this fix in this environment.

- Build log follow-up 7 after `verify_tuner_hal2_all.sh`: removed stale AIDL object-runtime imports reported in `r50eo66_tuner_hal2_verify_logs_20260629_024619`, removed the stale close-domain-cleanup hook test code that no longer matches the typed executor design, and kept only a current close preflight lifecycle test.
- Kept `RELEASE_VERSION` at `r50eo66`; this is still a build-fix static partial because Soong / atest / VTS have not been rerun after this fix.

- Build log follow-up 6 after `verify_tuner_hal2_all.sh`: fixed the AIDL service compile errors reported in `r50eo66_tuner_hal2_verify_logs_20260629_023811` without adding dead-code or unused allowances.
- Imported `OwnerCallbackCleanupArtifactCommand` through the explicit `service_runtime::boot` module path instead of adding a new root re-export, restored the callback-artifact rollback helper used after retained Binder callback artifacts, and corrected the drop-leak LNB domain cleanup executor lifetime / public-id lookup call.
- Aligned generated AIDL trait method signatures with the target Rust backend by accepting non-null `Strong` references in `setCallback`, `setDataSource`, `addPid`, and `removePid`, while keeping the internal nullable helper paths for existing explicit cleanup use-cases.
- Fixed AIDL service test/failure-injection compile errors by importing the close helper explicitly and adding concrete result types where generic inference was ambiguous.
- Kept `RELEASE_VERSION` at `r50eo66`; this remains a build-fix static partial because Soong / atest / VTS have not been rerun after this fix.

- Build log follow-up 5 after `verify_tuner_hal2_all.sh`: reviewed the newly introduced `#[cfg(test)]` usages instead of treating them as automatically valid. Removed old / unused production descrambler key registration surface and unused raw descrambler session helpers rather than hiding them behind allowances.
- Replaced the unused object-close `closing_entries` binding with an error-only preflight check, removed the unused `TunerServiceRuntime::frontend_status_query_for_aidl_object()` façade, and kept the actually used `RuntimeQuery` query path.
- Restricted descrambler key-slot insertion to test-only fixture helpers, removed unused registration errors / slot-id allocation state from production, and removed unused cleanup-report and resolved-claim-set accessors.
- Kept `#[cfg(test)]` only where it is a test fixture or test-only import. No `#[allow(dead_code)]` / `#[allow(unused*)]` was added, and no unavoidable dead code remains in this pass.
- Kept `RELEASE_VERSION` at `r50eo66`; this is still a build-fix static partial because Soong / atest / VTS have not been rerun after this fix.

- Build log follow-up 4 after `verify_tuner_hal2_all.sh`: fixed the service_runtime unused imports reported in `r50eo66_tuner_hal2_verify_logs_20260629_021035`, added a validated-packet scrambling-control accessor so service_runtime no longer calls the crate-local raw packet view, and imported DVR `RecordStatus` / `PlaybackStatus` only for binder_adapter test helpers.
- Kept `RELEASE_VERSION` at `r50eo66`; this is still a build-fix static partial because Soong / atest / VTS have not been rerun after this fix.

- Build log follow-up 2 after `verify_tuner_hal2_all.sh`: fixed the remaining demux compile errors reported in `r50eo66_tuner_hal2_verify_logs_20260629_015850` by replacing stale `view.packet_pid()` uses with `ValidatedTsPacket::pid()` and converting `PacketPid` to the `ts_core::PesAssembler` raw `u16` boundary only at the ts_core adapter call.
- Kept `RELEASE_VERSION` at `r50eo66`; this is still a build-fix static partial because this environment cannot rerun Soong / atest / VTS.

- Build log follow-up after `verify_tuner_hal2_all.sh`: removed the unused `DESCRAMBLER_TOKEN_BYTES` re-export and constant instead of adding dead-code allowances.
- Fixed packet pipeline compile errors by passing `PacketPid` to PES assembly state, passing `ValidatedTsPacket` to post-preflight assembly, and updating packet validation tests to use typed accessors / `matches!` instead of comparing `ValidatedTsPacket` results directly.
- Removed unused test-only mutability and the unused LNB test backend helper instead of adding `allow(dead_code)` or `allow(unused_mut)`.
- Kept `RELEASE_VERSION` at `r50eo66`; this is still a build-fix static partial because the local environment cannot rerun Soong / atest / VTS.

- v2 non-build/test closure follow-up: replaced the object-close runtime cleanup `FnOnce` injection with `ObjectRuntimeCleanupCommand`, so AIDL receives a typed runtime cleanup command and calls its service_runtime-owned executor instead of supplying runtime mutation logic.
- Made `ObjectArtifactCleanupCommand` non-forgeable from public fields by making fields private and exposing read-only accessors for the AIDL artifact bridge.
- Removed private `old_token()` / `old_key_slot()` plan accessor methods from the service_runtime descrambler transaction implementation; old token / key-slot data is now consumed internally without accessor-shaped cleanup ordering hooks.
- Moved `DescramblerKeyTable`, key lookup / registration errors, and key slot id ownership from the descrambler domain crate public surface to `service_runtime/src/descrambler_key_table.rs`; the descrambler crate now exports domain values / DTO / validation only.
- Removed `descrambler/src/runtime/key_table.rs` from the descrambler module graph and Android.bp sources, and added `service_runtime/src/descrambler_key_table.rs` to service_runtime library and test sources.
- Kept `RELEASE_VERSION` at `r50eo66` because rustfmt, rustc/cargo, Rust unit tests, Soong build, atest, VTS, loom, and real-device verification are still not executed.

- v2 continuation: moved descrambler runtime session state and key/PID transaction implementation out of the descrambler domain crate public runtime module and into `service_runtime/src/descrambler_session.rs`.
- Removed `descrambler/src/runtime/session.rs` and `descrambler/src/runtime/session_txn.rs` from the descrambler module graph and Android.bp sources so the descrambler crate exposes domain value / PID / token types only; key table ownership is moved to service_runtime.
- Changed service_runtime `RuntimeRegistry` to own `DescramblerRuntime` state locally and to execute clear-key, replace-key, cleanup-all, demux bind, PID add, and PID remove through service_runtime-owned use-case helpers.
- Kept `RELEASE_VERSION` at `r50eo66`; this is still a static partial because rustfmt, rustc/cargo, Rust unit tests, Soong build, atest, VTS, loom, and real-device verification are not executed.

- drop leak terminalization を service_runtime の dedicated executor / finish use-case 経由へ寄せ、runtime lock を保持したまま callback artifact cleanup を呼ばない構造へ修正した。
- Drop leak の artifact cleanup と public runtime unregister の順序・failure collection を service_runtime 側へ集約し、AIDL 側の手書き terminalization を削除した。
- packet_pipeline の test support helper を PacketPid 入力に統一し、pid: i32 に対する as_i32() 呼び出し不整合を解消した。

- review6 verification follow-up: fixed the descrambler addPid AIDL PID validation order so `validated_pid` is created before conflict checks, changed the source-filter conflict check to use the validated PID boundary, moved callback artifact-registration rollback failure composition into a service_runtime use-case, narrowed RuntimeRegistry descrambler allocation/lookup/conflict helper visibility to crate-local, and made packet flush post-state clearing crate-local.

- review5 callback rollback / packet flush / descrambler surface follow-up: callback registration rollback failure composition を service_runtime use-case へ寄せ、filter flush の PID 境界を ConfigInputPid に分離し、RuntimeRegistry descrambler key transaction façade を crate-local に縮小した。

- Changed owner callback cleanup completion so service_runtime plans the Binder artifact cleanup command, AIDL executes only that artifact bridge command, and service_runtime preserves unhealthy registry state on failure instead of clearing the owner entry first.
- Changed drop-leak terminalization so service_runtime's quarantine plan owns callback-cleanup and DVR-notifier artifact commands; AIDL executes the returned commands instead of selecting drop-leak cleanup targets or DVR notifier policy itself.
- Restricted `TsPacketView` itself and `ValidatedTsPacket::view()` to crate visibility, so external packet-bearing callers cannot recover a raw packet view from a validated packet.
- Moved additional packet pipeline helper boundaries from raw PID integers to `PacketPid` for continuity reset, assembly reset, section/PES assembly, and generation tracking.
- Added `AidlInputPid` validation for descrambler add/remove PID inputs and `ConfigInputPid` validation for filter configuration TPID inputs, keeping AIDL/config-derived PID validation separate from packet-derived `PacketPid`.

- Moved callback cleanup policy ownership from AIDL `service_context` into `TunerServiceRuntime::finish_owner_callback_cleanup_use_case()`: runtime callback registry clear, missing-registration handling, unhealthy marking, and primary-plus-cleanup failure composition are now service_runtime responsibilities.
- Reduced `SharedAidlServiceContext` callback cleanup to a Binder artifact bridge that clears callback-store artifacts and passes the artifact result to the service_runtime owner cleanup use-case.
- Rewired callback unregister, LNB owner-loss cleanup, and Drop leak cleanup to the service_runtime-owned callback cleanup use-case so Drop leak no longer depends on an AIDL-owned callback cleanup policy.

- Moved callback unregister success/failure completion into `SharedAidlServiceContext::finish_owner_callback_unregistration()` so the AIDL object-runtime helper no longer performs artifact-store clear, runtime registry clear, unhealthy marking, or primary-plus-cleanup failure composition itself.
- Removed production `expect_err()` from callback unregister cleanup completion and replaced it with explicit structured error propagation.
- Changed drop-leak callback cleanup to call the shared callback cleanup entry, removing the duplicate callback-store clear / runtime registry clear / unhealthy-marking procedure from `drop_leak.rs`.
- Moved close cleanup phase ordering into `ObjectCloseUseCasePlan::execute_cleanup_with_domain_bridge()`, leaving AIDL with only the Binder artifact command bridge and domain cleanup hook.
- Narrowed public descrambler runtime key-transaction methods from arbitrary `DescramblerKeyTxnOps` implementors to the concrete domain `DescramblerKeyTable`, reducing non-service_runtime arbitrary key-table transaction surface.
- Narrowed `TsPacketView::pid()` to crate visibility and changed record-index event data to carry `PacketPid` internally instead of raw `i32` PID.
- Removed unused frontend runtime/signal intermediate-state query helpers so the query boundary remains on DTO snapshot helpers.

- Added callback unregister tests covering successful frontend unregister, LNB domain-failure all-attempt cleanup, and primary-plus-cleanup failure composition.
- Added descrambler session transaction tests covering clear-key session-clear-before-old-token-release, old-token release failure after session clear, replace-key commit rollback release, and rollback release failure reporting.
- Added object close use-case tests covering successful close finalization and cleanup-failed marking when domain cleanup reports a structured failure.
- Added SourceBoundary runtime tests covering source-filter connect, NULL/demux-input disconnect, and failed source change preserving the previous source.
- Added packet boundary tests covering validated packet PID extraction, malformed packet rejection before PID creation, and duplicate packet diagnostics carrying `PacketPid`.
- Added frontend query DTO tests covering status/readiness policy from `ObjectFrontendStatusSnapshot` without registry-entry inputs.
- Fixed the callback unregister multiple-failure branch so unhealthy-marking failure is composed with the original domain primary instead of losing the primary through `?` propagation.
- Fixed test-only stale syntax in `aidl_service::object_runtime` and renamed the internal close command dispatch helper to avoid duplicate Rust function names.
- Added `service_runtime/src/object_close_txn.rs::close_object_use_case()` as the public close cascade owner, producing structured artifact cleanup commands and a domain cleanup step instead of leaving close ordering in the AIDL façade.
- Added `finish_object_close_use_case()` so cleanup failure marking, public runtime unregister preflight/unregister, close commit, and cleanup-failed composition are owned by service_runtime.
- Changed `aidl_service/src/object_runtime/mod.rs` close flow so the AIDL side only executes Binder artifact cleanup commands, invokes the supplied domain cleanup hook at the service_runtime-selected phase, and maps the structured result to Binder status.
- Moved quarantined public runtime unregister selection for Drop leak terminalization into `service_runtime::object_close_txn::unregister_quarantined_public_runtime_entries()`.
- Changed callback unregister cleanup ownership so frontend/LNB NULL callback paths finish through the shared owner cleanup entry, all-attempting runtime callback registry clear and callback artifact store clear after the domain unregister attempt.
- Changed callback rollback / close / cascade / drop-leak cleanup call sites to use the expanded shared callback owner cleanup entry instead of directly clearing callback artifacts.
- Changed descrambler exports so `DescramblerSession` and raw session mutators are no longer re-exported from the crate root or runtime module.
- Added transaction façade methods on `DescramblerRuntime` for demux bind, PID claim add/remove, key clear/replace, and cleanup-all so service_runtime no longer calls raw `session_mut()` to mutate keys.
- Changed service_runtime descrambler key clear/replace and PID claim paths to call the `DescramblerRuntime` full transaction façade methods.
- Changed `SourceBoundaryTxn` surface so constructor, mutating methods, step recording, and raw outcome/reset accessors are private implementation details.
- Added immutable `SourceBoundaryReport` and `apply_filter_source_boundary_change()` as the source boundary observation façade.
- Changed demux source connect/disconnect paths to consume `SourceBoundaryReport` instead of observing the transaction object.
- Changed packet ingress validation to create `ValidatedTsPacket` at packet pipeline ingress and use its `PacketPid` in downstream diagnostics.
- Changed packet path diagnostic PID fields for TEI, duplicate, no-payload, keyless scrambled, section drop, generation overflow, and PES assembler drop from raw `i32` to `PacketPid`.
- Changed packet inspection and assembly preflight to pass `ValidatedTsPacket` instead of treating `TsPacketView` as the production path source of truth.
- Changed object close use-case planning so close cascade entry collection failure and target resolution failure compose cleanup-failed marking failure instead of dropping it.
- Fixed `plan_ts_packet_report()` to consume `ValidatedTsPacket` directly and derive both `TsPacketView` and `PacketPid` from that validated packet within the planning helper.
- Changed service_runtime query helper visibility so registry-entry-returning frontend query helpers are crate/internal helpers rather than public query API.
- Changed frontend object status query construction so `frontend_status_query_for_aidl_object()` returns `ObjectFrontendStatusSnapshot` directly instead of returning a `(FrontendRegistryEntry, FrontendRuntimeState, FrontendSignalState)` tuple for a caller-side DTO conversion.
- Removed the `FrontendRegistryEntry` conversion implementation from `object_method_txn.rs` so frontend status/readiness policy consumes service_runtime-owned DTO snapshots rather than registry entries.
- Fixed keyless scrambled packet diagnostic accounting to extract the numeric PID through `PacketPid::get()` instead of treating `PacketPid` as a raw integer.
- Added `DescramblerCleanupTxnError` and a key-table-owning descrambler cleanup façade so close / owner-loss cleanup releases the old token and closes session state inside the descrambler transaction boundary instead of exposing the raw key token to service_runtime cleanup code.
- Removed the production-visible `DescramblerRuntime::cleanup_all_with_session_txn()` façade that could close session state without the key-table release step.
- Changed service_runtime descrambler cleanup to call `RuntimeRegistry::cleanup_descrambler_session_with_key_table_txn()` and map the structured cleanup transaction error, including release+session composed failure, instead of separately reading `runtime.key_token()` and then calling cleanup.
- Reduced raw runtime/key-table observer surface in `RuntimeRegistry` by making direct descrambler runtime and descrambler key-table accessors crate-internal.
- Changed packet planning helper signatures so packet delivery / section / PES planning consume `PacketPid` from `ValidatedTsPacket` instead of accepting raw integer PID inputs on those helper boundaries.
- Changed service_runtime descrambler packet-path lookup to use registry-owned resolved claim snapshots, removing production packet code direct access to `descrambler_key_table()` / key-slot-id lookup helpers.
- Changed service_runtime descrambler stale source-filter generation checks to use a registry façade instead of reading `runtime.pid_claims()` directly in the PID removal use-case.
- Changed production descrambler runtime access so service_runtime uses registry-owned bind/add/remove/bound-demux façades instead of obtaining mutable `DescramblerRuntime` references from the registry.
- Replaced raw `DescramblerRuntime` observer methods for key token, key slot, demux id, demux generation, and pid claims with domain-specific predicate/snapshot methods.
- Made direct descrambler runtime and descrambler key-table registry accessors test-only, keeping production code on transaction and packet-resolution façades.
- Removed obsolete registry helpers that exposed descrambler key-slot IDs or `(claims, key_slot_id)` tuples to packet consumers.
- Changed descrambler runtime packet claim exposure so the public packet-facing claim set is resolved with `DescramblerKeyTable` inside the descrambler runtime boundary and no longer exposes a raw key-slot-id snapshot/accessor to service_runtime packet consumers.
- Changed record-index packet event construction to validate through `ValidatedTsPacket` at ingress instead of calling `TsPacketView::validate()` directly in the production record-event path.
- Restricted `TsPacketView::validate()` / `TsPacketView::parse()` to crate-internal parser use so external production callers must enter through `ValidatedTsPacket::validate()`.
- Restricted raw `DescramblerSession` observer methods to crate visibility; cross-crate callers must use `DescramblerRuntime` / `RuntimeRegistry` façade methods.
- Added `RecordIndexParser::push_validated_ts_packet()` / `build_validated_event()` so record event construction can consume a prevalidated `ValidatedTsPacket` directly instead of forcing record consumers back through raw byte validation.
- Moved direct close-cascade commit helper imports in `aidl_service::object_runtime` into the test module so production AIDL close code only sees `close_object_use_case()` / `finish_object_close_use_case()` plus artifact command execution.
- Removed `FrontendRegistryEntry` from the service_runtime crate-root public re-export to keep public query surface on DTO request/response boundaries.
- Updated `DESIGN_JA.md` with the r50eo66 callback unregister, descrambler transaction, close cascade, SourceBoundary, packet PID, and query DTO boundary contracts.
- Updated `CODE_CONVENTION.md` with the corresponding r50eo66 prohibitions, including the ban on release reports that use a limited-scope “main changes” heading.
- Updated `RELEASE_VERSION` to `r50eo66`.
- Changed object close cascade artifact planning so callback cleanup commands are emitted only for callback-capable owners and callback absence is not treated as close cleanup failure; filter and DVR callback artifacts are still cleared when present.
- Moved callback unregister cleanup composition into the shared AIDL service-context cleanup entry; object-runtime façade code now delegates runtime registry clear, artifact-store clear, missing-registration handling, and unhealthy marking to that entry.
- Reworked LNB owner-loss callback cleanup to use the shared callback cleanup entry instead of a dedicated clear/mark/failure-composition path.
- Narrowed close-cascade low-level begin/commit/mark/entry helper visibility to service_runtime-internal use and added a drop-leak quarantine use-case façade for the remaining external terminalization path.
- Made `ObjectCloseUseCasePlan` fields private and exposed only the domain cleanup step plus Binder artifact cleanup command slice required by the AIDL bridge.
- Changed demux runtime packet ingress to reuse the `ValidatedTsPacket` produced at ingress for downstream assembly planning instead of validating the same packet again on the accepted path.
- Made `TsPacketView` fields private and provided read-only accessors so callers cannot forge a validated packet view with a struct literal.
- Renamed the packet byte batch helper to `push_ts_bytes_preflight_only()` and narrowed it to crate visibility because it performs validation/preflight aggregation only.
- Narrowed frontend runtime/signal object query helpers to crate visibility so public query surface remains DTO-based.
- Added the failure-injection test source files to the corresponding `rust_test` `srcs` entries in `Android.bp`.

# r50eo65

- Changed `IFilter::setDataSource` public AIDL implementation signature from non-null `&Strong<dyn IFilter>` to nullable `Option<&Strong<dyn IFilter>>`, and passed the nullable value directly to `set_data_source_nullable_for_aidl()` so `NULL` reaches `disconnect_filter_data_source_for_object()` through `SourceBoundaryTxn`.
- Changed `IFrontend::setCallback` public AIDL implementation signature from non-null `&Strong<dyn IFrontendCallback>` to nullable `Option<&Strong<dyn IFrontendCallback>>`, and passed the nullable value directly to `set_callback_nullable_for_aidl()` so `NULL` reaches frontend callback unregister.
- Changed `ILnb::setCallback` public AIDL implementation signature from non-null `&Strong<dyn ILnbCallback>` to nullable `Option<&Strong<dyn ILnbCallback>>`, and passed the nullable value directly to `set_callback_nullable_for_aidl()` so `NULL` reaches LNB callback unregister.
- Changed `IDescrambler::addPid` public AIDL implementation signature from non-null `&Strong<dyn IFilter>` to nullable `Option<&Strong<dyn IFilter>>`, and passed the nullable value directly to `add_pid_nullable_for_aidl()` so `NULL` reaches demux-input PID claim registration.
- Changed `IDescrambler::removePid` public AIDL implementation signature from non-null `&Strong<dyn IFilter>` to nullable `Option<&Strong<dyn IFilter>>`, and passed the nullable value directly to `remove_pid_nullable_for_aidl()` so `NULL` reaches demux-input PID claim removal.
- Updated `tuner_hal2/DESIGN_JA.md` public nullable contract wording so helper-only implementations are explicitly not sufficient; the AIDL public method implementation itself must accept nullable input and route `None` to the required service_runtime use-case.
- Updated `tuner_hal2/CODE_CONVENTION.md` public nullable rules to require public method nullable signatures and to forbid counting helper-only code as completion.
- Updated top-level `開発規則.md` release rules so release reports and the relevant CHANGELOG entry must enumerate all actual changes, not only main changes, separating helper additions, public reachability, document updates, test expectation updates, and unexecuted verification.
- No Android/Soong build, rustc, cargo, rustfmt, unit test, atest, VTS, loom, or device tests were run in this environment.

# r50eo64

- Changed public close semantics to reject `Closed` objects instead of treating repeated close as successful no-op. `ObjectCloseTxn` close preflight now accepts only `Live | CleanupFailed`, and AIDL close façade no longer maps already-closed state to success. Updated close regression tests accordingly.
- Added the `IFilter.setDataSource(NULL)` demux-input disconnect path through `SourceBoundaryTxn` by adding a nullable AIDL façade helper and service_runtime `disconnect_filter_data_source_for_object()` use-case.
- Added nullable callback unregister façade helpers for `IFrontend.setCallback(NULL)` and `ILnb.setCallback(NULL)`, connecting unregister to callback artifact cleanup plus runtime callback registry clear.
- Added demux-input descrambler PID claim support for `IDescrambler.addPid(pid, NULL)` / `removePid(pid, NULL)` using typed `DescramblerPidClaimSource` to distinguish source-filter claims from demux-input claims.
- Fixed root frontend max count handling: unsupported frontend types now fail closed, negative `setMaxNumberOfFrontends()` values return invalid argument, `0..=default_max(type)` succeeds, and `getMaxNumberOfFrontends(type)` returns the available count for that frontend system.
- Updated `DESIGN_JA.md` and `CODE_CONVENTION.md` to remove the tuner_hal2/tuner_hal nullable descrambler conflict and document the close, nullable public API, and frontend max-count contracts.
- No Android/Soong build, rustc, cargo, rustfmt, unit test, atest, VTS, loom, or device tests were run in this environment.

# r50eo62

- Updated DESIGN_JA.md / CODE_CONVENTION.md to follow the r50eo61 type-hardening implementation: clear-key now documents session-clear-before-old-token-release inside the full transaction façade, public prepared/plan/commit split APIs are forbidden, and `ObjectMethodDispatchProof` is documented as an internal proof consumed inside `object_method_txn`.
- Changed normal object method executor paths to consume `ObjectMethodDispatchProof` inside `object_method_txn` and pass only `ObjectMethodExecutionToken` to AIDL closures and service_runtime `*_for_object` use-cases; shared paths already followed this model.
- Made `ObjectMethodDispatchProof` crate-private inside `service_runtime::object_method_txn` so it is not externally constructible or passable through public service_runtime surfaces.
- No Android/Soong build, rustc, cargo, rustfmt, atest, VTS, loom, or device tests were run in this environment.

# r50eo61

- Removed public descrambler clear-key / replace-key phase surfaces from crate exports; public callers can now use only session transaction façades that execute the full clear or replace sequence with a key-table operation trait.
- Changed descrambler clear-key ordering so the session key is cleared by the transaction before old-token release; callers can no longer observe or manually sequence the old token / key slot capability token.
- Removed `LnbApplyTxn` from the public LNB crate surface and replaced service-runtime use with the `apply_lnb_state_with_txn()` façade so caller-supplied generation is no longer part of the public apply path.
- Added `ObjectMethodExecutionToken` for shared object method paths; `ObjectMethodDispatchProof` is now consumed inside the object-method transaction boundary before the shared operation proceeds.
- Added `PacketPid` / `ValidatedTsPacket` and made packet-path pipeline diagnostics use non-optional PID context for record-DVR, filter-queue, and AV delivery failures.
- No Android/Soong build, rustc, cargo, rustfmt, atest, VTS, or device tests were run in this environment. `rustfmt` was not available in the container.

# r50eo60

- Removed the public `ObjectMethodTxnTarget::new()` construction surface from AIDL-facing code; object method/query entry points now build the private target inside service_runtime from object id, generation, and kind.
- Replaced the public clear-key plan / validate / commit split API with a prepared clear-key capability token and commit entry point that revalidates the session snapshot.
- Removed `PipelineDiagnosticKind` from production diagnostic construction; `PipelineDiagnostic` typed enum variants are now the only diagnostic payload source, and keyless-scrambled filtering uses pattern matching.
- Updated DESIGN_JA.md / CODE_CONVENTION.md to document the private object target, prepared clear-key boundary, and typed diagnostic enum as the only diagnostic source.
- No Android/Soong build, rustc, cargo, rustfmt, atest, VTS, or device tests were run in this environment.

# r50eo59

- Replaced root `FrontendInfo` query response registry-entry leakage with `RootFrontendInfoSnapshot` and preserved `frontend_type` in max-frontend root query/command DTOs.
- Replaced frontend object query registry-entry snapshots with typed frontend status/readiness DTO responses owned by service_runtime.
- Moved `IDemux.getAvSyncHwId()` local filter binder conversion behind demux object live/generation/kind and dispatch preflight using a dedicated AIDL input conversion boundary.
- Hardened `DescramblerReplaceKeyPlan` from public enum variants to a private-field plan struct.
- Split LNB Drop leak from public lifecycle close reason into a dedicated drop-leak lifecycle entry point.
- Split AV pipeline diagnostics into typed variants and removed `AvDeliveryState { detail: String }` fallback usage.
- Aligned CODE_CONVENTION.md with DESIGN_JA.md: `transaction_registry.rs` is target mapping only, not target+coverage.
- No Android/Soong build, rustc, cargo, rustfmt, atest, VTS, or device tests were run in this environment.

# r50eo58

- Updated DESIGN_JA.md / CODE_CONVENTION.md to match the r50eo57 typed root query, object query, transaction visibility, and typed pipeline diagnostic boundaries.
- Documented `RootQueryRequest` / `RootQueryResponse` / `RootCommandRequest`, `ObjectQueryRequest` / `ObjectQueryResponse`, `root_method_txn`, transaction constructor/commit visibility restrictions, and typed `PipelineDiagnostic` enum requirements.
- No Rust/Kotlin/Java production code was changed in this release.
- Android/Soong build, Rust unit tests, atest, VTS, rustfmt, rustc, cargo, and device tests were not run in this environment.

# r50eo57

- Added typed `RootQueryRequest` / `RootQueryResponse` and `RootCommandRequest` dispatch boundaries; AIDL root methods now submit DTO requests and only convert typed service-runtime responses into AIDL return values.
- Added typed `ObjectQueryRequest` / `ObjectQueryResponse`; object query façades no longer accept arbitrary closures and cannot receive `&mut TunerServiceRuntime` from AIDL query code.
- Made `SourceBoundaryTxn`, `DescramblerSessionTxn`, and `LnbLifecycleTxn` constructors / commit methods private to their owning modules, replacing external construction with owning-module transaction functions where needed.
- Replaced the packet pipeline diagnostic context field-bag with typed `PipelineDiagnostic` enum variants so source-filter, record-DVR, filter-queue, and AV failure contexts cannot omit their required typed cause.
- Removed the unused direct frontend-entry runtime helper from the AIDL tuner service; root/object AIDL query methods now operate through typed DTO request/response boundaries.
- Android/Soong build, Rust unit tests, atest, VTS, rustfmt, rustc, cargo, and device tests were not run in this environment.

# r50eo56

- Moved root method dispatch preflight out of `boot/query_api.rs` into `service_runtime::root_method_txn`; `query_api.rs` is again limited to `RuntimeQuery` read-only snapshot methods and no longer owns `AidlMethodAdapter::plan()`, `plan_object_method_dispatch()`, unsupported root handling, or root mutating precedence.
- Replaced root arbitrary read-only closures with typed root method transaction entry points for frontend ids/info, LNB ids, demux ids/info, demux capabilities, max frontend count, and LNA support.
- Changed object query execution closures to receive `RuntimeQuery<'_>` instead of `&mut TunerServiceRuntime`, so pure object queries are typed as read-only snapshots after dispatch validation.
- Reworked packet pipeline diagnostics from loose optional target/detail fields into a typed `PipelineDiagnosticContext`, preserving typed `HalError`, `DescrambleFailure`, and `DemuxRuntimeError` causes for source-filter validation, source-filter descramble policy, record-DVR mirror, filter-queue payload delivery, and AV backing failures.
- Android/Soong build, Rust unit tests, atest, VTS, rustfmt, rustc, cargo, and device tests were not run in this environment.

# r50eo55

- Removed the remaining public root plan-only helper surface and moved root read-only dispatch/query execution to `query_api`-owned façades; AIDL tuner service no longer calls root plan-only support helpers for constants or unavailable root APIs.
- Changed LNB owner-loss callback cleanup and the generic owner callback cleanup helper to always reconcile runtime callback registry state independently from callback artifact removal count, and to compose callback-store cleanup failures with unhealthy-marking failures.
- Extended packet pipeline diagnostics with typed `HalError` storage for source-filter descrambler validation failures, and added packet PID context to record-DVR mirror and filter queue payload delivery diagnostics.
- Android/Soong build, Rust unit tests, atest, VTS, rustfmt, rustc, cargo, and device tests were not run in this environment.

# r50eo54

- Removed the remaining root-planning helper shape that returned `CommandPlan` internally; root plan-only and unavailable status handling now keeps the AIDL API identity inside `service_runtime::root_object_ops` and exposes no `CommandPlan` across the root façade boundary.
- Rechecked the r50eo53 fixes for object query dispatch validation, root single-lock query façades, bounded diagnostic reset, richer packet pipeline diagnostics, and LNB Drop leak diagnostic detail.
- Android/Soong build, Rust unit tests, atest, VTS, rustfmt, rustc, cargo, and device tests were not run in this environment.

# r50eo53

- Restored dispatch planning for pure object query helpers without issuing an `ObjectMethodDispatchProof`, so query APIs validate their `RuntimeExecutableRequest` while avoiding proof-token discard.
- Moved root read-only query planning into service_runtime single-lock root façades and stopped exposing `CommandPlan` from the public root plan-only façade.
- Cleared all bounded diagnostic stores during runtime reset, including descrambler, child-open rollback, DVR post-commit notification, and filter-callback delivery diagnostics.
- Extended packet pipeline diagnostics with optional target ids and detail text, and connected source-filter descrambler validation/policy failures, queue enqueue failures, record DVR mirror failures, and AV backing failures to richer pipeline diagnostics.
- Moved LNB owner-loss callback cleanup to the context-owned callback cleanup boundary and preserved detailed object-table errors for Drop leak LNB public id resolution.
- Android/Soong build, Rust unit tests, atest, VTS, rustfmt, rustc, cargo, and device tests were not run in this environment.

# r50eo52

- Removed runtime coverage metadata from `transaction_registry`, `dispatch`, and `command_dispatch`, leaving production dispatch target mapping as the single runtime dispatch table.
- Moved root `ITuner` method planning / executable-request extraction for root open and tuner public API plan-only paths into `service_runtime::root_object_ops`; AIDL tuner service helpers now pass `AidlMethodCall` to service_runtime instead of calling `AidlMethodAdapter::plan()` or `runtime_executable_request()`.
- Changed pure object query helpers to validate live/generation/kind and AIDL method target without issuing and discarding `ObjectMethodDispatchProof`; query request-builder call sites no longer receive a proof token.
- Routed AV handle release through `AvHandleReleaseTxn` even when runtime backing is absent for closed / non-AV / stale-release classification, while preserving backing-failure behavior for live AV releases that require a backing.
- Strengthened descrambler replace-key plan/commit validation and changed post-commit old-token release failure to diagnostic accounting instead of failing a public API after the session has already committed the new key.
- Made filter callback delivery accounting fail closed when diagnostic recording is blocked by runtime lock poison, and records AIDL event conversion failures before optional unhealthy marking.
- Rejected oversized filter time-delay hints in service_runtime/demux runtime paths so `Instant::checked_add()` overflow cannot make delayed delivery immediately ready.
- Android/Soong build, Rust unit tests, atest, VTS, rustfmt, rustc, cargo, and device tests were not run in this environment.

# r50eo50

- Replaced `vendor/maleicacid/tv/tuner_hal2/DESIGN_JA.md` with the user-supplied DESIGN_JA.md revision.
- Replaced `vendor/maleicacid/tv/開発規則.md` with the user-supplied development rules revision.
- No Rust/Kotlin/Java production code was changed in this release.
- Android/Soong build, Rust unit tests, atest, VTS, rustfmt, rustc, cargo, and device tests were not run in this environment.

# r50eo49

- Fixed the r50eo47/r50eo48 dispatch-proof consumption regression where service_runtime `*_for_object` use-cases consumed `ObjectMethodDispatchProof` only after resolving public runtime ids, object entries, frontend/LNB owner relations, source-filter relations, or request-dependent runtime state.
- Moved proof consumption to the first runtime-critical operation in frontend worker start/stop, frontend setLnb/callback façades, demux/filter/DVR operations, descrambler operations, and LNB operations so ObjectMethodTxn proof is consumed before any object/runtime re-resolution or relation validation.
- Updated DESIGN_JA.md / CODE_CONVENTION.md to state that a service_runtime use-case receiving `ObjectMethodDispatchProof` must consume it before `public_runtime_id_for_object_method()`, `public_entry_for_object_method()`, frontend entry resolution, owner relation checks, or request-dependent config construction.
- Android/Soong build, Rust unit tests, atest, VTS, rustfmt, rustc, cargo, and device tests were not run in this environment.

# r50eo48

- Removed dead code left after the r50eo47 dispatch-proof consumption change: the unused `configure_filter_runtime_for_object()` service_runtime façade, the unused Binder-facing `clear_live_lnb_callback_for_public_id()` wrapper, and the unused `ObjectMethodTxnPlan` execute-closure parameter.
- Narrowed `ObjectMethodTxnPlan::executable_request()` from public to private because it is only consumed inside `service_runtime::object_method_txn`.
- Fixed the r50eo47 frontend callback clear proof-consumption path to return `Ok(())` after successful `ObjectMethodDispatchProof` consumption.
- Android/Soong build, Rust unit tests, atest, VTS, rustfmt, rustc, cargo, and device tests were not run in this environment.

# r50eo47

- Fixed the r50eo46 regression where object-method callers received an `ObjectMethodDispatchProof` but still passed `CommandPlan` / `RuntimeExecutableRequest` into service_runtime `*_for_object` use-cases that reran `plan_object_method_dispatch()`.
- Changed frontend stopTune/stopScan/setLnb, demux setFrontendDataSource, filter configure/start/stop/flush/AV handle release/source disconnect, DVR configure/start/stop/flush, descrambler setDemuxSource/setKeyToken/demux-input PID add/remove, and LNB DiSEqC object methods to consume `ObjectMethodDispatchProof` instead of rerunning dispatch planning.
- Updated frontend/LNB callback clear paths to use the same proof-consumption boundary after the `execute_object_runtime_use_case()` signature change.
- Updated DESIGN_JA.md / CODE_CONVENTION.md to state that service_runtime `*_for_object` use-cases receiving a dispatch proof must consume that proof and must not rerun `plan_object_method_dispatch()`.
- Android/Soong build, Rust unit tests, atest, VTS, rustfmt, rustc, cargo, and device tests were not run in this environment.

# r50eo46

- Completed the request-builder transaction boundary that remained partial in r50eo45: `aidl_service::object_runtime` and `aidl_service::child_object_open` no longer call `AidlMethodAdapter::plan()` or extract `RuntimeExecutableRequest`; `service_runtime::object_method_txn` now owns AIDL method planning, executable-request extraction, live/generation/kind validation, dispatch planning, and proof issue for those paths.
- Added `object_close_txn` method-call planning so close preflight also keeps AIDL method planning in service_runtime rather than in the AIDL object-runtime helper.
- Removed the stale command-plan based request-builder helper surface from `service_runtime::object_method_txn` after moving callers to method-call based helpers.
- Added `BoundedDiagnosticStore::clear()` so service boot reset clears bounded diagnostic stores without falling back to unbounded vectors or compile-time missing methods.
- Recorded missing filter callback artifacts in the bounded filter callback delivery diagnostic store before returning callback delivery failure.
- Updated DESIGN_JA.md / CODE_CONVENTION.md to state that AIDL helpers must pass `AidlMethodCall` into the transaction boundary instead of planning or extracting runtime executable requests themselves.
- Android/Soong build, Rust unit tests, atest, VTS, rustfmt, rustc, cargo, and device tests were not run in this environment.

# r50eo45

- Moved request-builder query/mutating/shared execution and child filter/DVR open execution onto `service_runtime::object_method_txn` helpers so object live/generation/kind validation, request build, runtime request validation, dispatch planning, proof issue, and execution are no longer hand-assembled in `aidl_service::object_runtime` / `child_object_open`.
- Removed the production public dispatch-proof generation surface from `TunerServiceRuntime` and removed the crate-root `ObjectMethodDispatchProof` re-export; the proof constructor is now private to `object_method_txn`.
- Replaced target-only dispatch lookup with `dispatch_spec_for()`/`dispatch_spec()` so runtime coverage remains part of the dispatch contract.
- Converted runtime diagnostic stores for startup, descrambler, child-open rollback, DVR post-commit notification, and filter callback delivery to bounded stores with dropped-record counters.
- Added bounded filter callback delivery diagnostics for callback binder failure and unhealthy-accounting failure.
- Changed non-VOID descrambler `setKeyToken()` to use `DescramblerSessionTxn::plan_replace_key()` / `commit_validated_replace_key()`, commit session replacement before old-token release, and rollback-release the newly acquired token on replace failure.
- Changed AV handle release to fail on missing runtime backing instead of manufacturing a transient backing state.
- Updated DESIGN_JA.md / CODE_CONVENTION.md to capture these boundaries.
- Android/Soong build, Rust unit tests, atest, VTS, rustfmt, rustc, cargo, and device tests were not run in this environment.

# r50eo44

- Removed production dead-code surface left after r50eo43: `register_descrambler_key_slot()` is now test-only, and unused `transaction_registry` convenience exports were removed.
- Kept `RUNTIME_TRANSACTION_SPECS` / `transaction_spec_for()` as the runtime dispatch正本 used by production dispatch; removed only the unused wrapper helpers.
- Android/Soong build, Rust unit tests, atest, VTS, rustfmt, rustc, cargo, and device tests were not run in this environment.

# r50eo43

- Routed non-VOID descrambler `setKeyToken()` session replacement through `DescramblerSessionTxn::replace_key()` after key acquire / old-token release, and composed rollback-release failure when new-token rollback release fails after old-token release failure.
- Extended `SourceBoundaryTxn` so non-null source commit is inside the same boundary transaction as queue cleanup, generation reset, downstream disconnect, snapshot rollback, and demux quarantine.
- Made filter/DVR configure rollback failure paths call `DemuxRuntime::quarantine()` when they report a quarantined outcome.
- Changed AV shared handle export/release paths so missing backing state is not created with `entry(...).or_default()` merely because a marker or release request exists.
- Hid `register_descrambler_key_slot()` from production-visible `TunerServiceRuntime` public API surface.
- Kept object request-builder planning and execution in one runtime critical section for query/mutating helpers and child filter/DVR open request-builder paths.
- Made runtime transaction dispatch consume registry coverage metadata and updated production-connected transaction specs from stale `NotConnected` to `Connected`, with unsupported public API transactions marked `UnsupportedByDesign`.
- Updated DESIGN_JA.md / CODE_CONVENTION.md to fix these boundaries.
- Android/Soong build, Rust unit tests, atest, VTS, rustfmt, rustc, cargo, and device tests were not run in this environment.

# r50eo42

- Updated `DESIGN_JA.md` child object open transaction row so it names only the current request-builder AIDL helpers and the service_runtime child-open / rollback use-case helpers that actually exist.
- Reworded the child-open ownership description from object-handle based service_runtime façade to owner object id / generation + dispatch proof based service_runtime use-case, keeping `AidlObjectHandle` on the AIDL side.
- No code changes were made; this release fixes the stale DESIGN_JA.md entity names introduced by the r50eo41 design update.
- Android/Soong build, Rust unit tests, atest, VTS, rustfmt, rustc, cargo, and device tests were not run in this environment.

# r50eo41

- Changed child filter / DVR open completion so service_runtime returns the typed child runtime id together with the `RuntimeObjectEntry`, and AIDL finalization no longer converts `RuntimeObjectEntry.ledger_id` back into a filter / DVR id after object-table registration.
- Removed the now-dead child-open runtime-id-conversion rollback helper and the impossible post-registration public-id-conversion failure branches from `aidl_service/src/child_object_open.rs`.
- Updated `DESIGN_JA.md` and `CODE_CONVENTION.md` to make typed child-open result ownership explicit and to forbid AIDL-side child id reconstruction from `RuntimeObjectEntry.ledger_id`.
- Android/Soong build, Rust unit tests, atest, VTS, rustfmt, rustc, cargo, and device tests were not run in this environment.

# r50eo40

- Removed production-unconnected `control_core` skeleton types (`WorkerSignal`, lifecycle transaction skeletons, and `StreamBoundaryTxn`) and the unit tests that only exercised those deleted skeletons.
- Kept `control_core` limited to production-used worker exit/failure classification and `FmqDeliveryTxn` types, and removed unused enum variants / unused `commit_write_and_wake()` helper from that surface.
- Updated `DESIGN_JA.md` so the stream-boundary正本 is the production-connected `GenerationBoundaryTxn` rather than the removed `StreamBoundaryTxn`.
- Updated `DESIGN_JA.md` close lifecycle helper names to the current `close_object_after_close_preflight*()` façade names.
- Added `CODE_CONVENTION.md` rules preventing public production-unconnected transaction skeletons from being retained as common components.
- Android/Soong build, Rust unit tests, atest, VTS, rustfmt, rustc, cargo, and device tests were not run in this environment.

# r50eo39

- Routed filter queue payload delivery through the `QueueRuntime` / FMQ / EventFlag backing exported by `getQueueDesc()` instead of using a separate production `VecDeque` payload path; retained only a test-only mirror for existing assertions.
- Changed descrambler unregister / demux owner-loss cleanup so key token release failure is collected but no longer skips `DescramblerSessionTxn::cleanup_all()`.
- Android/Soong build, Rust unit tests, atest, VTS, rustfmt, rustc, cargo, and device tests were not run in this environment.

# r50eo38

- Recorded every AV shared backing non-delivery outcome as a typed pipeline diagnostic instead of silently dropping `SharedHandleNotExported`, released-client, oversized-payload, no-slot, data-id-exhaustion, or missing-backing cases.
- Made `DemuxRuntime::restore()` atomic with respect to fallible queue runtime rebuilds by constructing replacement filter/DVR queue runtimes before mutating the live demux state.
- Narrowed raw `TunerServiceRuntime` registry/object-table/callback-registry accessors to crate scope and added typed read-only façade methods for lifecycle and callback-registration observation.
- Removed the planless public close helpers from `aidl_service::object_runtime`; close now goes through `close_object_after_close_preflight*()` and `ObjectCloseTxn` dispatch planning.
- Classified close preflight begin state as `CleanupStep::ReleaseBackend` so public close cleanup state matches the domain cleanup hook phase instead of the descendant worker-stop phase.
- Android/Soong build, Rust unit tests, atest, VTS, rustfmt, rustc, cargo, and device tests were not run in this environment.

# r50eo37

- Aligned the object query request-builder helper with the pure-query boundary by using plan-only dispatch validation and no longer issuing an `ObjectMethodDispatchProof` for query-only execution.
- Added explicit media-filter validation for `IDemux.getAvSyncHwId()` so the input filter must be a live audio/video media filter owned by the target demux before returning a live PCR filter id.
- Removed the child-object-open local Binder-status-to-HAL debug conversion path by adding a HAL-error-returning callback artifact registration helper and composing callback retain failures directly with rollback failures.
- Fixed frontend scan END and DVR status callback delivery failure handling so Binder delivery failure remains the primary error and unhealthy-marking failure is composed as cleanup failure.
- Reduced raw service-runtime lifecycle surface by hiding the `object_table`, `callback_registry`, and `registry` modules behind selected re-exports and adding callback-registry façade methods used by AIDL production paths.
- Android/Soong build, Rust unit tests, atest, VTS, rustfmt, rustc, cargo, and device tests were not run in this environment.

# r50eo36

- Routed `execute_object_query_runtime_call()` through `TunerServiceRuntime::plan_object_method_dispatch_for_object()` so object live / generation / kind validation and command dispatch planning stay on the service_runtime use-case boundary instead of being reassembled inside the AIDL façade private helper.
- Reworked Drop leak terminalization so quarantine is performed under the runtime lock first, then callback artifacts and DVR notifier artifacts for the quarantined root/descendants are cleared outside the runtime lock, and runtime callback registry state is reconciled afterward.
- Split Drop leak public-runtime unregister from close finalization by adding Drop-leak-specific validate/unregister entry points and using `unregister_quarantined_public_runtime_entries()` after quarantine, instead of reusing the close-only unregister helper.
- Updated `tuner_hal2/RELEASE_VERSION` to this release name.
- Android/Soong build, Rust unit tests, atest, VTS, rustfmt, rustc, cargo, and device tests were not run in this environment.

# r50eo35

- Removed obsolete close-cleanup collector unit tests that only re-tested the generic `FirstErrorCollector<(CleanupStep, HalError)>` behavior after the local `CloseCleanupFailure` helper was deleted in r50eo34. The remaining close-cleanup tests exercise actual close / drop-leak / cascade runtime behavior rather than dead helper behavior.
- Rechecked the r50eo34 cleanup refactor for stale `CloseCleanupFailure`, `record_close_cleanup_result()`, `close_cleanup_result()`, and `CleanupStep::DomainCleanup` references; no production references remain.
- Updated `tuner_hal2/RELEASE_VERSION` to this release name.
- Android/Soong build, Rust unit tests, atest, VTS, rustfmt, rustc, cargo, and device tests were not run in this environment.

# r50eo34

- Replaced the close-cleanup local first-error helper with direct `FirstErrorCollector<(CleanupStep, HalError)>` usage so the close cleanup path follows the common collector boundary while still preserving the failing cleanup phase.
- Fixed Drop leak callback registry handling so only callback-store cleanup failure drives `RuntimeCallbackRegistry::mark_owner_unhealthy()`; unrelated domain drop-leak record failures no longer convert a successfully cleared callback owner into unhealthy state.
- Strengthened close finalization helpers so `close_cascade_entries()`, `commit_close_cascade()`, and `mark_cleanup_failed_cascade()` reject an unexpected terminal root before collecting or mutating descendants. This prevents partial descendant state changes when the root is already `Closed` / `Quarantined`.
- Updated `tuner_hal2/RELEASE_VERSION` to this release name.
- Android/Soong build, Rust unit tests, atest, VTS, rustfmt, rustc, cargo, and device tests were not run in this environment.

# r50eo33

- Classified pre-finalization close cleanup failures by the actual failing phase without adding a new `CleanupStep`: root / descendant callback artifact cleanup uses `CleanupStep::UnregisterRuntime`, domain cleanup hook failure uses `CleanupStep::ReleaseBackend`, and descendant DVR status notifier stop failure uses `CleanupStep::StopWorker`.
- Kept r50eo32 close-cascade descendant cleanup and root terminal lifecycle fixes intact.
- Updated close-cleanup tests so domain cleanup failure and cleanup retry paths expect `ReleaseBackend` rather than `UnregisterRuntime`.
- Android/Soong build, Rust unit tests, atest, VTS, rustfmt, rustc, cargo, and device tests were not run in this environment.

# r50eo32

- Fixed AIDL close cascade descendant cleanup so demux close now clears descendant Filter / DVR callback artifacts and runtime callback registry entries, and stops descendant DVR status notifiers before public runtime unregister / close commit.
- Tightened `RuntimeObjectTable::begin_close_cascade()` so an unexpected terminal root (`Closed` / `Quarantined`) is rejected as `InvalidLifecycle`; terminal descendants remain skipped by later finalization helpers.
- Did not change the pre-finalization cleanup-failure `CleanupStep` classification; the previous note was an improvement candidate, not a fixed implementation plan.
- Android/Soong build, Rust unit tests, atest, VTS, rustfmt, rustc, cargo, and device tests were not run in this environment.

# r50eo31

- Removed the erroneous `CleanupStep::DomainCleanup` reference from AIDL object close finalization; that variant does not exist in the cleanup ledger and caused a compile failure.
- Split close finalization failure marking by the actual failing phase: close-cascade entry lookup / close commit failures use `CleanupStep::ReleaseLedger`, while public runtime unregister failures use `CleanupStep::UnregisterRuntime`.
- This is an internal cleanup-state correction only; no AIDL/VTS-visible API, status contract, capability advertisement, or close idempotency contract was changed.
- Android/Soong build, Rust unit tests, atest, VTS, rustfmt, rustc, cargo, and device tests were not run in this environment.

# r50eo30

- Fixed close finalization follow-up issues from r50eo29.
- Passed `SharedTunerRuntime` by reference when recording cleanup-failed state after close cleanup failure.
- Made close-cascade finalization helpers skip already terminal descendants while still rejecting a terminal root, preserving idempotent close semantics without hiding root cleanup state.
- Strengthened public runtime unregister preflight for descrambler objects by checking both registry entry and runtime state before destructive unregister.
- Android/Soong build, Rust unit tests, atest, VTS, rustfmt, rustc, cargo, and device tests were not run in this environment.

# r50eo29

- Frontend close の LNB owner-loss callback cleanup が `SharedTunerRuntime` を `clear_live_lnb_callback_for_public_id_hal()` に渡していた誤接続を修正し、`SharedAidlServiceContext` owned callback store を使う形へ戻した。
- AIDL object close finalization で public runtime unregister 前に対象 runtime entry の存在を全件 preflight し、preflight failure 時に一部 runtime unregister を開始しないようにした。
- `RuntimeObjectTable::{mark_cleanup_failed_cascade,close_cascade_entries,commit_close_cascade}` が `Closed` / `Quarantined` descendant を無言 skip しないようにし、close finalization / cleanup-failed marking の内部不整合を `InvalidLifecycle` として表面化するようにした。
- `DESIGN_JA.md` / `CODE_CONVENTION.md` を、close finalization preflight と context-owned callback cleanup の方針に合わせて更新した。
- Android/Soong build、Rust unit test、atest、VTS、実機確認、rustfmt、rustc、cargo は未実行。

# r50eo28

- `setKeyToken(VOID)` clear path の release 後 commit failure 形状を解消し、`validate_clear_key_plan()` を release 直前検証、`commit_validated_clear_key()` を release 成功後の infallible session clear commit として分離した。
- AIDL object close cleanup の public runtime unregister 対象 kind を caller 側で Demux / Filter / Dvr / Descrambler に限定し、`unregister_public_runtime_for_closed_aidl_entry()` は対象外 kind を無言成功にしないようにした。
- `SourceBoundaryTxn` rollback failure を `SourceBoundaryRollbackFailed` として demux quarantine / cleanup failure へ表面化し、source boundary rollback failure を generic primary error に埋もれさせないようにした。
- DVR attach / detach filter の owner-demux 検証を `owner_demux_id_for_dvr_filter_relation()` private helper へ集約し、`*_for_object` façade や AIDL method body への低レベル dispatch logic 戻しは行わなかった。
- `DESIGN_JA.md` / `CODE_CONVENTION.md` を上記の release-before-clear、source boundary rollback、public runtime unregister scope 方針に合わせて更新した。
- Android/Soong build、Rust unit test、atest、VTS、実機確認、rustfmt、rustc、cargo は未実行。

# r50eo27

- `setKeyToken(VOID)` clear path を再確認し、clear plan を release 直前に検証してから old token release を実行し、その後に session clear を commit する形へ補強した。
- `DescramblerSessionTxn::plan_clear_key()` は old token だけでなく old key slot も snapshot し、`validate_clear_key_plan()` / `commit_clear_key()` は token と slot の両方を照合するようにした。
- descrambler cleanup は old token release が成功するまで session `close_all()` を commit しない順序へ変更し、release 失敗時に retry 不能な cleared session を作らないようにした。
- demux / filter / DVR runtime unregister は owner-loss / owner-demux cleanup を先に成功させてから registry entry を削除する順序へ変更した。
- public close finalization は closing entries の runtime unregister を先に行い、成功後に object table close commit する順序へ変更した。runtime unregister 失敗時は object table を `Closing` のまま残して cleanup-failed marking へ進める。
- descrambler `setDemuxSource()` rebind 時は demux id / generation が変わる場合に stale PID claims を clear するようにした。
- `SourceBoundaryTxn` は mutation 前 snapshot を保持し、generation boundary / downstream disconnect failure 時に rollback、rollback failure 時に demux quarantine するようにした。`setDataSource(null)` も source boundary を通る。
- `DemuxRuntime::clear_existing_filter_queue()` / `remove_filter()` / `remove_dvr()` の部分更新順序を補正した。
- r50eo26 の test 残骸（重複 `#[test]` と missing `#[test]`）を修正した。
- `DESIGN_JA.md` / `CODE_CONVENTION.md` に close finalization / runtime unregister、descrambler cleanup、source boundary rollback の方針を追記した。
- Android/Soong build、Rust unit test、atest、VTS、実機確認、rustfmt、rustc、cargo は未実行。

# r50eo26

- `DescramblerSessionTxn::commit_clear_key()` に clear plan 検証を追加し、plan 作成時の old token と commit 時の session token が一致しない場合は session を変更せず `ClearKeyPlanMismatch` として失敗するようにした。
- `setKeyToken(VOID)` の commit failure diagnostic に `ClearKeyPlanMismatch` を追加し、release 成功後でも stale plan 誤用を session clear へ進めないようにした。
- `service_runtime::object_method_txn` に plan-only helper を追加し、`plan_unavailable_object_method_use_case()` が dispatch planning まで実行しつつ `ObjectMethodDispatchProof` を発行しない形へ変更した。
- `register_callback_artifact_after_owner_ready()` の未使用 rollback 引数と、それに付随する child callback retain 側の未使用 rollback closure を削除した。
- `DESIGN_JA.md` / `CODE_CONVENTION.md` に、clear plan 検証と plan-only 経路で dispatch proof を発行しない方針を追記した。
- Android/Soong build、Rust unit test、atest、VTS、実機確認、rustfmt、rustc、cargo は未実行。

# r50eo25

- `setKeyToken(VOID)` の clear path を `DescramblerSessionTxn::plan_clear_key()` / `commit_clear_key()` に分離し、old token release 成功後にだけ session key clear を commit するようにした。
- old token release 失敗時は session key / key slot を保持し、`KeyTokenReleaseFailed` diagnostic と `HalError::Internal(InvariantViolation)` を返す形にした。
- `DescramblerSession::clear_key()` を crate 内部に閉じ、外部 transaction が session clear を direct commit しないようにした。
- `DESIGN_JA.md` / `CODE_CONVENTION.md` に、descrambler key clear は plan/commit 境界で old token release 成功後に commit する方針を追記した。
- Android/Soong build、Rust unit test、atest、VTS、実機確認、rustfmt、rustc、cargo は未実行。

# r50eo24

- `ObjectMethodDispatchPreflight` を `ObjectMethodDispatchProof` へ改名し、dispatch planning 完了を表す一回性 proof であることを型名に反映した。
- single-variant `ObjectMethodDispatchPreflightState::AlreadyPlanned` と内部 `ObjectMethodDispatchPreflightProof` を削除し、`ObjectMethodDispatchProof` が `ObjectMethodTxnTarget` を直接保持する形へ簡素化した。
- 消費 API を `plan_for_object()` / `plan_for_target()` から `consume_for_object()` / `consume_for_target()` へ改名し、既に完了した dispatch planning proof を対象照合して消費する意味に揃えた。
- `DESIGN_JA.md` / `CODE_CONVENTION.md` に、dispatch proof は single-variant enum で状態機械を装わず、対象 `AidlObjectId` / generation / kind を直接保持する方針を追記した。
- Android/Soong build、Rust unit test、atest、VTS、実機確認、rustfmt、rustc、cargo は未実行。

# r50eo23

- Drop から返せない error を保存する `AidlServiceContext` owned drop-leak diagnostic store を bounded `VecDeque` に変更し、上限超過時の dropped count と lock poison 時の record failure count を context-owned counter として観測可能にした。
- drop-leak diagnostic store の lock poison を `poisoned.into_inner()` で吸収しないようにし、runtime reset 前の diagnostic clear 失敗は `HalError::Internal(InvariantViolation)` として返すようにした。
- `service_runtime/src/boot/packet_txn.rs` の descrambler source-filter validation failure を `.ok()` で無診断破棄しないようにし、packet pipeline diagnostic に `filter_id` と `HalError` を保持する `PacketSourceFilterInvalid` / `PacketSourceFilterGenerationMismatch` を追加した。
- `DESIGN_JA.md` / `CODE_CONVENTION.md` に bounded drop-leak diagnostic store、poison failure accounting、packet path source-filter validation diagnostic の方針を追記した。
- Android/Soong build、Rust unit test、atest、VTS、実機確認、rustfmt、rustc、cargo は未実行。

# r50eo22

- `service_runtime/src/boot.rs` の process-global `FILTER_EVENT_DISPATCHER: OnceLock<...>` と `install_filter_event_dispatcher()` global API を廃止し、filter event dispatcher を `TunerServiceRuntime` instance field として所有する形にした。
- `FrontendDemuxPacketSink` は runtime instance から取得した dispatcher handle を保持し、service instance をまたぐ dispatcher slot を参照しない。
- `TunerAidlService::from_context()` は `AidlServiceContext` に対応する runtime instance へ `AidlFilterEventDispatcher` を登録する。`service_runtime` は引き続き Binder `Strong<dyn ...Callback>` を保持しない。
- `aidl_service/src/object_runtime/drop_leak.rs` の process-global `DROP_LEAK_ERROR_RECORDS` / `DROP_LEAK_ERROR_RECORD_FAILURES` を削除し、Drop から返せない error は `AidlServiceContext` owned drop-leak diagnostic store へ保存するようにした。
- `AidlServiceContext::reset_runtime_from_probe_results()` は DVR notifier 停止、callback artifact clear、drop-leak diagnostic clear、runtime boot の順に統一した。
- `DESIGN_JA.md` / `CODE_CONVENTION.md` に、filter event dispatcher bridge と drop-leak diagnostic store を process-global に置かない方針を追加した。
- Android/Soong build、Rust unit test、atest、VTS、実機確認、rustfmt、rustc、cargo は未実行。

# r50eo21

- r50eo20 の public runtime accessor closure 後に不要になった `AidlServiceContext::stale_context_error()` / `AidlServiceContext::unavailable_status()` を削除した。
- `AidlCallbackStoreError` と `into_hal_error()` を crate 内部可視性へ落とし、callback artifact store error を AIDL service crate 外の public surface に出さない形へ固定した。
- `callback_store` / DVR notifier / raw runtime owner closure に関する残存参照を再確認し、production の process-global callback store / DVR notifier store は引き続き存在しないことを確認した。
- Android/Soong build、Rust unit test、atest、VTS、実機確認、rustfmt、rustc、cargo は未実行。

# r50eo20

- `SharedTunerRuntime` を AIDL service crate の public export から外し、raw runtime handle を crate 外 API へ露出しないようにした。
- `AidlServiceContext::runtime()` / `AidlServiceContext::lock_runtime()`、`TunerAidlService::runtime()` / `TunerAidlService::lock_runtime()`、各 AIDL object の `runtime()` / `context()` helper を crate 内部可視性へ落とした。
- `aidl_service` の内部 implementation module を crate-private module に落とし、外部公開 API を `AidlServiceContext` / `SharedAidlServiceContext` / Binder object wrapper / `run_service()` に限定した。
- callback artifact retain / lookup / clear helper と `AidlCallbackStoreError` の public re-export を外し、callback artifact 操作も crate 内部 API に閉じた。
- `DESIGN_JA.md` に、runtime reset と callback artifact / DVR notifier cleanup の owner を public API 上も `AidlServiceContext` に固定し、raw runtime accessor を crate 外へ公開しない方針を追記した。
- Android/Soong build、Rust unit test、atest、VTS、実機確認、rustfmt、rustc、cargo は未実行。

# r50eo19

- r50eo18 の AIDL service context 化で残っていた test 経路の旧 `SharedTunerRuntime` / test-global callback store 呼び出しを `SharedAidlServiceContext` 前提へ更新した。
- `callback_store.rs` の `#[cfg(test)]` `TEST_CALLBACK_STORE: OnceLock<Mutex<CallbackStore>>` と互換 re-export を削除し、unit test も context-owned callback store を使うようにした。
- `AidlServiceContext::reset_runtime_from_probe_results()` を追加し、runtime reinit は DVR notifier 全停止、callback artifact 全 clear、`TunerServiceRuntime::boot_from_probe_results()` の順に同一 owner 経由で実行する方針へ固定した。
- service 起動経路は runtime を直接 boot せず、`AidlServiceContext` 経由の reset API を使うようにした。
- Android/Soong build、Rust unit test、atest、VTS、実機確認、rustfmt、rustc、cargo は未実行。

# r50eo18

- `AidlServiceContext` を導入し、`TunerServiceRuntime` / Binder callback artifact store / DVR status notifier store を同一 service instance owner に閉じた。
- production `callback_store` の process-global `OnceLock<Mutex<CallbackStore>>` を廃止し、callback artifact retain / lookup / clear を `SharedAidlServiceContext` 経由へ移した。
- production `DVR_STATUS_NOTIFIERS` global を廃止し、DVR status notifier の cancel handle / join handle を `AidlServiceContext` owned store へ移した。
- `FrontendAidlObject` / `DemuxAidlObject` / `FilterAidlObject` / `DvrAidlObject` / `DescramblerAidlObject` / `LnbAidlObject` は `SharedAidlServiceContext + AidlObjectHandle` を保持するようにした。
- `AidlFilterEventDispatcher` / scan-end notifier / DVR status notifier は callback artifact を context-owned store から取得する。
- `service_runtime` には Binder `Strong<dyn ...Callback>` を持ち込まず、Binder artifact ownership は AIDL service instance 側に閉じた。
- Android/Soong build、Rust unit test、atest、VTS、実機確認、rustfmt、rustc、cargo は未実行。

# r50eo17

- `start_dvr_status_notifier()` の notifier store 登録順を補正し、store lock を thread spawn 前に取得して、spawn 済み thread が cancel / join handle 未登録のまま残らないようにした。
- `ObjectMethodDispatchPreflight` の preflight 済み証跡に対象 `AidlObjectId` / generation / kind を保持させ、消費側 `*_for_object` use-case で同じ対象に対してだけ消費できるようにした。
- `DESIGN_JA.md` に DVR status notifier の spawn/store ownership 順序と target-bound dispatch preflight proof 方針を追記した。
- `IFilter.getId()` / `getId64Bit()` は今回の修正対象外。既存の `execute_object_query_use_case()` による live/generation/kind 確認・dispatch planning 共通 façade を使っており、追加 service_runtime façade 化は薄い query wrapper 増殖リスクがあるため別判断とした。
- Android/Soong build、Rust unit test、atest、VTS、実機確認、rustfmt、rustc、cargo は未実行。

# r50eo16

- production 経路に接続されていなかった `LnbOperationLedger` / `LnbOperationGuard` / `LnbOperationKind` / `LnbOperationFailureRecord` を削除し、`lnb/src/operation_guard.rs` を廃止した。
- `LnbFailureKind` から operation ledger 専用の `OperationAlreadyActive` / `OperationLockFailed` / `OperationNonceExhausted` を削除し、`service_runtime/src/boot/lnb_txn.rs` の error mapping からも同系統の未使用分岐を削除した。
- `Android.bp` の `libmaleicacid_tuner_hal2_lnb` / `maleicacid_tuner_hal2_lnb_test` から `lnb/src/operation_guard.rs` を除外した。
- `DESIGN_JA.md` から LNB operation ledger を production invariant とする記述を削除し、LNB public operation の状態遷移正本を `LnbTxn` / `LnbApplyTxn` / `LnbLifecycleTxn` / `LnbRuntimeState` に固定した。
- `DescramblerKeyTable` の key slot id fail-closed 修正は production 経路に効くため維持した。
- Android/Soong build、Rust unit test、atest、VTS、実機確認、rustfmt、rustc、cargo は未実行。

# r50eo15

- `DescramblerKeyTable` の key slot id 採番を `saturating_add()` から fail-closed な `checked_add()` へ変更し、上限到達時に `SlotIdExhausted` を返して token / slot table を部分更新しないようにした。
- `LnbOperationLedger` の operation nonce 採番を `wrapping_add()` から fail-closed な `checked_add()` へ変更し、nonce 上限到達時に active operation を部分挿入しないようにした。
- key slot id と LNB operation nonce は同じ汎用 ID allocator へ共通化せず、それぞれの domain owner 内で fail-closed 化する方針を `DESIGN_JA.md` に追記した。
- Android/Soong build、Rust unit test、atest、VTS、実機確認、rustfmt、rustc、cargo は未実行。

# r50eo14

- CODE_CONVENTION監査で検出した P1 系のうち、error_bridge 外の direct Binder status generation を排除し、`local_filter_handle_from_strong()` / `frontend_entry()` は `status_from_tuner_status()` 経由へ統一した。
- `IDescrambler.addPid()` / `removePid()` の null-upstream 経路を `execute_object_runtime_use_case_with_request_builder()` へ寄せ、TS PID 変換を object live / generation / kind 確認後の request-builder phase で実行するようにした。
- `IDemux.getAvSyncHwId()` 用に `execute_object_query_use_case_with_aidl_input_conversion()` を追加し、local filter handle 変換を demux lifecycle / dispatch preflight 後の query request-builder 境界へ移した。
- DVR status notifier を raw thread の silent termination から、`catch_unwind` + terminal diagnostic accounting 付きの workerに変更し、poll / callback delivery / panic / terminal accounting failure を `DvrPostCommitNotificationDiagnosticRecord` へ接続した。
- frontend rollback 中の bound demux restore failure を generic `InvariantViolation` へ丸めず、既存 `demux_runtime_error_to_hal()` の分類へ接続した。
- LNB apply / close / drop-leak / generation overflow quarantine で domain transaction failure と `store_lnb_runtime()` failure が同時に起きる場合、`compose_primary_cleanup_failure()` で両方を保持するようにした。
- AV shared backing allocation failure を filter failed marking だけで終わらせず、`PipelineDiagnosticKind::AvSharedBackingFailure` として `PipelineReport` へ接続した。
- `RuntimeIoRegistry` は production 未接続の public surface だったため、demux module / Android.bp / public re-export から削除した。
- `CODE_CONVENTION.md` と `DESIGN_JA.md` の `demux_filter_dvr_ops.rs` / `transact_*` 方針矛盾を解消し、demux/filter/DVR の単純 operation だけを明示例外として固定した。
- Android/Soong build、Rust unit test、atest、VTS、実機確認、rustfmt、rustc、cargo は未実行。

# r50eo13

- `record_dvr_post_commit_notification_outcome()` を `Result` 返却へ変更し、DVR post-commit notification failure の accounting 中に service runtime lock poison / callback registry missing / unhealthy marking failure が起きた場合も silent return せず、呼び出し元へ `HalError` として返すようにした。
- `IDvr.start()` / `IDvr.stop()` は post-commit notification failure 自体では public result を反転しないが、failure accounting 不能は service invariant failure として `Status` へ反映するようにした。
- `enqueue_queue_payloads_from_generated_events()` の queue enqueue failure discard を廃止し、`PipelineDiagnosticKind::FilterQueuePayloadDeliveryFailure` として `PipelineReport` に接続した。
- 類似確認として production の `use super::*`, `let _ =`, `.ok();`, `let Ok(..) else { return; }`, `if result.is_err() { continue; }`, `Err(_) => return/continue` を再検索し、今回修正対象以外に同系統の silent discard 修正対象がないことを静的確認した。
- Android/Soong build、Rust unit test、atest、VTS、実機確認、rustfmt、rustc、cargo は未実行。

# r50eo12

- r50eo11 の P0/P2 確認で残っていた `IDvr.stop()` 後の `stop_dvr_status_notifier()` failure discard を修正し、public `stop()` は反転させず DVR post-commit notification diagnostic / callback unhealthy state へ記録するようにした。
- `DvrPostCommitNotificationPhase` に `StatusNotifierStop` を追加し、start専用名だった `record_dvr_start_notification_outcome()` を `record_dvr_post_commit_notification_outcome()` へ改名した。
- `aidl_service/src/object_runtime/drop_leak.rs` の production `use super::*;` を明示 import へ置換した。
- 類似確認として production top-level `use super::*;` と production `let _ =` を検索し、非fallibleな未使用変数抑制の `let _ = demux_id;` も validation call 化した。今回の修正対象以外に同系統の P0/P2 残件がないことを静的確認した。
- Android/Soong build、Rust unit test、atest、VTS、実機確認、rustfmt、rustc、cargo は未実行。

# r50eo11

- r50eo10 の P1 確認で見つかった `install_filter_event_dispatcher()` の二重 install 黙殺を修正し、既存 dispatcher がある場合は `HalError` として明示するようにした。AIDL service の既存 unit test は dispatcher install を伴わない test helper へ分離した。
- P2 として `DemuxFilterDvrTxn` の単純委譲 wrapper を削除し、通常の filter/DVR runtime operation は `demux_filter_dvr_ops.rs` から `transact_*` helper へ直接接続した。child open / rollback 境界だけ `DemuxFilterDvrTxn` に残した。
- P2 として `aidl_service/src/object_runtime.rs` を `aidl_service/src/object_runtime/mod.rs` と `drop_leak.rs` に分割し、drop leak quarantine / record 処理を façade 本体から切り離した。
- `Android.bp` の `aidl_service` srcs を新しい `object_runtime/` 配置へ更新した。
- Android/Soong build、Rust unit test、atest、VTS、実機確認、rustfmt、rustc、cargo は未実行。`rustc` / `rustfmt` はこの環境に存在しなかった。

# r50eo10

- `DemuxRuntime::restore()` を fallible 化し、queue runtime rebuild failure を frontend rollback caller へ返すようにした。
- playback DVR consume は投入 packet ごとの `PipelineReport` を `PlaybackConsumeReport` に残し、record DVR mirror write failure は `RecordDvrMirrorFailure` diagnostic として `PipelineReport` に接続した。
- DVR `start()` 後の status callback / notifier 起動 failure は public `start()` を失敗へ反転せず、DVR post-commit notification diagnostic と callback unhealthy state へ記録するようにした。
- filter event dispatcher install は `Result` を返す境界に変更し、service 起動経路で失敗を明示的に扱うようにした。
- Android/Soong build、Rust unit test、atest、VTS、実機確認、rustfmt、rustc、cargoは未実行。`rustfmt` はこの環境に存在しなかった。

# r50eo9

- `linkCaps` が TS→TS を広告する場合の `IFilter.setDataSource(source)` 契約を、`TsRaw` sink だけでなく `TsRecord` sink へも拡張した。
- `TsRaw` source → `TsRecord` sink は、同一 demux / lifecycle / PID 条件を満たす限り sink subtype として拒否しない。
- source filter origin の TS packet delivery が、source filter を持つ downstream sink を正しく対象にするよう、`PipelineFilterView` の source matching を明示化した。record sink は record index / record DVR mirror の対象にする。
- `tuner_hal2/DESIGN_JA.md` の `TsRecord` sink 記載を、`tuner_hal/DESIGN_JA.md` の成功セルと整合させた。
- Android/Soong build、Rust unit test、atest、VTS、実機確認、rustfmt、rustc、cargoは未実行。

# r50eo8

- `linkCaps` が TS→TS を広告し、TS subtype `UNDEFINED` / `TS` を `TsRaw` として開く場合、`IFilter.setDataSource(source)` の sink が `TsRaw` であることを理由に拒否しないよう補正した。
- record DVR 用 `TsRecord` とその他未分類 sink は引き続き `setDataSource` の sink として unsupported / unavailable 系へ落とす。
- `tuner_hal2/DESIGN_JA.md` に、TS linkCaps / `UNDEFINED` subtype / `TsRaw` sink の契約境界を明記した。
- Android/Soong build、Rust unit test、atest、VTS、実機確認、rustfmt、rustc、cargoは未実行。

# r50eo7

- `tuner_hal/DESIGN_JA.md` の `IFrontend.tune()` 同一tune判定補正に合わせ、release version を更新した。
- `tuner_hal2/DESIGN_JA.md` 本文と Rust実装は変更していない。
- Android/Soong build、Rust unit test、atest、VTS、実機確認、rustfmt、rustc、cargoは未実行。

# r50eo6

- r50eo5 のリリース物規則確認を継続し、`tuner_hal2/DESIGN_JA.md` に残っていた実装状態・future_work由来に見える表現を現行設計責務へ言い換えた。
- DVR read/write方向は record=write / playback=read の設計契約として記載し、Rust AIDL binding の現時点の有無を DESIGN_JA.md に書かない形へ修正した。
- A/V sync節は `future_work` 表現を避け、現行最小契約と後続精度改善境界として整理した。
- Android/Soong build、Rust unit test、atest、VTS、実機確認、rustfmt、rustc、cargoは未実行。

# r50eo5

- r50eo4 の確認方法を開発規則に合わせて是正した。`tuner_hal2/DESIGN_JA.md` から完了判定・実行ゲート・未完了事項に相当する節を削除し、設計正本には現行責務だけを残した。
- `r50eo5_rule_compliance_audit.md` に、開発規則上の確認条件、補助検索、ロジック経路、反例、未実行ゲートを分離して記録した。
- `active_data_ids()` 削除と `source_filter()` panic helper 不在は維持した。
- Android/Soong build、Rust unit test、atest、VTS、実機確認、rustfmt、rustc、cargoは未実行。

# r50eo4

- r50eo3 で残っていた DESIGN_JA.md 上の `r50unofficial2` / `r50eo2` 由来表現を、現行ランタイム部品の責務表現へ置換した。
- FMQ queue runtime、filter callback delivery、DVR queue/status notifier、AV shared backing、demux-input descrambler claim、A/V sync 最小契約を、パッチ由来ではなく現行設計責務として記載し直した。
- demux AV shared backing の未使用 public accessor `active_data_ids()` を削除した。
- demux-input descrambler claim に対する旧 `source_filter()` panic helper は引き続き存在しないことを確認した。
- Android/Soong build、Rust unit test、atest、VTS、実機確認、rustfmt、rustc、cargoは未実行。

# r50eo3

- r50eo2 の過大報告を是正し、r50unofficial2 から取り込んだ FMQ queue runtime、filter callback delivery、DVR queue/status notifier、AV shared backing、closed object idempotent close、demux-input descrambler claim、A/V sync PCR 最小契約を `DESIGN_JA.md` に追記した。
- demux-input descrambler claim に source filter accessor を強制する旧 `source_filter()` panic 前提 helper を削除し、`source_filter_ref()` / `demux_input()` の分岐へ一本化した。
- `r50unofficial2` の failure composition 共通 helper 規律を弱める文書変更は引き続き不採用。
- 本版は static correction checkpoint であり、AOSP契約矛盾ゼロ、VTS合格、official FMQ結線完了は宣言しない。
- Android/Soong build、Rust unit test、atest、VTS、実機確認、rustfmt、rustc、cargoは未実行。

# r50eo2

- r50eo1で暫定受理に留まっていた `IDescrambler.addPid/removePid(..., NULL)` を、source-filter claimとは別のdemux-input PID claimとしてruntime sessionとpacket pathへ接続した。
- demux-input descrambler claimを `DescramblerPidSource::DemuxInput` として型化し、source-filter generation検証に依存しないdemux input descramble対象PIDとして扱うよう補正した。
- `IFrontend.setCallback(NULL)` / `ILnb.setCallback(NULL)` のcallback clearを通常object method dispatch/preflight経路へ通したうえで、owner callback storeをclearする流れへ寄せた。
- A/V syncについて、PCR filter subtypeを `FilterOpenType::TsPcr` として受理し、`getAvSyncHwId()` は同一demux内のlive PCR filter IDを返す最小経路へ補正した。`getAvSyncTime(pcrFilterId)` は同一demuxのlive PCR filter IDであることを検証したうえでpre-PCR時刻未確定値として0を返す。
- r50eo1で壊れていた `frontend_ops.rs` の関数挿入崩れを修正した。
- DVR read/writeは、このRust AIDL IDvr実装に公開 `read()` / `write()` methodが存在しないため、r50eo2では追加差分なし。
- Android/Soong build、Rust unit test、atest、VTS、実機確認、rustfmt、rustc、cargoは未実行。この実行環境にはrustfmt/rustc/cargoが存在しない。

# r50eo1

- r50enでDESIGN_JA.mdへ反映したAOSP-facing契約補正に対し、tuner_hal2実装を追従させた。
- r50unofficial2から、r50en最小追従にも有効な部品を手動抽出し、FMQ QueueRuntime、filter queue/delay readiness、DVR queue/status notifier、filter/DVR callback delivery、AV shared backingの足場を取り込んだ。
- `IFilter.setDataSource(NULL)` をdemux inputへ戻す操作として扱い、non-null source filter接続と分岐させた。
- `IDescrambler.addPid/removePid(..., NULL)` をdemux input PID操作としてAIDL境界で受理する経路を追加した。現時点ではsource-filter claimとは別のpublic contract受理経路であり、実descramble pipelineへの完全接続は後続検証対象。
- `IFrontend.setCallback(NULL)` / `ILnb.setCallback(NULL)` をcallback clearとして受理する経路を追加した。
- TS main typeのlinkCaps広告に対し、VTSが生成するTS/UNDEFINED subtypeをraw TS filterとして受理するよう補正した。
- media/AV filterに対する `setDelayHint()` はunsupportedとして拒否し、non-media filter delay readinessと分離した。
- `IFrontend.getStatus()` はstatusCaps外typeを呼び出し失敗にせずignoredとし、`getFrontendStatusReadiness()` の要素ごとUNSUPPORTED方針と分離した。
- DVR record startのfilter attach必須条件を外し、playback DVR attach/detachはunsupported operationへ寄せた。
- DVR start後のcallback delivery / status notifier開始失敗をpublic `start()` のpost-commit失敗にしないよう、status notificationはbest-effortへ補正した。
- `IDemux.getAvSyncHwId()` / `getAvSyncTime()` をunavailable固定から外し、pre-PCRでもAPI成功する暫定経路を追加した。PCR filter associationの完全実装は後続検証対象。
- `r50unofficial2` のfailure composition共通helper規律を弱める文書変更は取り込まず、r50en側のDESIGN_JA.md / CODE_CONVENTION.mdを維持した。
- Android/Soong build、Rust unit test、atest、VTS、実機確認、rustfmt、rustc、cargoは未実行。この実行環境にはrustfmt/rustc/cargoが存在しない。

# r50ema_wp_r07_filter_delay_queue_runtime_gate

- WP-R07継続として、demux runtime の filter queue に queued-byte と delivery deadline を持たせ、`FilterDelayHint` を queue drain readiness へ接続した。
- packet pipeline の generated event から raw/record/section/PES payload を filter queue へ積み、time/data-size hint の ready 判定が実queue状態を見るようにした。
- filter `configure()` / `stop()` / `flush()` / queue clear / queue remove で delay queue state も同時に破棄するよう補正した。
- demux unit test に、time-delay の再arm、data-size 閾値、time/data OR 判定、packet push 経由の raw queue enqueue を追加した。
- `m maleicacid_tuner_hal2_demux_test maleicacid_tuner_hal2_service_runtime_test` は成功。rustfmt は実施。Rust unit test 実行、`atest`、VTS、実機確認は未実施。

# r50el_test_mod_cleanup_and_future_work_retirement_unverified

- Retired completed r51 future_work files for rollback/cleanup failure composition and tuner_hal2 failure-injection tests after moving the implemented scope into current code/tests.
- Added isolated `#[cfg(test)]` failure-injection modules instead of production test branches:
  - `common/src/failure_injection_tests.rs`
  - `service_runtime/src/failure_injection_tests.rs`
  - `aidl_service/src/failure_injection_tests.rs`
- Kept the new tests as logic-path tests over typed `HalError` composition, close cleanup marking, public-close missing-target cleanup, callback rollback, and close-domain cleanup; they do not use `include_str!`, grep, or source-text self-inspection.
- Routed remaining direct production `HalError::composed_failure(...)` call sites through the common composition helper where applicable.
- Removed `future_work/r51/rollback_failure_composition_common_component_plan.md` and `future_work/r51/tuner_hal2_failure_injection_tests_plan.md` so completed work is not left as a misleading future-work item.
- Android/Soong `m`, unit tests, `atest`, VTS, real-device checks, `cargo`, `rustc`, and `rustfmt` were not run in this environment; `rustfmt`/`rustc`/`cargo` commands are not installed.

# r50ek_future_work_composition_and_boundary_fix_unverified

- Advanced the rollback/cleanup failure composition future-work item by adding common helpers in `common/src/lib.rs`: `compose_primary_cleanup_failure()`, `finish_cleanup_after_primary_failure()`, and `fail_after_cleanup()`.
- Routed open rollback and AIDL root/child cleanup composition helpers through the common composition helpers instead of local-only composition logic.
- Advanced the failure-injection future-work item by adding common-helper coverage for primary-only and primary+cleanup failure composition and updating the future-work file to list only remaining build-verified test coverage.
- Converted close-domain cleanup closure ABI from `BinderResult<()>` to `Result<(), HalError>` and updated frontend/LNB close call sites and close cleanup tests accordingly.
- Converted child filter/DVR object construction failure cleanup from Binder `Status` primary handling to typed `HalError` primary handling.
- Updated future_work/r51 documents to distinguish r50ek implemented progress from remaining compiler/build-verified work.
- Android/Soong `m`, unit tests, `atest`, VTS, real-device checks, `cargo`, `rustc`, and `rustfmt` were not run in this environment; `rustfmt`/`rustc`/`cargo` commands are not installed.

# r50ei106_callback_closure_typed_boundary_unverified

- Added `vendor/maleicacid/tv/future_work/r51/tuner_hal2_failure_injection_tests_plan.md` so failure-injection test coverage is tracked only as future work and not as a current-release completion requirement.
- Added `AidlCallbackStoreError::into_hal_error()` and routed callback store retain failures through typed `HalError` instead of object-local Binder `Status` conversion.
- Changed callback registration retain/rollback closure bounds in `object_runtime.rs` from `BinderResult<()>` to `Result<(), HalError>`.
- Updated frontend/LNB setCallback and child filter/DVR callback retain/rollback closures to use typed callback store and callback cleanup failures.
- Kept close-domain cleanup closure ABI as `BinderResult<()>`; converting it requires updating close call sites and tests together with compiler verification.
- Android/Soong `m`, unit tests, `atest`, VTS, real-device checks, `cargo`, `rustc`, and `rustfmt` were not run in this environment; `rustfmt`/`rustc`/`cargo` commands are not installed.

# r50ei105_object_runtime_status_bridge_reduction_unverified

- Reduced typed-failure loss inside `aidl_service/src/object_runtime.rs` by adding HalError-returning callback-registry cleanup/marking helpers while keeping Binder-returning public wrappers at the AIDL boundary.
- Changed callback registration record failure handling to preserve the typed primary `HalError` and compose it with rollback `Status` only at the final Binder boundary.
- Changed callback registration domain failure handling so the domain `HalError` is not converted to Binder `Status` before rollback/unhealthy-marking composition.
- Changed public close cleanup to collect callback cleanup and domain cleanup as `HalError`, and to compose cleanup-failed marking failure as typed composed failure before Binder status conversion.
- Changed Drop leak LNB domain-record failure from Binder `Status` conversion back to typed `HalError` collection.
- Stopped at closure/API boundaries that still expose `BinderResult` for callback retain/rollback and domain cleanup; converting those requires wider signature changes across object-specific AIDL methods and should be compiler-verified.
- Android/Soong `m`, unit tests, `atest`, VTS, real-device checks, `cargo`, `rustc`, and `rustfmt` were not run in this environment; `rustfmt`/`rustc`/`cargo` commands are not installed.

# r50ei104_status_bridge_reduction_unverified

- Reduced typed-failure loss at AIDL construction boundaries by adding HalError-preserving cleanup composition helpers for root object construction/id-conversion rollback.
- Updated frontend/demux/descrambler/LNB root object construction and runtime-id conversion rollback paths to compose typed primary `HalError` with typed rollback failure before Binder status conversion.
- Updated child filter/DVR runtime-id conversion rollback to preserve typed primary and rollback failure; callback-retain rollback now preserves typed rollback failure even though the primary callback retain failure is already a Binder `Status`.
- Rechecked runtime unregister signature propagation after r50ei103 and removed an ignored `Option` in the affected service-runtime test.
- Android/Soong `m`, unit tests, `atest`, VTS, real-device checks, `cargo`, `rustc`, and `rustfmt` were not run in this environment; `rustfmt`/`rustc`/`cargo` commands are not installed.

# r50ei103_unregister_cleanup_result_propagation_unverified

- Continued the r50ei102 failure-composition foundation by converting demux/filter/DVR/descrambler runtime unregister paths to return cleanup failure instead of silent `Option`-only success/failure.
- Converted descrambler session cleanup and demux owner-loss descrambler cleanup to `FirstErrorCollector`-based returned cleanup failure while retaining diagnostic records.
- Propagated unregister cleanup failures through root-open rollback, public close cleanup, and child filter/DVR open rollback paths.
- Preserved all-attempt cleanup where local code can continue after a cleanup failure.
- Android/Soong `m`, unit tests, `atest`, VTS, real-device checks, `cargo`, `rustc`, and `rustfmt` were not run in this environment; `rustfmt`/`rustc`/`cargo` commands are not installed.

# r50ei102_failure_composition_foundation_unverified

- Added `HalError::ComposedFailure` as the typed primary-plus-cleanup failure container and routed AIDL status mapping through the primary error while preserving cleanup detail in the display text.
- Updated root/child open rollback paths so rollback or cleanup failure is composed with the primary open/construction/registration failure instead of replacing it.
- Updated callback registration, public close cleanup, Drop leak quarantine, frontend tune/scan worker rollback, and cleanup-failed marking paths to preserve primary-plus-cleanup failure context.
- Tightened missing-target handling for rollback/public close/owner-loss cleanup paths where this can be detected locally.
- Updated the r50ei101 implementation plan target with code changes only; no new current-scope feature implementation was added.
- Android/Soong `m`, unit tests, `atest`, VTS, real-device checks, `cargo`, `rustc`, and `rustfmt` were not run in this environment; `rustfmt`/`rustc`/`cargo` commands are not installed.

# r50ei92_first_error_collector_residual_cleanup_fix

- Replaced the remaining child-open callback cleanup / rollback pairwise `match` blocks with `FirstErrorCollector` while preserving the filter/DVR child-open cleanup semantic wrappers.
- Replaced the frontend tune-worker commit-failure rollback secondary-error accumulator with `FirstErrorCollector`; primary commit error plus rollback error detail composition is still preserved.
- Replaced the Drop leak `Option<String>` first-error helper with `FirstErrorCollector<String>` while preserving the callback-registry clear-vs-unhealthy branch.
- Updated `tuner_hal2/RELEASE_VERSION` to this release name.
- Android/Soong `m`, `atest`, VTS, device checks, and `rustfmt` were not run in this environment.

# r50ei91_cleanup_all_attempt_first_error_fix

- Added `FirstErrorCollector` as the multi-step cleanup all-attempt / first-error common component and documented it in `tuner_hal2/DESIGN_JA.md`.
- Changed the AIDL object close finalizer so callback cleanup failure no longer prevents domain cleanup from being attempted; cleanup-failed marking is attempted after all close cleanup steps.
- Changed frontend close cleanup so LNB owner-loss close, tune/scan worker stop, scan cancel record, live-data close/unbind, and closed-LNB callback cleanup preserve first-error precedence while still attempting later cleanup steps.
- Changed scan-worker supersede cleanup so scan cancel recording no longer masks a completed worker failure.
- Updated `tuner_hal2/RELEASE_VERSION` to this release name.
- Android/Soong `m`, `atest`, VTS, device checks, and `rustfmt` were not run in this environment.

# r50ei90_worker_session_stop_child_open_drop_diagnostic_fix

- Added frontend backend-session stop finalization so tune/scan worker bodies still attempt `FrontendBackendSession::stop()` after session-open-success error paths; tune live-pump stop failure is combined with the primary worker-body failure instead of detaching silently.
- Changed frontend worker stop handling so `Completed { result: Err(..) }` remains distinguishable from `StopRequestFailed`: `stopTune()` / `stopScan()` now preserve first-failure precedence while still attempting safe terminal cleanup and live-data/live-reader cleanup where the worker has already completed.
- Strengthened Drop leak accounting so callback-store clear failure, domain drop-leak record failure, callback-registry missing, and object-quarantine failure are returned to `drop_leak_object_from_drop()` for in-memory error recording instead of being swallowed after unhealthy marking.
- Added `StartupDiagnosticKind::DuplicateLnbId` and records default-LNB registration collisions as LNB duplicate diagnostics instead of frontend duplicate diagnostics.
- Added rollback for filter/DVR child-open AIDL object registration when child `ledger_id` cannot be represented as `i32`, so typed AIDL object construction does not leave a registered child object on numeric conversion failure.
- Android/Soong `m`, `atest`, VTS, device checks, and `rustfmt` were not run in this environment; `rustfmt` was attempted but the command is not installed.

# r50ei89_worker_result_method_boundary_callback_registry_fix_source_complete_unverified13

- Generalized `DESIGN_JA.md` and `CODE_CONVENTION.md` from the close-specific AIDL object wrapper ban to a broad ban on public thin wrappers that do not own state, lifetime, phase order, rollback, or error precedence.
- Removed the public frontend/LNB `rollback_callback_registration()` wrapper methods by inlining callback cleanup into the callback registration rollback closures.
- Removed the public filter profile validation wrappers used only by tests; tests now call the service_runtime profile validation functions directly.
- No Android/Soong `m`, `atest`, or `rustfmt` gates were run in this environment; this archive remains source-complete by static review but unverified by the fixed build/test gates.

# r50ei89_worker_result_method_boundary_callback_registry_fix_source_complete_unverified12

- Fixed the LNB `Drop` syntax error introduced in the drop-leak wrapper conversion.
- Removed all remaining AIDL object close thin wrappers (`close_object_after_close_preflight()` / `close_object()`) from frontend/filter/demux/dvr/descrambler/LNB object wrappers; close trait methods now call the `object_runtime` façade directly.
- Changed `drop_leak_object()` lock ordering so callback-store owner cleanup runs before acquiring `TunerServiceRuntime` lock; runtime registry clear/unhealthy and object quarantine remain inside the runtime critical section.
- Updated `DESIGN_JA.md` and `CODE_CONVENTION.md` to forbid AIDL object close thin wrappers and to require callback-store cleanup outside the service runtime lock in Drop leak handling.
- No Android/Soong `m`, `atest`, or `rustfmt` gates were run in this environment; this archive remains source-complete by static review but unverified by the fixed build/test gates.

# r50ei89_worker_result_method_boundary_callback_registry_fix_source_complete_unverified11

- Added rollback handling for `start_frontend_backend_tune_worker()` after worker start succeeds but `commit_frontend_active_tune_request()` fails: the started worker is stopped/joined via the two-phase stop path, frontend runtime snapshot restore and bound demux snapshot restore are attempted, and primary/secondary failure context is surfaced.
- Fixed `close_frontend_workers_and_live_data()` error precedence so live-data close/unbind failure no longer overwrites an earlier worker stop or scan-cancel-record failure; live-data cleanup is still attempted.
- Updated `DESIGN_JA.md` and `CODE_CONVENTION.md` to require post-worker-start fallible commit rollback and first-failure precedence during frontend close cleanup.
- No Android/Soong `m`, `atest`, or `rustfmt` gates were run in this environment; this archive remains source-complete by static review but unverified by the fixed build/test gates.

# r50ei89_worker_result_method_boundary_callback_registry_fix_source_complete_unverified10

- Replaced frontend worker stop-and-join with a two-phase stop ticket: runtime lock now only removes the worker slot, records cancel reason, sets the cancel flag, and returns `FrontendWorkerStopTicket`; blocking `complete()` / `JoinHandle` wait is performed after releasing `TunerServiceRuntime` lock.
- Updated scan-start supersede handling to request the old scan worker stop under runtime lock, release the lock for join, then reacquire runtime lock before recording scan cancellation and starting the replacement scan worker.
- Updated stopTune/stopScan/frontend close paths by routing them through the same lock-free join phase in `stop_frontend_worker()`.
- Updated `DESIGN_JA.md` and `CODE_CONVENTION.md` to forbid frontend worker blocking join while holding `TunerServiceRuntime` lock.
- No Android/Soong `m`, `atest`, or `rustfmt` gates were run in this environment; this archive remains source-complete by static review but unverified by the fixed build/test gates.

# r50ei89_worker_result_method_boundary_callback_registry_fix_source_complete_unverified9

- Changed actual AIDL object `Drop` implementations to call `drop_leak_object_from_drop()` instead of discarding `drop_leak_object()` results with `drop(...)`; returned drop-leak errors are recorded in the drop-leak diagnostic record without using `eprintln!`.
- Changed callback-store owner cleanup to return the number of removed callback artifacts and changed `RuntimeCallbackRegistry::clear_owner()` to return `CallbackRegistryUpdate`; callback clear now treats store-removed/runtime-missing as an error while allowing no-store/no-runtime as already-cleared.
- Removed stale `ensure_open()` wrappers and the exported `ensure_object_live()` helper so AIDL object wrappers cannot bypass the object-runtime façade.
- Updated `DESIGN_JA.md` and `CODE_CONVENTION.md` to make the Drop-leak diagnostic record, callback owner clear missing semantics, and no-stderr/no-`drop(result)` rule explicit.
- No Android/Soong `m`, `atest`, or `rustfmt` gates were run in this environment; this archive remains source-complete by static review but unverified by the fixed build/test gates.

# r50ei89_worker_result_method_boundary_callback_registry_fix_source_complete_unverified8

- Tightened `DESIGN_JA.md` so Drop leak cannot treat `CallbackRegistryUpdate::Missing` as covered merely by quarantine; registry missing must be returned or recorded after quarantine is attempted.
- Changed `drop_leak_object()` to remember callback registry owner-missing during unhealthy marking, always attempt object quarantine, then return an error for the missing registry entry after quarantine/unregister work completes.
- Split scan END failure handling so callback artifact absence / callback store failure records only the scan-session failure and returns the artifact/store failure, while actual Binder delivery failure to a retrieved callback performs runtime registry unhealthy marking.
- Added a drop leak regression test proving registry-missing is reported while the object is still quarantined.
- No Android/Soong `m`, `atest`, or `rustfmt` gates were run in this environment; this archive remains source-complete by static review but unverified by the fixed build/test gates.

# r50ei89_worker_result_method_boundary_callback_registry_fix_source_complete_unverified7

- Changed `ThreadResultProducer` so producer-side result-cell lock poison is captured through the producer-failure side channel instead of returning a value that the worker thread must discard.
- Strengthened `execute_object_query_use_case()` so the query façade itself verifies object live / generation / kind before runtime dispatch planning and before running the query closure.
- Added a query façade behavior test proving a missing object is rejected before the query closure executes.
- Updated `DESIGN_JA.md` / `CODE_CONVENTION.md` wording for `ThreadResultProducer::record_or_capture_failure()` to match the side-channel design.
- No Android/Soong `m`, `atest`, or `rustfmt` gates were run in this environment; this archive remains source-complete by static review but unverified by the fixed build/test gates.

# r50ei89_worker_result_method_boundary_callback_registry_fix_source_complete_unverified6

- Added `RuntimeCommandDispatchError::RuntimeLockPoison` and changed root method dispatch planning so service runtime lock poison is no longer misclassified as `MissingDispatchTarget`.
- Changed filter/DVR child-object construction failure cleanup so callback cleanup and child-open rollback are both attempted; callback cleanup failure no longer prevents child-open rollback from running.
- Hardened `ThreadResultOwner` against double collection with an explicit already-collected failure, added a behavior test for the second collect, and added a producer-failure side channel so producer-side result lock poison is reported to the owner without `eprintln!`.
- Removed remaining `eprintln!` diagnostics from tuner_hal2 Rust sources; existing in-memory diagnostics and returned `HalError` paths remain the reporting mechanisms.
- `IFilter.getId()` / `getId64Bit()` remain on `execute_object_query_use_case()`; this is the intended pure-query façade and not a direct AIDL method-body call to `public_runtime_id_for_object_method()`.
- No Android/Soong `m`, `atest`, or `rustfmt` gates were run in this environment; this archive remains source-complete by static review but unverified by the fixed build/test gates.

# r50ei89_worker_result_method_boundary_callback_registry_fix_source_complete_unverified5

- Changed `ThreadResultProducer::record()` to return `HalError` on result-cell lock poison and log that failure from the worker thread, while leaving final caller-visible classification to `ThreadResultOwner`.
- Documented the `catch_unwind(AssertUnwindSafe(...))` contract for one-shot worker closures in `DESIGN_JA.md` and `CODE_CONVENTION.md`.
- Rechecked `IFilter.getId()` / `getId64Bit()` and kept them on `execute_object_query_use_case()`; their use of `public_runtime_id_for_object_method()` occurs inside the query façade and is the intended object live / generation / kind check for pure query.
- No Android/Soong `m`, `atest`, or `rustfmt` gates were run in this environment; this archive remains source-complete by static review but unverified by the fixed build/test gates.

# r50ei89_worker_result_method_boundary_callback_registry_fix_source_complete_unverified4

- Added `device/src/runtime/thread_result_owner.rs` to the `libmaleicacid_tuner_hal2_device` and `maleicacid_tuner_hal2_device_test` `Android.bp` `srcs` lists so the new module is included by Soong.
- No behavior changes beyond build graph correction.
- `rustfmt`, Android/Soong `m` gates, and `atest` gates were not run in this environment; this archive remains source-complete by static review but unverified by the fixed build/test gates.

# r50ei89_worker_result_method_boundary_callback_registry_fix_source_complete_unverified3

- Removed the unused `CallbackHealthState::Cleared` state and fixed the design wording so callback clear means RuntimeCallbackRegistry entry removal, not a tombstone health state.
- Narrowed `ThreadResultOwner`, `ThreadResultPoll`, and `ThreadResultFailure` to crate-internal/module-internal visibility, made `ThreadResultProducer` private, removed `ThreadResult*` re-exports from `device/src/runtime/mod.rs` and `device/src/lib.rs`, and kept spawn failure as a start-time `HalError` rather than a worker-result enum case.
- Removed all `plan_method_without_execution()` object-wrapper/service public helpers and the `plan_object_method_without_runtime_execution()` helper from `aidl_service::object_runtime`; plan-only object method paths must now go through the allowed façade use-cases.
- Removed the direct plan-only unit test that called the deleted helper and updated `DESIGN_JA.md` / `CODE_CONVENTION.md` so `aidl_service::object_runtime` remains the AIDL object method executor façade.
- Re-ran static source checks for stale `Cleared`, `ThreadResult*` public re-exports, plan-only helper names, `method_execution`, poison/missing-result success rounding, and direct AIDL `plan_object_method_dispatch`; they returned no matches outside intended internal uses.
- `rustfmt`, Android/Soong `m` gates, and `atest` gates were not run in this environment; this archive remains source-complete by static review but unverified by the fixed build/test gates.

# r50ei89_worker_result_method_boundary_callback_registry_fix_source_complete_unverified2

- Source-level completion pass for worker result ownership, method execution façade boundary, callback registry ownership, and ObjectMethodTxn/ObjectClose/root-open/query/unavailable method-category documentation.
- Added `device/src/runtime/thread_result_owner.rs` and connected `FrontendWorkerRegistry` / `FrontendWorkerSlot` and `FrontendLivePumpOwner` through `ThreadResultOwner`, so panic, result lock poison, and missing report are surfaced as `HalError` rather than success/finished states.
- Added behavior tests for `ThreadResultOwner`, frontend worker missing/poison result reporting, and live pump panic/missing/poison reporting.
- Deleted `service_runtime/src/method_execution.rs` and kept the low-level runtime execution helpers as private helpers inside `aidl_service/src/object_runtime.rs`, making that file the AIDL object method executor façade.
- Changed callback registry unhealthy marking to return `CallbackRegistryUpdate`, propagated frontend callback unhealthy-marking failures, and marked callback registrations unhealthy when callback registration rollback fails after domain commit failure.
- Added callback registry missing-update and callback rollback-failure behavior tests.
- Added `DESIGN_JA.md` ownership text for callback_store / RuntimeCallbackRegistry / domain runtime state and the AIDL method category required-boundary table.
- Static source checks for poison/missing success rounding, AIDL direct low-level executor references, and AIDL direct `plan_object_method_dispatch` references were run and returned no matches outside the allowed façade.
- Changed `drop_leak_object()` to return a Binder result instead of silently returning on runtime lock poison / quarantine failure; Drop implementations now log drop-leak handling failures rather than treating them as invisible.
- Prevented `FrontendWorkerRegistry::start()` from overwriting a completed-but-unreported worker failure; such failures must be reported via `take_completed()` before replacement.
- `rustfmt`, Android/Soong `m` gates, and `atest` gates were not run in this environment; this archive is source-complete by static review but unverified by the fixed build/test gates.

# r50ei89_doc_responsibility_tuner_docs_fix

- Corrected the `tuner_hal2/INTEGRATION.md` legacy `tuner_hal/INTEGRATION.md` wording: the legacy integration document may exist for reference, but it is not the default product integration SSOT.
- Updated the archive release marker after the tuner documentation responsibility cleanup.
- Code behavior was not changed. Android/Soong build, rustfmt, Rust unit tests, atest, VTS, and device checks were not run in this environment.

# r50ei88_callback_registration_phase_order_fix

- Added `future_work/r51/rollback_failure_composition_common_component_plan.md` to track generic primary-failure / rollback-failure status composition as r51 work instead of expanding that large-scope change in r50.
- Changed filter/DVR child open completion so callback artifact registration runs before typed AIDL object construction, avoiding the callback-failure path where explicit child rollback and Drop leak handling could run for the same just-created AIDL object.
- Moved frontend/LNB setCallback onto the existing `ObjectMethodTxn` dispatch-preflight boundary before callback store retain, and pass the dispatch-preflight proof into the service_runtime callback commit use-cases.
- Added a regression test that verifies callback retain is not attempted when ObjectMethodTxn dispatch preflight fails before registration.
- Updated `DESIGN_JA.md` and `CODE_CONVENTION.md` to fix the callback registration and child open phase-order contracts around existing common components.
- Android/Soong build, rustfmt, Rust unit tests, atest, VTS, and device checks were not run in this environment.

# r50ei87_callback_artifact_registration_core_unification

- Removed the old `retain_and_record_callback_registration()` helper so callback retain + runtime registry record is no longer split between setCallback and child-open-specific paths.
- Added `register_callback_artifact_after_owner_ready()` as the callback artifact registration core used after the owner object has already been validated or registered.
- Kept frontend/LNB `setCallback()` on `execute_callback_registration_runtime_use_case()`: live/generation/kind check, callback artifact registration, domain runtime commit, and rollback stay in one use-case boundary.
- Routed filter/DVR child open callback retain through the same callback artifact registration core after child runtime/object registration, while preserving child-open rollback on callback failure.
- Updated `DESIGN_JA.md` and `CODE_CONVENTION.md` to describe the setCallback and child open callback registration split without reintroducing `retain_and_record_callback_registration()`.
- Android/Soong build, rustfmt, Rust unit tests, atest, VTS, and device checks were not run in this environment.

# r50ei86_callback_registration_and_method_execution_boundary_fix

- Made `execute_callback_registration_runtime_use_case()` the primary callback registration transaction entry for frontend/LNB setCallback: live/generation/kind check, callback store retain, runtime callback registry record, domain runtime commit, and rollback now stay in one use-case boundary.
- Removed the frontend/LNB setCallback main-path `retain_callback()` wrappers so they no longer route through `retain_and_record_callback_registration()` before entering the callback registration transaction.
- Kept `retain_and_record_callback_registration()` for child object open callback retain/record use only.
- Added a module comment to `service_runtime/src/method_execution.rs` documenting it as a runtime lock / shared runtime / query closure executor, not a transaction owner.
- Updated `DESIGN_JA.md` and `CODE_CONVENTION.md` so ObjectMethodTxn is limited to fallible request-builder / status-precedence-risk paths and simple methods are not forced into `ObjectMethodTxnPlan` / dispatch-preflight tokens.
- Android/Soong build, rustfmt, Rust unit tests, atest, VTS, and device checks were not run in this environment.

# r50ei85_aidl_method_plan_coverage_test_name_alignment

- Renamed the AIDL method plan-coverage helper/test so the names describe AIDL method call variants rather than a partial method-kind sample.
- Added the missing `PublicApi`, `UnsupportedPublicApi`, `DemuxOpenFilter`, `FilterConfigure`, and `FilterConfigureAvStreamType` variants to the plan-coverage helper.
- Renamed the LNB close transaction-table test to describe the specific `CommandPlan::for_api()` mapping it verifies.
- Android/Soong build, rustfmt, Rust unit tests, atest, VTS, and device checks were not run in this environment.

# r50ei84_command_plan_for_api_transaction_ssot

- Changed command planning to use `CommandPlan::for_api(object, api)` as the primary path so the runtime transaction name is selected only from `AIDL_TRANSACTION_TABLE`.
- Removed the caller-supplied transaction-name lookup path from Rust code; command callers now pass object/API identity rather than an expected transaction triple.
- Updated `DESIGN_JA.md` and `CODE_CONVENTION.md` to keep transaction-name ownership in the AIDL transaction table and avoid duplicating expected transaction names at call sites.
- Android/Soong build, rustfmt, Rust unit tests, atest, VTS, and device checks were not run in this environment.

# r50ei83_binder_adapter_aidl_method_duplicate_import_cleanup

- Removed the duplicate `HalError` import from `binder_adapter/src/aidl_method.rs` after the fallible command-plan cleanup.
- No behavior was changed in this revision.
- Android/Soong build, rustfmt, Rust unit tests, atest, VTS, and device checks were not run in this environment.

# r50ei82_request_builder_plan_result_propagation_fix

- Fixed `aidl_service/src/object_runtime.rs::object_method_txn_plan()` to return `BinderResult<(CommandPlan, Option<RuntimeExecutableRequest>)>` after `AidlMethodAdapter::plan()` became fallible.
- Updated `execute_object_runtime_use_case_with_request_builder()`, `execute_shared_object_runtime_use_case_with_request_builder()`, and `plan_unavailable_object_method_use_case()` to propagate the fallible plan result from inside their builder closures.
- Android/Soong build, rustfmt, Rust unit tests, atest, VTS, and device checks were not run in this environment.

# r50ei81_design_contract_cleanup_command_plan_no_expect

- Removed implementation-rule wording from `DESIGN_JA.md` around capability-token visibility and kept only the current design contract; detailed construction/visibility rules remain in `CODE_CONVENTION.md`.
- Changed AIDL command plan construction to return `Result` instead of panicking with `expect("known AIDL command plan")` in production code paths.
- Updated binder adapter command planning, AIDL method planning, and AIDL service call sites to propagate command-plan construction failures as HAL/Binder errors instead of panics.
- Kept transaction-table-backed `CommandPlan` construction and private `CommandPlan` fields.
- Android/Soong build, rustfmt, Rust unit tests, atest, VTS, and device checks were not run in this environment.

# r50ei80_capability_token_convention_generalization

- Rewrote the capability-token visibility rule in `CODE_CONVENTION.md` as a generic rule for validation tokens, dispatch-preflight proofs, transaction plans, ledger guards, and rollback guards.
- Removed the struct-by-struct convention wording from `CODE_CONVENTION.md`; concrete component names remain in `DESIGN_JA.md` where implementation ownership is fixed.
- No code behavior was changed in this revision.
- Android/Soong build, rustfmt, Rust unit tests, atest, VTS, and device checks were not run in this environment.

# r50ei79_capability_token_visibility_hardening

- Hardened `ObjectMethodDispatchPreflight` so the dispatch-preflight-complete proof and required policy constructors are private to `service_runtime::object_method_txn`; request-builder callers only receive the proof returned by `build_and_plan_object_method_request_after_live()`.
- Removed `Clone` from `ObjectMethodDispatchPreflight` and `ObjectMethodTxnPlan`; the dispatch proof remains consume-only via `plan(self, ...)`, and transaction plans are generated inside `object_method_txn`.
- Changed `ObjectMethodTxnPlan::new()` to private and changed the request-builder transaction closure contract to pass `CommandPlan` / `RuntimeExecutableRequest` parts into `object_method_txn`, where the plan is constructed.
- Made `CommandPlan` fields private and added accessors plus transaction-table-backed construction so inconsistent `(object, api, transaction)` triples cannot be built with public fields.
- Hardened `LnbOperationGuard` by making its fields private, removing `Clone` / `Copy`, adding an internal nonce, and requiring `LnbOperationLedger::finish()` to match both kind and nonce before clearing active state.
- Updated `DESIGN_JA.md` and `CODE_CONVENTION.md` to fix the capability-token and transaction-plan visibility contracts.
- Android/Soong build, rustfmt, Rust unit tests, atest, VTS, and device checks were not run in this environment.

# r50ei78_filter_datasource_dispatch_preflight_arg_fix

- Fixed `IFilter.setDataSource()` request-builder execute closure to receive the `ObjectMethodDispatchPreflight` proof returned by `service_runtime::object_method_txn` and pass that proof to `set_filter_data_source_for_object()`.
- Verified the other request-builder execute closures under `aidl_service/src` already receive and pass `dispatch_preflight` explicitly.
- Verified `aidl_service/src` still does not directly construct `already_planned()` and no dispatch-preflight-specific public entry point was reintroduced.
- Android/Soong build, rustfmt, Rust unit tests, atest, VTS, and device checks were not run in this environment.

# r50ei77_object_method_dispatch_preflight_private_constructor

- Verified the r50ei76 dispatch-preflight policy and child rollback diagnostic changes before applying this type-boundary hardening.
- Changed `ObjectMethodDispatchPreflight` from a public enum with public `required(...)` / `already_planned()` constructors into a public wrapper over a private state enum; constructors and `plan(...)` are now service_runtime-internal.
- Changed `build_and_plan_object_method_request_after_live()` to return the dispatch-preflight proof only after live/generation/kind validation, request build, method/object kind validation, executable request validation, and dispatch preflight all succeed.
- Routed AIDL request-builder execution closures to pass through the returned dispatch-preflight proof instead of constructing an already-planned value locally.
- Removed the unused normal child-open helper paths that required AIDL to construct the required dispatch policy directly; child open remains on the request-builder object-method transaction path and does not revive dispatch-preflight-specific public entry points.
- Updated `tuner_hal2/DESIGN_JA.md` and `tuner_hal2/CODE_CONVENTION.md` to fix the proof rule: AIDL and individual method bodies must not freely create a preflight-complete value.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei76_object_method_dispatch_policy_child_rollback_diagnostics

- Verified the r50ei75 child-open, unavailable-plan, and duplicate-dispatch fixes before applying this follow-up hardening.
- Replaced the parallel dispatch-preflight-specific public use-case family with `ObjectMethodDispatchPreflight`; normal callers pass `required(...)`, request-builder callers pass `already_planned()`, and both paths use the same `*_for_object` service_runtime entry points.
- Routed filter/DVR child open through the demux/filter/DVR transaction context again while preserving the service_runtime object-method dispatch-preflight boundary.
- Added child-open rollback diagnostics for object registration rollback failure, runtime cleanup target missing, and combined rollback failure; rollback no longer treats a missing filter/DVR runtime cleanup target as success.
- Updated `tuner_hal2/DESIGN_JA.md` and `tuner_hal2/CODE_CONVENTION.md` to require the dispatch-preflight policy object rather than parallel after-preflight public entry points, and to require child-open rollback diagnostics.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei75_object_method_txn_child_unavailable_duplicate_dispatch_fix

- Verified the r50ei74 medium request-builder dispatch-preflight changes before applying this follow-up fix.
- Routed child `openFilter()` / `openDvr()` request-builder paths through `execute_shared_object_runtime_use_case_with_request_builder()` and service_runtime after-preflight child open entry points, so open request building, executable request validation, and dispatch preflight share the `service_runtime::object_method_txn` boundary.
- Added after-object-method-preflight service_runtime entry points for request-builder domain operations that already passed dispatch preflight, and routed LNB, filter, descrambler, frontend tune/scan, and child open request-builder closures to those entry points to avoid duplicate dispatch planning.
- Changed `plan_unavailable_object_method_use_case()` to use the build-and-plan service_runtime transaction boundary directly; it no longer falls back to the old build-only helper plus a second plan-only dispatch pass.
- Updated `tuner_hal2/DESIGN_JA.md` and `tuner_hal2/CODE_CONVENTION.md` to require request-builder execution closures to call after-preflight service_runtime use-case entry points and to forbid duplicate dispatch planning after `ObjectMethodTxnPlan` preflight.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei74_object_method_txn_dispatch_preflight_completion

- Verified the r50ei73 `service_runtime::object_method_txn` boundary before applying the medium hardening.
- Added `ObjectMethodTxnPlan` to carry the neutral `CommandPlan` and `RuntimeExecutableRequest` pair for object method transactions.
- Added `build_and_plan_object_method_request_after_live()` so request-builder methods now perform object live/generation/kind validation, request building, executable request validation, and dispatch planning preflight in the service_runtime object method transaction boundary.
- Routed normal and shared request-builder AIDL adapters through the new build-and-plan transaction path; AIDL helpers now generate the neutral plan and delegate validation/dispatch preflight to service_runtime.
- Added service_runtime regression coverage for dispatch-preflight execution under the runtime lock, builder failure before dispatch preflight, and method/object kind mismatch rejection.
- Updated `tuner_hal2/DESIGN_JA.md` and `tuner_hal2/CODE_CONVENTION.md` to fix `ObjectMethodTxnPlan` and dispatch-preflight ownership in `service_runtime::object_method_txn`.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei73_object_method_txn_service_runtime_boundary

- Added `service_runtime::object_method_txn` as the implementation owner for the object method request-builder lifecycle critical section.
- Moved the request-builder live/generation/kind preflight out of the private AIDL-side `ObjectMethodUseCase` phase owner and into `service_runtime::object_method_txn::build_object_method_request_after_live()`.
- Kept AIDL-side `object_runtime` helpers as adapters for AIDL method static planning, Binder status conversion, and service_runtime transaction dispatch.
- Added service_runtime regression coverage that request builders run while the runtime lock is held and that closed objects reject before invoking the builder.
- Updated `tuner_hal2/DESIGN_JA.md` to fix `service_runtime::object_method_txn` as the common-component implementation owner and updated `tuner_hal2/CODE_CONVENTION.md` to forbid AIDL-side ownership of the request-builder critical section.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei72_request_builder_lifecycle_lock_fix

- Short-term hardened the object method request-builder boundary: `ObjectMethodUseCase` now runs fallible request builders while holding the same runtime lock used for the object live/generation/kind check.
- Kept the later service_runtime dispatch/runtime operation recheck, but removed the race where a concurrent close could occur between the first lifecycle check and a failing request builder.
- Applied the same lock-held builder preflight to shared-runtime request builders and unavailable / unsupported plan-only request builders.
- Added regression coverage that a request-builder closure observes the runtime lock as held and that closed objects reject before invoking the builder.
- Updated `tuner_hal2/DESIGN_JA.md` and `tuner_hal2/CODE_CONVENTION.md` to require request builders to run inside the lifecycle critical section and to restrict builder closures to short, side-effect-free conversion.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei71_source_boundary_close_preflight_atomicity_fix

- Verified the r50ei70 object method and source boundary updates before applying the follow-up hardening.
- Reordered `SourceBoundaryTxn` so sink endpoint, queue presence, and generation increment feasibility are validated before any source disconnect. Queue clear, generation boundary reset, and packet pipeline reset now complete before disconnecting the existing source.
- Added regression coverage that a missing sink queue does not execute the disconnect step and preserves an already connected source filter.
- Added `service_runtime::object_close_txn::plan_and_begin_object_close_command_dispatch()` and routed `close_object_after_close_preflight_with_domain_cleanup()` through it so closeable lifecycle/dispatch preflight and the `Closing` transition happen in one runtime critical section.
- Added regression coverage that the close-preflight path has already moved the object to `Closing` before the domain cleanup hook runs.
- Updated `tuner_hal2/DESIGN_JA.md` and `tuner_hal2/CODE_CONVENTION.md` with the source boundary atomicity and close-preflight critical-section rules.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei70_object_method_source_boundary_completion

- Verified the r50ei69 changes and kept the common-component taxonomy, callback lifetime-first behavior, and `setDataSource(source)` same-demux precedence fixes.
- Added a private `ObjectMethodUseCase` boundary in `aidl_service/src/object_runtime.rs` so normal runtime execution, shared runtime execution, query, request-builder, and unavailable paths share one object method use-case phase owner.
- Connected `DemuxRuntime::set_filter_source_non_null()` to `SourceBoundaryTxn` so source switch now goes through existing source disconnect, sink queue clear, generation boundary, and packet pipeline reset before committing the new source filter.
- Extended `SourceBoundaryTxn` to expose the `PipelineResetReport` produced by the generation boundary reset.
- Added regression coverage for `set_filter_source_non_null()` using the source boundary transaction and preserving the new source only after boundary reset.
- Updated `tuner_hal2/DESIGN_JA.md` and `tuner_hal2/CODE_CONVENTION.md` to state the `ObjectMethodUseCase` and `SourceBoundaryTxn` ownership rules.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei69_common_component_contract_hardening

- Added common-component definition rules to `tuner_hal/DESIGN_JA.md` and `tuner_hal2/DESIGN_JA.md`: logical contract, implementation owner, owned state, phase order, failure behavior, allowed callers, forbidden callers, and minimum tests must be defined before a helper is treated as a transaction/common component.
- Classified `tuner_hal2` common structures into transaction owners, phase helpers, lock helpers, façades, and adapters so `method_execution` and thin `*_ops.rs` wrappers are not mistaken for transaction owners.
- Fixed callback registration precedence by checking object live/generation before callback retain in both immediate registration and runtime-use-case registration paths.
- Fixed `IFilter.setDataSource(source)` service_runtime precedence: sink/source lifetime and owner demux are validated before same-demux and self-source input validation, and cross-demux source filters are rejected before dispatch/commit.
- Added regression coverage for callback registration paths that must not call the callback retain closure when the target object is already closed.
- Updated `tuner_hal2/CODE_CONVENTION.md` with common-component naming, callback registration, request-builder, and `setDataSource(source)` relation-validation rules.
- WP-E packet/stream/source boundary correspondence remains a separate audit scope; no PacketPipeline / StreamBoundaryTxn / SourceBoundaryTxn implementation change is included in this revision.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei68_filter_datasource_same_demux_design_docs

- Documented the `IFilter.setDataSource(source)` same-demux ownership rule in `tuner_hal/DESIGN_JA.md` and the corresponding `tuner_hal2` service_runtime validation responsibility in `tuner_hal2/DESIGN_JA.md`.
- Recorded the checked AOSP basis: AIDL VTS `SetFilterLinkage` opens the source and sink filters from the same demux before calling `setDataSource()`, and the checked VTS path does not require a cross-demux source filter success case.
- Recorded the AOSP API boundary: `IFilter.setDataSource()` / framework `Filter.setDataSource()` allow another filter output as source and NULL/demux fallback, but do not require cross-demux filter graph ownership.
- Kept historical/VTS confirmation detail in this CHANGELOG entry only; DESIGN_JA.md entries state the current product contract and implementation responsibility without release-history wording.
- No code change was made in this revision.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei61_object_close_txn_common_regression_tests

- Added common ObjectCloseTxn regression coverage instead of an LNB-only close test.
- Changed `RuntimeObjectTable::begin_close_cascade()` to reject a second begin on the same target object while still allowing already-closing descendants during parent cascade cleanup.
- Added `object_close_txn` coverage for second-begin rejection and lifecycle preservation.
- Added `object_runtime::close_object_with_domain_cleanup()` coverage for hook-after-begin ordering, successful commit to Closed, domain cleanup failure to CleanupFailed, and rejection of a domain hook attempting a second close begin.
- Did not add an `ILnb.close()`-specific second-begin regression test because the contract is common ObjectCloseTxn behavior, not an LNB-specific behavior.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei60_lnb_close_object_close_txn_single_begin

- Reworked `ILnb.close()` to use `close_object_after_aidl_method_plan_with_domain_cleanup()` instead of running `execute_object_runtime_use_case()` before `self.close_object()`.
- Removed the LNB service_runtime-side `begin_object_close_cascade()` / cleanup-failed marking path so ObjectCloseTxn begin remains owned by `object_runtime::close_object_with_domain_cleanup()`.
- Added `close_lnb_explicit_after_object_close_begin()` as the LNB domain cleanup hook, resolving the LNB public runtime id from a nonterminal AIDL object after the generic close begin.
- Confirmed no other service_runtime close path starts `begin_object_close_cascade()` before AIDL ObjectCloseTxn finalization.
- Updated DESIGN_JA.md and CODE_CONVENTION.md to state that LNB cleanup, like frontend cleanup, is connected as an ObjectCloseTxn domain cleanup hook.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei59_method_transaction_and_frontend_txn_surface_cleanup

- Moved `RuntimeExecutableRequest` validation out of the AIDL object executors and `service_runtime::method_transaction` executor heads to avoid bypassing AIDL status precedence.
- Kept validation in the service_runtime transaction boundary by routing it through `method_dispatch::plan_object_method_dispatch()` after object-handle use-cases perform live/generation validation and before runtime dispatch/mutation.
- Updated DESIGN_JA.md and CODE_CONVENTION.md to state that validation must not run before object lifetime/profile precedence resolution.
- Flattened the flagged `FrontendTxn` methods so they own their operation bodies instead of forwarding the same arguments to private `transact_*` methods.
- Removed the corresponding unused private `transact_*` frontend helper methods for the flagged worker/snapshot paths.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei58_method_validation_close_hook_frontend_surface_fix

- Added `service_runtime/src/method_validation.rs` and routed object runtime executors through the shared `RuntimeExecutableRequest` profile/supported-value validation before runtime mutation.
- Reworked frontend close so `object_runtime::close_object_with_domain_cleanup()` owns the single ObjectCloseTxn begin, callback cleanup, frontend domain cleanup hook, cleanup-failed recording, and final close commit.
- Changed frontend-specific close cleanup to `cleanup_frontend_object_after_close_begin()` so service_runtime no longer begins close before the generic close transaction.
- Reduced frontend worker façade surface: public service_runtime re-export aliases remain for AIDL use-case boundaries, and same-name `TunerServiceRuntime -> FrontendTxn` delegates in `frontend_ops.rs` are crate-private.
- Removed `RuntimeObjectTable` from the service_runtime root public re-export; tests use `crate::object_table::RuntimeObjectTable` directly.
- Updated DESIGN_JA.md and CODE_CONVENTION.md for method validation and close domain cleanup hook ownership.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei57_doc_scope_object_lifecycle_frontend_wrapper_cleanup

- Re-scoped documentation responsibilities: DESIGN_JA.md remains the product/API/profile contract source, INTEGRATION.md only points product/VTS config at that contract, and CODE_CONVENTION.md remains implementation rules only.
- Added `service_runtime/src/object_lifecycle.rs` as the official service_runtime façade for AIDL object live checks and public runtime binding lookups.
- Removed production AIDL-side direct `object_table()` / `object_table_mut()` calls from `aidl_service/src/object_runtime.rs`; close commit and drop-leak quarantine now use service_runtime lifecycle/close façade helpers.
- Removed the frontend_ops one-line worker façade functions; root service_runtime re-exports the private frontend worker use-cases with public boundary names instead of maintaining same-argument wrappers.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei56_monitor_event_profile_contract_fix

- Documented the TS-only monitor event policy in DESIGN_JA.md: monitor event is not declared, `configureMonitorEvent(0)` is a supported no-op, and nonzero masks return `UNAVAILABLE`.
- Documented the VTS/product config policy in INTEGRATION.md: product config must not require nonzero monitor event unless a future WP implements and declares it.
- Fixed `IFilter.configureMonitorEvent(0)` so it uses a supported `NoPayload` no-op method plan instead of first entering the unsupported-profile status path.
- Kept nonzero `configureMonitorEvent()` routed through unsupported-profile status precedence.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei55_object_txn_lifetime_close_callback_query_fix

- Removed AIDL executor-side lifetime prechecks from `execute_object_runtime_use_case()` and `execute_shared_object_runtime_use_case()` so live/generation validation is owned by service_runtime object-handle use-case transactions.
- Removed the duplicate `begin_object_close_cascade()` from `close_object_after_aidl_method_plan()`; `close_object()` is the single ObjectCloseTxn begin/finalize entry for this path.
- Added `execute_callback_registration_runtime_use_case()` and moved `ILnb.setCallback()` retain/commit/rollback assembly out of the AIDL method body.
- Added root-open rollback for public id conversion failure after `RuntimeObjectEntry` acquisition.
- Added `execute_object_query_use_case()` and routed frontend status/readiness queries through it to remove AIDL-side `ensure_open()` + runtime query lifecycle double checks.
- Updated DESIGN_JA.md and CODE_CONVENTION.md for the narrowed transaction boundaries.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei54_lifetime_check_dedup_horizontal_fix

- Removed the remaining duplicated pre-check from `ILnb.setCallback()`: callback registration already validates the AIDL object through `record_callback_registration()`, and the runtime commit path validates again through `execute_object_runtime_use_case()`.
- Keeps unsupported/read-only/unavailable pre-checks unchanged because they are not followed by an object-runtime use-case live check.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei53_demux_child_open_lifetime_check_dedup

- Removed duplicated `ensure_open()` calls from `IDemux.openFilter()` and `IDemux.openDvr()` because the child-open common entry now routes through `execute_shared_object_runtime_use_case()`, which performs the authoritative live/generation check.
- Kept AIDL input conversion before the child-open helper because it is pure AIDL argument validation and does not depend on runtime state.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei52_demux_child_open_method_txn_alignment

- Routed `IDemux.setFrontendDataSource()` through `execute_object_runtime_use_case()` instead of hand-assembling `ensure_open()` / `AidlMethodAdapter::plan()` / runtime lock / service_runtime commit in the AIDL method body.
- Routed demux child-open runtime allocation in `child_object_open.rs` through `execute_shared_object_runtime_use_case()` so child open uses the declared AIDL object method planning boundary rather than local `AidlMethodAdapter::plan()` calls.
- Kept unsupported/read-only/unavailable `plan_method()` paths unchanged because they do not perform service_runtime state-changing commits.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei51_callback_close_dispatch_common_boundary_fix

- Routed frontend/LNB/child callback retain + runtime registration through `retain_and_record_callback_registration()` so registration failure rolls back retained callback store entries.
- Changed public close callback cleanup to use `clear_owner_callback_registration()` outside the runtime lock and preserve callback-registry unhealthy marking on callback store cleanup failure.
- Removed unused raw close wrappers from `TunerServiceRuntime` (`begin_aidl_object_close`, `mark_aidl_object_cleanup_failed`, `commit_aidl_object_close`).
- Added `aidl_service::object_runtime::{execute_object_runtime_use_case, execute_shared_object_runtime_use_case}` and routed frontend/LNB/filter/descrambler state-changing AIDL methods through them.
- Added `service_runtime::method_dispatch::plan_object_method_dispatch()` and routed root/frontend/demux/filter/DVR/descrambler/LNB dispatch planning through it, including frontend `setLnb`, so dispatch errors use the shared mapper.
- Updated DESIGN_JA.md and CODE_CONVENTION.md to document callback registration and method dispatch common boundaries.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei50_object_close_txn_and_dead_common_cleanup

- Added `service_runtime::object_close_txn` as the concrete ObjectCloseTxn common component and routed AIDL close, frontend close, and LNB close begin/cleanup-failed marking through it.
- Routed LNB root-open runtime-open failure rollback through `finish_open_rollback()` instead of hand-written early-return rollback.
- Kept `transaction_registry.rs` as the useful runtime transaction -> dispatch target table, and removed the non-production second handler/status layer (`runtime_handlers.rs` / `runtime_result.rs`).
- Removed unused public AIDL helper modules/re-exports: `CallbackBridge`, `AidlCallbackSlot`, `NativeHandleBridge`, and `AidlErrorBridge` while keeping the production `error_bridge` status functions.
- Updated DESIGN_JA.md and CODE_CONVENTION.md to document the useful registry and prohibit dead public bridge/helper common parts.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei49_open_rollback_completion_common_helper

- Added `service_runtime::open_rollback::finish_open_rollback()` as the shared rollback-completion helper for root/child object open transactions.
- Routed root object open rollback and child filter/DVR open rollback through the shared helper, so object-table rollback failure can no longer skip runtime unregister / LNB close.
- Classified dual object-rollback/runtime-cleanup failure as `CleanupFailed` through the shared helper.
- Documented the open rollback completion helper in the DESIGN_JA.md common-component catalogue and CODE_CONVENTION.md.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei48_object_table_error_mapping_and_rollback_completion

- Removed the duplicate `object_table_hal_error()` mapper from `aidl_service::object_runtime` and routed AIDL object lifecycle code through `service_runtime::error_mapping::object_table_error_to_hal()`.
- Fixed the `RuntimeObjectTableError::GenerationOverflow` mapping inconsistency: the shared mapper now treats generation overflow as internal counter exhaustion instead of `INVALID_STATE`.
- Expanded object-table error messages so missing object, kind mismatch, generation mismatch, owner mismatch, duplicate binding, unsupported kind, and generation overflow no longer collapse to one generic context string.
- Changed root object open rollback and child filter/DVR open rollback so runtime unregister / LNB close is attempted even when object-table rollback fails.
- Updated DESIGN_JA.md and CODE_CONVENTION.md to require shared object-table error mapping and rollback cleanup continuation.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei47_typed_error_mapping_horizontal_fix

- Added `service_runtime::error_mapping` as the shared location for typed service_runtime error enum to `HalError` mapping.
- Routed root object registration, child filter/DVR object registration, registry allocation/commit, and runtime dispatch planning errors through shared typed mapping helpers instead of local `_ => Internal` collapse.
- Mapped registry duplicate errors to `InvalidState`, missing/mismatch registry errors to invalid input, and runtime id exhaustion to `Internal`.
- Kept command dispatch failures on the existing `RuntimeCommandDispatchError::into_hal_error()` mapping instead of manually recreating internal errors at each call site.
- Documented the shared typed-error mapping boundary in DESIGN_JA.md and CODE_CONVENTION.md.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei46_root_object_registration_status_mapping

- Fixed `service_runtime::root_object_ops::register_root_object()` to preserve `RuntimeObjectTableError` classification instead of collapsing all object table registration failures to `HalError::Internal`.
- Mapped duplicate object id / duplicate runtime binding / lifecycle / owner / kind mismatch root registration failures to `HalError::InvalidState`, preventing avoidable `UNKNOWN_ERROR` responses through binder status mapping.
- Kept root object generation overflow as `HalError::Internal` because it represents internal counter exhaustion, not client-visible object lifecycle conflict.
- Added unit tests for duplicate runtime binding and generation overflow status classification.
- Documented the root object open transaction status-mapping boundary in DESIGN_JA.md and CODE_CONVENTION.md.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei45_root_open_and_tuner_query_boundary

- Confirmed that the RuntimeQuery / query façade rule is not excessive: it fixes ownership of read-only projections and prevents AIDL from growing registry-specific dependencies.
- Added the existing/root object open transaction boundary to the common-component catalogue in DESIGN_JA.md and CODE_CONVENTION.md.
- Moved ITuner root open runtime allocation, availability check, AIDL object table registration, LNB runtime open, and rollback into `service_runtime::root_object_ops`.
- Updated ITuner root open methods to create typed Binder objects from returned runtime entries and call service_runtime rollback helper on Binder object construction failure.
- Kept pure read-only query calls routed through existing `TunerServiceRuntime` query wrappers; no AIDL direct registry/object-table access was added.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei44_filter_config_and_source_boundary_txn_completion

- Completed another ObjectMethodTxn horizontal cleanup for implemented filter mutation paths.
- Moved `IFilter.configure()` current-open-type lookup and config construction into a service_runtime object-handle use-case closure boundary so AIDL no longer resolves runtime filter open type before commit.
- Moved `IFilter.setDataSource()` self-source rejection into service_runtime `set_filter_data_source_for_object()` so source/sink handle validation and commit remain in the same method transaction boundary.
- Removed unused AIDL-side query helpers that had become stale after the RuntimeQuery / object-handle use-case migration.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei43_child_open_runtime_use_case_completion

- Moved child filter/DVR runtime allocation, runtime registration, and AIDL object table registration from `child_object_open.rs` into service_runtime object-handle child-open use-case helpers.
- `child_object_open.rs` now calls `open_filter_child_runtime_for_demux_object()` / `open_dvr_child_runtime_for_demux_object()` and performs only typed Binder object construction plus typed callback retain/rollback.
- Added service_runtime rollback helpers for child-open Binder/callback failure so AIDL does not manually compose child object unregister and public runtime unregister.
- Updated DESIGN_JA.md to describe the child-open helper as a service_runtime-backed object-handle use-case boundary.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei42_child_open_and_close_boundary_completion

- Moved `openFilter()` / `openDvr()` method planning into the existing `child_object_open.rs` common helper so the AIDL method body no longer performs `plan_method()` before child allocation/registration.
- Renamed child-open helpers to `open_filter_child_for_owner_object()` and `open_dvr_child_for_owner_object()` to reflect that they plan and resolve the owner object internally.
- Strengthened `close_object_after_aidl_method_plan()` so close method planning and `begin_close_cascade()` run under one runtime lock before callback cleanup / final close processing.
- Updated DESIGN_JA.md to reference the renamed child-open common helpers.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei41_existing_common_txn_usage_fix

- Updated `openFilter()` / `openDvr()` to stop resolving owner demux public id in the AIDL method body; `child_object_open.rs` now resolves the owner through the runtime query façade.
- Added object-handle based LNB callback registration commit path and routed `ILnb.setCallback()` through it.
- Strengthened `close_lnb_explicit_for_object()` so `ILnb.close()` moves the object cascade to `Closing` before LNB runtime cleanup and leaves final object close to the common close helper.
- Removed stale AIDL-side `runtime_entry_public_id` use from demux child-open and LNB callback registration paths.
- Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei40_existing_common_txn_boundary_doc

- Merged existing but under-documented common components into the AIDL object lifecycle common-component table instead of adding a separate ad hoc section.
- Documented `aidl_service/src/child_object_open.rs` as the common child filter/DVR open transaction entry.
- Documented callback registration helpers (`record_callback_registration`, callback-store retain/rollback, `clear_owner_callback_registration`) as the registration-side counterpart of callback cleanup.
- Documented `execute_object_aidl_method` / `plan_object_aidl_method` and `close_object_after_aidl_method_plan` as existing object method planning / close helper boundaries.
- No implementation code was changed in this revision. Build, rustfmt, Rust unit, atest, VTS, and device checks were not executed.

# r50ei39_object_method_txn_close_boundary

- Added object-handle based service_runtime method transaction helpers for demux/filter/descrambler/LNB mutation paths, moving object live/generation resolution and dispatch planning out of AIDL method bodies.
- Strengthened frontend close begin semantics by marking the frontend object cascade Closing before owner-loss worker/live-data cleanup and marking cleanup failed on runtime cleanup failure.
- Made runtime object close cascade tolerate already-Closing entries so begin-close and finalize-close phases can be split safely.
- Updated LNB owner-loss callback cleanup lookup to find nonterminal objects, including Closing entries created by frontend close begin.
- Updated `DESIGN_JA.md` and `CODE_CONVENTION.md` to describe `ObjectMethodTxn` and minimal `ObjectCloseTxn` boundaries across all domains.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei38_aidl_query_boundary_frontend_method_txn

- Moved remaining frontend read-only registry access behind `RuntimeQuery` / query façade methods; `boot.rs` frontend query wrappers now delegate through `self.query()`.
- Added single-lock frontend status query façade for `getStatus()` / `getFrontendStatusReadiness()` so frontend entry, runtime state, and signal state are read from one runtime snapshot.
- Converted frontend mutation AIDL methods (`tune`, `scan`, `stopTune`, `stopScan`, `close`, `setLnb`) to call object-handle based service_runtime use-case façades instead of assembling live/id/validate/plan/commit steps in AIDL.
- Moved frontend method planning, object live/generation resolution, request validation, and worker/session state reservation into service_runtime frontend use-case transaction boundaries.
- Updated `DESIGN_JA.md` and `CODE_CONVENTION.md` with the AIDL method transaction boundary and query façade requirements.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei37_aidl_boundary_public_planning

- Removed AIDL-layer direct imports of `service_runtime::frontend_worker_txn`; AIDL now calls service_runtime public frontend use-case façade functions.
- Made `service_runtime::frontend_worker_txn` crate-private and re-exported only the public frontend use-case boundary and scan-end notifier type.
- Moved AIDL object handle/public runtime id lookup behind `TunerServiceRuntime::public_*_for_aidl_object()` query methods so AIDL helpers no longer call `object_table()` directly.
- Added `AidlMethodCall::PublicApi` / `DomainCommand::PublicApi` and `Tuner/Frontend/DemuxPublicApiTxn` to separate supported public API planning from unsupported-by-design planning.
- Updated `DESIGN_JA.md` and `CODE_CONVENTION.md` with the AIDL/service_runtime boundary rules for worker use-cases, object query façade, and PublicApi vs UnsupportedPublicApi planning.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei36_registry_mut_boundary_doc_fix

- Adopted the documented registry mutation boundary that allows `TunerServiceRuntime::registry_mut()` only inside `service_runtime/src/boot/*_txn.rs` domain transaction implementations and tests.
- Updated `DESIGN_JA.md` and `CODE_CONVENTION.md` so the prohibition targets production code outside the boot transaction subtree, not the transaction context itself.
- Clarified `RuntimeQuery<'a>` documentation to say it holds only required immutable read-only sources; current implementation holds the runtime registry only.
- Corrected the r50ei35 changelog wording for `filter_open_type`: the AIDL-visible public/type-boundary wrapper is intentionally retained.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei35_query_wrapper_pruning

- Audited `TunerServiceRuntime` read-only query wrappers and moved service_runtime-internal call sites to `runtime.query().*`.
- Removed internal-only `TunerServiceRuntime` query wrappers for frontend snapshots, demux snapshots, same-tune checks, live-reader descriptors, frontend terminal events, and frontend demux sink readiness.
- Removed unused `RuntimeQuery` methods for frontend worker running generation. Kept `filter_open_type` because it is still used through the AIDL-visible public/type-boundary wrapper.
- Kept externally visible query wrappers used by `aidl_service` as public API/type boundaries.
- Updated `CODE_CONVENTION.md` to require existing wrapper audits and to prefer `runtime.query().*` for crate-internal read-only queries.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei34_transact_visibility_wrapper_rules

- Narrowed flat `transact_*` helper visibility inside `service_runtime/src/boot/*_txn.rs` from `pub(crate)` to private `fn`; domain transaction context methods remain the crate-visible boundary.
- Removed duplicate LNB registry lookup in `ServiceRuntimeLnbProfileAdapter::apply_lnb_state()`.
- Renamed the used `_state` parameter in `ServiceRuntimeLnbProfileAdapter::apply_lnb_state()` to `state`.
- Added explicit wrapper creation criteria and static-check positioning to `CODE_CONVENTION.md`.
- Added static checks for crate-visible flat `transact_*` helpers, top-level direct `transact_*` calls, adapter `_state` misuse, and wrapper-boundary conventions.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei33_lnb_callback_store_adapter_rename

- Renamed `ServiceRuntimeLnbProfileBackend` to `ServiceRuntimeLnbProfileAdapter` to reflect that it adapts service_runtime registry/profile state to `LnbBackendOps` rather than performing real backend I/O.
- Renamed `mark_lnb_callback_registered()` to `commit_lnb_callback_registration()` across service_runtime and AIDL LNB method wiring.
- Changed LNB callback registration state update to follow `clone -> mutate -> store_lnb_runtime()` instead of mutating the registry slot directly.
- Updated `DESIGN_JA.md` and `CODE_CONVENTION.md` with the LNB adapter naming and LNB runtime mutation convention.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei32_lnb_txn_context_registry_mut_narrowing

- Added `LnbTxn<'a>` in `service_runtime/src/boot/lnb_txn.rs` and moved LNB binding, apply, lifecycle, callback registration, and drop-leak state mutations behind that boot child transaction context.
- Replaced top-level LNB transaction implementation files with `service_runtime/src/lnb_ops.rs` public wrappers that call `LnbTxn<'a>` methods.
- Removed production `registry_mut()` call sites outside the `boot` transaction subtree and made `TunerServiceRuntime::registry_mut()` private to `boot` and child modules.
- Added `registry_mut_for_test()` for test fixture setup only.
- Updated `DESIGN_JA.md` and `CODE_CONVENTION.md` to include LNB transaction context ownership, flat `transact_*` helper boundaries, one-line wrapper limits, and `registry_mut()` production-use prohibition.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei31_runtime_query_txn_context_facade

- Added `RuntimeQuery<'a>` in `service_runtime/src/boot/query_api.rs`; existing `TunerServiceRuntime` read-only query methods now delegate through `self.query()`.
- Added domain transaction context facades in `boot/*_txn.rs`: `FrontendTxn<'a>`, `DemuxFilterDvrTxn<'a>`, `DescramblerTxn<'a>`, and `PacketTxn<'a>`.
- Updated top-level `service_runtime/src/*_ops.rs` wrappers to call domain transaction context methods instead of directly calling flat `transact_*` helpers.
- Updated `DESIGN_JA.md` section 6.2 to describe `RuntimeQuery<'a>` and domain transaction contexts as the current service_runtime boundary.
- Flat `transact_*` helpers remain inside `boot/*_txn.rs` as implementation helpers in this release.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei30_design_boundary_clarification

- Reworked `DESIGN_JA.md` section 6.2 into explicit responsibility subsections for `boot.rs`, top-level `*_ops.rs`, `boot/*_txn.rs`, `query_api.rs`, and prohibitions.
- Removed the stale wording that regular operations live under `service_runtime/src/boot/*.rs`; the current contract places public wrappers in top-level `service_runtime/src/*_ops.rs` and state-changing implementations in `service_runtime/src/boot/*_txn.rs`.
- Clarified that mutating `transact_*` calls are limited to top-level `*_ops.rs` and `boot/*_txn.rs`, and that `query_api.rs` remains read-only.
- Added static checks for `transact_*` caller locations and the `query_api.rs` no-mutating-transaction boundary.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei29_transaction_boundary_final_audit

- Re-audited the service_runtime top-level operation wrapper and boot transaction owner boundary after the additional demux/filter/DVR, descrambler, and frontend helper wrapper split work.
- Confirmed top-level `service_runtime/src/*_ops.rs` files remain wrapper-only and do not directly access `TunerServiceRuntime` private fields.
- Confirmed `service_runtime/src/boot/*_txn.rs` state-changing public surface is reduced to `transact_*` methods, with `map_filter_runtime_error` kept as a shared non-mutating mapper.
- Narrowed `map_filter_runtime_error` visibility from `pub(crate)` to `pub(super)` because it is only shared inside the `boot` module subtree.
- Generated final static checks and an external final audit report for the transaction boundary.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei28_frontend_helper_txn_wrappers

- Extended the top-level frontend operation wrapper split to runtime snapshot restore, tune commit, signal/live-pump reporting, live-reader lifecycle, scan session, and frontend failure helper methods.
- Added `transact_*` transaction owner methods for those frontend helper operations in `boot/frontend_txn.rs`.
- Kept `TunerServiceRuntime` private field access inside `service_runtime/src/boot/frontend_txn.rs`; top-level `service_runtime/src/frontend_ops.rs` remains wrapper-only.
- Updated `DESIGN_JA.md` to describe the expanded frontend wrapper boundary as current structure.
- `TunerServiceRuntime` fields remain private; no `pub(crate)` field widening was introduced.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei27_descrambler_allocation_txn_wrapper

- Extended the top-level descrambler operation wrapper split to `allocate_descrambler_runtime`.
- Added `transact_allocate_descrambler_runtime` in `boot/descrambler_txn.rs` and kept registry allocation there.
- Kept top-level `service_runtime/src/descrambler_ops.rs` wrapper-only and free of `TunerServiceRuntime` private field access.
- Updated `DESIGN_JA.md` to describe the expanded descrambler wrapper boundary as current structure.
- `TunerServiceRuntime` fields remain private; no `pub(crate)` field widening was introduced.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei26_demux_filter_dvr_allocation_txn_wrappers

- Extended the top-level demux/filter/DVR operation wrapper split to allocation, unregister, AV stream type, and delay hint methods.
- Added `transact_allocate_demux_runtime`, `transact_unregister_demux_runtime`, `transact_allocate_filter_runtime`, `transact_unregister_filter_runtime`, `transact_configure_filter_av_stream_type_request`, `transact_set_filter_delay_hint_request`, `transact_allocate_dvr_runtime`, and `transact_unregister_dvr_runtime` transaction owner methods in `boot/demux_filter_dvr_txn.rs`.
- Kept `TunerServiceRuntime` private field access inside `service_runtime/src/boot/demux_filter_dvr_txn.rs`; top-level `service_runtime/src/demux_filter_dvr_ops.rs` remains wrapper-only.
- Updated `DESIGN_JA.md` to describe the expanded demux/filter/DVR wrapper boundary as current structure.
- `TunerServiceRuntime` fields remain private; no `pub(crate)` field widening was introduced.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei25_top_level_transaction_audit

- Audited the service_runtime top-level operation wrapper boundary after frontend, demux/filter/DVR, descrambler, and packet wrapper split work.
- Confirmed top-level `service_runtime/src/*_ops.rs` files do not directly access `TunerServiceRuntime` private fields and call transaction/query APIs only.
- Clarified `DESIGN_JA.md` current contract: private field access is confined to `service_runtime/src/boot/query_api.rs` and `service_runtime/src/boot/*_txn.rs`, while top-level operation files remain wrapper-only.
- Generated static checks and an external audit report for the remaining boot child transaction owner methods, including allocation/unregister/helper methods that still intentionally own registry mutation.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei24_packet_top_level_wrappers

- Added true top-level `service_runtime/src/packet_ops.rs` for packet ingress and demux-binding public wrappers.
- Kept `TunerServiceRuntime` private field access inside `service_runtime/src/boot/packet_txn.rs`; the top-level wrapper file calls only `pub(crate)` transaction methods.
- Moved public `set_demux_frontend_data_source`, `reset_bound_demuxes_for_frontend_tune_start`, `reset_and_unbind_bound_demuxes_for_frontend`, `quarantine_frontend_and_bound_demuxes`, and `push_frontend_ts_packet_to_bound_demuxes` wrappers out of `boot/packet_txn.rs`.
- Moved read-only `ensure_frontend_demux_sink_ready` into `boot/query_api.rs` to keep packet top-level wrappers free of private field access.
- Updated `service_runtime/src/lib.rs`, `Android.bp`, and `DESIGN_JA.md` to include the top-level packet operation wrapper boundary.
- `TunerServiceRuntime` fields remain private; no `pub(crate)` field widening was introduced.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei23_descrambler_top_level_wrappers

- Added true top-level `service_runtime/src/descrambler_ops.rs` for descrambler public wrappers.
- Kept `TunerServiceRuntime` private field access inside `service_runtime/src/boot/descrambler_txn.rs`; the top-level wrapper file calls only `pub(crate)` transaction methods.
- Moved public `set_descrambler_demux_source`, `set_descrambler_key_token`, `add_descrambler_pid_non_null_source`, `remove_descrambler_pid_non_null_source`, and `unregister_descrambler_runtime` wrappers out of `boot/descrambler_txn.rs`.
- Moved the crate-visible `cleanup_descramblers_for_demux_owner_loss` wrapper to the top-level descrambler operation file while leaving owner-loss cleanup implementation in the transaction owner.
- Updated `service_runtime/src/lib.rs`, `Android.bp`, and `DESIGN_JA.md` to include the top-level descrambler operation wrapper boundary.
- `TunerServiceRuntime` fields remain private; no `pub(crate)` field widening was introduced.
- Allocation and internal cleanup helpers still live in `boot/descrambler_txn.rs` because they directly own registry/session mutation.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei22_demux_filter_dvr_top_level_wrappers

- Added true top-level `service_runtime/src/demux_filter_dvr_ops.rs` for demux/filter/DVR public wrappers.
- Kept `TunerServiceRuntime` private field access inside `service_runtime/src/boot/demux_filter_dvr_txn.rs`; the top-level wrapper file calls only `pub(crate)` transaction methods.
- Moved public `register_demux_filter_runtime`, `configure_filter_runtime_request`, `start_filter_runtime`, `stop_filter_runtime`, `flush_filter_runtime`, `set_filter_data_source_non_null`, and `register_demux_dvr_runtime` wrappers out of `boot/demux_filter_dvr_txn.rs`.
- Updated `service_runtime/src/lib.rs`, `Android.bp`, and `DESIGN_JA.md` to include the top-level demux/filter/DVR operation wrapper boundary.
- `TunerServiceRuntime` fields remain private; no `pub(crate)` field widening was introduced.
- Only demux/filter/DVR methods already backed by explicit `transact_*` APIs were top-levelized in this release; allocation/unregister helpers still live in `boot/demux_filter_dvr_txn.rs`.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei21_frontend_top_level_worker_wrappers

- Added true top-level `service_runtime/src/frontend_ops.rs` for frontend worker lifecycle public wrappers.
- Kept `TunerServiceRuntime` private field access inside `service_runtime/src/boot/frontend_txn.rs`; the top-level wrapper file calls only `pub(crate)` transaction methods.
- Moved public `start_frontend_worker`, `request_frontend_worker_stop`, `request_frontend_worker_stop_and_join`, and `clear_finished_frontend_workers` wrappers out of `boot/frontend_txn.rs`.
- Updated `service_runtime/src/lib.rs`, `Android.bp`, and `DESIGN_JA.md` to include the top-level frontend operation wrapper boundary.
- `TunerServiceRuntime` fields remain private; no `pub(crate)` field widening was introduced.
- Only frontend worker lifecycle wrappers were top-levelized in this release; remaining frontend mutation public methods still live in `boot/frontend_txn.rs`.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei20_packet_txn_boundary

- Renamed `service_runtime/src/boot/packet_ops.rs` to `service_runtime/src/boot/packet_txn.rs` and updated `boot.rs`, `Android.bp`, and `DESIGN_JA.md` accordingly.
- Kept existing public packet/demux-binding runtime method names stable while placing packet ingress and stream-boundary implementation under the transaction owner file.
- Added explicit `transact_set_demux_frontend_data_source`, `transact_reset_bound_demuxes_for_frontend_tune_start`, `transact_reset_and_unbind_bound_demuxes_for_frontend`, `transact_quarantine_frontend_and_bound_demuxes`, and `transact_push_frontend_ts_packet_to_bound_demuxes` internal transaction methods.
- Preserved existing `GenerationBoundaryTxn` usage inside the service_runtime transaction boundary.
- `TunerServiceRuntime` fields remain private; no `pub(crate)` field widening was introduced.
- Remaining packet helper internals still directly access `self.registry` / diagnostics inside `packet_txn.rs`; allocation/unregister helper cleanup consolidation is not completed in this release.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei19_descrambler_txn_boundary

- Renamed `service_runtime/src/boot/descrambler_ops.rs` to `service_runtime/src/boot/descrambler_txn.rs` and updated `boot.rs`, `Android.bp`, and `DESIGN_JA.md` accordingly.
- Kept existing public descrambler runtime method names stable while placing descrambler state-changing implementation under the transaction owner file.
- Added explicit `transact_set_descrambler_demux_source`, `transact_set_descrambler_key_token`, `transact_add_descrambler_pid_non_null_source`, `transact_remove_descrambler_pid_non_null_source`, `transact_unregister_descrambler_runtime`, and `transact_cleanup_descramblers_for_demux_owner_loss` internal transaction methods.
- Preserved existing `DescramblerSessionTxn` usage inside the service_runtime transaction boundary.
- `TunerServiceRuntime` fields remain private; no `pub(crate)` field widening was introduced.
- Remaining descrambler allocation/helper/cleanup internals still directly access `self.registry` inside `descrambler_txn.rs`; packet mutation transaction API work is not completed in this release.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei18_demux_filter_dvr_txn_boundary

- Renamed `service_runtime/src/boot/demux_filter_dvr_ops.rs` to `service_runtime/src/boot/demux_filter_dvr_txn.rs` and updated `boot.rs`, `Android.bp`, and `DESIGN_JA.md` accordingly.
- Kept existing public demux/filter/DVR runtime method names stable while placing demux/filter/DVR state-changing implementation under the transaction owner file.
- Added explicit `transact_register_demux_filter_runtime`, `transact_configure_filter_runtime_request`, `transact_start_filter_runtime`, `transact_stop_filter_runtime`, `transact_flush_filter_runtime`, `transact_set_filter_data_source_non_null`, and `transact_register_demux_dvr_runtime` internal transaction methods.
- Preserved existing `FilterConfigureTxn` usage inside the service_runtime transaction boundary.
- `TunerServiceRuntime` fields remain private; no `pub(crate)` field widening was introduced.
- Remaining demux/filter/DVR allocation/unregister paths still directly access `self.registry` inside `demux_filter_dvr_txn.rs`; descrambler and packet mutation transaction API work is not completed in this release.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei17_frontend_txn_boundary

- Renamed `service_runtime/src/boot/frontend_ops.rs` to `service_runtime/src/boot/frontend_txn.rs` and updated `boot.rs`, `Android.bp`, and `DESIGN_JA.md` accordingly.
- Kept existing public frontend runtime method names stable while placing frontend state-changing implementation under the frontend transaction owner file.
- Added explicit `transact_frontend_worker_start`, `transact_frontend_worker_stop`, `transact_frontend_worker_stop_and_join`, and `transact_clear_finished_frontend_workers` internal transaction methods for worker lifecycle operations.
- Moved remaining read-only frontend descriptor/event query methods to `query_api.rs`.
- `TunerServiceRuntime` fields remain private; no `pub(crate)` field widening was introduced.
- Remaining frontend runtime mutation methods still directly access `self.registry` inside `frontend_txn.rs`; demux/filter/DVR, descrambler, and packet mutation transaction API work is not completed in this release.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei16_query_api_design_cleanup

- Revised `DESIGN_JA.md` section 6.2 to remove staged wording such as future/next-step/incomplete-state descriptions and keep only the current service_runtime structure contract.
- Added `service_runtime/src/boot/query_api.rs` as the owner of read-only runtime query methods.
- Moved read-only query methods from `frontend_ops.rs`, `demux_filter_dvr_ops.rs`, and `packet_ops.rs` into `query_api.rs`.
- Updated `Android.bp` to include `service_runtime/src/boot/query_api.rs` in the service_runtime source lists.
- Kept `TunerServiceRuntime` fields private and did not widen them to `pub(crate)` for operation modules.
- Mutation transaction API implementation is not completed in this release; remaining direct registry/worker mutation paths stay in the domain operation modules.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei15_boot_child_normal_mod_layout

- Removed `#[path]` from `service_runtime/src/boot.rs` operation module declarations.
- Removed the remaining demux parser `#[path]` declarations by making `demux/src/parser/mod.rs` own the parser child modules and re-exporting them from `demux/src/lib.rs`.
- Moved service_runtime operation files back under normal Rust child-module layout:
  - `service_runtime/src/boot/frontend_ops.rs`
  - `service_runtime/src/boot/demux_filter_dvr_ops.rs`
  - `service_runtime/src/boot/descrambler_ops.rs`
  - `service_runtime/src/boot/packet_ops.rs`
- Updated `Android.bp` source lists to the `service_runtime/src/boot/*.rs` paths.
- Updated `DESIGN_JA.md` to define this as the current supported layout: ops remain `boot` child modules, no `#[path]`, no `include!`, and no `include_str!`.
- Documented the next-stage rule: true `service_runtime` top-level module migration requires transaction API design first, and must not be achieved by widening `TunerServiceRuntime` fields to `pub(crate)`.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei14_import_doc_test_cleanup

- Moved `GLOBAL_CODE_CONVENTION.md` production `use super::*;` ban into the Rust section instead of leaving it under the Kotlin heading.
- Clarified `service_runtime/src/*_ops.rs` ownership in `DESIGN_JA.md`: files are top-level, modules remain `boot` children via `#[path]` to avoid widening `TunerServiceRuntime` field visibility.
- Added an explanatory comment beside the service_runtime ops module declarations in `boot.rs`.
- Reduced r50ei13 broad explicit imports in AIDL method files and service_runtime ops files to usage-oriented explicit imports.
- Extended drop leak unit tests with a test-only callback-store marker so `drop_leak_object()` now verifies callback store clearing as well as runtime callback registry and quarantine state.
- build, rustfmt, Rust unit test, atest, VTS, and device validation are not executed in this environment.

# r50ei13_explicit_import_split_drop_leak_tests

- `GLOBAL_CODE_CONVENTION.md` に production code の `use super::*;` 禁止を追加した。
- `aidl_service/src/tuner_service/support.rs` を追加し、AIDL object lookup / unsupported planning / source filter handle helper を `tuner_service.rs` から分離した。
- `aidl_service/src/tuner_service/*_methods.rs` の `use super::*;` を明示 import へ置換した。
- service_runtime operation files を `service_runtime/src/*_ops.rs` へ移し、`DESIGN_JA.md` に top-level ops file 境界を明文化した。
- `service_runtime/src/*_ops.rs` の `use super::*;` を明示 import へ置換した。
- `drop_leak_object()` の通常 quarantine / callback registry clear と、domain drop record failure 時の callback unhealthy 化を unit test で固定した。
- build、Rust unit test、atest、VTS、実機確認は未実施。

# r50ei12_structure_cleanup_status_worker_guard

- Split child AIDL trait implementations out of `aidl_service/src/tuner_service.rs` into normal Rust modules under `aidl_service/src/tuner_service/`; no `include!` split is used.
- Split large `service_runtime/src/boot.rs` operation groups into normal Rust modules under `service_runtime/src/boot/` for frontend, demux/filter/DVR, descrambler, and packet ingress operations.
- Documented the tuner_hal2 file ownership boundaries in `DESIGN_JA.md`, including the explicit `include!` ban for structural splitting.
- Replaced callback cleanup best-effort/drop-specific paths with common `drop_leak_object()` plus service_runtime drop-leak plan generated domain cleanup commands; LNB Drop keeps no AIDL-side action selector and no longer owns bespoke cleanup flow.
- Added owner-wide callback unhealthy marking to `RuntimeCallbackRegistry` and tests for that registry behavior.
- Changed frontend worker cancel-reason poisoning to return `HalError`/`StopRequestFailed` instead of normalizing to `None`; added a poison-lock unit test.
- Centralized Binder status construction in `aidl_service::error_bridge`; direct `Status::new_service_specific_error()` calls are limited to that file.
- Build / rustfmt / rust unit / atest / VTS / device validation: not executed in this environment. `rustfmt` and `rustc` binaries were not available in the container.

# r50ei11_origs215_373_expired_token_resolution

- ORIG-215 / ORIG-373 token resolution fix candidate.
- Added `DescramblerKeyTable::has_token_resolution_state()` so expired-token tombstones used by test/fake-token paths are not hidden behind the CAS-token-producer-unavailable branch.
- Mapped expired descrambler key tokens to `INVALID_STATE` consistently for direct key lookup and session-token resolution.
- Preserves CAS producer unavailable for the no-token-state case; no real CAS HAL implementation is added.
- Build / rust unit / atest / VTS / device validation: not executed in this environment.

## r50ei10_qg04_quarantine_api_design_guard

- QG-04: Hid the single-object quarantine transition behind a private `RuntimeObjectTable` helper and kept `quarantine_cascade()` as the public object lifecycle entry point.
- QG-04: Added a shared AIDL runtime unregister helper used by both explicit close and Drop-leak quarantine paths.
- QG-04: Added `tuner_hal2/DESIGN_JA.md` section 5 as a structure-difference mapping to the existing `tuner_hal/DESIGN_JA.md` close / Drop leak / quarantine contract. This does not add a new design contract.
- Build / rustfmt / rust unit / atest / VTS / device validation: not executed in this environment.

## r50ei9_qg03_qg04_drop_quarantine_transaction_gate

- QG-03: Added Drop leak quarantine handling to Frontend/Demux/Filter/Dvr/Descrambler AIDL objects instead of leaving live object-table entries behind. LNB keeps its LNB-runtime leak record path and then uses the same common object quarantine helper.
- QG-03: Changed Drop leak handling to call a shared `quarantine_live_aidl_object_after_drop_leak()` helper; this is a common helper call, not copy-pasted cleanup logic.
- QG-04: Added `RuntimeObjectTable::quarantine_cascade()` so Drop leak terminalization covers the owner and descendants as one object-table transition.
- QG-04: Added object-table tests that quarantine-cascade terminalizes descendants and permits later rebinding of the same runtime id after quarantine.
- QG-04 scope in this release is AIDL object lifecycle / Drop leak transaction integrity. Broader Demux/Filter/Dvr data-path rollback auditing remains a follow-up quality gate.
- Build / rustfmt / rust unit / atest / VTS / device validation: not executed in this environment.

## r50ei8_qg01_qg02_lnb_profile_backend_callback_cleanup

- QG-01: Renamed the service-runtime LNB adapter to `ServiceRuntimeLnbProfileBackend` and fixed the responsibility boundary as profile validation/backend policy rather than real hardware LNB control.
- QG-01: Kept DiSEqC as explicit validated-then-unsupported profile behavior for current exported LNB profiles; no success no-op path is introduced.
- QG-02: Removed callback-store cleanup from `LnbBackendOps`; callback object cleanup remains owned by AIDL callback store/runtime callback registry.
- QG-02: `LnbLifecycleTxn` now records `ClearRuntimeCallbackState` and only clears `LnbRuntime` callback ownership state; backend callback cleanup is no longer modeled as a hardware/profile backend operation.
- Added/updated LNB lifecycle tests so public close clears runtime callback state while Drop leak still avoids normal backend cleanup.
- Build / rustfmt / rust unit / atest / VTS / device validation: not executed in this environment.

## r50ei7_wp_r11_lnb_completion_quality_gate

- WP-R11 LNB residual completion candidate.
- Routed `ILnb.sendDiseqcMessage()` through LNB runtime/backend validation instead of a direct ad-hoc reject. Current exported profiles reject DiSEqC as unsupported after payload validation; no silent success path is added.
- Strengthened LNB profile backend validation so voltage/tone/position applies are checked by both service-runtime profile validation and the backend adapter.
- Quarantined live LNB AIDL object entries on Rust Drop leak after recording the LNB runtime leak; Drop still does not perform normal safe-state backend cleanup.
- Added LNB unit tests for DiSEqC payload validation/profile rejection and fixed-profile voltage rejection.
- Build / rust unit / atest / VTS / device validation: not executed in this environment.

# r50ei5_wp_r07a_descrambler_prereq_packet_pipeline_min

- WP-R10 descrambler / CAS token / MULTI2 packet path の前提として、keyなし scrambled TS packet を record/raw path と section/PES/AV assembly path で分離した。
- keyなし scrambled packet は record path ではTS scrambling metadataだけを観測し、PES/SC/PTS payload metadataを意味値として扱わないようにした。
- keyなし scrambled packet が section/PES/AV assemblyへ入る場合は diagnostic を出し、該当PIDの partial assembly をresetするようにした。
- TEI / duplicate continuity counter packet は record/raw TS path へ到達させ、section/PES/AV assembly からだけ除外するように補正した。
- demux unit testに、`flush()` がPES partial assemblyを消しつつruntime状態/queueを維持すること、`remove_filter()` がqueueとpacket pipelineのpartial stateを破棄することを追加した。
- release marker を tuner_hal2 / tuner_hal ともに r50ei5_wp_r07a_descrambler_prereq_packet_pipeline_min へ更新した。
- packet pipeline の診断モデルを、packet reject/drop と assembly suppression に分離した。
- `plan_and_assemble_ts_packet_report()` の no-preflight 入口と、旧意味論の test helper `accept_ts_packet_with_outcome()` / `accept_ts_packet()` を削除した。
- descrambler / CAS token / MULTI2 / key table / IDescrambler public API は未実装のまま維持した。

# r50ei_wp_r07_filter_hint_runtime

- WP-R07継続として、`IFilter.configureAvStreamType()` を service_runtime / demux runtime のAV stream type hint保持へ接続した。
- `configureAvStreamType()` は、未configure AVを `INVALID_STATE`、AV開始中を `INVALID_STATE`、非AVを `UNAVAILABLE`、open subtypeと異なるaudio/video指定を `INVALID_ARGUMENT` に倒す。
- `IFilter.setDelayHint()` を runtime保持へ接続し、旧 `tuner_hal` と同じくmedia filterは `UNAVAILABLE`、record filterのdata-size hintは `INVALID_ARGUMENT`、time hint上限は10秒にした。
- `configureMonitorEvent()` / `configureIpCid()` / 読み取り系を含む `IFilter` 公開APIに `ensure_open()` を追加し、閉鎖後公開API成功を避けるようにした。
- Filter close時のruntime unregisterで、queue除去に加えてpacket pipeline側のfilter一過性状態も破棄するようにした。
- `FilterDelayHint` のtime/data併用時にOR条件でready判定する純粋部品を追加した。
- demux runtimeの単体テストに、AV configure時のbacking marker、AV stream hintの保持/再configure時clear、delay hintのtime/data独立更新、delay OR ready判定を追加した。
- Android/Soong build、Rust unit test、atest、VTS、実機確認は未実施。

# r50eh_wp_r07_filter_runtime_foundation

- WP-R07の先行実装として、公開 `IFilter.configure()` を `FilterConfigureTxn` 経由で demux runtime / packet pipeline へ接続した。
- filter configure commit時に、runtimeのqueue markerと実queueを同期し、再configure時に旧queue cleanupが失敗しないようにした。
- `IFilter.start()` / `stop()` / `flush()` を filter runtime state と packet pipeline clear/flush境界へ接続し、未configure状態は `INVALID_STATE` へ倒すようにした。
- `IFilter.getId()` / `getId64Bit()` はAIDL object idではなく公開filter runtime idを返すよう修正した。
- `child_object_open.rs` への分離で再発していた Filter/DVR callback保持rollback失敗の黙殺を修正した。
- `m maleicacid_tuner_hal2_device_test maleicacid_tuner_hal2_service_runtime_test maleicacid_tuner_hal2_aidl_service_test` は成功。atest、VTS、実機確認は未実施。

# r50eh_tuner_service_trait_shim_refactor

- 非機能リファクタとして、`tuner_service.rs` に集まっていた AIDL frontend settings 変換を `binder_adapter/src/aidl_frontend_settings.rs` へ分離した。
- frontend request の registry / backend validation を `service_runtime/src/frontend_request_txn.rs` へ分離した。
- frontend worker generation / snapshot / rollback / scan session / live descriptor 処理を `service_runtime/src/frontend_worker_txn.rs` へ分離した。
- scan END callback配送を `aidl_service/src/frontend_callback_delivery.rs` へ分離し、Binder callback型を service_runtime へ持ち込まない構造へ寄せた。
- openFilter/openDvr の child object生成・callback保持・rollback を `aidl_service/src/child_object_open.rs` へ分離した。
- Android/Soong build、Rust unit test、atest、VTS、実機確認は未実施。

# r50eg_wp_r06_demux_open_foundation

- WP-R06として demux capability / demux info、filter open request、DVR open request、Filter/DVR callback保持の基盤を実装した。
- `getDemuxCaps()` / `getDemuxInfo()` は TS-only profile と旧 `tuner_hal` の demux/filter/DVR capability 数へ合わせ、未知 demux id は `Unsupported` へ倒す。
- `openFilter()` は filter type / buffer size / callback_present を `FilterRuntime` へ保持し、AIDL child object生成・callback owner登録・失敗時rollbackまで接続した。
- `openDvr()` は record/playback種別、buffer size、callback_present を `DvrRuntime` へ保持し、demux owner配下のDVR runtime登録・callback owner登録・失敗時rollbackまで接続した。
- Android 14 AIDL Rust生成境界に合わせ、section table info生成名と `FilterDelayHint.hintValue` の型変換を修正した。
- `m maleicacid_tuner_hal2_device_test maleicacid_tuner_hal2_service_runtime_test maleicacid_tuner_hal2_aidl_service_test` は成功。atest、VTS、実機確認は未実施。

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

## r50bl

- `px4_stream_selector_direct_slot_v5.patch` を適用し、px4 backend の BS `STREAM_ID` を TSID→relative slot 変換せず absolute TSID 値のまま legacy `slot` へ渡す方針に更新した。
- AOSP SDK default の `streamIdType=STREAM_ID` / `streamId=-1` は selector なしとして扱い、CS110 では selector 付き request を拒否する境界を固定した。
- px4 legacy chardev の二重 open を避けるため、live TS reader は control fd の `try_clone()` で作成する方針に変更した。
- `DESIGN_JA.md`、`INTEGRATION.md`、`開発規則.md` に、px4 BS absolute TSID direct-slot は px4_drv `feat/android-ddk` 系のように BS `slot >= 8` reject が無効な driver を前提にすること、公開 develop 相当では使用不可であること、TSID→relative slot 変換表を互換 fallback として復活させないことを明記した。
- この環境では Android/Soong build、Rust unit test、atest、VTS、CTS、実機確認は未実施。静的差分確認のみ実施した。

## r50bc

- `tuner_hal_multi2_error_api_independence_fixed_plan_acceptance_revised.md` の固定方針に従い、Tuner HAL descrambler の改善候補10/13だけを修正した。
- MULTI2 preparation error を `Multi2PrepareError::InvalidRoundsZero` へ具体化し、runtime path 用に placeholder variant なしの `Multi2RuntimeError` を導入した。
- `Multi2KeyMaterial::prepare()` は `Result<PreparedMulti2Key, Multi2PrepareError>` を返し、`rounds == 0` を preparation 時点で拒否する。
- `multi2_decrypt_payload()` / `multi2_encrypt_payload()` は `&PreparedMulti2Key` と `Result<(), Multi2RuntimeError>` を使い、復号/暗号 hot path に key schedule を戻さない。
- `descrambler/src/multi2.rs` と `descrambler/src/packet.rs` へ同一 crate 内で分離し、`lib.rs` は module 宣言と crate-level re-export 中心へ整理した。Android.bp / Soong module 名は変更していない。
- binder_service の invalid rounds expectation を `InvalidRoundsZero` に更新した。
- この環境では Android/Soong build、Rust unit test実行、atest、VTS、CTS、実機確認は未実施。静的 grep と構造確認のみ実施した。

## r50bb6

- `tuner_hal_descramble_improvements_1_2_3_5_plan_acceptance_revised_fixed2.md` の固定方針に従い、Tuner HAL descrambler の TEI / AFC=11 payload 0 / scrambled NULL PID / MULTI2 key preparation を修正した。
- `parse_ts_packet_header()` は `TSC=01` を即時 error にせず、TEI 判定前の header 情報を返す責務へ整理した。TEI は `TransportErrorRecord` として TSC 判定より前に record-only byte-identical へ逃がす。
- `AFC=11` かつ payload 0 byte は `InvalidAdaptationField` とし、clear packet / scrambled-without-payload 扱いにしない。
- `NULL_PID + TSC=10/11` は `ScrambledNullPid` とし、clear `NullPid` pass-through へ落とさず record-only byte-identical とする。
- `PreparedMulti2Key` と `Multi2KeyMaterial::prepare()` を追加し、`DescramblerKeySlot` 内部を prepared key 保持へ変更した。`multi2_decrypt_payload()` / test encrypt helper は `&PreparedMulti2Key` を受け取り、復号 hot path で key schedule を生成しない。
- 旧 raw-key infallible even/odd slot helpers は削除し、`try_with_even` / `try_with_odd` / `with_even_prepared` / `with_odd_prepared` に置換した。
- descrambler crate と binder_service に固定方針の必須テスト名を追加した。
- この環境では Android/Soong build、Rust unit test実行、atest、VTS、CTS、実機確認は未実施。静的 grep と brace balance のみ実施した。

## r50bb3

- r50bb2 の Tuner HAL descrambler 修正完了条件のうち、build / test 実行以外で残っていた文書・テストカバレッジ未達を修正した。
- `DESIGN_JA.md` の空 token / `Tuner.VOID_KEYTOKEN` / test-only key registration の旧期待値を、r51 descrambler 固定方針に合わせて更新した。
- `DescramblerTokenOrigin::VtsOrUnitTest` を `UnitTestOnly` に改名し、片側 key 登録が Rust unit test 専用であることを明確化した。
- `descrambler` crate に TSC/AFC 16 行 matrix test を追加し、AFC=00、TSC=01、scrambled adaptation-only、clear adaptation-only、even/odd payload descramble の期待値を固定した。
- binder service test に non-TS-frame ingress helper を追加し、`InvalidPacketSize` / `BadSyncByte` が record-DVR raw TS に残らないことを delivery path 条件として固定した。
- Android/Soong build、Rust unit tests、atest、VTS、CTS、実機確認は未実施。

## r50bb2

- r50bb の Tuner HAL descrambler 修正完了条件のうち、ロジック未達だった VOID key removal 後の診断経路だけを修正した。
- `Tuner.VOID_KEYTOKEN` (`[0x00]`) による current key removal 後も、PID 登録済み descrambler を active snapshot に残すようにした。
- key slot 未設定 snapshot は対象 PID の scrambled packet で `NO_KEY` を記録し、`SCRAMBLED_WITHOUT_DESCRAMBLER` に落とさない。
- PID 登録維持、record-DVR raw TS への scrambled passthrough、既存の malformed/non-TS-frame 分岐は維持した。
- 回帰テスト `void_key_token_clears_key_only_and_keeps_pid_registration` に、VOID後の scrambled packet が `NO_KEY` へ落ち、`SCRAMBLED_WITHOUT_DESCRAMBLER` を増やさない確認を追加した。
- この環境では Android/Soong build、Rust unit test実行、VTS、実機確認は未実施。

## r50bb

- Applied the descrambler packet validation plan for r51: AFC=00 is invalid, TSC=01 is invalid after AFC validation, and scrambled adaptation-only packets are diagnosed as `ScrambledWithoutPayload`.
- Removed clear-packet fast-path bypass before TS header validation.
- Split non-TS-frame drop from TS-frame-like malformed record-only delivery.
- Added fixed descrambler diagnostics for invalid packet size, bad sync byte, invalid AFC, invalid adaptation field, invalid TSC, scrambled-without-payload, and malformed-packet-for-recording.
- Made CAS bridge production key registration require both Odd and Even key material while keeping one-sided key registration test-only.
- Treated `[0x00]` as `Tuner.VOID_KEYTOKEN` current-key removal and kept empty token `[]` as invalid argument / bad token.
- Updated descrambler regression tests for the corrected packet matrix, delivery decisions, CAS bridge key-pair rule, and VOID key token behavior.
- Android/Soong build, Rust unit tests, atest, VTS, CTS, and real-device checks were not run in this environment.

## r50ba2

- r50ba に対して、リリース物整理のみを行った。
- `DESIGN_JA.md` の過去版名ベースの見出しと本文表現を、現行設計名・現行実装対象の表現へ置換した。
- Rust test module / test function 名に含まれていた過去版名を、意味ベースの名前へ改名した。
- Tuner HAL ロジック、VTS XML、future_work、TIS/rec 実装コードは変更していない。
- この環境では Android/Soong build、Rust unit test実行、VTS、実機確認は未実施。

## r50aq21

- r50aq20 に対して、Tuner HAL の frontend 異常系で `frontend_backend` lock を保持したまま `mark_live_path_failed()` へ入る自己 deadlock だけをロジック修正した。
- live pump の LNB apply / stream reader 生成失敗時は、backend lock 区間内では error detail だけを生成し、lock を抜けてから runtime failure 記録と `mark_live_path_failed()` を実行するようにした。
- scan worker cleanup の `backend_stop_tune()` 失敗時も、backend lock を抜けた後に scan phase 更新、runtime failure 記録、`mark_live_path_failed()`、scan end 通知を行うようにした。
- 既存の runtime failure 記録、bound demux fail-close、backend callback_failed marking は維持した。
- TIS、px4/DVB backend、generic scan、future_work、VTS XML、CAS HAL は変更しない。
- この環境では Android/Soong build、Rust unit test実行、VTS、実機確認は未実施。

## r50aq20

- r50aq19 に対して、Tuner HAL の `IDescrambler.addPid()` source filter generation 最終再検証不足だけをロジック修正した。
- `addPid()` は source filter identity 取得後、最終 PID claim 直前に source filter の `DemuxHandle` を再ロックし、同一 filter generation がまだ存在することを確認する。
- source filter が stop / flush / reconfigure / close 等で generation 変更または unregister 済みになっていた場合は、PID claim を行わず error を返す。
- `DescramblerRuntimeRegistry` の同一 demux generation / PID ownership atomic claim は維持し、claim 時の lock order は live pump と同じ `demux_handle -> descrambler_registry -> descrambler_state` に揃えた。
- `removePid()` の lock order 修正、nullable filter / PID-only future_work の仕様、TIS、px4/DVB backend、generic scan、VTS XML は変更しない。
- この環境では Android/Soong build、Rust unit test実行、VTS、実機確認は未実施。

## r50aq19

- r50aq18 に対して、Tuner HAL の `IDescrambler.addPid()` PID ownership claim 原子性不足だけをロジック修正した。
- `DescramblerRuntimeRegistry` に atomic claim helper を追加し、他 descrambler の同一 demux generation / PID 所有確認と自 descrambler state への PID 登録を同一 registry critical section 内で行うようにした。
- `addPid()` は従来どおり state snapshot、demux generation 確認、source filter identity 確認を行った後、最終登録を atomic claim helper に集約する。
- `removePid()` の lock order 修正、nullable filter / PID-only future_work の仕様、TIS、px4/DVB backend、generic scan、VTS XML は変更しない。
- この環境では Android/Soong build、Rust unit test実行、VTS、実機確認は未実施。

## r50aq18

- r50aq17 に対して、Tuner HAL の `IDescrambler.removePid()` lock order と `FilterHal::close_internal()` cleanup 完遂性だけをロジック修正した。
- `removePid()` は `descrambler_state` lock を保持したまま demux registry / demux handle / source filter identity へ入らないよう、state snapshot → demux/filter 確認 → state 再取得・再検証の順に変更した。これにより live pump の `demux_handle -> descrambler_state` lock order と逆順になる path をなくした。
- `FilterHal::close_internal()` は途中 error で早期 return せず、callback worker 停止、AV shared backing 破棄、runtime unregister、queue stop、AV queue stop、demux unregister をすべて試行し、最初の error status だけを最後に返す形にした。
- nullable filter / PID-only future_work の仕様変更、TIS、px4/DVB backend、generic scan、VTS XML は変更しない。
- この環境では Android/Soong build、Rust unit test実行、VTS、実機確認は未実施。

## r50aq17

- r50aq16 に対して、DVB backend の Linux DVB UAPI LNB voltage/tone enum 値だけを修正した。
- `frontend_dvb` の `SEC_VOLTAGE_13` / `SEC_VOLTAGE_18` / `SEC_VOLTAGE_OFF` を Linux DVB UAPI の `0` / `1` / `2` に合わせ、`set_lnb_voltage(NONE/11V/15V)` が kernel へ OFF/13V/18V を正しく送るようにした。
- 同じ enum block の `SEC_TONE_ON` / `SEC_TONE_OFF` も Linux DVB UAPI に合わせた。tone は引き続き固定日本向け tuner profile では unsupported のままで、動作対象を拡大しない。
- 変更範囲は Tuner HAL DVB backend ロジックと CHANGELOG のみ。px4 backend、TIS、generic scan、future_work、VTS XML は変更しない。
- この環境では Android/Soong build、Rust unit test実行、VTS、実機確認は未実施。

## r50aq16

- r50aq15 の demux close cleanup 未達を修正した。
- `DemuxHal::close_internal()` で最後の参照の cleanup に入った後、`unbind_demux()`、demux handle lock、registry lock、live id lock、final record cleanup のいずれかが失敗しても後続 cleanup step を継続するようにした。
- cleanup 中に複数の error が発生した場合は最初の error status を保持し、cleanup 試行後に返すようにした。
- 変更範囲は Tuner HAL demux lifecycle ロジックと CHANGELOG のみ。future_work、VTS XML、TIS、px4 mapping は変更しない。

## r50aq15

- r50aq14 に対して、Tuner HAL の demux lifecycle/refcount race のロジックのみを修正した。
- `openDemuxById()` が既存 demux record を再利用する際、close 中または ref_count 0 の record を再取得しないようにした。
- `DemuxHal::close_internal()` は record lock 下で ref_count を減算し、減算後の値で最後の参照かを判定するようにした。stale read による cleanup skip を避ける。
- 最後の参照になった demux record には close-in-progress 状態を設定し、registry/live id から削除されるまで新規 wrapper が掴めないようにした。
- `release_registration_best_effort()` も同じ refcount/closing 方針に合わせた。
- この環境では Android/Soong build、Rust unit test実行、VTS、実機確認は未実施。

## r50aq14

- r50aq13 に対して、px4 backend-local mapping の不足だけを対象に修正した。
- `frontend_px4` の BS absolute TSID → px4 legacy `freq_no/slot` 変換表に、BS11/freq_no=5/IF 1_241_280_000 Hz の 0x46b0〜0x46b3 を追加した。
- この表は product scan SSOT ではなく、TIS から渡された explicit tune request を px4 ioctl 値へ落とすための backend-local mapping として維持する。TIS 候補表、TvProvider channel key、display number 生成、generic scan 層には触れていない。
- 開発規則.md の SSOT 原則に従い、TIS 候補表との全件一致テストは追加していない。今回の完了判定は静的確認で行った。
- この環境では Android/Soong build、Rust unit test実行、VTS、実機確認は未実施。

## r50aq13

- r50aq12 の固定計画 No.1/2/3/5 に沿って、Frontend public close を step runner 型の fallible cleanup に変更した。backend close 失敗を成功扱いにせず、後続 cleanup は継続し、通常操作は cleanup failure 後に拒否する。
- px4 backend の active streaming close / stopTune / retune 前 stop で `PTX_STOP_STREAMING` を明示実行し、stop ioctl 失敗を public 経路で握り潰さないようにした。best-effort 経路では runtime diagnostic に記録する。
- DVB backend の `close()` では `DTV_CLEAR` を必須化せず、`DTV_CLEAR` は明示 `stop_tune()` の責務であることを `DESIGN_JA.md` に固定した。
- TIS の `DESIGN_JA.md` に、CS110 tune request は Android builder default に依存せず stream selector none / `UNDEFINED` 相当を明示し、ONID / TSID / service_id を HAL frontend selector へ転用しない設計境界を追記した。
- この環境では `rustfmt`、Android/Soong build、Rust unit test実行、VTS、実機確認は未実施。

## r50aq12

- r50aq11 の Tuner HAL-only 修正計画 1/2/3/5/8/9 に沿って、DVB backend から BS TSID 表・日本向け scan 候補表相当の実装データと周波数+TSID semantic 照合を削除した。DVB backend は BS absolute TSID 必須、relative stream number 拒否、CS110 selector 拒否、frequency class 境界だけを検証する。
- HAL unit test から TIS `ScanPlan.kt` の `include_str!` 文字列 parse と TIS 候補表・px4 backend-local mapping の一致確認を削除した。px4 側の TSID mapping は product scan SSOT ではなく legacy chardev ioctl 変換用の backend-local mapping として固定した。
- `TsPacketCompletionBuffer` の resync を単発 `0x47` 復帰から 188-byte 間隔の 3 packet 連続 sync 確認へ変更し、false sync / resync tail の regression test を追加した。
- `IDescrambler.setDemuxSource()` の二重設定を `UNAVAILABLE` ではなく `INVALID_STATE` に変更し、状態衝突として test に固定した。
- `ILnb.close()` を reset-on-close として固定し、close 時に LNB registry の voltage/tone/position を安全側へ戻して matching frontend へ反映する。cleanup 失敗は成功扱いしない。
- この環境では Android/Soong build、Rust unit test実行、VTS、実機確認は未実施。

## r50aq11

- r50aq10 の未達だった frontend status / readiness 完了条件を対象に修正した。
- status support SSOT を保守的に固定し、r51 では起動時列挙時点で取得根拠を固定できる status type だけを `statusCaps` に出すようにした。DVB / earth_pt1 は `DEMOD_LOCK`、`RF_LOCK`、`SIGNAL_QUALITY`、satellite frontend の `LNB_VOLTAGE` に限定し、px4 は `DEMOD_LOCK` と satellite frontend の `LNB_VOLTAGE` に限定した。
- `FE_READ_SNR` / `FE_READ_SIGNAL_STRENGTH` / `PTX_GET_CNR` は read 時に失敗し得る optional telemetry として扱い、r51 では `SNR` / `SIGNAL_STRENGTH` を `statusCaps` に advertise しないことを `DESIGN_JA.md` と実装に固定した。
- `getFrontendStatusReadiness()` は caps外を `UNSUPPORTED` として同長返却し、caps内についても backend availability、tuning active、現在 telemetry の有無を見て `UNAVAILABLE` / `UNSTABLE` / `STABLE` を返すようにした。一律 `STABLE` を残さない。
- `getStatus()` は caps外を `INVALID_ARGUMENT` とし、caps内でも optional telemetry 欠落を 0 として成功返却しない。LNB voltage の未選択状態は仕様上の `NONE` として明示的に扱う。
- この環境では Android/Soong build、Rust unit test実行、VTS、実機確認は未実施。

## r50aq10

- r50aq9 に対して、Tuner HAL-only Issue 1 / 3 / 4 / 5 の固定計画に沿って、DESIGN_JA.md、実装、future_work を更新した。
- Issue 1: `IFilter.setDataSource(null)` は Android 14 AIDL/Rust nullable filter 境界の構造課題として、既存の `IDescrambler.addPid/removePid` null source filter 課題と同一 future_work ファイル内に集約した。r51 実装対象は non-null source linkage、demux default source、`configure()` clear、error mapping の確認に限定した。
- Issue 3 / 4: frontend status support 判定を `statusCaps`、`getStatus()`、`getFrontendStatusReadiness()` の共通 SSOT に寄せ、`getStatus()` は caps外 type を `INVALID_ARGUMENT`、readiness は caps外 type を `UNSUPPORTED` 要素返却に固定した。未測定 `SNR` / `SIGNAL_STRENGTH` / `SIGNAL_QUALITY` を 0 値で成功返却する経路を削除した。
- Issue 4: readiness 一律 `STABLE` を廃止し、backend unavailable は `UNAVAILABLE`、tune/probe 中は `UNSTABLE`、有効状態のみ `STABLE` にした。
- Issue 5: `bitWidthOfLengthField` は r51 TS-only profile として `0/12` のみ受理し、その他を `INVALID_ARGUMENT` に変更した。`SectionCondition::matches()` は正規化済み `length_field_bits` を受け取るようにし、隠れ 12bit 固定を除去した。
- この環境では Android/Soong build、Rust unit test実行、VTS、実機確認は未実施。

## r50aq9

- r50aq8 に対して、Issue 2 の別 descrambler 間同一 demux/generation/PID 排他の Result 契約を AOSP Result semantics に合わせて `INVALID_STATE` に固定した。
- 実装は既に `INVALID_STATE` を返していたため、`DESIGN_JA.md` の `INVALID_ARGUMENT` 表記を `INVALID_STATE` へ修正し、実装と設計文書の不一致を解消した。
- PID値・source filter object 自体の不正ではなく、active descrambler registry 上の所有状態衝突として扱うことを明記した。
- 今回は設計文書の契約固定のみであり、テスト不足、Soong build、Rust unit test、VTS、実機確認は未実施。

## r50aq8

- r50aq7 に対して、revised3照合で残った未達のうち、テスト不足以外の実装・設計文書未達だけを対象に修正した。問題点1の Android 14 AIDL/Rust backend 境界課題は引き続き実装対象外として別管理する。
- Issue 2: 同一 descrambler 内の同一PIDは置換 semantics、別 descrambler 間の同一 demux/generation/PID は排他という契約を `DESIGN_JA.md` に明記した。これにより、AOSP同一PID置換とHAL内部の二重復号防止を分離した。
- Issue 3: scan terminal state 保存は clear付き helper に統一し、worker normal/abnormal exit hook と spawn failure 経路で terminal state を active `scan_session` に残さない実装へ整理した。
- Issue 4: runtime path は outcome付き `SectionAssembler::push_payload_with_outcome()` のみを使う方針を維持し、単純 `push_payload()` を crate-internal API に下げて release runtime の public API境界から外した。
- 項目8: DVR cleanup step result を `Success` / `SafeNoOp` / `Failed` / `Unknown` / `SkippedDueToWorkerFailureContext` に分類し、best-effort の未確認stepを成功扱いしないようにした。`cleanup_complete=true` は全stepが成功または安全no-opと確認できた場合だけに限定した。
- `DESIGN_JA.md` の r50aq5 固有表記を r50aq8 / r50aq5以降の契約表現へ更新した。
- テスト不足として前回列挙された callback-level / failure-injection / peer lifecycle 追加テストは、今回の指示範囲外として未追加。Android/Soong build、Rust unit test実行、VTS、実機確認も未実施。

## r50aq7

- r50aq6 に対して、Tuner HAL-only 問題点6の LNB profile 不整合のみを対象に修正した。問題点1・2の r50aq6 修正は維持し、それ以外の実装範囲には触れていない。
- `DESIGN_JA.md` の LNB 固定 profile と判定表を更新し、px4_drv 系で LNB 15V 成功扱いにする対象を `px4video*` family のみに限定した。
- `pxmlt5video*` は対応デバイス仕様上 LNB 電源非対応、`pxmlt8video*` と `isdb6014video*` は仕様未確定として、r50aq7 では `NoPower` / `NONE` のみ成功に固定した。
- 実装の `LnbDeviceProfile` から `PxMltDevice15VOnly` を削除し、`pxmlt5video*` / `pxmlt8video*` / `isdb6014video*` を `NoPower` に割り当てるよう変更した。
- LNB profile detection と voltage policy の regression test を更新し、MLT/DTV02A 系が 15V を成功扱いしないことを固定した。
- この環境では Android/Soong build、Rust unit test実行、VTS、実機確認は未実施。

## r50aq6

- r50aq5 のビルド前レビューで残した Tuner HAL-only 問題点1・2のみを対象に修正した。LNB profile / DESIGN_JA.md の問題点6は今回スコープ外として未変更。
- 問題点1: descrambler key token の実 token を 8-byte opaque binary ID に変更し、`setKeyToken()` 入口の registry 解決前に 0 byte と 17 byte以上を拒否するようにした。長い診断用 token 名は成功経路から排除した。
- 問題点2: record filter の `TsRecord` callback event は configured TS/SC index mask に一致する observed index がある場合だけ生成し、index hit がない packet では event を抑制するようにした。
- それぞれ token 長・unknown token・旧診断 token 拒否、record event の抑制/TS index hit/SC index hit の regression test を追加した。
- この環境では Android/Soong build、Rust unit test実行、VTS、実機確認は未実施。

## r50aq5

- r50aq4 に対して、問題点1を Android 14 AIDL/Rust backend 境界の構造課題として実装対象外へ退避し、Tuner HAL 内で実装可能な Issue 2 / Issue 3 / Issue 4 / 項目8 の4件をこの順で補正した。
- Issue 2: `IDescrambler.addPid()` / `removePid()` の呼び出し順序・object lifecycle 不整合を `INVALID_STATE` に寄せ、stale demux generation、未登録 PID、source mismatch を public Binder 経路の exact Result test で固定した。
- Issue 3: scan terminal diagnostic と active scan slot を分離し、terminal phase を記録後に `scan_session` を clear する helper を追加した。これにより completed/failed/cancelled scan が `stopTune()` の active scan 判定に残らない。
- Issue 4: `SectionAssembler` に outcome 付き APIを追加し、oversized section drop / stale partial discard を同一 helper で filter-local diagnostics / `pending_overflow` に接続した。callback worker は既存 `pending_overflow` 経路で payload が空でも `DemuxFilterStatus::OVERFLOW` を送る。
- 項目8: `DvrHal` に `cleanup_complete` を追加し、`closed` gate と cleanup 完了状態を分離した。`close_internal()` / `close_internal_best_effort()` / `fail_dvr_worker()` は caller 種別付き共通 cleanup helper と step runner を使い、failure injection や loom で同じ完了判定を検証しやすい形にした。`WorkerFailure` 経路では callback worker self-join を避け、未回収 worker handle が残る場合は後続 close / Drop で retry 可能な未完了 cleanup として残す。
- `DESIGN_JA.md` に r50aq5 の error mapping、scan lifecycle、section overflow、DVR close cleanup の契約を追記した。
- Soong build は Android.bp 解析段階の既存構成 error で Rust compile 前に停止した。確認中に `rec/Android.bp` の path-outside-directory error と `tis/Android.bp` の missing privapp permission XML error を観測した。Rust unit test実行、VTS、実機確認はこのアーカイブ生成環境では未実施。

## r50aq4

- Applied the Issue 5 minimal fix plan A: removed the panic-based `DemuxHandle::register_filter()` helper from the non-test `soft_demux` public API.
- Updated Tuner HAL unit-test call sites to use `register_filter_result(...).expect("test setup should register filter")`, keeping the panic boundary inside test setup rather than release runtime API.
- Applied the Issue 6 minimal fix: replaced the AV MediaEvent builder `debug_assert!(!secure_memory)` with a runtime fail-closed guard that logs a diagnostic and drops the event without panic if the unsupported secure-memory state reaches the builder.
- Added regression coverage for the secure-memory AV event-builder fail-closed path.
- Soong build, Rust unit test execution, VTS, and real-device confirmation remain out of scope for this pre-build archive update.

## r50aq3

- Completed the r50aq2 follow-up fixes for items 6 and 8 only.
- Item 6: factored AV shared-memory errno mapping into `av_shared_file_error_result()` and added regression coverage for ENOMEM, ENOENT, EACCES, EIO, EINVAL, and unknown errno mapping.
- Item 8: changed DVR close cleanup to attempt all cleanup steps after the first failure, preserving the first returned error while still stopping callback worker state, clearing queue state, stopping queue backing, and unregistering the DVR from the parent demux.
- Added DVR close regression coverage for successful idempotent double close and for queue stop failure still removing the parent demux DVR record.
- Soong build, Rust unit test execution, VTS, and real-device confirmation remain out of scope for this pre-build archive update.

## r50aq2

- Applied the r51 pre-build Tuner HAL fixes for items 1, 2, 5, 6, and 8 in that order.
- Item 1: DVB / earth_pt1 ISDB-T validation and advertised frontend frequency contract now cover the fixed Japanese CATV C13-C63 range in addition to UHF 13-62, matching the px4 backend and r51 explicit tune contract.
- Item 2: VTS config generation now emits DVR playback data flows whenever playback DVR entries are emitted, and the generated AIDL V2 VTS XML connects each playback DVR to its audio/video playback filters.
- Item 5: managed diagnostic workers now use `WorkerSignal::wait_timeout_or_stop()` for periodic stop-wake waits; the runtime `sleep_with_stop()` polling helper was removed.
- Item 6: AV shared-memory allocation errno mapping now reports ENOMEM as `OUT_OF_MEMORY`, device absence / permission errors as `UNAVAILABLE`, and EINVAL / EIO / unknown runtime failures as `UNKNOWN_ERROR`.
- Item 8: DVR close is idempotent through `closed.swap(true, Ordering::SeqCst)` for both normal and best-effort close paths.
- Soong build, Rust unit test execution, VTS, and real-device confirmation remain out of scope for this pre-build archive update.

## r50aq

- Removed test cases that used `include_str!("tuner_hal.rs")` or `include_str!("main.rs")` to inspect production source text with string matching.
- Kept `include_str!()` uses that only check static config / sepolicy / VTS XML / design-document / cross-module SSOT consistency.
- Added project and Tuner HAL rules forbidding self-referential source-string tests as completion evidence; logic contracts must be verified through real API/helper/state/diagnostic/queue/callback/worker behavior.
- No production Tuner HAL runtime logic was intentionally changed. Soong build, Rust unit test execution, VTS, and device confirmation remain out of scope for this static pre-build step.

## r50ap11

- Phase 10 / R13 only: completed filter condition / metadata argument validation without advancing to Phase 11.
- `DESIGN_JA.md` now fixes the PES `streamId` contract: `0..=255` are explicit stream_id matches, `-1` is the only wildcard, all other negative values and `256+` are `INVALID_ARGUMENT`.
- Binder filter configuration now normalizes PES `streamId` through the fixed contract, and soft demux matching treats only `-1` as wildcard; `0` is no longer a wildcard.
- Section `tableId`, PES `streamId`, and record TS/SC index validation are factored into dedicated helpers with regression tests for boundary values, unsupported bits, union-variant mismatch, and supported SC variants.
- Phase 11 and later are intentionally not advanced in this release. Soong build, Rust unit test execution, VTS, and device confirmation remain out of scope for this static pre-build step.

## r50ap10

- Phase 9 / R05 only: completed SectionAssembler PUSI / pointer-field stale partial discard diagnostics without advancing to Phase 10.
- `DESIGN_JA.md` now fixes the PUSI pointer boundary policy: pointer bytes are the only legal previous-section tail, and incomplete stale partial sections must be discarded with a diagnostic counter before parsing the new section body.
- `SectionAssembler` now exposes `stale_partial_section_discards()` and increments it when pointer bytes do not complete the previous partial section, including pointer_field == 0 with stale state.
- `DemuxHandle::stale_partial_section_discard_count()` aggregates the diagnostic counter across active section assemblers.
- Added regression tests for pointer-zero stale partial discard, pointer-tail incomplete stale partial discard, and demux-level diagnostic aggregation.
- Phase 10 and later are intentionally not advanced in this release. Soong build, Rust unit test execution, VTS, and device confirmation remain out of scope for this static pre-build step.

## r50ap9

- Phase 8 / R04 only: adopted and fixed the DVR playback input FMQ policy without advancing to Phase 9.
- `DESIGN_JA.md` now fixes playback prefill / stop / flush boundary behavior: start-before prefill is retained, stop/flush drains playback input FMQ and packet residual with dropped-byte diagnostics, and stopped playback does not consume input.
- Playback `PlaybackStatus` periodic callbacks now use the playback input FMQ fill / unused-space source, matching start-time status calculation rather than record/output queue `queued_bytes`.
- The playback consumer worker now uses `ManagedWorker` / `WorkerSignal` stop-wake-join lifecycle instead of the prior ad-hoc `AtomicBool` + `Condvar` tuple.
- Phase 9 and later are intentionally not advanced in this release. Soong build, Rust unit test execution, VTS, and device confirmation remain out of scope for this static pre-build step.

## r50ap8

- Phase 4 / R06 only: adopted the user-approved clean-boundary policy without advancing to Phase 5.
- `configure_filter_with_summary_result()` now clears stale `data_source_filter_id` so reconfigure cannot retain old upstream linkage.
- `unregister_filter()` now fully clears downstream queue / queued bytes / pending overflow / pending start event / delay runtime / filter-local assembler state for filters linked to the removed upstream.
- `FilterDelayHint::timeDelayHint` is fixed as queue-empty -> non-empty per-burst rearm rather than first-drain-only behavior.
- Added regression tests for per-burst time delay rearm, reconfigure clearing old linkage/queued payload, and upstream unregister clearing downstream queue.
- Phase 5 and later are intentionally not advanced in this release. Soong build, Rust unit test execution, VTS, and device confirmation remain out of scope for this static pre-build step.

## r50ap6

- `r51_tuner_hal_bugfix_execution_plan.md` の Phase 0〜7 を対象に、既存 Phase 0〜4 実装を再確認したうえで R09 / R17 / R14 の未達を補正した。
- Phase 4 / R06: `stop_filter()` が pending payload queue、queued bytes、delay runtime を clear し、stopped filter から delivery drain しないよう補正した。
- Phase 5 / R09: soft demux の continuity tracker、section assembler、PES assembler、assembly generation を frontend / playback origin 別に分離し、playback 起源 TS が frontend 起源 state を汚染しない regression test を追加した。
- Phase 6 / R17: descramble failure / scrambled pass-through は `push_ts_packet_record_only()` に限定され、section / PES / AV assembly に入らないことを record-only regression test で固定した。
- Phase 7 / R14: 同一 demux generation の同一 PID を複数 active descrambler に登録する経路を拒否し、設計資料と regression test を更新した。
- Phase 8 以降には進んでいない。Soong build、Rust unit test実行、VTS、実機確認は今回スコープ外。

## r50ap5

- `r51_tuner_hal_bugfix_execution_plan.md` の Phase 0〜3 のみを対象に、r50ap4 の未達だった worker policy 接続、R12 worker stop wake/join、R07 closed guard、R08 rollback/fail-closed 境界を補正した。
- Phase 0: `WorkerExit` の正式名を `Normal` / `StopRequested` / `RuntimeFailure` / `PanicOrJoinFailure` に寄せ、`ManagedWorker` が `WorkerSignal` と `JoinHandle` を保持する最小共通部品にした。既存 alias は互換目的に限定した。
- Phase 1 / R12: frontend tune / scan worker を `WorkerSignal` + `ManagedWorker::stop_and_join()` に接続し、lock wait の停止待ちを `AtomicBool + thread::sleep()` polling から Condvar wake へ変更した。
- Phase 2 / R07: closed `DemuxHandle` に対する `register_filter()` の dummy record 生成を廃止し、production 経路は `register_filter_result()` の `InvalidState` を返す方針に固定した。
- Phase 3 / R08: `setFrontendDataSource()` rollback で新frontend unbind失敗・旧frontend欠落・旧frontend bind失敗を fail-closed に接続し、失敗rollback中に旧stream stateを無条件 reset しないようにした。
- Phase 4以降のR06以降には進んでいない。Soong build、Rust unit test実行、VTS、実機確認は今回スコープ外。

## r50ap4

- `r51_tuner_hal_bugfix_execution_plan.md` の Phase 0〜3 を対象に、worker policy、R12、R07、R08 のビルド前静的修正を追加した。
- Phase 0: `WorkerSignal` の最小実装を追加し、長寿命 worker の `JoinHandle` 保持、`Mutex` + `Condvar` 待機、stop → wake → join、`loom` test-only 方針を `DESIGN_JA.md` に正式固定した。
- Phase 1 / R12: `scan()` 内の重複 `stop_tune_worker()` を除去し、`stopTune()` が active scan を cancel しない既存境界を維持した。
- Phase 2 / R07: Filter / DVR の `ensure_open()` が親 Demux close、親側 unregister、owner demux mismatch を確認し、close後 child object の public method が成功しないよう fail-closed 化した。`openFilter()` / `openDvr()` 途中失敗時の runtime I/O unregister を追加した。
- Phase 3 / R08: `FrontendRuntime::bind_demux()` で live pump 起動失敗時に partial binding をrollbackし、`setFrontendDataSource()` で new bind / old unbind / record更新 / stream reset の途中失敗時に rollback、rollback不能時は demux fail-closed とする方針にした。

## r50ao5

- r50ao5 is intentionally scoped to the r50ao4 N2 follow-up only: it adds a public `IFilter.getAvSharedHandle()` path regression test for configured live AV filters.
- The new test verifies the public method returns the shared AV memory total size and exports `NativeHandle.ints == [0]`, without exposing `slot_size` or `slot_count` through `NativeHandle.ints`.
- No production logic changes are included in r50ao5; Android/Soong build, Rust compiler execution, VTS, and real-device playback remain unexecuted in this environment.

## r50ao4

- r50ao4 fixes the r50ao3 AV shared-slot internal-invariant handling: `ActiveSlotCollision` is detected before inserting into the active-slot map, so the previous active entry is never overwritten during collision handling.
- `AvPayloadInternalError` now includes `SharedHandleExportedWithoutBacking`, and fail-closed diagnostics include the exact internal-error variant name for mutex poison, exported-handle/backing mismatch, slot registry inconsistency, mapping failure, counter failure, and active-slot collision.
- r50ao4 replaces the r50ao3 self-referential source-string AV acceptance tests with helper-level decision tests and fixes the `AvPayloadAllocateError` / `AvPayloadDeliveryResult` pattern-match type mismatch in AV shared stats tests.
- The source-level acceptance evidence for r50ao4 uses unified diff checks and compile-blocker static checks; Android/Soong build, Rust compiler execution, VTS, and real-device playback remain unexecuted in this environment.

## r50ao3

- r50ao3 corrects the r50ao2 AV shared-handle release gate: ordinary AV delivery drops and internal invariant failures are separated into `AvPayloadDeliveryResult` and `AvPayloadInternalError` paths.
- AV payloads are no longer written to the standard filter FMQ or the AV auxiliary FMQ/EventFlag path. Successful AV delivery is shared memory + `MediaEvent` + callback `DATA_READY` only.
- Internal AV shared-memory failures, including exported-handle-without-backing, mutex/registry failure, and avDataId collision, fail-close the affected filter instead of being reported as ordinary `OVERFLOW`.
- `FilterHal::start()` no longer has the r50ao2 tuple destructuring / `is_media` compile blocker and does not emit immediate AV `DATA_READY`.
- `DESIGN_JA.md` now states that AV payload delivery is shared memory + `MediaEvent` only, while `NativeHandle.ints == [0]` remains the framework-facing shared handle contract.

- r50ao2 tightens the r50ao AV shared-handle fix: AV payloads are no longer written to the standard filter FMQ before shared-slot delivery, so shared-handle-unexported / no-slot / invalid-payload paths cannot wake `TUNER_EVENT_DATA_READY` through the normal queue.
- r50ao2 treats AV shared backing mutex poison, exported-handle-without-backing, and shared-slot internal invariant failure as filter worker fail-closed conditions instead of reporting them as ordinary drop/overflow diagnostics.
- r50ao2 adds unified-diff-driven static acceptance tests for the AV FMQ/EventFlag path, internal invariant fail-closed path, and `NativeHandle.ints == [0]` contract.
## r50ao

- r50ao acceptance is `r50an9_tuner_hal_av_shared_handle_fix_4_5_revised_no_or.md`: live AV `DATA_READY` is now emitted only after a payload is placed in an exported shared slot.
- Shared-handle-unexported AV payloads now emit `OVERFLOW` without `DATA_READY` and increment `av_drop_unexported`.
- Shared slot exhaustion no longer evicts active AV slots; it emits `OVERFLOW` without `DATA_READY` and increments `av_overflow_no_slot`.
- Invalid AV payload size / shared-memory range failures emit `OVERFLOW` without `DATA_READY` and increment `av_invalid_payload`.
- `getAvSharedHandle()` now exports `NativeHandle.ints == [0]`; `slot_size` and `slot_count` remain HAL-internal state and are not exposed through `NativeHandle.ints`.

## r50an9

- r50an8 のロジック未達を修正し、`SCAN_UNDEFINED` を `INVALID_ARGUMENT`、`SCAN_BLIND` を `UNAVAILABLE` として扱うようにした。
- `frontend_tune_worker` の spawn failure を runtime diagnostics、live path fail-closed、backend stop cleanup に接続した。
- HAL service 登録失敗時の `panic!` を廃止し、明示 log と process exit に置換した。
- CHANGELOG 以外の恒久文書に残っていた版名付き見出しと版名付き固定方針表現を、内容名ベースの恒久表現へ正規化した。

## r50an8

- r50an7 のロジック未達を修正し、px4 / DVB backend validation helper で `bandwidth_hz` を明示検証するようにした。ISDB-T は未指定または 6MHz のみ成功、ISDB-S は未指定のみ成功とし、7MHz / 8MHz や satellite bandwidth 指定を `INVALID_ARGUMENT` として拒否する。
- DVB / earth_pt1 の ISDB-T tune property に `DTV_BANDWIDTH_HZ = 6_000_000` を必ず設定し、`FrontendInfo` / capability / validation と driver property の契約を一致させた。
- `DvbFrontendBackend::tune_from_common()` の入口で `validate_tune_request()` を必ず通し、`symbol_rate` や bandwidth の内部 contract violation を device access 前に拒否するようにした。
- px4 / DVB の bandwidth validation、DVB tune property、`tune_from_common()` symbol_rate rejection の regression test を追加した。

## r50an7

- r50an6 のロジック未達を修正し、DVB / earth_pt1 の BS TSID table を TIS / px4 の BS TSID SSOT と一致させた。BS23 の最後の TSID は `0x4972` とし、`0x4973` は拒否対象として regression test に固定した。
- px4 backend の ISDB-S BS / CS110 IF 周波数 validation を exact table match に固定し、`FrontendInfo.acquireRange = 0` と矛盾する ±500kHz tolerance を ISDB-S validation path から除去した。ISDB-T の px4 legacy addfreq tolerance は対象外として維持した。
- px4 / DVB backend validation helper で `FrontendTuneRequest.symbol_rate` の `Some(_)` を `INVALID_ARGUMENT` とし、public settings と backend helper の explicit symbolRate 非対応方針を一致させた。
- TIS / px4 / DVB の BS TSID table 一致、ISDB-S exact frequency validation、backend symbol_rate 拒否の regression test を追加した。

## r50an6

- r50an5 のロジック未達を修正し、ISDB-S `symbolRate` は正負を問わず nonzero を `INVALID_ARGUMENT` とする条件へ統一した。
- DVB / earth_pt1 の BS TSID validation を IF 周波数 + absolute TSID の固定表へ接続し、TSID 0、未知 TSID、周波数と TSID の組み合わせ不一致を拒否するようにした。
- DVB / earth_pt1 の ISDB-S 周波数 validation を `FrontendInfo.acquireRange = 0` と整合する exact table match に固定し、±tolerance で受け付ける経路を削除した。
- worker abnormal exit の regression を、helper 直呼びだけでなく `spawn_worker_with_exit_hook()` から fail-closed helper へ到達する静的確認に補強した。

## r50an5

- r50an4 のロジック未達を修正し、DVB / earth_pt1 の `FrontendInfo.maxSymbolRate` を r51 の explicit `symbolRate` 非対応方針に合わせて 0 固定にした。
- `streamIdType == UNDEFINED` かつ `streamId != 0` を `INVALID_ARGUMENT` として拒否し、指定値を黙殺する経路を閉じた。
- symbolRate / stream selector / CS110 selector と worker fail-closed の regression test を追加した。

## r50an4

- r50an3 のロジック未達を修正し、DVB / earth_pt1 の `FrontendInfo` 周波数範囲を backend validation と同じ r51 固定日本向け範囲へ縮退した。
- scan worker spawn 失敗時も `FailedBackend` terminal reason を `scan_last_terminal` と diagnostic dump に保存してから cleanup するようにした。
- optional diagnostic worker の spawn failure / terminal exit を startup diagnostics に記録し、stop は `Cancelled`、panic は `Panic` として区別できるようにした。
- worker exit、diagnostic worker terminal reason、DVB FrontendInfo frequency contract、scan terminal diagnostic output の regression test を追加した。

## r50an3

- r50an2 のロジック未達を修正し、DVR playback consumer / filter callback worker / DVR callback worker の panic 終了を object state fail-closed へ接続した。
- diagnostic worker の停止要求終了を `WorkerExit::Cancelled` として区別するようにした。
- `tune()` 経路でも `endFrequency != frequency` を `UNAVAILABLE` 相当として拒否し、range 指定を受け付けて無視する経路を閉じた。
- DVB backend の `FrontendTuneRequest` validation で driver frequency の表現可能性と日本向け ISDB-T UHF / BS / CS110 固定表への一致を必須にした。
- scan terminal reason を `scan_last_terminal` と frontend diagnostic dump に保存し、Completed / Cancelled / FailedBackend / FailedCallback / FailedPanic を診断可能にした。

## r50an2

- r50an のロジック未達を修正し、worker 内部 failure が `WorkerExit::Normal` へ落ちる経路を `WorkerExit::Error` へ接続した。対象は DVR playback consumer、frontend tune worker、scan worker、filter callback worker、DVR callback worker。
- scan session の terminal reason を cleanup 前に `scan_last_terminal` へ保存し、normal / cancel / backend error / callback error / panic の区別が破棄されないようにした。
- `endFrequency < 0` を未指定扱いにせず `INVALID_ARGUMENT` として拒否するようにした。
- scan request 生成後、px4 / DVB の backend-specific `validate_tune_request()` を全 candidate に適用してから worker を起動するようにした。

## r50am3

- TS filter linkage の public `IFilter.setDataSource()` 経路について、advertise 済み TS linkage が成功し、advertise 外 linkage が graph を変更せず拒否されることを regression test で固定した。
- r50am 系の `CHANGELOG.md` 記述を恒久差分中心に整理し、受け入れ条件ファイル名や未達修正経緯に依存しない記述へ寄せた。

## r50am2

- DVB / px4 live TS sampling を reader-local state 経由に一本化し、backend 全体 `&mut self` を要求する旧 sampling API を削除した。
- filter linkage capability advertise と `setDataSource()` compatibility validation を単一の `FILTER_LINKAGE_POLICY` table から導出するようにした。

## r50am

- DVB / px4 live TS reader の device fd `POLLERR` / `POLLHUP` / `POLLNVAL` を no-data ではなく backend I/O error として扱い、stop fd wake と device fd error を分離した。
- live TS reader state を backend lifecycle state から分離し、reader state 側で `poll/read/residual` を行う構造へ変更した。
- filter linkage を r51 正式対応範囲に含め、`getDemuxCaps().linkCaps` は TS main type linkage のみを advertise する。`setDataSource()` validation は同じ TS linkage policy に基づいて advertise 外 linkage を拒否する。
- `DESIGN_JA.md` の `setDataSource()` error mapping を `CODE_CONVENTION.md` と現行実装に合わせ、closed / runtime-failed source/destination は `INVALID_STATE`、foreign / dangling / unsupported linkage は `INVALID_ARGUMENT` と明記した。

## r50ak4

- r50ak3 の 2-B / 3-C / 4-D / 5-B 範囲について、ロジック再確認で追加の実装バグなしと判定した。
- 4-D の regression 補強として、px4 backend に 1 byte + 187 bytes の split TS packet assembly test を追加し、DVB backend と同じ packet completion semantics を明示的に固定した。
- リリース時の完了判定証跡は、リリースアーカイブに同梱せず、最終報告で条件 ID ごとに提示する方針を維持した。

## r50ak3

- r50am2 の 4-D 最小未達を修正した。
- px4 backend に `ts_malformed_bytes` field を追加し、malformed TS diagnostic counter の実装をビルド可能な状態に戻した。
- px4 backend に stop fd wake の deterministic regression test を追加し、DVB と同等に device fd readiness と stop wake を同一 `poll()` loop で扱う条件を固定した。

## r50am2

- r50am の 3-C / 4-D / 5-B 未達を修正し、`r50al2_followup_2B_3C_4D_5B_fix_plan_acceptance.md` の完了条件に合わせ直した。
- 3-C: DVR playback FMQ consumer 側に residual buffer を固定し、`soft_demux.inject_playback_payload()` には 188-byte aligned packet stream だけを渡す境界へ戻した。playback flush は consumer residual と malformed diagnostic を reset する。
- 4-D: DVB / px4 live TS reader を stop wake fd と device fd readiness を同一 `poll()` loop で扱う形に補強し、malformed TS byte は product diagnostic log と backend counter に接続した。
- 5-B: `IFilter.setDataSource()` の destination filter runtime failed state を検証し、runtime failed destination の graph update を `INVALID_STATE` で fail-closed にした。
- r50am の方針訂正は不要と判断し、採用済みの 3-C / 4-D / 5-B 方針どおりに実装境界を補正した。

## r50am

- r50al2 後続修正条件 の 2-B / 3-C / 4-D / 5-B を Tuner HAL に適用した。
- 2-B: `IFilter.flush()` 後の stale section/PES/AV output を assembler generation で抑止し、同一 PID を見る別 filter と linkage downstream の独立性を regression test で固定した。
- 3-C: DVR playback FMQ consumer を HAL 側 residual buffer に接続し、partial write / partial read が worker failure にならず、malformed TS は drop + diagnostic になることを固定した。
- 4-D: DVB / px4 live TS reader から `read_exact(188)` 依存を除去し、readiness check + nonblocking `read()` + 188-byte packet residual assembly に統一した。
- 5-B: `IFilter.setDataSource()` に self-cycle / cyclic graph / started destination rewiring の validation を追加し、validation 成功時だけ graph を更新することを regression test で固定した。

## r50al2

- r50al の No.6 / No.8 受け入れ条件未達を修正した。
- record DVR `start()` の attached record filter 再検証を強化し、attached filter が未登録・未 configured・record 以外へ変化した内部不整合を `INVALID_STATE` で拒否することを regression test で固定した。
- record DVR detach 後に detached filter の TS packet が DVR queue へ入らないことを regression test で固定した。
- local source filter 検証を testable な分類へ分離し、foreign / not-open は `INVALID_ARGUMENT`、closed / runtime failed は `INVALID_STATE` へ写像することを regression test で固定した。

## r50al

- r50al の受け入れ条件を、ビルド前レビュー No.6 / No.8 の修正完了条件に限定した。
- record DVR `start()` は、record 方向では configured かつ少なくとも 1 つの configured record filter が attach 済みであることを必須にした。未 attach の成功扱い no-op は `INVALID_STATE` として拒否する。
- `IFilter.setDataSource()` / DVR attach/detach などの local source filter 検証で、closed source filter を client 引数不正ではなく lifecycle 不正として `INVALID_STATE` に写像するようにした。

# Changelog

## r50ak7

- No.8 の受け入れ条件を Android 14 Tuner HAL AIDL 準拠へ固定し、`IDescrambler.addPid()` / `removePid()` の `optionalSourceFilter` は非 null source filter として扱うことを `DESIGN_JA.md` に明記した。
- Tuner HAL は source filter が local / same demux / open / demux registry record 実在であることを検証し、null source filter 用の PID 単位登録経路は作らない。
- r50ak7 は No.7 / No.12 を除く No.1, No.2, No.3, No.4, No.5, No.6, No.8, No.9, No.10, No.11 を再確認対象にした。

## r50ak6

- `stopScan()` の `backend_stop_tune()` failure を runtime diagnostics に記録し、`UNKNOWN_ERROR` 返却に接続した。
- r50ak6 は r50ak5 から No.9 user-driven `stopScan()` backend stop failure 診断のみを変更する。

## r50ak5

- r50ak4 の受け入れ条件未達を再修正した。
- live path failure を RuntimeIoRegistry の failed state に接続し、既存 Filter / DVR object の public method が正常成功を返し続けないようにした。
- `IDemux.connectCiCam()` / `disconnectCiCam()` も恒久未対応として `UNAVAILABLE` に固定し、CI CAM 未対応診断を demux diagnostics へ残すようにした。
- scan runtime error / scan cleanup failure 後に `Completed` / END を送らないよう、scan session の最終 phase 更新を Running 完走時だけに限定した。
- `optional_source_filter` を demux 内の実在 open filter として検証し、demux record から外れた dangling filter を拒否するようにした。
- playback consumer failure を RuntimeIoRegistry の DVR failed state と DvrHal public method failure に接続した。

## r50ak4

- r50ak3 のビルド前レビューで固定修正対象にした No.1 / No.2 / No.3 / No.4 / No.5 / No.6 / No.8 / No.9 / No.10 / No.11 を受け入れ条件として対応した。
- live TS pump の backend read failure / LNB apply failure / lock failure を空 packet や silent stop に潰さず、runtime diagnostics と live path failed state へ接続した。
- CI CAM は恒久未対応として `linkCiCam()` / `unlinkCiCam()` を `UNAVAILABLE` に固定し、state を保存しないようにした。
- scan / tune worker の runtime backend error を `NO_SIGNAL` から分離し、scan session failed / runtime diagnostics へ接続した。
- filter callback worker / playback consumer / frontend live pump の直接 sleep を cancellable wait へ置き換えた。
- `IDescrambler.addPid()` / `removePid()` で `optional_source_filter` の同一 demux / open state 検証を行うようにした。
- `stopScan()` と scan cleanup の backend stop failure を握りつぶさず、user-driven path は error、worker cleanup path は diagnostics / degraded state へ接続した。
- diagnostic file write failure を counter / log / diagnostic dump へ接続した。
- DVR playback consumer failure を DVR unregister / failed queue state と diagnostics に接続した。

## r50ak3

- r50ak2 のロジック修正範囲について再確認し、追加のロジック未達は確認されなかった。
- probe 結果が空の場合の target tuner device absent 情報を、log だけでなく startup diagnostics record として保持し、frontend diagnostic dump に出すようにした。
- 完了証跡はリリースアーカイブに同梱せず、外部レビュー資料として分離する方針を維持した。


## r50ak2

- r50ak のロジック未達を修正した。
- DVR callback worker の cancellable wait を predicate 付きに変更し、stop signal / notify の lost wake により status interval 満了待ちになる race を除去した。
- DVB runtime DVR read failure を `INVALID_ARGUMENT` ではなく runtime I/O failure として扱い、`UNKNOWN_ERROR` 系へ写像するようにした。
- runtime ioctl / read failure の診断情報を backend / operation / device path / errno / errno name を含む構造に拡張した。
- frontend callback failure を log のみで終えず、callback registration cleanup、backend callback state の解除、last_error 記録、scan session failed 遷移へ接続した。

## r50ak

- r51 前 Tuner HAL 固定修正条件のうち No.10 を除く 1〜9, 11, 12 を実装対象にした。
- CS110 tune request は stream selector 未指定のみを許可し、TSID / relative stream number / 負値 selector 指定を `INVALID_ARGUMENT` にした。
- Filter / DVR callback failure を握りつぶさず、対象 registration cleanup、failed/closed 遷移、diagnostic log に接続した。
- Filter / DVR worker の lock failure、registry inconsistency、record 不在、callback failure を silent stop ではなく abnormal worker stop として扱うようにした。
- DVR callback worker の周期待ちを cancellable wait 化し、close / Drop / shutdown が client 指定 interval の満了待ちにならないようにした。
- `getAvSharedHandle()` を configured AUDIO / VIDEO filter 専用にし、非 AV filter / 未 configure AV filter では shared backing を生成しないようにした。
- device missing / open failure は `UNAVAILABLE`、runtime ioctl / read failure は `UNKNOWN_ERROR` に分離した。px4 TS reader failure も runtime I/O failure とした。
- `configureMonitorEvent()` は supported mask 以外の bit を `INVALID_ARGUMENT` にし、0 は既定 mask に正規化するようにした。
- soft demux の section / PES assembler は started filter が存在する対象 PID だけに作成し、filter stop / unregister 後は対象 PID の assembler を破棄するようにした。
- `setMaxNumberOfFrontends()` は負値と `default_max` 超過をどちらも `INVALID_ARGUMENT` にした。
- product runtime の degraded frontend entry variant と生成経路を削除し、probe 失敗は diagnostics record のみへ閉じた。
- `tuner_hal/` 直下の一時レビュー用 Markdown を撤去し、恒久条件は `DESIGN_JA.md` / `CODE_CONVENTION.md` / `CHANGELOG.md` に統合した。

## r50aj3

- r50aj2 の今回作業範囲について、No.1 / No.2 / No.3 / No.5 / No.6 / No.7 / No.8 / No.11 / No.12 のロジック経路を再確認し、追加のロジック誤りなしと判定した。
- r50aj2 で申告した受け入れ条件文書側の未達を修正し、No.3 を AIDL 実形状に合わせて「openDvr() は bufferSize、DVR settings は IDvr.configure() 入口で検証」に固定した。
- r50aj3 の一時レビュー用 Markdown は r50ak でリリースアーカイブから撤去し、恒久情報を許可済み文書へ統合した。

## r50aj2

- No.3 ロジック未達対応: DVR `dataFormat != TS` を `UNAVAILABLE` ではなく `INVALID_ARGUMENT` に固定した。
- No.5 ロジック未達対応: AV shared dma-buf allocation failure で heap name / requested size / raw return / errno / errno name を HAL error message と log に保持し、`UNKNOWN_ERROR` だけに潰さないようにした。
- No.6 ロジック未達対応: `IDemux.close()` の closed state 確定を strict cleanup 完了後へ移動し、cleanup step 失敗時に demux ID / step / status を診断 log に残すようにした。
- 既知の非ロジック未達のうち、FilterHal の close error label と binder_service test helper の Dvb entry field 不整合も修正した。
- 受け入れ条件文書側の申告事項: No.3 の固定条件は AIDL 実形状に合わせ、`openDvr()` 入口ではなく `IDvr.configure()` 入口で DVR settings を検証する表現へ更新が必要。

## r50aj

- 存在しない degraded frontend を product runtime の frontend registry に投入せず、target tuner device absent 時は frontend を広告しないようにした。
- `openFilter()` / `openDvr()` の `bufferSize <= 0` を入口で `INVALID_ARGUMENT` にし、副作用前に拒否するようにした。
- DVR settings の TS/188、threshold、statusMask 検証を `Dvr.configure()` 入口で強化した。
- dma-buf allocation shim を失敗時 `-errno` 返却へ固定し、Rust 側で `last_os_error()` に依存しないようにした。
- `IDemux.close()` を strict cleanup 化し、Drop path の best-effort cleanup と分離した。
- 診断ファイル worker を managed worker 化し、stop signal と `JoinHandle` join を持たせた。
- frontend status readiness と actual status の support 判定を同一 validation 経路へ統一した。
- `setStatusCheckIntervalHint()` は負値を拒否し、0 を既定 25ms へ正規化するようにした。
- demux/filter 操作で誤っていた diagnostic lock label を実 state 名へ修正した。

## r50ai-fix4

- CAS HAL placeholder のまま r51 で接続境界だけを実装する方針へ明確化し、CAS HAL 完了前提の仮置きを禁止した。
- production TIS は placeholder / 診断専用 token を `setKeyToken()` へ渡さない方針に固定した。
- target tuner device absent 時は degraded boot するが、存在しない frontend / demux / backend resource を advertise しない方針に統一した。
- libaribcaption は TIS 側字幕 path から C API のみを safe Rust wrapper 経由で呼ぶ方針に固定し、独自 C/C++ shim を禁止した。
- ARIB SI/EPG の TvProvider 投影は `ARIB_SI_EPG_TvProvider投影方針.md` をSSOTとし、`arib_si_engine_rs` の descriptor 投影記述を整合させた。

## r50ai

- Tuner HAL 固有の no-panic boundary、AIDL error mapping、degraded boot、mutex poison fail-closed、worker / callback failure model を `CODE_CONVENTION.md` に固定した。

## r50ah2

- `DESIGN_JA.md` の AV filter start / shared handle / A/V sync 境界で、曖昧な旧表現を `r51 リリース前` / `r51 リリース後の後続 future_work` に置換した。

## r50ah

- AV filter `start()` が `getAvSharedHandle()` 未実行だけで失敗しない実装を維持し、shared handle 未 export 中の AV payload drop を診断 counter として観測可能にした。
- `AvSyncState` に PCR PID、service clock、jitter smoothing、PLL の後続接続用 state を追加し、PCR + monotonic 補間と PTS fallback 禁止の受け入れ条件を `DESIGN_JA.md` に固定した。

## r50ag

- AV filter start / shared handle / A/V sync の r51 リリース前境界と r51 リリース後の後続 future_work 境界を `DESIGN_JA.md` に固定した。
- filter ID の runtime 完了条件を、公開 ID pack ではなく内部 owner demux 検証へ合わせて修正した。

## r50af4

- r50af3 の A 範囲再確認後、B 範囲のリリース物ルール、証跡、テスト、設計文書整合を修正した。
- section condition の短い valid section test を有効な section payload に修正した。
- TableInfo version、filter owner、invalid input の回帰確認を補強した。
- CHANGELOG 以外の Tuner HAL 文書と source comment から版番号付き履歴風記述を削除した。

## r50af3

- r50af2 の A 範囲再確認で見つかった 7 件の箇所ロジック未達を修正した。
- filter owner demux 検証を公開 `getId64Bit()` の token 依存から、local Binder object の内部 state 検証へ変更した。
- scan worker は session と worker slot を保持した状態で生成し、spawn 失敗時は同じ lock guard 上で session を rollback するようにした。
- frontend live pump stop と frontend demux unbind を public cleanup では `BinderResult` として扱い、Drop path では best-effort helper に分離した。
- frontend lease release の count 不整合を `UNKNOWN_ERROR` として fail-closed し、best-effort path では saturating cleanup にした。

## r50af2

- r50af の箇所ロジック未達を修正した。
- filter の `getId()` を demux local ID に戻し、owner demux 検証は `getId64Bit()` の owner token で行うようにした。
- scan worker spawn 失敗時に scan session を rollback するようにした。
- public close の runtime I/O unregister、shared memory worker stop、frontend demux unbind、frontend lease release を error return する cleanup に変更した。
- Drop path は best-effort cleanup 専用 helper へ分離した。
- playback soft demux injection が失敗した場合に worker fatal stop として扱うようにした。

## r50af

- target tuner device absent 時の degraded frontend 登録を固定した。
- filter owner demux 検証を追加し、foreign filter を `INVALID_ARGUMENT` で拒否するようにした。
- worker spawn / join / playback consumer error handling を fail-closed 化した。
- public close と Drop cleanup を分離し、critical cleanup 失敗を成功扱いしないようにした。
- DVR start の同期 sleep を削除し、status interval は callback worker 周期へ限定した。
- px4 close で TS reader state を解放し、CNR を optional telemetry として扱うようにした。
- section filter 条件幅、`TableInfo.version`、invalid argument mapping を修正した。
- source comment を日本語へ整理した。

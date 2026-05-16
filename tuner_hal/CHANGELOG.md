## r50dk
- r50dj に残っていた AV passthrough / AV source filter 境界の未固定を修正した。
- AV passthrough は本製品では恒久的に対応せず、`DemuxFilterAvSettings.isPassthrough=true` は r51 以降も configure 時点で `UNAVAILABLE` とする設計を `DESIGN_JA.md` と `開発規則.md` に固定した。
- ライブ AV filter は non-passthrough `MediaEvent` + shared memory 経路のみを正式対応とし、AV payload を通常 FMQ / EventFlag へ載せる経路、および AV filter を他 filter の source とする経路を禁止した。
- `setDataSource()` の source 側から `TsAudio` / `TsVideo` を除外し、AV filter を終端 filter として扱うようにした。destination としての AV filter は維持する。
- `FilterPayload::PesData` に PTS/DTS/stream_id metadata を保持し、`TsPes -> TsAudio/TsVideo` の linked 経路でも `MediaEvent` が PES 由来 timestamp を参照できるようにした。
- Android/Soong build、Rust 単体テスト実行、atest、VTS、CTS、実機確認はこの環境では未実施。静的差分確認のみ実施した。

## r50dj
- r50di に残っていた RECORD filter と TS raw source の責務混線を修正した。
- RECORD filter は DVR record buffer と TsRecordEvent の終端 filter とし、`setDataSource()` の source filter としては拒否する。
- `propagate_filter_output_with_origin_generation()` と root TS packet path で `TsRecord` を downstream TS packet source として扱わないようにした。`TsRaw` は引き続き downstream source として扱う。
- RECORD filter を source にした `setDataSource()` が `INVALID_ARGUMENT` 相当の `InvalidKind` で失敗し、接続状態を変更しない単体テストを追加した。
- Android/Soong build、Rust 単体テスト実行、atest、VTS、CTS、実機確認はこの環境では未実施。静的差分確認のみ実施した。

## r50di
- r50dh に残った 21/22 を修正した。
- `entry_frontend_max_symbol_rate_contract()` を復活させ、ISDB-T / ISDB-S の public frontend contract では explicit symbolRate を広告しないため、`maxSymbolRate=0` に固定した。
- RECORD filter の record metadata event は filter FMQ payload として扱わず、queue の使用量計算では 0 byte とする。`TsRecordEvent` 生成用の 188 byte TS packet 参照は `event_bytes()` 側に保持する。
- TS raw filter は従来どおり 188 byte TS packet を filter FMQ payload として扱う。RECORD filter と TS raw filter の出力先・queue 使用量計算を分離した。
- Android/Soong build、Rust 単体テスト実行、atest、VTS、CTS、実機確認はこの環境では未実施。静的差分確認のみ実施した。

## r50dh
- r50dg に残った 17/18/19/20 を修正した。
- `setDataSource()` は TS main type の `linkCaps` 広告を維持しつつ、source / destination subtype と PID が意味的に整合する組み合わせだけを受理する。PID 不一致や成立しない subtype pair は `INVALID_ARGUMENT` に倒す。
- linked TS path は raw TS filter 自身への 188 byte 配送と、下流 section / PES / AV / record 解析を分離し、下流解析は transport error / continuity duplicate / discontinuity の処理後に行う。
- `setTone(NONE)` と `setSatellitePosition(UNDEFINED)` は `setVoltage()` と同じく LNB registry 更新後に選択中 frontend へ適用し、失敗時は旧 state へ rollback する。
- ISDB-T 周波数契約 helper を common crate に集約し、DVB backend と binder capability が同じ C13〜UHF62 契約を参照するようにした。90 MHz は r51 契約外として拒否するテストに反転した。
- Android/Soong build、Rust 単体テスト実行、atest、VTS、CTS、実機確認はこの環境では未実施。静的差分確認のみ実施した。

## r50dg
- r50df の追加バグ 12/13/15/16 を修正した。
- RECORD filter は DVR record buffer と record index event 用に限定し、filter FMQ へ 188 byte TS packet を二重配送しないようにした。TS raw filter は従来どおり対象 PID の TS packet を filter FMQ に配送する。
- `linkCaps` が TS→TS を main type 粒度で広告する契約に合わせ、`setDataSource()` の source を `TsRaw` だけに限定せず、TS 系 source / destination 全体を受理し、配送時に destination 条件を再評価するようにした。
- `setDataSource()` は source filter の開始済み状態を要求せず、configured / demux / cycle / destination stopped の条件で接続できるようにした。配送は source と destination が実際に start 済みのときだけ発生する。
- `IDescrambler.addPid()` の source filter subtype 許可表は `TsAudio / TsVideo / TsPes / TsRecord` で統一済みであることを確認し、r50dg 差分では該当経路を維持した。
- Android/Soong build、Rust 単体テスト実行、atest、VTS、CTS、実機確認はこの環境では未実施。静的差分確認のみ実施した。

## r50df
- r50de のフェーズ5完了条件を再確認し、同一 demux 内の record DVR / playback DVR 2本目を `INVALID_STATE` に固定した。
- `numRecord=numPlayback=demux_count` 恒久仕様では、別 demux で demux 数ぶん同時 open 可能、同一 demux の同方向2本目は現在状態による容量超過として `INVALID_STATE` に倒す。
- フェーズ6〜7には進めていない。Android/Soong build、Rust 単体テスト実行、atest、VTS、CTS、実機確認はこの環境では未実施。静的差分確認のみ実施した。

## r50de
- r50dd のフェーズ1〜2完了条件を静的確認し、フェーズ3〜5として section repeat、DVR statusMask、StartId、descrambler source filter、record SC index mask、DVR capability 恒久仕様を修正した。
- section `isRepeat=false` は filter 全体の one-shot 停止ではなく、`table_id / table_id_extension / version / section_number` 単位の同一 section 重複抑止に変更した。
- DVR `statusMask=0` を全 status 購読扱いにせず、設定 bit が立った status だけを通知対象にした。
- filter `StartId` は filter id 流用を廃止し、初回 start は `StartId(0)`、再設定・再開始後は非0の delivery generation 由来 ID を送る。
- `IDescrambler.addPid()` は non-null source filter の configured、generation、PID、subtype を検査し、SECTION / raw TS source を復号対象 PID source として拒否する。
- record `scIndexMask` は SC / AVC / HEVC / VVC ごとに実際に event 化できる bit だけを許可し、未対応 bit は `INVALID_ARGUMENT` とする。
- `DemuxCapabilities.numRecord` / `numPlayback` を demux 数ぶん同時 open 可能な恒久製品仕様として `DESIGN_JA.md` に明記した。
- Android/Soong build、Rust 単体テスト実行、atest、VTS、CTS、実機確認はこの環境では未実施。静的差分確認のみ実施した。

## r50dd
- r50dc の Tuner HAL 修正計画のフェーズ1〜2として、non-null `setDataSource()` linkage と raw TS filter subtype を修正した。
- `setDataSource()` は source を `DemuxTsFilterType::TS` 由来の raw TS filter に固定し、destination を SECTION / PES / RECORD / AUDIO / VIDEO に限定した。下流 filter へ payload を丸流しせず、PID、section 条件、PES 条件、record 条件、AV 条件を再評価する。
- 上流 filter の configure / flush / stop / unregister により、下流 filter の接続、起動状態、queue、runtime、pending status を破棄するようにした。
- `DemuxTsFilterType::TS` を `TsRaw` として open 可能にし、`DemuxTsFilterSettings.Noinit` は raw TS filter subtype だけで受理する。対象 PID の188 byte TS packet は filter queue / FMQ 配送対象とする。
- Android/Soong build、Rust 単体テスト実行、atest、VTS、CTS、実機確認はこの環境では未実施。静的差分確認のみ実施した。

## r50cd2
- r50cd の未達1だけを修正し、`IDescrambler.removePid()` が `Tuner.VOID_KEYTOKEN` 相当の `setKeyToken([0x00])` 後でも登録済み PID を解除できるようにした。
- `removePid()` の現在鍵必須条件を外し、demux 束縛、世代、PID登録、入力元フィルタ世代の検証は維持した。
- 対応する Rust 単体テストを追加した。Android/Soong build、Rust unit test実行、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50cd
- Tuner HAL の r51 候補に残っていたロジックバグ全件の修正に着手し、能力値広告、選局入力検証、AV共有領域の寿命、スクランブル解除器登録表、実行時入出力登録表、入力元切替、境界リセット、LNB失敗写像、排他ロック異常の扱いを修正した。
- `releaseAvHandle()` は正の `avDataId` だけを受け付け、0を全解放として扱わない。`configureAvStreamType()` はAV共有領域の旧寿命を無効化する。
- 実行時登録表、スクランブル解除器登録表、鍵表、境界リセット、入力元切替の内部異常は成功、対象なし、通常の不正状態へ丸めず、閉鎖側失敗または `UNKNOWN_ERROR` へ倒す。
- DVB 地上波周波数検証、ISDB-T/S 能力値、`FrontendSettings` 検証、px4 CS110周波数分類、px4能力調査の副作用を修正した。
- Android/Soong build、Rust 単体テスト、atest、VTS、CTS、実機確認はこの環境では未実施。静的差分確認のみ実施した。

## r50cb
- WP-13対応として、Tuner HAL の `rust_test` module、直接試験 crate source、`#[cfg(test)]` source は、tv直下の作業メモではなく `tuner_hal/INTEGRATION.md` の r51 ビルド・試験確認ゲートを正とする。
- Tuner HAL 実装コードは変更していない。Android/Soong build、Rust 単体テスト、atest、VTS、CTS、実機確認は未実施。

## r50bv
- WP-07対応として、Tuner HAL の r51 範囲について frontend / demux / filter / DVR / AV 共有ハンドル / descrambler / LNB / VTS config の静的差分監査を実施した。
- 既存実装は r51 TS-only profile の契約に沿っており、FMQ/EventFlag は `libfmq` shim 経由、未対応 API は `UNAVAILABLE` または `INVALID_ARGUMENT` に倒す境界を維持している。
- Android.bp の `rust_test` module と `#[cfg(test)]` source が r51 期待値を保持していることを確認した。実装コードは変更していない。Android/Soong build、Rust 単体テスト、atest、VTS、CTS、実機確認は未実施。

## r50bm6
- r50bm5 の確認サマリが機械的検査中心だったため、人手観点で再確認し、コーディング規則文書に残っていた英語自然文を追加で日本語化した。
- 実装ロジックは変更していない。Android/Soong build、Rust 単体テスト、atest、VTS、CTS、実機確認は未実施。

## r50bm5
- r50bm4 の確認漏れ是正として、Tuner HAL の英語エラー文、テスト診断文、コメントを日本語化した。
- 実装ロジックは変更していない。Android/Soong build、Rust 単体テスト、atest、VTS、CTS、実機確認は未実施。

## r50bm3
- 承認済みスコープ拡大の仕様固定に合わせ、プロジェクト横断の到達点文書を更新した。Tuner HAL の実装コードと Tuner HAL 固有設計は変更していない。
- Android/Soong build、Rust 単体テスト、atest、VTS、CTS、実機確認は未実施。

## r50bm2
- リリース物規則違反の追加是正として、英語自然文コメント、英語主体のエラー文、旧版名を含む future_work ファイル名、フィルタ境界説明のルー語残件を修正した。
- 仕様 scope と実装 logic は変更せず、文書・コメント・エラーメッセージ表現の整理に限定した。

## r50bm
- リリース物規則違反の是正として、CHANGELOG の重複見出し・途中表題・降順崩れを整理した。
- CHANGELOG 以外に残っていた旧版名、作業番号、修正経緯、英語自然文コメントを現行仕様の日本語表現へ置換した。
- 仕様 scope と実装 logic は変更せず、文書・コメント・履歴整理のみに限定した。

## r50bl
- `px4_stream_selector_direct_slot_v5.patch` を適用し、px4 backend の BS `STREAM_ID` を TSID→relative slot 変換せず absolute TSID 値のまま legacy `slot` へ渡す方針に更新した。
- AOSP SDK default の `streamIdType=STREAM_ID` / `streamId=-1` は selector なしとして扱い、CS110 では selector 付き request を拒否する境界を固定した。
- px4 legacy chardev の二重 open を避けるため、ライブ TS reader は control fd の `try_clone()` で作成する方針に変更した。
- `DESIGN_JA.md`、`INTEGRATION.md`、`開発規則.md` に、px4 BS absolute TSID direct-slot は px4_drv `feat/android-ddk` 系のように BS `slot >= 8` reject が無効な driver を前提にすること、公開 develop 相当では使用不可であること、TSID→relative slot 変換表を互換 代替処理 として復活させないことを明記した。
- この環境では Android/Soong build、Rust 単体テスト、atest、VTS、CTS、実機確認は未実施。静的差分確認のみ実施した。

## r50bc
- `tuner_hal_multi2_error_api_independence_fixed_plan_acceptance_revised.md` の固定方針に従い、Tuner HAL descrambler の改善候補10/13だけを修正した。
- MULTI2 preparation error を `Multi2PrepareError::InvalidRoundsZero` へ具体化し、runtime path 用に 仮実装 variant なしの `Multi2RuntimeError` を導入した。
- `Multi2KeyMaterial::prepare()` は `Result<PreparedMulti2Key, Multi2PrepareError>` を返し、`rounds == 0` を preparation 時点で拒否する。
- `multi2_decrypt_payload()` / `multi2_encrypt_payload()` は `&PreparedMulti2Key` と `Result<(), Multi2RuntimeError>` を使い、復号/暗号 hot path に key schedule を戻さない。
- `descrambler/src/multi2.rs` と `descrambler/src/packet.rs` へ同一 crate 内で分離し、`lib.rs` は module 宣言と crate-level re-export 中心へ整理した。Android.bp / Soong module 名は変更していない。
- binder_service の invalid rounds expectation を `InvalidRoundsZero` に更新した。
- この環境では Android/Soong build、Rust unit test実行、atest、VTS、CTS、実機確認は未実施。静的 grep と構造確認のみ実施した。

## r50bb6
- `tuner_hal_descramble_improvements_1_2_3_5_plan_acceptance_revised_fixed2.md` の固定方針に従い、Tuner HAL descrambler の TEI / AFC=11 payload 0 / scrambled NULL PID / MULTI2 key preparation を修正した。
- `parse_ts_packet_header()` は `TSC=01` を即時 error にせず、TEI 判定前の header 情報を返す責務へ整理した。TEI は `TransportErrorRecord` として TSC 判定より前に record-only バイト単位で同一 へ逃がす。
- `AFC=11` かつ payload 0 byte は `InvalidAdaptationField` とし、平文 packet / scrambled-without-payload 扱いにしない。
- `NULL_PID + TSC=10/11` は `ScrambledNullPid` とし、平文 `NullPid` pass-through へ落とさず record-only バイト単位で同一 とする。
- `PreparedMulti2Key` と `Multi2KeyMaterial::prepare()` を追加し、`DescramblerKeySlot` 内部を prepared key 保持へ変更した。`multi2_decrypt_payload()` / test encrypt helper は `&PreparedMulti2Key` を受け取り、復号 hot path で key schedule を生成しない。
- 旧 raw-key infallible even/odd slot helpers は削除し、`try_with_even` / `try_with_odd` / `with_even_prepared` / `with_odd_prepared` に置換した。
- descrambler crate と binder_service に固定方針の必須テスト名を追加した。
- この環境では Android/Soong build、Rust unit test実行、atest、VTS、CTS、実機確認は未実施。静的 grep と brace balance のみ実施した。

## r50bb3
- r50bb2 の Tuner HAL descrambler 修正完了条件のうち、build / test 実行以外で残っていた文書・テストカバレッジ未達を修正した。
- `DESIGN_JA.md` の空 トークン / `Tuner.VOID_KEYTOKEN` / テスト専用 key registration の旧期待値を、r51 descrambler 固定方針に合わせて更新した。
- `DescramblerTokenOrigin::VtsOrUnitTest` を `UnitTestOnly` に改名し、片側 key 登録が Rust 単体テスト 専用であることを明確化した。
- `descrambler` crate に TSC/AFC 16 行 matrix test を追加し、AFC=00、TSC=01、scrambled adaptation-only、平文 adaptation-only、even/odd payload descramble の期待値を固定した。
- binder サービス test に non-TS-frame ingress helper を追加し、`InvalidPacketSize` / `BadSyncByte` が record-DVR raw TS に残らないことを delivery path 条件として固定した。
- Android/Soong build、Rust unit tests、atest、VTS、CTS、実機確認は未実施。

## r50bb2
- r50bb の Tuner HAL descrambler 修正完了条件のうち、ロジック未達だった VOID key removal 後の診断経路だけを修正した。
- `Tuner.VOID_KEYTOKEN` (`[0x00]`) による current key removal 後も、PID 登録済み descrambler を active snapshot に残すようにした。
- key slot 未設定 snapshot は対象 PID の scrambled packet で `NO_KEY` を記録し、`SCRAMBLED_WITHOUT_DESCRAMBLER` に落とさない。
- PID 登録維持、record-DVR raw TS への scrambled passthrough、既存の malformed/non-TS-frame 分岐は維持した。
- 回帰テスト `void_key_token_clears_key_only_and_keeps_pid_registration` に、VOID後の scrambled packet が `NO_KEY` へ落ち、`SCRAMBLED_WITHOUT_DESCRAMBLER` を増やさない確認を追加した。
- この環境では Android/Soong build、Rust unit test実行、VTS、実機確認は未実施。

## r50bb
- r51向け descrambler packet validation 計画を適用した。AFC=00 は不正、TSC=01 は AFC 検証後に不正、scrambled adaptation-only packet は `ScrambledWithoutPayload` と診断する。
- TS header 検証前の平文 packet fast-path bypass を削除した。
- 非TSフレームの破棄と、TSフレーム風の不正record-only配送を分離した。
- invalid packet size、bad sync byte、invalid AFC、invalid adaptation field、invalid TSC、scrambled-without-payload、malformed-packet-for-recording に対応する descrambler 診断情報を固定値として追加した。
- CAS bridge の本番経路 key registration では Odd / Even の両方の key material を必須にし、片側だけの key registration はテスト専用に維持した。
- Treated `[0x00]` as `Tuner.VOID_KEYTOKEN` current-key removal and kept empty トークン `[]` as invalid argument / bad トークン.
- 修正済み packet matrix、配送判断、CAS bridge key-pair 規則、VOID key トークン挙動に合わせて descrambler 回帰テストを更新した。
- Android/Soong build, Rust unit tests, atest, VTS, CTS, and real-device checks were not run in this environment.

## r50ba2
- r50ba に対して、リリース物整理のみを行った。
- `DESIGN_JA.md` の過去版名ベースの見出しと本文表現を、現行設計名・現行実装対象の表現へ置換した。
- Rust test module / test function 名に含まれていた過去版名を、意味ベースの名前へ改名した。
- Tuner HAL ロジック、VTS XML、future_work、TIS/rec 実装コードは変更していない。
- この環境では Android/Soong build、Rust unit test実行、VTS、実機確認は未実施。

## r50aq21
- r50aq20 に対して、Tuner HAL の frontend 異常系で `frontend_backend` ロックを保持したまま `mark_live_path_failed()` へ入る自己 deadlock だけをロジック修正した。
- ライブ pump の LNB apply / stream reader 生成失敗時は、backend ロック 区間内では error detail だけを生成し、ロックを抜けてから runtime failure 記録と `mark_live_path_failed()` を実行するようにした。
- scan ワーカー cleanup の `backend_stop_tune()` 失敗時も、backend ロックを抜けた後に scan phase 更新、runtime failure 記録、`mark_live_path_failed()`、scan end 通知を行うようにした。
- 既存の runtime failure 記録、bound demux fail-close、backend callback_failed marking は維持した。
- TIS、px4/DVB backend、generic scan、future_work、VTS XML、CAS HAL は変更しない。
- この環境では Android/Soong build、Rust unit test実行、VTS、実機確認は未実施。

## r50aq20
- r50aq19 に対して、Tuner HAL の `IDescrambler.addPid()` source filter generation 最終再検証不足だけをロジック修正した。
- `addPid()` は source filter identity 取得後、最終 PID 対応宣言 直前に source filter の `DemuxHandle` を再ロックし、同一 filter generation がまだ存在することを確認する。
- source filter が stop / flush / 再設定 / close 等で generation 変更または unregister 済みになっていた場合は、PID 対応宣言を行わず error を返す。
- `DescramblerRuntimeRegistry` の同一 demux generation / PID ownership atomic 対応宣言は維持し、対応宣言 時の ロック order は ライブ pump と同じ `demux_handle -> descrambler_registry -> descrambler_state` に揃えた。
- `removePid()` の ロック order 修正、nullable filter / PID-only future_work の仕様、TIS、px4/DVB backend、generic scan、VTS XML は変更しない。
- この環境では Android/Soong build、Rust unit test実行、VTS、実機確認は未実施。

## r50aq19
- r50aq18 に対して、Tuner HAL の `IDescrambler.addPid()` PID ownership 対応宣言 原子性不足だけをロジック修正した。
- `DescramblerRuntimeRegistry` に atomic 対応宣言 helper を追加し、他 descrambler の同一 demux generation / PID 所有確認と自 descrambler state への PID 登録を同一 registry critical section 内で行うようにした。
- `addPid()` は従来どおり state snapshot、demux generation 確認、source filter identity 確認を行った後、最終登録を atomic 対応宣言 helper に集約する。
- `removePid()` の ロック order 修正、nullable filter / PID-only future_work の仕様、TIS、px4/DVB backend、generic scan、VTS XML は変更しない。
- この環境では Android/Soong build、Rust unit test実行、VTS、実機確認は未実施。

## r50aq18
- r50aq17 に対して、Tuner HAL の `IDescrambler.removePid()` ロック order と `FilterHal::close_internal()` cleanup 完遂性だけをロジック修正した。
- `removePid()` は `descrambler_state` ロックを保持したまま demux registry / demux handle / source filter identity へ入らないよう、state snapshot → demux/filter 確認 → state 再取得・再検証の順に変更した。これにより ライブ pump の `demux_handle -> descrambler_state` ロック order と逆順になる path をなくした。
- `FilterHal::close_internal()` は途中 error で早期 return せず、コールバック ワーカー 停止、AV shared backing 破棄、runtime unregister、queue stop、AV queue stop、demux unregister をすべて試行し、最初の error 状態 だけを最後に返す形にした。
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
- `DemuxHal::close_internal()` で最後の参照の cleanup に入った後、`unbind_demux()`、demux handle ロック、registry ロック、ライブ id ロック、final record cleanup のいずれかが失敗しても後続 cleanup step を継続するようにした。
- cleanup 中に複数の error が発生した場合は最初の error 状態 を保持し、cleanup 試行後に返すようにした。
- 変更範囲は Tuner HAL demux lifecycle ロジックと CHANGELOG のみ。future_work、VTS XML、TIS、px4 mapping は変更しない。

## r50aq15
- r50aq14 に対して、Tuner HAL の demux lifecycle/refcount race のロジックのみを修正した。
- `openDemuxById()` が既存 demux record を再利用する際、close 中または ref_count 0 の record を再取得しないようにした。
- `DemuxHal::close_internal()` は record ロック 下で ref_count を減算し、減算後の値で最後の参照かを判定するようにした。stale read による cleanup skip を避ける。
- 最後の参照になった demux record には close-in-progress 状態を設定し、registry/ライブ id から削除されるまで新規 ラッパー が掴めないようにした。
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
- px4 backend の active streaming close / stopTune / retune 前 stop で `PTX_STOP_STREAMING` を明示実行し、stop ioctl 失敗を public 経路で握り潰さないようにした。best-effort 経路では runtime 診断に記録する。
- DVB backend の `close()` では `DTV_CLEAR` を必須化せず、`DTV_CLEAR` は明示 `stop_tune()` の責務であることを `DESIGN_JA.md` に固定した。
- TIS の `DESIGN_JA.md` に、CS110 tune request は Android builder default に依存せず stream selector none / `UNDEFINED` 相当を明示し、ONID / TSID / service_id を HAL frontend selector へ転用しない設計境界を追記した。
- この環境では `rustfmt`、Android/Soong build、Rust unit test実行、VTS、実機確認は未実施。

## r50aq12
- r50aq11 の Tuner HAL-only 修正計画 1/2/3/5/8/9 に沿って、DVB backend から BS TSID 表・日本向け scan 候補表相当の実装データと周波数+TSID semantic 照合を削除した。DVB backend は BS absolute TSID 必須、relative stream number 拒否、CS110 selector 拒否、frequency class 境界だけを検証する。
- HAL 単体テスト から TIS `ScanPlan.kt` の `include_str!` 文字列 parse と TIS 候補表・px4 backend-local mapping の一致確認を削除した。px4 側の TSID mapping は product scan SSOT ではなく legacy chardev ioctl 変換用の backend-local mapping として固定した。
- `TsPacketCompletionBuffer` の resync を単発 `0x47` 復帰から 188-byte 間隔の 3 packet 連続 sync 確認へ変更し、false sync / resync tail の regression test を追加した。
- `IDescrambler.setDemuxSource()` の二重設定を `UNAVAILABLE` ではなく `INVALID_STATE` に変更し、状態衝突として test に固定した。
- `ILnb.close()` を reset-on-close として固定し、close 時に LNB registry の voltage/tone/position を安全側へ戻して matching frontend へ反映する。cleanup 失敗は成功扱いしない。
- この環境では Android/Soong build、Rust unit test実行、VTS、実機確認は未実施。

## r50aq11
- r50aq10 の未達だった frontend 状態 / readiness 完了条件を対象に修正した。
- 状態 support SSOT を保守的に固定し、r51 では起動時列挙時点で取得根拠を固定できる 状態 type だけを `statusCaps` に出すようにした。DVB / earth_pt1 は `DEMOD_LOCK`、`RF_LOCK`、`SIGNAL_QUALITY`、satellite frontend の `LNB_VOLTAGE` に限定し、px4 は `DEMOD_LOCK` と satellite frontend の `LNB_VOLTAGE` に限定した。
- `FE_READ_SNR` / `FE_READ_SIGNAL_STRENGTH` / `PTX_GET_CNR` は read 時に失敗し得る optional telemetry として扱い、r51 では `SNR` / `SIGNAL_STRENGTH` を `statusCaps` に advertise しないことを `DESIGN_JA.md` と実装に固定した。
- `getFrontendStatusReadiness()` は caps外を `UNSUPPORTED` として同長返却し、caps内についても backend availability、tuning active、現在 telemetry の有無を見て `UNAVAILABLE` / `UNSTABLE` / `STABLE` を返すようにした。一律 `STABLE` を残さない。
- `getStatus()` は caps外を `INVALID_ARGUMENT` とし、caps内でも optional telemetry 欠落を 0 として成功返却しない。LNB voltage の未選択状態は仕様上の `NONE` として明示的に扱う。
- この環境では Android/Soong build、Rust unit test実行、VTS、実機確認は未実施。

## r50aq10
- r50aq9 に対して、Tuner HAL-only Issue 1 / 3 / 4 / 5 の固定計画に沿って、DESIGN_JA.md、実装、future_work を更新した。
- Issue 1: `IFilter.setDataSource(null)` は Android 14 AIDL/Rust nullable filter 境界の構造課題として、既存の `IDescrambler.addPid/removePid` null source filter 課題と同一 future_work ファイル内に集約した。r51 実装対象は non-null source linkage、demux default source、`configure()` 平文、error mapping の確認に限定した。
- Issue 3 / 4: frontend 状態 support 判定を `statusCaps`、`getStatus()`、`getFrontendStatusReadiness()` の共通 SSOT に寄せ、`getStatus()` は caps外 type を `INVALID_ARGUMENT`、readiness は caps外 type を `UNSUPPORTED` 要素返却に固定した。未測定 `SNR` / `SIGNAL_STRENGTH` / `SIGNAL_QUALITY` を 0 値で成功返却する経路を削除した。
- Issue 4: readiness 一律 `STABLE` を廃止し、backend unavailable は `UNAVAILABLE`、tune/probe 中は `UNSTABLE`、有効状態のみ `STABLE` にした。
- Issue 5: `bitWidthOfLengthField` は r51 TS-only profile として `0/12` のみ受理し、その他を `INVALID_ARGUMENT` に変更した。`SectionCondition::matches()` は正規化済み `length_field_bits` を受け取るようにし、隠れ 12bit 固定を除去した。
- この環境では Android/Soong build、Rust unit test実行、VTS、実機確認は未実施。

## r50aq9
- r50aq8 に対して、Issue 2 の別 descrambler 間同一 demux/generation/PID 排他の Result 契約を AOSP Result semantics に合わせて `INVALID_STATE` に固定した。
- 実装は既に `INVALID_STATE` を返していたため、`DESIGN_JA.md` の `INVALID_ARGUMENT` 表記を `INVALID_STATE` へ修正し、実装と設計文書の不一致を解消した。
- PID値・source filter オブジェクト 自体の不正ではなく、active descrambler registry 上の所有状態衝突として扱うことを明記した。
- 今回は設計文書の契約固定のみであり、テスト不足、Soong build、Rust 単体テスト、VTS、実機確認は未実施。

## r50aq8
- r50aq7 に対して、revised3照合で残った未達のうち、テスト不足以外の実装・設計文書未達だけを対象に修正した。問題点1の Android 14 AIDL/Rust backend 境界課題は引き続き実装対象外として別管理する。
- Issue 2: 同一 descrambler 内の同一PIDは置換 semantics、別 descrambler 間の同一 demux/generation/PID は排他という契約を `DESIGN_JA.md` に明記した。これにより、AOSP同一PID置換とHAL内部の二重復号防止を分離した。
- Issue 3: scan terminal state 保存は 平文付き helper に統一し、ワーカー normal/abnormal exit hook と spawn failure 経路で terminal state を active `scan_session` に残さない実装へ整理した。
- Issue 4: runtime path は outcome付き `SectionAssembler::push_payload_with_outcome()` のみを使う方針を維持し、単純 `push_payload()` を crate-internal API に下げて release runtime の 公開API境界から外した。
- 項目8: DVR cleanup step result を `Success` / `SafeNoOp` / `Failed` / `Unknown` / `SkippedDueToWorkerFailureContext` に分類し、best-effort の未確認stepを成功扱いしないようにした。`cleanup_complete=true` は全stepが成功または安全no-opと確認できた場合だけに限定した。
- `DESIGN_JA.md` の r50aq5 固有表記を r50aq8 / r50aq5以降の契約表現へ更新した。
- テスト不足として前回列挙された コールバック-level / failure-injection / peer lifecycle 追加テストは、今回の指示範囲外として未追加。Android/Soong build、Rust unit test実行、VTS、実機確認も未実施。

## r50aq7
- r50aq6 に対して、Tuner HAL-only 問題点6の LNB profile 不整合のみを対象に修正した。問題点1・2の r50aq6 修正は維持し、それ以外の実装範囲には触れていない。
- `DESIGN_JA.md` の LNB 固定 profile と判定表を更新し、px4_drv 系で LNB 15V 成功扱いにする対象を `px4video*` family のみに限定した。
- `pxmlt5video*` は対応デバイス仕様上 LNB 電源非対応、`pxmlt8video*` と `isdb6014video*` は仕様未確定として、r50aq7 では `NoPower` / `NONE` のみ成功に固定した。
- 実装の `LnbDeviceProfile` から `PxMltDevice15VOnly` を削除し、`pxmlt5video*` / `pxmlt8video*` / `isdb6014video*` を `NoPower` に割り当てるよう変更した。
- LNB profile detection と voltage policy の regression test を更新し、MLT/DTV02A 系が 15V を成功扱いしないことを固定した。
- この環境では Android/Soong build、Rust unit test実行、VTS、実機確認は未実施。

## r50aq6
- r50aq5 のビルド前レビューで残した Tuner HAL-only 問題点1・2のみを対象に修正した。LNB profile / DESIGN_JA.md の問題点6は今回スコープ外として未変更。
- 問題点1: descrambler key トークン の実 トークン を 8-byte opaque binary ID に変更し、`setKeyToken()` 入口の registry 解決前に 0 byte と 17 byte以上を拒否するようにした。長い診断用 トークン 名は成功経路から排除した。
- 問題点2: record filter の `TsRecord` コールバック event は configured TS/SC index mask に一致する observed index がある場合だけ生成し、index hit がない packet では event を抑制するようにした。
- それぞれ トークン 長・unknown トークン・旧診断 トークン 拒否、record event の抑制/TS index hit/SC index hit の regression test を追加した。
- この環境では Android/Soong build、Rust unit test実行、VTS、実機確認は未実施。

## r50aq5
- r50aq4 に対して、問題点1を Android 14 AIDL/Rust backend 境界の構造課題として実装対象外へ退避し、Tuner HAL 内で実装可能な Issue 2 / Issue 3 / Issue 4 / 項目8 の4件をこの順で補正した。
- Issue 2: `IDescrambler.addPid()` / `removePid()` の呼び出し順序・オブジェクト lifecycle 不整合を `INVALID_STATE` に寄せ、stale demux generation、未登録 PID、source mismatch を public Binder 経路の exact Result test で固定した。
- Issue 3: scan terminal 診断と active scan slot を分離し、terminal phase を記録後に `scan_session` を 平文 する helper を追加した。これにより completed/失敗/cancelled scan が `stopTune()` の active scan 判定に残らない。
- Issue 4: `SectionAssembler` に outcome 付き APIを追加し、oversized section drop / stale partial discard を同一 helper で filter-local 診断情報 / `pending_overflow` に接続した。コールバック ワーカー は既存 `pending_overflow` 経路で payload が空でも `DemuxFilterStatus::OVERFLOW` を送る。
- 項目8: `DvrHal` に `cleanup_complete` を追加し、`closed` gate と cleanup 完了状態を分離した。`close_internal()` / `close_internal_best_effort()` / `fail_dvr_worker()` は caller 種別付き共通 cleanup helper と step runner を使い、failure injection や loom で同じ完了判定を検証しやすい形にした。`WorkerFailure` 経路では コールバック ワーカー self-join を避け、未回収 ワーカー handle が残る場合は後続 close / Drop で retry 可能な未完了 cleanup として残す。
- `DESIGN_JA.md` に r50aq5 の error mapping、scan lifecycle、section overflow、DVR close cleanup の契約を追記した。
- Soong build は Android.bp 解析段階の既存構成 error で Rust compile 前に停止した。確認中に `rec/Android.bp` の path-outside-directory error と `tis/Android.bp` の missing privapp permission XML error を観測した。Rust unit test実行、VTS、実機確認はこのアーカイブ生成環境では未実施。

## r50aq4
- Issue 5 の最小修正案Aを適用し、`panic` ベースの `DemuxHandle::register_filter()` 補助関数を non-test の `soft_demux` 公開APIから削除した。
- Tuner HAL 単体テストの呼び出し箇所を `register_filter_result(...).expect("test setup should register filter")` に更新し、`panic` 境界をリリース実行時APIではなくテスト準備内に閉じた。
- Issue 6 の最小修正を適用し、AV MediaEvent builder の `debug_assert!(!secure_memory)` を実行時 fail-closed guard に置換した。未対応の secure-memory 状態が builder に到達した場合は、診断ログを残し、`panic` せず event を破棄する。
- secure-memory AV event-builder の fail-closed 経路を確認する回帰テストを追加した。
- Soong build, Rust 単体テスト execution, VTS, and real-device confirmation remain out of scope for this pre-build archive update.

## r50aq3
- Completed the r50aq2 follow-up fixes for items 6 and 8 only.
- 項目6: AV shared-memory errno mapping を `av_shared_file_error_result()` へ分離し、ENOMEM、ENOENT、EACCES、EIO、EINVAL、unknown errno mapping を確認する回帰テストを追加した。
- 項目8: DVR close cleanup は、最初の失敗後も全 cleanup 手順を試行するように変更した。最初に返すエラーは保持しつつ、コールバックワーカー状態の停止、queue状態の解除、queue backing の停止、親 demux からの DVR 登録解除を継続する。
- DVR close の成功時冪等二重 close と、queue stop 失敗時にも親 demux の DVR record を削除することを確認する回帰テストを追加した。
- Soong build, Rust 単体テスト execution, VTS, and real-device confirmation remain out of scope for this pre-build archive update.

## r50aq2
- r51ビルド前 Tuner HAL 修正として、項目1、2、5、6、8をこの順に適用した。
- 項目1: DVB / earth_pt1 ISDB-T validation と advertise する frontend 周波数契約は、px4 backend と r51明示選局契約に合わせ、UHF 13-62 に加えて日本の固定 CATV C13-C63 範囲も対象にした。
- 項目2: VTS設定生成では、playback DVR entry を出力する場合に DVR playback data flow も出力し、生成される AIDL V2 VTS XML が各 playback DVR を音声/映像 playback filter へ接続するようにした。
- 項目5: 管理対象の診断ワーカーは、周期的な stop-wake wait に `WorkerSignal::wait_timeout_or_stop()` を使うようにし、実行時の `sleep_with_stop()` polling 補助関数を削除した。
- 項目6: AV shared-memory allocation errno mapping は、ENOMEM を `OUT_OF_MEMORY`、device absence / permission errors を `UNAVAILABLE`、EINVAL / EIO / unknown runtime failures を `UNKNOWN_ERROR` として報告するようにした。
- 項目8: DVR close は、通常close経路とbest-effort close経路のどちらでも `closed.swap(true, Ordering::SeqCst)` により冪等にした。
- Soong build, Rust 単体テスト execution, VTS, and real-device confirmation remain out of scope for this pre-build archive update.

## r50aq
- `include_str!("tuner_hal.rs")` または `include_str!("main.rs")` で本番経路の source text を文字列照合するテストケースを削除した。
- static config / sepolicy / VTS XML / design-document / cross-module SSOT の整合だけを確認する `include_str!()` は維持した。
- 完了証跡として自己参照の source-string テストを禁止するプロジェクト規則と Tuner HAL 規則を追加した。ロジック契約は実際の API / helper / state / 診断 / queue / コールバック / ワーカーの挙動で検証する。
- No 本番経路 Tuner HAL runtime logic was intentionally changed. Soong build, Rust 単体テスト execution, VTS, and device confirmation remain out of scope for this static pre-build step.

## r50ap11
- Phase 10 / R13 のみ: Phase 11 へ進まず、filter condition / メタデータ引数検証を完了した。
- `DESIGN_JA.md` で PES `streamId` 契約を固定した。`0..=255` は明示的な stream_id 一致、`-1` だけが wildcard、それ以外の負値と `256+` は `INVALID_ARGUMENT` とする。
- Binder filter configuration は固定済み契約に従って PES `streamId` を正規化し、soft demux matching は `-1` だけを wildcard として扱う。`0` は wildcard ではない。
- Section `tableId`, PES `streamId`, and record TS/SC index validation are factored into dedicated helpers with regression tests for boundary values, unsupported bits, union-variant mismatch, and supported SC variants.
- Phase 11 and later are intentionally not advanced in this release. Soong build, Rust 単体テスト execution, VTS, and device confirmation remain out of scope for this static pre-build step.

## r50ap10
- Phase 9 / R05 のみ: Phase 10 へ進まず、SectionAssembler の PUSI / pointer field stale partial discard 診断情報を完了した。
- `DESIGN_JA.md` で PUSI pointer 境界方針を固定した。pointer bytes だけを正当な previous-section tail とし、不完全な古い partial section は新しい section body を解析する前に診断カウンター付きで破棄する。
- `SectionAssembler` は `stale_partial_section_discards()` を公開し、pointer bytes が前回の partial section を完成させない場合にこれを増やすようにした。stale state で pointer_field == 0 の場合も対象に含める。
- `DemuxHandle::stale_partial_section_discard_count()` aggregates the 診断カウンター across active section assemblers.
- pointer-zero stale partial discard、pointer-tail incomplete stale partial discard、demux単位の診断集計を確認する回帰テストを追加した。
- Phase 10 and later are intentionally not advanced in this release. Soong build, Rust 単体テスト execution, VTS, and device confirmation remain out of scope for this static pre-build step.

## r50ap9
- Phase 8 / R04 のみ: Phase 9 へ進まず、DVR playback input FMQ 方針を採用して固定した。
- `DESIGN_JA.md` で playback prefill / stop / flush の境界動作を固定した。start 前 prefill は保持し、stop/flush は dropped-byte 診断情報を残して playback input FMQ と packet residual を排出し、停止済み playback は入力を消費しない。
- Playback `PlaybackStatus` の周期コールバックは、record/output queue の `queued_bytes` ではなく、start-time 状態計算と一致する playback input FMQ の fill / unused-space source を使うようにした。
- playback consumer ワーカーは、従来の ad-hoc な `AtomicBool` + `Condvar` tuple ではなく、`ManagedWorker` / `WorkerSignal` の stop-wake-join lifecycle を使うようにした。
- Phase 9 and later are intentionally not advanced in this release. Soong build, Rust 単体テスト execution, VTS, and device confirmation remain out of scope for this static pre-build step.

## r50ap8
- Phase 4 / R06 のみ: Phase 5 へ進まず、ユーザー承認済みの clean-boundary 方針を採用した。
- `configure_filter_with_summary_result()` は古い `data_source_filter_id` を消去し、再設定で古い上流接続が残らないようにした。
- `unregister_filter()` は、削除された upstream に接続していた filter について、downstream queue / queued bytes / pending overflow / pending start event / delay runtime / filter-local assembler state を完全に消去するようにした。
- `FilterDelayHint::時間遅延指定` is fixed as queue-empty -> non-empty per-まとまり rearm rather than first-drain-only behavior.
- まとまり単位 time delay rearm、再設定時の旧 linkage / queued payload 解除、upstream unregister 時の downstream queue 解除を確認する回帰テストを追加した。
- Phase 5 and later are intentionally not advanced in this release. Soong build, Rust 単体テスト execution, VTS, and device confirmation remain out of scope for this static pre-build step.

## r50ap6
- `r51_tuner_hal_bugfix_execution_plan.md` の Phase 0〜7 を対象に、既存 Phase 0〜4 実装を再確認したうえで R09 / R17 / R14 の未達を補正した。
- Phase 4 / R06: `stop_filter()` が pending payload queue、queued bytes、delay runtime を 平文 し、stopped filter から delivery drain しないよう補正した。
- Phase 5 / R09: soft demux の continuity tracker、section assembler、PES assembler、assembly generation を frontend / playback origin 別に分離し、playback 起源 TS が frontend 起源 state を汚染しない regression test を追加した。
- Phase 6 / R17: descramble failure / scrambled pass-through は `push_ts_packet_record_only()` に限定され、section / PES / AV assembly に入らないことを record-only regression test で固定した。
- Phase 7 / R14: 同一 demux generation の同一 PID を複数 active descrambler に登録する経路を拒否し、設計資料と regression test を更新した。
- Phase 8 以降には進んでいない。Soong build、Rust unit test実行、VTS、実機確認は今回スコープ外。

## r50ap5
- `r51_tuner_hal_bugfix_execution_plan.md` の Phase 0〜3 のみを対象に、r50ap4 の未達だった ワーカー policy 接続、R12 ワーカー stop wake/join、R07 閉鎖済み guard、R08 rollback/fail-閉鎖済み 境界を補正した。
- Phase 0: `WorkerExit` の正式名を `Normal` / `StopRequested` / `RuntimeFailure` / `PanicOrJoinFailure` に寄せ、`ManagedWorker` が `WorkerSignal` と `JoinHandle` を保持する最小共通部品にした。既存 alias は互換目的に限定した。
- Phase 1 / R12: frontend tune / scan ワーカーを `WorkerSignal` + `ManagedWorker::stop_and_join()` に接続し、ロック wait の停止待ちを `AtomicBool + thread::sleep()` polling から Condvar wake へ変更した。
- Phase 2 / R07: 閉鎖済み `DemuxHandle` に対する `register_filter()` の dummy record 生成を廃止し、本番経路は `register_filter_result()` の `InvalidState` を返す方針に固定した。
- Phase 3 / R08: `setFrontendDataSource()` rollback で新frontend unbind失敗・旧frontend欠落・旧frontend bind失敗を fail-閉鎖済み に接続し、失敗rollback中に旧stream stateを無条件 reset しないようにした。
- Phase 4以降のR06以降には進んでいない。Soong build、Rust unit test実行、VTS、実機確認は今回スコープ外。

## r50ap4
- `r51_tuner_hal_bugfix_execution_plan.md` の Phase 0〜3 を対象に、ワーカー policy、R12、R07、R08 のビルド前静的修正を追加した。
- Phase 0: `WorkerSignal` の最小実装を追加し、長寿命 ワーカー の `JoinHandle` 保持、`Mutex` + `Condvar` 待機、stop → wake → join、`loom` テスト専用 方針を `DESIGN_JA.md` に正式固定した。
- Phase 1 / R12: `scan()` 内の重複 `stop_tune_worker()` を除去し、`stopTune()` が active scan を cancel しない既存境界を維持した。
- Phase 2 / R07: Filter / DVR の `ensure_open()` が親 Demux close、親側 unregister、owner demux mismatch を確認し、close後 child オブジェクト の public method が成功しないよう fail-閉鎖済み 化した。`openFilter()` / `openDvr()` 途中失敗時の runtime I/O unregister を追加した。
- Phase 3 / R08: `FrontendRuntime::bind_demux()` で ライブ pump 起動失敗時に partial binding をrollbackし、`setFrontendDataSource()` で new bind / old unbind / record更新 / stream reset の途中失敗時に rollback、rollback不能時は demux fail-閉鎖済み とする方針にした。

## r50ao5
- r50ao5 is intentionally scoped to the r50ao4 N2 follow-up only: it adds a public `IFilter.getAvSharedHandle()` path regression test for configured ライブ AV filters.
- 新規テストは、公開メソッドが共有AVメモリの総サイズを返し、`NativeHandle.ints == [0]` を export する一方、`slot_size` や `slot_count` を `NativeHandle.ints` から公開しないことを確認する。
- No 本番経路 logic changes are included in r50ao5; Android/Soong build, Rust compiler execution, VTS, and real-device playback remain unexecuted in this environment.

## r50ao4
- r50ao4 では r50ao3 の AV shared-slot internal-invariant handling を修正した。`ActiveSlotCollision` は active-slot map へ挿入する前に検出し、collision handling 中に以前の active entry を上書きしないようにした。
- `AvPayloadInternalError` に `SharedHandleExportedWithoutBacking` を追加し、mutex汚染、exported-handle/backing mismatch、slot registry inconsistency、mapping failure、counter failure、active-slot collision について、fail-closed 診断情報が正確な internal-error variant 名を含むようにした。
- r50ao4 replaces the r50ao3 self-referential source-string AV acceptance tests with helper-level decision tests and fixes the `AvPayloadAllocateError` / `AvPayloadDeliveryResult` pattern-match type mismatch in AV shared stats tests.
- The source-level acceptance evidence for r50ao4 uses unified diff checks and compile-blocker static checks; Android/Soong build, Rust compiler execution, VTS, and real-device playback remain unexecuted in this environment.

## r50ao3
- r50ao3 corrects the r50ao2 AV shared-handle release gate: ordinary AV delivery drops and internal invariant failures are separated into `AvPayloadDeliveryResult` and `AvPayloadInternalError` paths.
- AV payloads are no longer written to the standard filter FMQ or the AV auxiliary FMQ/EventFlag path. Successful AV delivery is shared memory + `MediaEvent` + コールバック `DATA_READY` only.
- exported-handle-without-backing、mutex/registry failure、avDataId collision を含む内部AV共有メモリ失敗は、通常の `OVERFLOW` として報告せず、影響を受けた filter を fail-close にする。
- `FilterHal::start()` no longer has the r50ao2 tuple destructuring / `is_media` compile blocker and does not emit immediate AV `DATA_READY`.
- `DESIGN_JA.md` に、AV payload delivery は shared memory + `MediaEvent` のみであり、`NativeHandle.ints == [0]` は framework に見せる共有ハンドル契約として維持することを明記した。

- r50ao2 では r50ao の AV共有ハンドル修正を強化した。AV payload は shared-slot delivery 前に標準 filter FMQ へ書かれなくなり、shared-handle-unexported / no-slot / invalid-payload 経路から通常 queue 経由で `TUNER_EVENT_DATA_READY` が起床しないようにした。
- r50ao2 では、AV shared backing mutex汚染、exported-handle-without-backing、shared-slot internal invariant failure を、通常の drop/overflow 診断情報としてではなく、filter ワーカーの fail-closed 条件として扱う。
- r50ao2 adds unified-diff-driven static acceptance tests for the AV FMQ/EventFlag path, internal invariant fail-閉鎖済み path, and `NativeHandle.ints == [0]` contract.

## r50ao
- r50ao の受け入れ条件は `r50an9_tuner_hal_av_shared_handle_fix_4_5_revised_no_or.md` である。ライブAVの `DATA_READY` は、payload が export 済み shared slot に置かれた後だけ送出される。
- 共有ハンドル未公開時の AV payload は、`DATA_READY` なしで `OVERFLOW` を送出し、`av_drop_unexported` を増やす。
- shared slot 枯渇時は active AV slot を追い出さず、`DATA_READY` なしで `OVERFLOW` を送出し、`av_overflow_no_slot` を増やす。
- 不正な AV payload size / shared-memory range failure は、`DATA_READY` なしで `OVERFLOW` を送出し、`av_invalid_payload` を増やす。
- `getAvSharedHandle()` は `NativeHandle.ints == [0]` を export する。`slot_size` と `slot_count` は HAL内部状態のままとし、`NativeHandle.ints` から公開しない。

## r50an9
- r50an8 のロジック未達を修正し、`SCAN_UNDEFINED` を `INVALID_ARGUMENT`、`SCAN_BLIND` を `UNAVAILABLE` として扱うようにした。
- `frontend_tune_worker` の spawn failure を runtime 診断情報、ライブ path fail-閉鎖済み、backend stop cleanup に接続した。
- HAL サービス登録失敗時の `panic!` を廃止し、明示 ログ と process exit に置換した。
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
- ワーカー abnormal exit の regression を、helper 直呼びだけでなく `spawn_worker_with_exit_hook()` から fail-閉鎖済み helper へ到達する静的確認に補強した。

## r50an5
- r50an4 のロジック未達を修正し、DVB / earth_pt1 の `FrontendInfo.maxSymbolRate` を r51 の explicit `symbolRate` 非対応方針に合わせて 0 固定にした。
- `streamIdType == UNDEFINED` かつ `streamId != 0` を `INVALID_ARGUMENT` として拒否し、指定値を黙殺する経路を閉じた。
- symbolRate / stream selector / CS110 selector と ワーカー fail-閉鎖済み の regression test を追加した。

## r50an4
- r50an3 のロジック未達を修正し、DVB / earth_pt1 の `FrontendInfo` 周波数範囲を backend validation と同じ r51 固定日本向け範囲へ縮退した。
- scan ワーカー spawn 失敗時も `FailedBackend` terminal reason を `scan_last_terminal` と 診断 dump に保存してから cleanup するようにした。
- optional 診断 ワーカー の spawn failure / terminal exit を startup 診断情報に記録し、stop は `Cancelled`、`panic` は `Panic` として区別できるようにした。
- ワーカー exit、診断 ワーカー terminal reason、DVB FrontendInfo frequency contract、scan terminal 診断 output の regression test を追加した。

## r50an3
- r50an2 のロジック未達を修正し、DVR playback consumer / filter コールバック ワーカー / DVR コールバック ワーカー の `panic` 終了を オブジェクト state fail-閉鎖済み へ接続した。
- 診断 ワーカー の停止要求終了を `WorkerExit::Cancelled` として区別するようにした。
- `tune()` 経路でも `endFrequency != frequency` を `UNAVAILABLE` 相当として拒否し、range 指定を受け付けて無視する経路を閉じた。
- DVB backend の `FrontendTuneRequest` validation で driver frequency の表現可能性と日本向け ISDB-T UHF / BS / CS110 固定表への一致を必須にした。
- scan terminal reason を `scan_last_terminal` と frontend 診断 dump に保存し、Completed / Cancelled / FailedBackend / FailedCallback / FailedPanic を診断可能にした。

## r50an2
- r50an のロジック未達を修正し、ワーカー 内部 failure が `WorkerExit::Normal` へ落ちる経路を `WorkerExit::Error` へ接続した。対象は DVR playback consumer、frontend tune ワーカー、scan ワーカー、filter コールバック ワーカー、DVR コールバック ワーカー。
- scan session の terminal reason を cleanup 前に `scan_last_terminal` へ保存し、normal / cancel / backend error / コールバック error / `panic` の区別が破棄されないようにした。
- `endFrequency < 0` を未指定扱いにせず `INVALID_ARGUMENT` として拒否するようにした。
- scan request 生成後、px4 / DVB の backend-specific `validate_tune_request()` を全 candidate に適用してから ワーカーを起動するようにした。

## r50am3
- TS filter linkage の public `IFilter.setDataSource()` 経路について、advertise 済み TS linkage が成功し、advertise 外 linkage が graph を変更せず拒否されることを regression test で固定した。
- r50am 系の `CHANGELOG.md` 記述を恒久差分中心に整理し、受け入れ条件ファイル名や未達修正経緯に依存しない記述へ寄せた。

## r50am2
- DVB / px4 ライブ TS sampling を reader-local state 経由に一本化し、backend 全体 `&mut self` を要求する旧 sampling API を削除した。
- filter linkage capability advertise と `setDataSource()` compatibility validation を単一の `FILTER_LINKAGE_POLICY` table から導出するようにした。


- r50am の 3-C / 4-D / 5-B 未達を修正し、`r50al2_followup_2B_3C_4D_5B_fix_plan_acceptance.md` の完了条件に合わせ直した。
- 3-C: DVR playback FMQ consumer 側に residual buffer を固定し、`soft_demux.inject_playback_payload()` には 188-byte aligned packet stream だけを渡す境界へ戻した。playback flush は consumer residual と malformed 診断 を reset する。
- 4-D: DVB / px4 ライブ TS reader を stop wake fd と device fd readiness を同一 `poll()` loop で扱う形に補強し、malformed TS byte は product 診断ログ と backend counter に接続した。
- 5-B: `IFilter.setDataSource()` の destination filter runtime 失敗 state を検証し、runtime 失敗 destination の graph update を `INVALID_STATE` で fail-閉鎖済み にした。
- r50am の方針訂正は不要と判断し、採用済みの 3-C / 4-D / 5-B 方針どおりに実装境界を補正した。

## r50am
- DVB / px4 ライブ TS reader の device fd `POLLERR` / `POLLHUP` / `POLLNVAL` を no-data ではなく backend I/O error として扱い、stop fd wake と device fd error を分離した。
- ライブ TS reader state を backend lifecycle state から分離し、reader state 側で `poll/read/residual` を行う構造へ変更した。
- filter linkage を r51 正式対応範囲に含め、`getDemuxCaps().linkCaps` は TS main type linkage のみを advertise する。`setDataSource()` validation は同じ TS linkage policy に基づいて advertise 外 linkage を拒否する。
- `DESIGN_JA.md` の `setDataSource()` error mapping を `CODE_CONVENTION.md` と現行実装に合わせ、閉鎖済み / runtime-失敗 source/destination は `INVALID_STATE`、foreign / dangling / unsupported linkage は `INVALID_ARGUMENT` と明記した。


- r50al2 後続修正条件 の 2-B / 3-C / 4-D / 5-B を Tuner HAL に適用した。
- 2-B: `IFilter.flush()` 後の stale section/PES/AV output を assembler generation で抑止し、同一 PID を見る別 filter と linkage 下流の独立性を regression test で固定した。
- 3-C: DVR playback FMQ consumer を HAL 側 residual buffer に接続し、partial write / partial read が ワーカー failure にならず、malformed TS は drop + 診断になることを固定した。
- 4-D: DVB / px4 ライブ TS reader から `read_exact(188)` 依存を除去し、readiness check + nonblocking `read()` + 188-byte packet residual assembly に統一した。
- 5-B: `IFilter.setDataSource()` に self-cycle / cyclic graph / started destination rewiring の validation を追加し、validation 成功時だけ graph を更新することを regression test で固定した。

## r50al2
- r50al の No.6 / No.8 受け入れ条件未達を修正した。
- record DVR `start()` の attached record filter 再検証を強化し、attached filter が未登録・未 configured・record 以外へ変化した内部不整合を `INVALID_STATE` で拒否することを regression test で固定した。
- record DVR detach 後に detached filter の TS packet が DVR queue へ入らないことを regression test で固定した。
- local source filter 検証を testable な分類へ分離し、foreign / not-open は `INVALID_ARGUMENT`、閉鎖済み / runtime 失敗 は `INVALID_STATE` へ写像することを regression test で固定した。

## r50al
- r50al の受け入れ条件を、ビルド前レビュー No.6 / No.8 の修正完了条件に限定した。
- record DVR `start()` は、record 方向では configured かつ少なくとも 1 つの configured record filter が attach 済みであることを必須にした。未 attach の成功扱い no-op は `INVALID_STATE` として拒否する。
- `IFilter.setDataSource()` / DVR attach/detach などの local source filter 検証で、閉鎖済み source filter を client 引数不正ではなく lifecycle 不正として `INVALID_STATE` に写像するようにした。

## r50ak7
- No.8 の受け入れ条件を Android 14 Tuner HAL AIDL 準拠へ固定し、`IDescrambler.addPid()` / `removePid()` の `optionalSourceFilter` は非 null source filter として扱うことを `DESIGN_JA.md` に明記した。
- Tuner HAL は source filter が local / same demux / open / demux registry record 実在であることを検証し、null source filter 用の PID 単位登録経路は作らない。
- r50ak7 は No.7 / No.12 を除く No.1, No.2, No.3, No.4, No.5, No.6, No.8, No.9, No.10, No.11 を再確認対象にした。

## r50ak6
- `stopScan()` の `backend_stop_tune()` failure を runtime 診断情報に記録し、`UNKNOWN_ERROR` 返却に接続した。
- r50ak6 は r50ak5 から No.9 user-driven `stopScan()` backend stop failure 診断のみを変更する。

## r50ak5
- r50ak4 の受け入れ条件未達を再修正した。
- ライブ path failure を RuntimeIoRegistry の 失敗 state に接続し、既存 Filter / DVR オブジェクト の public method が正常成功を返し続けないようにした。
- `IDemux.connectCiCam()` / `disconnectCiCam()` も恒久未対応として `UNAVAILABLE` に固定し、CI CAM 未対応診断を demux 診断情報へ残すようにした。
- scan runtime error / scan cleanup failure 後に `Completed` / END を送らないよう、scan session の最終 phase 更新を Running 完走時だけに限定した。
- `optional_source_filter` を demux 内の実在 open filter として検証し、demux record から外れた dangling filter を拒否するようにした。
- playback consumer failure を RuntimeIoRegistry の DVR 失敗 state と DvrHal public method failure に接続した。

## r50ak4
- r50ak3 の 2-B / 3-C / 4-D / 5-B 範囲について、ロジック再確認で追加の実装バグなしと判定した。
- 4-D の regression 補強として、px4 backend に 1 byte + 187 バイト列の split TS packet assembly test を追加し、DVB backend と同じ packet completion semantics を明示的に固定した。
- リリース時の完了判定証跡は、リリースアーカイブに同梱せず、最終報告で条件 ID ごとに提示する方針を維持した。


- r50ak3 のビルド前レビューで固定修正対象にした No.1 / No.2 / No.3 / No.4 / No.5 / No.6 / No.8 / No.9 / No.10 / No.11 を受け入れ条件として対応した。
- ライブ TS pump の backend read failure / LNB apply failure / ロック failure を空 packet や silent stop に潰さず、runtime 診断情報と ライブ path 失敗 state へ接続した。
- CI CAM は恒久未対応として `linkCiCam()` / `unlinkCiCam()` を `UNAVAILABLE` に固定し、state を保存しないようにした。
- scan / tune ワーカー の runtime backend error を `NO_SIGNAL` から分離し、scan session 失敗 / runtime 診断情報へ接続した。
- filter コールバック ワーカー / playback consumer / frontend ライブ pump の直接 sleep を cancellable wait へ置き換えた。
- `IDescrambler.addPid()` / `removePid()` で `optional_source_filter` の同一 demux / open state 検証を行うようにした。
- `stopScan()` と scan cleanup の backend stop failure を握りつぶさず、user-driven path は error、ワーカー cleanup path は 診断情報 / 劣化 state へ接続した。
- 診断 file write failure を counter / ログ / 診断 dump へ接続した。
- DVR playback consumer failure を DVR unregister / 失敗 queue state と 診断情報に接続した。

## r50ak3
- r50am2 の 4-D 最小未達を修正した。
- px4 backend に `ts_malformed_bytes` フィールドを追加し、malformed TS 診断カウンター の実装をビルド可能な状態に戻した。
- px4 backend に stop fd wake の deterministic regression test を追加し、DVB と同等に device fd readiness と stop wake を同一 `poll()` loop で扱う条件を固定した。


- r50ak2 のロジック修正範囲について再確認し、追加のロジック未達は確認されなかった。
- probe 結果が空の場合の target tuner device absent 情報を、ログ だけでなく startup 診断情報 record として保持し、frontend 診断 dump に出すようにした。
- 完了証跡はリリースアーカイブに同梱せず、外部レビュー資料として分離する方針を維持した。

## r50ak2
- r50ak のロジック未達を修正した。
- DVR コールバック ワーカー の cancellable wait を predicate 付きに変更し、stop signal / notify の lost wake により 状態 interval 満了待ちになる race を除去した。
- DVB runtime DVR read failure を `INVALID_ARGUMENT` ではなく runtime I/O failure として扱い、`UNKNOWN_ERROR` 系へ写像するようにした。
- runtime ioctl / read failure の診断情報を backend / operation / device path / errno / errno name を含む構造に拡張した。
- frontend コールバック failure を ログ のみで終えず、コールバック registration cleanup、backend コールバック state の解除、last_error 記録、scan session 失敗 遷移へ接続した。

## r50ak
- r51 前 Tuner HAL 固定修正条件のうち No.10 を除く 1〜9, 11, 12 を実装対象にした。
- CS110 tune request は stream selector 未指定のみを許可し、TSID / relative stream number / 負値 selector 指定を `INVALID_ARGUMENT` にした。
- Filter / DVR コールバック failure を握りつぶさず、対象 registration cleanup、失敗/閉鎖済み 遷移、診断ログ に接続した。
- Filter / DVR ワーカー の ロック failure、registry inconsistency、record 不在、コールバック failure を silent stop ではなく abnormal ワーカー stop として扱うようにした。
- DVR コールバック ワーカー の周期待ちを cancellable wait 化し、close / Drop / shutdown が client 指定 interval の満了待ちにならないようにした。
- `getAvSharedHandle()` を configured AUDIO / VIDEO filter 専用にし、非 AV filter / 未 configure AV filter では shared backing を生成しないようにした。
- device missing / open failure は `UNAVAILABLE`、runtime ioctl / read failure は `UNKNOWN_ERROR` に分離した。px4 TS reader failure も runtime I/O failure とした。
- `configureMonitorEvent()` は supported mask 以外の bit を `INVALID_ARGUMENT` にし、0 は既定 mask に正規化するようにした。
- soft demux の section / PES assembler は started filter が存在する対象 PID だけに作成し、filter stop / unregister 後は対象 PID の assembler を破棄するようにした。
- `setMaxNumberOfFrontends()` は負値と `default_max` 超過をどちらも `INVALID_ARGUMENT` にした。
- product runtime の 劣化 frontend entry variant と生成経路を削除し、probe 失敗は 診断情報 record のみへ閉じた。
- `tuner_hal/` 直下の一時レビュー用 Markdown を撤去し、恒久条件は `DESIGN_JA.md` / `CODE_CONVENTION.md` / `CHANGELOG.md` に統合した。

## r50aj3
- r50aj2 の今回作業範囲について、No.1 / No.2 / No.3 / No.5 / No.6 / No.7 / No.8 / No.11 / No.12 のロジック経路を再確認し、追加のロジック誤りなしと判定した。
- r50aj2 で申告した受け入れ条件文書側の未達を修正し、No.3 を AIDL 実形状に合わせて「openDvr() は bufferSize、DVR settings は IDvr.configure() 入口で検証」に固定した。
- r50aj3 の一時レビュー用 Markdown は r50ak でリリースアーカイブから撤去し、恒久情報を許可済み文書へ統合した。

## r50aj2
- No.3 ロジック未達対応: DVR `dataFormat != TS` を `UNAVAILABLE` ではなく `INVALID_ARGUMENT` に固定した。
- No.5 ロジック未達対応: AV shared dma-buf allocation failure で heap name / requested size / raw return / errno / errno name を HAL error message と ログ に保持し、`UNKNOWN_ERROR` だけに潰さないようにした。
- No.6 ロジック未達対応: `IDemux.close()` の 閉鎖済み state 確定を strict cleanup 完了後へ移動し、cleanup step 失敗時に demux ID / step / 状態 を診断ログ に残すようにした。
- 既知の非ロジック未達のうち、FilterHal の close error label と binder_service test helper の Dvb entry フィールド不整合も修正した。
- 受け入れ条件文書側の申告事項: No.3 の固定条件は AIDL 実形状に合わせ、`openDvr()` 入口ではなく `IDvr.configure()` 入口で DVR settings を検証する表現へ更新が必要。

## r50aj
- 存在しない 劣化 frontend を product runtime の frontend registry に投入せず、target tuner device absent 時は frontend を広告しないようにした。
- `openFilter()` / `openDvr()` の `bufferSize <= 0` を入口で `INVALID_ARGUMENT` にし、副作用前に拒否するようにした。
- DVR settings の TS/188、threshold、statusMask 検証を `Dvr.configure()` 入口で強化した。
- dma-buf allocation shim を失敗時 `-errno` 返却へ固定し、Rust 側で `last_os_error()` に依存しないようにした。
- `IDemux.close()` を strict cleanup 化し、Drop path の best-effort cleanup と分離した。
- 診断ファイル ワーカーを managed ワーカー 化し、stop signal と `JoinHandle` join を持たせた。
- frontend 状態 readiness と actual 状態 の support 判定を同一 validation 経路へ統一した。
- `setStatusCheckIntervalHint()` は負値を拒否し、0 を既定 25ms へ正規化するようにした。
- demux/filter 操作で誤っていた 診断 ロック label を実 state 名へ修正した。

## r50ai-fix4
- CAS HAL 仮実装 のまま r51 で接続境界だけを実装する方針へ明確化し、CAS HAL 完了前提の仮置きを禁止した。
- 本番TIS は 仮実装 / 診断専用 トークン を `setKeyToken()` へ渡さない方針に固定した。
- target tuner device absent 時は 劣化 boot するが、存在しない frontend / demux / backend resource を advertise しない方針に統一した。
- libaribcaption は TIS 側字幕 path から C API のみを 安全なRustラッパー 経由で呼ぶ方針に固定し、独自 C/C++ 薄層 を禁止した。
- ARIB SI/EPG の TvProvider 投影は `ARIB_SI_EPG_TvProvider投影方針.md` をSSOTとし、`arib_si_engine_rs` の descriptor 投影記述を整合させた。

## r50ai
- Tuner HAL 固有の no-`panic` boundary、AIDL error mapping、劣化 boot、mutex汚染 fail-閉鎖済み、ワーカー / コールバック failure モデル を `CODE_CONVENTION.md` に固定した。

## r50ah2
- `DESIGN_JA.md` の AV filter start / 共有ハンドル / A/V sync 境界で、曖昧な旧表現を `r51 リリース前` / `r51 リリース後の後続 future_work` に置換した。

## r50ah
- AV filter `start()` が `getAvSharedHandle()` 未実行だけで失敗しない実装を維持し、共有ハンドル 未 export 中の AV payload drop を診断カウンター として観測可能にした。
- `AvSyncState` に PCR PID、サービス clock、jitter smoothing、PLL の後続接続用 state を追加し、PCR + monotonic 補間と PTS代替同期 禁止の受け入れ条件を `DESIGN_JA.md` に固定した。

## r50ag
- AV filter start / 共有ハンドル / A/V sync の r51 リリース前境界と r51 リリース後の後続 future_work 境界を `DESIGN_JA.md` に固定した。
- filter ID の runtime 完了条件を、公開 ID pack ではなく内部 owner demux 検証へ合わせて修正した。

## r50af4
- r50af3 の A 範囲再確認後、B 範囲のリリース物ルール、証跡、テスト、設計文書整合を修正した。
- section condition の短い valid section test を有効な section payload に修正した。
- TableInfo version、filter owner、invalid input の回帰確認を補強した。
- CHANGELOG 以外の Tuner HAL 文書と source comment から版番号付き履歴風記述を削除した。

## r50af3
- r50af2 の A 範囲再確認で見つかった 7 件の箇所ロジック未達を修正した。
- filter owner demux 検証を公開 `getId64Bit()` の トークン 依存から、local Binder オブジェクト の内部 state 検証へ変更した。
- scan ワーカー は session と ワーカー slot を保持した状態で生成し、spawn 失敗時は同じ ロック guard 上で session を rollback するようにした。
- frontend ライブ pump stop と frontend demux unbind を public cleanup では `BinderResult` として扱い、Drop path では best-effort helper に分離した。
- frontend lease release の count 不整合を `UNKNOWN_ERROR` として fail-閉鎖済み し、best-effort path では saturating cleanup にした。

## r50af2
- r50af の箇所ロジック未達を修正した。
- filter の `getId()` を demux local ID に戻し、owner demux 検証は `getId64Bit()` の owner トークン で行うようにした。
- scan ワーカー spawn 失敗時に scan session を rollback するようにした。
- public close の runtime I/O unregister、shared memory ワーカー stop、frontend demux unbind、frontend lease release を error return する cleanup に変更した。
- Drop path は best-effort cleanup 専用 helper へ分離した。
- playback soft demux injection が失敗した場合に ワーカー fatal stop として扱うようにした。

## r50af
- target tuner device absent 時の 劣化 frontend 登録を固定した。
- filter owner demux 検証を追加し、foreign filter を `INVALID_ARGUMENT` で拒否するようにした。
- ワーカー spawn / join / playback consumer error handling を fail-閉鎖済み 化した。
- public close と Drop cleanup を分離し、critical cleanup 失敗を成功扱いしないようにした。
- DVR start の同期 sleep を削除し、状態 interval は コールバック ワーカー 周期へ限定した。
- px4 close で TS reader state を解放し、CNR を optional telemetry として扱うようにした。
- セクションフィルター 条件幅、`TableInfo.version`、invalid argument mapping を修正した。
- source comment を日本語へ整理した。

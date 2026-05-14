# r50ck
- r50cj 設計・実装不一致レポートの残件5に対応し、TIS の通常 DTO から旧表示用 flat field を削除し、free_CA_mode と series は nested JSON 構造を provider-data へ渡す境界にした。
- Program provider-data へ渡す audio / video metadata に schema required の codec を付与した。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

# r50ci
- r50ch の優先順位 1〜4 を再確認し、優先順位 5〜7 として components 構造、未対応視聴年齢制限保持、旧 indexed path 掃除を進めた。
- `AribService` に Rust bulk JSON 由来の `componentsJson` を保持し、event へ attach する経路を Kotlin 再構築優先から Rust 出力優先へ変更した。
- `ratings[]` / `diagnostics.publishDiagnostics[]` に未対応視聴年齢制限を保持する既存経路を維持し、descriptor diagnostic へ戻さない境界を確認した。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

# r50cg
- `NativeAribSiParser` の未使用 indexed path と private external 宣言を削除し、bulk snapshot のみを通常境界にした。
- Rust bulk event JSON の `descriptors` 構造から番組補足情報を読むようにし、旧 flat event field へ依存しない実装へ変更した。
- Program provider-data へ旧 `canonicalGenres` / `freeCaText` / `seriesName` 入力を渡さないようにした。
- Channel provider-data 生成要求に表示名を渡し、Rust側の nested ChannelProviderDataV1 形と整合させた。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

# r50cf
- Rust由来の旧 `canonicalGenres` event field をTISモデルから削除し、canonical genre は TIS の明示写像だけで決定する形へ統一した。
- Kotlin側で未対応視聴年齢制限を descriptor diagnostic JSON として手作りする経路と、`unsupportedDescriptorJson` 旧フィールドを削除した。
- Program provider-data へ渡す未対応視聴年齢制限は `parentalRatingDiagnostics` から Rust側の `ratings[]` / `publishDiagnostics[]` へ統合する経路に寄せた。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

# r50ce
- provider-data / 診断情報の r51 設計固定として、`diagnostics.currentProgram`、ChannelProviderDataV1、未対応視聴年齢制限の格納先を明記した。
- 旧 `canonicalGenres` event field と `nativeGetEventCount()` / `nativeGetEvent*` indexed JNI getter 群を使わない境界を固定し、Kotlin 側の未使用 indexed path と private external 宣言を削除した。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

# r50cc
- WP-14静的対応として、TIS product統合の正式ファイル `tis/config/product_integration.mk` を追加し、TIS本体、priv-app権限、ARIB SI JNI、ARIB字幕JNI、ライブ TV feature XML を一括で製品へ組み込む手順に統一した。
- `tis/config/product_integration.tis.example.mk` は正式ファイルを継承するだけの例に変更し、`tis/INTEGRATION.md` も字幕JNIを含む製品統合手順へ更新した。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

# r50cb
- WP-13対応として、unsupported codec provider-data テストデータを TIS test asset に追加し、Rust側 テストデータと同一内容で保持するようにした。
- `ProviderDataAssetsR51ContractTest` を追加し、provider-data v1 asset、descriptor 診断 element asset、unsupported codecメタデータ が r51再生可能表明にならないことを固定した。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

# r50ca
- WP-12対応として、HEVC `stream_type=0x24` と MPEG-4 AAC LATM `stream_type=0x11` を provider-data component メタデータ として認識しつつ、r51再生可能表明から分離した。
- 未対応 codecメタデータ には `r51PlaybackSupported=false`、`liveViewableClaim=false`、`diagnosticCode=UNSUPPORTED_R51_CODEC`、`parseStatus=UNSUPPORTED_R51` を残し、TIS の video/audio track 選択と再生開始対象には含めないことを test source で固定した。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

# r50bz
- WP-11対応として、r51では録画・予約を表明しない境界をTIS設計と統合手順に明記した。
- `onCreateRecordingSession()` が null を返す r51境界を instrumentationテスト で固定した。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

# r50by
- WP-10対応として、setup / boot EPG sync / user unlock drain が固定文字列ではなく `TvInputManager` から解決した自TISの `TvInputInfo.id` を使うよう修正した。
- setup activity 入口は サービス 側の `BIND_TV_INPUT` 契約とは分離し、システムTVアプリ から起動できるよう activity 側の `BIND_TV_INPUT` 要求を外した。scan 開始可否は inputId が自TISに属するかで判定する。
- inputId 解決不能時は boot EPG sync を pending のまま延期し、不正 inputId で scan / TvProvider 書き込みへ進まない境界をテストで固定した。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

# r50bx
- WP-09対応として、`deletionAuthoritative` が true の更新区間だけ obsolete Program delete に進み、false の区間では既存 Program を削除しない期待へ TIS test を更新した。
- Rust parser から出る `deletionAuthoritative` を `NativeAribSiParser` が受け取り、`ProgramPublishCoordinator` / `TvProviderWriter` の authoritative delete 境界へ渡す経路を確認した。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

# r50bw
- WP-08 の CAS 仮実装 境界確認として、診断-only ECM が `setKeyToken()` / `addPid()` に進まないテストを追加した。
- CAS plugin unavailable 時に TIS が fake / 仮トークン を成功扱いしないことをテストで固定した。

## r50bu

- WP-06対応として、r51対象の映像ESがない サービス または r51未対応映像codecのみの サービスを ライブ playback 開始前に視聴不可として扱い、TUNING状態のまま成功扱いにしないよう修正した。
- Tuner が利用不能または `currentTune` 未設定で `PlaybackPipeline.start()` に進めない場合、`notifyVideoUnavailable()` を返す経路を追加した。
- audio-only サービス / HEVC-only サービスが r51非スクランブル視聴成功扱いにならないことを TIS 受け入れテスト に追加した。

## r50bt
- WP-05 未達補修として、`NativeAribCaptionRenderer_nativeDecodePes` に混入していた重複 `let text = match ...` を除去し、字幕 JNI Rust module が構文上成立する形へ戻した。
- `onSelectTrack(TYPE_SUBTITLE, null)` が早期 return で拒否され、字幕track解除分岐に到達しない問題を修正した。
- PTS未指定 sentinel は `Long.MIN_VALUE` / `i64::MIN` のまま維持し、caption disabled 時およびsubtitle track未選択時に描画しない境界を再確認した。
- Android/Soong build、Rust 単体テスト、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

## r50bs
- WP-05 未達補修として、TIS 字幕 Rust JNI 境界の `libaribcaption` C API 定数名と値を公開 header に合わせて整理した。
- `ARIBCC_PTS_NOPTS` を `INT64_MIN` として扱い、PTS 未指定の字幕PESを `-1` ではなく libaribcaption の公開値で渡すよう修正した。
- `libmaleicacid_arib_caption_jni_test` を追加し、字幕 JNI 境界の定数値と PTS 未指定 sentinel を test crate で固定した。
- Android/Soong build、Rust 単体テスト、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

## r50br
- WP-05 対応として、PMT由来のARIB字幕ESを `TvTrackInfo.TYPE_SUBTITLE` として通知する経路を追加した。
- `onSetCaptionEnabled()`、subtitle track 選択、ARIB字幕PES filter、Rust JNI字幕境界、`libaribcaption` C API 呼び出し、overlay描画を接続した。
- 字幕trackメタデータを Program provider-data `components.subtitle[]` へ保存する入力経路を追加し、字幕本文・DRCS・BML状態は保存しない境界を維持した。
- caption disabled 時は字幕描画を消去し、字幕PESを自前ARIB SI/EPG文字列decoderへ渡さない制御とテスト観点を追加した。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

## r50bp
- WP-03 対応として、TvProvider Program 標準列へ canonical genre、scrambled、series id、episode display number、item count を投影する実装へ更新した。
- イベントグループを LONG_DESCRIPTION へ出す経路を除去し、provider-data JSON `relatedItems` 保存へ寄せた。
- free_CA_mode、canonical genre、series / episode / item count、視聴年齢制限、audio language、broadcast genre の provider-data 保存入力を `ProgramProviderDataV1` 前提に更新した。
- 旧仕様の「canonical genre を書かない」期待を持つテストを r51 SSOT に合わせて更新した。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

## r50bo
- WP-02 対応として、TIS 側の Program provider-data 期待値を `ProgramProviderDataV1` 出力に合わせて更新した。
- 新規 provider-data に旧 `programKeyB64` / `eventGroupText` / `freeCaText` / `seriesName` / `videoFormat` / `unsupportedDescriptorDiagnostics` が出ないことをテスト期待に反映した。
- Kotlin は provider-data 保存 JSON を直接構築せず、Rust JNI の build / 正規化 / extract API を経由する境界を維持した。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

## r50bn
- WP-01 対応として、TIS 設計文書から Program provider-data schema と descriptor 診断情報 schema の本文再定義を除去し、`arib_si_engine_rs` 側の SSOT と検証用JSONを参照する記述に統一した。
- TIS 側 テストデータ が Rust 側 テストデータと バイト単位で同一であり、schema 検証を通ることを確認した。
- TIS instrumentationテスト の 記述子診断情報 確認を旧 top-level 形の期待から、asset 上の `DescriptorDiagnosticV1` と `ProgramProviderDataV1.diagnostics.descriptorDiagnostics[]` の確認へ更新した。
- unsupported 視聴年齢制限 由来の 記述子診断情報 生成を `DescriptorDiagnosticV1` の element schema に合わせ、`remainingLength` / `rawPrefix` / `serviceKey` 直置きの旧形を出さないようにした。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

## r50bm6
- r50bm5 の確認サマリ不足を受け、リリース物規則違反の人手観点を追加確認した。
- 本モジュールの実装ロジックは変更していない。Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

## r50bm5
- r50bm4 の確認漏れ是正として、TIS の英語UI文、Deprecated文、テスト診断文、コメントを日本語化した。
- 実装ロジックは変更していない。Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

## r50bm4
- r50bm3 の仕様固定内容を再確認し、イベントグループを LONG_DESCRIPTION へ出す旧記述が残っていた箇所を provider-data JSON `relatedItems` 保存方針へ統一した。
- free_CA_mode の自然対応を `Programs.COLUMN_SCRAMBLED` と provider-data JSON へ反映する方針として明確化した。
- 実装コードは変更していない。Android/Soong build、Kotlin compile、instrumentationテスト、atest、VTS、CTS、実機確認は未実施。

## r50bm3
- 承認済みスコープ拡大の仕様固定として、canonical genre 明示写像、series id / episode 標準列投影、イベントグループ provider-data 保存、free_CA_mode / audio language / 視聴年齢制限 投影、字幕 track / libaribcaption 表示経路、r52 codec 固定表を TIS 設計に反映した。
- 実装コードは変更していない。Android/Soong build、Kotlin compile、instrumentationテスト、atest、VTS、CTS、実機確認は未実施。

## r50bm2
- リリース物規則違反の追加是正として、TIS 設計文書に残っていた英語自然文、境界見出し、逆圧説明を日本語の現行仕様文へ置換した。
- 仕様 scope と実装 logic は変更せず、文書・コメント・表現整理のみに限定した。

## r50bm
- リリース物規則違反の是正として、CHANGELOG の重複見出し・途中表題・降順崩れを整理した。
- CHANGELOG 以外に残っていた旧版名、作業番号、修正経緯、英語自然文コメントを現行仕様の日本語表現へ置換した。
- 仕様 scope と実装 logic は変更せず、文書・コメント・履歴整理のみに限定した。

## r50bk12
- 仕掛かり修正とは別に残っていた r51 設計契約の未達として、`Programs.COLUMN_CANONICAL_GENRE` を `ContentValues` に直接設定していた経路を削除した。
- boot EPG sync / background maintenance の開始判定に ライブセッション creation in progress を追加し、`TvInputService.onCreateSession()` 入口から `MaleicacidLiveSession` が active session 登録を終えるまでの tuner 資源 race を塞いだ。
- `TisR51FixedPlanAcceptanceTest` に ライブセッション 作成中は boot/background task を開始しないことを固定した。
- Android/Soong build、Kotlin compile、instrumentationテスト 実行、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bk11
- r50bk10 後に残った仕掛かり未達として、retry queue の サービス/global cap が DESIGN_JA.md の 32 / 512 と実装の 16 / 128 で不一致だった点を修正した。
- retry retention を enqueue 時と drain/test accessor 時の両方で適用し、期限切れ 再試行区間 を保持しないようにした。
- `ProgramPublishCoordinator` の retry backoff helper に残っていた重複式を除去し、completion test に per-サービス cap / global cap 定数 / retention drop の固定を追加した。
- Android/Soong build、Kotlin compile、instrumentationテスト 実行、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bk10
- r50bk8 completion 版で残っていた完了条件未達を仕掛かり範囲に限定して再固定した。
- SetupActivity の generation / purpose / published 件数判定を pure helper 化し、stale setup Completed、boot/background Completed、invalid inputId では成功終了しないことを test で固定した。
- Program publish の 必須問い合わせ failure、insert failure、廃止行削除 failure、retry failureClass、backoff / attempts / retention を テスト可能な境界 として固定した。
- publish 失敗時に 署名キャッシュ を commit せず、同一入力の retry が unchanged skip されないことを test で固定した。
- Android/Soong build、Kotlin compile、instrumentationテスト 実行、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bk8-rerelease
- r50bk8 TIS / arib_si_engine_rs 追加修正計画の7セクションに対応した。
- provider-data 新規 write を Rust/JNI bridge 経由の JSON v1 へ寄せ、Channel/Program の key 抽出・署名 生成・current-program overlap 診断情報 追記を native 側 API で扱う経路を追加した。
- EIT 更新区間 の `deletionAuthoritative` を TIS publish transaction へ伝播し、authoritative でない window では obsolete Program delete を実行しないようにした。
- Program publish retry に failureClass / attempt / nextAttemptAtMs / backoff / 上限 trim を追加した。ただし pending retry は引き続き process-local であり、永続化 store への移行は未実施。
- 本番経路 の snapshot 利用を `snapshotTransaction()` に寄せ、旧 snapshot API を deprecated として明示した。
- SetupActivity を `BIND_TV_INPUT` permission で保護し、自 TIS inputId 検証と scan generation 照合により外部起動・過去 Completed state による成功終了を抑止した。
- CAS descrambler PID type を AOSP `Descrambler.PID_TYPE_T` に修正した。
- AudioTrack 生成時に Android 14 の AttributionSource を可能な範囲で伝播する処理を追加した。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bk8
- r50bk7 のフェーズ1〜6静的再監査で、unsupported 視聴年齢制限 由来の `unsupportedDescriptorJson` を `EventModelMapper` が旧 `diagnosticCode` / `descriptorOffset` 形で新規生成しており、フェーズ6の「新規 provider-data write は schemaVersion=1 の canonical 記述子診断情報 shape だけにする」完了条件に未達であることを確認した。
- `unsupportedDescriptorJson` の生成を `schemaVersion=1` / `diagnostics[]` / `parseStatus=UnsupportedValue` / `tag=0x55` / `serviceKey` / `eventId` を持つ canonical shape に変更し、`TvProviderWriter` の migration-read 経路に依存しない新規 write へ固定した。
- r50bk8 は改訂2版 Markdown のフェーズ1〜6完了版として扱う。Android/Soong build、Kotlin compile、Rust 単体テスト、instrumentationテスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bk5
- r50bk4 のフェーズ3・4A・4B・4C静的再監査で、PlaybackPipeline の MediaCodec first-frame コールバック が main handler から playbackGeneration / surface / videoAvailableNotified を直接参照・更新し得る点を4C未達として確認した。
- first-frame コールバック は playback executor へ enqueue し、playbackGeneration / surface / first-frame state の確認と onVideoAvailable 通知を playback executor 上で実行するよう修正した。
- フェーズ5・6には進まず、r50bk5 は改訂2版 Markdown のフェーズ1〜4C完了版として扱う。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bk4
- r50bk3 のフェーズ3・4A・4B・4C静的再監査で、SetupActivity が user-unlock receiver 経由で boot EPG sync を開始し得る点と、ChannelScanController の SI collection 判定が ServiceListBuilder 経由で services / publishability を別snapshot から合成し得る点を未達として確認した。
- SetupActivity から user-unlock drain receiver 登録を削除し、setup activity 起動中は Direct Boot pending を表示するだけで boot EPG sync を開始しないよう固定した。
- ChannelScanController の 登録可能判定を、同一 snapshotTransaction 由来の serviceCounts に一本化し、serviceListBuilder.registrationReadySnapshot() の別snapshot 合成を 本番経路から除去した。
- フェーズ5・6には進まず、r50bk4 は改訂2版 Markdown のフェーズ1〜4C完了版として扱う。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bk2
- r50bk のフェーズ1・2静的再監査で、同一 event の start/end が 更新区間 外へ移動した場合に旧 Program row を stable key で発見できず duplicate insert し得る未達を確認した。
- Program upsert の既存 Program index を window 限定から サービス/channel 全体の stable programKey index へ変更し、ONID/TSID/SID/event identity が同じ Program は start/end 変更後も既存 row update になるよう修正した。
- フェーズ3以降には進まず、r50bk2 は改訂2版 Markdown のフェーズ1・2完了版として扱う。
- Android/Soong build、instrumentationテスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bk
- 改訂2版 Markdown のフェーズ1・2に従い、Program identity / 現在番組 / provider-data 署名 と TvProvider query failure / null cursor / Program upsert safety の実装を修正した。
- `programKey` を ONID/TSID/SID/event の安定キーに固定し、start/end と row id dependent 診断情報を 署名 対象から外した。
- TvProvider null cursor を failure として扱い、`existingChannels()` の空 代替処理 を 本番経路から除去し、サービス 単位 failure 時の 廃止行削除 を禁止した。
- Android/Soong build、instrumentationテスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bj3
- r50bj2 後に残っていた設計未固定事項として、malformed descriptor / malformed SI の fail-閉鎖済み 方針、malformed EIT event を 廃止行削除根拠にしない条件、SectionEvent / MediaEvent 入力上限、Direct Boot drain と ライブセッション 優先、TvProvider 必須問い合わせ の null cursor failure 扱いを固定した。
- `ARIB_SI_EPG_TvProvider投影方針.md` に、malformed descriptor / malformed EIT event 由来値を TvProvider 標準列へ正常投影しないことを明記した。
- 実装コードは変更していない。Android/Soong build、instrumentationテスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bj2
- r50bj の設計固定後に残っていた旧未固定記述を整理し、`ARIB_SI_EPG_TvProvider投影方針.md` の `internal_provider_data` schema/key/サイズ上限/LONG_DESCRIPTION 最大長を未固定扱いしないようにした。
- `onUnblockContent()` の start/end は stable identity ではなく current Program row 照合用の補助条件であることを `DESIGN_JA.md` / `INTEGRATION.md` に明記した。
- 実装コードは変更していない。Android/Soong build、instrumentationテスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bj
- 設計文書上で provider-data JSON v1、Rust serde SSOT、descriptor 診断情報 schema、transaction DTO、session/playback/scan executor 境界、SetupActivity 保護、retry/backoff を固定した。
- `programKey` を ONID/TSID/SID/event_id のみに固定し、start/end を stable identity から外した。
- 実装コードは変更していない。Android/Soong build、instrumentationテスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bi6
- Phase A の lifecycle / generation / executor / cancel / CAS state 未達を是正した。`TunerController` の section コールバック を controller serial executor + generation + filter トークン で隔離し、`onTune()` 解決失敗時も旧 ライブ state を先に破棄するようにした。
- `CasController` の mutation / ECM / EMM / close を専用 serial executor に閉じ、`ChannelScanManager.cancel()` が executor 外から controller / engine を close しないようにした。
- Phase B の別 transport サービス登録を PAT/PMT 由来の actual transport key に限定し、SDT-other / NIT-other / BAT だけで見えた サービスを現在 candidate の物理情報へ紐づけないようにした。
- `NativeAribSiParser` に 本番経路 bulk snapshot ラッパー を追加し、`AribSiEngine` の 公開snapshot path を bulk ラッパー 経由にした。
- Android/Soong build、instrumentationテスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bi4
- 「追加だけで対応できる」テスト補強として、malformed / truncated 相当の 視聴年齢制限 が `Programs.COLUMN_CONTENT_RATING` に投影されず internal provider data 診断へ残ることを 受け入れテスト で固定した。
- unsupported country / out-of-range レーティングが Programs のコンテンツレーティング column に出ず、診断 provider data に残ることを追加確認した。
- 旧 custom `ARIB_JP` / `AGE_*` レーティングが通常 product path の mapper 出力にならないことと、merged CAS state が Programs provider data に保持されることを追加確認した。
- Android/Soong build、instrumentationテスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bi3
- Programs CAS代替参照 の 現在診断情報の完備 判定を、理由文字列だけでなく Rust 由来の `pmtPidResolved` / `pmtParsed` / `caStateResolved` / `freeCaModeResolved` 明示 フィールド で判定するようにした。
- Rust 公開可否診断 から PMT/CAS/free_CA_mode 解決状態を JNI/TIS へ公開した。
- 現在番組 identity が変わった場合に一時 unblock key を明示破棄するようにし、同一番組・同一 レーティング 限定の unblock 条件をテストで固定した。
- 診断情報完備を示す明示フィールド と 現在番組 change unblock 破棄の 受け入れテストを追加した。
- Android/Soong build、instrumentationテスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bi2
- `onUnblockContent()` を 現在番組 の レーティング 一致かつ event/start/end identity 完備時だけ一時 unblock するように修正した。
- Programs CAS代替参照 の 現在診断情報の完備 判定を厳格化し、不完全 診断 + 既存 scrambled channel 状態の維持を `MERGED_CHANNEL_CAS_STATE` として明示した。
- AOSP ISDB レーティングの境界値、未対応値、UNRATED 代替の視聴許可解除 禁止、merged CAS state の 受け入れテストを追加した。
- Android/Soong build、instrumentationテスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bi
- Programs CAS 状態 代替処理 を追加し、current診断 欠落または不完全時に既存 channel `internal_provider_data` から `requiresCas` / `unsupportedCas` / `clearLivePlaybackSupported` / `channelRegistrationReady` / `epgPublishable` を復元して Programs 側へ保存するようにした。
- 視聴年齢制限の Programs 投影と Live session enforcement を r51 対応宣言対象として固定し、AOSP system-defined ISDB レーティング（`com.android.tv / ISDB / ISDB_4..20`）へ変更した。
- custom ARIB_JP / AGE_* レーティング projection を通常 product path から廃止した。
- parental block 時は `notifyContentBlocked()` + AV停止を主通知とし、parental block 理由で `notifyVideoUnavailable()` を呼ばないようにした。
- CAS 未完成 / scrambled unsupported の video unavailable reason を `VIDEO_UNAVAILABLE_REASON_CAS_UNKNOWN` に固定した。

## r50bh
- TIS の readiness 判定は、ローカル再計算ではなく Rust の `channelRegistrationReady` / `epgPublishable` / `clearLivePlaybackSupported` 診断情報を消費する形へ切り替えた。
- 登録済みまたはEPG公開可能なサービスについて、CAS未対応のスクランブルサービスを含めて Programs を公開する一方、スクランブルサービスは平文ライブ視聴成功対象から除外した。
- Stored `requiresCas`, `unsupportedCas`, `clearLivePlaybackSupported`, `channelRegistrationReady`, and `epgPublishable` in channel `internal_provider_data`.
- ライブセッションまたはスキャンが動作中の場合は起動時EPG同期を延期し、ライブセッション数が0へ戻った後、または妨げになっているスキャン/保守処理の完了後に、保留中の起動時EPG同期を再試行するようにした。
- Android/Soong build, instrumentationテスト, atest, VTS, CTS, and real-device checks were not run in this environment.

## r50bg2
- background channel maintenance の r51 必須実装条件を満たすため、boot EPG sync 成功後に background maintenance 起動を試行する接続を追加した。
- background channel maintenance は scan/maintenance 実行中または active ライブセッション 存在時には開始せず、skip 理由を `BackgroundChannelMaintenanceDiagnostics` に残すようにした。
- ライブセッション の active count を `ChannelScanManager` で管理し、maintenance 開始判定のテストを追加した。
- Android/Soong build、instrumentationテスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bg
- CAT-only EMM メタデータを CAS state / 診断情報に残し、PMT CA がない 非スクランブルサービスの playback は block しないまま、descrambler attach / key トークン / scrambled success へ接続しないようにした。
- empty EIT 更新区間 を JNI/TIS publish path に伝え、非空→空の EPG 更新でも obsolete `TvProvider.Programs` を削除できるようにした。
- boot EPG sync を既存 channel の p/f 最小更新に限定し、`background channel maintenance` を r51 スコープ内の必須実装として追加した。どちらも新規 channel insert は行わない。
- setup scan channel registration を サービス単位の登録可能 gate に変更し、global discovery complete 前でも 登録可能 な partial サービスは登録可能にした。
- `ProgramPublishResult.changed` に deleted を含め、delete-only update を変化として扱うようにした。
- `tis/DESIGN_JA.md` を r51 の boot/background maintenance と サービス単位の登録可能 方針に合わせて改訂した。
- Android/Soong build、instrumentationテスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bf2
- r50bf のロジック未達を是正し、decoder oversized sample drop 時に MediaCodec input buffer を zero-size queue で必ず返すようにした。
- AudioTrack write を blocking write + bounded zero-write retry に変更し、positive partial write 後に一時的な 0 write が返っても残り PCM を破棄しないようにした。
- ARIB broadcast genre の期待値更新に合わせ、TIS 側テストの genre トークン を `<majorName>/<middleName>` 形式へ更新した。
- `TisR51FixedPlanAcceptanceTest` の `SiCollectionResult` 呼び出しに `countsSignature` を追加し、r50bf の test compile 未達を解消した。
- Android/Soong build、instrumentationテスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bf
- r51 の音声対応範囲を ARIB STD-B32 の TS 音声前提に合わせ、AAC は `stream_type=0x0f` の ADTS のみ supported とし、`0x11` は LATM/LOAS 未実装のため supported/viewable/decoder 対象から除外した。
- setup scan の channel 登録を complete discovery のみに固定し、partial discovery は診断だけに残すようにした。
- CAT-only EMM メタデータを dynamic filter 対象へ含め、CAS 仮実装 のまま descramble 成功扱いにしない境界を維持した。
- TvProvider Programs 更新で、同一 channel/サービスの今回 EPG 更新区間 内にある obsolete event row を削除するようにした。
- `TvTrackInfo` language を `Locale` と最小 alias map で ISO 639-2/T へ正規化し、空文字・無効値では `setLanguage()` を呼ばないようにした。
- decoder input buffer 超過 sample の prefix queue を禁止し、sample 全体を drop + 診断カウンター に変更した。zero-size video output と AudioTrack partial write の扱いも修正した。
- dynamic receiver 登録を API 33+ で `RECEIVER_NOT_EXPORTED` 明示に統一し、parental-control action は framework 定数参照へ変更した。
- Android/Soong build、instrumentationテスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50be
- `tis/INTEGRATION.md` を開発規則で許可する方針に合わせ、TIS product integration 手順をリリース物として維持した。
- `tis/INTEGRATION.md` から release 固有表現を外し、product package、priv-app、Direct Boot、TIS discovery の統合確認手順に限定した。
- CHANGELOG の見出しを `# CHANGELOG` と `## r50be` 形式に統一した。

## r50bd
- r51向け Direct Boot 境界、TvProvider Programs 更新、サービス単位 CAS、AudioTrack write 診断、PTS代替同期 診断、extended event JSON 解析、TIS product integration を更新。

## r50bc6
- ライブセッション が検出した video メタデータを 現在番組 key ごとに保持し、後続の EIT 由来 Programs 再 publish でも `videoFormat` / `videoWidth` / `videoHeight` を保持するようにした。
- `MaleicacidLiveSession` の video メタデータ merge helper と、EIT 再 publish 後も `internal_provider_data` の `videoFormatB64` が消えない回帰テストを追加した。
- Android/Soong build、instrumentationテスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bc5
- `MaleicacidLiveSession` の ライブ Programs 更新を `ProgramPublishCoordinator` 経由へ統一し、r50bc4 の証跡不一致を解消した。
- ライブ更新 の重複抑止 署名 を TvProvider 投影対象全体に広げ、description、コンテンツレーティング、unsupported descriptor、video format などの更新を落とさないようにした。
- ライブ更新 が未登録 channel の Programs を作らず、既存 channel の投影内容変更だけを upsert する回帰テストを追加した。
- Android/Soong build、instrumentationテスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bc4
- r50bc3 完了判定で検出された test 側の `PlaybackUnavailableReason` 未定義参照を修正した。
- `ServiceListBuilder` の r51 平文視聴可能 判定を実経路から呼ぶ共通関数へ分離し、PMT/PCR/video/free_ca/CA descriptor/Rust 対応宣言可能 の反例を 受け入れテスト に追加した。
- r50bc3 で残った英語文コメントのうち、今回の修正範囲と既知指摘範囲を日本語化した。
- Android/Soong build、instrumentationテスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bc3
- r51ビルド前レビューに向けた r50bc2 の未達として、サービス単位の平文視聴可能部分公開、ライブ Programs のデバウンス、初回フレームタイムアウト補助、section短読みに関する分岐網羅、音声master代替処理診断情報を修正した。
- ライブ更新が既存チャンネルだけを更新し、同一 EIT スナップショットで余分な `TvProvider` upsert が発生しないよう、`ProgramPublishCoordinator` を追加した。
- リリースツリーから不正なトップレベル r50bc2 リリースノートを削除した。
- Android/Soong build, instrumentation tests, atest, VTS, CTS, and real-device checks were not run in this environment.

## r50bb11
- ビルド前レビューで見つかった r50bb10 の実装限定未達を修正した。
- `PlaybackStartGate` に渡す前に `onSelectTrack(TYPE_AUDIO)` の再生署名を非null化し、Kotlin nullable 型不一致を避けるようにした。
- 視聴制限によるブロック処理を統一し、初回フレーム時点のブロック判定でもAV pipelineを停止し、再生署名とゲート状態をリセットするようにした。
- Android/Soong build, instrumentation tests, atest, VTS, CTS, and real-device checks were not run in this environment.

## r50bb10
- 視聴制限の再評価で現在番組がブロック状態へ遷移した場合に再生を停止し、レーティングが再び許可されるまで pipeline を安全な利用不可状態に保つようにした。
- `onSelectTrack(TYPE_AUDIO)` は音声専用 filter/decoder 切替経路を使うように作り直した。音声切替失敗時は全体の `PlaybackPipeline.start()` を呼んで映像を破棄せず、`false` を返して現在の再生署名を維持する。
- 視聴制限による停止動作と、音声切替失敗時の状態維持を確認する対象限定の受け入れ確認を追加した。
- Android/Soong build, instrumentation tests, atest, VTS, CTS, and real-device checks were not run in this environment.

## r50bb7
- Android 14 の `TvInputService.onCreateSession(inputId, sessionId)` sessionId 付き overload を追加した。
- r51 TIS の視聴可能/再生選択経路から HEVC `0x24` を除外した。
- ARIB 視聴年齢制限 JNI getter、Kotlin レーティング写像、TvProvider コンテンツレーティング投影、LiveSession 視聴制限ゲートを追加した。
- 初回フレームコールバック用の再生世代ガードを追加した。
- Limited ライブ更新 program publication to existing channels and added boot-time minimal EPG sync entrypoint.
- H.264 SPS の寸法解析を追加し、AVC MediaFormat 構築時の固定 1920x1080 代替処理を削除した。
- PMT由来の trackメタデータ通知と、`onSelectTrack()` による音声切替を追加した。
- NONE 時に selector setter を使わないことで、CS110 stream selector 既定値への依存を削除した。
- Enabled セクションフィルター CRC and added PID/table/状態 ingest counters.

## r50bb4
- PMT PID を r51視聴可能サービス公開前に検出できるよう、PMT filter 開始処理を `snapshotPmtPidsForSectionFilters()` へ切り替えた。
- TvProvider のチャンネル公開を行わなくてもスクランブルサービスを診断情報上で観測できるよう、ライブ/スキャンの CAS メタデータ経路を `snapshotCaMetadataForCasDiscovery()` と `snapshotServicesForCasDiscovery()` へ切り替えた。
- サービス/PCR/video/audio/CAS の再生状態を変えない section 更新で AV 再生が再起動しないよう、`MaleicacidLiveSession` に再生署名ゲートを追加した。
- Android/Soong build, instrumentation tests, VTS, CTS, and real-device checks were not run in this environment.

## r50ba2
- `ChannelKeys.kt` を package 外の `../` source path ではなく Soong module dependency 経由で `rec` tests から参照できるよう、`maleicacid_tvinput_channel_keys_sources` filegroup を追加した。
- No TIS Kotlin implementation, resources, manifest, permissions, or product integration files were changed.

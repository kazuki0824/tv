## r50bk10

- r50bk8 completion 版で残っていた完了条件未達を仕掛かり範囲に限定して再固定した。
- SetupActivity の generation / purpose / published 件数判定を pure helper 化し、stale setup Completed、boot/background Completed、invalid inputId では成功終了しないことを test で固定した。
- Program publish の required query failure、insert failure、obsolete delete failure、retry failureClass、backoff / attempts / retention を testable boundary として固定した。
- publish 失敗時に signature cache を commit せず、同一入力の retry が unchanged skip されないことを test で固定した。
- Android/Soong build、Kotlin compile、instrumentation test 実行、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bk8-rerelease

- r50bk8 TIS / arib_si_engine_rs 追加修正計画の7セクションに対応した。
- provider-data 新規 write を Rust/JNI bridge 経由の JSON v1 へ寄せ、Channel/Program の key 抽出・signature 生成・current-program overlap diagnostics 追記を native 側 API で扱う経路を追加した。
- EIT update window の `deletionAuthoritative` を TIS publish transaction へ伝播し、authoritative でない window では obsolete Program delete を実行しないようにした。
- Program publish retry に failureClass / attempt / nextAttemptAtMs / backoff / 上限 trim を追加した。ただし pending retry は引き続き process-local であり、永続化 store への移行は未実施。
- production path の snapshot 利用を `snapshotTransaction()` に寄せ、旧 snapshot API を deprecated として明示した。
- SetupActivity を `BIND_TV_INPUT` permission で保護し、自 TIS inputId 検証と scan generation 照合により外部起動・過去 Completed state による成功終了を抑止した。
- CAS descrambler PID type を AOSP `Descrambler.PID_TYPE_T` に修正した。
- AudioTrack 生成時に Android 14 の AttributionSource を可能な範囲で伝播する処理を追加した。
- Android/Soong build、Kotlin compile、instrumentation test、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bk8

- r50bk7 のフェーズ1〜6静的再監査で、unsupported parental rating 由来の `unsupportedDescriptorJson` を `EventModelMapper` が旧 `diagnosticCode` / `descriptorOffset` 形で新規生成しており、フェーズ6の「新規 provider-data write は schemaVersion=1 の canonical descriptor diagnostics shape だけにする」完了条件に未達であることを確認した。
- `unsupportedDescriptorJson` の生成を `schemaVersion=1` / `diagnostics[]` / `parseStatus=UnsupportedValue` / `tag=0x55` / `serviceKey` / `eventId` を持つ canonical shape に変更し、`TvProviderWriter` の migration-read 経路に依存しない新規 write へ固定した。
- r50bk8 は改訂2版 Markdown のフェーズ1〜6完了版として扱う。Android/Soong build、Kotlin compile、Rust unit test、instrumentation test、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bk5

- r50bk4 のフェーズ3・4A・4B・4C静的再監査で、PlaybackPipeline の MediaCodec first-frame callback が main handler から playbackGeneration / surface / videoAvailableNotified を直接参照・更新し得る点を4C未達として確認した。
- first-frame callback は playback executor へ enqueue し、playbackGeneration / surface / first-frame state の確認と onVideoAvailable 通知を playback executor 上で実行するよう修正した。
- フェーズ5・6には進まず、r50bk5 は改訂2版 Markdown のフェーズ1〜4C完了版として扱う。
- Android/Soong build、Kotlin compile、instrumentation test、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bk4

- r50bk3 のフェーズ3・4A・4B・4C静的再監査で、SetupActivity が user-unlock receiver 経由で boot EPG sync を開始し得る点と、ChannelScanController の SI collection 判定が ServiceListBuilder 経由で services / publishability を別 snapshot から合成し得る点を未達として確認した。
- SetupActivity から user-unlock drain receiver 登録を削除し、setup activity 起動中は Direct Boot pending を表示するだけで boot EPG sync を開始しないよう固定した。
- ChannelScanController の registration-ready 判定を、同一 snapshotTransaction 由来の serviceCounts に一本化し、serviceListBuilder.registrationReadySnapshot() の別 snapshot 合成を production path から除去した。
- フェーズ5・6には進まず、r50bk4 は改訂2版 Markdown のフェーズ1〜4C完了版として扱う。
- Android/Soong build、Kotlin compile、instrumentation test、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bk2

- r50bk のフェーズ1・2静的再監査で、同一 event の start/end が update window 外へ移動した場合に旧 Program row を stable key で発見できず duplicate insert し得る未達を確認した。
- Program upsert の既存 Program index を window 限定から service/channel 全体の stable programKey index へ変更し、ONID/TSID/SID/event identity が同じ Program は start/end 変更後も既存 row update になるよう修正した。
- フェーズ3以降には進まず、r50bk2 は改訂2版 Markdown のフェーズ1・2完了版として扱う。
- Android/Soong build、instrumentation test、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bk

- 改訂2版 Markdown のフェーズ1・2に従い、Program identity / current program / provider-data signature と TvProvider query failure / null cursor / Program upsert safety の実装を修正した。
- `programKey` を ONID/TSID/SID/event の安定キーに固定し、start/end と row id dependent diagnostics を signature 対象から外した。
- TvProvider null cursor を failure として扱い、`existingChannels()` の空 fallback を production path から除去し、service 単位 failure 時の obsolete delete を禁止した。
- Android/Soong build、instrumentation test、atest、VTS、CTS、実機確認はこの環境では未実施。


## r50bj3

- r50bj2 後に残っていた設計未固定事項として、malformed descriptor / malformed SI の fail-closed 方針、malformed EIT event を obsolete delete 根拠にしない条件、SectionEvent / MediaEvent 入力上限、Direct Boot drain と live session 優先、TvProvider required query の null cursor failure 扱いを固定した。
- `ARIB_SI_EPG_TvProvider投影方針.md` に、malformed descriptor / malformed EIT event 由来値を TvProvider 標準列へ正常投影しないことを明記した。
- 実装コードは変更していない。Android/Soong build、instrumentation test、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bj2

- r50bj の設計固定後に残っていた旧未固定記述を整理し、`ARIB_SI_EPG_TvProvider投影方針.md` の `internal_provider_data` schema/key/サイズ上限/LONG_DESCRIPTION 最大長を未固定扱いしないようにした。
- `onUnblockContent()` の start/end は stable identity ではなく current Program row 照合用の補助条件であることを `DESIGN_JA.md` / `INTEGRATION.md` に明記した。
- 実装コードは変更していない。Android/Soong build、instrumentation test、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bj

- 設計文書上で provider-data JSON v1、Rust serde SSOT、descriptor diagnostics schema、transaction DTO、session/playback/scan executor 境界、SetupActivity 保護、retry/backoff を固定した。
- `programKey` を ONID/TSID/SID/event_id のみに固定し、start/end を stable identity から外した。
- 実装コードは変更していない。Android/Soong build、instrumentation test、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bi6

- Phase A の lifecycle / generation / executor / cancel / CAS state 未達を是正した。`TunerController` の section callback を controller serial executor + generation + filter token で隔離し、`onTune()` 解決失敗時も旧 live state を先に破棄するようにした。
- `CasController` の mutation / ECM / EMM / close を専用 serial executor に閉じ、`ChannelScanManager.cancel()` が executor 外から controller / engine を close しないようにした。
- Phase B の別 transport service 登録を PAT/PMT 由来の actual transport key に限定し、SDT-other / NIT-other / BAT だけで見えた service を現在 candidate の物理情報へ紐づけないようにした。
- `NativeAribSiParser` に production bulk snapshot wrapper を追加し、`AribSiEngine` の public snapshot path を bulk wrapper 経由にした。
- Android/Soong build、instrumentation test、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bi4

- 「追加だけで対応できる」テスト補強として、malformed / truncated 相当の parental rating が `Programs.COLUMN_CONTENT_RATING` に投影されず internal provider data 診断へ残ることを acceptance test で固定した。
- unsupported country / out-of-range rating が Programs の content rating column に出ず、diagnostic provider data に残ることを追加確認した。
- 旧 custom `ARIB_JP` / `AGE_*` rating が通常 product path の mapper 出力にならないことと、merged CAS state が Programs provider data に保持されることを追加確認した。
- Android/Soong build、instrumentation test、atest、VTS、CTS、実機確認はこの環境では未実施。


## r50bi3

- Programs CAS fallback の current diagnostic complete 判定を、理由文字列だけでなく Rust 由来の `pmtPidResolved` / `pmtParsed` / `caStateResolved` / `freeCaModeResolved` 明示 field で判定するようにした。
- Rust publishability diagnostic から PMT/CAS/free_CA_mode 解決状態を JNI/TIS へ公開した。
- current program identity が変わった場合に一時 unblock key を明示破棄するようにし、同一番組・同一 rating 限定の unblock 条件をテストで固定した。
- diagnostic complete 明示 field と current program change unblock 破棄の acceptance test を追加した。
- Android/Soong build、instrumentation test、atest、VTS、CTS、実機確認はこの環境では未実施。


## r50bi2

- `onUnblockContent()` を current program の rating 一致かつ event/start/end identity 完備時だけ一時 unblock するように修正した。
- Programs CAS fallback の current diagnostic complete 判定を厳格化し、不完全 diagnostic + 既存 scrambled channel 状態の維持を `MERGED_CHANNEL_CAS_STATE` として明示した。
- AOSP ISDB rating の境界値、未対応値、UNRATED fallback unblock 禁止、merged CAS state の acceptance test を追加した。
- Android/Soong build、instrumentation test、atest、VTS、CTS、実機確認はこの環境では未実施。


## r50bi

- Programs CAS 状態 fallback を追加し、current diagnostic 欠落または不完全時に既存 channel `internal_provider_data` から `requiresCas` / `unsupportedCas` / `clearLivePlaybackSupported` / `channelRegistrationReady` / `epgPublishable` を復元して Programs 側へ保存するようにした。
- parental rating の Programs 投影と Live session enforcement を r51 claim 対象として固定し、AOSP system-defined ISDB rating（`com.android.tv / ISDB / ISDB_4..20`）へ変更した。
- custom ARIB_JP / AGE_* rating projection を通常 product path から廃止した。
- parental block 時は `notifyContentBlocked()` + AV停止を主通知とし、parental block 理由で `notifyVideoUnavailable()` を呼ばないようにした。
- CAS 未完成 / scrambled unsupported の video unavailable reason を `VIDEO_UNAVAILABLE_REASON_CAS_UNKNOWN` に固定した。

# CHANGELOG

## r50bh

- Switched TIS readiness consumption to Rust `channelRegistrationReady` / `epgPublishable` / `clearLivePlaybackSupported` diagnostics instead of local re-computation.
- Published Programs for registered/EPG-publishable services, including CAS-unsupported scrambled services, while keeping scrambled services out of clear live playback success.
- Stored `requiresCas`, `unsupportedCas`, `clearLivePlaybackSupported`, `channelRegistrationReady`, and `epgPublishable` in channel `internal_provider_data`.
- Deferred boot EPG sync when a live session or scan is active and retried pending boot EPG sync after the live session count returns to zero or after the blocking scan/maintenance finishes.
- Android/Soong build, instrumentation test, atest, VTS, CTS, and real-device checks were not run in this environment.

## r50bg2

- background channel maintenance の r51 必須実装条件を満たすため、boot EPG sync 成功後に background maintenance 起動を試行する接続を追加した。
- background channel maintenance は scan/maintenance 実行中または active live session 存在時には開始せず、skip 理由を `BackgroundChannelMaintenanceDiagnostics` に残すようにした。
- live session の active count を `ChannelScanManager` で管理し、maintenance 開始判定のテストを追加した。
- Android/Soong build、instrumentation test、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bg

- CAT-only EMM metadata を CAS state / diagnostics に残し、PMT CA がない clear service の playback は block しないまま、descrambler attach / key token / scrambled success へ接続しないようにした。
- empty EIT update window を JNI/TIS publish path に伝え、非空→空の EPG 更新でも obsolete `TvProvider.Programs` を削除できるようにした。
- boot EPG sync を既存 channel の p/f 最小更新に限定し、`background channel maintenance` を r51 スコープ内の必須実装として追加した。どちらも新規 channel insert は行わない。
- setup scan channel registration を service-local registration-ready gate に変更し、global discovery complete 前でも registration-ready な partial service は登録可能にした。
- `ProgramPublishResult.changed` に deleted を含め、delete-only update を変化として扱うようにした。
- `tis/DESIGN_JA.md` を r51 の boot/background maintenance と service-local registration-ready 方針に合わせて改訂した。
- Android/Soong build、instrumentation test、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bf2

- r50bf のロジック未達を是正し、decoder oversized sample drop 時に MediaCodec input buffer を zero-size queue で必ず返すようにした。
- AudioTrack write を blocking write + bounded zero-write retry に変更し、positive partial write 後に一時的な 0 write が返っても残り PCM を破棄しないようにした。
- ARIB broadcast genre の期待値更新に合わせ、TIS 側テストの genre token を `<majorName>/<middleName>` 形式へ更新した。
- `TisR51FixedPlanAcceptanceTest` の `SiCollectionResult` 呼び出しに `countsSignature` を追加し、r50bf の test compile 未達を解消した。
- Android/Soong build、instrumentation test、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bf

- r51 の音声対応範囲を ARIB STD-B32 の TS 音声前提に合わせ、AAC は `stream_type=0x0f` の ADTS のみ supported とし、`0x11` は LATM/LOAS 未実装のため supported/viewable/decoder 対象から除外した。
- setup scan の channel 登録を complete discovery のみに固定し、partial discovery は診断だけに残すようにした。
- CAT-only EMM metadata を dynamic filter 対象へ含め、CAS placeholder のまま descramble 成功扱いにしない境界を維持した。
- TvProvider Programs 更新で、同一 channel/service の今回 EPG update window 内にある obsolete event row を削除するようにした。
- `TvTrackInfo` language を `Locale` と最小 alias map で ISO 639-2/T へ正規化し、空文字・無効値では `setLanguage()` を呼ばないようにした。
- decoder input buffer 超過 sample の prefix queue を禁止し、sample 全体を drop + diagnostic counter に変更した。zero-size video output と AudioTrack partial write の扱いも修正した。
- dynamic receiver 登録を API 33+ で `RECEIVER_NOT_EXPORTED` 明示に統一し、parental-control action は framework 定数参照へ変更した。
- Android/Soong build、instrumentation test、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50be

- `tis/INTEGRATION.md` を開発規則で許可する方針に合わせ、TIS product integration 手順をリリース物として維持した。
- `tis/INTEGRATION.md` から release 固有表現を外し、product package、priv-app、Direct Boot、TIS discovery の統合確認手順に限定した。
- CHANGELOG の見出しを `# CHANGELOG` と `## r50be` 形式に統一した。

## r50bd

- r51向け Direct Boot 境界、TvProvider Programs 更新、service scoped CAS、AudioTrack write 診断、PTS fallback 診断、extended event JSON 解析、TIS product integration を更新。

## r50bc6

- live session が検出した video metadata を current program key ごとに保持し、後続の EIT 由来 Programs 再 publish でも `videoFormat` / `videoWidth` / `videoHeight` を保持するようにした。
- `MaleicacidLiveSession` の video metadata merge helper と、EIT 再 publish 後も `internal_provider_data` の `videoFormatB64` が消えない回帰テストを追加した。
- Android/Soong build、instrumentation test、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bc5

- `MaleicacidLiveSession` の live Programs 更新を `ProgramPublishCoordinator` 経由へ統一し、r50bc4 の証跡不一致を解消した。
- live refresh の重複抑止 signature を TvProvider 投影対象全体に広げ、description、content rating、unsupported descriptor、video format などの更新を落とさないようにした。
- live refresh が未登録 channel の Programs を作らず、既存 channel の投影内容変更だけを upsert する回帰テストを追加した。
- Android/Soong build、instrumentation test、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bc4

- r50bc3 完了判定で検出された test 側の `PlaybackUnavailableReason` 未定義参照を修正した。
- `ServiceListBuilder` の r51 clear-viewable 判定を実経路から呼ぶ共通関数へ分離し、PMT/PCR/video/free_ca/CA descriptor/Rust claimable の反例を acceptance test に追加した。
- r50bc3 で残った英語文コメントのうち、今回の修正範囲と既知指摘範囲を日本語化した。
- Android/Soong build、instrumentation test、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bc3

- Fixed r50bc2 completion gaps for r51 pre-build review: service-level clear-viewable partial publication, live Programs debounce, first-frame timeout helpers, section short-read decision coverage, and audio-master fallback diagnostics.
- Added `ProgramPublishCoordinator` so live refresh updates existing channels only and identical EIT snapshots do not cause redundant `TvProvider` upserts.
- Removed the invalid top-level r50bc2 release note from the release tree.
- Android/Soong build, instrumentation tests, atest, VTS, CTS, and real-device checks were not run in this environment.

## r50bb11

- Fixed the implementation-only r50bb10 gaps found in pre-build review.
- Made `onSelectTrack(TYPE_AUDIO)` non-null the playback signature before passing it to `PlaybackStartGate`, avoiding a Kotlin nullable type mismatch.
- Unified parental-control blocked handling so first-frame blocked decisions also stop the AV pipeline and reset playback signature/gate state.
- Android/Soong build, instrumentation tests, atest, VTS, CTS, and real-device checks were not run in this environment.

## r50bb10

- Stopped playback when parental-control reevaluation transitions the current program into a blocked state, keeping the pipeline in a safe unavailable state until the rating becomes allowed again.
- Reworked `onSelectTrack(TYPE_AUDIO)` to use an audio-only filter/decoder switch path. Failed audio switching now returns `false` and preserves the current playback signature instead of invoking full `PlaybackPipeline.start()` and tearing down video.
- Added targeted acceptance checks for blocked parental-control stop behavior and audio-switch failure preservation.
- Android/Soong build, instrumentation tests, atest, VTS, CTS, and real-device checks were not run in this environment.

## r50bb7

- Added Android 14 sessionId overload for `TvInputService.onCreateSession(inputId, sessionId)`.
- Removed HEVC `0x24` from r51 TIS viewable/playback selection paths.
- Added ARIB parental rating JNI getters, Kotlin rating mapping, TvProvider content rating projection, and LiveSession parental blocking gate.
- Added playback generation guard for first-frame callbacks.
- Limited live refresh program publication to existing channels and added boot-time minimal EPG sync entrypoint.
- Added H.264 SPS dimension parsing and removed fixed 1920x1080 fallback for AVC MediaFormat construction.
- Added PMT-derived track metadata notification and `onSelectTrack()` audio switching.
- Removed CS110 stream selector default dependency by leaving selector setters unused for NONE.
- Enabled section filter CRC and added PID/table/status ingest counters.

## r50bb4

- Switched PMT filter opening to `snapshotPmtPidsForSectionFilters()` so PMT PIDs are discovered before r51 viewable service publication.
- Switched live/scan CAS metadata paths to `snapshotCaMetadataForCasDiscovery()` and `snapshotServicesForCasDiscovery()` so scrambled services remain visible for diagnostics without TvProvider channel publication.
- Added playback signature gating in `MaleicacidLiveSession` so section updates that do not change service/PCR/video/audio/CAS playback state no longer restart AV playback.
- Android/Soong build, instrumentation tests, VTS, CTS, and real-device checks were not run in this environment.

## r50ba2

- Added the `maleicacid_tvinput_channel_keys_sources` filegroup so `ChannelKeys.kt` can be referenced by `rec` tests through a Soong module dependency instead of an out-of-package `../` source path.
- No TIS Kotlin implementation, resources, manifest, permissions, or product integration files were changed.

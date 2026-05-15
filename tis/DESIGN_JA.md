# TIS 設計判断

## AOSP 標準経路

TIS は `TvInputService` として システムTVアプリ から呼ばれ、Tuner HAL には Tuner SDK API 経由でアクセスする。HAL binder を直接呼ばない。
TIS の setup / boot EPG sync / user unlock drain は、固定文字列や package 名を inputId とみなしてはならない。`TvInputManager.tvInputList` から自 `MaleicacidTvInputService` に一致する `TvInputInfo.id` を一意に解決し、その inputId だけを scan / sync / TvProvider writer へ渡す。解決不能または複数一致の場合、boot EPG sync は pending のまま延期し、setup scan は開始しない。

## BS と CS110 の選局契約

BS は IF 周波数と typed stream selector を保持し、selector は TSID または px4 専用の相対 TS 番号として保存する。earth_pt1 は TSID のみ許容し、px4 は TSID と相対 TS 番号を許容する。CS110 は周波数帯のみで scan candidate と tune selector を作り、stream selector を保存しない。

CS110 tune request 生成時、TIS は Android Tuner API builder の default `streamId` / `streamIdType` に依存しない。CS110 では frontend stream selector を明示的に none / `UNDEFINED` 相当に設定する。CS110 の ONID / TSID / service_id は channel identity / サービス識別子 として保持してよいが、HAL frontend selector へ転用してはならない。BS は IF 周波数 + TSID、または px4 backend 限定の relative stream number を使う。

TvProvider の channel internal provider data には JSON v1 `tune.streamIdType` と `tune.streamId` を保存する。`NONE` は `streamId=null`、`TSID` は 0..0xffff、`RELATIVE` は 0..7 とする。


## 製品 scan 候補表の保持者

TIS は製品 scan 候補表の実装データ保持者である。選局対象、対象周波数帯、BS/CS110 selector 境界、CATV 候補範囲の設計契約は tv 直下の開発規則.mdを正とする。

TIS が保持する候補表の具体値は製品 scan 実装データのSSOTである。ただし、選局対象範囲、VHF除外、CATV C13〜C63限定、BS/CS110 selector 境界などの設計契約は tv 直下の `開発規則.md` を正とする。TIS の候補表は `開発規則.md` の設計契約に反してはならない。

TIS 以外の文書や実装に同等の scan 候補表を重複保持してはならない。Tuner HAL に渡す値は、TIS が生成した explicit tune candidate に限定する。

TIS は地上UHF、CATV、BS、CS110の候補を持ち、Tuner HALには explicit tune candidate として渡す。Tuner HAL は日本向け scan 候補表を自前生成しない。

CATV候補表は C13〜C63 に固定する。MID band は C13〜C22、SHB band は C23〜C63 とし、中心周波数は ARIB STD-B21 Appendix 10 の `+1/7 MHz` オフセット込みで保持する。C22 は `167 + 1/7 MHz`、C23 は `225 + 1/7 MHz` であり、C21からC22、C22からC23は単純な6MHz連続として計算しない。

VHF 1〜12ch は開発規則.mdで恒久的にスコープ外であり、TISのCATV候補表、地上波候補表、共同受信候補表に追加してはならない。

BSの通常実行時候補生成はTISが持つBS TSID表だけを正とする。px4向けの相対TS番号候補は診断またはbackend指定候補としてのみ許可し、earth_pt1向けには使わない。px4 backend側にTSIDからlegacy slotへの同等表または変換表を持つ場合でも、TISのBS TSID表と不一致になってはならない。


## 録画・予約の r51 除外

r51 では録画・予約を製品機能として表明しない。TIS メタデータの `android:canRecord` は `false` のまま維持し、`MaleicacidTvInputService.onCreateRecordingSession()` は `null` を返す。`RecordingSession`、DVR/file output、`RecordedPrograms` 登録、`notifyRecordingStopped()` / `notifyError()`、`TvRecordingClient` による予約録画開始は r53 対象である。

`rec/` 配下の実装とテストは r53 準備領域であり、r51 product package、TIS manifest、boot receiver、release確認条件へ混ぜない。r51 で起動してよい receiver / サービスは TIS の ライブ視聴・setup・EPG publish に必要なものだけとする。

## CAS / descrambler の r51 境界

r51 では CAS HAL 本体はプレースホルダーのままにする。TIS は Tuner SDK API の filter 経由で PMT/CAT/SDT/ECM/EMM section payload を取得し、PMT/CAT から得た CA_descriptor と SDT 等から得た free_CA_mode / サービス識別子 補助情報を arib_si_engine_rs / TIS 側で CA情報 / サービスメタデータ意味モデル に変換する。TIS はその CA情報 に基づいて ECM/EMM セクションフィルター と MediaCas/CAS bridge を型付き API で制御し、実 key トークン が得られた場合だけ Tuner descrambler へ不透明な参照値を渡す。仮実装 や診断専用結果は復号成功を意味しないため、`setKeyToken()` へ渡さない。Tuner HAL が未接続診断を返した場合も成功扱いにしない。

## Tuner SDK API 呼び出し

`openDescrambler()`、`setKeyToken()`、`addPid()`、`removePid()` は reflection を使わず、対象 build の system/privileged API として直接呼ぶ。API が利用できない build は r51 対象外とする。

## 再生経路

r51 では MediaCodec block モデル の reflection 代替処理 を禁止する。対象 build で型付き block モデル を安定利用できない場合は、成功を偽装せず `notifyVideoUnavailable()` へ落とす。`notifyVideoAvailable()` は `Filter.start()` 成功、汎用 `FilterEvent` 到着、payload 付き `MediaEvent` 到着だけでは呼ばない。ES header から decoder 構成に必要な情報を抽出し、MediaCodec output frame が Surface へ render された コールバック を映像到達 gate として呼ぶ。

## EIT と TvProvider

r51 は EIT p/f を主経路として使う。scan/setup 後に `TvProvider.Programs` へ出す最低限の Programs だけ EIT schedule actual `0x50..0x5F` から短期補完として拾う。schedule actual を常時・長期 EPG 収集として扱わず、EIT schedule other `0x60..0x6F`、長期 schedule window、サービス横断更新は r53 対象とする。Programs の `internal_provider_data` には JSON v1 の stable `programKey`、timing、CAS state、長形式イベント項目、component/audio メタデータ、series 完全構造、イベントグループ `relatedItems`、linkage、free_CA_mode、音声言語、レーティング、診断 JSON を TIS 内部データとして保存する。TvProvider の標準 column には title / short description / long description、broadcast genre、明示写像できる canonical genre、series id、episode display number、item count、scrambled、audio language、コンテンツレーティング など自然対応できる範囲だけ反映する。

## 字幕表示の責務

ARIB 字幕は TIS 側の字幕 path で `libaribcaption` を使用する。r51 では PMT から字幕 track を検出し、`TvTrackInfo.TYPE_SUBTITLE` として通知し、`onSetCaptionEnabled()` と字幕表示経路を接続する。字幕 track を advertise する場合は、ARIB 字幕 PES を libaribcaption C API 経路で処理し、実際に表示できることを完了条件に含める。`arib_si_engine_rs` の自前 ARIB 文字列 decoder はサービス名・番組名・番組説明など字幕以外の SI/EPG 文字列に限定し、字幕 PES や字幕本文をその decoder に渡さない。libaribcaption は C API のみを使用し、独自 C/C++ 薄層 は書かない。Kotlin から直接 C API を呼ばず、TIS Kotlin → Rust JNI boundary → 安全なRustラッパー → libaribcaption C API の順に接続する。BML / data broadcast 実行環境、双方向データ放送 UI、データ放送 UI は恒久対象外である。

## r51 ライブ playback 実装方式

TIS の ライブ playback は、案Aだけを採用する。すなわち、Tuner AV filter の `MediaEvent` を 平文 ES payload として受け取り、video は `MediaCodec` へ投入して `Surface` へ出力し、audio は `MediaCodec` で PCM 化して `AudioTrack` へ流す。

`tunneled` / platform passthrough playback path は r51 の設計候補から外し、実装しない。`notifyVideoAvailable()` は `Filter.start()` 成功、汎用 `FilterEvent`、payload 付き `MediaEvent` 到着だけでは呼ばない。video decoder が起動し、最初の decoded frame が `Surface` へ render された コールバック を gate とする。

setup scan の channel registration は global discovery complete を必須条件にしない。ただし partial snapshot を無条件に channel insert に使ってはならない。TvProvider 登録には サービス単位の登録可能 gate を使う。登録可能 は、ONID / TSID / SID が確定し、channel URI から物理 tune key に戻せ、PMT PID と PMT、PCR PID、r51対応 video ES が取得済みで、audio は対応済みまたは video-only として診断可能であり、サービス名 は正式名または deterministic な仮名と後続更新方針を持ち、平文ライブ視聴の対応宣言可能 または scrambled unsupported として状態通知可能な サービスに限定する。登録可能未満の partial snapshot は 診断情報 / ライブ更新 / debugにのみ使い、channel insert しない。scrambled サービスは channel 登録してよいが、CAS 仮実装 のまま 平文ライブ視聴成功 対応宣言 してはならない。

## codec header / A-V sync / publish mode の固定

ライブ playback の codec 構成は、r51 では video は MPEG-2 video と H.264/AVC、audio は AAC と MPEG audio を対象 codec とする。r52 では ARIB資料ベースで国内放送全般であり得る codec を固定表として認識し、対応可能な codec を MediaFormat / decoder 起動 / AudioTrack / first-frame gate / unsupported 診断情報に接続する。

r51 対応 video ES が存在しない サービスは viewable としない。HEVC など r51 未対応 video codec のみを持つ サービスは、再生不能として `notifyVideoUnavailable()` へ落とす。r52 では HEVC を codecメタデータ として認識するが、ISDB-S3 / MMT / TLV 等の恒久対象外 transport profile 由来の場合は ライブ viewable capability として 対応宣言しない。r51 時点でも、PMT上で認識できる未対応 codecメタデータは provider-data の `components.video[]` / `components.audio[]` に `r51PlaybackSupported=false`、`liveViewableClaim=false`、`diagnosticCode=UNSUPPORTED_R51_CODEC` として保存し、再生可能表明とは分離する。

r51 対応 video ES が存在し、audio ES が存在しない、または audio codec だけが r51 未対応の場合は、video-only サービスとして視聴可能にする。この場合、`notifyVideoUnavailable()` には落とさず、`audio absent` または `unsupported audio codec` を診断に残す。未対応 audio codec は、対応 video が存在する限り video-only 診断の対象であり、video unavailable の直接理由にしてはならない。AC-3 / Enhanced AC-3 / MPEG-H 3D Audio は、今回確認した ARIB 資料群では国内放送全般の対象 codec として固定する根拠を持たないため、r52 codec 固定表には含めない。

decoder は PMT の stream_type だけでは構成せず、MediaEvent payload から MPEG-2 sequence header、H.264 SPS/PPS、AAC ADTS/AudioSpecificConfig、MPEG audio header を検出してから MediaFormat を構成する。

MediaEvent payload は、`offset >= 0`、`dataLength > 0`、`offset + dataLength <= mapped buffer capacity`、`dataLength <= MAX_MEDIA_SAMPLE_BYTES` を満たす場合だけ decoder queue に渡す。`MAX_MEDIA_SAMPLE_BYTES` は r51 では 4 MiB hard limit とする。範囲不正は playback pipeline 例外ではなく sample drop と 診断カウンター として扱う。

A/V同期方式は r51 で non-tunneled 平文視聴 に固定する。tunneled playback と avSyncHwId は r51 範囲外であり、TIS の non-tunneled playback では avSyncHwId を使わないが、Tuner HAL API としては r51 で実装・AOSP準拠に仕様を固定する。video/audio は MediaCodec と AudioTrack の PTS により同期する。audio が存在する場合は AudioTrack を master clock とする。video-only サービスは視聴可能として扱い、audio が存在しない場合は `audio absent`、audio codec だけが未対応の場合は `unsupported audio codec` を診断に残す。

TvProvider公開モードは `PublishMode` で channel row 追加を setup scan / explicit rescan に限定する。ライブ tune refresh、boot EPG sync、background channel maintenance では既存 channel の番組・診断更新だけを許可し、新規 channel row は追加しない。

## ARIB SI/EPG のTvProvider投影

ARIB SI/EPG の標準列投影は tv直下の `ARIB_SI_EPG_TvProvider投影方針.md` を正とし、`internal_provider_data` の具体 schema / canonical encode / 署名 は `arib_si_engine_rs` の Rust provider-data serde構造体を SSOT とする。TISは、同文書で標準列投影が固定された項目だけを TvProvider 標準列へ出し、標準列へ自然対応しない項目は JSON v1 `internal_provider_data` のみに構造化保存する。

`Programs.COLUMN_CANONICAL_GENRE` については、TIS が直接設定する値と、Android TvProvider が `Programs.COLUMN_BROADCAST_GENRE` から内部補完した読み出し結果を区別する。r51 では `ARIB_SI_EPG_TvProvider投影方針.md` の明示写像表に一致する分類だけを `ContentValues` に直接設定する。写像不能分類、reserved、extension、others、user_nibble 由来分類は直接設定しない。

`Programs.COLUMN_BROADCAST_GENRE` には、`arib_si_engine_rs` から受け取った ARIB content_descriptor の分類値とARIB表示名を、TIS が `TvContract.Programs.Genres.encode(...)` 形式で格納する。TIS は ARIB分類を Android canonical genre に推測変換しない。

## 視聴制限 / コンテンツレーティング 契約

TIS は `arib_si_engine_rs` から受け取った `parental_rating_descriptor` の構造化データを、AOSP system-defined ISDB レーティングドメイン（`com.android.tv / ISDB / ISDB_<age>`）の `TvContentRating` へ変換する。Android `TvContentRating` の domain / ratingSystem / レーティング 文字列は TIS 側で固定し、Rust 側のSSOTにしない。

TvProvider へ番組を登録または更新する場合、変換できる レーティングは `TvContentRating.flattenToString()` の結果を `Programs.COLUMN_CONTENT_RATING` に格納する。変換できない レーティングは推測で `COLUMN_CONTENT_RATING` に入れず、`internal_provider_data` と診断に保持する。

ライブセッション は、現在番組のレーティング と system 視聴制限 設定を同期して扱う。`TvInputManager.isParentalControlsEnabled()` が true の場合、TIS は現在番組の `TvContentRating`、または レーティング 未取得時の `TvContentRating.UNRATED` を `TvInputManager.isRatingBlocked(...)` に渡して判定する。blocked の場合は video frame を表示する前に再生を停止または抑止し、`notifyContentBlocked(rating)` を呼ぶ。許可された場合は `notifyContentAllowed()` を呼ぶ。

TIS は `TvInputManager.ACTION_BLOCKED_RATINGS_CHANGED` と `TvInputManager.ACTION_PARENTAL_CONTROLS_ENABLED_CHANGED` を監視し、設定変更時に現在番組の 視聴制限判定を即時再評価する。

## r51 TIS/arib_si_engine_rs 固定事項

- Android 14 系の通常 ライブセッション 生成では `onCreateSession(inputId, sessionId)` を実装し、framework 由来 `sessionId` を `Tuner(context, sessionId, useCase)` へ渡す。1引数 overload の 代替処理 sessionId は互換経路専用とする。
- r51 の video 対応宣言対象は MPEG-2 video `0x02` と H.264/AVC `0x1b` に限定する。HEVC `0x24` は r51 平文ライブ視聴 / playback selection 対象外であり、診断上は `NO_SUPPORTED_VIDEO_ES` 相当として扱う。r52 では MPEG-2 Video、H.264/AVC、HEVC を国内放送全般であり得る video codec として認識する。
- ARIB 視聴年齢制限 は Android `TvContentRating` へ `domain=com.android.tv`, `ratingSystem=ISDB`, `rating=ISDB_<age>` として写像する。対応範囲は JPN かつ レーティング 4..20 のみとし、未対応 country / レーティングは推測変換せず `internal_provider_data` / 診断に残す。レーティング 未取得時は `TvContentRating.UNRATED` として 視聴制限判定する。
- `notifyVideoAvailable()` は decoder の first frame コールバック が現行 playback generation と一致し、かつ 視聴制限 で block されていない場合だけ呼ぶ。現行世代と一致しない decoder コールバック は無視する。
- ライブ tune refresh では新規 channel row を作らず、既存 channel の program 更新だけを行う。setup/rescan のみ channel row を作成できる。
- H.264 は SPS/PPS 検出だけでなく SPS 由来の width / height を MediaFormat へ反映する。SPS 解析不能時は固定 1920x1080 代替処理 で成功扱いしない。
- PMT 由来の video/audio/subtitle track は `TvTrackInfo` として通知し、`onSelectTrack(TYPE_AUDIO, trackId)` と `onSetCaptionEnabled()` を受ける。r51 では字幕 track と libaribcaption 表示経路を実装対象に含める。別 video track と data track 選択は、対応 codec / 実行環境がない限り 対応宣言しない。
- CS110 は stream selector `NONE` のみ許可し、TSID / relative selector を HAL tune request へ渡さない。Android Tuner builder では NONE 時に selector setter を呼ばない。
- boot 後 EPG 再同期は既存 channel の p/f 最小更新に限定し、新規 channel row は作成しない。`JapanIsdbScanPlan.defaultInitialScan()` は setup scan / explicit rescan 専用であり、boot EPG sync の既定候補に使わない。
- background channel maintenance は r51 スコープ内の必須実装とする。ただし boot critical path から分離し、boot EPG sync 完了後または明示的保守タイミングで実行開始を試行する。実行開始は scan/maintenance が未実行で、かつ ライブセッション が存在しない場合に限る。active ライブセッション または scan 実行中の場合は開始せず、skip 理由を 診断情報に残す。対象は既存 channel と既存 transport メタデータ refresh までに限定し、新規 channel insert は行わない。
- セクションフィルター は CRC protected section で `setCrcEnabled(true)` を使用し、Rust 側 CRC 検査を defense-in-depth として維持する。TIS 側には PID / table / 状態 別 counter を持つ。


## 視聴年齢制限 / CAS 代替参照の固定

- `Programs.COLUMN_CONTENT_RATING` と Live session の 視聴制限判定は同じ `AribRatingMapper` を使い、`com.android.tv / ISDB / ISDB_4..20` の AOSP system-defined レーティングに統一する。
- TIS は custom レーティング-system XML / receiver を追加しない。product 統合時は システムTVアプリ / レーティング definitions に `com.android.tv / ISDB / ISDB_4..20` が存在することを確認する。
- Live session は 現在番組 レーティングを `TvProvider current Program -> latest EIT cache -> TvContentRating.UNRATED` の順で解決する。
- parental blocked の通知は `notifyContentBlocked(rating)` と AV停止を主とし、parental block の通知手段として `notifyVideoUnavailable()` を呼ばない。
- `onUnblockContent()` の解除範囲は同一 `channelUri + serviceKey + eventId + ratingString` の 現在番組 / レーティングに限定する。start/end は stable identity ではなく、解除対象が現在表示中の同一 Program row であることを確認する補助条件としてのみ使ってよい。start/end/duration を provider-data `programKey`、unblock stable identity、または Program identity の SSOT にしてはならない。
- CAS 未完成 / scrambled unsupported で 再生成功 にしない場合は `TvInputManager.VIDEO_UNAVAILABLE_REASON_CAS_UNKNOWN` を使う。具体的な CAS 状態 reason は CAS HAL 本実装まで使わない。
- Programs CAS 状態は current complete 診断 を優先し、不完全または欠落 診断 では既存 channel `internal_provider_data` の `requiresCas` / `unsupportedCas` / `clearLivePlaybackSupported` / `channelRegistrationReady` / `epgPublishable` を 代替処理 する。

## TIS / EPG 公開境界

r51 の EIT publish/delete 対象は、TvProvider に channel が存在する `ServiceKey`、または同一 setup/rescan transaction で channel insert が成功して channelId が確定した `ServiceKey` に限定する。ライブセッション の `currentService` だけには限定しない。Program row を持たない サービス へ Programs を publish/delete してはならない。

r51 の EIT 対象 table は、present/following actual `0x4E`、present/following other `0x4F`、および scan/setup 後の初期登録・短期補完に使う schedule actual `0x50..0x5F` のうち、上記対象 サービスに属する event とする。schedule actual を常時・長期収集として扱わない。schedule other `0x60..0x6F` は r51 Programs publish/delete 対象外であり、更新区間 を発生させない。

EIT 更新時の update/削除区間 は、追加・変更・削除された event の既存 `[start,end)` と新 `[start,end)` の union とする。r51 では長期固定 lookahead window を導入しない。r53 で長期 EPG lookahead window を扱う場合は、EIT scope / version / event identity / authoritative 条件と併用する。EIT table scope の version 変更で既存 section が消えた場合は、消えた event の既存 window も 廃止行削除 対象に含める。

ただし、廃止行削除 の根拠にできる EIT section / table snapshot は Rust parser が `deletionAuthoritative=true` と判定したものに限る。start_time BCD、duration BCD、event descriptor_loop_length、event fixed フィールド が malformed の event を含む section は、既存 event 削除用の authoritative valid-event-set として扱わない。malformed event は既存正常 Program を消す根拠にせず、DescriptorDiagnosticV1 / ParserDiagnosticV1 に記録する。

boot EPG sync の pending 平文 は boot EPG sync task 単位で判定する。task 中に cancel request を観測した場合は、成功 candidate があっても pending を 平文 しない。成功 candidate は `collectSiForCandidate()` が `COMPLETE` で、かつ 登録可能サービスが1件以上存在する candidate とする。background maintenance は pending 平文 を扱わない。

登録可能サービスは、`ServiceKey`、物理選局情報、inputId へ戻せる channel provider data、表示名が揃い、TvProvider channel insert/update に進める サービスとする。表示名は `ChannelRecord.displayName` が nonblank ならそれを使い、なければ SDT service_name、さらに無ければ `service-<onid>-<tsid>-<sid>` を使う。この 代替処理名は 登録可能判定上の有効な 表示名と扱う。

## CAS 仮実装 境界

CAS HAL 仮実装 のまま scrambled サービスを 平文ライブ視聴 再生成功 として扱ってはならない。scrambled unsupported サービス でも、PMT/CAT/CA情報 と診断を使って EPG / Programs / レーティング / provider-data は更新する。ただし CAS key トークン を提供できない状態では 再生成功 にせず、CAS 起因の unavailable のみ `VIDEO_UNAVAILABLE_REASON_CAS_UNKNOWN` へ map する。初回映像到達timeout、filter start failure、非対応stream、codec失敗、audio失敗 は CAS unknown に map しない。

CAS provider-data は current診断を優先する。`caStateResolved=true` で CAS required / unsupported / clearLivePlaybackSupported が判断できる場合は、`freeCaModeResolved=false` でも current診断を採用する。代替参照した診断情報を使った場合は `publishStateSource=fallback` とし、採用可能な状態がない場合は `publishStateSource=none` とする。

Descrambler API の `setKeyToken()`、`addPid()`、`removePid()` は戻り値が `Tuner.RESULT_SUCCESS` の場合だけ成功とする。非 SUCCESS result は CAS 診断 failure として扱い、成功扱いで握り潰してはならない。

## TvProvider failure semantics

TvProvider query failure と channel なしは別状態として扱う。既存 channel query が失敗した場合は `skippedNoChannel` として扱わず、failure 診断とし、署名 更新・pending 平文 の根拠に使わない。

TvProvider query は 必須問い合わせ と 任意問い合わせ を区別する。チャンネル・番組の追加または更新、廃止行削除、既存チャンネル・番組検索、provider-data代替参照、Direct Boot準備完了 判定に使う query は 必須問い合わせ とする。必須問い合わせ で `ContentResolver.query()` が null cursor を返した場合は `TvProviderQueryFailure` とし、empty result とみなさない。`TvProviderQueryFailure` が発生した サービス/window では channel insert、program insert/update、廃止行削除、署名キャッシュ更新、Direct Boot保留解除 に進まず、再試行区間 を保持する。

Programs publish/delete が provider failure になった場合は、`ProgramPublishCoordinator` の process-local retry queue に `ServiceKey + updateWindow + failureClass` を key として enqueue する。backoff は 1分、5分、15分、60分、以後最大60分、jitter ±20%、最大10回、保持期間24時間または次回正常 snapshot までとする。次回 `publishLiveProgramsForCurrentService()`、boot EPG sync、background maintenance の publish entrypoint 先頭で retry queue を drain する。成功した key は削除し、失敗した key は保持する。process restart では retry queue を破棄し、boot/background sync による再収集を正とする。provider failure 時は 廃止行削除、署名 update、pending 平文 に進まない。

retry queue は全体上限 512 windows、ServiceKey ごと上限 32 windows とする。超過時は古い順に破棄し、ServiceKey 別 `droppedRetryWindowCount` を加算する。process restart 後は counter を 0 に戻す。

SDT-other / NIT-other / BAT 由来で現在 candidate の actual transport に解決できない サービスは、現在 candidate の物理情報で channel insert しない。未登録で Program row が存在しない unresolved transport は scan/maintenance 診断情報に `skippedUnresolvedTransportCount` として記録し、Program provider-data には書かない。publish 済み Program には自 サービスの `skippedUnresolvedTransport=false` を入れる。

## provider-data schema / 署名

`Programs.COLUMN_INTERNAL_PROVIDER_DATA` の新規書き込み・読み取り正形式は UTF-8 JSON v1 バイト列のみとする。`programKeyB64`、`;` 区切り key-value 形式、旧 flat provider-data、旧 provider-data 断片は読み取り互換入力としても残さない。既存端末の旧形式データは r51 リリース物では移行対象にせず、DB 再構築または setup 再実行で JSON v1 を再生成する。

provider-data JSON v1 の構造、canonical encode、正規化、署名、安定キー抽出は `arib_si_engine_rs` の Rust `provider_data` module の `serde` struct を SSOT とする。TIS Kotlin は provider-data JSON を `JSONObject.put()` や手書き string concatenation で直接構築してはならない。TIS Kotlin は Rust JNI の build / 正規化 / 署名 / key extraction API で得た bytes と 署名 を TvProvider に書く。

Program provider-data の top-level envelope、必須フィールド、検証規則、canonical encode、正規化、署名、安定キー抽出は TIS では再定義しない。正本は `arib_si_engine_rs/DESIGN_JA.md`、`arib_si_engine_rs/schema/program_provider_data_v1.schema.json`、`arib_si_engine_rs/schema/descriptor_diagnostic_v1.schema.json`、`arib_si_engine_rs/testdata/program_provider_data_v1/minimal_clear_program.json` とする。TIS instrumentationテスト 用 テストデータは `tis/tests/assets/program_provider_data_v1/minimal_clear_program.json` に置き、Rust 側 テストデータと バイト単位で同一 に保つ。TIS は Rust JNI が返した provider-data bytes を保存し、新規 provider-data JSON を Kotlin 側独自 schema で構築してはならない。

`programKey` は `originalNetworkId / transportStreamId / serviceId / eventId` のみで構成する。`startUtcMillis`、`endUtcMillis`、`durationMillis` は `timing` に置き、stable key に含めない。event_id 不明の event を stable ARIB event として扱ってはならない。

TIS は `components.video[]`、`components.audio[]`、`components.subtitle[]`、`components.data[]` を provider-data schema として再定義しない。映像・音声 codecメタデータ、字幕 trackメタデータ、非対応 codec 診断は Rust JNI が返す JSON v1 バイト列の中に保存される。TIS は TvProvider 標準列、`TvTrackInfo`、MediaFormat / AudioTrack / 字幕表示経路へ接続する接着層に限定する。`audio` / `video` が `null` の場合は主track 未選択または未確定を意味し、空オブジェクト と同義に扱ってはならない。

Channel provider-data の top-level envelope は `schema="maleicacid.tv.channel"`, `schemaVersion=1`, `serviceKey`, `tune`, `cas`, `diagnostics` を持つ JSON v1 とする。`tune` は `inputId`、`displayName`、`deliverySystem`、`frequencyHz`、`streamId`、`streamIdType`、`physicalChannel`、`backendHint`、`satelliteBand`、`remoteControlKeyId` を持つ。CS110 は `streamIdType="NONE"` とし、`streamId` は null とする。

Program署名 は TvProvider に実際に書く `ContentValues` と Rust JNI が返した provider-data bytes から生成する。署名入力は固定 column list 順に `<columnName>\0<byteLength>\0<bytes>
` で連結し、そのバイト列の SHA-256 lowercase hex とする。`ContentValues` の iteration order には依存しない。insert 後に provider-data を再生成した場合、cache する signature は再生成後に実際に書いた バイト列の signature とする。

`selectedProgramId` のような TvProvider row id 依存値は stable provider-data 署名 の構成要素にしてはならない。必要な場合は 補助診断として扱い、署名skip 判定を壊さない。

## 現在番組 選択

現在番組 resolver は TvProvider query 時点で `START_TIME_UTC_MILLIS <= now AND END_TIME_UTC_MILLIS > now` に絞る。sort order は `START_TIME_UTC_MILLIS DESC, END_TIME_UTC_MILLIS ASC, _ID DESC` に固定する。overlap がある場合も cursor 返却順には依存せず、この selection rule で1件を選ぶ。

provider-data 診断情報は `diagnostics.currentProgram` 配下に保存する。`selectionRule` は `START_DESC_END_ASC_ID_DESC` とする。対象なしの場合は empty string とする。`overlapCount` と `selectedProgramId` は補助診断として扱い、stable 署名の意味上の identity にしない。ARIB `event_id` は `COLUMN_EVENT_ID` と JSON v1 `programKey.eventId` で扱う。

## CA descriptor / provider-data 直列化

CA_descriptor の raw bytes は Rust parser が元 section から保持し、JNI snapshot DTO に raw bytes として渡す。Kotlin 本番経路 code で CA_descriptor を再構築しない。malformed CA_descriptor は 元記述子 / CAS メタデータ から除外し、サービス自体は保持する。診断情報には `malformedCaDescriptorCount` と table/PID/サービス context を残す。Kotlin 側で修復して provider-data や CASメタデータに不正な元記述子を入れてはならない。

## transaction DTO / provider-data SSOT / executor / setup / retry の固定

### Rust JNI provider-data API

TIS Kotlin は provider-data JSON を解釈せず、以下の Rust JNI API 相当だけを使う。

```kotlin
object NativeProviderData {
    external fun buildProgramProviderData(inputJson: String): ProviderDataResult
    external fun normalizeProgramProviderData(rawBytes: ByteArray): ProviderDataResult
    external fun programProviderDataSignature(rawBytes: ByteArray): String
    external fun extractProgramKey(rawBytes: ByteArray): ProgramKeyResult?
}

data class ProviderDataResult(
    val bytes: ByteArray,
    val signature: String,
    val schemaVersion: Int,
    val truncated: Boolean,
    val diagnosticsDroppedCount: Int,
)
```

`inputJson` は Rust builder への入力 DTO であり、TvProvider に保存する provider-data schema ではない。最終JSONバイト列、署名、正規化、安定キー抽出は Rust が行う。

### 診断情報 schema

Descriptor診断 の機械検証規則は `arib_si_engine_rs/schema/descriptor_diagnostic_v1.schema.json` を正とする。TIS は `ProgramProviderDataV1.diagnostics.descriptorDiagnostics[]` 配下のオブジェクトを別 schema へ変換せず、Rust JNI が返した provider-data JSON 内の診断情報を保存する。未対応の視聴年齢制限は `ratings[]` に構造化値を残し、補足説明が必要な場合だけ `diagnostics.publishDiagnostics[]` に warning を追加する。TIS Kotlin は descriptor diagnostic JSON を独自生成しない。

### provider-data 保存上限

provider-data の soft limit / hard limit、診断情報・長文補助情報の切り詰め規則、切り詰め時の診断 key は `arib_si_engine_rs/DESIGN_JA.md` と Rust provider-data 実装を正とする。TIS は保存前に Rust JNI が返した bytes と 署名 をそのまま扱い、Kotlin 側で独自の切り詰め schema を定義しない。


### SectionEvent 入力上限

TIS の PSI/SI section path は allocation 前に `SectionEvent.dataLength` を検証する。`MAX_SECTION_BYTES` は 4096 bytes とし、`dataLength` が 1..4096 の範囲にある場合だけ ByteArray 確保と `AribSiEngine` / CAS / Program publish への投入を許可する。section read size 不一致、0 length、負値相当、4096 bytes 超過は parser に渡さず 診断カウンター に記録する。

### transaction DTO API

`AribSiEngine` 呼び出し側 は複数 snapshot を合成してはならない。本番経路 は以下の用途別 bulk DTO を使う。

```kotlin
data class ProgramPublishSnapshot(
    val snapshotGeneration: Long,
    val ingestSequence: Long,
    val events: List<AribEvent>,
    val updateWindows: List<EpgUpdateWindow>,
    val publishabilityByServiceKey: Map<ServiceKey, ProgramPublishability>,
    val descriptorDiagnostics: List<DescriptorDiagnostic>,
    val parserDiagnostics: List<ParserDiagnostic>,
)

fun takeProgramPublishSnapshot(): ProgramPublishSnapshot
```

```kotlin
data class ServiceRegistrationSnapshot(
    val snapshotGeneration: Long,
    val services: List<AribService>,
    val actualTransports: Set<TransportKey>,
    val publishabilityByServiceKey: Map<ServiceKey, ProgramPublishability>,
    val diagnostics: List<ParserDiagnostic>,
)

fun serviceRegistrationSnapshot(): ServiceRegistrationSnapshot
```

```kotlin
data class CasDiscoverySnapshot(
    val snapshotGeneration: Long,
    val services: List<AribService>,
    val caMetadata: List<CaMetadata>,
    val pmtPids: Map<ServiceKey, Int>,
    val catEmmPids: List<Int>,
    val diagnostics: List<DescriptorDiagnostic>,
)

fun casDiscoverySnapshot(): CasDiscoverySnapshot
```

`takeProgramPublishSnapshot()` は events / updateWindows / publishability / 診断情報を同一ロック / 同一 native state から取得し、updateWindows の drain もこの API 内だけで行う。`snapshotEvents()` と `takeEpgUpdateWindows()` を 本番経路 呼び出し側 で別々に呼ぶことは禁止する。

### LiveSession / PlaybackPipeline / Scan の直列化

`MaleicacidLiveSession` は session-level serial executor を持ち、current サービス、generation、playback 署名、track state、unblock state、latest video メタデータ、`ProgramPublishCoordinator` へのアクセスを同一 executor に閉じる。TunerController、PlaybackPipeline、parental receiver の コールバック は直接 state mutation せず、session executor に enqueue する。

`PlaybackPipeline` は playback-level serial executor を持ち、`setSurface()`、`setVolume()`、`start()`、`switchAudio()`、`stop()`、`release()` の state mutation を同一 executor に閉じる。filter / decoder / generation / surface / トークン の変更を呼び出し元スレッドで直接行わない。release 後の queued task は released flag と generation で破棄する。

`ChannelScanManager` は scan generation と purpose を持つ。cancel / cleanup task は対象 generation にだけ作用し、stale cleanup が後続 scan の `running`、controller、engine を変更してはならない。

### SetupActivity 保護

`SetupActivity.onCreate()` は scan を自動開始しない。scan 開始前に正規 setup flow の inputId が自 TIS の inputId と一致することを検証する。inputId 欠落または不一致時に 代替処理 inputId で scan へ進まない。scan は検証済みユーザー操作または検証済み setup request の後に開始する。

product 側で システムTVアプリ に grant 可能な場合、SetupActivity は 署名 / privileged permission で保護する。permission grant が成立しない target でも、自動 scan 禁止、inputId 検証、ユーザー操作開始は必須とする。

SetupActivity は自分が開始した `SETUP_SCAN` purpose かつ同一 scan generation の Completed だけで `RESULT_OK` にする。過去の Completed、boot EPG sync、background maintenance の Completed で finish してはならない。

### Direct Boot drain / ライブセッション 優先

`MaleicacidTvInputService.onCreate()` は Direct Boot pending drain、boot EPG sync、background maintenance を開始しない。Boot EPG sync / background maintenance は BootReceiver、UserUnlockReceiver、または明示的な maintenance scheduler からのみ起動する。

Boot EPG sync / background maintenance の開始条件は、`activeLiveSessionCount == 0`、`sessionCreationInProgress == false`、`setupScanRunning == false`、`playbackPipelineRunning == false`、`scanManager running == false` をすべて満たすこととする。ライブセッション 作成要求が来た時点で boot/background task が未開始なら defer する。boot/background task が既に running の場合、r51 では boot/background task を cancel/defer し ライブ tune を優先する。


## TIS コールバック 入力境界と逆圧

- `SectionEvent.dataLength` は、Tuner コールバック から読み取る section の正確な byte 長として扱う。
- TIS が section event として受け付ける長さは `1..4096` byte だけとする。`dataLength <= 0` は不正、`dataLength > 4096` は過大として、どちらも `ByteArray` 確保前に破棄し、PID 別診断に計上する。
- `MediaEvent` sample は `1 MiB` を上限とする。負の offset、0 以下の length、offset + length の overflow、LinearBlock 容量超過は sample 確保なしで破棄し、診断に計上する。
- decoder input-buffer の逆圧は無通知破棄ではない。sample は上限付き pending queue に保持し、後続 コールバック / drain で再試行する。sample を破棄するのは上限付き queue が満杯の場合だけとし、破棄 counter を加算する。

## provider-data / retry / attribution 境界の完了条件

### Provider-data SSOT

`TvContract.Channels/Programs.COLUMN_INTERNAL_PROVIDER_DATA` の新規書き込みは `arib_si_engine_rs` の provider-data JNI API が返す JSON v1 bytes をそのまま保存する。TIS Kotlin は TvProvider 標準列を詰める接着層であり、provider-data 本体、program stable key、descriptor 診断情報 schema、provider-data 署名 を独自 JSON schema として再構築してはならない。

Channel provider-data の新規書き込み・読み取り正形式は JSON v1 のみとする。`key=value;...` 形式、旧 flat provider-data、旧 provider-data 断片は読み取り互換入力としても残さない。JSON v1 は `schema="maleicacid.tv.channel"` / `schemaVersion=1` を持ち、channel tune 復元に必要な inputId、物理選局情報、ONID / TSID / service_id、表示名、登録可能性診断を Rust provider-data API 由来の構造として保存する。



### 旧 indexed JNI / 廃止経路の禁止

TIS は `nativeSnapshotBulkJson()` と provider-data JNI API を通常境界とする。`nativeGetEventCount()`、`nativeGetEvent*` indexed JNI getter、旧 event JSON `canonicalGenres` フィールド、互換専用の空返却シンボル、未使用 private external 宣言は残してはならない。旧経路を使う呼び出し不能コードや test-only 以外の廃止予定 path は、互換維持ではなく削除する。

### Program publish retry

Program publish retry queue は r51 では process-local とする。process death 後の retry 永続化は行わず、boot/background scan による再収集を正とする。ただし、process-local queue であっても retry key は `ServiceKey + updateWindow + failureClass`、entry は `attempt / nextAttemptAtMillis / firstFailureAtMillis / lastFailureAtMillis` を持ち、1/5/15/60分 backoff、決定的 jitter ±20%、最大10回、24時間 retention を適用する。

Provider 必須問い合わせ failure、Program insert/update failure、廃止行削除 failure、署名 build failure では 署名キャッシュ 更新と Direct Boot保留解除 に進まない。廃止行削除 は `deletionAuthoritative=true` の 更新区間 でのみ実行する。

### AttributionSource

`AttributionSource?` は `TvInputService.onCreateSession(..., AttributionSource)` から `MaleicacidLiveSession`、`TunerController`、`PlaybackPipeline` へ保持して渡す。対象 Android の `AudioTrack.Builder` が `setAttributionSource(AttributionSource)` を公開している場合は reflection を使わない直接呼び出しへ移行する。Android 14 system SDK 境界では compile visibility 差があるため、`PlaybackPipeline` は reflection による補助設定を行い、失敗時は警告 ログ に残して audio usage/content type/session 設定を継続する。


## 国内放送全般であり得る codec 固定表

r52 では、ARIB 資料上の国内放送全般であり得る codec を次の固定表として扱う。ここでの「扱う」は、PMT / component descriptor / 音声コンポーネントdescriptor / stream type / MMT asset メタデータ / codecメタデータを認識し、TvProvider / trackメタデータ / 診断情報へ正しく反映することを含む。transport profile や受信方式が本プロダクトの恒久対象外である場合は、codec を認識しても ライブ 視聴可能性 / 再生可能性 として 対応宣言しない。

### 参照した ARIB 資料と根拠

| 根拠資料 | 本改訂で固定する内容 |
|---|---|
| `ARIB/doc/2-STD-B32v3_7.pdf` 第1部 第3章 | 国内デジタル放送の映像符号化方式は MPEG-2 Video、MPEG-4 AVC、HEVC の3系統として扱う。 |
| `ARIB/doc/2-STD-B32v3_7.pdf` 第2部 第3章 | 国内デジタル放送の音声符号化方式は MPEG-2 AAC、MPEG-2 BC、MPEG-4 AAC、MPEG-4 ALS の4系統として扱う。 |
| `ARIB/doc/2-STD-B10v5_8.pdf` 第2部 表 6-5 / 6.2.26 / 付録 E | MPEG-2 系映像、H.264/AVC、H.265/HEVC、MPEG-2 Audio、AAC ADTS、MPEG-4 Audio LATM の signaling を認識対象にする。 |
| `ARIB/doc/2-STD-B60v1_7.pdf` MMT / asset signaling | ISDB-S3 / MMT 側では HEVC 映像、ISO/IEC 14496-3 音声、MPEG-4 AAC / MPEG-4 ALS を codecメタデータ として認識対象にする。ただし本プロダクトが ISDB-S3 / MMT / TLV を恒久対象外とする場合、ライブ 視聴可能性 / 再生可能性 には入れない。 |
| `ARIB/doc/2-STD-B59v2_0.pdf` | 22.2 ch 等の音響チャンネル構成の参照資料であり、放送 bitstream codec としては B32 / B60 の MPEG-4 AAC / MPEG-4 ALS 側で扱う。MPEG-H 3D Audio codec を本表へ追加する根拠にはしない。 |

### video codec

| codec | r52 の扱い |
|---|---|
| MPEG-2 Video | 必須対応。PMT / component descriptor から codec、解像度、走査方式、aspect を認識し、MediaFormat、decoder 起動、first-frame gate、unsupported 診断情報を固定する。 |
| H.264 / MPEG-4 AVC | 必須対応。profile / level は AVC video descriptor と実 MediaCodec capability を照合し、未対応時は codec unsupported 診断に落とす。 |
| H.265 / HEVC | codec として認識対象。対象 transport profile を本プロダクトが 対応宣言しない場合は ライブ viewable capability に入れない。対応する場合は MediaFormat / decoder / first-frame gate まで必須。 |

ISO/IEC 14496-2 Visual、JPEG 2000、auxiliary video、SVC、MVC、3D additional view は、今回の ISDB-T/S product scope の ライブ viewable codec として 対応宣言しない。必要なら provider-data / 診断情報に保持する。

### audio codec

| codec | r52 の扱い |
|---|---|
| MPEG-2 AAC | 必須対応。ADTS / MPEG-2 AAC LC、channel count、sample rate、ISO639 language、main/sub、dual mono、音声モード、音質表示を保持する。 |
| MPEG-2 BC Audio | 認識対象。decoder が利用できる場合だけ再生対応を 対応宣言 し、未対応時は video-only 診断に落とす。 |
| MPEG-4 AAC / HE-AAC | 必須認識。AAC LC / HE-AAC profile、LATM/LOAS / ADTS、AudioSpecificConfig、channel count、sample rate を保持する。decoder が利用できる場合だけ再生対応を 対応宣言する。 |
| MPEG-4 ALS | codec として認識対象。対象 transport profile を本プロダクトが 対応宣言しない場合は playable capability に入れない。対応する場合は decoder / AudioTrack / メタデータ / unsupported 診断情報 まで必須。 |

AC-3、Enhanced AC-3、MPEG-H 3D Audio、DTS、DTS-HD、Dolby TrueHD は、今回確認した ARIB 資料群では国内デジタル放送の対象 codec として固定する根拠を確認できないため、r52 codec 固定表には含めない。

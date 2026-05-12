# TIS 設計判断

## AOSP 標準経路

TIS は `TvInputService` として system TV app から呼ばれ、Tuner HAL には Tuner SDK API 経由でアクセスする。HAL binder を直接呼ばない。

## BS と CS110 の選局契約

BS は IF 周波数と typed stream selector を保持し、selector は TSID または px4 専用の相対 TS 番号として保存する。earth_pt1 は TSID のみ許容し、px4 は TSID と相対 TS 番号を許容する。CS110 は周波数帯のみで scan candidate と tune selector を作り、stream selector を保存しない。

CS110 tune request 生成時、TIS は Android Tuner API builder の default `streamId` / `streamIdType` に依存しない。CS110 では frontend stream selector を明示的に none / `UNDEFINED` 相当に設定する。CS110 の ONID / TSID / service_id は channel identity / service identity として保持してよいが、HAL frontend selector へ転用してはならない。BS は IF 周波数 + TSID、または px4 backend 限定の relative stream number を使う。

TvProvider の channel internal provider data には `streamSelectorType` と `streamSelectorValue` を分けて保存する。`NONE` は値なし、`TSID` は 0..0xffff、`RELATIVE` は 0..7 とする。


## 製品 scan 候補表の保持者

TIS は製品 scan 候補表の実装データ保持者である。選局対象、対象周波数帯、BS/CS110 selector 境界、CATV 候補範囲の設計契約は tv 直下の開発規則.mdを正とする。

TIS が保持する候補表の具体値は製品 scan 実装データのSSOTである。ただし、選局対象範囲、VHF除外、CATV C13〜C63限定、BS/CS110 selector 境界などの設計契約は tv 直下の `開発規則.md` を正とする。TIS の候補表は `開発規則.md` の設計契約に反してはならない。

TIS 以外の文書や実装に同等の scan 候補表を重複保持してはならない。Tuner HAL に渡す値は、TIS が生成した explicit tune candidate に限定する。

TIS は地上UHF、CATV、BS、CS110の候補を持ち、Tuner HALには explicit tune candidate として渡す。Tuner HAL は日本向け scan 候補表を自前生成しない。

CATV候補表は C13〜C63 に固定する。MID band は C13〜C22、SHB band は C23〜C63 とし、中心周波数は ARIB STD-B21 Appendix 10 の `+1/7 MHz` オフセット込みで保持する。C22 は `167 + 1/7 MHz`、C23 は `225 + 1/7 MHz` であり、C21からC22、C22からC23は単純な6MHz連続として計算しない。

VHF 1〜12ch は開発規則.mdで恒久的にスコープ外であり、TISのCATV候補表、地上波候補表、共同受信候補表に追加してはならない。

BSの通常実行時候補生成はTISが持つBS TSID表だけを正とする。px4向けの相対TS番号候補は診断またはbackend指定候補としてのみ許可し、earth_pt1向けには使わない。px4 backend側にTSIDからlegacy slotへの同等表または変換表を持つ場合でも、TISのBS TSID表と不一致になってはならない。

## CAS / descrambler の r51 境界

r51 では CAS HAL 本体はプレースホルダーのままにする。TIS は Tuner SDK API の filter 経由で PMT/CAT/SDT/ECM/EMM section payload を取得し、PMT/CAT から得た CA_descriptor と SDT 等から得た free_CA_mode / service identity 補助情報を arib_si_engine_rs / TIS 側で CA metadata / service metadata semantic model に変換する。TIS はその CA metadata に基づいて ECM/EMM section filter と MediaCas/CAS bridge を型付き API で制御し、実 key token が得られた場合だけ Tuner descrambler へ不透明な参照値を渡す。placeholder や診断専用結果は復号成功を意味しないため、`setKeyToken()` へ渡さない。Tuner HAL が未接続診断を返した場合も成功扱いにしない。

## Tuner SDK API 呼び出し

`openDescrambler()`、`setKeyToken()`、`addPid()`、`removePid()` は reflection を使わず、対象 build の system/privileged API として直接呼ぶ。API が利用できない build は r51 対象外とする。

## 再生経路

r51 では MediaCodec block model の reflection fallback を禁止する。対象 build で型付き block model を安定利用できない場合は、成功を偽装せず `notifyVideoUnavailable()` へ落とす。`notifyVideoAvailable()` は `Filter.start()` 成功、汎用 `FilterEvent` 到着、payload 付き `MediaEvent` 到着だけでは呼ばない。ES header から decoder 構成に必要な情報を抽出し、MediaCodec output frame が Surface へ render された callback を映像到達 gate として呼ぶ。

## EIT と TvProvider

r51 は EIT p/f を中心に使う。scan/setup 後に `TvProvider.Programs` へ出す最低限の Programs だけ schedule から拾う。Programs の `internal_provider_data` には JSON v1 の stable `programKey`、timing、CAS state、extended item、component/audio/series text、診断 JSON を TIS 内部データとして保存する。TvProvider の標準 column には title / short description / long description として自然に入る範囲だけ反映する。

## 字幕表示の責務

ARIB 字幕は TIS 側の字幕 path で `libaribcaption` を使用する。`arib_si_engine_rs` の自前 ARIB 文字列 decoder はサービス名・番組名・番組説明など字幕以外の SI/EPG 文字列に限定し、字幕 PES や字幕本文をその decoder に渡さない。libaribcaption は C API のみを使用し、独自 C/C++ shim は書かない。Kotlin から直接 C API を呼ばず、TIS Kotlin → Rust JNI boundary → safe Rust wrapper → libaribcaption C API の順に接続する。

## r51 live playback 実装方式

TIS の live playback は、案Aだけを採用する。すなわち、Tuner AV filter の `MediaEvent` を clear ES payload として受け取り、video は `MediaCodec` へ投入して `Surface` へ出力し、audio は `MediaCodec` で PCM 化して `AudioTrack` へ流す。

`tunneled` / platform passthrough playback path は r51 の設計候補から外し、実装しない。`notifyVideoAvailable()` は `Filter.start()` 成功、汎用 `FilterEvent`、payload 付き `MediaEvent` 到着だけでは呼ばない。video decoder が起動し、最初の decoded frame が `Surface` へ render された callback を gate とする。

setup scan の channel registration は global discovery complete を必須条件にしない。ただし partial snapshot を無条件に channel insert に使ってはならない。TvProvider 登録には service-local registration-ready gate を使う。registration-ready は、ONID / TSID / SID が確定し、channel URI から物理 tune key に戻せ、PMT PID と PMT、PCR PID、r51対応 video ES が取得済みで、audio は対応済みまたは video-only として診断可能であり、service name は正式名または deterministic な仮名と後続更新方針を持ち、clear live claimable または scrambled unsupported として状態通知可能な service に限定する。registration-ready 未満の partial snapshot は diagnostics / live refresh / debug にのみ使い、channel insert しない。scrambled service は channel 登録してよいが、CAS placeholder のまま clear live 視聴成功 claim してはならない。

## codec header / A-V sync / publish mode の固定

live playback の codec 構成は、video は MPEG-2 video と H.264/AVC、audio は AAC と MPEG audio を r51 対象 codec とする。

r51 対応 video ES が存在しない service は viewable としない。HEVC など r51 未対応 video codec のみを持つ service は、再生不能として `notifyVideoUnavailable()` へ落とす。

r51 対応 video ES が存在し、audio ES が存在しない、または audio codec だけが r51 未対応の場合は、video-only service として視聴可能にする。この場合、`notifyVideoUnavailable()` には落とさず、`audio absent` または `unsupported audio codec` を診断に残す。AC-3 等の未対応 audio codec は、対応 video が存在する限り video-only 診断の対象であり、video unavailable の直接理由にしてはならない。

decoder は PMT の stream_type だけでは構成せず、MediaEvent payload から MPEG-2 sequence header、H.264 SPS/PPS、AAC ADTS/AudioSpecificConfig、MPEG audio header を検出してから MediaFormat を構成する。

MediaEvent payload は、`offset >= 0`、`dataLength > 0`、`offset + dataLength <= mapped buffer capacity`、`dataLength <= MAX_MEDIA_SAMPLE_BYTES` を満たす場合だけ decoder queue に渡す。`MAX_MEDIA_SAMPLE_BYTES` は r51 では 4 MiB hard limit とする。範囲不正は playback pipeline 例外ではなく sample drop と diagnostic counter として扱う。

A/V同期方式は r51 で non-tunneled clear playback に固定する。tunneled playback と avSyncHwId は r51 範囲外であり、TIS の non-tunneled playback では avSyncHwId を使わないが、Tuner HAL API としては r51 で実装・AOSP準拠に仕様を固定する。video/audio は MediaCodec と AudioTrack の PTS により同期する。audio が存在する場合は AudioTrack を master clock とする。video-only service は視聴可能として扱い、audio が存在しない場合は `audio absent`、audio codec だけが未対応の場合は `unsupported audio codec` を診断に残す。

TvProvider公開モードは `PublishMode` で channel row 追加を setup scan / explicit rescan に限定する。live tune refresh、boot EPG sync、background channel maintenance では既存 channel の番組・診断更新だけを許可し、新規 channel row は追加しない。

## ARIB SI/EPG のTvProvider投影

ARIB SI/EPG の標準列投影は tv直下の `ARIB_SI_EPG_TvProvider投影方針.md` を正とし、`internal_provider_data` の具体 schema / canonical encode / signature は `arib_si_engine_rs` の Rust provider-data serde model を SSOT とする。TISは、同文書で標準列投影が固定された項目だけを `Programs.COLUMN_LONG_DESCRIPTION` 等へ出し、r51 標準列投影対象外の項目は JSON v1 `internal_provider_data` のみに構造化保存する。

`Programs.COLUMN_CANONICAL_GENRE` については、TIS が直接設定する値と、Android TvProvider が `Programs.COLUMN_BROADCAST_GENRE` から内部補完した読み出し結果を区別する。r51 では ARIB→Android canonical genre 写像表を採用対象外とするため、TIS は `Programs.COLUMN_CANONICAL_GENRE` を `ContentValues` に直接設定しない。ただし TvProvider 読み出し後に canonical genre が非空になることは、AOSP 標準の内部補完として許容する。

`Programs.COLUMN_BROADCAST_GENRE` には、`arib_si_engine_rs` から受け取った ARIB content_descriptor の分類値とARIB表示名を、TIS が `TvContract.Programs.Genres.encode(...)` 形式で格納する。TIS は ARIB分類を Android canonical genre に変換しない。

## parental control / content rating 契約

TIS は `arib_si_engine_rs` から受け取った `parental_rating_descriptor` の構造化データを、AOSP system-defined ISDB rating domain（`com.android.tv / ISDB / ISDB_<age>`）の `TvContentRating` へ変換する。Android `TvContentRating` の domain / ratingSystem / rating 文字列は TIS 側で固定し、Rust 側のSSOTにしない。

TvProvider へ番組を登録または更新する場合、変換できる rating は `TvContentRating.flattenToString()` の結果を `Programs.COLUMN_CONTENT_RATING` に格納する。変換できない rating は推測で `COLUMN_CONTENT_RATING` に入れず、`internal_provider_data` と診断に保持する。

live session は、現在番組の rating と system parental control 設定を同期して扱う。`TvInputManager.isParentalControlsEnabled()` が true の場合、TIS は現在番組の `TvContentRating`、または rating 未取得時の `TvContentRating.UNRATED` を `TvInputManager.isRatingBlocked(...)` に渡して判定する。blocked の場合は video frame を表示する前に再生を停止または抑止し、`notifyContentBlocked(rating)` を呼ぶ。許可された場合は `notifyContentAllowed()` を呼ぶ。

TIS は `TvInputManager.ACTION_BLOCKED_RATINGS_CHANGED` と `TvInputManager.ACTION_PARENTAL_CONTROLS_ENABLED_CHANGED` を監視し、設定変更時に現在番組の parental control 判定を即時再評価する。

## r50bb7 r51 TIS/arib_si_engine_rs 固定事項

- Android 14 系の通常 live session 生成では `onCreateSession(inputId, sessionId)` を実装し、framework 由来 `sessionId` を `Tuner(context, sessionId, useCase)` へ渡す。旧1引数 overload の fallback sessionId は互換経路専用とする。
- r51 の video claim 対象は MPEG-2 video `0x02` と H.264/AVC `0x1b` に限定する。HEVC `0x24` は r51 clear live playback / playback selection 対象外であり、診断上は `NO_SUPPORTED_VIDEO_ES` 相当として扱う。
- ARIB parental rating は Android `TvContentRating` へ `domain=com.android.tv`, `ratingSystem=ISDB`, `rating=ISDB_<age>` として写像する。対応範囲は JPN かつ rating 4..20 のみとし、未対応 country / rating は推測変換せず `internal_provider_data` / 診断に残す。rating 未取得時は `TvContentRating.UNRATED` として parental control 判定する。
- `notifyVideoAvailable()` は decoder の first frame callback が現行 playback generation と一致し、かつ parental control で block されていない場合だけ呼ぶ。旧 decoder callback は無視する。
- live tune refresh では新規 channel row を作らず、既存 channel の program 更新だけを行う。setup/rescan のみ channel row を作成できる。
- H.264 は SPS/PPS 検出だけでなく SPS 由来の width / height を MediaFormat へ反映する。SPS 解析不能時は固定 1920x1080 fallback で成功扱いしない。
- PMT 由来の video/audio track は `TvTrackInfo` として通知し、`onSelectTrack(TYPE_AUDIO, trackId)` で audio track 切替を受ける。r51 では別 video track / subtitle / data track 選択は false を返す。
- CS110 は stream selector `NONE` のみ許可し、TSID / relative selector を HAL tune request へ渡さない。Android Tuner builder では NONE 時に selector setter を呼ばない。
- boot 後 EPG 再同期は既存 channel の p/f 最小更新に限定し、新規 channel row は作成しない。`JapanIsdbScanPlan.defaultInitialScan()` は setup scan / explicit rescan 専用であり、boot EPG sync の既定候補に使わない。
- background channel maintenance は r51 スコープ内の必須実装とする。ただし boot critical path から分離し、boot EPG sync 完了後または明示的保守タイミングで実行開始を試行する。実行開始は scan/maintenance が未実行で、かつ live session が存在しない場合に限る。active live session または scan 実行中の場合は開始せず、skip 理由を diagnostics に残す。対象は既存 channel と既存 transport metadata refresh までに限定し、新規 channel insert は行わない。
- section filter は CRC protected section で `setCrcEnabled(true)` を使用し、Rust 側 CRC 検査を defense-in-depth として維持する。TIS 側には PID / table / status 別 counter を持つ。


## r50bi parental rating / CAS fallback 固定

- `Programs.COLUMN_CONTENT_RATING` と Live session の parental control 判定は同じ `AribRatingMapper` を使い、`com.android.tv / ISDB / ISDB_4..20` の AOSP system-defined rating に統一する。
- TIS は custom rating-system XML / receiver を追加しない。product 統合時は system TV app / rating definitions に `com.android.tv / ISDB / ISDB_4..20` が存在することを確認する。
- Live session は current program rating を `TvProvider current Program -> latest EIT cache -> TvContentRating.UNRATED` の順で解決する。
- parental blocked の通知は `notifyContentBlocked(rating)` と AV停止を主とし、parental block の通知手段として `notifyVideoUnavailable()` を呼ばない。
- `onUnblockContent()` の解除範囲は同一 `channelUri + serviceKey + eventId + ratingString` の current program / rating に限定する。start/end は stable identity ではなく、解除対象が現在表示中の同一 Program row であることを確認する補助条件としてのみ使ってよい。start/end/duration を provider-data `programKey`、unblock stable identity、または Program identity の SSOT にしてはならない。
- CAS 未完成 / scrambled unsupported で playback success にしない場合は `TvInputManager.VIDEO_UNAVAILABLE_REASON_CAS_UNKNOWN` を使う。具体的な CAS 状態 reason は CAS HAL 本実装まで使わない。
- Programs CAS 状態は current complete diagnostic を優先し、不完全または欠落 diagnostic では既存 channel `internal_provider_data` の `requiresCas` / `unsupportedCas` / `clearLivePlaybackSupported` / `channelRegistrationReady` / `epgPublishable` を fallback する。

## r50bi5 TIS / EPG publish boundary

r51 の EIT publish/delete 対象は、TvProvider に channel が存在する `ServiceKey`、または同一 setup/rescan transaction で channel insert が成功して channelId が確定した `ServiceKey` に限定する。live session の `currentService` だけには限定しない。Program row を持たない service へ Programs を publish/delete してはならない。

r51 の EIT 対象 table は、present/following actual `0x4E`、present/following other `0x4F`、schedule actual `0x50..0x5F` のうち、上記対象 service に属する event とする。schedule other `0x60..0x6F` は r51 Programs publish/delete 対象外であり、update window を発生させない。

EIT 更新時の update/delete window は、追加・変更・削除された event の旧 `[start,end)` と新 `[start,end)` の union とする。長期固定 lookahead window は導入しない。EIT table scope の version 変更で旧 section が消えた場合は、消えた event の旧 window も obsolete delete 対象に含める。

ただし、obsolete delete の根拠にできる EIT section / table snapshot は Rust parser が `deletionAuthoritative=true` と判定したものに限る。start_time BCD、duration BCD、event descriptor_loop_length、event fixed field が malformed の event を含む section は、旧 event 削除用の authoritative valid-event-set として扱わない。malformed event は旧正常 Program を消す根拠にせず、DescriptorDiagnosticV1 / ParserDiagnosticV1 に記録する。

boot EPG sync の pending clear は boot EPG sync task 単位で判定する。task 中に cancel request を観測した場合は、成功 candidate があっても pending を clear しない。成功 candidate は `collectSiForCandidate()` が `COMPLETE` で、かつ registration-ready service が1件以上存在する candidate とする。background maintenance は pending clear を扱わない。

registration-ready service は、`ServiceKey`、物理選局情報、inputId へ戻せる channel provider data、display name が揃い、TvProvider channel insert/update に進める service とする。display name は `ChannelRecord.displayName` が nonblank ならそれを使い、なければ SDT service_name、さらに無ければ `service-<onid>-<tsid>-<sid>` を使う。この fallback 名は registration-ready 判定上の有効な display name と扱う。

## r50bi5 CAS placeholder boundary

CAS HAL placeholder のまま scrambled service を clear live playback success として扱ってはならない。scrambled unsupported service でも、PMT/CAT/CA metadata と診断を使って EPG / Programs / rating / provider-data は更新する。ただし CAS key token を提供できない状態では playback success にせず、CAS 起因の unavailable のみ `VIDEO_UNAVAILABLE_REASON_CAS_UNKNOWN` へ map する。first-frame timeout、filter start failure、unsupported stream、codec failure、audio failure は CAS unknown に map しない。

CAS provider-data は current diagnostic を優先する。`caStateResolved=true` で CAS required / unsupported / clearLivePlaybackSupported が判断できる場合は、`freeCaModeResolved=false` でも current diagnostic を採用する。fallback diagnostic を使った場合は `publishStateSource=fallback` とし、採用可能な状態がない場合は `publishStateSource=none` とする。

Descrambler API の `setKeyToken()`、`addPid()`、`removePid()` は戻り値が `Tuner.RESULT_SUCCESS` の場合だけ成功とする。非 SUCCESS result は CAS diagnostic failure として扱い、成功扱いで握り潰してはならない。

## r50bi5 TvProvider failure semantics

TvProvider query failure と channel なしは別状態として扱う。既存 channel query が失敗した場合は `skippedNoChannel` として扱わず、failure diagnostic とし、signature 更新・pending clear の根拠に使わない。

TvProvider query は required query と optional query を区別する。channel / program upsert、obsolete delete、existing channel/program lookup、provider-data fallback、Direct Boot ready 判定に使う query は required query とする。required query で `ContentResolver.query()` が null cursor を返した場合は `TvProviderQueryFailure` とし、empty result とみなさない。`TvProviderQueryFailure` が発生した service/window では channel insert、program insert/update、obsolete delete、signature cache update、Direct Boot pending clear に進まず、retry window を保持する。

Programs publish/delete が provider failure になった場合は、`ProgramPublishCoordinator` の process-local retry queue に `ServiceKey + updateWindow + failureClass` を key として enqueue する。backoff は 1分、5分、15分、60分、以後最大60分、jitter ±20%、最大10回、保持期間24時間または次回正常 snapshot までとする。次回 `publishLiveProgramsForCurrentService()`、boot EPG sync、background maintenance の publish entrypoint 先頭で retry queue を drain する。成功した key は削除し、失敗した key は保持する。process restart では retry queue を破棄し、boot/background sync による再収集を正とする。provider failure 時は obsolete delete、signature update、pending clear に進まない。

retry queue は全体上限 512 windows、ServiceKey ごと上限 32 windows とする。超過時は古い順に破棄し、ServiceKey 別 `droppedRetryWindowCount` を加算する。process restart 後は counter を 0 に戻す。

SDT-other / NIT-other / BAT 由来で現在 candidate の actual transport に解決できない service は、現在 candidate の物理情報で channel insert しない。未登録で Program row が存在しない unresolved transport は scan/maintenance diagnostics に `skippedUnresolvedTransportCount` として記録し、Program provider-data には書かない。publish 済み Program には自 service の `skippedUnresolvedTransport=false` を入れる。

## r50bj provider-data schema / signature

r50bj 以降、`Programs.COLUMN_INTERNAL_PROVIDER_DATA` の新規書き込み正形式は UTF-8 JSON v1 bytes のみとする。旧 `programKeyB64` や `;` 区切り key-value 形式は legacy input として読み取り移行に限り許可し、新規書き込みは禁止する。

provider-data JSON v1 の構造、canonical encode、normalize、signature、stable key extraction は `arib_si_engine_rs` の Rust `provider_data` module の `serde` struct を SSOT とする。TIS Kotlin は provider-data JSON を `JSONObject.put()` や手書き string concatenation で直接構築してはならない。TIS Kotlin は Rust JNI の build / normalize / signature / key extraction API で得た bytes と signature を TvProvider に書く。

Program provider-data の top-level envelope は次を必須とする。

```json
{
  "schema": "maleicacid.tv.program",
  "schemaVersion": 1,
  "programKey": {
    "kind": "arib-event-v1",
    "originalNetworkId": 4,
    "transportStreamId": 16400,
    "serviceId": 101,
    "eventId": 12345
  },
  "serviceKey": {
    "originalNetworkId": 4,
    "transportStreamId": 16400,
    "serviceId": 101
  },
  "timing": {
    "startUtcMillis": 1730000000000,
    "endUtcMillis": 1730001800000,
    "durationMillis": 1800000
  },
  "source": {
    "tableId": 78,
    "version": 12,
    "sectionNumber": 0,
    "lastSectionNumber": 1
  },
  "cas": {
    "requiresCas": false,
    "unsupportedCas": false,
    "clearLivePlaybackSupported": true,
    "source": "CURRENT_DIAGNOSTIC"
  },
  "ratings": [],
  "genres": [],
  "audio": {},
  "video": {},
  "diagnostics": {
    "descriptorDiagnostics": [],
    "publishDiagnostics": [],
    "parserDiagnostics": []
  }
}
```

`programKey` は `originalNetworkId / transportStreamId / serviceId / eventId` のみで構成する。`startUtcMillis`、`endUtcMillis`、`durationMillis` は `timing` に置き、stable key に含めない。event_id 不明の event を stable ARIB event として扱ってはならない。

Channel provider-data の top-level envelope は `schema="maleicacid.tv.channel"`, `schemaVersion=1`, `serviceKey`, `tune`, `cas`, `diagnostics` を持つ JSON v1 とする。`tune` は `deliverySystem`, `frequencyHz`, `streamId`, `streamIdType` を持ち、CS110 は `streamIdType="NONE"` とし selector value を持たない。

Program signature は TvProvider に実際に書く `ContentValues` と Rust JNI が返した provider-data bytes から生成する。signature 入力は固定 column list 順に `<columnName>\0<byteLength>\0<bytes>
` で連結し、その byte列の SHA-256 lowercase hex とする。`ContentValues` の iteration order には依存しない。insert 後に provider-data を再生成した場合、cache する signature は再生成後に実際に書いた bytes の signature とする。

`selectedProgramId` のような TvProvider row id 依存値は stable provider-data signature の構成要素にしてはならない。必要な場合は best-effort diagnostic として扱い、signature skip 判定を壊さない。

## r50bi5 current program selection

current program resolver は TvProvider query 時点で `START_TIME_UTC_MILLIS <= now AND END_TIME_UTC_MILLIS > now` に絞る。sort order は `START_TIME_UTC_MILLIS DESC, END_TIME_UTC_MILLIS ASC, _ID DESC` に固定する。overlap がある場合も cursor 返却順には依存せず、この selection rule で1件を選ぶ。

provider-data diagnostics の `selectionRule` は `START_DESC_END_ASC_ID_DESC` とする。対象なしの場合は empty string とする。`selectedProgramId` は best-effort diagnostic として扱い、stable signature の構成要素にしない。ARIB `event_id` は `COLUMN_EVENT_ID` と JSON v1 `programKey.eventId` で扱う。

## r50bi5 CA descriptor / provider-data serialization

CA_descriptor の raw bytes は Rust parser が元 section から保持し、JNI snapshot DTO に raw bytes として渡す。Kotlin production code で CA_descriptor を再構築しない。malformed CA_descriptor は raw descriptor / CAS metadata から除外し、service 自体は保持する。diagnostics には `malformedCaDescriptorCount` と table/PID/service context を残す。Kotlin 側で修復して provider-data や CAS metadata に不正 raw descriptor を入れてはならない。

## r50bj transaction DTO / provider-data SSOT / executor / setup / retry 固定

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

`inputJson` は Rust builder への入力 DTO であり、TvProvider に保存する provider-data schema ではない。最終 JSON bytes、signature、normalization、stable key extraction は Rust が行う。

### diagnostics schema

Descriptor diagnostic は次の JSON object schema に統一する。Kotlin はこの object を別 schema に変換してはならない。

```json
{
  "schema": "maleicacid.tv.descriptorDiagnostic",
  "schemaVersion": 1,
  "severity": "ERROR",
  "code": "MALFORMED_DESCRIPTOR_LENGTH",
  "scope": {
    "pid": 18,
    "tableId": 78,
    "tableIdExtension": 101,
    "version": 12,
    "sectionNumber": 0,
    "originalNetworkId": 4,
    "transportStreamId": 16400,
    "serviceId": 101,
    "eventId": 12345
  },
  "descriptor": {
    "tag": 77,
    "name": "short_event_descriptor",
    "offset": 42,
    "declaredLength": 25,
    "actualRemainingLength": 10,
    "rawPrefixHex": "4d19"
  },
  "message": "descriptor length exceeds remaining section bytes"
}
```

必須 key は `schema`, `schemaVersion`, `severity`, `code`, `scope`, `descriptor`, `message` とする。`scope` の不明数値は `null` または key omission とし、`-1` に潰して semantic loss させない。`descriptor.rawPrefixHex` は最大 64 bytes 相当までとする。

### provider-data 保存上限

provider-data 全体は 16 KiB soft limit、32 KiB hard limit とする。hard limit 超過時は identity / timing / CAS state / ratings を保持し、diagnostics と長文補助情報を truncate する。descriptor diagnostics / publish diagnostics はそれぞれ program あたり最大16件、diagnostic message は最大256 chars、extendedItems は最大32件、long description は最大4096 chars、一般文字列 field は最大1024 chars とする。truncate 時は `DIAGNOSTICS_TRUNCATED` または `PROVIDER_DATA_TRUNCATED` を保存する。


### SectionEvent 入力上限

TIS の PSI/SI section path は allocation 前に `SectionEvent.dataLength` を検証する。`MAX_SECTION_BYTES` は 4096 bytes とし、`dataLength` が 1..4096 の範囲にある場合だけ ByteArray 確保と `AribSiEngine` / CAS / Program publish への投入を許可する。section read size 不一致、0 length、負値相当、4096 bytes 超過は parser に渡さず diagnostic counter に記録する。

### transaction DTO API

`AribSiEngine` call-site は複数 snapshot を合成してはならない。production path は以下の用途別 bulk DTO を使う。

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

`takeProgramPublishSnapshot()` は events / updateWindows / publishability / diagnostics を同一 lock / 同一 native state から取得し、updateWindows の drain もこの API 内だけで行う。`snapshotEvents()` と `takeEpgUpdateWindows()` を production call-site で別々に呼ぶことは禁止する。

### LiveSession / PlaybackPipeline / Scan の直列化

`MaleicacidLiveSession` は session-level serial executor を持ち、current service、generation、playback signature、track state、unblock state、latest video metadata、`ProgramPublishCoordinator` へのアクセスを同一 executor に閉じる。TunerController、PlaybackPipeline、parental receiver の callback は直接 state mutation せず、session executor に enqueue する。

`PlaybackPipeline` は playback-level serial executor を持ち、`setSurface()`、`setVolume()`、`start()`、`switchAudio()`、`stop()`、`release()` の state mutation を同一 executor に閉じる。filter / decoder / generation / surface / token の変更を呼び出し元スレッドで直接行わない。release 後の queued task は released flag と generation で破棄する。

`ChannelScanManager` は scan generation と purpose を持つ。cancel / cleanup task は対象 generation にだけ作用し、stale cleanup が後続 scan の `running`、controller、engine を変更してはならない。

### SetupActivity 保護

`SetupActivity.onCreate()` は scan を自動開始しない。scan 開始前に正規 setup flow の inputId が自 TIS の inputId と一致することを検証する。inputId 欠落または不一致時に fallback inputId で scan へ進まない。scan は検証済みユーザー操作または検証済み setup request の後に開始する。

product 側で system TV app に grant 可能な場合、SetupActivity は signature / privileged permission で保護する。permission grant が成立しない target でも、自動 scan 禁止、inputId 検証、ユーザー操作開始は必須とする。

SetupActivity は自分が開始した `SETUP_SCAN` purpose かつ同一 scan generation の Completed だけで `RESULT_OK` にする。過去の Completed、boot EPG sync、background maintenance の Completed で finish してはならない。

### Direct Boot drain / live session 優先

`MaleicacidTvInputService.onCreate()` は Direct Boot pending drain、boot EPG sync、background maintenance を開始しない。Boot EPG sync / background maintenance は BootReceiver、UserUnlockReceiver、または明示的な maintenance scheduler からのみ起動する。

Boot EPG sync / background maintenance の開始条件は、`activeLiveSessionCount == 0`、`sessionCreationInProgress == false`、`setupScanRunning == false`、`playbackPipelineRunning == false`、`scanManager running == false` をすべて満たすこととする。live session 作成要求が来た時点で boot/background task が未開始なら defer する。boot/background task が既に running の場合、r51 では boot/background task を cancel/defer し live tune を優先する。


## r50bk6: TIS callback input bounds and backpressure

- `SectionEvent.dataLength` is treated as the exact section byte length to read from the Tuner callback.
- TIS accepts only `1..4096` bytes for a section event. `dataLength <= 0` is malformed and `dataLength > 4096` is oversized. Both are dropped before `ByteArray` allocation and counted in PID-scoped diagnostics.
- `MediaEvent` samples are bounded to `1 MiB`. Negative offset, non-positive length, offset+length overflow, and LinearBlock capacity overrun are dropped without sample allocation and counted as diagnostics.
- Decoder input-buffer backpressure is not a silent drop. Samples are held in a bounded pending queue and retried on later callback/drain. A sample is dropped only when the bounded queue is full, and the drop counter is incremented.

## r50bk8 completion: provider-data / retry / attribution boundary

### Provider-data SSOT

`TvContract.Channels/Programs.COLUMN_INTERNAL_PROVIDER_DATA` の新規書き込みは `arib_si_engine_rs` の provider-data JNI API が返す JSON v1 bytes をそのまま保存する。TIS Kotlin は TvProvider 標準列を詰める接着層であり、provider-data 本体、program stable key、descriptor diagnostics schema、provider-data signature を独自 JSON schema として再構築してはならない。

Channel provider-data の旧 `key=value;...` 形式は read-only legacy fallback である。新規 channel row は JSON v1 のみを書き込む。

### Program publish retry

Program publish retry queue は r51 では process-local とする。process death 後の retry 永続化は行わず、boot/background scan による再収集を正とする。ただし、process-local queue であっても retry key は `ServiceKey + updateWindow + failureClass`、entry は `attempt / nextAttemptAtMillis / firstFailureAtMillis / lastFailureAtMillis` を持ち、1/5/15/60分 backoff、決定的 jitter ±20%、最大10回、24時間 retention を適用する。

Provider required query failure、Program insert/update failure、obsolete delete failure、signature build failure では signature cache 更新と Direct Boot pending clear に進まない。obsolete delete は `deletionAuthoritative=true` の update window でのみ実行する。

### AttributionSource

`AttributionSource?` は `TvInputService.onCreateSession(..., AttributionSource)` から `MaleicacidLiveSession`、`TunerController`、`PlaybackPipeline` へ保持して渡す。Android target の `AudioTrack.Builder` が `setAttributionSource(AttributionSource)` を公開している場合は reflection-free 直接呼び出しへ移行する。r50bk8 の Android 14 system SDK 境界では compile visibility 差があるため、`PlaybackPipeline` は reflection による best-effort 設定を行い、失敗時は warning log に残して audio usage/content type/session 設定を継続する。

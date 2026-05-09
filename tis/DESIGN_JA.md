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

r51 は EIT p/f を中心に使う。scan/setup 後に `TvProvider.Programs` へ出す最低限の Programs だけ schedule から拾う。Programs の `internal_provider_data` には Base64 化した安定 `programKey` に加え、extended item、component/audio/series text、診断 JSON を TIS 内部データとして保存する。TvProvider の標準 column には title / short description / long description として自然に入る範囲だけ反映する。

## 字幕表示の責務

ARIB 字幕は TIS 側の字幕 path で `libaribcaption` を使用する。`arib_si_engine_rs` の自前 ARIB 文字列 decoder はサービス名・番組名・番組説明など字幕以外の SI/EPG 文字列に限定し、字幕 PES や字幕本文をその decoder に渡さない。libaribcaption は C API のみを使用し、独自 C/C++ shim は書かない。Kotlin から直接 C API を呼ばず、TIS Kotlin → Rust JNI boundary → safe Rust wrapper → libaribcaption C API の順に接続する。

## r51 live playback 実装方式

TIS の live playback は、案Aだけを採用する。すなわち、Tuner AV filter の `MediaEvent` を clear ES payload として受け取り、video は `MediaCodec` へ投入して `Surface` へ出力し、audio は `MediaCodec` で PCM 化して `AudioTrack` へ流す。

`tunneled` / platform passthrough playback path は r51 の設計候補から外し、実装しない。`notifyVideoAvailable()` は `Filter.start()` 成功、汎用 `FilterEvent`、payload 付き `MediaEvent` 到着だけでは呼ばない。video decoder が起動し、最初の decoded frame が `Surface` へ render された callback を gate とする。

scan は partial viewable service で成功扱いにしない。candidate ごとに `engine.isDiscoveryComplete()` または timeout 診断へ到達させ、TvProvider 登録には Rust engine の publishability と TIS の viewable 判定を合わせた `publishable ∧ viewable` の snapshot を使う。

## codec header / A-V sync / publish mode の固定

live playback の codec 構成は、video は MPEG-2 video と H.264/AVC、audio は AAC と MPEG audio を r51 対象 codec とする。

r51 対応 video ES が存在しない service は viewable としない。HEVC など r51 未対応 video codec のみを持つ service は、再生不能として `notifyVideoUnavailable()` へ落とす。

r51 対応 video ES が存在し、audio ES が存在しない、または audio codec だけが r51 未対応の場合は、video-only service として視聴可能にする。この場合、`notifyVideoUnavailable()` には落とさず、`audio absent` または `unsupported audio codec` を診断に残す。AC-3 等の未対応 audio codec は、対応 video が存在する限り video-only 診断の対象であり、video unavailable の直接理由にしてはならない。

decoder は PMT の stream_type だけでは構成せず、MediaEvent payload から MPEG-2 sequence header、H.264 SPS/PPS、AAC ADTS/AudioSpecificConfig、MPEG audio header を検出してから MediaFormat を構成する。

A/V同期方式は r51 で non-tunneled clear playback に固定する。tunneled playback と avSyncHwId は r51 範囲外であり、TIS の non-tunneled playback では avSyncHwId を使わないが、Tuner HAL API としては r51 で実装・AOSP準拠に仕様を固定する。video/audio は MediaCodec と AudioTrack の PTS により同期する。audio が存在する場合は AudioTrack を master clock とする。video-only service は視聴可能として扱い、audio が存在しない場合は `audio absent`、audio codec だけが未対応の場合は `unsupported audio codec` を診断に残す。

TvProvider公開モードは `PublishMode` で channel row 追加を setup scan / explicit rescan に限定する。live tune refresh では既存 channel の番組・診断更新だけを許可し、新規 channel row は追加しない。

## ARIB SI/EPG のTvProvider投影

ARIB SI/EPG の標準列投影と `internal_provider_data` への保存方針は、tv直下の `ARIB_SI_EPG_TvProvider投影方針.md` をSSOTとする。TISは、同文書で標準列投影が固定された項目だけを `Programs.COLUMN_LONG_DESCRIPTION` 等へ出し、未固定項目は当面 `internal_provider_data` のみに保存する。

`Programs.COLUMN_CANONICAL_GENRE` については、TIS が直接設定する値と、Android TvProvider が `Programs.COLUMN_BROADCAST_GENRE` から内部補完した読み出し結果を区別する。r51 では ARIB→Android canonical genre 写像表を固定しないため、TIS は `Programs.COLUMN_CANONICAL_GENRE` を `ContentValues` に直接設定しない。ただし TvProvider 読み出し後に canonical genre が非空になることは、AOSP 標準の内部補完として許容する。

## parental control / content rating 契約

TIS は `arib_si_engine_rs` から受け取った `parental_rating_descriptor` の構造化データを、TIS 側で定義するARIB rating domainの `TvContentRating` へ変換する。

TvProvider へ番組を登録または更新する場合、変換できる rating は `TvContentRating.flattenToString()` の結果を `Programs.COLUMN_CONTENT_RATING` に格納する。変換できない rating は推測で `COLUMN_CONTENT_RATING` に入れず、`internal_provider_data` と診断に保持する。

live session は、現在番組の rating と system parental control 設定を同期して扱う。`TvInputManager.isParentalControlsEnabled()` が true の場合、TIS は現在番組の `TvContentRating`、または rating 未取得時の `TvContentRating.UNRATED` を `TvInputManager.isRatingBlocked(...)` に渡して判定する。blocked の場合は video frame を表示する前に再生を停止または抑止し、`notifyContentBlocked(rating)` を呼ぶ。許可された場合は `notifyContentAllowed()` を呼ぶ。

TIS は `TvInputManager.ACTION_BLOCKED_RATINGS_CHANGED` と `TvInputManager.ACTION_PARENTAL_CONTROLS_ENABLED_CHANGED` を監視し、設定変更時に現在番組の parental control 判定を即時再評価する。

# arib_si_engine_rs 設計判断

## 責務

`arib_si_engine_rs` は、Tuner HAL → framework/JNI/Tuner SDK API → TIS → arib_si_engine_rs という経路で渡された PSI/SI section payload と TIS 側 メタデータを入力として、PSI/SI/EIT descriptor の 意味解析 を Rust で実装する。PMT/CAT の CA_descriptor から得られる CA_system_id、ECM PID、EMM PID と、SDT 等から得られる free_CA_mode / scrambling flag、サービス識別子 補助情報を含む CA情報 / サービスメタデータ意味モデル も arib_si_engine_rs / TIS 側の責務とする。raw TS packet demux、PID filter、section assembly、section payload delivery は Tuner HAL の責務であり、arib_si_engine_rs に重複実装しない。Tuner HAL を CA情報 / サービスメタデータ意味モデル の生成者またはSSOTにしない。


## ARIB 文字列 decoder の適用範囲

自前の ARIB 文字列 decoder は、サービス名、番組名、短形式イベント、長形式イベント、各種 descriptor のテキストなど、字幕以外の SI/EPG 文字列に限定して使う。字幕 PES、字幕管理データ、字幕本文、外字・DRCS を含む字幕表示処理は `libaribcaption` の責務とし、`arib_si_engine_rs` の自前 decoder に字幕用 ARIB B24 decoder としての完全性を 対応宣言しない。

未対応の SI/EPG 文字・escape は `panic` させず、置換文字または 診断によって安定動作させる。字幕 payload を `decode_arib_string_lossy()` に渡す経路は禁止する。字幕本文処理は TIS 側の libaribcaption 経路だけで行う。
`arib_si_engine_rs` は libaribcaption ラッパー を所有しない。libaribcaption は TIS 側の字幕 path から Rust JNI boundary と 安全なRustラッパー 経由で呼ぶ。

ARIB本文の選定は `../開発規則.md` の ARIB 本文選定規則を正とする。本decoderについて現時点で条項単位に取得・確認できる本文は ARIB 公式英語版 STD-B24 6.4-E1 Fascicle 1 の7.1.1.1〜7.1.2.4であり、7.1.1.1のTable 7-1〜7-3をinvocation・designation・Final Byte、7.1.1.2〜7.1.1.5を文字集合とDRCS、7.1.1.6をMacro、7.1.2.1〜7.1.2.4を制御機能の根拠として条項単位で用いる。改定概要、版一覧、二次資料を取得できない本文の具体規定の代用にしない。STD-B24の他Fascicleまたは字幕への適合は本decoderの主張に含めない。

本decoderの適合主張は、字幕ではないSI/EPG文字列について、次の境界に限定する。

| 項目 | 対応境界 |
|---|---|
| 初期状態 | G0=Kanji、G1=Alphanumeric、G2=Hiragana、G3=Macro、GL=LS0(G0)、GR=LS2R(G2) |
| 文字集合 | SI/EPGで使用するKanji、Alphanumeric、Hiragana、Katakanaと、実装・試験で対応を確認した追加記号だけを文字として出力する |
| designation / invocation | ESCによるdesignation、LS0/LS1/LS2/LS3、LS1R/LS2R/LS3R、SS2/SS3を、対応済み文字集合とMacroの選択に使用する |
| Macro | STD-B24 6.4-E1で定義された既定Macroだけを展開し、再帰・入力消費量に上限を設ける。未定義Macroは置換と診断にする |
| DRCS・外字 | 自前で字形を生成しない。SI/EPG用の明示的な外字辞書に一致する場合だけ変換し、それ以外は置換と診断にする |
| UCS | UCS符号方式を現行の対応能力として主張しない。UCS切替を検出した場合は、その文字列を未対応符号方式として置換と診断にする |
| 不明・切詰めescape | `U+FFFD`へ置換し、offset、入力prefix、理由を診断へ記録する。`panic`、無言の脱落、推測による状態遷移を禁止する |
| lossy境界 | 置換を許すAPIは`decode_arib_string_lossy()`だけとし、置換数と理由を返す。strict APIは未対応または不正な符号列をエラーにする |

この表にない文字集合、制御機能、字幕、BML、組版、DRCS字形レンダリング、UCS符号方式は未対応である。対応を追加する場合は、参照するSTD-B24の版・分冊・条項、入力状態、出力、置換規則、試験ベクトルを先に更新する。


## EIT 範囲

本crateは、TISから渡されたEIT sectionについてEIT/descriptorの構文・意味解析を担当する。どのEIT tableをいつ収集し、TvProviderへどの期間・用途で利用するかという製品releaseの収集scopeは`../開発規則.md`を正とし、本書では再定義しない。TIS runtimeのfilter起動・停止は`../tis/DESIGN_JA.md`を正とする。

### 複数table instanceの完成・更新・寿命

`repeat=true`で継続配送されたsectionについて、本crateは`table_id_extension`、actual version、`current_next_indicator`、`section_number`、`last_section_number`に基づいてtable instanceを区別し、instance別の完成・更新・寿命を管理する。

本crateは、製品または個別操作が必要とするinstance集合そのものを決定せず、instance別の完成・更新・寿命状態をTISへ返す。どの集合の完成でfilterを停止するかはTISのruntime責務とする。

## descriptor 変換

表示・保存対象として扱う EIT descriptor は現行仕様で構造化変換する。TvProvider 標準列への投影は tv 直下の `ARIB_SI_EPG_TvProvider投影方針.md` を正とし、`internal_provider_data` の具体 schema / canonical encode は本 crate の Rust provider-data serde構造体を SSOT とする。同文書で標準列投影が固定されている component、音声コンポーネント、コンテンツジャンル、free_CA_mode、視聴年齢制限、series id、episode number、音声言語は provider 用 フィールドとして出せる。last episode number は通常の `TvContract.Programs` 標準列へ投影する候補ではなく、series の完全構造、イベントグループ、linkage、unknown、診断JSON などと同様に JSON v1 `internal_provider_data` に構造化保存する。Android canonical genre は本crateでは決定せず、投影SSOTに従いTISが決定した結果だけをprovider-dataへ保持できる。

`arib_si_engine_rs` は Android canonical genre の写像表をSSOTとして所有しない。

本 crate は provider-data schema、canonical encode、保存上限、診断 schema の正本を所有する。TvProvider標準列への投影判断は `ARIB_SI_EPG_TvProvider投影方針.md`、TIS runtime での書き込み契機、retry、現在番組解決、視聴セッション利用は `tis/DESIGN_JA.md` を正とする。

content_descriptor 由来のARIB分類、表示文字列、user_nibble を構造化して出力し、TIS が `ARIB_SI_EPG_TvProvider投影方針.md` の明示写像表に基づいて `Programs.COLUMN_CANONICAL_GENRE` へ入れる値を決定する。

## parental_rating_descriptor の構造化契約

`arib_si_engine_rs` は `parental_rating_descriptor` を診断文字列だけに落とさず、TIS が `TvContentRating` へ変換できる構造化データとして出力する。

出力する最小フィールドは次とする。

```text
parental_rating_descriptor:
  entries[]:
    country_code
    rating_value        # ARIB STD-B10 5.13-E1 Part 2 6.2.12のRating 8 uimsbfを8bit値のまま保持する
    raw_rating_byte     # 元8bitレーティング値
  raw_descriptor_bytes
  parse_status          # ok / malformed_length / truncated_descriptor / unsupported_value
```

`arib_si_engine_rs` は Android `TvContentRating` の domain 名や flattened string をSSOTとして決めない。Android TvProvider列への投影と `TvContentRating` 生成は TIS 側の責務とし、投影方針は tv 直下の `ARIB_SI_EPG_TvProvider投影方針.md` をSSOTとする。

未対応 country_code、未定義 rating_value、不正 descriptor は破棄せず、`parse_status` と 診断JSON に保持する。未対応値を推測で一般ユーザー向け レーティングに変換してはならない。

## BS / CS110 discovery

BS と CS110 の complete 判定には BAT、SDT other、NIT other を含める。これらは table_id だけの global 完了ではなく、table_extension と NIT/BAT transport loop から得た ONID/TSID scope を使って transport 単位で判定する。リモコンキー が得られない場合は service_id を表示番号の代替値 とする。

`arib_si_engine_rs` は、service / transport単位の意味解析結果として、ONID / TSID / SID、PMT、PCR、audio/video ESの存在・欠落理由、scrambling情報、および`publishability_by_service`を構造化してTISへ渡す。Android channelを登録するか、`TvContract.Channels.SERVICE_TYPE_*`へどう写像するか、partial snapshotをchannel insertへ使用するかはTISの責務であり、`../tis/DESIGN_JA.md`を正とする。 `publishability_by_service`はservice / transport単位の登録判断材料を構造化してTISへ渡す意味解析結果であり、Android channel登録、`TvContract.Channels.SERVICE_TYPE_*`写像、channel insertの最終判断はTISが行う。

## system_management_descriptor と通常受信判定

`system_management_descriptor`（SMD、`descriptor_tag=0xFE`）はNITのnetwork loopに属するネットワーク単位の意味情報として`arib_si_engine_rs`が解析する。Tuner HALはSMDを解釈せず、他のsectionと同じ汎用section配送だけを行う。SMDの構文・意味はARIB STD-B10 5.13-E1 Part 2 §6.2.21、通常受信対象の判定はARIB STD-B21 5.12-E2 Chapter 13 §13.2を根拠とする。

SMDの意味モデルは`system_management_id`の16 bit原値、上位2 bitの`broadcasting_flag`、次の6 bitの`broadcasting_identifier`、下位8 bitの`additional_broadcasting_identification`、後続の`additional_identification_info`、構文検査結果を保持する。未知値を既知方式へ丸めずraw値と診断を残す。ただし現行productの通常受信可否を下位8 bitまたは`additional_identification_info`で制限しない。

現行productでは、正常なSMDについて`broadcasting_flag=0b00`かつ選局候補のdelivery systemと`broadcasting_identifier`が一致する場合だけ`SUPPORTED_BROADCAST`とする。対応する`broadcasting_identifier`はBSデジタル=`0b000010`、地上デジタルテレビ=`0b000011`、広帯域CSデジタル=`0b000100`とし、CS110は広帯域CSデジタルとして判定する。`broadcasting_flag`が`01`または`10`なら`NON_BROADCAST`、`11`なら`UNDEFINED_BROADCAST_CLASS`、`00`で`broadcasting_identifier`が一致しない場合は`UNSUPPORTED_BROADCAST_SYSTEM`とする。SMD欠落または構文不正は再取得可能な例外状態`UNDETERMINED_SMD`として診断し、永久的な`UNSUPPORTED`には確定しない。ただし、正常なSMDで`SUPPORTED_BROADCAST`を確認できるまでは通常受信成立の根拠に使わない。

SMDの判定対象は既存のtable-instance完成・version・寿命規則で有効とされたNITとし、SMD専用の`PENDING`状態や別のversion切替状態機械を設けない。

`publishability_by_service`では`NON_BROADCAST`、`UNDEFINED_BROADCAST_CLASS`、`UNSUPPORTED_BROADCAST_SYSTEM`、`UNDETERMINED_SMD`のいずれでも`channel_registration_ready=false`、`epg_publishable=false`、`clear_live_playback_supported=false`とする。意味解析・診断用の`publishable`自体はSMDだけでfalseにしない。`SUPPORTED_BROADCAST`の場合だけSMD gateを通過したものとして、PMT、PCR、service type、codec、CAS等の既存条件で最終判定する。`UNDETERMINED_SMD`は再取得によって正常なSMDを得た時点で再評価し、SMD適合を肯定する根拠には使わない。Android channel登録と視聴セッションの最終制御は引き続きTISが所有する。

## EIT 時刻状態と event identity

EIT event の `start_time` と `duration` は、ARIB の未定義値と不正値を混同せず次の状態に正規化する。

- `DEFINED`: `start_time` と `duration` がともに具体的で構文的に有効。`original_network_id / transport_stream_id / service_id / event_id` を stable identity として扱う。
- `UNDEFINED_TIME`: `start_time=0xFFFFFFFFFF` または `duration=0xFFFFFF` の片方だけが all-1。event 自体は確定しており `event_id` は有効なので、同じ4要素を stable identity として保持してよい。ただし具体時刻が揃うまで `TvProvider.Programs` row へ投影しない。
- `UNDECIDED_EVENT`: `start_time=0xFFFFFFFFFF` かつ `duration=0xFFFFFF`。event 内容自体が未定で `event_id` に identity としての意味がないため、raw event_id は診断・raw意味objectに保持してよいが、stable key、`ProgramKeyV1`、deletion-authoritative な valid-event-set、後続の具体eventとの相関に使用しない。
- `MALFORMED_TIMING`: 上記未定義値ではなく、BCDその他の構文規則に違反する。正常eventへ昇格せず診断に保持する。

## section 更新

PAT/PMT/SDT/NIT/BAT/EIT の version 更新では collector 全体を捨てない。table 単位、section 単位、サービス 単位で差分更新する。

EIT は section version 更新で消えた event を削除候補として扱う。ただし TvProvider / TIS 側へ stable identity として `original_network_id / transport_stream_id / service_id / event_id` を提供できるのは `DEFINED` または `UNDEFINED_TIME` の event に限る。`UNDECIDED_EVENT` は valid event identity set に含めず、既存 Program の削除根拠にも後続具体eventとの相関根拠にも使わない。section 更新後の stable event set が空になった場合も no-op として破棄せず、サービスキー、更新区間、空の valid event identity set を JNI/TIS へ返す。TIS は、Rust parser が `deletionAuthoritative=true` と判定した snapshot だけを obsolete Programs delete に使う。

EIT event fixed フィールド、start_time BCD、duration BCD、descriptor_loop_length が不正な event を含む section は、既存 event 削除用の authoritative valid-event-set として扱わない。不正 event は Programs から消すのではなく、既存正常 event を保持したまま 診断情報に記録する。

開始時刻、終了時刻、duration、番組名、説明文の変更は、同一 stable identity の event 更新として扱う。開始時刻は stable identity に含めない。

ただし TvProvider の時間範囲制約、row 更新制約、または TIS 実装都合により provider row の再作成が必要な場合は、既存 provider row を削除して再 insert してよい。その場合でも、内部 stable identity は `original_network_id / transport_stream_id / service_id / event_id` のまま維持する。

## 診断 API

TvProvider に自然に入らない descriptor は構造化した内部データとして `internal_provider_data` に保存し、診断 API にも出す。EIT event ごとの 診断 文字列には、content、component、音声コンポーネント、視聴年齢制限、series、イベントグループ、linkage、未知 descriptor の数と主要値を含める。

provider-data JSON v1 は `provider-data / diagnostics Rust SSOT` 節の `ProgramProviderDataV1` を唯一の正式 schema とする。少なくとも `series`、`relatedItems`、`linkage`、`freeCaMode`、`audioLanguages`、`ratings`、`genres`、`extendedItems`、`components`、`audio`、`video`、`diagnostics` を最上位フィールドとして保持する。`relatedItems` は `shared` / `relay` / `movement` の種別、ONID、TSID、service_id、event_id を保持する。`series` は series_id、repeat_label、program_pattern、expire_date、episode_number、last_episode_number、series_name を保持する。


## 構造化変換対象 descriptor

short_event、extended_event、content、component、audio_component、parental_rating、series、event_group、linkage を現行仕様で構造化変換する。未知 descriptor は破棄せず 診断に保持する。

ARIB descriptor は `descriptor_length`、descriptor 内部 length、loop 単位、fragment sequence が妥当な場合だけ正常フィールドとして採用する。length 不整合、余剰 byte、fragment 欠落、`descriptor_number` 重複、`last_descriptor_number` 不一致、必須フィールド 不足は 不正 descriptor とし、番組名、short text、長形式イベント本文、コンテンツジャンル、component、音声コンポーネント、series、event_group、linkage の正常フィールドには採用しない。不正 descriptor は parser を停止させず、`DescriptorDiagnosticV1` に tag、offset、declaredLength、actualRemainingLength、parseStatus、rawPrefixHex、section scope を保持する。

## API 境界の固定

Kotlin/JNI の通常 サービススナップショット は channel registration 用の `registration_ready_snapshot()` 相当を使う。これは現行の平文ライブ視聴対応宣言対象だけでなく、サービス単位の登録可能条件を満たす scrambled unsupported サービス も含み得る。平文ライブ視聴対応宣言対象は別途 `clear_live_playback_supported_snapshot()` / `clear_live_playback_supported` で判定する。`publishable_snapshot()` は診断・test 用であり、登録可能未満の サービスを通常 channel 登録経路に出さない。publishable だが現行ライブ視聴対象外の サービスについては `publishability_by_service` を JNI 診断として公開し、ONID、TSID、service_id、publishable / channel_registration_ready / epg_publishable / clear_live_playback_supported / requires_cas / unsupported_cas 可否、欠落 component、除外理由を分けて観測する。

PAT は ONID を持たないため、`(transport_stream_id, service_id) -> pmt_pid` をそのまま publishable サービス識別子 として扱わない。SDT/NIT/BAT 等で ONID が一意に解決できた場合だけ `(original_network_id, transport_stream_id, service_id, pmt_pid)` へ昇格し、ONID が曖昧な場合は publish 抑止または欠落診断に留める。

EIT event の stable key は `DEFINED` または `UNDEFINED_TIME` の場合だけ `original_network_id / transport_stream_id / service_id / event_id` とし、開始時刻は表示・更新用フィールドとして別に扱う。`UNDECIDED_EVENT` は stable key を持たず、bulk snapshot DTO から `ProgramProviderDataV1.programKey` を必要とする公開対象へ昇格させない。TIS/TvProvider は `event_id + start_time` に依存した stable key を作らず、`UNDECIDED_EVENT` の raw event_id だけからstable keyを作らない。旧 indexed JNI getterである `nativeGetEventStableIdentity()` は提供しない。

開始時刻変更によって TvProvider row を削除・再作成する場合でも、TIS / arib_si_engine_rs の stable identity は変更しない。`event_id + start_time` は表示・検索・provider row 再作成補助には使ってよいが、event identity の SSOT にしてはならない。

記述子診断は bulk snapshot DTO と `ProgramProviderDataV1.diagnostics.descriptorDiagnostics[]` で渡し、TIS はその内容を `internal_provider_data` の内部データとして保存する。旧 indexed JNI getter である `nativeGetEventDiagnosticDescriptorJson()` は提供しない。TvProvider の標準 title / description / 時刻列には 番組名、short text、長形式イベント本文 を入れる。さらに `ARIB_SI_EPG_TvProvider投影方針.md` で固定された範囲では、component / 音声コンポーネント / コンテンツジャンル / freeCA 由来の補足を `Programs.COLUMN_LONG_DESCRIPTION` へ整形して出してよい。イベントグループは LONG_DESCRIPTION へ出さず provider-data JSON の `relatedItems` に保存する。series、linkage、unknown descriptor、診断JSON は標準列へ出さず内部データに分離する。

自前 ARIB 文字列 decoder は字幕以外の SI/EPG 文字列だけを対象にする。未対応 escape、切り詰め escape、切り詰め漢字、置換文字数は 診断要約 として観測できる。字幕は `libaribcaption` の責務である。

### 文字 decoder 固定方針

自前 ARIB 文字列 decoder の設計対象範囲は、mirakc が EPG / サービスモデル 構築で扱う範囲に合わせる。すなわち、字幕本文レンダリングではなく、サービス名、番組名、短形式イベント記述、長形式イベント記述、各種 SI/EPG descriptor の テキストフィールドを安定して文字列化する範囲を対象にする。

この範囲を超える字幕 PES、字幕管理データ、字幕本文、DRCS/外字レンダリング、厳密な組版制御は恒久的に `arib_si_engine_rs` の対象外であり、必要な場合は `libaribcaption` 側の責務とする。未対応 escape / 未対応文字は `panic` ではなく 診断情報と置換文字へ変換する。これは本crateの設計方針として固定する。

## mirakc 相当の ARIB 文字列範囲

自前 decoder は mirakc-arib が EPG / サービスモデル 構築で文字列化している範囲に限定する。対象は SDT サービス descriptor の サービス名、EIT short_event の 番組名 / text、EIT extended_event の item description / item text / text、component descriptor、音声コンポーネントdescriptor、series descriptor の text/name である。

extended_event は、全 fragment の `last_descriptor_number` が一致し、`descriptor_number` が 0 から `last_descriptor_number` まで重複なく連続して揃う場合だけ、`descriptor_number` 順に fragment を連結して ARIB 文字列として復号する。欠番、重複、`last_descriptor_number` 不一致がある場合は extended description / 長形式イベント項目s を正常フィールドに採用せず、診断に記録する。字幕 PES、字幕管理データ、字幕本文、DRCS/外字レンダリング、組版制御、BML は対象外であり、`libaribcaption` 側の責務とする。

## ARIB 文字列 decoder 入力境界と TvProvider 連携境界

ARIB SI/EPG文字デコードの仕様固定に使う入力形態は、実波 TS ファイルを必須形式にせず、descriptor byte array / section builder を主入力とする。対象は SDT サービス名、EIT short_event、extended_event fragment、長形式イベント項目、component、audio_component、series、unsupported escape、truncated text、replacement 診断である。

Rust descriptor モデル から Kotlin/TvProvider へ渡す通常境界は、`ProgramProviderDataV1` と、TvProvider 標準列へ投影するための構造化 DTO だけにする。旧来の `eventGroupText`、`freeCaText`、`seriesName` のような表示用 flat フィールド は通常投影経路では使わない。イベントグループは provider-data JSON の `relatedItems`、free_CA_mode は `freeCaMode`、series name は `series.name` に保存する。TvProvider の title / description / long description への投影は `ARIB_SI_EPG_TvProvider投影方針.md` を SSOT とし、同文書で固定済みの component/audio/content/freeCA 補足だけを `Programs.COLUMN_LONG_DESCRIPTION` へ出す。イベントグループは LONG_DESCRIPTION や一般 UI 本文へ出さない。

設計書は現行仕様中心にし、過去の経緯は CHANGELOG.md に分離する。


## Android レーティングドメイン 境界

`arib_si_engine_rs` は ARIB `parental_rating_descriptor` の構造化解析結果だけをSSOTとする。Android `TvContentRating` の `domain` / `ratingSystem` / `rating` 文字列、`flattenToString()`、`Programs.COLUMN_CONTENT_RATING` への投影、`TvInputManager.isRatingBlocked()` に渡す値は TIS 側の責務である。

Rust 側に `com.android.tv` や `ISDB_<age>` の Android domain 決定文字列を持ち込んではならない。Rust は `country_code`, `rating_value`, `raw_rating_byte`, `parse_status`, `raw_descriptor_bytes` を保持し、未対応値を推測変換しない。

## provider-data / 診断情報 Rust SSOT

### provider-data 受け渡し境界（推奨案A）

TIS が JNI へ渡す JSON は、保存形式ではなく Rust serde 型へ値を渡すための受け渡し用形式である。受け渡し用形式の型、必須項目、欠落時の扱い、旧形式拒否、値域検査は Rust 側の serde 型を正とする。

Rust provider-data builder は、受け渡し用 JSON を serde 型へ読み込み、必須項目、型、値域、旧形式混入を検査する。検査に通った入力だけから、保存用 JSON、識別子、切り詰め診断を生成する。

保存用 JSON の schema、正規化、識別子抽出、サイズ上限処理は Rust が単独で所有する。TIS は保存用 JSON を直接生成してはならない。

受け渡し用形式の schema 名は `maleicacid.tv.programRequest` / `maleicacid.tv.channelRequest` とし、保存用 schema 名 `maleicacid.tv.program` / `maleicacid.tv.channel` と分離する。

受け渡し用形式と保存用形式は別物である。受け渡し用形式を `Programs.COLUMN_INTERNAL_PROVIDER_DATA` / `Channels.COLUMN_INTERNAL_PROVIDER_DATA` に保存してはならない。

required field 欠落時に `0`、`false`、`jpn`、`UNKNOWN`、空文字で補完して provider-data を成立させてはならない。r50 以前の `;` 区切り形式、旧 flat provider-data、旧 provider-data 断片は受け渡し用形式としても保存用形式としても拒否する。

`DescriptorDiagnosticV1` は Rust が生成した正規 JSON を正とする。TIS から戻ってくる場合も、TIS が項目単位で再構築した JSON ではなく、Rust が生成した正規 JSON を透過保持したものだけを受ける。


`arib_si_engine_rs` は SI/EIT 意味解析 に加えて、TvProvider `internal_provider_data` JSON v1 の構造 SSOT を持つ。実装上は `provider_data` module に Rust `serde` struct を置き、JSON canonical encode、正規化、安定キー抽出 をこの module に閉じる。

Programs の `internal_provider_data` には、`requiresCas`, `unsupportedCas`, `clearLivePlaybackSupported`, `channelRegistrationReady`, `epgPublishable`, `publishStateSource` 相当の CAS / 準備状態を `cas` または診断情報に保存する。視聴年齢制限については `countryCode`, `ratingValue`, `rawRatingByte`, `supported`, `parseStatus`, `mappedTvContentRating` 相当の情報を `ratings` または診断情報に保存する。現在の診断情報が完全であれば、その値を Programs CAS 状態の正とする。診断情報が欠落または不完全な場合、既存 channel の `internal_provider_data` から CAS / 準備状態を代替参照して Programs 側に保存する。channel 側だけに保存して Programs 側を false に落としてはならない。

provider-data 全体は canonical UTF-8で16 KiBを目安上限、32 KiBを絶対上限とする。絶対上限を超える場合は、各操作後にcanonical encodeし直してサイズを測りながら、`diagnostics.rawProviderDataExtensions`、`diagnostics.descriptorDiagnostics`、`diagnostics.publishDiagnostics`、`extendedItems`の順に配列末尾から要素を除く。最後に長文フィールドをUTF-8 scalar境界で末尾から短縮する。それでも32 KiB以下にならない場合はprovider-data生成を失敗させ、識別子、時刻、CAS状態、レーティングを欠落させた結果を保存しない。切り詰めた結果には`PROVIDER_DATA_TRUNCATED`、種類別dropped count、短縮前後のbyte数を必ず保存する。この診断自体を加えた後にも再度32 KiB以下であることを検証する。

TIS Kotlin は provider-data schema を定義しない。TIS は Rust JNI が返す JSON bytes を `Programs.COLUMN_INTERNAL_PROVIDER_DATA` へ保存し、標準列用の値だけを `ARIB_SI_EPG_TvProvider投影方針.md` に従って `ContentValues` へ詰める。

### Rust struct SSOT

少なくとも以下の struct を Rust 側で定義し、Kotlin 側に同名 schema を二重定義しない。

```rust
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgramProviderDataV1 {
    pub schema: String,                  // "maleicacid.tv.program"
    pub schema_version: u32,             // 1
    pub program_key: ProgramKeyV1,
    pub service_key: ServiceKeyV1,
    pub timing: ProgramTimingV1,
    pub source: ProgramSourceV1,
    pub cas: CasStateV1,
    pub ratings: Vec<RatingV1>,
    pub genres: Vec<GenreV1>,
    pub series: Option<SeriesV1>,
    pub related_items: Vec<RelatedItemV1>,
    pub linkage: Vec<LinkageV1>,
    pub free_ca_mode: Option<FreeCaModeV1>,
    pub audio_languages: Vec<AudioLanguageV1>,
    pub audio: Option<AudioMetadataV1>,
    pub video: Option<VideoMetadataV1>,
    pub extended_items: Vec<ExtendedItemV1>,
    pub components: ComponentsV1,
    pub diagnostics: DiagnosticsV1,
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct ProgramKeyV1 {
    pub kind: String,
    pub original_network_id: u16,
    pub transport_stream_id: u16,
    pub service_id: u16,
    pub event_id: u16,
}
```

`ProgramKeyV1.kind` は `arib-event-v1` とする。`ProgramKeyV1` に start/end/duration を入れてはならない。

### JSON 表現規則

JSON は正規表現ではなく、Rust `serde` / Kotlin JSON parser / JSON Schema によって読み書き・検証する。`ProgramProviderDataV1` の canonical JSON では、任意の単一 オブジェクト は値が無い場合 `null`、繰り返し要素は空の場合 `[]`、常設 container は空でも オブジェクト として出力する。具体的には、`series`、`freeCaMode`、`audio`、`video` は未取得時 `null`、`ratings`、`genres`、`relatedItems`、`linkage`、`audioLanguages`、`extendedItems` は未取得時 `[]`、`components` は常に オブジェクト とし、内部の `video`、`audio`、`subtitle`、`data` は空でも `[]` とする。

未知 key を読み込んだ場合は、無言で破棄しない。既存 JSON v1 の未知 key は読み取り時に保持可能とし、新規 canonical 出力では既知 schema フィールド と `diagnostics.rawProviderDataExtensions[]` へ正規化する。`JSONObject` の手書き構築や文字列連結による JSON 生成を禁止する。

`series` は series_id、repeat_label、program_pattern、expire_date_valid、expire_date、episode_number、last_episode_number、series_name、parse_status を保持する。series name は番組表 title を置換する値ではない。

`relatedItems` は `event_group_descriptor` の構造保存先であり、`kind` は `shared` / `relay` / `movement` のいずれかに正規化する。`group_type=0x1` は `shared`、`0x2` / `0x4` は `relay`、`0x3` / `0x5` は `movement` とする。ONID / TSID / service_id / event_id は数値のまま保持する。

`linkage` は `linkage_descriptor` の transport_stream_id、original_network_id、service_id、linkage_type、private_data_prefix、parse_status を保持する。現行仕様では標準列、一般 UI、予約追従へ接続しない。

`freeCaMode` は EIT `free_CA_mode` の raw 値、scrambled 投影用 boolean、parse_status を保持する。CAS 権利状態、カード状態、CAS HAL 状態と混同しない。

`audioLanguages` は PMT / 音声コンポーネントdescriptor から取得できる ISO639 language だけを保持する。取得不能時に推測値を入れない。

`genres` は ARIB content descriptor の level1、level2、user_nibble、ARIB 表示名、parse_status と、TIS が明示写像表に基づいて決定した Android canonical genre 投影結果を保持できる。`arib_si_engine_rs` の SI event DTO は Android canonical genre を生成しない。Android canonical genre の判定と `Programs.COLUMN_CANONICAL_GENRE` への投影は TIS 側の責務であり、user_nibble はその判定に使わない。

`ratings` は parental_rating_descriptor の country_code、rating_value、raw_rating_byte、supported、parse_status を保持する。未対応値を推測で Android レーティングに変換しない。補足説明が必要な場合だけ `diagnostics.publishDiagnostics[]` に warning を追加し、DescriptorDiagnosticV1 には入れない。Android `TvContentRating` 文字列は TIS 側が生成する。

`components.video[]` は ES PID、stream_type、component_tag、component_type、codec、解像度、走査方式、aspect、profile / level、根拠 descriptor を ES/component 単位で保持する。`components.audio[]` は ES PID、stream_type、component_tag、component_type、codec、ISO639 language、channel configuration、sampling info、根拠 descriptor を ES/component 単位で保持する。`components.subtitle[]` は ES PID、component_tag、data_component_id、ISO639 language、TIS trackId、caption サービス kind、parse_status を保持する。`components.data[]` はデータ component の メタデータを保持するが、BML / data broadcast 実行状態や UI 状態は保持しない。

`video` と `audio` は実際に主track 候補として選択された component の要約であり、未選択の場合は `null` とする。codecメタデータの認識は ライブ viewable / playable 対応宣言を意味しない。unsupported codec、decoder unavailable、transport profile out of scope は 診断情報に保存する。

### DescriptorDiagnosticV1

Descriptor診断情報は Rust が生成し、Kotlin はその JSONオブジェクトを別 schema に変換してはならない。`DescriptorDiagnosticV1` は `ProgramProviderDataV1.diagnostics.descriptorDiagnostics[]` の要素 schema であり、provider-data 全体の schema ではない。TvProvider `internal_provider_data` 全体の唯一の schema は `ProgramProviderDataV1` とする。

```rust
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescriptorDiagnosticV1 {
    pub schema: String,
    pub schema_version: u32,
    pub severity: DiagnosticSeverity,
    pub code: String,
    pub scope: SectionScopeV1,
    pub descriptor: DescriptorScopeV1,
    pub message: String,
}
```

`SectionScopeV1` は PID、table_id、table_id_extension、version、section_number、ONID、TSID、service_id、event_id を持てる構造とする。unknown numeric を `-1` へ潰さず、`Option`、`null`、または key omission とする。JSON Schema ではこれらの key を最小検証対象として定義し、未知 key は `additionalProperties: true` により保持可能にする。

`DescriptorScopeV1` は tag、name、offset、declared_length、actual_remaining_length、parse_status、raw_prefix_hex を持つ。`raw_prefix_hex` は最大64 bytes相当までとする。JSON Schema では tag、offset、declaredLength、actualRemainingLength、parseStatus、rawPrefixHex を必須最小フィールドとする。name は未知 descriptor で決定できないため任意フィールドとし、parseStatus は診断分類の根拠であるため必須フィールドとする。

### canonical JSON

canonical JSON は Rust `serde_json` で生成し、struct フィールド順序と`BTreeMap`により出力順序を固定する。これは保存bytesの決定性、32 KiB上限制御、schema整合確認データとのbyte比較のために必要である。provider-data単体の同一内容判定には、TISがTvProvider更新抑止用に計算する行全体のpublish fingerprintが既にprovider-data bytesを含むため、別のSHA-256値を生成・返却・保存しない。provider-dataの暗号学的署名、MAC、真正性、送信者認証、改ざん防止も要件としない。

### JNI boundary

Rust は少なくとも以下の JNI API 相当を提供する。

```text
buildProgramProviderData(inputJson) -> ProviderDataResult
normalizeProgramProviderData(rawBytes) -> ProviderDataResult
extractProgramKey(rawBytes) -> ProgramKeyResult?
buildChannelProviderData(inputJson) -> ProviderDataResult
decodeChannelProviderData(rawBytes) -> ChannelProviderDataResult?
```

`decodeChannelProviderData()` は UTF-8、JSON、schema を Rust 側で検証し、canonical bytes、schema version、型付き `ServiceKey`、型付き `ChannelTune` を返す。`ChannelTune` は `inputId`、`deliverySystem`、`frequencyHz`、`streamIdType`、`streamId`、`physicalChannel`、`satelliteBand`、`remoteControlKeyId` を持ち、backend名、driver名、driver固有slotを含めない。Kotlin は channel provider-data JSON を `JSONObject`、文字列連結、個別key抽出で解釈しない。

`inputJson` は Rust builder への入力 DTO であり、TvProvider 保存 schema ではない。Rustは最終provider-data bytes、schema version、切り詰め結果、診断件数を返す。`ProviderDataResult`に`signature`または`contentDigest`フィールドを設けない。

`rawBytes` は任意バイナリではなく、既存 TvProvider に保存済みの JSON v1 UTF-8 バイト列を指す。JNI 呼び出し元は provider-data を `String` 化して渡してはならず、保存済み BLOB バイト列をそのまま渡す。互換上 TvProvider が文字列として返す場合も、呼び出し元は UTF-8 バイト列へ戻すだけに限定し、provider-data JSON を Kotlin 側で解釈・再構築しない。

Rust は `rawBytes` が invalid UTF-8 または malformed JSON の場合、通常実行経路では panic せず、`ProviderDataResult` の失敗または key 抽出失敗へ落とす。provider-data bytesだけのdigest APIは設けない。同一公開内容の抑止判定はTISの行全体publish fingerprintを正とし、Rust builderの責務へ重複させない。



### current-program 診断情報

現在番組選択の `overlapCount`、`selectedProgramId`、`selectionRule` は TvProvider row id と process 内の query 結果に依存する runtime 診断であり、`ProgramProviderDataV1` へ保存しない。これらは TIS が process-local `CurrentProgramResolutionDiagnostic` として保持し、provider-data identity、canonical bytes、publish fingerprint の入力にしない。Rust provider-data schema と JNI に `diagnostics.currentProgram` および `appendCurrentProgramDiagnostics()` を設けない。

### ChannelProviderDataV1

Channel provider-data の正形式は JSON v1 のみとし、schema は `maleicacid.tv.channel` / `schemaVersion=1` とする。`arib_si_engine_rs/schema/channel_provider_data_v1.schema.json` は、channel row の tune 復元に必要な `inputId`、物理選局情報、ONID / TSID / service_id、表示名、登録可能性診断を検証対象にする。r50 以前の `;` 区切り key-value 形式、旧 flat provider-data、旧 provider-data 断片は読み取り互換入力としても残さない。 Channel provider-data の top-level envelope は `schema="maleicacid.tv.channel"`, `schemaVersion=1`, `serviceKey`, `tune`, `cas`, `diagnostics` を持つ JSON v1 とする。`tune` は `inputId`、`displayName`、`deliverySystem`、`frequencyHz`、`streamId`、`streamIdType`、`physicalChannel`、`satelliteBand`、`remoteControlKeyId` を持つ。backend名、driver名、px4相対slot等のbackend固有値は永続channel tune identityへ保存しない。CS110 は `streamIdType="NONE"` とし、`streamId` は null とする。

### 旧 event field / indexed JNI の廃止

`arib_si_engine_rs` の SI event DTO は旧 `canonicalGenres` フィールドを出力しない。Rust parser は Android canonical genre を決定しないため、`nativeGetEventCanonicalGenre()`、`nativeGetEventCanonicalGenresJson()` は互換シンボルとしても残さない。provider-data に保持する canonical genre 投影結果は、TIS が明示写像表で決定した値だけとする。

`nativeGetEventCount()` と `nativeGetEvent*` indexed JNI getter 群は廃止する。EIT event の通常境界は `nativeSnapshotBulkJson()` による bulk snapshot と provider-data builder API のみとする。未使用・廃止予定・互換専用の JNI シンボル、Kotlin private external 宣言、呼び出し不能な indexed path をリリース物へ残してはならない。互換のための空配列返却や空文字返却も禁止する。

### JSON Schema / schema 整合確認データ

現行仕様では Rust serde struct を SSOT としつつ、`arib_si_engine_rs/schema/program_provider_data_v1.schema.json`、`arib_si_engine_rs/schema/descriptor_diagnostic_v1.schema.json`、schema 整合確認データを置く。`ProgramProviderDataV1` の JSON Schema は、top-level と nested オブジェクト の双方で required 最小フィールド と `additionalProperties: true` を併用し、固定済み フィールドを検証しながら ARIB descriptor 拡張を保持できる形にする。schema 整合確認データは `arib_si_engine_rs/testdata/program_provider_data_v1/minimal_clear_program.json` と `tis/tests/assets/program_provider_data_v1/minimal_clear_program.json` の双方に バイト単位で同一 に複製して置く。これは Rust host test と Android instrumentation asset packaging の参照経路が異なるためであり、2つの内容差分は違反とする。Rust test と Kotlin test は同じ内容の テストデータを読み、Rust JSON -> Kotlin round-trip と Kotlin input -> Rust build -> schema 整合確認データとの一致 を確認する。

### 現行実装との関係

文書上の正式 schema は本節を正とする。既存実装に flat JSON 生成、`eventGroupText`、`freeCaText`、`seriesName`、`canonicalGenres`、indexed JNI getter などの旧境界が残っている場合、それは実装未達であり、完成済み仕様として扱わない。旧境界は互換経路として残さず削除する。本節は文書・schema・schema 整合確認データの整合を固定する。`provider_data.rs` は serde_json ベースの ProgramProviderDataV1 / ChannelProviderDataV1 構造を通常経路とし、canonical JSON生成と安定キー抽出をこの境界へ閉じる。既存のprovider-data `signature`フィールド/API、SHA-256計算、JSON断片のraw流用、flat event DTO、indexed JNI getterは実装未達として扱い、リリース物へ残してはならない。

## event_group_descriptor の provider-data 契約

`event_group_descriptor` は現行仕様で構造化変換する。`group_type=0x1` は `shared`、`0x2` / `0x4` は `relay`、`0x3` / `0x5` は `movement` として provider-data JSON の `relatedItems` に保存する。ONID / TSID / service_id / event_id は数値のまま保持する。現行仕様では一般 UI や予約追従へ接続しない。予約追従へ接続する場合は、event identity と authoritative 条件を設計正本へ固定し、安全に確定できる場合だけにする。

## series_descriptor の provider-data と標準列連携

`series_descriptor` は現行仕様で構造化変換する。`series_id` と episode number は TIS が `ARIB_SI_EPG_TvProvider投影方針.md` に従って Android 標準列へ投影できるように出力する。last episode number は通常の `TvContract.Programs` に自然対応する標準列がないため標準列候補として扱わず、repeat label、program pattern、expire date、series name と合わせて provider-data JSON の series 構造に保持する。series name は番組表表示 title を置換する値として扱わない。

## free_CA_mode / 音声言語 / 視聴年齢制限の構造化契約

EIT `free_CA_mode` は ARIB 運用上の無料/有料区分として保持し、TS component の実スクランブル状態、CAS 権利状態、カード状態、CAS HAL 状態とは別軸とする。TIS は AOSP 契約に従う TvProvider 投影を `ARIB_SI_EPG_TvProvider投影方針.md` に従って行うが、`free_CA_mode` 単独から実 descramble の要否またはライブ再生可否を導出しない。実スクランブル状態は `transport_scrambling_control` 等の別情報から判定する。音声 ISO639 language は PMT / 音声コンポーネントdescriptor 等から取得できる値だけを保持し、取得不能時に推測しない。視聴年齢制限は既存レーティングドメインへ変換できる構造化値と、未対応・不正・reserved の診断情報を分離して保持する。

## PSI/SIのTable ID規則と意味解釈の責務

Tuner HALは汎用的なMPEG-TS sectionの伝送処理（ペイロード抽出、sectionの区切り、宣言長の検査、任意のCRC検査、フィルター照合、queueまたはFMQへの配送、伝送診断）だけを担当する。PAT、CAT、PMT、NIT、SDT、BAT、EIT、TDT、TOT、BIT、NBIT、LDT、CDT、PCAT、SDTT、AIT、AMTを含む表固有の意味解析、正規化、複数sectionの集約、意味オブジェクトの生成は`arib_si_engine_rs`とTISが担当し、Tuner HALへ戻さない。

TSの伝送構文、`table_id`別のsection長上限、CRCとraw配送条件、公開フィルター状態は`../tuner_hal/DESIGN_JA.md`の「セクションフィルターの条件幅とsection長上限」を正とする。本crateは、それらの条件を満たして上位から入力されたsectionについてだけ、次表の意味解釈を担当する。予約済み、未割り当て、私用、外部所有の`table_id`を型付き意味オブジェクトとして推測しない。

### 意味解釈の責務

| 対象 | 主なtable ID | 意味解釈の責務 | Tuner HALの処理 | 配送規則 | 禁止事項 | 理由 |
|---|---|---|---|---|---|---|
| すべてのPSI/SI | PAT 0x00、CAT 0x01、PMT 0x02、NIT 0x40/0x41、SDT 0x42/0x46、BAT 0x4A、EIT 0x4E-0x6F、TDT 0x70、TOT 0x73、BIT 0xC4、AMT 0xFE、私用・将来用ID | TISまたはTuner HALより上位の要求元 | 汎用sectionフィルターの照合、外形処理、宣言長・CRC処理、メタデータとバイト列の配送だけ | 条件に一致する完全なsectionは、要求元の有効な経路へすべて配送する。条件に一致しないsectionだけを配送対象外とし、`table_id`を理由に無言で破棄しない | 表ごとの意味解析・正規化・オブジェクト生成、EPG・時刻・アプリケーションDBの更新、特定の`table_id`に対する固定破棄、HAL内の意味別振り分け | AOSP Tuner HALのsection APIは、PSI/SI表ごとの意味APIではなく、汎用のsection転送を公開しているため |

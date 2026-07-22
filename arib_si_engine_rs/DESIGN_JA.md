# arib_si_engine_rs 設計判断

## 責務

`arib_si_engine_rs` は、Tuner HAL → framework/JNI/Tuner SDK API → TIS → arib_si_engine_rs という経路で渡された PSI/SI section payload と TIS 側 メタデータを入力として、PSI/SI/EIT descriptor の 意味解析 を Rust で実装する。PMT/CAT の CA_descriptor から得られる CA_system_id、ECM PID、EMM PID と、SDT 等から得られる free_CA_mode / scrambling flag、サービス識別子 補助情報を含む CA情報 / サービスメタデータ意味モデル も arib_si_engine_rs / TIS 側の責務とする。raw TS packet demux、PID filter、section assembly、section payload delivery は Tuner HAL の責務であり、arib_si_engine_rs に重複実装しない。Tuner HAL を CA情報 / サービスメタデータ意味モデル の生成者またはSSOTにしない。


## ARIB 文字列 decoder の適用範囲

自前の ARIB 文字列 decoder は、サービス名、番組名、短形式イベント、長形式イベント、各種 descriptor のテキストなど、字幕以外の SI/EPG 文字列に限定して使う。字幕 PES、字幕管理データ、字幕本文、外字・DRCS を含む字幕表示処理は `libaribcaption` の責務とし、`arib_si_engine_rs` の自前 decoder に字幕用 ARIB B24 decoder としての完全性を 対応宣言しない。

未対応の SI/EPG 文字・escape は `panic` させず、置換文字または 診断によって安定動作させる。字幕 payload を `decode_arib_string_lossy()` に渡す経路は禁止する。字幕本文処理は TIS 側の libaribcaption 経路だけで行う。
`arib_si_engine_rs` は libaribcaption ラッパー を所有しない。libaribcaption は TIS 側の字幕 path から Rust JNI boundary と 安全なRustラッパー 経由で呼ぶ。

ARIB文字列decoderの初期状態は ARIB STD-B24 の SI/EPG 前提に合わせ、G0=Kanji、G1=Alphanumeric、G2=Hiragana、G3=Macro、GL=LS0(G0)、GR=LS2R(G2) とする。ESCによるdesignation/invocation、LS0/LS1/LS2/LS3、LS1R/LS2R/LS3R、SS2/SS3 は、字幕ではなくSI/EPG文字列の安定復号に必要な範囲で扱う。


## EIT 範囲

現行仕様は EIT p/f を主経路とする。EIT schedule actual `0x50..0x5F` は、scan/setup 後に `TvProvider.Programs` へ最低限の初期番組情報を出すための短期補完に限って利用する。schedule actual を常時収集や長期 EPG 収集として扱わない。EIT schedule other `0x60..0x6F`、長期 schedule window、サービス横断 EPG 更新、予約録画と追従録画の高度利用は、この文書の現行仕様としては接続しない。

## descriptor 変換

表示・保存対象として扱う EIT descriptor は現行仕様で構造化変換する。TvProvider 標準列への投影は tv 直下の `ARIB_SI_EPG_TvProvider投影方針.md` を正とし、`internal_provider_data` の具体 schema / canonical encode / 署名 は本 crate の Rust provider-data serde構造体を SSOT とする。同文書で標準列投影が固定されている component、音声コンポーネント、コンテンツジャンル、Android canonical genre、free_CA_mode、視聴年齢制限、series id、episode number、last episode number、音声言語は provider 用 フィールドとして出せる。series の完全構造、イベントグループ、linkage、unknown、診断JSON など標準列へ自然対応しない項目は、JSON v1 `internal_provider_data` に構造化保存し、同時に診断 API でも観測できるようにする。

`arib_si_engine_rs` は Android canonical genre の写像表をSSOTとして所有しない。

本 crate は provider-data schema、canonical encode、署名、保存上限、診断 schema の正本を所有する。TvProvider標準列への投影判断は `ARIB_SI_EPG_TvProvider投影方針.md`、TIS runtime での書き込み契機、retry、現在番組解決、視聴セッション利用は `tis/DESIGN_JA.md` を正とする。

content_descriptor 由来のARIB分類、表示文字列、user_nibble を構造化して出力し、TIS が `ARIB_SI_EPG_TvProvider投影方針.md` の明示写像表に基づいて `Programs.COLUMN_CANONICAL_GENRE` へ入れる値を決定する。

## parental_rating_descriptor の構造化契約

`arib_si_engine_rs` は `parental_rating_descriptor` を診断文字列だけに落とさず、TIS が `TvContentRating` へ変換できる構造化データとして出力する。

出力する最小フィールドは次とする。

```text
parental_rating_descriptor:
  entries[]:
    country_code
    rating_value        # ARIB B10 Rating 8 uimsbf を8bit値のまま保持する
    raw_rating_byte     # 元8bitレーティング値
  raw_descriptor_bytes
  parse_status          # ok / malformed_length / truncated_descriptor / unsupported_value
```

`arib_si_engine_rs` は Android `TvContentRating` の domain 名や flattened string をSSOTとして決めない。Android TvProvider列への投影と `TvContentRating` 生成は TIS 側の責務とし、投影方針は tv 直下の `ARIB_SI_EPG_TvProvider投影方針.md` をSSOTとする。

未対応 country_code、未定義 rating_value、不正 descriptor は破棄せず、`parse_status` と 診断JSON に保持する。未対応値を推測で一般ユーザー向け レーティングに変換してはならない。

## BS / CS110 discovery

BS と CS110 の complete 判定には BAT、SDT other、NIT other を含める。これらは table_id だけの global 完了ではなく、table_extension と NIT/BAT transport loop から得た ONID/TSID scope を使って transport 単位で判定する。リモコンキー が得られない場合は service_id を表示番号の代替値 とする。

partial snapshot は サービス単位の登録可能判定に使ってよい。ただし partial snapshot を無条件に channel 登録へ出してはならない。global complete 判定だけで publish 可否を決めず、サービス / transport 単位の `publishability_by_service` と 登録可能判定で、service_id、TSID、ONID、PMT、PCR、必要 table、現行ライブ視聴対応 video ES の欠落理由を分離する。登録可能サービスは、ONID / TSID / SID、PMT PID と PMT、有効 PCR、現行ライブ視聴対応 video ES、後続更新可能な internal key を持つ サービスに限定する。audio は必須ではなく、video-only サービスは登録可能として扱い、audio absent / unsupported を診断に残す。audio-only サービスは AOSP/TIF 上は `VIDEO_UNAVAILABLE_REASON_AUDIO_ONLY` に該当するため、登録可能snapshot には含めない。scrambled サービスは 登録可能 として channel 登録してよいが、現行の平文ライブ視聴成功対応宣言対象にはしない。登録可能未満の partial snapshot は 診断情報 / ライブ更新 / debugに限定し、channel insert に使わない。

## section 更新

PAT/PMT/SDT/NIT/BAT/EIT の version 更新では collector 全体を捨てない。table 単位、section 単位、サービス 単位で差分更新する。

EIT は section version 更新で消えた event を削除候補として扱い、TvProvider / TIS 側へ stable identity として `original_network_id / transport_stream_id / service_id / event_id` を提供する。section 更新後の event set が空になった場合も no-op として破棄せず、サービスキー、更新区間、空の valid event identity set を JNI/TIS へ返す。TIS は、Rust parser が `deletionAuthoritative=true` と判定した snapshot だけを obsolete Programs delete に使う。

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

EIT event の stable key は `original_network_id / transport_stream_id / service_id / event_id` とし、開始時刻は表示・更新用 フィールドとして別に扱う。bulk snapshot DTO と `ProgramProviderDataV1.programKey` は stable identity を含む。TIS/TvProvider は `event_id + start_time` に依存した stable key を作らない。旧 indexed JNI getter である `nativeGetEventStableIdentity()` は提供しない。

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

Rust provider-data builder は、受け渡し用 JSON を serde 型へ読み込み、必須項目、型、値域、旧形式混入を検査する。検査に通った入力だけから、保存用 JSON、署名、識別子、切り詰め診断を生成する。

保存用 JSON の schema、正規化、署名、識別子抽出、サイズ上限処理は Rust が単独で所有する。TIS は保存用 JSON を直接生成してはならない。

受け渡し用形式の schema 名は `maleicacid.tv.programRequest` / `maleicacid.tv.channelRequest` とし、保存用 schema 名 `maleicacid.tv.program` / `maleicacid.tv.channel` と分離する。

受け渡し用形式と保存用形式は別物である。受け渡し用形式を `Programs.COLUMN_INTERNAL_PROVIDER_DATA` / `Channels.COLUMN_INTERNAL_PROVIDER_DATA` に保存してはならない。

required field 欠落時に `0`、`false`、`jpn`、`UNKNOWN`、空文字で補完して provider-data を成立させてはならない。r50 以前の `;` 区切り形式、旧 flat provider-data、旧 provider-data 断片は受け渡し用形式としても保存用形式としても拒否する。

`DescriptorDiagnosticV1` は Rust が生成した正規 JSON を正とする。TIS から戻ってくる場合も、TIS が項目単位で再構築した JSON ではなく、Rust が生成した正規 JSON を透過保持したものだけを受ける。


`arib_si_engine_rs` は SI/EIT 意味解析 に加えて、TvProvider `internal_provider_data` JSON v1 の構造 SSOT を持つ。実装上は `provider_data` module に Rust `serde` struct を置き、JSON canonical encode、正規化、署名、安定キー抽出 をこの module に閉じる。

Programs の `internal_provider_data` には、`requiresCas`, `unsupportedCas`, `clearLivePlaybackSupported`, `channelRegistrationReady`, `epgPublishable`, `publishStateSource` 相当の CAS / 準備状態を `cas` または診断情報に保存する。視聴年齢制限については `countryCode`, `ratingValue`, `rawRatingByte`, `supported`, `parseStatus`, `mappedTvContentRating` 相当の情報を `ratings` または診断情報に保存する。現在の診断情報が完全であれば、その値を Programs CAS 状態の正とする。診断情報が欠落または不完全な場合、既存 channel の `internal_provider_data` から CAS / 準備状態を代替参照して Programs 側に保存する。channel 側だけに保存して Programs 側を false に落としてはならない。

provider-data 全体は 16 KiB を目安上限、32 KiB を絶対上限とする。絶対上限を超える場合は、識別子、時刻、CAS 状態、レーティングを保持し、診断情報と長文補助情報を切り詰める。切り詰め時は `DIAGNOSTICS_TRUNCATED` または `PROVIDER_DATA_TRUNCATED` 診断と dropped count を必ず保存する。

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

### canonical JSON / 署名

canonical JSON は Rust `serde_json` で生成し、struct フィールド順序 と `BTreeMap` により出力順序を固定する。provider-data 署名 は TvProvider に実際に書く UTF-8 JSON バイト列の SHA-256 lowercase hex とする。

### JNI boundary

Rust は少なくとも以下の JNI API 相当を提供する。

```text
buildProgramProviderData(inputJson) -> ProviderDataResult
normalizeProgramProviderData(rawBytes) -> ProviderDataResult
appendCurrentProgramDiagnostics(rawBytes, overlapCount, selectedProgramId, selectionRule) -> ProviderDataResult
programProviderDataSignature(rawBytes) -> String
extractProgramKey(rawBytes) -> ProgramKeyResult?
```

`inputJson` は Rust builder への入力 DTO であり、TvProvider 保存 schema ではない。最終 provider-data bytes と 署名 は Rust が返す。

`rawBytes` は任意バイナリではなく、既存 TvProvider に保存済みの JSON v1 UTF-8 バイト列を指す。JNI 呼び出し元は provider-data を `String` 化して渡してはならず、保存済み BLOB バイト列をそのまま渡す。互換上 TvProvider が文字列として返す場合も、呼び出し元は UTF-8 バイト列へ戻すだけに限定し、provider-data JSON を Kotlin 側で解釈・再構築しない。

Rust は `rawBytes` が invalid UTF-8 または malformed JSON の場合、通常実行経路では panic せず、`ProviderDataResult` の空結果または key 抽出失敗へ落とす。`programProviderDataSignature(rawBytes)` は入力 `rawBytes` そのものの SHA-256 lowercase hex を返す。`buildProgramProviderData(inputJson)` と `normalizeProgramProviderData(rawBytes)` が返す `ProviderDataResult.signature` は、返却する canonical provider-data bytes に対する署名であり、入力 raw bytes の署名ではない。



### current-program 診断情報

現在番組選択の診断情報は `ProgramProviderDataV1.diagnostics.currentProgram` にだけ保存する。構造は少なくとも `overlapCount`、`selectedProgramId`、`selectionRule` を持つ。`selectedProgramId` は補助診断であり、provider-data 署名の意味上の identity には使わない。`appendCurrentProgramDiagnostics()` は JSON の末尾を削る文字列連結ではなく、Rust `serde` 構造体へ読み戻して `diagnostics.currentProgram` を更新し、canonical JSON として再出力する。

### ChannelProviderDataV1

Channel provider-data の正形式は JSON v1 のみとし、schema は `maleicacid.tv.channel` / `schemaVersion=1` とする。`arib_si_engine_rs/schema/channel_provider_data_v1.schema.json` は、channel row の tune 復元に必要な `inputId`、物理選局情報、ONID / TSID / service_id、表示名、登録可能性診断を検証対象にする。r50 以前の `;` 区切り key-value 形式、旧 flat provider-data、旧 provider-data 断片は読み取り互換入力としても残さない。 Channel provider-data の top-level envelope は `schema="maleicacid.tv.channel"`, `schemaVersion=1`, `serviceKey`, `tune`, `cas`, `diagnostics` を持つ JSON v1 とする。`tune` は `inputId`、`displayName`、`deliverySystem`、`frequencyHz`、`streamId`、`streamIdType`、`physicalChannel`、`backendHint`、`satelliteBand`、`remoteControlKeyId` を持つ。CS110 は `streamIdType="NONE"` とし、`streamId` は null とする。

### 旧 event field / indexed JNI の廃止

`arib_si_engine_rs` の SI event DTO は旧 `canonicalGenres` フィールドを出力しない。Rust parser は Android canonical genre を決定しないため、`nativeGetEventCanonicalGenre()`、`nativeGetEventCanonicalGenresJson()` は互換シンボルとしても残さない。provider-data に保持する canonical genre 投影結果は、TIS が明示写像表で決定した値だけとする。

`nativeGetEventCount()` と `nativeGetEvent*` indexed JNI getter 群は廃止する。EIT event の通常境界は `nativeSnapshotBulkJson()` による bulk snapshot と provider-data builder API のみとする。未使用・廃止予定・互換専用の JNI シンボル、Kotlin private external 宣言、呼び出し不能な indexed path をリリース物へ残してはならない。互換のための空配列返却や空文字返却も禁止する。

### JSON Schema / schema 整合確認データ

現行仕様では Rust serde struct を SSOT としつつ、`arib_si_engine_rs/schema/program_provider_data_v1.schema.json`、`arib_si_engine_rs/schema/descriptor_diagnostic_v1.schema.json`、schema 整合確認データを置く。`ProgramProviderDataV1` の JSON Schema は、top-level と nested オブジェクト の双方で required 最小フィールド と `additionalProperties: true` を併用し、固定済み フィールドを検証しながら ARIB descriptor 拡張を保持できる形にする。schema 整合確認データは `arib_si_engine_rs/testdata/program_provider_data_v1/minimal_clear_program.json` と `tis/tests/assets/program_provider_data_v1/minimal_clear_program.json` の双方に バイト単位で同一 に複製して置く。これは Rust host test と Android instrumentation asset packaging の参照経路が異なるためであり、2つの内容差分は違反とする。Rust test と Kotlin test は同じ内容の テストデータを読み、Rust JSON -> Kotlin round-trip と Kotlin input -> Rust build -> schema 整合確認データとの一致 を確認する。

### 現行実装との関係

文書上の正式 schema は本節を正とする。既存実装に flat JSON 生成、`eventGroupText`、`freeCaText`、`seriesName`、`canonicalGenres`、indexed JNI getter などの旧境界が残っている場合、それは実装未達であり、完成済み仕様として扱わない。旧境界は互換経路として残さず削除する。本節は文書・schema・schema 整合確認データの整合を固定する。`provider_data.rs` は serde_json ベースの ProgramProviderDataV1 / ChannelProviderDataV1 構造を通常経路とし、canonical JSON 生成、署名、安定キー抽出をこの境界へ閉じる。既存 JSON 断片の raw 流用、flat event DTO、indexed JNI getter は実装未達として扱い、リリース物へ残してはならない。

## event_group_descriptor の provider-data 契約

`event_group_descriptor` は現行仕様で構造化変換する。`group_type=0x1` は `shared`、`0x2` / `0x4` は `relay`、`0x3` / `0x5` は `movement` として provider-data JSON の `relatedItems` に保存する。ONID / TSID / service_id / event_id は数値のまま保持する。現行仕様では一般 UI や予約追従へ接続しない。予約追従へ接続する場合は、event identity と authoritative 条件を設計正本へ固定し、安全に確定できる場合だけにする。

## series_descriptor の provider-data と標準列連携

`series_descriptor` は現行仕様で構造化変換する。`series_id`、episode number、last episode number は TIS が Android 標準列へ自然対応として投影できるように出力する。repeat label、program pattern、expire date、series name は provider-data JSON に保持する。series name は番組表表示 title を置換する値として扱わない。

## free_CA_mode / 音声言語 / 視聴年齢制限の構造化契約

EIT `free_CA_mode` は CAS 権利状態ではなく番組の暗号化有無として保持し、TIS が TvProvider scrambled 判定へ投影する。音声 ISO639 language は PMT / 音声コンポーネントdescriptor 等から取得できる値だけを保持し、取得不能時に推測しない。視聴年齢制限 は既存レーティングドメイン へ変換できる構造化値と、未対応・不正・reserved の診断情報を分離して保持する。

## PSI/SIのTable ID規則と意味解釈の責務

Tuner HALは汎用的なMPEG-TS sectionの伝送処理（ペイロード抽出、sectionの区切り、宣言長の検査、任意のCRC検査、フィルター照合、queueまたはFMQへの配送、伝送診断）だけを担当する。PAT、CAT、PMT、NIT、SDT、BAT、EIT、TDT、TOT、BIT、NBIT、LDT、CDT、PCAT、SDTT、AIT、AMTを含む表固有の意味解析、正規化、複数sectionの集約、意味オブジェクトの生成は`arib_si_engine_rs`とTISが担当し、Tuner HALへ戻さない。

1021区分は`section_length <= 1021`かつsection全体`<= 1024`、拡張区分は`section_length <= 4093`かつsection全体`<= 4096`とする。予約済み、未割り当て、私用、外部所有の`table_id`をARIB SIの型付き意味オブジェクトとして推測しない。ただし、Tuner SDKまたはTISが有効なsectionフィルターで選択した汎用の生sectionは、意味解析が未対応であることだけを理由にTuner HALが破棄してはならない。本crateは入力されたペイロードを次表に従って型付きで解析するか、未対応または不明な構造として保持する。

下表の`STD-B10 5.13-E1`は、英語版で参照箇所を特定するための基準である。現行STD-B10 5.14との対応は、`../tuner_hal/DESIGN_JA.md`の「ARIB現行版との対応」に従い、5.10から5.14までの公式改定履歴を照合する。英語版と5.14日本語版全文の同一性、および未照合箇所への適用は主張しない。

### Table ID別section長上限

| 規格 | `table_id`または範囲 | 表名 | `section_length`上限 | section全体の上限バイト数 | 解析責務 | 根拠箇所 | 配送区分 |
|---|---|---|---|---|---|---|---|
| STD-B10 5.13-E1 | 0x40-0x41 | NIT actual/other | 1021 | 1024 | TISまたはTuner HALより上位の要求元 | 5.2.4、`section_length`の定義、PDF印刷ページ89〜90 | 汎用section配送、意味解釈は要求元が所有 |
| STD-B10 5.13-E1 | 0x4A | BAT | 1021 | 1024 | TISまたはTuner HALより上位の要求元 | 5.2.5、PDF印刷ページ92〜93 | 汎用section配送、意味解釈は要求元が所有 |
| STD-B10 5.13-E1 | 0x42,0x46 | SDT actual/other | 1021 | 1024 | TISまたはTuner HALより上位の要求元 | 5.2.6、PDF印刷ページ95〜97 | 汎用section配送、意味解釈は要求元が所有 |
| STD-B10 5.13-E1 | 0x4E-0x6F | EIT p/f、schedule | 4093 | 4096 | TISまたはTuner HALより上位の要求元 | 5.2.7、PDF印刷ページ98〜101 | 汎用section配送、意味解釈は要求元が所有 |
| STD-B10 5.13-E1 | 0x70 | TDT | 5 | 8 | TISまたはTuner HALより上位の要求元 | 5.2.8、表5-8 | 汎用section配送、意味解釈は要求元が所有 |
| STD-B10 5.13-E1 | 0x71 | RST | 1021 | 1024 | TISまたはTuner HALより上位の要求元 | 5.2.10、PDF印刷ページ103〜104 | 汎用section配送、意味解釈は要求元が所有 |
| STD-B10 5.13-E1 | 0x72 | ST | 4093 | 4096 | TISまたはTuner HALより上位の要求元 | 5.2.11、PDF印刷ページ104〜105 | 汎用section配送、意味解釈は要求元が所有 |
| STD-B10 5.13-E1 | 0xC2 | PCAT | 4093 | 4096 | TISまたはTuner HALより上位の要求元 | 5.2.12、PDF印刷ページ106〜108 | 汎用section配送、意味解釈は要求元が所有 |
| STD-B10 5.13-E1 | 0xC4 | BIT | 4093 | 4096 | TISまたはTuner HALより上位の要求元 | 5.2.13、PDF印刷ページ109〜110 | 汎用section配送、意味解釈は要求元が所有 |
| STD-B10 5.13-E1 | 0xC5-0xC6 | NBIT本体/参照 | 4093 | 4096 | TISまたはTuner HALより上位の要求元 | 5.2.14、PDF印刷ページ110〜114 | 汎用section配送、意味解釈は要求元が所有 |
| STD-B10 5.13-E1 | 0xC7 | LDT | 4093 | 4096 | TISまたはTuner HALより上位の要求元 | 5.2.15、PDF印刷ページ114〜116 | 汎用section配送、意味解釈は要求元が所有 |
| STD-B10 5.13-E1 | 0xD0 | LIT | 4093 | 4096 | TISまたはTuner HALより上位の要求元 | Part 3 5.1.1、LITの `section_length` 定義 | 汎用section配送、意味解釈は要求元が所有 |
| STD-B10 5.13-E1 | 0xD1 | ERT | 4093 | 4096 | TISまたはTuner HALより上位の要求元 | Part 3 5.1.2、ERTの `section_length` 定義 | 汎用section配送、意味解釈は要求元が所有 |
| STD-B10 5.13-E1 | 0xD2 | ITT | 4093 | 4096 | TISまたはTuner HALより上位の要求元 | Part 3 5.1.3、ITTの `section_length` 定義 | 汎用section配送、意味解釈は要求元が所有 |
| STD-B10 5.13-E1 | 0x4C | INT | 4093 | 4096 | TISまたはTuner HALより上位の要求元 | 5.2.17、`section_length` の定義、PDF印刷ページ118〜121 | 汎用section配送、意味解釈は要求元が所有 |
| STD-B10と本設計 | 0x73 | TOT | 1021 | 1024 | TISまたはTuner HALより上位の要求元 | STD-B10の表割り当て、意味責務は下表 | 汎用section配送、意味解釈は要求元が所有 |
| STD-B10と本設計 | 0xFE | AMT | 4093 | 4096 | TISまたはTuner HALより上位の要求元 | STD-B10の表割り当て、意味責務は下表 | 汎用section配送、意味解釈は要求元が所有 |
| STD-B10 5.13-E1 | その他、予約済み、private | Tuner HALに型付きの意味責務を置かない | 構文上の長さだけを検証 | 12ビット長の絶対上限4096 | TISまたはTuner HALより上位の要求元 | 表の割り当てだけを用い、意味を推測しない | 汎用section配送、意味解釈は要求元が所有 |

### 意味解釈の責務

| 対象 | 主なtable ID | 意味解釈の責務 | Tuner HALの処理 | 配送規則 | 禁止事項 | 理由 |
|---|---|---|---|---|---|---|
| すべてのPSI/SI | PAT 0x00、CAT 0x01、PMT 0x02、NIT 0x40/0x41、SDT 0x42/0x46、BAT 0x4A、EIT 0x4E-0x6F、TDT 0x70、TOT 0x73、BIT 0xC4、AMT 0xFE、私用・将来用ID | TISまたはTuner HALより上位の要求元 | 汎用sectionフィルターの照合、外形処理、宣言長・CRC処理、メタデータとバイト列の配送だけ | 条件に一致する完全なsectionは、要求元の有効な経路へすべて配送する。条件に一致しないsectionだけを配送対象外とし、`table_id`を理由に無言で破棄しない | 表ごとの意味解析・正規化・オブジェクト生成、EPG・時刻・アプリケーションDBの更新、特定の`table_id`に対する固定破棄、HAL内の意味別振り分け | AOSP Tuner HALのsection APIは、PSI/SI表ごとの意味APIではなく、汎用のsection転送を公開しているため |

# arib_si_engine_rs 設計判断

## 責務

`arib_si_engine_rs` は、Tuner HAL → framework/JNI/Tuner SDK API → TIS → arib_si_engine_rs という経路で渡された PSI/SI section payload と TIS 側 metadata を入力として、PSI/SI/EIT descriptor の semantic parse を Rust で実装する。PMT/CAT の CA_descriptor から得られる CA_system_id、ECM PID、EMM PID と、SDT 等から得られる free_CA_mode / scrambling flag、service identity 補助情報を含む CA metadata / service metadata semantic model も arib_si_engine_rs / TIS 側の責務とする。raw TS packet demux、PID filter、section assembly、section payload delivery は Tuner HAL の責務であり、arib_si_engine_rs に重複実装しない。Tuner HAL を CA metadata / service metadata semantic model の生成者またはSSOTにしない。


## ARIB 文字列 decoder の適用範囲

自前の ARIB 文字列 decoder は、サービス名、番組名、短形式イベント、長形式イベント、各種 descriptor のテキストなど、字幕以外の SI/EPG 文字列に限定して使う。字幕 PES、字幕管理データ、字幕本文、外字・DRCS を含む字幕表示処理は `libaribcaption` の責務とし、`arib_si_engine_rs` の自前 decoder に字幕用 ARIB B24 decoder としての完全性を claim しない。

未対応の SI/EPG 文字・escape は panic させず、置換文字または diagnostic によって安定動作させる。字幕 payload を `decode_arib_string_lossy()` に渡す経路は r51 の対象外とする。
`arib_si_engine_rs` は libaribcaption wrapper を所有しない。libaribcaption は TIS 側の字幕 path から Rust JNI boundary と safe Rust wrapper 経由で呼ぶ。

ARIB文字列decoderの初期状態は ARIB STD-B24 の SI/EPG 前提に合わせ、G0=Kanji、G1=Alphanumeric、G2=Hiragana、G3=Macro、GL=LS0(G0)、GR=LS2R(G2) とする。ESCによるdesignation/invocation、LS0/LS1/LS2/LS3、LS1R/LS2R/LS3R、SS2/SS3 は、字幕ではなくSI/EPG文字列の安定復号に必要な範囲で扱う。


## EIT 範囲

r51 は EIT p/f を中心に扱う。scan/setup 後に `TvProvider.Programs` へ最低限出す番組情報だけ schedule から拾う。常時 EPG 収集、長期 schedule 収集、予約録画と追従録画の高度利用は r53 とする。

## descriptor 変換

今後表示できる必要がある EIT descriptor は r51 で構造化変換する。TvProvider 標準列への投影は tv 直下の `ARIB_SI_EPG_TvProvider投影方針.md` を正とし、`internal_provider_data` の具体 schema / canonical encode / signature は本 crate の Rust provider-data serde model を SSOT とする。同文書で `Programs.COLUMN_LONG_DESCRIPTION` への補足表示が固定されている component、audio component、content genre、event group、freeCA / isFree は provider 用 field として出せる。series、linkage、unknown、diagnostic JSON など同文書で r51 標準列投影対象外とした項目は、JSON v1 `internal_provider_data` に構造化保存し、同時に診断 API でも観測できるようにする。

`arib_si_engine_rs` は Android canonical genre の写像表をSSOTとして所有しない。content_descriptor 由来のARIB分類と表示文字列を構造化して出力するが、`Programs.COLUMN_CANONICAL_GENRE` へ直接入れる値を決定する責務は持たない。

## parental_rating_descriptor の構造化契約

`arib_si_engine_rs` は `parental_rating_descriptor` を診断文字列だけに落とさず、TIS が `TvContentRating` へ変換できる構造化データとして出力する。

出力する最小フィールドは次とする。

```text
parental_rating_descriptor:
  entries[]:
    country_code
    rating_value        # ARIB B10 Rating 8 uimsbf を8bit値のまま保持する
    raw_rating_byte     # raw 8bit Rating 値
  raw_descriptor_bytes
  parse_status          # ok / malformed_length / truncated_descriptor / unsupported_value
```

`arib_si_engine_rs` は Android `TvContentRating` の domain 名や flattened string をSSOTとして決めない。Android TvProvider列への投影と `TvContentRating` 生成は TIS 側の責務とし、投影方針は tv 直下の `ARIB_SI_EPG_TvProvider投影方針.md` をSSOTとする。

未対応 country_code、未定義 rating_value、不正 descriptor は破棄せず、`parse_status` と diagnostic JSON に保持する。未対応値を推測で一般ユーザー向け rating に変換してはならない。

## BS / CS110 discovery

BS と CS110 の complete 判定には BAT、SDT other、NIT other を含める。これらは table_id だけの global 完了ではなく、table_extension と NIT/BAT transport loop から得た ONID/TSID scope を使って transport 単位で判定する。リモコンキー が得られない場合は service_id を表示番号の代替値 とする。

partial snapshot は service-local registration-ready 判定に使ってよい。ただし partial snapshot を無条件に channel 登録へ出してはならない。global complete 判定だけで publish 可否を決めず、service / transport 単位の `publishability_by_service` と registration-ready 判定で、service_id、TSID、ONID、PMT、PCR、必要 table、r51対応 video ES の欠落理由を分離する。registration-ready service は、ONID / TSID / SID、PMT PID と PMT、有効 PCR、r51対応 video ES、後続更新可能な internal key を持つ service に限定する。audio は必須ではなく、video-only service は登録可能として扱い、audio absent / unsupported を診断に残す。audio-only service は AOSP/TIF 上は `VIDEO_UNAVAILABLE_REASON_AUDIO_ONLY` に該当するため、registration-ready snapshot には含めない。scrambled service は registration-ready として channel 登録してよいが、r51 の clear live 視聴成功 claim 対象にはしない。registration-ready 未満の partial snapshot は diagnostics / live refresh / debug に限定し、channel insert に使わない。

## section 更新

PAT/PMT/SDT/NIT/BAT/EIT の version 更新では collector 全体を捨てない。table 単位、section 単位、service 単位で差分更新する。

EIT は section version 更新で消えた event を削除候補として扱い、TvProvider / TIS 側へ stable identity として `original_network_id / transport_stream_id / service_id / event_id` を提供する。section 更新後の event set が空になった場合も no-op として破棄せず、service key、update window、空の valid event identity set を JNI/TIS へ返す。TIS は、Rust parser が `deletionAuthoritative=true` と判定した snapshot だけを obsolete Programs delete に使う。

EIT event fixed field、start_time BCD、duration BCD、descriptor_loop_length が malformed の event を含む section は、旧 event 削除用の authoritative valid-event-set として扱わない。malformed event は Programs から消すのではなく、旧正常 event を保持したまま diagnostics に記録する。

開始時刻、終了時刻、duration、番組名、説明文の変更は、同一 stable identity の event 更新として扱う。開始時刻は stable identity に含めない。

ただし TvProvider の時間範囲制約、row 更新制約、または TIS 実装都合により provider row の再作成が必要な場合は、旧 provider row を削除して再 insert してよい。その場合でも、内部 stable identity は `original_network_id / transport_stream_id / service_id / event_id` のまま維持する。

## 診断 API

TvProvider に自然に入らない descriptor は構造化した内部データとして `internal_provider_data` に保存し、診断 API にも出す。EIT event ごとの diagnostic 文字列には、content、component、audio component、parental rating、series、event group、linkage、未知 descriptor の数と主要値を含める。

## r51 descriptor 対象

short_event、extended_event、content、component、audio_component、parental_rating、series、event_group、linkage を r51 で構造化変換する。未知 descriptor は破棄せず diagnostic に保持する。

ARIB descriptor は `descriptor_length`、descriptor 内部 length、loop 単位、fragment sequence が妥当な場合だけ正常 field として採用する。length 不整合、trailing byte、fragment 欠落、`descriptor_number` 重複、`last_descriptor_number` 不一致、必須 field 不足は malformed descriptor とし、event name、short text、extended_event text、content genre、component、audio component、series、event_group、linkage の正常 field には採用しない。malformed descriptor は parser を停止させず、`DescriptorDiagnosticV1` に tag、offset、declaredLength、actualRemainingLength、parseStatus、rawPrefixHex、section scope を保持する。

## API 境界の固定

Kotlin/JNI の通常 service snapshot は channel registration 用の `registration_ready_snapshot()` 相当を使う。これは r51 の clear live 視聴 claim 対象だけでなく、service-local registration-ready 条件を満たす scrambled unsupported service も含み得る。clear live 視聴 claim 対象は別途 `clear_live_playback_supported_snapshot()` / `clear_live_playback_supported` で判定する。`publishable_snapshot()` は診断・test 用であり、registration-ready 未満の service を通常 channel 登録経路に出さない。publishable だが r51 live 視聴対象外の service については `publishability_by_service` を JNI 診断として公開し、ONID、TSID、service_id、publishable / channel_registration_ready / epg_publishable / clear_live_playback_supported / requires_cas / unsupported_cas 可否、欠落 component、除外理由を分けて観測する。

PAT は ONID を持たないため、`(transport_stream_id, service_id) -> pmt_pid` をそのまま publishable service identity として扱わない。SDT/NIT/BAT 等で ONID が一意に解決できた場合だけ `(original_network_id, transport_stream_id, service_id, pmt_pid)` へ昇格し、ONID が曖昧な場合は publish 抑止または欠落診断に留める。

EIT event の stable key は `original_network_id / transport_stream_id / service_id / event_id` とし、開始時刻は表示・更新用 field として別に扱う。JNI は `nativeGetEventStableIdentity()` を提供し、TIS/TvProvider は `event_id + start_time` に依存した stable key を作らない。

開始時刻変更によって TvProvider row を削除・再作成する場合でも、TIS / arib_si_engine_rs の stable identity は変更しない。`event_id + start_time` は表示・検索・provider row 再作成補助には使ってよいが、event identity の SSOT にしてはならない。

`nativeGetEventDiagnosticDescriptorJson()` は診断 API であり、TIS はその内容を `internal_provider_data` の内部データとしても保存する。TvProvider の標準 title / description / 時刻列には event name、short text、extended_event text を入れる。さらに `ARIB_SI_EPG_TvProvider投影方針.md` で固定された範囲では、component / audio component / content genre / event group / freeCA 由来の補足を `Programs.COLUMN_LONG_DESCRIPTION` へ整形して出してよい。series、linkage、unknown descriptor、diagnostic JSON は標準列へ出さず内部データに分離する。

自前 ARIB 文字列 decoder は字幕以外の SI/EPG 文字列だけを対象にする。未対応 escape、切り詰め escape、切り詰め漢字、置換文字数は diagnostic summary として観測できる。字幕は `libaribcaption` の責務である。

### 文字 decoder 固定方針

自前 ARIB 文字列 decoder の完了条件は、mirakc が EPG / service model 構築で扱う範囲に合わせる。すなわち、字幕本文レンダリングではなく、サービス名、番組名、短形式イベント記述、長形式イベント記述、各種 SI/EPG descriptor の text field を安定して文字列化する範囲を対象にする。

この範囲を超える字幕 PES、字幕管理データ、字幕本文、DRCS/外字レンダリング、厳密な組版制御は恒久的に `arib_si_engine_rs` の対象外であり、必要な場合は `libaribcaption` 側の責務とする。未対応 escape / 未対応文字は panic ではなく diagnostic と置換文字に落とす。これは r51 の設計方針として固定する。

## mirakc 相当の ARIB 文字列範囲

自前 decoder は mirakc-arib が EPG / service model 構築で文字列化している範囲に限定する。対象は SDT service descriptor の service name、EIT short_event の event name / text、EIT extended_event の item description / item text / text、component descriptor、audio component descriptor、series descriptor の text/name である。

extended_event は、全 fragment の `last_descriptor_number` が一致し、`descriptor_number` が 0 から `last_descriptor_number` まで重複なく連続して揃う場合だけ、`descriptor_number` 順に fragment を連結して ARIB 文字列として復号する。欠番、重複、`last_descriptor_number` 不一致がある場合は extended description / extended items を正常 field に採用せず、diagnostic に記録する。字幕 PES、字幕管理データ、字幕本文、DRCS/外字レンダリング、組版制御、BML は対象外であり、`libaribcaption` 側の責務とする。

## unit test と TvProvider 境界の固定

ARIB SI/EPG文字デコードの受け入れ判定は、単体テストのみで通過してよい。実波 TS ファイル fixture は r51 必須条件にせず、descriptor byte array / section builder による Rust unit test を正式な受け入れ条件にする。対象は SDT service name、EIT short_event、extended_event fragment、extended item、component、audio_component、series、unsupported escape、truncated text、replacement 診断である。

Rust descriptor model から Kotlin/TvProvider へ渡す構造化データ境界では、Rust の EventDescriptors から JNI getter を通して Kotlin `AribEvent` へ extendedItems、componentText、audioComponentText、contentGenreText、eventGroupText、freeCaText、seriesName、diagnosticDescriptorJson を渡す。TvProvider の title / description / long description への投影は `ARIB_SI_EPG_TvProvider投影方針.md` をSSOTとし、同文書で固定済みの component/audio/content/event_group/freeCA 補足は `Programs.COLUMN_LONG_DESCRIPTION` へ出す。series と diagnostic JSON は `internal_provider_data` の TIS 内部データとして保存する。

設計書は現行仕様中心にし、過去の経緯は CHANGELOG.md に分離する。


## r50bi Android rating domain 境界

`arib_si_engine_rs` は ARIB `parental_rating_descriptor` の構造化解析結果だけをSSOTとする。Android `TvContentRating` の `domain` / `ratingSystem` / `rating` 文字列、`flattenToString()`、`Programs.COLUMN_CONTENT_RATING` への投影、`TvInputManager.isRatingBlocked()` に渡す値は TIS 側の責務である。

Rust 側に `com.android.tv` や `ISDB_<age>` の Android domain 決定文字列を持ち込んではならない。Rust は `country_code`, `rating_value`, `raw_rating_byte`, `parse_status`, `raw_descriptor_bytes` を保持し、未対応値を推測変換しない。

## r50bj provider-data / diagnostics Rust SSOT

r50bj 以降、`arib_si_engine_rs` は SI/EIT semantic parse に加えて、TvProvider `internal_provider_data` JSON v1 の構造 SSOT を持つ。実装上は `provider_data` module に Rust `serde` struct を置き、JSON canonical encode、normalize、signature、stable key extraction をこの module に閉じる。

### Rust struct SSOT

少なくとも以下の struct を Rust 側で定義し、Kotlin 側に同名 schema を二重定義しない。

```rust
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgramProviderDataV1 {
    pub schema: String,
    pub schema_version: u32,
    pub program_key: ProgramKeyV1,
    pub service_key: ServiceKeyV1,
    pub timing: ProgramTimingV1,
    pub source: ProgramSourceV1,
    pub cas: CasStateV1,
    pub ratings: Vec<RatingV1>,
    pub genres: Vec<GenreV1>,
    pub audio: Option<AudioMetadataV1>,
    pub video: Option<VideoMetadataV1>,
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

### DescriptorDiagnosticV1

Descriptor diagnostic は Rust が生成し、Kotlin はその JSON object を別 schema に変換してはならない。

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

`SectionScopeV1` は PID、table_id、table_id_extension、version、section_number、ONID、TSID、service_id、event_id を持てる構造とする。unknown numeric を `-1` へ潰さず、`Option` または key omission とする。

`DescriptorScopeV1` は tag、name、offset、declared_length、actual_remaining_length、raw_prefix_hex を持つ。`raw_prefix_hex` は最大64 bytes相当までとする。

### canonical JSON / signature

canonical JSON は Rust `serde_json` で生成し、struct field order と `BTreeMap` により出力順序を固定する。provider-data signature は TvProvider に実際に書く UTF-8 JSON bytes の SHA-256 lowercase hex とする。

### JNI boundary

Rust は少なくとも以下の JNI API 相当を提供する。

```text
buildProgramProviderData(inputJson) -> ProviderDataResult
normalizeProgramProviderData(rawBytes) -> ProviderDataResult
programProviderDataSignature(rawBytes) -> String
extractProgramKey(rawBytes) -> ProgramKeyResult?
```

`inputJson` は Rust builder への入力 DTO であり、TvProvider 保存 schema ではない。最終 provider-data bytes と signature は Rust が返す。

### JSON Schema / golden fixture

r51 では Rust serde struct を SSOT としつつ、`schema/program_provider_data_v1.schema.json`、`schema/descriptor_diagnostic_v1.schema.json`、golden fixture を置く。Rust test と Kotlin test は同じ fixture を読み、Rust JSON -> Kotlin round-trip と Kotlin input -> Rust build -> fixture match を確認する。


# ARIB SI/EPG TvProvider投影方針

## 1. 目的

この文書は、`arib_si_engine_rs` が抽出したARIB SI/EPG情報を Android `TvProvider` の標準列と `internal_provider_data` にどう投影するかを固定する。

この文書では、EDCBとEPGStationから補完できた範囲を TvProvider 標準列への投影として固定する。r51 で標準列への自然な投影を採用しない情報は、TvProvider 標準列や一般ユーザー向け UI 本文へは投影しない。ただし、`internal_provider_data` の schema、key 名、canonical encode、signature、保存上限、diagnostics schema は r50bj2 以降この文書でも固定済みであり、標準列非投影項目とは別に扱う。

## 2. 基本原則

```text
UIに表示させる情報:
  TvProvider標準列へ入れる。

UIに表示させたいが専用標準列がない情報:
  人間向けに整形して Programs.COLUMN_LONG_DESCRIPTION へ入れる。
  完全な構造は internal_provider_data に保存する。

TIS内部だけが使う情報:
  TvProvider の internal_provider_data に置く。

r51でTvProvider標準列へ投影しない情報:
  標準列や一般ユーザー向け UI 本文へは投影しない。
  JSON v1 internal_provider_data へ構造化保存する。
```

`internal_provider_data` は、挿入した TV input service が内部で使う私的データであり、system TV app や他アプリがdecodeする前提にしない。ただし TIS 自身の内部形式は JSON v1 に固定し、`arib_si_engine_rs` の Rust provider-data serde model を SSOT とする。

TvProvider 標準列へ投影する ARIB descriptor 由来値は、Rust parser が構文的に有効な descriptor / event と判定したものに限る。malformed descriptor、fragment 欠落、length 不整合、malformed EIT event 由来の値を title / description / genre / audio / rating / long description の正常 field として投影してはならない。これらは JSON v1 diagnostics にのみ保持する。

## 3. 設計として固定する投影

EDCBとEPGStationの参照から補完できたため、次を設計として固定する。

| ARIB由来データ | TvProvider標準列への投影 | internal_provider_dataへの保存 | 固定理由 |
|---|---|---|---|
| event name | `Programs.COLUMN_TITLE` | event keyと合わせて保持する | EDCB/EPGStationとも番組名として扱う |
| short_event text | `Programs.COLUMN_SHORT_DESCRIPTION` と `Programs.COLUMN_LONG_DESCRIPTION` 冒頭 | 元文字列を保持する | 概要としてUI表示する |
| extended_event text | `Programs.COLUMN_LONG_DESCRIPTION` | 元文字列を保持する | 詳細説明としてUI表示する |
| extended_event item_description / item_text | `Programs.COLUMN_LONG_DESCRIPTION` に `【項目名】本文` としてflatten | extended item listを構造保持 | EPGStationの `extended` はflatten文字列、元構造は `rawExtended` 相当 |
| component_descriptor text | `Programs.COLUMN_LONG_DESCRIPTION` に `映像: ...` として補足 | component構造を保持 | EDCB系UIでは映像情報として表示される |
| audio_component_descriptor text | `Programs.COLUMN_LONG_DESCRIPTION` に `音声: ...` として補足 | audio component構造を保持 | EDCB系UIでは音声情報として表示される |
| audio language | `Programs.COLUMN_AUDIO_LANGUAGE` | audio component構造を保持 | Android標準列がある |
| content genre 大分類 / 中分類 | `arib_si_engine_rs` がARIB分類値とARIB表示名を出力し、TIS がその表示名を `Programs.COLUMN_BROADCAST_GENRE` へ `TvContract.Programs.Genres.encode(...)` 形式で格納する | 元ARIB分類、大分類、中分類、表示文字列を保持 | Android TvProvider には放送規格由来ジャンル用の `COLUMN_BROADCAST_GENRE` があり、ARIB分類を直接 canonical genre と混同しないため |
| Android canonical genre | r51 では TIS の primary projection として直接書き込まない。ARIB分類から `TvContract.Programs.Genres` の定義済み値へ写像する表は r51 の採用対象外であるため、TIS実装は `Programs.COLUMN_CANONICAL_GENRE` を `ContentValues` に設定しない。ただし Android TvProvider は `Programs.COLUMN_BROADCAST_GENRE` から `Programs.COLUMN_CANONICAL_GENRE` を内部補完する場合があるため、TvProvider 読み出し後に canonical genre が非空になることは AOSP 標準動作として許容する。 | 写像元のARIB分類、TISが直接設定したcanonical genreの有無、TvProvider読み出し後のcanonical genreを診断用に区別して保持する | canonical genre は Android 定義済み値の列であり、ARIB分類のSSOTにしないため。また、TISの直接投影責務とTvProviderの内部補完結果を混同しないため |
| content genre UI補足 | `Programs.COLUMN_LONG_DESCRIPTION` に `ジャンル: ...` として補足 | 元ARIB分類を保持 | 準正式案でUI向け補足として固定 |
| event_group_descriptor | `Programs.COLUMN_LONG_DESCRIPTION` に `関連番組: ...` として補足 | event group構造を保持 | EDCBで関連/リレー番組として表示対象 |
| parental_rating_descriptor | `TvContentRating` に変換できる範囲を `Programs.COLUMN_CONTENT_RATING` へ `TvContentRating.flattenToString()` 形式で格納する | country_code、rating値、未対応値、raw descriptorを保持 | Android TIF の parental control は `COLUMN_CONTENT_RATING` と `TvInputService.Session` の content block通知に接続するため |
| freeCA / isFree | `Programs.COLUMN_LONG_DESCRIPTION` に `放送種別: 無料放送/有料放送` として補足 | free_ca_modeを保持 | EDCB/EPGStationでユーザー向け情報として扱う |
| event_id | `Programs.COLUMN_EVENT_ID` | event keyとして保持する | Android標準列がある |
| service name | `Channels.COLUMN_DISPLAY_NAME` | service構造を保持する | チャンネル名としてUI表示する |
| service_id | `Channels.COLUMN_SERVICE_ID` | service keyとして保持する | Android標準列がある |
| transport_stream_id | `Channels.COLUMN_TRANSPORT_STREAM_ID` | service keyとして保持する | Android標準列がある |
| original_network_id | `Channels.COLUMN_ORIGINAL_NETWORK_ID` | service keyとして保持する | Android標準列がある |

## 4. 表示文の固定形式

`Programs.COLUMN_LONG_DESCRIPTION` は、次の順で構成する。

```text
1. short_event text
2. extended_event text
3. extended_event item list
4. component_descriptor text
5. audio_component_descriptor text
6. content genre UI補足
7. event_group_descriptor UI補足
8. freeCA / isFree UI補足
```

各要素の整形は次で固定する。

```text
extended item:
  【<item_description>】<item_text>

item_description が空の場合:
  <item_text>

component_descriptor text:
  映像: <text>

audio_component_descriptor text:
  音声: <text>

content genre UI補足:
  ジャンル: <text>

event_group_descriptor UI補足:
  関連番組: <text>

freeCA / isFree UI補足:
  放送種別: <無料放送または有料放送>
```

空文字列は出力しない。空セクションの見出しも出力しない。セクション間は改行1つで結合する。

## 5. r51でTvProvider標準列へ投影しないもの

以下は、EDCB/EPGStationを参照しても Android TvProvider の標準列または一般ユーザー向け UI 本文への自然な投影を r51 の必須仕様として採用しないため、r51 では標準列へ投影しない。これらも `internal_provider_data` への保存形式は JSON v1 schema と Rust provider-data serde model に従って構造化保存する。

| データ・判断 | r51 の扱い | 標準列へ投影しない理由 |
|---|---|---|
| series_descriptor series_name | JSON v1 `internal_provider_data` の series 構造に保存し、`LONG_DESCRIPTION` や episode 標準列へは出さない | EDCB/EPGStationから Android 標準列への自然な投影が決まらない |
| series episode/count | JSON v1 `internal_provider_data` の series 構造に保存し、Android episode列へは出さない | Android episode列への写像は r51 の標準列投影対象外 |
| linkage_descriptor | JSON v1 `internal_provider_data` の linkage 構造に保存し、標準列へは出さない | Android標準UIでリンク動作を保証できない |
| multi-lingual name の候補列 | r51 で選んだ1文字列だけ標準 title/name へ出し、候補列は JSON v1 `internal_provider_data` に保存する | 日本語優先/端末 locale 優先のUI方針は product 方針に依存する |
| decode diagnostic | JSON v1 `diagnostics.parserDiagnostics` または `diagnostics.descriptorDiagnostics` に保存し、標準列へは出さない | 一般ユーザー向けUI情報ではない |
| publishability diagnostic | JSON v1 `diagnostics.publishDiagnostics` に保存し、標準列へは出さない | 一般ユーザー向けUI情報ではない |
| raw descriptor bytes | JSON v1 diagnostics の `rawPrefixHex` または descriptor 構造に上限内で保存し、標準列へは出さない | UI表示情報ではなく、標準列を肥大化させるため |

この表は「r51で標準列へ投影しないもの」の一覧である。`internal_provider_data` の schema 名、JSON key 名、BLOB サイズ上限、diagnostics key 名、`LONG_DESCRIPTION` 最大長、長文 truncate 方針は r50bj2 以降固定済みであり、この表に含めてはならない。

## 6. 実装契約

現行の実装契約は次とする。

```text
Programs.COLUMN_TITLE:
  event name

Programs.COLUMN_SHORT_DESCRIPTION:
  short_event text を先頭から短縮したもの

Programs.COLUMN_LONG_DESCRIPTION:
  short_event text
  extended_event text
  extended_event item listをflattenしたもの
  component_descriptor text
  audio_component_descriptor text
  content genre UI補足
  event_group_descriptor UI補足
  freeCA / isFree UI補足

Programs.COLUMN_AUDIO_LANGUAGE:
  audio componentから得られる言語情報。

Programs.COLUMN_BROADCAST_GENRE:
  ARIB content_descriptor の大分類 / 中分類を、放送規格由来ジャンル文字列として `TvContract.Programs.Genres.encode(...)` 形式で格納する。
  値はARIB由来であることが分かる表示文字列または内部正規名にし、Android canonical genre と混同しない。
  元の大分類 / 中分類 / raw value / 表示文字列は `internal_provider_data` に保持する。

Programs.COLUMN_CANONICAL_GENRE:
  r51 では TIS が primary projection として直接設定しない。
  将来 TIS が直接設定する場合は、ARIB分類から `TvContract.Programs.Genres` の定義済み canonical genre への写像表を更新対象の設計文書で固定し、写像後の値だけを `TvContract.Programs.Genres.encode(...)` 形式で `ContentValues` に設定する。
  r51 で直接投影対象外とした ARIB 分類を、TIS が推測で canonical genre に入れてはならない。
  ただし Android TvProvider は、`Programs.COLUMN_CANONICAL_GENRE` が未設定の場合でも `Programs.COLUMN_BROADCAST_GENRE` から canonical genre を内部補完する場合がある。したがって TvProvider 読み出し後の `Programs.COLUMN_CANONICAL_GENRE` が非空になることは、TIS の直接投影違反とはみなさない。

Programs.COLUMN_CONTENT_RATING:
  parental_rating_descriptor から TIS が AOSP system-defined ISDB rating domain（`com.android.tv / ISDB / ISDB_<age>`）の `TvContentRating` を作り、`TvContentRating.flattenToString()` の結果を格納する。
  複数 rating を持つ場合は、Android TvProvider の content rating 形式に従って複数の flattened rating を保持する。
  変換できない値、未対応 country_code、未取得 rating は推測で標準列へ入れず、`internal_provider_data` と診断に保持する。
  live session 側で現在番組の rating が未取得または未対応の場合は、parental control 判定では `TvContentRating.UNRATED` として扱う。

Programs.COLUMN_LONG_DESCRIPTION のUI補足:
  ジャンル: ...
  関連番組: ...
  放送種別: 無料放送/有料放送

Programs.COLUMN_INTERNAL_PROVIDER_DATA:
  JSON v1 UTF-8 bytes のみを新規書き込み正形式とする。
  `schema="maleicacid.tv.program"`, `schemaVersion=1`, `programKey`, `serviceKey`, `timing`, `source`, `cas`, `ratings`, `genres`, `audio`, `video`, `diagnostics` を持つ。
  provider-data JSON v1 の構造、canonical encode、normalize、signature、stable key extraction は `arib_si_engine_rs` の Rust provider-data serde model を SSOT とする。
  extended item list、component/audio/series/linkage等の完全構造、decode/publishability/descriptor diagnostics は JSON v1 内に保存する。
```

## 7. テスト方針

最低限、次をテストする。

```text
1. extended item が `【項目名】本文` として LONG_DESCRIPTION に出る。
2. component text が `映像: ...` として LONG_DESCRIPTION に出る。
3. audio component text が `音声: ...` として LONG_DESCRIPTION に出る。
4. genre補足が `ジャンル: ...` として LONG_DESCRIPTION に出る。
5. ARIB content_descriptor は `Programs.COLUMN_BROADCAST_GENRE` に入り、元ARIB分類が `internal_provider_data` に残る。
6. 明示的なARIB→Android canonical genre写像表が固定されていない分類では、TIS が `ContentValues` に `Programs.COLUMN_CANONICAL_GENRE` を直接設定しないことを確認する。TvProvider 読み出し後の `Programs.COLUMN_CANONICAL_GENRE` は、AOSP TvProvider の broadcast genre → canonical genre 内部補完により非空になる場合を許容し、空であることを合格条件にしない。
7. event groupが `関連番組: ...` として LONG_DESCRIPTION に出る。
8. freeCAが `放送種別: ...` として LONG_DESCRIPTION に出る。
9. parental_rating_descriptor が変換可能な場合、`Programs.COLUMN_CONTENT_RATING` に `TvContentRating.flattenToString()` 形式で出る。
10. parental_rating_descriptor の元値、未対応値、raw descriptor は `internal_provider_data` に残る。
11. 未対応 rating は推測で `COLUMN_CONTENT_RATING` に出ない。
12. series_name は r51 では標準列投影対象外のため LONG_DESCRIPTION に出ず、JSON v1 internal_provider_data の series 構造に残る。
13. diagnostic は標準列へ出ず、JSON v1 internal_provider_data の diagnostics 構造に残る。
```

## 8. 今後の固定方法

この文書で r51 標準列投影対象外とした項目を将来標準列へ投影したい場合は、次を満たすこと。

```text
1. どの標準列へ入れるかを明記する。
2. 一般ユーザー向けUIに表示させる理由を明記する。
3. JSON v1 internal_provider_data に残す完全構造を明記し、Rust provider-data serde model と JSON Schema / golden fixture を更新する。
4. unit testで標準列と JSON v1 internal_provider_data の両方を確認する。
5. この文書を更新し、開発規則.mdのリリース物ルールに反しないことを確認する。
```

## r50bj 追加: internal_provider_data JSON v1 schema

`internal_provider_data` の新規書き込み正形式は JSON v1 のみとする。旧 `;` 区切り key-value 形式は legacy input として読み取り移行用に限り許可し、新規書き込みは禁止する。

この文書は TvProvider 標準列への投影方針を固定する。`internal_provider_data` の具体 schema、canonical encode、normalize、signature、stable key extraction は `arib_si_engine_rs` の Rust provider-data serde model を SSOT とする。TIS Kotlin は provider-data JSON を手書き構築しない。

Programs の JSON v1 は以下を基本形とする。

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
  "source": {},
  "cas": {},
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

`programKey` は ONID / TSID / SID / event_id 由来の安定IDであり、start/end/duration を含めない。開始時刻、終了時刻、duration は `timing` と TvProvider 標準列に保持する。

Programs の `internal_provider_data` には、`requiresCas`, `unsupportedCas`, `clearLivePlaybackSupported`, `channelRegistrationReady`, `epgPublishable`, `publishStateSource` 相当の CAS / readiness state を `cas` または diagnostics に保存する。parental rating については `countryCode`, `ratingValue`, `rawRatingByte`, `supported`, `parseStatus`, `mappedTvContentRating` 相当の情報を `ratings` または diagnostics に保存する。

current diagnostic が complete であればその値を Programs CAS 状態の正とする。diagnostic が欠落または不完全な場合、既存 channel の `internal_provider_data` から CAS / readiness 状態を fallback して Programs 側に保存する。channel 側だけに保存して Programs 側を false に落としてはならない。

provider-data 全体は 16 KiB soft limit、32 KiB hard limit とする。hard limit 超過時は identity / timing / CAS state / rating を保持し、diagnostics と長文補助情報を truncate する。

## r50bk6: malformed descriptor / EIT projection rules

- Descriptor diagnostics schema v1 is the single schema for new provider-data writes. Rust emits `schemaVersion=1` and `diagnostics[]`; Kotlin normalizes that schema directly.
- New extended-event items are written as `description/text`. Legacy `key/value` and `itemDescription/itemText` are accepted only as migration input.
- Malformed short/extended/content/audio_component/event_group descriptors must not be partially projected as normal title, description, extended items, genre, audio, or event-group fields.
- Malformed EIT event timing must not be used as evidence that a previously valid event disappeared. A malformed-only section does not create an obsolete-delete window.

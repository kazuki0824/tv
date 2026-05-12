# ARIB SI/EPG TvProvider投影方針

## 1. 目的

この文書は、`arib_si_engine_rs` が抽出したARIB SI/EPG情報を Android `TvProvider` の標準列と `internal_provider_data` にどう投影するかを固定する。

この文書では、EDCBとEPGStationから補完できた範囲だけを設計として固定する。補完できなかった範囲は、この文書では設計判断として固定しない。その実装は当面 `internal_provider_data` のみに保存し、TvProvider標準列や一般ユーザー向けUI本文へは投影しない。

## 2. 基本原則

```text
UIに表示させる情報:
  TvProvider標準列へ入れる。

UIに表示させたいが専用標準列がない情報:
  人間向けに整形して Programs.COLUMN_LONG_DESCRIPTION へ入れる。
  完全な構造は internal_provider_data に保存する。

TIS内部だけが使う情報:
  TvProvider の internal_provider_data に置く。

準正式案で未決定の情報:
  設計として固定しない。
  実装は当面 internal_provider_data のみに置く。
```

`internal_provider_data` は、挿入した TV input service が内部で使う私的データであり、system TV app や他アプリがdecodeする前提にしない。

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
| Android canonical genre | r51 では TIS の primary projection として直接書き込まない。ARIB分類から `TvContract.Programs.Genres` の定義済み値へ写像する表を別途固定するまでは、TIS実装は `Programs.COLUMN_CANONICAL_GENRE` を `ContentValues` に設定しない。ただし Android TvProvider は `Programs.COLUMN_BROADCAST_GENRE` から `Programs.COLUMN_CANONICAL_GENRE` を内部補完する場合があるため、TvProvider 読み出し後に canonical genre が非空になることは AOSP 標準動作として許容する。 | 写像元のARIB分類、TISが直接設定したcanonical genreの有無、TvProvider読み出し後のcanonical genreを診断用に区別して保持する | canonical genre は Android 定義済み値の列であり、ARIB分類のSSOTにしないため。また、TISの直接投影責務とTvProviderの内部補完結果を混同しないため |
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

## 5. 設計として固定しないもの

以下は、EDCB/EPGStationを参照してもAndroid TvProvider上の最終判断を一意に固定できないため、この文書では設計として固定しない。

| データ・判断 | 当面の実装 | 固定しない理由 |
|---|---|---|
| `internal_provider_data` のschema名 | 現行の内部形式で保存 | Android固有のprivate schema名はEDCB/EPGStationから決まらないため固定しない |
| `internal_provider_data` のJSON key名 | 内部データとして保存 | Android private BLOB schemaは別途決める必要がある |
| `internal_provider_data` のBLOBサイズ上限 | 実装依存。超過時は標準列へ無理に出さない | EDCB/EPGStationのDB/API制約とTvProvider制約が異なる |
| `LONG_DESCRIPTION` 最大長 | 未固定。必要なら別途決める | Android TV UI側の表示制約が端末依存になり得る |
| 長文省略順序 | 未固定。現時点では入力を標準投影対象範囲だけに留める | 省略規則はUI方針に依存する |
| series_descriptor series_name | `internal_provider_data` のみに保存 | EDCB/EPGStationからAndroid標準列への自然な投影が決まらない |
| series episode/count | `internal_provider_data` のみに保存 | Android episode列への写像条件が未固定 |
| linkage_descriptor | `internal_provider_data` のみに保存 | Android標準UIでリンク動作を保証できない |
| multi-lingual name の優先順位 | 選ばれた1文字列だけ標準列、候補は内部保存 | 日本語優先/端末locale優先が未固定 |
| decode diagnostic | `internal_provider_data` に保存 | 一般ユーザー向けUI情報ではない |
| publishability diagnostic | `internal_provider_data` に保存 | 一般ユーザー向けUI情報ではない |
| raw descriptor bytes | 原則として標準列へ出さない | UI表示情報ではなく肥大化するため |

この表の項目は、標準列へ投影してはならないという意味ではない。別途設計判断が固定されるまでは、標準列へ投影しないという意味である。

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
  将来 TIS が直接設定する場合は、ARIB分類から `TvContract.Programs.Genres` の定義済み canonical genre への写像表を別途固定し、写像後の値だけを `TvContract.Programs.Genres.encode(...)` 形式で `ContentValues` に設定する。
  写像表が未固定のARIB分類を、TISが推測で canonical genre に入れてはならない。
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
  extended item listの完全構造
  component/audio/series等の構造
  diagnostic
  将来固定予定の項目
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
12. series_name は未固定のため LONG_DESCRIPTION に出ず、internal_provider_data に残る。
13. diagnostic は標準列へ出ず、internal_provider_data に残る。
```

## 8. 今後の固定方法

この文書で未固定とした項目を標準列へ投影したい場合は、次を満たすこと。

```text
1. どの標準列へ入れるかを明記する。
2. 一般ユーザー向けUIに表示させる理由を明記する。
3. internal_provider_data に残す完全構造を明記する。
4. unit testで標準列とinternal_provider_dataの両方を確認する。
5. この文書を更新し、開発規則.mdのリリース物ルールに反しないことを確認する。
```

## r50bb7 追加: internal_provider_data schema

`Programs.COLUMN_INTERNAL_PROVIDER_DATA` は `;` 区切りの key-value 形式を維持する。r50bb7 以降、標準列へ推測投影しないARIB固有情報は以下の key に格納する。

- `programKeyB64`: ONID / TSID / SID / event_id 由来の安定ID。
- `extendedItemsB64`: extended_event_descriptor の item 配列JSON。
- `componentTextB64`: component_descriptor の表示用補足。
- `audioComponentTextB64`: audio_component_descriptor の表示用補足。
- `audioLanguageB64`: audio language code。
- `broadcastGenreB64`: TvProvider 標準列へ投影した broadcast genre の元情報。
- `genreSupplementTextB64`: ARIB content_descriptor の補足文字列。
- `eventGroupTextB64`: event_group_descriptor の補足文字列。
- `freeCaTextB64`: free_CA_mode の補足文字列。
- `seriesNameB64`: series_descriptor のシリーズ名。
- `diagnosticTextB64`: 変換診断。
- `descriptorJsonB64`: descriptor 診断JSON。
- `contentRatingsB64`: `TvContentRating.flattenToString()` のカンマ区切り。
- `unsupportedDescriptorJsonB64`: 未対応 country/rating 等、標準列へ推測投影しない情報。
- `videoFormatB64`: codec header から確定した video format / width / height。


## r50bi parental rating / CAS fallback 投影固定

### Programs.COLUMN_CONTENT_RATING

- `parental_rating_descriptor` の `country_code=JPN` かつ `rating_value=4..20` は、TIS が Android `TvContentRating.createRating("com.android.tv", "ISDB", "ISDB_<age>")` で作成し、`flattenToString()` 形式で `Programs.COLUMN_CONTENT_RATING` に保存する。
- JPN 以外、rating 4..20 以外、malformed / truncated descriptor、推測変換が必要な値は `COLUMN_CONTENT_RATING` に投影しない。
- rating 未取得または未対応の番組も EPG から除外しない。Live session の parental control 判定では `TvContentRating.UNRATED` として扱う。

### Programs.COLUMN_INTERNAL_PROVIDER_DATA

Programs の `internal_provider_data` には、`requiresCas`, `unsupportedCas`, `clearLivePlaybackSupported`, `channelRegistrationReady`, `epgPublishable`, `publishStateSource` を保存する。parental rating については `countryCode`, `ratingValue`, `rawRatingByte`, `supported`, `parseStatus`, `mappedTvContentRating` を含む診断JSONを保存する。

current diagnostic が complete であればその値を Programs CAS 状態の正とする。diagnostic が欠落または不完全な場合、既存 channel の `internal_provider_data` から CAS / readiness 状態を fallback して Programs 側に保存する。channel 側だけに保存して Programs 側を false に落としてはならない。

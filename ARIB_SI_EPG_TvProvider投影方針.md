# ARIB SI/EPG TvProvider投影方針

## 1. 目的

この文書は、`arib_si_engine_rs` が抽出したARIB SI/EPG情報を Android `TvProvider` の標準列と `internal_provider_data` にどう投影するかを固定する。

この文書では、EDCBとEPGStationから補完できた範囲を TvProvider 標準列への投影として固定する。現行仕様で標準列へ自然対応できる値だけを部分投影し、自然対応できない情報は TvProvider 標準列や一般ユーザー向け UI 本文へ投影しない。`internal_provider_data` の schema、key 名、正規化、保存上限、診断情報 schema は `arib_si_engine_rs/DESIGN_JA.md` と `arib_si_engine_rs/schema/*.schema.json` を正とし、本書では再定義しない。

## 2. 基本原則

```text
UIに表示させる情報:
  TvProvider標準列へ入れる。

UIに表示させたいが専用標準列がない情報:
  人間向けに整形して Programs.COLUMN_LONG_DESCRIPTION へ入れる。
  完全な構造は internal_provider_data に保存する。

TIS内部だけが使う情報:
  TvProvider の internal_provider_data に置く。

現行仕様でTvProvider標準列へ非投影または部分投影にする情報:
  自然対応できる一部の値だけを標準列へ投影する。
  自然対応できない値は標準列や一般ユーザー向け UI 本文へ投影しない。
  完全な構造は JSON v1 internal_provider_data へ構造化保存する。
```

`internal_provider_data` は、本TISが内部で使う私的データであり、システムTVアプリや他アプリが解釈する前提にしない。保存形式は JSON v1 とし、具体 schema、正規化、安定キー抽出、保存上限は `arib_si_engine_rs` の Rust provider-data serde構造体を SSOT とする。

TvProvider 標準列へ投影する ARIB descriptor 由来値は、Rust parser が構文的に有効な descriptor / event と判定したものに限る。不正 descriptor、fragment 欠落、length 不整合、不正 EIT event 由来の値を title / description / genre / audio / レーティング / long description の正常フィールドとして投影してはならない。これらは JSON v1 診断情報にのみ保持する。

### EIT時刻のTvProvider投影境界

ARIB EIT の `start_time` は日本標準時（JST、UTC+09:00）のMJD + BCDとして解釈し、具体値を `TvProvider.Programs` へ投影するときだけUTCのUnix epoch millisecondへ変換する。端末の既定time zone、夏時間設定、現在のlocaleを変換規則へ混ぜない。

`start_time=0xFFFFFFFFFF` と `duration=0xFFFFFF` はARIB上の有効な未定義値であり、BCD不正や壊れたeventとして扱わない。ただし、両者の組合せで意味が異なる。

- `start_time` または `duration` の片方だけが all-1 の場合は時刻未定状態とする。event自体は確定しており `event_id` は有効である。未定の時刻を0秒、1ミリ秒、固定番組長などの架空値へ置換してはならず、後続EITで具体値が得られた場合は同じ `original_network_id / transport_stream_id / service_id / event_id` のeventとして相関してよい。
- `start_time=0xFFFFFFFFFF` かつ `duration=0xFFFFFF` の場合はevent未定状態とする。event内容自体が未確定であり `event_id` に意味はない。この状態を stable event identity に昇格させず、後続EITの具体eventと `event_id` だけで同一eventとして相関してはならない。

通常の `TvProvider.Programs` row は具体的な開始時刻と終了時刻を必要とするため、時刻未定eventもevent未定状態も、未定状態のまま `Programs` rowを新規作成または時刻更新する対象にしない。これらの未定義値だけを理由に既存の正常な `Programs` rowを削除する根拠にも使わない。

`ProgramProviderDataV1.timing.startUtcMillis / endUtcMillis / durationMillis` は、`TvProvider.Programs` へ実際に投影できる具体的な時刻が揃ったrowの保存表現であり、ARIB EITの全時刻状態を表す一般モデルではない。したがって同schemaの整数必須条件を緩めて未定値を格納するのではなく、未定eventはparser/TISの未投影状態に留める。

## 3. 設計として固定する投影

EDCBとEPGStationの参照から補完できたため、次を設計として固定する。

| ARIB由来データ | TvProvider標準列への投影 | internal_provider_dataへの保存 | 固定理由 |
|---|---|---|---|
| 番組名 | `Programs.COLUMN_TITLE` | イベントキーと合わせて保持する | EDCB/EPGStationとも番組名として扱う |
| 短形式イベント本文 | `Programs.COLUMN_SHORT_DESCRIPTION` と `Programs.COLUMN_LONG_DESCRIPTION` 冒頭 | 元文字列を保持する | 概要としてUI表示する |
| 長形式イベント本文 | `Programs.COLUMN_LONG_DESCRIPTION` | 元文字列を保持する | 詳細説明としてUI表示する |
| extended_event の項目説明 / item_text | `Programs.COLUMN_LONG_DESCRIPTION` に `【項目名】本文` として平坦化 | 長形式イベント項目リストを構造保持 | EPGStationの `extended` は平坦化文字列、元構造は `rawExtended` 相当 |
| component_descriptor の text | `Programs.COLUMN_LONG_DESCRIPTION` に `映像: ...` として補足 | コンポーネント構造を保持 | EDCB系UIでは映像情報として表示される |
| audio_component_descriptor の text | `Programs.COLUMN_LONG_DESCRIPTION` に `音声: ...` として補足 | 音声コンポーネント構造を保持 | EDCB系UIでは音声情報として表示される |
| audio language | `Programs.COLUMN_AUDIO_LANGUAGE` | 音声コンポーネント構造を保持 | Android標準列がある |
| コンテンツジャンル 大分類 / 中分類 | `arib_si_engine_rs` がARIB分類値とARIB表示名を出力し、TIS がその表示名を `Programs.COLUMN_BROADCAST_GENRE` へ `TvContract.Programs.Genres.encode(...)` 形式で格納する | 元ARIB分類、大分類、中分類、表示文字列を保持 | Android TvProvider には放送規格由来ジャンル用の `COLUMN_BROADCAST_GENRE` があり、ARIB分類を直接 canonical genre と混同しないため |
| Android canonical genre | 本文「ARIB分類から Android canonical genre への明示写像表」に一致する分類だけを TIS が `Programs.COLUMN_CANONICAL_GENRE` へ `TvContract.Programs.Genres.encode(...)` 形式で格納する。写像不能分類は直接設定しない。 | 写像元のARIB分類、TIS が直接設定した canonical genre、写像不能理由、TvProvider読み出し後のcanonical genreを診断用に区別して保持する。`arib_si_engine_rs` の SI event DTO は Android canonical genre を出力しない。provider-data に保持する canonical genre 投影結果は TIS が決定した値に限る。 | canonical genre は Android 定義済み値の列であるため、TIS が明示写像できる分類だけを設定し、推測写像を禁止するため |
| コンテンツジャンルUI補足 | `Programs.COLUMN_LONG_DESCRIPTION` に `ジャンル: ...` として補足 | 元ARIB分類を保持 | 準正式案でUI向け補足として固定 |
| event_group_descriptor | 現行仕様では標準列や一般 UI 本文へは出さず、JSON v1 `internal_provider_data.relatedItems` に `shared` / `relay` / `movement` として構造化保存する。予約追従へ接続する場合は、event identity と authoritative 条件を設計正本へ固定してから扱う。 | イベントグループ構造、グループ種別、ONID / TSID / service_id / event_id を保持 | Android標準列に自然対応しないが、予約追従に必要なARIB-native構造であるため |
| parental_rating_descriptor | `TvContentRating` に変換できる範囲を `Programs.COLUMN_CONTENT_RATING` へ `TvContentRating.flattenToString()` 形式で格納する | country_code、レーティング値、未対応値、元記述子を保持 | Android TIF の視聴制限は `COLUMN_CONTENT_RATING` と `TvInputService.Session` の content block 通知に接続するため |
| freeCA / isFree | `Programs.COLUMN_SCRAMBLED` に暗号化有無を格納し、必要に応じて `Programs.COLUMN_LONG_DESCRIPTION` に `放送種別: 無料放送/有料放送` として補足 | free_ca_modeを保持 | EDCB/EPGStationでユーザー向け情報として扱う |
| event_id | `Programs.COLUMN_EVENT_ID` | イベントキーとして保持する | Android標準列がある |
| サービス名 | `Channels.COLUMN_DISPLAY_NAME` | サービス構造を保持する | チャンネル名としてUI表示する |
| service_id | `Channels.COLUMN_SERVICE_ID` | サービスキーとして保持する | Android標準列がある |
| transport_stream_id | `Channels.COLUMN_TRANSPORT_STREAM_ID` | サービスキーとして保持する | Android標準列がある |
| original_network_id | `Channels.COLUMN_ORIGINAL_NETWORK_ID` | サービスキーとして保持する | Android標準列がある |

## 4. 表示文の固定形式

`Programs.COLUMN_LONG_DESCRIPTION` は、次の順で構成する。

```text
1. 短形式イベント本文
2. 長形式イベント本文
3. extended_event item list
4. component_descriptor の text
5. audio_component_descriptor の text
6. コンテンツジャンルUI補足
7. freeCA / isFree UI補足
```

各要素の整形は次で固定する。

```text
長形式イベント項目:
  【<項目説明>】<item_text>

項目説明が空の場合:
  <item_text>

component_descriptor の text:
  映像: <text>

audio_component_descriptor の text:
  音声: <text>

コンテンツジャンルUI補足:
  ジャンル: <text>


freeCA / isFree UI補足:
  放送種別: <無料放送または有料放送>
```

空文字列は出力しない。空セクションの見出しも出力しない。セクション間は改行1つで結合する。

## 5. 現行仕様で標準列非投影または部分投影にするもの

以下は、Android TvProvider の標準列または一般ユーザー向け UI 本文へ機械的に全量投影してはならない情報である。自然対応できる一部の値だけを標準列へ投影し、それ以外は `internal_provider_data` の JSON v1 schema と Rust provider-data serde構造体に従って構造化保存する。

| データ・判断 | 現行仕様の扱い | 境界を設ける理由 |
|---|---|---|
| series_descriptor series_name | JSON v1 `internal_provider_data` の series 構造に保存する。`COLUMN_TITLE` や `COLUMN_EPISODE_TITLE` へ機械的に入れない。 | EIT `event_name_char` の番組表表示名を壊さないため |
| series episode/count / series id | `series_id` は `COLUMN_SERIES_ID` または `COLUMN_MULTI_SERIES_ID` へ、episode number は `COLUMN_EPISODE_DISPLAY_NUMBER` へ、last episode number は `COLUMN_ITEM_COUNT` へ自然対応として出す。repeat_label、program_pattern、expire_date、series_name などの完全構造は JSON v1 `internal_provider_data` に保持する。 | Android 標準列へ自然対応できる値だけを投影し、残りはARIB-native構造として保持するため |
| linkage_descriptor | JSON v1 `internal_provider_data` の linkage 構造に保存し、現行仕様では標準列・一般 UI・予約追従へ接続しない。予約追従へ接続する場合は、event identity と authoritative 条件を設計正本へ固定してから扱う。 | Android標準列に自然対応しないため |
| event_group_descriptor | JSON v1 `internal_provider_data.relatedItems` に `shared` / `relay` / `movement` として構造化保存し、現行仕様では標準列・一般 UI・予約追従へ接続しない。予約追従へ接続する場合は、event identity と authoritative 条件を設計正本へ固定してから扱う。 | Android標準列には自然対応しないが、予約追従に必要なARIB-native構造であるため |
| multi-lingual name の候補列 | 現行仕様で選んだ1文字列だけ標準 title/name へ出し、候補列は JSON v1 `internal_provider_data` に保存する。 | 標準 title/name は1値であり、候補列の全量投影先がないため |
| 復号診断 | JSON v1 `diagnostics.parserDiagnostics` または `diagnostics.descriptorDiagnostics` に保存し、標準列へは出さない。 | 一般ユーザー向けUI情報ではないため |
| 公開可否診断 | JSON v1 `diagnostics.publishDiagnostics` に保存し、標準列へは出さない。 | 一般ユーザー向けUI情報ではないため |
| 元記述子バイト列 | JSON v1 診断情報の `rawPrefixHex` または descriptor 構造に上限内で保存し、標準列へは出さない。 | UI表示情報ではなく、標準列を肥大化させるため |

この表は「現行仕様で標準列非投影または部分投影にするもの」の一覧である。`internal_provider_data` の schema 名、JSON key 名、BLOB サイズ上限、診断情報キー名、`LONG_DESCRIPTION` 最大長、長文切り詰め方針は `arib_si_engine_rs/DESIGN_JA.md` と schema ファイル側で固定し、この表に含めてはならない。

## 6. 実装契約

現行の実装契約は次とする。

```text
Programs.COLUMN_TITLE:
  番組名

Programs.COLUMN_SHORT_DESCRIPTION:
  短形式イベント本文を先頭から短縮したもの

Programs.COLUMN_LONG_DESCRIPTION:
  短形式イベント本文
  長形式イベント本文
  extended_event item listを平坦化したもの
  component_descriptor の text
  audio_component_descriptor の text
  コンテンツジャンルUI補足
  freeCA / isFree UI補足

Programs.COLUMN_AUDIO_LANGUAGE:
  音声コンポーネントから得られる言語情報。

Programs.COLUMN_BROADCAST_GENRE:
  ARIB content_descriptor の大分類 / 中分類を、放送規格由来ジャンル文字列として `TvContract.Programs.Genres.encode(...)` 形式で格納する。
  値はARIB由来であることが分かる表示文字列または内部正規名にし、Android canonical genre と混同しない。
  元の大分類 / 中分類 / 元値 / 表示文字列は `internal_provider_data` に保持する。

Programs.COLUMN_CANONICAL_GENRE:
  現行仕様では、本文「ARIB分類から Android canonical genre への明示写像表」に一致する分類だけを TIS が 主投影として直接設定する。
  写像後の値だけを `TvContract.Programs.Genres.encode(...)` 形式で `ContentValues` に設定する。
  写像不能な ARIB 分類、reserved、extension、others、user_nibble 由来分類を、TIS が推測で canonical genre に入れてはならない。
  Android TvProvider が `Programs.COLUMN_BROADCAST_GENRE` から canonical genre を内部補完する場合があるため、TIS が直接設定した値と TvProvider 読み出し後の値は 診断情報で区別する。

Programs.COLUMN_CONTENT_RATING:
  parental_rating_descriptor から TIS が AOSP system-defined ISDB レーティングドメイン（`com.android.tv / ISDB / ISDB_<age>`）の `TvContentRating` を作り、`TvContentRating.flattenToString()` の結果を格納する。
  複数のレーティングを持つ場合は、Android TvProvider のコンテンツレーティング 形式に従って複数の `flattenToString()` 後のレーティングを保持する。
  変換できない値、未対応 country_code、未取得レーティングは推測で標準列へ入れず、`internal_provider_data` と診断に保持する。
  ライブセッション側で現在番組のレーティングが未取得または未対応の場合は、視聴制限判定では `TvContentRating.UNRATED` として扱う。

Programs.COLUMN_LONG_DESCRIPTION のUI補足:
  ジャンル: ...
  放送種別: 無料放送/有料放送
  イベントグループは LONG_DESCRIPTION に出さない

Programs.COLUMN_INTERNAL_PROVIDER_DATA:
  JSON v1 UTF-8 バイト列のみを新規書き込み正形式とする。
  `schema`, `schemaVersion`, `programKey`, `serviceKey`, `timing`, `source`, `cas`, `ratings`, `genres`, `series`, `relatedItems`, `linkage`, `freeCaMode`, `audioLanguages`, `audio`, `video`, `extendedItems`, `components`, `diagnostics` を最上位フィールドとして持つ。
  provider-data JSON v1 の構造、canonical encode、正規化、安定キー抽出は `arib_si_engine_rs/DESIGN_JA.md` の `ProgramProviderDataV1` / `ChannelProviderDataV1` を SSOT とする。provider-data単体のdigestまたはsignatureは生成せず、同一process内の重複書き込み抑止にはprovider-data bytesを含むTvProvider行全体のpublish fingerprintだけを使用する。現在番組選択の診断は `diagnostics.currentProgram` 配下に保存する。
  長形式イベント項目リスト、component/audio/series/linkage/event_group/free_CA_mode/audioLanguages等の完全構造、decode/publishability/記述子診断情報は JSON v1 内に保存する。
```

## 7. 投影確認観点

投影仕様の確認観点は次とする。本節は実行手順、atest名、成果物名、完了判定の正本ではない。

```text
1. 長形式イベント項目が `【項目名】本文` として LONG_DESCRIPTION に出る。
2. component text が `映像: ...` として LONG_DESCRIPTION に出る。
3. 音声コンポーネント本文が `音声: ...` として LONG_DESCRIPTION に出る。
4. genre補足が `ジャンル: ...` として LONG_DESCRIPTION に出る。
5. ARIB content_descriptor は `Programs.COLUMN_BROADCAST_GENRE` に入り、元ARIB分類が `internal_provider_data` に残る。
6. 明示写像表に一致するARIB分類では、TIS が `ContentValues` に `Programs.COLUMN_CANONICAL_GENRE` を直接設定することを確認する。写像不能分類では直接設定しないことを確認する。
7. イベントグループ が LONG_DESCRIPTION に出ず、provider-data JSON `relatedItems` に保存される。
8. freeCA が `Programs.COLUMN_SCRAMBLED` と provider-data JSON に反映され、UI補足を出す場合は `放送種別: ...` として LONG_DESCRIPTION に出る。
9. parental_rating_descriptor が変換可能な場合、`Programs.COLUMN_CONTENT_RATING` に `TvContentRating.flattenToString()` 形式で出る。
10. parental_rating_descriptor の元値、未対応値、元記述子は `internal_provider_data` に残る。
11. 未対応 レーティングは推測で `COLUMN_CONTENT_RATING` に出ない。
12. `series_id`、episode number、last episode number は自然対応する標準列へ出る。`series_name` は `COLUMN_TITLE` / `COLUMN_EPISODE_TITLE` / `LONG_DESCRIPTION` へ機械的に出ず、JSON v1 internal_provider_data の series 構造に残る。
13. 診断情報は標準列へ出ず、JSON v1 internal_provider_data の 診断情報 構造に残る。
14. EIT `start_time` の具体値はJSTとして解釈してUTC epoch millisへ変換する。片方だけall-1の時刻未定eventは架空の開始・終了時刻で `Programs` へ投影せず、後続EITの同じevent identityと相関できる。`start_time=0xFFFFFFFFFF`かつ`duration=0xFFFFFF`のevent未定状態は `event_id` をstable identityとして扱わず、後続の具体eventと `event_id` だけで相関しない。
```

## 8. 今後の固定方法

この文書で標準列投影対象外とした項目を標準列へ投影対象として追加する場合は、次を満たすこと。

```text
1. どの標準列へ入れるかを明記する。
2. 一般ユーザー向けUIに表示させる理由を明記する。
3. JSON v1 internal_provider_data に残す完全構造を明記し、Rust provider-data serde構造体、`arib_si_engine_rs/schema/program_provider_data_v1.schema.json`、`arib_si_engine_rs/testdata/program_provider_data_v1/minimal_clear_program.json`、`tis/tests/assets/program_provider_data_v1/minimal_clear_program.json` を更新する。2つの schema 整合確認データはバイト単位で同一な複製とし、片方だけを更新してはならない。
4. 標準列と JSON v1 internal_provider_data の両方で投影結果を確認する。
5. この文書を更新し、開発規則.mdのリリース物ルールに反しないことを確認する。
```

## 9. internal_provider_data 参照方針

`internal_provider_data` の具体 schema、正規化、安定キー抽出、保存上限、診断情報 schema は `arib_si_engine_rs/DESIGN_JA.md` の「provider-data / diagnostics Rust SSOT」と `arib_si_engine_rs/schema/*.schema.json` を正とする。本書は TvProvider 標準列への投影規則、一般ユーザー向け本文への補足出力規則、標準列非投影または部分投影の境界だけを固定する。

標準列へ自然対応しない完全構造は JSON v1 `internal_provider_data` に保存する。ただし、そのフィールド定義、正規化、切り詰め、診断情報の詳細を本書で再定義してはならない。

## 10. 不正 descriptor / EIT 投影規則

- 新規 provider-data 書き込みでは `arib_si_engine_rs` の `ProgramProviderDataV1` を provider-data 全体の唯一の schema とする。descriptor 診断情報 schema v1 は `diagnostics.descriptorDiagnostics[]` 配下の要素 schema であり、provider-data 全体の schema ではない。
- extended-event item は `description/text` として書き込む。`key/value` と `itemDescription/itemText` の旧入力形式は受け付けない。
- 不正な short / extended / content / audio_component / event_group descriptor は、通常の title、description、長形式イベント項目、genre、audio、event-group フィールドとして部分投影してはならない。
- ARIBで定義された `start_time=0xFFFFFFFFFF` / `duration=0xFFFFFF` の未定義値は不正timingに含めない。片方だけall-1の場合は `event_id` が有効な時刻未定状態、両方all-1の場合は `event_id` に意味がないevent未定状態として区別する。それ以外の不正な EIT event timing は、以前有効だった event が消滅した根拠にしてはならない。不正 section だけでは 廃止行削除区間 を作らない。


## ARIB分類から Android canonical genre への明示写像表

TIS は次の表に一致する分類だけを `Programs.COLUMN_CANONICAL_GENRE` へ直接設定する。複数の content descriptor がある場合は、写像結果を重複なしで統合して `TvContract.Programs.Genres.encode(...)` に渡す。空集合の場合は列を設定しない。

| ARIB分類 | Android canonical genre | 扱い |
|---|---|---|
| `0x0` ニュース・報道 | `NEWS` | 大分類だけで設定可 |
| `0x1` スポーツ | `SPORTS` | 大分類だけで設定可 |
| `0x3` ドラマ | `DRAMA` | 大分類だけで設定可 |
| `0x4` 音楽 | `MUSIC` | 大分類だけで設定可 |
| `0x5` バラエティ | `ENTERTAINMENT` | 大分類で設定可。中分類に応じて追加 genre を付ける |
| `0x5/0x3` お笑い・コメディ | `ENTERTAINMENT`, `COMEDY` | 中分類で追加 |
| `0x5/0x4` 音楽バラエティ | `ENTERTAINMENT`, `MUSIC` | 中分類で追加 |
| `0x5/0x5` 旅バラエティ | `ENTERTAINMENT`, `TRAVEL` | 中分類で追加 |
| `0x5/0x6` 料理バラエティ | `ENTERTAINMENT`, `LIFE_STYLE` | 中分類で追加 |
| `0x6` 映画 | `MOVIES` | 大分類だけで設定可 |
| `0x7` アニメ・特撮 | `ENTERTAINMENT` | `FAMILY_KIDS` へは推測しない |
| `0x8/0x2` 自然・動物・環境 | `ANIMAL_WILDLIFE` | 中分類で設定 |
| `0x8/0x3` 宇宙・科学・医学 | `TECH_SCIENCE` | 中分類で設定 |
| `0x8/0x4` 文化・伝統文化 | `ARTS` | 中分類で設定 |
| `0x8/0x5` 文学・文芸 | `ARTS` | 中分類で設定 |
| `0x8/0x6` スポーツ | `SPORTS` | 中分類で設定 |
| `0x9` 劇場・公演 | `ARTS` | 大分類で設定可 |
| `0x9/0x1` ミュージカル | `ARTS`, `MUSIC` | 中分類で追加 |
| `0x9/0x3` 落語・演芸 | `ARTS`, `COMEDY` | 中分類で追加 |
| `0xA/0x1` 園芸・ペット・手芸 | `LIFE_STYLE` | 中分類で設定 |
| `0xA/0x6` コンピュータ・テレビゲーム | `GAMING` | 中分類で設定 |
| `0xA/0x7` 会話・語学 | `EDUCATION` | 中分類で設定 |
| `0xA/0x8` 幼児・小学生 | `EDUCATION`, `FAMILY_KIDS` | 中分類で設定 |
| `0xA/0x9` 中学生・高校生 | `EDUCATION` | 中分類で設定 |
| `0xA/0xA` 大学生・受験 | `EDUCATION` | 中分類で設定 |
| `0xA/0xB` 生涯教育・資格 | `EDUCATION` | 中分類で設定 |
| `0xA/0xC` 教育問題 | `EDUCATION` | 中分類で設定 |

次の分類は `COLUMN_CANONICAL_GENRE` へ設定しない。`COLUMN_BROADCAST_GENRE` と provider-data JSON に保持する。

- `0x2/0x0`、`0x2/0x6`、`0x2/0x7` のようにニュース、生活、娯楽、番組案内の境界が曖昧なもの。
- `0x8/0x0`、`0x8/0x1`、`0x8/0x7`、`0x8/0x8` のように documentary / travel / arts / news の境界が曖昧なもの。
- `0xA/0x0`、`0xA/0x2`、`0xA/0x3`、`0xA/0x4`、`0xA/0x5` のように Android canonical genre へ一意に落ちない趣味・娯楽項目。
- `0xB` 福祉、`0xC` / `0xD` reserved、`0xE` 拡張、`0xF` その他。
- `user_nibble` 由来の放送事業者定義分類。

## TvProvider 自然対応項目の追加固定

| ARIB / PMT 要素 | 投影先 | 投影成立条件 |
|---|---|---|
| EIT `free_CA_mode` | TvProvider scrambled 判定、provider-data JSON | `1` を scrambled、`0` を not scrambled とし、CAS 状態と混同しない。 |
| 音声 ISO639 language | TvProvider audio language メタデータ、provider-data JSON | PMT / descriptor から取得可能な言語だけ設定し、取得不能時に推測しない。 |
| 視聴年齢制限 | `COLUMN_CONTENT_RATING`、provider-data JSON、診断情報 | 既存レーティングドメイン と整合する値だけ設定し、reserved / malformed / domain不明は 診断情報に留める。 |
| event_group_descriptor | provider-data JSON `relatedItems` | 現行仕様では保存・診断のみ。予約追従へ接続する場合は安全条件を設計正本へ固定する。 |
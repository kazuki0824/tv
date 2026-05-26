# r50dx10

- r50dx10 では TIS 側テストの `TsPid` 型化追随のみを実施し、Rust provider-data 本体の追加変更は行っていない。
- r50dx の provider-data 失敗 result 化と旧 `canonicalGenres` 削除、および r50dx7 の `LivePlaybackSnapshot` 境界の完了条件は維持した。
- Android/Soong build、Rust単体テスト、atest、VTS、CTS、実機確認は未実施。

# r50dx9

- r50dx9 では TIS フェーズ5のみを実施し、Rust provider-data 本体の追加変更は行っていない。
- r50dx の provider-data 失敗 result 化と旧 `canonicalGenres` 削除、および r50dx7 の `LivePlaybackSnapshot` 境界の完了条件は維持した。
- Android/Soong build、Rust単体テスト、atest、VTS、CTS、実機確認は未実施。

# r50dx8

- r50dx8 では TIS フェーズ4の資源制御のみを実施し、Rust provider-data 本体の追加変更は行っていない。
- r50dx7 の `LivePlaybackSnapshot` 境界、および r50dx の provider-data 失敗 result 化と旧 `canonicalGenres` 削除の完了条件は維持した。
- Android/Soong build、Rust単体テスト、atest、VTS、CTS、実機確認は未実施。

# r50dx7

- r50dx6 のフェーズ1・2完了条件を維持し、TIS フェーズ3用に `LivePlaybackSnapshot` を追加した。
- Rust native transaction 由来の bulk snapshot から、ライブ視聴用の service / PMT / CAT / CA metadata / 診断情報を一括取得する Kotlin 境界を追加した。
- Android/Soong build、Rust単体テスト、atest、VTS、CTS、実機確認は未実施。静的確認のみ実施した。

# r50dx6

- r50dx6 では TIS 側フェーズ2未達の是正のみを実施し、Rust provider-data 本体の追加変更は行っていない。
- r50dx の provider-data 失敗 result 化と旧 `canonicalGenres` 削除の完了条件は維持した。
- Android/Soong build、Rust単体テスト、atest、VTS、CTS、実機確認は未実施。

# r50dx5

- r50dx5 では TIS 側フェーズ2未達の是正のみを実施し、Rust provider-data 本体の追加変更は行っていない。
- r50dx の provider-data 失敗 result 化と旧 `canonicalGenres` 削除の完了条件は維持した。
- Android/Soong build、Rust単体テスト、atest、VTS、CTS、実機確認は未実施。

# r50dx4

- r50dx4 では TIS 側フェーズ1・2未達の是正のみを実施し、Rust provider-data 本体の追加変更は行っていない。
- r50dx の provider-data 失敗 result 化と旧 `canonicalGenres` 削除の完了条件は維持した。
- Android/Soong build、Rust単体テスト、atest、VTS、CTS、実機確認は未実施。

# r50dx3

- r50dx3 では TIS 側フェーズ2未達の是正のみを実施し、Rust provider-data 本体の追加変更は行っていない。
- r50dx / r50dx2 の provider-data 失敗 result 化、旧 `canonicalGenres` 削除、型化済み Kotlin 境界の完了条件は維持した。
- Android/Soong build、Rust単体テスト、atest、VTS、CTS、実機確認は未実施。

# r50dx2

- r50dx2 では TIS 側フェーズ2未達の是正のみを実施し、Rust provider-data 本体の追加変更は行っていない。
- r50dx の provider-data 失敗 result 化と旧 `canonicalGenres` 削除の完了条件は維持した。
- Android/Soong build、Rust単体テスト、atest、VTS、CTS、実機確認は未実施。

# r50dx

- Program / Channel provider-data 生成失敗時の `{}` 成功扱いを廃止し、JNI result に `success`、`errorCode`、`errorMessage` を追加した。
- 保存用 Program provider-data schema / serde model / 期待値データから旧 `canonicalGenres` を削除した。
- 不正 provider-data request が失敗 result になり、空 JSON を返さないことを単体試験に追加した。
- Android/Soong build、Rust単体テスト、atest、VTS、CTS、実機確認は未実施。JSON 構文確認と静的差分確認のみ実施した。

# r50dc

- TIS 側テストの provider-data raw bytes 境界追随に合わせ、arib_si_engine_rs 側の設計・実装変更は行っていない。
- Android/Soong build、Rust単体テスト、Kotlin compile、instrumentationテスト、atest、VTS、CTS、実機確認は未実施。静的差分確認のみ実施した。

# r50db

- provider-data の既存データ入力境界について、`rawBytes` の意味、invalid UTF-8 / malformed JSON の扱い、署名対象を DESIGN_JA.md に補足固定した。
- Rust provider-data API の normalize / signature / key extraction / current-program diagnostics 入力を `&str` から `&[u8]` へ変更した。
- JNI の該当 provider-data API を `JString` ではなく `JByteArray` 受けに変更し、Kotlin 側の `ByteArray` 境界と一致させた。
- Android/Soong build、Rust単体テスト、Kotlin compile、instrumentationテスト、atest、VTS、CTS、実機確認は未実施。静的差分確認のみ実施した。

# r50da

- malformed CA_descriptor の詳細診断を CAS discovery snapshot の一次診断として出力するため、CA_descriptor parser が table / PID / service / ES 文脈、理由、raw prefix を保持するようにした。
- Program provider-data 用の `malformedCaDescriptorCount` summary が EIT descriptor 診断件数を誤用しないよう、CA_descriptor 診断の service summary から渡す境界を追加した。
- audio component の unsupported codec 診断にも `r51PlaybackSupported` / `liveViewableClaim` を出力し、video と同じ provider-data 診断形状に揃えた。
- Android/Soong build、Rust単体テスト、Kotlin compile、instrumentationテスト、atest、VTS、CTS、実機確認は未実施。静的差分確認のみ実施した。

# r50cz

- ProviderDataResult の JNI JSON 表現を `bytes` / `signature` / `schemaVersion` / `truncated` / `diagnosticsDroppedCount` へ変更し、安定キー抽出結果を result から分離した。
- ProgramProviderDataV1 の component schema / serde model に unsupported codec 診断 field を追加した。
- ProgramProviderDataV1 diagnostics に `malformedCaDescriptorCount` summary を追加した。

## r50cx
- r50cw 静的再確認で残っていた Program provider-data の nested unknown key 保持未達を修正した。
- 既存 JSON v1 の正規化時に top-level だけでなく、`programKey`、`timing`、`source`、`cas`、`ratings[]`、`genres[]`、`components.*[]`、`diagnostics.*` 等の nested object 由来 unknown key も `diagnostics.rawProviderDataExtensions[]` へ正規化するようにした。
- bulk transaction JSON に `snapshotGeneration`、`ingestSequence`、`parserDiagnostics` を追加し、TIS の用途別 transaction DTO と同一 native state で対応させた。
- Android/Soong build、Rust単体テスト、Kotlin compile、instrumentationテスト、atest、VTS、CTS、実機確認は未実施。静的差分確認のみ実施した。

## r50cw
- r51リリース前の設計・実装不一致のうち、Program provider-data の未知キー保持、provider-data 切り詰め診断、extended_event 空項目名の許容、README旧情報の整理を実装した。
- 既存 JSON v1 の top-level unknown key は正規化時に `diagnostics.rawProviderDataExtensions[]` へ移し、新規 canonical 出力で無言破棄しないようにした。
- hard limit 超過時は `PROVIDER_DATA_TRUNCATED` 診断、上限値、dropped count を保存するようにした。
- Android/Soong build、Rust単体テスト、Kotlin compile、instrumentationテスト、atest、VTS、CTS、実機確認は未実施。静的差分確認のみ実施した。

## r50cv
- r50cu 推奨案A固定後の未達1〜8に対し、実装側を設計へ合わせた。
- provider-data builder 入力を `maleicacid.tv.programRequest` / `maleicacid.tv.channelRequest` の Rust serde request 型へ分離し、保存用 schema と分けた。
- Rust provider-data API の手書き Value parser と default 合成経路を削除し、serde parse と明示検査へ寄せた。
- DescriptorDiagnosticV1 は TIS typed DTO を廃止し、Rust 生成 canonical JSON の不透明保持境界へ戻した。
- Program stable key は Kotlin 生成ではなく Rust JNI `nativeBuildProgramKey` へ統一した。
- Android/Soong build、Rust 単体テスト、Kotlin compile、instrumentationテスト、atest、VTS、CTS、実機確認は未実施。

## r50cu
- 推奨案Aに従い、provider-data の受け渡し境界を DESIGN_JA.md に固定した。
- TIS から JNI へ渡す JSON は保存形式ではなく Rust serde 型への受け渡し用形式であり、型、検査、正規化、署名、識別子抽出は Rust が所有することを明記した。
- 実装修正は行わず、固定後の設計と実装の不一致は別レポートで抽出した。

## r50ct
- r50cs 設計・実装不一致レポートのうち、境界整理対象の TIS provider-data input JSON 手組み問題を除き、デグレ1件と未達6件を設計に合わせて修正した。
- DescriptorDiagnosticV1 は TIS が JSON array を parse / rewrite せず、Rust 由来 canonical JSON を opaque string として Rust provider-data builder へ返す形にした。
- Program provider-data builder の必須 source / cas 検証、freeCaMode / series / audioLanguages / components の欠落値拒否を強化した。
- Android/Soong build、Rust 単体テスト、Kotlin compile、instrumentationテスト、atest、VTS、CTS、実機確認は未実施。

## r50cs
- r50cr 設計・実装不一致レポートの10件を、未達とデグレを区別した上で全件修正した。
- Rust bulk event DTO を nested `programKey` / `serviceKey` / `timing` 形へ寄せ、手書き巨大 JSON 境界を `serde_json::json!` 生成へ変更した。
- EIT component/audio_component descriptor だけで ES PID を捏造しないよう `esPid=null` とし、TIS 側で PMT service component と一致した場合だけ provider-data components へ統合するようにした。
- Channel provider-data build / extract は schema と required 欠落を拒否し、program extractedKey は JSON programKey 文字列へ変更した。
- Program provider-data の source/cas root-level fallback と videoFormat fallback を削除した。
- Android/Soong build、Rust単体テスト、Kotlin compile、instrumentationテスト、atest、VTS、CTS、実機確認は未実施。

## r50cr
- r50cq 設計・実装不一致レポートの残件1〜7に対応した。
- bulk event DTO の content genre / parental rating 境界を構造化名へ寄せ、旧 `broadcastGenre` 文字列表現と旧 `rating` / `rawRating` field を通常境界から外した。
- DescriptorDiagnosticV1 は Rust 生成 canonical JSON を TIS が透過保持する経路へ戻し、Kotlin 側 field-by-field 再構築によるデグレを解消した。
- provider-data API の programKey 欠落時ゼロ key 生成と、旧 descriptor diagnostic field alias 受け入れを拒否するようにした。
- Android/Soong build、Rust 単体テスト、Kotlin compile、instrumentationテスト、atest、VTS、CTS、実機確認は未実施。

## r50cq
- r50cp 設計・実装不一致レポートの残件1〜8に対応した。
- Program provider-data へ DescriptorDiagnosticV1 を保存する経路を復旧し、r50以前の旧 flat provider-data 入力を normalize / extract 系 API から拒否するようにした。
- EIT section 由来 source を bulk event DTO から TIS provider-data へ渡し、DescriptorDiagnosticV1 生成は provider_data.rs の serde struct を使う形へ寄せた。
- 旧 descriptor dump helper を test cfg に閉じ、通常 source の旧診断断片生成経路から外した。
- Android/Soong build、Rust 単体テスト、Kotlin compile、instrumentationテスト、atest、VTS、CTS、実機確認は未実施。

## r50cp
- r50co 設計・実装不一致レポートの残件1〜7に対応した。
- Channel provider-data の tune 形を設計どおり `deliverySystem` / `frequencyHz` / `streamId` / `streamIdType` へ統一し、旧 `system` / `streamSelector` 形を schema と実装から削除した。
- provider-data builder の旧 descriptor diagnostic container fallback と、Rust 側の旧 flat event helper / 孤立 `#[no_mangle]` 残骸を削除した。
- Android/Soong build、Rust 単体テスト、Kotlin compile、instrumentationテスト、atest、VTS、CTS、実機確認は未実施。

## r50co
- r50cn 後の設計固定に従い、r50 以前の provider-data 互換入力経路を廃止した。
- channel tune 復元 API は JSON v1 だけを受け、`;` 区切り key-value 形式を解析しないようにした。
- Program provider-data の入力 DTO も `programKey` object を使う形へ寄せ、旧 key 文字列入力を通常経路から外した。
- DescriptorScopeV1 の `name` は任意、`parseStatus` は必須であることを DESIGN_JA.md に固定した。
- Android/Soong build、Rust 単体テスト、Kotlin compile、instrumentationテスト、atest、VTS、CTS、実機確認は未実施。

## r50cn
- r50cm 設計・実装不一致レポートの残件1〜3に対応した。
- EIT component_descriptor / audio_component_descriptor 由来の構造を provider-data components へ渡し、service components と component_tag で統合するようにした。
- DescriptorDiagnosticV1 の Kotlin rawJson 再投入経路を削除し、型付き DTO から JSON を再構成する境界へ変更した。
- EIT section の version / sectionNumber を DescriptorDiagnosticV1 scope へ保持するようにした。
- Android/Soong build、Rust 単体テスト、Kotlin compile、instrumentationテスト、atest、VTS、CTS、実機確認は未実施。

## r50cm
- r50cl 設計・実装不一致レポートの残件1〜8に対応した。
- program provider-data の genres[] に TIS が直接設定した canonical genre を保持できるようにし、free_CA_mode の raw / parseStatus と DescriptorDiagnosticV1 の scope / parseStatus を設計どおり保持するようにした。
- channel provider-data の上限超過時も CAS 状態を保持するようにした。
- DescriptorDiagnosticV1 schema の parseStatus 必須化と provider-data fixture の canonical genre 反映を行った。
- Android/Soong build、Rust 単体テスト、atest、VTS、CTS、実機確認は未実施。

## r50cl
- r50ck の残件1〜5を静的再確認したうえで、残件6〜9に対応した。
- Rust bulk event JSON の `descriptorDiagnostics` を DescriptorDiagnosticV1 配列に直接し、旧診断コンテナを通常境界から外した。
- event diagnostic summary へ旧 descriptor JSON 全体を埋め込まないようにした。
- TIS 側 provider-data fixture と Rust 側 testdata を byte 単位で一致させた。
- Android/Soong build、Rust 単体テスト、atest、VTS、CTS、実機確認は未実施。

## r50ck
- r50cj 設計・実装不一致レポートの残件1〜5に対応し、ProgramProviderDataV1 の relatedItems / linkage / components / descriptorDiagnostics を型付き serde 構造へ寄せた。
- program provider-data の audio / video metadata に schema required の codec を必ず出すようにし、extendedItems の新規出力名を description / text へ統一した。
- hard-limit 時の program provider-data 切り詰めでも CAS 状態と ratings を保持するようにした。
- Android/Soong build、Rust 単体テスト、atest、VTS、CTS、実機確認は未実施。

## r50ci
- r50ch の優先順位 1〜4 を再確認し、serde / serde_json provider-data、schema required field、unknown key 値付き退避、TIS 側 nested DTO 境界は静的に維持されていることを前提に、優先順位 5〜7 を進めた。
- service bulk JSON に `components` を追加し、PMT 由来 ES メタデータを `video` / `audio` / `subtitle` / `data` の provider-data 構造へ渡せるようにした。
- event bulk JSON の `descriptors.components` に空構造を明示し、TIS 側で service bulk JSON 由来 components を上書きできる境界にした。
- event 以外も含めた旧 indexed helper と未使用 bool helper を削除し、通常境界を bulk snapshot と provider-data JNI API に限定した。
- Android/Soong build、Rust 単体テスト、atest、VTS、CTS、実機確認は未実施。

## r50cg
- provider-data 生成・正規化を `serde_json` ベースの ProgramProviderDataV1 / ChannelProviderDataV1 構造経由に変更し、既存 JSON 断片の raw 流用をやめた。
- Channel provider-data を `serviceKey` / `tune` / `cas` / `diagnostics` の nested JSON v1 形へ変更し、schema も同形へ更新した。
- bulk event JSON の通常境界から `freeCaText` / `seriesName` / `diagnosticDescriptorJson` 等の flat field を削除し、`descriptors` 配下の構造化 DTO に寄せた。
- event 以外も含めた旧 indexed JNI getter export を削除し、通常境界を `nativeSnapshotBulkJson()` と provider-data JNI API に限定した。
- Android/Soong build、Rust 単体テスト、atest、VTS、CTS、実機確認は未実施。

## r50cf
- provider-data 生成・正規化・現在番組診断追記を、既存 JSON をそのまま返す経路ではなく固定順の JSON v1 再出力経路へ寄せ、署名を再出力後バイト列から計算するようにした。
- Channel provider-data に `schema="maleicacid.tv.channel"` を追加し、`arib_si_engine_rs/schema/channel_provider_data_v1.schema.json` を追加した。
- 未対応視聴年齢制限は記述子診断情報ではなく `ratings[]` と `diagnostics.publishDiagnostics[]` に保持するようにした。
- Android/Soong build、Rust 単体テスト、atest、VTS、CTS、実機確認は未実施。

## r50ce
- provider-data / 診断情報の r51 設計固定として、unknown key の扱い、`diagnostics.currentProgram`、ChannelProviderDataV1、未対応視聴年齢制限の格納先を明記した。
- Android canonical genre の Rust 所有を禁止し、旧 `canonicalGenres` event field と `nativeGetEventCount()` / `nativeGetEvent*` indexed JNI getter 群を廃止境界として固定した。
- 実装追随として、bulk event JSON から Rust 由来の `canonicalGenres` を削除し、旧 indexed event JNI シンボルを削除した。
- Android/Soong build、Rust 単体テスト、atest、VTS、CTS、実機確認は未実施。

## r50cb
- WP-13対応として、unsupported codec provider-data テストデータを testdata に追加し、`maleicacid_arib_si_engine_rs_test` の data から参照できるようにした。
- schema、試験データ、provider-data v1 の r51確認対象は、tv直下の作業メモではなく `tis/INTEGRATION.md` の r51 ビルド・試験確認ゲートを正とする。
- Android/Soong build、Rust 単体テスト、atest、VTS、CTS、実機確認は未実施。

## r50ca
- WP-12対応として、HEVC-only サービスの 公開可否診断に `UNSUPPORTED_VIDEO_CODEC` を追加し、codecメタデータ認識と r51 平文ライブ視聴の対応宣言を分離した。
- HEVC-only サービスが `NO_SUPPORTED_VIDEO_ES` だけでなく `UNSUPPORTED_VIDEO_CODEC` を持つことを Rust test source で固定した。
- Android/Soong build、Rust 単体テスト、atest、VTS、CTS、実機確認は未実施。

## r50bx
- WP-09対応として、EIT更新区間JSONに `deletionAuthoritative` を出力し、TIS側の廃止削除判定へ Rust parser の authoritative 判定を伝搬できるようにした。
- EIT schedule other `0x60..0x6F` が r51 Programs publish/delete 対象の snapshot / 更新区間に入らないことを Rust test source で固定した。
- Android/Soong build、Rust 単体テスト、atest、VTS、CTS、実機確認は未実施。

## r50br
- WP-05 対応として、PMT由来ESの `dataComponentId`、字幕判定、スーパーインポーズ判定を JNI / JSON snapshot からTISへ渡せるようにした。
- `arib_si_engine_rs` 本体は字幕本文処理を所有せず、字幕本文は TIS 側の別 Rust JNI 境界で `libaribcaption` C API へ渡す分離を維持した。
- Android/Soong build、Rust 単体テスト、atest、VTS、CTS、実機確認は未実施。

## r50bq
- WP-04 対応として、未知 descriptor を通常フィールドに混ぜず `DescriptorDiagnosticV1` 形の診断へ出すようにした。
- `event_descriptors_to_json()` の descriptor 診断要素を `schema` / `schemaVersion` / `severity` / `code` / `scope` / `descriptor` / `message` を持つ v1 形へ変更した。
- `event_group_descriptor` の `group_type` 変換を、`0x1=shared`、`0x2/0x4=relay`、`0x3/0x5=movement` とする SSOT に合わせた。
- Android/Soong build、Rust 単体テスト、atest、VTS、CTS、実機確認は未実施。

## r50bp
- WP-03 対応として、ARIB content_descriptor から r51 明示写像に一致する canonical genre 配列を出力できるようにした。
- event_group_descriptor を `relatedItems`、linkage_descriptor を `linkage`、series_descriptor を series 構造と episode/count 用値として TIS へ渡せる JSON / JNI 経路を追加した。
- EIT `free_CA_mode` を TvProvider scrambled 投影用 boolean と provider-data `freeCaMode` へ渡せるようにした。
- Android/Soong build、Rust 単体テスト、atest、VTS、CTS、実機確認は未実施。

## r50bo
- WP-02 対応として、Program provider-data の新規生成を `ProgramProviderDataV1` の top-level schema へ切り替えた。
- 旧 `programKeyB64` / flat フィールド 形式は新規書き込みから外し、読み取り互換と正規化入力だけに限定した。
- `descriptorDiagnostics` は `ProgramProviderDataV1.diagnostics.descriptorDiagnostics[]` 配下の `DescriptorDiagnosticV1` 要素として生成する形へ変更した。
- provider-data が上限を超える場合も `ProgramProviderDataV1` の必須フィールドを維持した切り詰め JSON を生成するようにした。
- Android/Soong build、Rust 単体テスト、atest、VTS、CTS、実機確認は未実施。

## r50bn
- WP-01 対応として、`ProgramProviderDataV1` schema から重複していた `DescriptorDiagnosticV1` 定義を外し、`descriptor_diagnostic_v1.schema.json` への外部参照に統一した。
- `descriptor_diagnostic_v1.schema.json` の `$id` を `program_provider_data_v1.schema.json` からの相対参照と整合する URI に変更した。
- provider-data / 記述子診断情報の検証用JSONが schema 検証を通り、Rust 側と TIS 側の複製が バイト単位で同一であることを確認した。
- Android/Soong build、Rust 単体テスト、atest、VTS、CTS、実機確認は未実施。

## r50bm6
- r50bm5 の確認サマリ不足を受け、リリース物規則違反の人手観点を追加確認した。
- 本モジュールの実装ロジックは変更していない。Android/Soong build、Rust 単体テスト、atest、VTS、CTS、実機確認は未実施。

## r50bm5
- r50bm4 の確認漏れ是正として、リリース物規則違反の再スキャンを実施し、残っていた英語自然文コメントと診断文字列を日本語化した。
- 実装ロジックは変更していない。Android/Soong build、Rust 単体テスト、atest、VTS、CTS、実機確認は未実施。

## r50bm4
- r50bm3 の仕様固定内容を再確認し、イベントグループを LONG_DESCRIPTION へ出す旧記述が残っていた箇所を provider-data JSON `relatedItems` 保存方針へ統一した。
- 実装コードは変更していない。Android/Soong build、Rust 単体テスト、atest、VTS、CTS、実機確認は未実施。

## r50bm3
- 承認済みスコープ拡大の仕様固定として、canonical genre 投影の入力構造、series / イベントグループ / linkage / free_CA_mode / audio language / 視聴年齢制限の provider-data 契約を更新した。
- 実装コードは変更していない。Android/Soong build、Rust 単体テスト、atest、VTS、CTS、実機確認は未実施。

## r50bm2
- リリース物規則違反の追加是正として、provider-data 投影方針に残っていた英語自然文を日本語の現行仕様文へ置換した。
- 仕様 scope と実装 logic は変更していない。

## r50bm
- リリース物規則違反の是正として、CHANGELOG の重複見出し・途中表題・降順崩れを整理した。
- CHANGELOG 以外に残っていた旧版名、作業番号、修正経緯、英語自然文コメントを現行仕様の日本語表現へ置換した。
- 仕様 scope と実装 logic は変更せず、文書・コメント・履歴整理のみに限定した。

## r50bk12
- TIS 側の r51 設計契約未達修正のみで、arib_si_engine_rs の実装変更はない。
- Android/Soong build、Rust 単体テスト 実行、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bk11
- DescriptorDiagnosticV1 の canonical schema を計画どおり `actualRemainingLength` / `rawPrefixHex` を持つ形に更新し、互換用に `remainingLength` / `rawPrefix` も保持するようにした。
- JSON schema、期待値テストデータ、Rust 単体テストの期待値を新 schema に合わせた。
- Android/Soong build、Rust 単体テスト 実行、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bk10
- r50bk8 completion 版からの追加として、provider-data JSON v1 の golden / deterministic 署名 / 上限超過時の代替処理 の Rust 単体テスト を保持し、TIS 側完了条件の テスト可能な境界 と整合する形にした。
- Rust provider-data API の Android/Soong build、Rust 単体テスト 実行、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bk8-rerelease
- r50bk8 TIS / arib_si_engine_rs 追加修正計画の provider-data / EIT authoritative delete / malformed descriptor 境界に対応した。
- `provider_data.rs` を追加し、Channel/Program provider-data JSON v1 の生成、program/channel key 抽出、SHA-256 署名、current-program 診断情報 追記を native API として公開した。
- `NativeAribSiParser` JNI に provider-data build / 正規化 / 抽出 / 診断情報追記 用 entry point を追加した。
- EIT 更新区間に `deletion_authoritative` を追加し、malformed EIT event または 記述子診断情報を含む 更新区間 を 廃止行削除根拠にしない情報を TIS へ返すようにした。
- malformed descriptor loop を検出した section を parser 全体で即破棄せず、collector へ投入したうえで malformed 状態 を返すように変更した。
- Android/Soong build、Rust 単体テスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bj3
- r50bj2 後に残っていた設計未固定事項として、ARIB descriptor の length / loop / fragment sequence 不整合を正常フィールドに採用しないこと、extended_event fragment の欠番・重複・last_descriptor_number 不一致を 診断 扱いにすること、malformed EIT event を旧 event 削除根拠にしないことを DESIGN_JA.md に固定した。
- 実装コードは変更していない。Android/Soong build、Rust 単体テスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bj2
- r50bj の Rust provider-data / 診断情報 SSOT 方針と矛盾しないよう、TvProvider 投影方針文書側の旧未固定記述を整理した。
- 実装コードは変更していない。Android/Soong build、Rust 単体テスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bj
- 設計文書上で provider-data / 記述子診断情報の Rust serde SSOT、canonical JSON、署名、JNI boundary、JSON Schema / 期待値テストデータ 方針を固定した。
- 実装コードは変更していない。Android/Soong build、Rust 単体テスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bi6
- Phase B 完了証跡として、既存の SDT / NIT / BAT scope 実装が ONID+TSID / table-specific scope に閉じていることを静的確認した。
- TIS 側 JNI 本番経路から呼ばれる snapshot を bulk ラッパー 経由にし、count + index 型 getter を AribSiEngine public path から直接呼ばないようにした。
- Android/Soong build、Rust 単体テスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bi4
- parental_rating_descriptor の Rust 側出力が ARIB 構造化データと 診断JSON に留まり、Android `TvContentRating` domain / ISDB レーティング文字列 を持たないことを 挙動テスト で固定した。
- malformed length / truncated 視聴年齢制限descriptor が 診断として記録され、Android レーティング projection 文字列を Rust 側に混入させないことを test で補強した。
- Android/Soong build、Rust 単体テスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bi3
- サービス 公開可否診断に `pmt_pid_resolved` / `pmt_parsed` / `ca_state_resolved` / `free_ca_mode_resolved` を追加し、TIS が 現在診断情報の完備を理由文字列だけに依存せず判定できるようにした。
- Android/Soong build、Rust 単体テスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bh
- Replaced persistent/product-path `r51_live_claimable` naming with `clear_live_playback_supported`.
- `channel_registration_ready`、`epg_publishable`、`requires_cas`、`unsupported_cas` をサービス公開可否フィールドとして追加し、`registration_ready_snapshot()` が Rust 側の明示的な準備完了フラグに依存するようにした。
- 平文ライブ視聴対応は、transport / サービスの公開可否、登録準備完了、対応映像、平文またはCA情報なしの状態に依存するようにした。スクランブルサービスはチャンネル/EPG公開可能でも、平文ライブ視聴対応とは扱わない。
- Android/Soong build, Rust 単体テスト, atest, VTS, CTS, and real-device checks were not run in this environment.

## r50bg
- サービス単位の登録可能snapshot を通常 channel registration 用 snapshot として公開し、平文ライブ視聴の対応宣言可能 と scrambled unsupported registration を分離した。
- EIT section 更新後の event set が空になった場合も 更新区間 を保持し、TIS が obsolete Programs delete に使える JNI accessor を追加した。
- `arib_si_engine_rs/DESIGN_JA.md` を r51 の サービス単位の登録可能 方針と empty EIT 更新区間 方針に合わせて改訂した。
- Android/Soong build、Rust 単体テスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bf2
- r50bf のロジック未達を是正し、ARIB content_descriptor の 表示名 を `<majorName>/<middleName>` 形式に変更した。
- JNI の broadcast genre トークン は `ARIB(0xM/0xN):<majorName>/<middleName>` を返し、supplement text も同じ分類名を保持するようにした。
- Android/Soong build、Rust 単体テスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bf
- PMT の `PCR_PID=0x1fff` を PCR なしとして正規化し、r51 平文ライブ視聴の対応宣言可能 判定で `NO_PCR_PID` reason を出すようにした。
- PMT parse / descriptor-loop malformed 判定を PAT で確定した PMT PID に限定し、`table_id=0x02` 単独では PMT と見なさないようにした。
- `SectionAssembler` を テスト専用 に閉じ、本番経路の `arib_si_engine_rs` は assembled section payload の 意味解析 だけを担当する境界へ戻した。
- ARIB content_descriptor 由来の broadcast genre トークン を `ARIB(0xM/0xN):<表示名>` 形式で JNI から返す accessor を追加した。
- Android/Soong build、Rust 単体テスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50be
- CHANGELOG の見出しを `# CHANGELOG` と `## r50be` 形式に統一した。
- arib_si_engine_rs の実装ロジックは r50bd から変更していない。

## r50bd
- r51向け Direct Boot 境界、TvProvider Programs 更新、サービス単位 CAS、AudioTrack write 診断、PTS代替同期 診断、extended event JSON 解析、TIS product integration を更新。

## r50bc4
- r50bc3 完了判定で指摘された証跡不一致を踏まえ、EIT same-version 差分削除の説明コメントを日本語化し、r51 平文ライブ視聴の対応宣言可否の静的証跡対象を整理した。
- Android/Soong build、Rust 単体テスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bc3
- r51の 平文ライブ視聴の対応宣言可否 を transport 単位の公開可否から分離し、 サービスが PMT/PCR, r51対応映像, `free_ca_mode=false`, and no CA_descriptor が r51 視聴可能スナップショット even NIT など transport 単位の検出が未完了でも.
- transport単位のNIT完了条件が未達でも、サービス単位のr51対応宣言可否を確認する Rust 回帰テストを追加した。
- Android/Soong build, Rust unit tests, atest, VTS, CTS, and real-device checks were not run in this environment.

## r50bb7
- TIS が ARIB の視聴年齢制限情報を Android `TvContentRating` へ投影できるよう、構造化した `parental_rating_descriptor` 要素の JNI アクセサを追加した。

## r50bb4
- 検出用 PMT PID を セクションフィルター制御, r51視聴可能サービススナップショットのフィルタリングから独立して取得できるようにした。.
- スクランブルサービスを平文視聴可能チャンネルとして公開せずに、PMT/CAT の CA情報を診断情報と ECM/EMM フィルター設定に使えるよう、JNI/Kotlin に検出用 CAS サービス情報と CA情報アクセサを追加した。
- PAT由来 PMT PID が r51視聴可能スナップショット公開前にセクションフィルターで利用できることを確認する Rust 回帰テストを追加した。
- Android/Soong build, Rust unit tests, atest, VTS, CTS, and real-device checks were not run in this environment.

## r50bb
- 提供された Soong パッチに従い、`libmaleicacid_arib_si_engine_jni` を `product_available: true` から `product_specific: true` へ変更した。
- Android/Soong build, Rust unit tests, atest, VTS, CTS, and real-device checks were not run in this environment.

## r50ba5
- 平文 MPEG-2/AVC 映像サービスの r51対応宣言可否、および audio-only、data-only、HEVC-only、SDT scrambled、PMT program-CA、video ES-CA サービスの除外を確認する r51 テストを追加した。
- short_event、content、component、event_group、linkage、未知 descriptor の保持、診断JSONを確認する descriptor parser テストを追加した。
- 不正な時間範囲、未定義MJD、descriptor loop overflow 診断情報を確認する EIT テストを追加した。
- version 置換と同一version複数section統合を確認する CAT テストを追加した。
- ARIB 文字列診断 entry フィールドを確認するテストを追加した。

## r50ba4
- Rust test module が Soong `libtest` の product image variant を要求しないよう、`maleicacid_arib_si_engine_rs_test` から `product_available: true` を削除した。
- Rust JNI と Kotlin モデルに、`viewable`、`r51LiveClaimable`、r51除外理由を含む r51公開可否診断フィールドを追加した。
- ARIB文字列の損失許容復号診断情報を、集計カウンターだけではなく offset / code-set / reason / replacement の要素として持つ形式に変更し、要約カウンターも維持した。
- 不正な EIT event descriptor-loop overflow は、parse loop を黙って中断するのではなく event 診断情報として保持するようにした。

## r50ba3
- r51 ARIB SI / TvProvider 投影計画の実装を作り直した。

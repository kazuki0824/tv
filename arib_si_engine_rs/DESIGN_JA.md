# arib_si_engine_rs 設計判断

## 責務

`arib_si_engine_rs` は、Tuner HAL → framework/JNI/Tuner SDK API → TIS → arib_si_engine_rs という経路で渡された PSI/SI section payload と TIS 側 メタデータを入力として、PSI/SI/EIT descriptor の構文・意味解析を Rust で実装する。PMT/CAT の CA_descriptor から得られる CA_system_id、ECM PID、EMM PID と、SDT 等から得られる free_CA_mode / scrambling flag、サービス識別子補助情報を含むCA情報 / サービスメタデータ意味モデルも本crateの責務とする。raw TS packet demux、PID filter、section assembly、section payload delivery は Tuner HAL の責務であり、本crateに重複実装しない。Tuner HAL を CA情報 / サービスメタデータ意味モデルの生成者またはSSOTにしない。

本crateの公開意味境界は、放送信号とARIB/MPEG規格から導出できる事実と構文・意味診断に限定する。Android `TvContract` / `TvTrackInfo` / TIFの状態、製品releaseの対応codec、実decoder availability、CAS実装可否、channel登録可否、EPG公開可否、live playback可否はTISのpolicyであり、本crateで算出・キャッシュ・fallback・永続化しない。provider-data builderはARIB意味データのcanonical保存形式を所有するが、TIS/product policyの判定器にはしない。


## ARIB 文字列 decoder の適用範囲

自前の ARIB 文字列 decoder は、サービス名、番組名、短形式イベント、長形式イベント、各種 descriptor のテキストなど、字幕以外の SI/EPG 文字列に限定して使う。本crateはPMT Data Component DescriptorをPSI/SI descriptorとして構文・意味解析し、`data_component_id`と`additional_arib_caption_info`のDMF/Timing等の放送factをTISへ渡す。一方、字幕/文字スーパーPES、caption management/statement data、字幕本文、外字・DRCSのdecode/renderは本crateへ持ち込まない。caption managementのTIF track discovery / TMD / STMに必要な有限構造fieldは`tis/DESIGN_JA.md`を正としてTIS caption pathのRust JNI boundaryが扱い、全文字decode/renderは`libaribcaption`の責務とする。`arib_si_engine_rs` の自前 decoder に字幕用 ARIB B24 decoderとしての完全性を対応宣言しない。

未対応の SI/EPG 文字・escape は `panic` させず、置換文字または診断によって安定動作させる。字幕 payload を `decode_arib_string_lossy()` に渡す経路は禁止する。字幕本文処理は TIS 側の libaribcaption 経路だけで行う。
`arib_si_engine_rs` は libaribcaption ラッパーを所有しない。libaribcaption は TIS 側の字幕 path から Rust JNI boundary と安全なRustラッパー経由で呼ぶ。

strict APIとlossy APIはdesignation / invocation、graphic set、APR/SP/MSZ/NSZ、CSI/XCSを処理するdecoder coreを一つだけ共有する。差分は`ErrorPolicy::Strict`が最初の異常を返すか、`ErrorPolicy::Replace`が`U+FFFD`とoffset・理由付き診断へ変換するかだけとする。正常入力で両APIの文字出力が一致しなければならない。意味論を二本のdecoderへ複製しない。

ARIB適合性の規範対象と検証証拠の分離は `../開発規則.md` を正とする。本decoderがSI/EPG文字列として受理する符号profileは、対象放送方式に適用される現行日本語TR-B14 / TR-B15のSI運用規定を正とし、STD-B24が定義する汎用的な文字符号機能全体をSI/EPG入力能力へ自動的に昇格させない。STD-B10 / STD-B24は、当該SI運用profileから参照される構文、designation / invocation、文字集合、制御機能の意味を解釈する基礎規格として用いる。取得可能なARIB公式英語版を条項単位の検証証拠に用いる場合も、現行日本語原文との版差を未証明差分として残し、改定概要、版一覧、二次資料を未取得本文の具体規定の代用にしない。

従来8単位符号のSI/EPG文字列は、TR-B14 / TR-B15のSI運用profileで定義された初期状態と使用文字集合を適用する。初期状態は G0=Kanji、G1=Alphanumeric、G2=Hiragana、G3=Katakana、GL=LS0(G0)、GR=LS2R(G2)、文字サイズ=NSZ とする。SI運用profileが使用しないMacro code set、DRCS code set、外字字形転送を正常なSI入力として要求しない。これらがSI/EPG入力に現れた場合、strict APIは規格外または未対応入力としてエラーにし、lossy APIだけが`U+FFFD`とoffset・理由付き診断へ変換する。STD-B24の汎用Macro展開器、DRCS renderer、字幕組版機能を本crateのSI decoder capabilityとして設計しない。

SI運用profileで使用するAPR、SP、MSZ、NSZは、未対応制御ではなく正常なSI/EPG入力として受理する。APRはoperation positionで改行を発生させ、SPは空白を出力する。MSZはmiddle size（半角）へ、NSZはstandard size（全角）へ文字サイズ状態を切り替える。MSZを指定できる対象はSI運用profileの制約に従い、alphanumeric characterとspaceに限定し、対象外の文字へMSZを適用する入力はstrict APIでエラー、lossy APIで置換とoffset・理由付き診断にする。LS0 / LS1 / SS2 / SS3 / ESCは後記designation / invocation契約、XCS(CSI)は後記CSI / XCS契約に従う。

UCSについても、STD-B24に符号方式が存在することだけを根拠にSI/EPG入力として受理しない。対象放送方式のSI運用profileが対象fieldについてUCSのsignalingとcoding formを明示的に許可する場合に限り、そのprofileが指定する境界で受理する。適用profile上でUCSを使用しないfieldでは、BOMやbyte patternからUCSを推測して復号しない。したがって、SI/EPG decoderの通常契約に「BOMなしならUTF-8」「`FE FF`ならUTF-16BE」のようなSTD-B24汎用UCS判定を置かず、profileで許可されたUCS入力専用の入口を設ける場合にだけSTD-B24のcoding-form規定を適用する。

XCS の実装方針は、実装上の先例として `xtne6f/EDCB` の `work-plus-s` commit `9770536e9f04835fab2bddee26af1f17c7c40a9c` にある `EpgDataCap3/EpgDataCap3/ARIB8CharDecode.cpp` に倣う。EDCBはCSI sequenceを構文として最後までconsumeし、XCSを認識したうえでXCS固有の意味処理を行わない。本crateも同じ境界を採用し、構文的に正しいXCSはCSI sequence全体をconsumeしてdecoderの文字出力・designation / invocation stateを変更しないno-opとして扱う。XCS固有の意味処理を実装しないことだけを理由にstrict APIを失敗させず、lossy APIで`U+FFFD`も出力しない。構文不正または切り詰められたCSI / XCSはこの例外に含めず、strict APIではエラー、lossy APIでは置換とoffset・理由付き診断にする。EDCBはARIB適合性の規範資料ではなく、このno-op境界の実装先例としてのみ参照し、XCSの構文・適用可否そのものは前記TR-B14 / TR-B15およびSTD-B24の規範判断に従う。

本decoderの適合主張は、字幕ではないSI/EPG文字列について、次の境界に限定する。

| 項目 | 対応境界 |
|---|---|
| 初期状態 | 対象放送方式のSI運用profileに従い、従来8単位符号では G0=Kanji、G1=Alphanumeric、G2=Hiragana、G3=Katakana、GL=LS0(G0)、GR=LS2R(G2)、NSZ |
| 文字集合 | SI/EPG運用profileで使用するKanji、Alphanumeric、Hiragana、Katakana、追加記号だけを正常な文字入力として扱う |
| designation / invocation | SI運用profileで使用可能な文字集合を選択するために必要なESC designation、LS/SS invocationだけを受理し、汎用STD-B24 capabilityを理由に使用禁止集合へ遷移しない |
| SI control (APR / SP / MSZ / NSZ) | APR、SP、MSZ、NSZを正常なSI controlとして受理する。APRは改行、SPは空白、MSZはmiddle size（半角）、NSZはstandard size（全角）として扱う。MSZの適用対象はSI運用profileで許可されたalphanumeric characterとspaceに限定する |
| Macro | SI運用profileで使用しない。Macro code setを正常なSI/EPG入力能力として宣言せず、出現時はstrictでエラー、lossyで置換と診断にする |
| DRCS・外字 | SI運用profileで使用しないDRCS code set / 外字字形転送を正常なSI/EPG入力能力として宣言しない。字幕・DRCS表示は`libaribcaption`側の責務とする |
| UCS | 対象放送方式のSI運用profileが対象fieldについて明示的に許可・signalingする場合だけ、そのprofileで指定された入口から受理する。STD-B24にUCSが存在することやBOM/byte patternだけを根拠に従来8単位符号fieldをUCSとして推測しない |
| CSI / XCS | CSIはsequence終端まで構文として完全にconsumeする。構文的に正しいXCSはEDCB方式に倣い、XCS固有の意味処理を行わないno-opとしてconsumeし、文字出力・designation / invocation stateを変更せず、XCS自体を未対応escapeとしてstrict errorまたは`U+FFFD`へ変換しない。構文不正・切詰めCSI / XCSは通常のstrict/lossy異常境界に従う |
| 不明・切詰めescape | `U+FFFD`へ置換し、offset、入力prefix、理由を診断へ記録する。`panic`、無言の脱落、推測による状態遷移を禁止する |
| lossy境界 | 置換を許すAPIは`decode_arib_string_lossy()`だけとし、置換数と理由を返す。strict APIは未対応または不正な符号列をエラーにする |

この表にない文字集合、制御機能、字幕、BML、組版、DRCS字形レンダリングを、STD-B24に定義があるという理由だけでSI/EPG対応能力に含めない。対応文字集合、制御機能、または別coding systemを追加する場合は、適用するTR-B14 / TR-B15のSI運用条項、参照するSTD-B10 / STD-B24の版・分冊・条項、入力状態、出力、置換規則、試験ベクトルを先に更新する。


## EIT 範囲

本crateは、TISから渡されたEIT sectionについてEIT/descriptorの構文・意味解析を担当する。どのEIT tableをいつ収集し、TvProviderへどの期間・用途で利用するかという製品releaseの収集scopeは`../開発規則.md`を正とし、本書では再定義しない。TIS runtimeのfilter起動・停止は`../tis/DESIGN_JA.md`を正とする。

### 複数table instanceの完成・更新・寿命

`repeat=true`で継続配送されたsectionについて、本crateは`table_id_extension`、actual version、`current_next_indicator`、`section_number`、`last_section_number`に基づいてtable instanceを区別し、instance別の完成・更新・寿命を管理する。

本crateは、製品または個別操作が必要とするinstance集合そのものを決定せず、instance別の完成・更新・寿命状態をTISへ返す。どの集合の完成でfilterを停止するかはTISのruntime責務とする。

## descriptor 変換

表示・保存対象として扱う EIT descriptor は現行仕様で構造化変換する。TvProvider 標準列への投影は tv 直下の `ARIB_SI_EPG_TvProvider投影方針.md` を正とし、`internal_provider_data` の具体 schema / canonical encode は本 crate の Rust provider-data serde構造体を SSOT とする。異なる言語の `short_event_descriptor` は `shortEvents[]`、言語ごとに再構成した `extended_event_descriptor` 本文は `extendedTexts[]`、長形式itemは `extendedItems[]` としてlanguage codeを失わず保持する。標準列へ選択済みのtitle/description文字列だけを別fieldへ重複保存しない。同文書で標準列投影が固定されている component、音声コンポーネント、コンテンツジャンル、free_CA_mode、視聴年齢制限、series id、episode number、音声言語は provider 用フィールドとして出せる。last episode number は通常の `TvContract.Programs` 標準列へ投影する候補ではなく、series の完全構造、イベントグループ、linkageの型付きidentityとbounded private-data prefix、unknown、診断JSON などと同様に JSON v1 `internal_provider_data` に構造化保存する。Android canonical genre の写像結果、Android rating文字列、runtime選択track、decoder/CAS capability結果はprovider-dataへ保存しない。

`components.audio[]` は `audio_component_descriptor` の独立意味fieldをAndroid runtime metadataへ潰さず保持する。取得できた `stream_content / component_type / component_tag / stream_type / simulcast_group_tag / ES_multi_lingual_flag / main_component_flag / quality_indicator / sampling_rate / ISO_639_language_code(_2) / text_char` と、明示的に導出できる channel configuration / sampling表示をprovider-dataの型付きfieldへ保存する。`components.video[]` も `component_descriptor` の `stream_content / component_type / component_tag / ISO_639_language_code / text_char` を保持する。runtime `TvTrackInfo`投影結果は保存しない。

PMT Data Component Descriptorで`data_component_id=0x0008`を受信した場合、`additional_arib_caption_info`を固定byte値として比較せず、`DMF:4bit / reserved:2bit / Timing:2bit`へ構造化する。raw DMF、raw Timing、DMFから一意に導出できる受信時automatic-presentation factを放送由来意味factとして保持する。現行日本向けprofileでのservice kindは`Timing=01`をcaption、`Timing=00/10`をsuperimposeとしてTISが利用できるtyped factへ正規化し、`Timing=11`はreserved診断として既知kindへ丸めない。DMFをcaption/superimpose分類へ転用せず、`data_component_id=0x0008`だけでもkindを決定しない。`data_component_id=0x0012`等の別profileはその適用規定に従い、0x0008のTiming規則を無条件転用しない。

`arib_si_engine_rs` は Android canonical genre の写像表をSSOTとして所有しない。

本crateはprovider-data schema、canonical encode、保存上限、parser/descriptor診断schemaの正本を所有する。TvProvider標準列への投影判断は `ARIB_SI_EPG_TvProvider投影方針.md`、TIS runtimeでの書き込み契機、retry、現在番組解決、視聴セッション利用は `tis/DESIGN_JA.md` を正とする。

content_descriptor 由来のARIB分類、表示文字列、user_nibble を構造化して出力し、TIS が `ARIB_SI_EPG_TvProvider投影方針.md` の明示写像表に基づいて `Programs.COLUMN_CANONICAL_GENRE` へ入れる値を決定する。そのAndroid投影結果を本crateの意味モデルまたはprovider-dataへ戻さない。

## parental_rating_descriptor の構造化契約

`arib_si_engine_rs` は `parental_rating_descriptor` を診断文字列だけに落とさず、TIS が `TvContentRating` へ変換できる構造化データとして出力する。

出力する最小フィールドは次とする。

```text
parental_rating_descriptor:
  entries[]:
    country_code
    raw_rating_byte     # ARIB STD-B10 5.13-E1 Part 2 6.2.12のRating 8 uimsbfを8bit値のまま保持する
  raw_descriptor_bytes
  parse_status          # ok / malformed_length / truncated_descriptor / unsupported_value
```

`arib_si_engine_rs` は Android `TvContentRating` の domain 名、flattened string、対応可否をSSOTとして決めない。Android TvProvider列への投影と `TvContentRating` 生成は TIS 側の責務とし、投影方針は tv 直下の `ARIB_SI_EPG_TvProvider投影方針.md`をSSOTとする。

未対応 country_code、未定義 raw rating byte、不正 descriptor は破棄せず、`parse_status` と診断JSONに保持する。未対応値を推測で一般ユーザー向けレーティングに変換してはならない。

## 放送profile別 discovery

discovery profileはTISの選局候補から`ISDB_T / BS / CS110`を明示して設定し、ONIDや受信済みtableから推測しない。PAT、PMT、SDT actual、NIT actualはprofileにかかわらず固定必須条件とする。変化するprofile値はoptional tableの必須化だけであり、ISDB_TはSDT other/NIT otherを必須化せず、BSはSDT otherを必須・NIT otherを任意、CS110はSDT other/NIT otherを必須とする。BATは受信した場合に解析・意味利用するが、未受信だけをdiscovery incompleteの理由にしない。

完成状態の正本は`TableRequirementStatus(component, scope, required, complete)`の集合とする。global missing、complete、discovery stageはこの集合から導出し、同じ意味のbooleanやmissing listを並行して保持しない。complete判定はtable_idだけのglobal完了ではなく、table_extensionとNIT/BAT transport loopから得たONID/TSID scopeを使ってtransport/service単位で判定する。リモコンキーが得られない場合はservice_idを表示番号の代替値とする。

`arib_si_engine_rs` は、service / transport単位の `ServiceSemanticFacts` として、ONID / TSID / SID、ARIB `service_type`のraw 8-bit値、PMT/PCRの存在・構文状態、audio/video/subtitle/data ES一覧とcodec signaling、caption/superimpose用Data Component Descriptorの`data_component_id / DMF / Timing / automatic-presentation`等の放送fact、CA descriptor / free_CA_mode、CA descriptor等から導出した`requiresCas`、SMD意味状態、欠落・不正理由を構造化してTISへ渡す。`ServiceSemanticFacts` は放送信号から導ける事実と構文・意味解析結果だけを持ち、Android channel登録可否、EPG公開可否、現行productのdecoder/CAS対応可否、字幕言語のplayable capability、ライブ再生可否を算出しない。caption management data由来のlanguage set/TMD/STMはPES runtime factなので本snapshotへ混ぜない。Android channelを登録するか、partial snapshotをchannel insertへ使用するかはTISの責務であり、`../tis/DESIGN_JA.md`を正とする。`Channels.COLUMN_SERVICE_TYPE`への最終投影は`../ARIB_SI_EPG_TvProvider投影方針.md`を正とし、本crateはAndroid generic `TvContract.Channels.SERVICE_TYPE_*`への意味変換を行わない。

## system_management_descriptor と通常受信判定

`system_management_descriptor`（SMD、`descriptor_tag=0xFE`）はNITのnetwork loopに属するネットワーク単位の意味情報として`arib_si_engine_rs`が解析する。Tuner HALはSMDを解釈せず、他のsectionと同じ汎用section配送だけを行う。SMDの構文・意味はARIB STD-B10 5.13-E1 Part 2 §6.2.21、通常受信対象の判定はARIB STD-B21 5.12-E2 Chapter 13 §13.2を根拠とする。

SMDの意味モデルは`system_management_id`の16 bit原値、上位2 bitの`broadcasting_flag`、次の6 bitの`broadcasting_identifier`、下位8 bitの`additional_broadcasting_identification`、後続の`additional_identification_info`、構文検査結果を保持する。未知値を既知方式へ丸めずraw値と診断を残す。ただし現行productの通常受信可否を下位8 bitまたは`additional_identification_info`で制限しない。

正常なSMDについては、`broadcasting_flag=0b00`かつ現行productが認識する`broadcasting_identifier`である場合を`SUPPORTED_BROADCAST`、`01`または`10`を`NON_BROADCAST`、`11`を`UNDEFINED_BROADCAST_CLASS`、`00`で未知のidentifierを`UNSUPPORTED_BROADCAST_SYSTEM`としてARIB意味状態に正規化する。現行productが認識する`broadcasting_identifier`はBSデジタル=`0b000010`、地上デジタルテレビ=`0b000011`、広帯域CSデジタル=`0b000100`とする。本crateはONIDから放送方式または期待identifierを推定せず、現在の選局候補との一致判定も所有しない。選局候補との適合はTISがraw `broadcasting_identifier`と`ScanCandidate.kind`から判定する。SMD欠落または構文不正は再取得可能な意味状態`UNDETERMINED_SMD`として診断し、永久的な`UNSUPPORTED`には確定しない。

SMDの判定対象は既存のtable-instance完成・version・寿命規則で有効とされたNITとし、SMD専用の`PENDING`状態や別のversion切替状態機械を設けない。

本crateは上記SMD意味状態と、その根拠となるraw値・構文診断だけを`ServiceSemanticFacts`へ出力する。SMD意味状態をAndroid channel登録、EPG公開、ライブ再生可否のbooleanへ変換せず、選局候補のdelivery systemも意味状態へ混入させない。`UNDETERMINED_SMD`は再取得によって正常なSMDを得た時点で意味状態を再評価し、SMD適合を肯定する根拠には使わない。raw `broadcasting_identifier`と現在の選局候補との適合を含め、SMD事実を他のPMT/PCR/service type/codec/CAS事実と組み合わせて製品policyを決める責務はTISが所有する。

## EIT 時刻状態と event identity

EIT event の `start_time` と `duration` は、ARIB が各フィールドのall-1を未定義値として規定していることと、本製品の誤相関・誤削除防止ポリシーを分離して次の状態に正規化する。ARIB本文から、両フィールドが同時にall-1の場合に`event_id`自体の識別子としての意味が失われることまでは導出しない。

- `DEFINED`: `start_time` と `duration` がともに具体的で構文的に有効。`original_network_id / transport_stream_id / service_id / event_id` を stable identity として扱う。
- `UNDEFINED_TIME`: `start_time=0xFFFFFFFFFF` または `duration=0xFFFFFF` の片方だけが all-1。raw `event_id` はARIB fieldとして保持する。本製品では同じ4要素を継続用stable identityとして使用してよいが、具体時刻が揃うまで `TvProvider.Programs` row へ投影しない。
- `BOTH_TIMING_UNDEFINED`: `start_time=0xFFFFFFFFFF` かつ `duration=0xFFFFFF`。raw `event_id` はARIB fieldとして診断・raw意味objectに保持する。ARIBが`event_id`を無意味と規定したものとは扱わず、本製品の保守的ポリシーとして、具体時刻を持つeventとの誤相関または既存Programの誤削除を避けるため、persistent stable key、`ProgramKeyV1`、deletion-authoritativeなvalid-event-set、後続具体eventとの自動相関へ昇格させない。
- `MALFORMED_TIMING`: 上記未定義値ではなく、BCDその他の構文規則に違反する。正常eventへ昇格せず診断に保持する。

## section 更新

MPEG-2 PSI / ARIB SIのlong-form section headerにある`section_length`は12 bit固定として、parser内部の単一`parse_section_header(section)`で`0x0fff` maskを適用する。bit幅を呼び出し側引数にせず、0や別幅をlegacy互換として受理しない。宣言長、buffer境界、CRCの検査は同じheader結果を使う。

PAT/PMT/SDT/NIT/BAT/EIT の version 更新では collector 全体を捨てない。table 単位、section 単位、サービス 単位で差分更新する。

EIT は section version 更新で消えた event を削除候補として扱う。ただし TvProvider / TIS 側へ stable identity として `original_network_id / transport_stream_id / service_id / event_id` を提供できるのは `DEFINED` または `UNDEFINED_TIME` の event に限る。`BOTH_TIMING_UNDEFINED` は本製品の保守的ポリシーとしてvalid event identity setに含めず、既存Programの削除根拠にも後続具体eventとの自動相関根拠にも使わない。section 更新後の stable event set が空になった場合も no-op として破棄せず、サービスキー、更新区間、空の valid event identity set を JNI/TIS へ返す。TIS は、Rust parser が `deletionAuthoritative=true` と判定した snapshot だけを obsolete Programs delete に使う。

EIT event fixed フィールド、start_time BCD、duration BCD、descriptor_loop_length が不正な event を含む section は、既存 event 削除用の authoritative valid-event-set として扱わない。不正 event は Programs から消すのではなく、既存正常 event を保持したまま診断情報に記録する。

開始時刻、終了時刻、duration、番組名、説明文の変更は、同一 stable identity の event 更新として扱う。開始時刻は stable identity に含めない。

ただし TvProvider の時間範囲制約、row 更新制約、または TIS 実装都合により provider row の再作成が必要な場合は、既存 provider row を削除して再 insert してよい。その場合でも、内部 stable identity は `original_network_id / transport_stream_id / service_id / event_id` のまま維持する。

## 診断 API

TvProvider に自然に入らない descriptor は構造化した内部データとして `internal_provider_data` に保存し、診断 API にも出す。EIT event ごとの診断文字列には、content、component、音声コンポーネント、視聴年齢制限、series、イベントグループ、linkage、未知 descriptor の数と主要値を含める。

provider-data JSON v1 は `provider-data / 診断情報 Rust SSOT` 節の `ProgramProviderDataV1` を唯一の正式 schema とする。少なくとも `series`、`eventGroups`、`linkage`、`freeCaMode`、`ratings`、`genres`、`extendedItems`、`components`、`diagnostics` を最上位フィールドとして保持する。番組identityは`programKey`だけ、時刻は`timing.startUtcMillis + durationMillis`だけを正本とし、重複する`serviceKey`、`endUtcMillis`、`audioLanguages`はcanonical保存しない。音声言語は`components.audio[].language / secondLanguage`に保持する。`eventGroups` は `event_group_descriptor` をdescriptor単位で保持する。各要素はraw `groupType`、共通の`events[] { serviceId, eventId }`、`groupType=0x4/0x5`でだけ存在する`otherNetworkEvents[] { originalNetworkId, transportStreamId, serviceId, eventId }`、それ以外のgroup typeで残余byteを保持する`privateDataHex`、`parseStatus`を持つ。`kind`のように`groupType`から導出できる値はcanonical保存しない。`series` は series_id、repeat_label、program_pattern、expire_date、episode_number、last_episode_number、series_name を保持する。


## 構造化変換対象 descriptor

short_event、extended_event、content、component、audio_component、parental_rating、series、event_group、linkage を現行仕様で構造化変換する。未知 descriptor は破棄せず診断に保持する。

ARIB descriptor は `descriptor_length`、descriptor 内部 length、loop 単位、fragment sequence が妥当な場合だけ正常フィールドとして採用する。length 不整合、余剰 byte、fragment 欠落、`descriptor_number` 重複、`last_descriptor_number` 不一致、必須フィールド不足は不正 descriptor とし、番組名、short text、長形式イベント本文、コンテンツジャンル、component、音声コンポーネント、series、event_group、linkage の正常フィールドには採用しない。`extended_event_descriptor` の `descriptor_number` 重複、欠落、`last_descriptor_number` 一致は同一eventかつ同一`ISO_639_language_code`のdescriptor set内で判定し、異なるlanguage set間の同一番号を重複とみなさない。不正 descriptor は parser を停止させず、`DescriptorDiagnosticV1` に tag、offset、declaredLength、actualRemainingLength、parseStatus、rawPrefixHex、section scope を保持する。

## API 境界の固定

Kotlin/JNI の通常サービス境界は、channel registrationやplayback policyを確定済みのsnapshotではなく、service / transport単位の `ServiceSemanticFacts` bulk snapshotとする。snapshotはONID / TSID / SID、ARIB `service_type`、PMT/PCRの存在・構文状態、ES/component一覧とcodec signaling、CA descriptor / free_CA_mode、CA descriptor等から導出した`requiresCas`、SMD意味状態、欠落・不正理由を返す。`registration_ready_snapshot()`、`clear_live_playback_supported_snapshot()`、`publishability_by_service`のようにAndroid/TIS/product policyをRust側で確定する公開境界は設計しない。TISは`ServiceSemanticFacts`から`requiresCas`を受け取り、current product capabilityと組み合わせて`channelRegistrationReady`、`epgPublishable`、`clearLivePlaybackSupported`、`unsupportedCas`を算出し、その判断をchannel登録、Programs公開、視聴セッションへ一貫して使用する。

PAT は ONID を持たないため、`(transport_stream_id, service_id) -> pmt_pid` をそのまま公開可能サービス識別子として扱わない。SDT/NIT/BAT 等で ONID が一意に解決できた場合だけ `(original_network_id, transport_stream_id, service_id, pmt_pid)` へ昇格し、ONID が曖昧な場合は意味objectへの昇格を抑止または欠落診断に留める。

EIT event の stable key は `DEFINED` または `UNDEFINED_TIME` の場合だけ `original_network_id / transport_stream_id / service_id / event_id` とし、開始時刻は表示・更新用フィールドとして別に扱う。`BOTH_TIMING_UNDEFINED` はARIB上の`event_id` field自体を否定せず、誤相関・誤削除を避ける本製品ポリシーとしてpersistent stable keyを割り当てず、bulk snapshot DTO から `ProgramProviderDataV1.programKey` を必要とする公開対象へ昇格させない。TIS/TvProvider は `event_id + start_time` に依存した stable key を作らず、`BOTH_TIMING_UNDEFINED` の raw event_id だけからstable keyを作らない。旧 indexed JNI getterである `nativeGetEventStableIdentity()` は提供しない。

開始時刻変更によって TvProvider row を削除・再作成する場合でも、TIS / arib_si_engine_rs の stable identity は変更しない。`event_id + start_time` は表示・検索・provider row 再作成補助には使ってよいが、event identity の SSOT にしてはならない。

記述子診断は bulk snapshot DTO と `ProgramProviderDataV1.diagnostics.descriptorDiagnostics[]` で渡し、TIS はその内容を `internal_provider_data` の内部データとして保存する。旧 indexed JNI getter である `nativeGetEventDiagnosticDescriptorJson()` は提供しない。TvProvider の標準 title / description / 時刻列には番組名、short text、長形式イベント本文を入れる。さらに `ARIB_SI_EPG_TvProvider投影方針.md` で固定された範囲では、component / 音声コンポーネント / コンテンツジャンル / freeCA 由来の補足を `Programs.COLUMN_LONG_DESCRIPTION` へ整形して出してよい。イベントグループは LONG_DESCRIPTION へ出さず provider-data JSON の `eventGroups` に保存する。series、linkage、unknown descriptor、診断JSON は標準列へ出さず内部データに分離する。

自前 ARIB 文字列 decoder は字幕以外の SI/EPG 文字列だけを対象にする。未対応 escape、切り詰め escape、切り詰め漢字、置換文字数は診断要約として観測できる。字幕は `libaribcaption` の責務である。

### 文字 decoder 固定方針

自前 ARIB 文字列 decoder の設計対象範囲は、mirakc が EPG / サービスモデル構築で扱う範囲に合わせる。すなわち、字幕本文レンダリングではなく、サービス名、番組名、短形式イベント記述、長形式イベント記述、各種 SI/EPG descriptor のテキストフィールドを安定して文字列化する範囲を対象にする。

この範囲を超える字幕/文字スーパーPES、caption management/statement data、字幕本文、DRCS/外字レンダリング、厳密な組版制御は恒久的に `arib_si_engine_rs` の対象外である。有限なmanagement/timing構造factのruntime抽出はTIS caption Rust JNI boundary、全文字decode/renderは`libaribcaption`の責務とする。未対応 escape / 未対応文字は `panic` ではなく診断情報と置換文字へ変換する。これは本crateの設計方針として固定する。

## mirakc 相当の ARIB 文字列範囲

自前 decoder は mirakc-arib が EPG / サービスモデル構築で文字列化している範囲に限定する。対象は SDT サービス descriptor のサービス名、EIT short_event の番組名 / text、EIT extended_event の item description / item text / text、component descriptor、音声コンポーネントdescriptor、series descriptor の text/name である。

`extended_event_descriptor` は同一event内で `ISO_639_language_code` ごとに独立したdescriptor setとして扱う。各language setについてだけ、全fragmentの`last_descriptor_number`が一致し、`descriptor_number`が0から`last_descriptor_number`まで重複なく連続して揃うことをcomplete条件とする。異なるlanguage set間で同じ`descriptor_number`が存在しても重複とみなさず、一方のlanguage setの欠番・重複・`last_descriptor_number`不一致によって、独立してcompleteな別language setをinvalidにしない。completeなlanguage setは`descriptor_number`順に意味組み立てするが、fragmentのraw文字byte列を単純連結して1回だけ復号してはならない。各descriptor内の各文字列fieldはTR-B14 / TR-B15のSI文字列初期化規則に従って所定の初期状態から復号する。ただし連続する`descriptor_number`で`item_description_length == 0`となるitemについて、TR-B14 / TR-B15が前descriptor_numberのitem descriptionの継続と定める場合だけ、その継続文字列のdecoder stateを前descriptorから引き継ぎ、境界で再初期化しない。language code変更、descriptor_number欠落・重複、不正descriptor、またはこの継続条件に該当しない別文字列fieldへdecoder stateを持ち越さない。不完全なlanguage setだけをextended description / 長形式イベント項目の正常フィールドに採用せず、language codeを含む診断に記録する。字幕/文字スーパーPES、caption management/statement data、字幕本文、DRCS/外字レンダリング、組版制御、BMLは本crateの対象外であり、caption runtime境界は`../tis/DESIGN_JA.md`を正とする。

extended_eventのcollector完全性判定と文字decoder stateは別責務とする。collectorはlanguage set内の`descriptor_number`系列の完全性だけを確定し、文字decoderはfield boundaryと規定された継続だけを扱う。構造的にcompleteなlanguage setでも個別fieldのstrict decodeに失敗した場合は、そのfieldを正常値へ昇格させず、language code、`descriptor_number`、field種別、offsetを診断に残す。

## ARIB 文字列 decoder 入力境界と TvProvider 連携境界

ARIB SI/EPG文字デコードの仕様固定に使う入力形態は、実波 TS ファイルを必須形式にせず、descriptor byte array / section builder を主入力とする。対象は SDT サービス名、EIT short_event、extended_event fragment、長形式イベント項目、component、audio_component、series、従来8単位符号のunsupported escape、truncated text、replacement 診断である。APR / SP / MSZ / NSZは正常系回帰試験に含め、APRの改行、SPの空白、MSZ→NSZの文字サイズ状態遷移、およびMSZ適用対象外入力のstrict/lossy境界を確認する。XCSはEDCB方式の回帰試験として、構文的に正しいCSI/XCS sequenceの直後に通常文字列を置き、XCS sequence全体がconsumeされ、XCS自体の置換文字や出力が発生せず、後続文字列が初期のdesignation / invocation stateを保って復号されることを確認する。切り詰めまたは構文不正のCSI/XCSはstrict APIで失敗し、lossy APIではoffset・理由付き診断と置換になることを確認する。extended_eventについては、言語別complete set、通常fieldの初期化、連続するdescriptorで`item_description_length == 0`となる規定継続時のstate継承、継続条件外でstateを持ち越さないことを入力契約と試験対象に含める。別coding systemの入力試験を追加する場合は、対象放送方式のSI運用profileが当該fieldについてそのcoding systemを許可・signalingすることを先に設計契約へ固定し、汎用STD-B24 capabilityだけを根拠に試験対象へ加えない。

Rust descriptor モデルから Kotlin/TvProvider へ渡す通常境界は、`ProgramProviderDataV1` と、TvProvider標準列へ投影するための構造化DTOだけにする。旧来の `eventGroupText`、`freeCaText`、`seriesName` のような表示用flatフィールドは通常投影経路では使わない。イベントグループは provider-data JSON の `eventGroups`、free_CA_mode は `freeCaMode`、series name は `series.name` に保存する。TvProvider の title / description / long description への投影は `ARIB_SI_EPG_TvProvider投影方針.md` を SSOT とし、同文書で固定済みの component/audio/content/freeCA 補足だけを `Programs.COLUMN_LONG_DESCRIPTION` へ出す。イベントグループは LONG_DESCRIPTION や一般 UI 本文へ出さない。

設計書は現行仕様中心にし、過去の経緯は CHANGELOG.md に分離する。


## Android レーティングドメイン境界

`arib_si_engine_rs` は ARIB `parental_rating_descriptor` の構造化解析結果だけをSSOTとする。Android `TvContentRating` の `domain` / `ratingSystem` / `rating` 文字列、`flattenToString()`、`Programs.COLUMN_CONTENT_RATING` への投影、`TvInputManager.isRatingBlocked()` に渡す値は TIS 側の責務である。

Rust 側に `com.android.tv` や `ISDB_<age>` の Android domain 決定文字列を持ち込んではならない。Rust は `country_code`, `raw_rating_byte`, `parse_status`, `raw_descriptor_bytes` を保持し、年齢値やAndroid ratingを別stateとして保存せず投影時に導出する。

## provider-data / 診断情報 Rust SSOT

### provider-data 受け渡し境界（推奨案A）

TIS が JNI へ渡す JSON は、保存形式ではなく Rust serde 型へ値を渡すための受け渡し用形式である。受け渡し用形式の型、必須項目、欠落時の扱い、旧形式拒否、値域検査は Rust 側の serde 型を正とする。

Rust provider-data builder は、受け渡し用 JSON を serde 型へ読み込み、必須項目、型、値域、旧形式混入を検査する。検査に通った入力だけから、保存用 JSON、識別子、切り詰め診断を生成する。

保存用 JSON の schema、正規化、識別子抽出、サイズ上限処理は Rust が単独で所有する。TIS は保存用 JSON を直接生成してはならない。ただしこの所有は保存表現の機械的SSOTに限り、TIS/product policyの算出責務を意味しない。

受け渡し用形式の schema 名は `maleicacid.tv.programRequest` / `maleicacid.tv.channelRequest` とし、保存用 schema 名 `maleicacid.tv.program` / `maleicacid.tv.channel` と分離する。

受け渡し用形式と保存用形式は別物である。受け渡し用形式を `Programs.COLUMN_INTERNAL_PROVIDER_DATA` / `Channels.COLUMN_INTERNAL_PROVIDER_DATA` に保存してはならない。

required field 欠落時に `0`、`false`、`jpn`、`UNKNOWN`、空文字で補完して provider-data を成立させてはならない。r50 以前の `;` 区切り形式、旧 flat provider-data、旧 provider-data 断片は受け渡し用形式としても保存用形式としても拒否する。

`DescriptorDiagnosticV1` は Rust が生成した正規 JSON を正とする。TIS から戻ってくる場合も、TIS が項目単位で再構築した JSON ではなく、Rust が生成した正規 JSON を透過保持したものだけを受ける。

`arib_si_engine_rs` は SI/EIT 意味解析に加えて、TvProvider `internal_provider_data` JSON v1 の構造SSOTを持つ。実装上は `provider_data` module に Rust `serde` struct を置き、JSON canonical encode、正規化、安定キー抽出をこの module に閉じる。

Programs / Channels のprovider-dataには、放送由来の意味事実だけを保存する。CASについて保存してよいのはCA descriptor/free_CA_mode等から導出した「CASを要する信号が存在するか」とその根拠・parse状態であり、`unsupportedCas`、`clearLivePlaybackSupported`、`channelRegistrationReady`、`epgPublishable`、`publishStateSource`のような現在の製品能力・TIF判断を保存しない。保存済みprovider-dataをcurrent policyのfallback sourceにしない。現在のchannel登録、EPG公開、CAS対応、ライブ再生可否はTISがcurrent `ServiceSemanticFacts`とcurrent product capabilityから決定する。

provider-data 全体は canonical UTF-8で16 KiBを目安上限、32 KiBを絶対上限とする。絶対上限を超える場合は、各操作後にcanonical encodeし直してサイズを測りながら、`diagnostics.rawProviderDataExtensions`、`diagnostics.descriptorDiagnostics`、`diagnostics.publishDiagnostics`、`extendedItems`の順に配列末尾から要素を除く。最後に長文フィールドをUTF-8 scalar境界で末尾から短縮する。それでも32 KiB以下にならない場合はprovider-data生成を失敗させ、識別子、時刻、CAS意味事実、レーティングraw値を欠落させた結果を保存しない。切り詰めた結果には`PROVIDER_DATA_TRUNCATED`、種類別dropped count、短縮前後のbyte数を必ず保存する。この診断自体を加えた後にも再度32 KiB以下であることを検証する。

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
    pub timing: ProgramTimingV1,
    pub source: ProgramSourceV1,
    pub cas: CasSemanticStateV1,
    pub ratings: Vec<RatingV1>,
    pub genres: Vec<GenreV1>,
    pub series: Option<SeriesV1>,
    pub event_groups: Vec<EventGroupV1>,
    pub linkage: Vec<LinkageV1>,
    pub free_ca_mode: Option<FreeCaModeV1>,
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

`ProgramTimingV1` は `startUtcMillis` と `durationMillis` だけをcanonical保存し、終了時刻はchecked additionで導出する。旧JSON v1の一致する`endUtcMillis`は正規化入力としてだけ受理し、新しいcanonical出力から除く。

### JSON 表現規則

JSON は正規表現ではなく、Rust `serde` / Kotlin JSON parser / JSON Schema によって読み書き・検証する。`ProgramProviderDataV1` の canonical JSON では、任意の単一オブジェクトは値が無い場合 `null`、繰り返し要素は空の場合 `[]`、常設containerは空でもオブジェクトとして出力する。具体的には、`series`、`freeCaMode` は未取得時 `null`、`ratings`、`genres`、`eventGroups`、`linkage`、`extendedItems` は未取得時 `[]`、`components` は常にオブジェクトとし、内部の `video`、`audio`、`subtitle`、`data` は空でも `[]` とする。runtimeで選択したmain `audio` / `video`要約をtop-levelへ保存しない。

未知のtop-level keyを読み込んだ場合は、無言で破棄せず`diagnostics.rawProviderDataExtensions[]`へ正規化する。version 1のnested DTOはclosedとし、未知nested keyはschema不一致として拒否する。これによりbuilder requestはstrict DTOへ1回だけdeserializeでき、`Value`走査による第二validatorを持たない。nested構造を拡張する場合はschema versionを更新する。`JSONObject` の手書き構築や文字列連結によるcanonical JSON生成を禁止する。

`series` は series_id、repeat_label、program_pattern、expire_date_valid、expire_date、episode_number、last_episode_number、series_name、parse_status を保持する。series name は番組表 title を置換する値ではない。

`eventGroups` は `event_group_descriptor` の構造保存先である。`groupType`はraw 4-bit値を保持し、先頭の`event_count` loopは`events[]`として`serviceId` / `eventId`だけを保存する。`groupType=0x4/0x5`の追加loopだけを`otherNetworkEvents[]`としてONID / TSID / serviceId / eventId付きで保存し、この場合`privateDataHex`は空とする。それ以外のgroup typeでは`otherNetworkEvents`を空にして残余byteを`privateDataHex`へ保存する。`shared` / `relay` / `movement`のような派生`kind`は`groupType`からruntimeで必要時に導出し、canonical JSONへ重複保存しない。

`linkage` は `linkage_descriptor` の transport_stream_id、original_network_id、service_id、linkage_type、private_data_prefix、parse_status を保持する。現行仕様では標準列、一般 UI、予約追従へ接続しない。

`freeCaMode` は EIT `free_CA_mode` の raw 値、scrambled 投影用 boolean、parse_status を保持する。CAS権利状態、カード状態、CAS HAL状態と混同しない。

PMT / 音声コンポーネントdescriptor から取得できるISO639言語は、対応する`components.audio[]`要素の`language`と`secondLanguage`だけに保持する。標準列向けの言語一覧はこの構造から投影し、top-levelへ複製しない。取得不能時に推測値を入れない。

`genres` は ARIB content descriptor の level1、level2、user_nibble、ARIB表示名、parse_statusだけを保持する。Android canonical genre の判定と `Programs.COLUMN_CANONICAL_GENRE` への投影はTIS側の責務であり、その投影結果、unmapped理由を本crateのprovider-dataへ保存しない。

`ratings` は parental_rating_descriptor の country_code、raw_rating_byte、parse_statusだけを保持する。年齢値はraw byteから投影時に導出する。未対応値を推測で Android レーティングに変換せず、`supported`や`mappedTvContentRating`をprovider-dataへ保存しない。Android `TvContentRating`文字列はTIS側が生成する。

`components.video[]` は ES PID、stream_type、component_tag、component_type、codec signaling、解像度、走査方式、aspect、profile / level、根拠 descriptor を ES/component単位で保持する。`components.audio[]` は ES PID、stream_type、component_tag、component_type、codec signaling、primary/secondary ISO639 language、channel configuration、sampling info、根拠 descriptor を ES/component単位で保持する。EIT の component descriptor 事実に対応する PMT component_tag が無い場合も descriptor 事実は保持し、PMT からだけ確定できる ES PID / stream_type / codec は `null` とする。PAT 等の sentinel PIDや推測値を入れない。`components.subtitle[]` は ES PID、component_tag、data_component_id、PMT Data Component Descriptorから得たDMF/Timing/service-kind fact、parse_statusを保持し、Android/TIS runtimeの`trackId`を保持しない。caption management data由来ISO639 languageはPES runtime factであり、本crateがgeneric PMT ISO639 descriptorから代用・捏造してprovider-dataへ保存しない。PES runtimeで得たlanguage setを永続provider-dataへ戻す場合は別の明示的なpublication input契約を先に設計する。`components.data[]` はデータcomponentのメタデータを保持するが、BML / data broadcast実行状態やUI状態は保持しない。

codec metadataの認識はライブviewable / playable対応宣言を意味しない。`ProgramProviderDataV1.components.video[]` / `components.audio[]` にrelease固有またはruntime capability判定の `r51PlaybackSupported` / `liveViewableClaim` を保存せず、再生可否とtrack選択はTIS runtimeの製品policyとdecoder capability判定に閉じる。

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

`decodeChannelProviderData()` は UTF-8、JSON、schema を Rust 側で検証し、canonical bytes、schema version、型付き `ServiceKey`、型付き `ChannelTune`、放送由来の`requiresCas`を返す。現行のString JNI surfaceではこれらを単一JSON result envelopeで返し、TAB区切り・hexという第二wire protocolを設けない。Kotlinが解釈するのはこのresult envelopeだけで、保存済みchannel provider-dataの検証・修復・canonical化はRustに閉じる。`ChannelTune` は `deliverySystem`、`frequencyHz`、`streamIdType`、`streamId`、`physicalChannel`、`satelliteBand`、`remoteControlKeyId` を持ち、`inputId`、表示名、backend名、driver名、driver固有slotを含めない。TV input ownershipはTvProvider channel rowのrequired `TvContract.Channels.COLUMN_INPUT_ID`をSSOTとし、Kotlin/TISはprovider-data decode前にrowのinputIdがcurrent TIS inputIdと一致することを検証する。

`inputJson` は Rust builder への入力 DTO であり、TvProvider 保存 schema ではない。Rustは最終provider-data bytes、schema version、切り詰め結果、診断件数を返す。`ProviderDataResult`に`signature`または`contentDigest`フィールドを設けない。

`rawBytes` は任意バイナリではなく、既存 TvProvider に保存済みの JSON v1 UTF-8 バイト列を指す。JNI 呼び出し元は provider-data を `String` 化して渡してはならず、保存済み BLOB バイト列をそのまま渡す。互換上 TvProvider が文字列として返す場合も、呼び出し元は UTF-8 バイト列へ戻すだけに限定し、provider-data JSON を Kotlin 側で解釈・再構築しない。

Rust は `rawBytes` が invalid UTF-8 または malformed JSON の場合、通常実行経路では panic せず、`ProviderDataResult` の失敗または key 抽出失敗へ落とす。provider-data bytesだけのdigest APIは設けない。同一公開内容の抑止判定はTISの行全体publish fingerprintを正とし、Rust builderの責務へ重複させない。

### current-program 診断情報

現在番組選択の `overlapCount`、`selectedProgramId`、`selectionRule` は TvProvider row id と process 内の query 結果に依存する runtime 診断であり、`ProgramProviderDataV1` へ保存しない。これらは TIS が process-local `CurrentProgramResolutionDiagnostic` として保持し、provider-data identity、canonical bytes、publish fingerprint の入力にしない。Rust provider-data schema と JNI に `diagnostics.currentProgram` および `appendCurrentProgramDiagnostics()` を設けない。

### ChannelProviderDataV1

Channel provider-data の正形式は JSON v1 のみとし、schema は `maleicacid.tv.channel` / `schemaVersion=1` とする。`arib_si_engine_rs/schema/channel_provider_data_v1.schema.json` は、channel rowのtune復元に必要な物理選局情報、ONID / TSID / service_id、ARIB/CAS意味事実の診断を検証対象にし、`inputId`、表示名、TIS/product policyをprovider-data schemaへ重複定義しない。r50以前の `;` 区切りkey-value形式、旧flat provider-data、旧provider-data断片は読み取り互換入力としても残さない。Channel provider-data の top-level envelope は `schema="maleicacid.tv.channel"`, `schemaVersion=1`, `serviceKey`, `tune`, `cas`, `diagnostics` を持つ JSON v1 とする。`cas`は放送信号から導出した`requiresCas`等の意味事実だけを保持し、`unsupportedCas` / `clearLivePlaybackSupported`を持たない。`diagnostics`に`channelRegistrationReady` / `epgPublishable` / `publishStateSource`を保存しない。`tune` は `deliverySystem`、`frequencyHz`、`streamId`、`streamIdType`、`physicalChannel`、`satelliteBand`、`remoteControlKeyId` を持つ。旧JSON v1の`displayName`は正規化入力としてだけ受理してcanonical出力から除く。`inputId`は保存せず、TvProvider rowの`Channels.COLUMN_INPUT_ID`を唯一のSSOTとする。backend名、driver名、px4相対slot等のbackend固有値は永続channel tune identityへ保存しない。CS110 は `streamIdType="NONE"` とし、`streamId` は null とする。

### 旧 event field / indexed JNI の廃止

`arib_si_engine_rs` の SI event DTO は旧 `canonicalGenres` フィールドを出力しない。Rust parser は Android canonical genre を決定しないため、`nativeGetEventCanonicalGenre()`、`nativeGetEventCanonicalGenresJson()` は互換シンボルとしても残さない。provider-dataにも canonical genre 投影結果を保持しない。

`nativeGetEventCount()` と `nativeGetEvent*` indexed JNI getter 群は廃止する。EIT event の通常境界は `nativeSnapshotBulkJson()` による bulk snapshot と provider-data builder API のみとする。未使用・廃止予定・互換専用の JNI シンボル、Kotlin private external 宣言、呼び出し不能な indexed path をリリース物へ残してはならない。互換のための空配列返却や空文字返却も禁止する。

### JSON Schema / schema 整合確認データ

現行仕様では Rust serde struct を SSOT としつつ、`arib_si_engine_rs/schema/program_provider_data_v1.schema.json`、`arib_si_engine_rs/schema/descriptor_diagnostic_v1.schema.json`、schema整合確認データを置く。`ProgramProviderDataV1` の JSON Schema はtop-levelだけをextension envelopeとして`additionalProperties: true`にし、nested DTOは`additionalProperties: false`で固定する。TIS/runtime/product policy用の旧fieldは未知extensionとして再保存しない。schema整合確認データは `arib_si_engine_rs/testdata/program_provider_data_v1/minimal_clear_program.json` と `tis/tests/assets/program_provider_data_v1/minimal_clear_program.json` の双方にバイト単位で同一に複製して置く。これは Rust host test と Android instrumentation asset packaging の参照経路が異なるためであり、2つの内容差分は違反とする。Rust test と Kotlin test は同じ内容のテストデータを読み、Rust JSON -> Kotlin round-trip と Kotlin input -> Rust build -> schema整合確認データとの一致を確認する。

### 現行実装との関係

文書上の正式schemaは本節を正とする。既存実装にflat JSON生成、`eventGroupText`、`freeCaText`、`seriesName`、`canonicalGenres`、indexed JNI getter、TIS runtime `trackId`、product capability field、selected main-track summaryなどの旧境界が残っている場合、それは実装未達であり完成済み仕様として扱わない。旧境界は互換経路として残さず削除する。本節は文書・schema・schema整合確認データの整合を固定する。`provider_data.rs` は serde_json ベースの ProgramProviderDataV1 / ChannelProviderDataV1 構造を通常経路とし、canonical JSON生成と安定キー抽出をこの境界へ閉じる。既存のprovider-data `signature`フィールド/API、SHA-256計算、JSON断片のraw流用、flat event DTO、indexed JNI getterも実装未達として扱い、リリース物へ残してはならない。

## event_group_descriptor の provider-data 契約

`event_group_descriptor` は現行仕様でdescriptor単位に構造化変換し、provider-data JSON の `eventGroups` に保存する。各descriptorはraw `groupType`、先頭の `event_count` loopを表す `events[] { serviceId, eventId }`、`groupType=0x4/0x5` の追加loopを表す `otherNetworkEvents[] { originalNetworkId, transportStreamId, serviceId, eventId }`、その他のgroup typeの残余byteを表す `privateDataHex`、`parseStatus` を保持する。`groupType=0x4/0x5` では残余を8-byte単位のother-network entryとして完全に解釈できる場合だけ受理し `privateDataHex` は空、それ以外のgroup typeでは `otherNetworkEvents` は空とし残余byteを `privateDataHex` に損失なく保持する。現在transportのONID / TSIDを `events[]` に補完してはならず、存在しないONID / TSIDを0やnullで擬似的な同一item shapeへ押し込まない。`kind`は保存せず必要時に`groupType`から導出する。現行仕様では一般 UI や予約追従へ接続しない。予約追従へ接続する場合は、event identity と authoritative 条件を設計正本へ固定し、安全に確定できる場合だけにする。

## series_descriptor の provider-data と標準列連携

`series_descriptor` は現行仕様で構造化変換する。`series_id` と episode number は TIS が `ARIB_SI_EPG_TvProvider投影方針.md` に従って Android 標準列へ投影できるように出力する。last episode number は通常の `TvContract.Programs` に自然対応する標準列がないため標準列候補として扱わず、repeat label、program pattern、expire date、series name と合わせて provider-data JSON の series 構造に保持する。series name は番組表表示 title を置換する値として扱わない。

## free_CA_mode / 音声言語 / 視聴年齢制限の構造化契約

EIT `free_CA_mode` の規範対象は、地上デジタルについて現行日本語TR-B14 6.13、BS/広帯域CSについて現行日本語TR-B15 8.9のうち製品scopeに適用されるCA運用規定とする。現時点の検証証拠は、ARIB公式英訳TR-B14 6.7-E1 Fascicle 5のconditional-access運用における`Non-scramble/Scramble`およびfree/pay判定の節と、ARIB公式英訳TR-B15 4.6-E1のCA運用における`Non-Scramble/Scramble`および`Free Program/Pay Program`の節である。これら取得可能な英訳では、`free_CA_mode`は無料/有料区分の判定に用い、componentの実スクランブル状態はTS packet headerの`transport_scrambling_control`で判定する別軸としている。この意味分離を本製品でも採用し、CAS権利状態、カード状態、CAS HAL状態とも混同しない。TIS は AOSP 契約に従う TvProvider 投影を `ARIB_SI_EPG_TvProvider投影方針.md` に従って行うが、`free_CA_mode` 単独から実 descramble の要否またはライブ再生可否を導出しない。なお現行日本語TR-B14 6.13 / TR-B15 8.9本文は本レビュー環境で未取得であり、上記英訳版から現行日本語原文までの当該規定差分は未証明なので、この英訳確認だけをもって6.13 / 8.9への完全適合確認済みとは扱わない。音声 ISO639 language は PMT / 音声コンポーネントdescriptor 等から取得できる値だけを保持し、取得不能時に推測しない。視聴年齢制限はARIB raw値とparse状態を意味データとして保持し、Androidへの対応可否・写像結果はTISに閉じる。

### TDT/TOT broadcast clock fact

PID `0x0014` の TDT (`table_id=0x70`) と TOT (`table_id=0x73`) は、いずれもSTD-B10のJST date/time fieldを同じtyped broadcast clock fact (`tableId / MJD / millisOfDay`)へ正規化してTISへ渡す。TDTは`section_length=5`のshort section、TOTはJST_time・descriptor loop・CRC_32を含むshort sectionとして構文を検証し、TOTはCRC_32一致後だけfactを更新する。local time offset descriptorはJST clock factそのものと混同せず、Timing=10のclock authorityに必要な現在JST date/timeだけをこのfactへ含める。受信monotonic時刻との相関、clock continuity/discontinuity generation、STM deadlineはTISのpresentation policyであり、本crateへ持ち込まない。

## PSI/SIのTable ID規則と意味解釈の責務

Tuner HALは汎用的なMPEG-TS sectionの伝送処理（ペイロード抽出、sectionの区切り、宣言長の検査、任意のCRC検査、フィルター照合、queueまたはFMQへの配送、伝送診断）だけを担当する。PAT、CAT、PMT、NIT、SDT、BAT、EIT、TDT、TOT、BIT、NBIT、LDT、CDT、PCAT、SDTT、AIT、AMTを含む表固有の意味解析、正規化、複数sectionの集約、意味オブジェクトの生成は`arib_si_engine_rs`とTISが担当し、Tuner HALへ戻さない。

TSの伝送構文、`table_id`別のsection長上限、CRCとraw配送条件、公開フィルター状態は`../tuner_hal/DESIGN_JA.md`の「セクションフィルターの条件幅とsection長上限」を正とする。本crateは、それらの条件を満たして上位から入力されたsectionについてだけ、次表の意味解釈を担当する。予約済み、未割り当て、私用、外部所有の`table_id`を型付き意味オブジェクトとして推測しない。

### 意味解釈の責務
| 対象 | 主なtable ID | 意味解釈の責務 | Tuner HALの処理 | 配送規則 | 禁止事項 | 理由 |
|---|---|---|---|---|---|---|
| すべてのPSI/SI | PAT 0x00、CAT 0x01、PMT 0x02、NIT 0x40/0x41、SDT 0x42/0x46、BAT 0x4A、EIT 0x4E-0x6F、TDT 0x70、TOT 0x73、BIT 0xC4、AMT 0xFE、私用・将来用ID | TISまたはTuner HALより上位の要求元 | 汎用sectionフィルターの照合、外形処理、宣言長・CRC処理、メタデータとバイト列の配送だけ | 条件に一致する完全なsectionは、要求元の有効な経路へすべて配送する。条件に一致しないsectionだけを配送対象外とし、`table_id`を理由に無言で破棄しない | 表ごとの意味解析・正規化・オブジェクト生成、EPG・時刻・アプリケーションDBの更新、特定の`table_id`に対する固定破棄、HAL内の意味別振り分け | AOSP Tuner HALのsection APIは、PSI/SI表ごとの意味APIではなく、汎用のsection転送を公開しているため |


### EIT分類・component metadata・CAS signaling fact の責務境界

- EITの分類はARIB/DVB SI上のtable identityだけを表し、`present/following actual`、`present/following other`、`schedule actual`、`schedule other`として保持する。製品release名や製品ごとの収集範囲をSI engineの型へ埋め込まない。
- ESの`component_tag`、`component_type`、`data_component_id`、language等はdescriptorから観測できた場合だけ値を持つ。descriptor不在は`null`/`None`であり、0、0x0008、`und`等の実在値を欠損表現として捏造しない。
- CASについては、PMT CA descriptorの解決完了、CA descriptorの存在、SDT/EIT `free_CA_mode`を独立したbroadcast factとして保持する。これらをSI engine内部で単一の製品policy状態へ畳み込まない。


### codec capability・欠損値・CA cross-check・section整合性

- `stream_type`やdescriptorから導出できるcodec名は放送事実として保持する。SI engineはHEVC等を製品releaseの再生可否へ変換せず、codec capability判定はTIS/MediaCodec側のpolicyとする。
- optional descriptor値が存在しない場合は合法的absenceとして`null`を保持し、syntax破損による取得不能とはtyped diagnosticで区別する。
- `free_CA_mode`はCA descriptorの代用品ではない。PMT解析完了後に`free_CA_mode=1`なのにCA descriptorを観測できない場合はbroadcast fact間の不整合として診断し、`requires_cas`をSI flagだけで上書きしない。
- 同一table versionで`last_section_number`が変化した場合、または`section_number > last_section_number`の場合、そのversionをcompleteと判定しない。EIT scheduleのsegment gapはEIT固有storeで扱い、この一般section trackerの連番complete判定をEITへ流用しない。

## MMT/TLV SI意味解析

`JapanAdvancedMmtTlvProfile` では、本crateの責務をMPEG-2 PSI/SIだけに固定せず、TISがTuner SDK filterから受け取った **完全なTLV-SI signaling単位、MMTP control packet、M2 section** の構文・意味解析まで拡張する。raw TLV packet framing、IP reassembly、一般MMTP media packet/MPU assembly、filter routingはTuner HALの責務であり、本crateへ複製しない。

規範対象は現行日本語 ARIB STD-B60 / STD-B32 と対象物理方式のSTD-B44 / STD-B79 / STD-B80を正とする。公開詳細確認に用いるSTD-B60 1.14-E1では、映像・音声等がMFU/MPUとしてMMTP化されIP packetで伝送され、一個のIPまたはheader-compressed IP packetが一個のTLV packetで運ばれること、TLV-SIがtuningとIP/service対応を、MMT-SIがprogram構成を表すこと、PA messageがMMT-SI entry pointでMPTがassetとlocationを表すことを確認している。2.0で追加された高度地上固有descriptorは現行日本語本文を取得・照合するまで、未知descriptorを既知意味へ推測昇格させない。

### transport-tagged identity

既存TSとMMT/TLVを一つの整数tupleへ押し込めず、意味モデルのtransport identityを次のtagged shapeにする。

```text
BroadcastTransportIdentity =
  MpegTs { originalNetworkId, transportStreamId }
  Tlv    { originalNetworkId, tlvStreamId }

BroadcastServiceIdentity = {
  transport: BroadcastTransportIdentity,
  serviceId
}
```

TLV stream IDを `transportStreamId` と呼び替えない。PATがないMMT/TLV serviceに架空のPMT PID / PCR PID / TSIDを生成しない。共通 `ServiceSemanticFacts` はtransport identity、service ID、service type、codec/asset、CA、構文診断を持つtransport-neutral外形へ拡張し、TS固有のPMT/PCR存在状態は `MpegTs` branch、MMT固有のMPT/asset状態は `Tlv` branchだけで意味を持つ。

### TLV-SI

TLV `SECTION` filterから受けるsignalingについて、少なくともTLV-NITとAMTを解析対象とする。TLV-NITから `(original_network_id, tlv_stream_id)`、service list、tuning/service関連情報を構造化し、AMTからserviceに対応するIPv4/IPv6 data flowを構造化する。section/version/current-next/CRC等、適用規格で存在する更新単位をinstance別に管理し、不完全instanceをcomplete snapshotへ混ぜない。未知descriptorはtag/raw/diagnosticとして保持し、TS NIT descriptorとして誤解釈しない。

### MMT-SI / PA / MPT / M2 section

`mmtpPid=0x0000` の完全なcontrol packetからPA messageを解析し、PA内table list/version/lengthを検証してからMPTへ進む。MPTはpackage ID、asset ID/type、clock relation、location list、descriptorを構造化する。STD-B60でpackage ID下位16bitがservice identificationと一致する規定をservice相関に使用するが、不正長や矛盾時にservice IDを推測補正しない。

asset locationは少なくとも同一packet flow、IPv4/IPv6 source/destination/port + packet IDをtagged locationとして保持する。`hvc1/hev1/mp4a`等のasset typeは放送由来factとして返すが、Android decoder対応可否へ変換しない。

M2 section messageで運ばれるMH-EIT / MH-SDT / MH-TOT / CAT(MH)等は、message lengthと内包section lengthをそれぞれ検証してからtable parserへ渡す。MH-EIT eventの時刻、descriptor、version/section寿命はTS EITと共通化できる意味だけを共通型へ上げ、table IDやtransport identityを失わない。CA message/CAT(MH)はCA system/scramble system/descriptor事実だけを返し、CAS HAL対応可否を算出しない。

### parser failure と更新寿命

TLV-SI、PA/MPT、M2 sectionは `(transport identity, table/message identity, version, section/subset)` を含むinstance keyで管理する。length不整合、CRC不正、fragment欠落、version混在、同一identityの矛盾は正常snapshotへ採用せず、offset/raw prefix/reasonをdiagnosticへ残す。旧generationから到着したpayloadを新generationのinstanceへ合流させない。未知の2.0高度地上descriptorはunknownとして保持できるが、その存在を無視して「完全適合」と判定しない。

### provider-data v2

TLV stream IDをv1 `serviceKey.transportStreamId`へ流用しないため、MMT/TLV実装と同時にchannel/program provider-dataを **v2** へ移行する。v2のservice identityは次のcanonical shapeを持つ。

```json
{
  "originalNetworkId": 1,
  "serviceId": 101,
  "transport": { "kind": "TLV", "tlvStreamId": 7 }
}
```

TSの場合は `transport = { "kind": "MPEG_TS", "transportStreamId": ... }` とする。program stable keyはこのservice identityへ `eventId` を加える。`kind`と対応しないIDを同居させず、TLVでTSID、TSでTLV stream IDを補完しない。

実装時のreaderは既存v1を読み取って `MPEG_TS` identityへ正規化できなければならない。writerはv2導入commit以後、新規/更新rowをv2でcanonical encodeする。v1 rowを読み出しただけでDB全件を書き換えず、通常のchannel/program update transactionでv2へ移行する。schemaVersionだけを2へ上げてv1 shapeを残すことは禁止する。具体JSON SchemaとRust serde structは実装PRで同時に追加し、Kotlin側へ第二schema定義を作らない。

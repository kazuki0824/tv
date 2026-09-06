# tuner_hal2 product integration

この文書は、`tuner_hal2` を LineageOS 22.1 / Android 15 product image の既定 Tuner HAL service として組み込むためのSSOTである。

## 0. 固定方針

```text
- product default Tuner HAL service は tuner_hal2 だけとする。
- 旧 tuner_hal は参照用ソースとして repository に残すだけで、product image へ入れない。
- ITuner/default を登録する実体は android.hardware.tv.tuner-service.maleicacid2 だけとする。
- 旧 tuner_hal の product package、VINTF fragment、init rc、PRODUCT_PACKAGES、product integration を同一productで有効化しない。
- 旧 `tuner_hal/INTEGRATION.md` は legacy/reference 用であり、既定 product 統合手順のSSOTにはしない。
```

## 1. product makefile

製品の product makefile で次を継承する。

```make
$(call inherit-product, vendor/maleicacid/tv/tuner_hal2/config/product_integration.mk)
```

`config/product_integration.mk` は次だけを `PRODUCT_PACKAGES` に追加する。

```make
PRODUCT_PACKAGES += \
    android.hardware.tv.tuner-service.maleicacid2 \
    maleicacid_tuner_hal2_ueventd_rc
```

旧 `tuner_hal` の `maleicacid.tv.tuner_hal-service` は追加しない。

## 2. BoardConfig / sepolicy

BoardConfig 側で次を取り込む。

```make
include vendor/maleicacid/tv/tuner_hal2/config/BoardConfigVendorSePolicy.mk
```

`BoardConfigVendorSePolicy.mk` は `vendor/maleicacid/tv/tuner_hal2/sepolicy` だけを既定Tuner HAL用のvendor sepolicyとして追加する。

## 3. ueventd import

製品側の vendor ueventd rc から次を import する。

```rc
import /vendor/etc/ueventd.tuner_hal2.rc
```

`ueventd.tuner_hal2.rc` はDVB / px4 / dma_heapのdevice node permissionを設定する。

## 3.1 px4_drv readback ABI のproduct前提

px4 backendをproductへ組み込む場合、採用kernel driverは `../開発規則.md` のpx4_drv product-level invariantを満たす版へ固定する。公開AIDLの意味、status capability、generation/readiness、scan callbackの規範値は `../tuner_hal/DESIGN_JA.md` を正とし、本節では再定義しない。

product build / 実機VTSの前に、少なくとも次のABI接続を確認する。

```text
- include/ptx_ioctl.h に PTX_GET_LOCK_STATUS が存在すること
- include/ptx_ioctl.h に pointer-free fixed-size PTX_GET_TMCC_TSID_LIST が存在すること
- tuner_hal2/device の ABI mirror と ioctl number / payload size が一致すること
- TMCC TSID readbackがactive frontend sessionの既存control fdを使い、同一px4 chardevを再openしないこと
- VTS/profile toolingがdriver ioctlを直接呼ばず public Tuner AIDLだけを試験すること
```

これらを満たさないdriverでは、対応するpx4 frontend capabilityをproductで有効化したままVTS成功を宣言しない。

## 4. service登録

`tuner_hal2/Android.bp` の `rust_binary` は次を持つ。

```text
name: android.hardware.tv.tuner-service.maleicacid2
init_rc: tuner_hal2/init/android.hardware.tv.tuner-service.maleicacid2.rc
vintf_fragments: tuner_hal2/manifest/android.hardware.tv.tuner-service.maleicacid2.xml
```

init rc は `android.hardware.tv.tuner.ITuner/default` を登録する。VINTF fragmentも `ITuner/default` だけを宣言する。

## 5. 旧tuner_halの扱い

旧 `tuner_hal` は参照用ソースである。次をproductへ入れてはならない。

```text
- maleicacid.tv.tuner_hal-service
- tuner_hal/tuner-hal-service.rc
- tuner_hal/tuner-hal-service.xml
```

旧実装を手動でビルド・参照することは妨げないが、同一productで `ITuner/default` を二重登録してはならない。

## 6. VTS / product config policy

VTS / product config の公開契約、capability、`VtsEnvironmentProfile`の入力、状態、`VTS-STATE-BOUND` / `VTS-STATE-REJECTED`の意味は `../tuner_hal/DESIGN_JA.md` の`製品スコープ / AOSP capability / VTS profile 境界`、`CapabilitySnapshot`、`ProductProfile`、`VTS環境に関する設計保留`を正とする。本節は、それらをproduct buildと実機VTSへ接続する配置・生成・検証経路だけを所有し、profile入力の規範値、HAL capability、公開API戻り値、VTS状態を再定義しない。

本製品は monitor event feature を製品能力として採用せず、静的VTS/product configでも同featureを要求・広告する構成にしない。monitor event の公開API戻り値とcapability契約は `../tuner_hal/DESIGN_JA.md` を正とし、本書では重複定義しない。本書のproduct integration設定を、未定義の将来profileでmonitor eventを有効化するための切替点として扱ってはならない。

### 6.1 単一VtsEnvironmentProfileファイルと依存方向

`VtsEnvironmentProfile` は論理概念だけではなく、対象productごとに実在する単一設定ファイルとする。VTS環境に関して人間またはCLIが保存する設定はこの1ファイルだけを正本とし、1回のVTS構成生成で複数のprofileファイル、product makefileの個別変数、生成済みXMLの手編集値を合成して1個の論理profileを作ってはならない。serialization形式と物理pathは実装詳細として一意に選んでよいが、同じproduct/profileについて複数の人間編集設定ファイルを設けてはならない。

対話CLIはこの同一ファイルを生成・読み込み・更新する。地域から生成した受信候補、実機接続後に解決したfrequency / service / PAT / PMT / PIDに由来する具体値も、別のderived-resolution設定ファイルへ分離せず、同じ`VtsEnvironmentProfile`ファイルへ保存する。生成AOSP Tuner VTS XMLはderived artifactであり、この設定ファイルと同格の正本ではない。

Tuner HALのcapability、公開個数、FMQ/PES/AV/DVR/worker等の製品資源上限、frontend probe結果を`VtsEnvironmentProfile`の独立した規範値として複製してはならない。これらは`../tuner_hal/DESIGN_JA.md`の`ProductProfile` / `CapabilitySnapshot`と`tuner_hal2`の実機probeを正本とする。profile compilerが静的照合に必要とするHAL側情報は、同じ正本から機械的に取得したread-only contractとして入力してよいが、人間編集設定として保存せず、HALのruntime能力を変更する入力にも使用しない。

依存方向は次に固定する。

```text
interactive VTS profile CLI
        |
        v
single VtsEnvironmentProfile file
        |
        +--> regional candidate resolver --+
        |                                   |
        +--> device resolver through AIDL --+
        |                                   |
        +<----------------------------------+
        |
        v
VTS profile compiler / validator
        |
        +--> selected AOSP VTS schema / loader contract
        |
        +--> read-only tuner_hal2 capability contract
        |
        v
static AOSP Tuner VTS XML
        |
        v
AOSP Tuner VTS
        |
        v
public Tuner AIDL
        |
        v
tuner_hal2
```

逆方向に、VTS XML、variant property、VTS profileまたはVTS test resultから`tuner_hal2`の`CapabilitySnapshot`、frontend registry、backend probe結果、公開API成功範囲を変更してはならない。VTS設定は`tuner_hal2`を試験するための環境記述であり、被試験HALの能力を選択・拡張・縮小する設定面ではない。

### 6.2 profileフィールドの消費契約

`VtsEnvironmentProfile` はAOSP Tuner VTS XMLより高水準の単一入力SSOTである。このためprofileの全フィールドがXMLへそのままシリアライズされる必要はない。ただし、profileに永続化する各フィールドは必ず次のいずれかに分類できなければならない。

1. AOSP Tuner VTS XMLの1個以上の具体値へ直接変換される。
2. XMLへ出力する具体値を決定するresolver入力として消費される。
3. XMLの値、対象VTS契約、生成先またはinstall先を検証・選択するためにcompiler / validatorが消費する。

上記のどれにも該当せず、XML生成・検証・配置のいずれにも影響しないprofileフィールドを追加してはならない。「後で使う可能性がある」という理由だけで未消費metadataを保存しない。compiler / validatorは、認識しているprofile fieldが上記の消費経路を持たない場合にfail-closedで拒否する。

代表的な対応は次とする。

| profile情報 | XMLへの反映 | 消費先 |
|---|---|---|
| frontend type / frequency / stream selector | 直接反映 | frontend設定 |
| filter / DVR種別、PID、flow、queue要求 | 直接反映 | filter / DVR / data flow設定 |
| 地域指定 | 直接は反映しない | region resolverがfrontend/frequency候補を決定 |
| 地域から得た受信候補集合 | 最終採用値だけ反映 | device resolverが実機でfrequency候補を評価 |
| service / PAT / PMTで得た識別値 | XMLが必要とする最終PID等へ反映またはその決定に使用 | device resolver / compiler |
| AOSP/VTS契約識別 | 直接は反映しない | compilerが使用schema / loader contractを照合 |
| target product / backend | 直接は反映しない | compilerが`tuner_hal2`用capabilityと生成先を選択・検証 |
| variant指定 | loader契約に応じてfilename/propertyへ反映 | filename解決 / product配置 |

### 6.3 対話CLIによるprofile生成とtuner_hal2限定target

VTS環境profileには、実機が接続されていない開発環境でも作成・保存できる対話CLIを設ける。CLIの論理操作は少なくとも次を持つ。

```text
init            対話入力から単一VtsEnvironmentProfileファイルを新規作成して保存する
resolve-region  同じファイルの地域入力から受信候補集合を生成して同じファイルへ保存する
resolve-device  public Tuner AIDLと受信TSを使って実機依存値を解決し同じファイルへ保存する
compile         同じファイルだけを入力に検証しAOSP Tuner VTS XMLを生成する
install-device  compile済みVTS XMLをadb root/remount可能な試験端末の解決済みvendor pathへ配置する
```

`init` は実機接続を前提にしない。AOSP/VTS契約識別、対象backend/product、受信方式、明示入力または地域入力、要求するVTS flow、queue要求等、入力時点で確定できる値を対話的に取得し、未確定項目を架空値で埋めずにprofileを保存する。必要入力が揃っていないprofileは `../tuner_hal/DESIGN_JA.md` の `VTS-STATE-UNBOUND` 判定に従い、保存可能であっても静的VTS XMLをinstall可能とは扱わない。

CLIと生成profileのtargetはproduct defaultである`tuner_hal2`に固定する。profile compiler、生成XML module、variant設定、vendor imageへの配置は`tuner_hal2`のproduct integrationだけへ接続し、旧`tuner_hal`の`profiles/`、`tools/render_vts_config.py`、`config/tuner_vts_config_*`、旧service packageを更新・参照・fallback先にしてはならない。旧`tuner_hal`に存在するprofile rendererは設計参考として読めても、このCLIの実行対象または生成先にはしない。

ここでいう`tuner_hal2`への反映は、`tuner_hal2`を被試験HALとするVTS構成を生成・配置することだけを意味する。`VtsEnvironmentProfile`をHAL serviceがruntime設定として読み込み、`CapabilitySnapshot`、frontend registry、backend probe結果、資源上限、公開API成功範囲を変更する経路は設けない。

### 6.4 地域入力からの受信候補解決と実機解決

`init`では具体的な受信チャンネルを必須入力にせず、地上波では住所、郵便番号、緯度経度から、実機VTS確認を開始するための少数の受信候補を導出する。都道府県名だけでは地点ごとの距離を一意に定められないため、県内全送信所の物理ch和集合へ拡大せずfail-closedで拒否する。

住所入力はGSIの全国市区町村表で行政区域をcanonicalizeしてからGSI AddressSearchへ渡す。都道府県名が省略され、入力先頭が`横浜市緑区`や`座間市`のような市区町村名と全国で一意に一致する場合は対応する都道府県名を補完する。`府中市`のように最長一致する市区町村名が複数都道府県に存在する場合は推測で選ばずfail-closedとし、都道府県名の入力を要求する。市区町村prefixが得られない自由形式住所は補完せず、その文字列をGSIへ渡し、GSIが一意の座標を返せない場合はfail-closedとする。

住所と郵便番号はGSIで緯度経度へ解決し、緯度経度入力はその座標を直接使用する。入力住所の都道府県を送信所探索境界にしてはならない。

地上波候補の放送情報正本はINA4Nの公開地上デジタル中継局ページに固定する。region resolverは47都道府県のINA4N周波数ページと送信所詳細ページを全国送信所集合として取得し、送信所名、詳細URL、放送局別物理ch、偏波、出力、INA4N記載の「主なカバーエリア」原文、所在地を送信所単位で保持する。県境を越えた受信を候補から除外しない。`prefecture_channels`のような県内全送信所の物理ch和集合を通常候補生成の正本にしてはならない。

INA4Nで偏波または出力が空欄の場合はunknownのまま保持し、既定値を捏造しない。物理chが有効なのに出力・偏波だけがunknownという理由で送信所を候補datasetから削除してはならない。現行ISDB-T物理chを1件も持たない旧局等はVTS受信候補ではないため候補datasetから除外してよい。

送信所座標はINA4N詳細ページの地図リンクに埋め込まれた座標を第一選択とする。INA4Nに座標リンクがない場合は、同一局であることを人間が確認したA-PAB公開UIの局位置を`coordinate_overrides`へ明示して補完してよい。A-PABから物理ch、出力、偏波を取り込んではならない。A-PAB overrideがない場合はINA4N所在地文字列をGSIでgeocodeしてよい。これらでも座標が得られない場合、座標unknownのまま送信所を保持する。

各送信所では、既知出力が最大のcurrent ISDB-T serviceを代表probe serviceとする。全serviceの出力がunknownならremote-control key、物理ch、service名の順で決定的に1件を選ぶ。region resolverは代表probe serviceのINA4N記載送信出力`P[W]`と入力地点から送信所までの大円距離`d[km]`について`P / max(d, 0.1)^2`を計算し、値が計算できる送信所をscore降順に並べる。出力unknownで座標既知の送信所は既知score群の後ろで距離昇順、座標unknownの送信所は最後尾とする。`P/d^2`と距離は実機確認の探索順を決めるheuristicに限定し、受信電界強度、ERP/EIRP、terrain、建物、アンテナ高を含む受信可能性の証明として扱わない。INA4N「主なカバーエリア」は順位付けに使用しない。

`region.transmitter_candidate_count`はregion resolverが採用する送信所数`k`を表すresolver入力であり、自然数（1以上の整数）だけを受理する。既定値は`2`とする。`0`、負数、浮動小数、文字列その他の自然数でない値はfail-closedで拒否する。対話`init`は地域入力があるISDB-T profileで`k`を尋ね、未入力なら`2`を保存する。`resolve-region -k N`で明示的に上書きしてよい。

region resolverは全国送信所を上記規則で順位付けした後、まず上位`k`送信所を確定し、その各送信所について代表probe serviceの物理chを1件ずつ候補化する。同一送信所の別物理chをfallback channelとして展開してはならない。上位`k`送信所の中で同一frequencyが重複した場合、同じTune操作を重複実行する意味がないため最初の1件だけを保持するが、その重複を埋めるために`k`位より下の送信所を繰り上げてはならない。したがって`k`は物理ch数や一意frequency数ではなく採用送信所数を表し、実際のTune候補数は`k`以下となる。

`resolve-region`が生成するのはVTS実機確認を開始するための順位付き受信候補であり、service ID、PMT PID、audio/video/record PID等のTS内識別値を地域情報から推定して確定してはならない。実機接続後、`resolve-device`はpublic Tuner AIDLで候補frequencyを順位順にtuneし、LOCKEDを確認したTSからPATを取得してserviceとPMT PIDを解決し、PMTから要求flowに必要なES PIDを解決する。解決したfrequency、service、PAT/PMTに基づくPID等は同じ`VtsEnvironmentProfile`へ保存する。

要求flowを満たすservice候補が1件ならそのserviceを採用してよい。複数存在する場合は対話CLIで選択させるか、profileに明示されたselectorで決定する。非対話実行で複数候補が残りselectorがない場合はfail-closedとし、偶然の列挙順からserviceを選ばない。service選択のambiguityは受信frequencyの失敗ではないため、別frequencyへ自動fallbackする理由にしてはならない。

具体的なfrontend/frequency/PIDからXMLを生成した後、VTS preflight失敗を理由に別候補へ自動fallbackして同じ生成物の意味を変えてはならない。別候補を採用する場合は`resolve-device`で同じprofileファイルの解決値を更新し、その更新後profileからXMLを再生成する。

### 6.5 build-time compiler / validator契約

VTS用静的XMLは手編集正本にせず、単一`VtsEnvironmentProfile`ファイルだけをprofile入力としてbuild-timeのcompiler / validatorで生成する。compiler / validatorは少なくとも次の順序でfail-closedに検証する。

1. profile自体のschema、必須項目、型、ID参照、および全profile fieldに6.2の消費経路があることを検証する。profileが保存可能でも、`../tuner_hal/DESIGN_JA.md`が静的XMLに要求する具体入力が未解決ならXML生成へ進めない。
2. `../tuner_hal/DESIGN_JA.md` が要求するVTS契約識別入力と、実際にbuild/testへ使用するAOSP Tuner VTS契約を照合する。一意に一致しない場合はXMLを生成・installしない。
3. profileのfrontend設定、flow、filter種別、DVR種別、PID、queue容量が`../tuner_hal/DESIGN_JA.md`で成功対応として認めた公開契約と矛盾しないことを検証する。
4. `tuner_hal2`の同一product capability正本から機械的に取得したread-only contractと照合し、VTSが要求するfilter / DVR個数、FMQ / processing bufferその他の静的資源claimが製品上限を超えないことを依存閉包単位で検証する。VTS合格のためにHAL側の能力値を上書きまたは縮退させてはならない。
5. 選択したAOSP Tuner VTS schemaで生成XMLを検証する。
6. `../tuner_hal/DESIGN_JA.md` のfilename解決契約に従い、選択したVTS loaderとvariant入力からinstall先を一意に解決する。

いずれかが失敗した場合は、推測値、既定PID、既定周波数、sample XML値、別profileへのfallbackで補完せず、VTS config artifactを成立させない。生成済みXMLを直接修正してvalidatorを迂回してはならない。

### 6.6 build-timeとdevice preflightの境界

build-time compiler / validatorは、静的なHAL product contractとAOSP VTS契約の整合を検証する。起動時probeで初めて確定するfrontendの実在性、公開frontend ID、hardware info、実信号のLOCKED到達、PID上の実データ到来はbuild-timeに捏造しない。

実機がある場合、`resolve-device`はprofileの未解決な受信候補とTS内識別値を具体化するためにpublic Tuner AIDLを使用する。具体値の解決後、VTS実行前には同じくpublic Tuner AIDLだけを使用するdevice preflightを行い、生成済みVTS構成が要求するfrontend種別、公開`FrontendInfo` / `DemuxCapabilities`、解決済み信号のLOCKED到達、対象PIDのdata pathが実機上で成立することを確認する。具体的な合否項目と実行手順は`タスク完了判定の実施方法.md`を正とし、本書では試験手順を二重定義しない。

`resolve-device`とpreflightはHAL内部registry、private diagnostic、driver-private stateをVTS成功条件の正本にしない。解決済みprofileに対するpreflight不成立時は別frontend、別周波数、別PIDへ自動fallbackして同じprofileの解決結果を変更せず、そのprofileによるVTS実行を開始しない。preflight結果またはVTS実行結果をHAL runtime capabilityへフィードバックして次回起動時の公開能力を変更してはならない。

#### VTS device agentの配置と実行

`resolve-device`で使用する`maleicacid_tuner_hal2_vts_agent`は通常productの`PRODUCT_PACKAGES`へ含めない。host CLIは明示されたagent binaryを一時的に`/data/local/tmp`へadb-pushして起動し、解決処理終了後に除去する。この通常経路によってproduct imageへtest helperを恒久配置しない。

対象deviceのSELinuxまたはlinker policyにより、shell domainから起動した一時agentがpublic Tuner AIDLへ接続できない場合は、同じagentを明示的なVTS/test imageだけへ含める`config/vts_test_agent_integration.mk`を使用する。このtest image経路を通常product integrationへ継承してはならない。

agentの論理責務・禁止責務、C++をFMQ descriptor import/read境界へ限定する規則、およびSI意味解析をhost側のcanonical `arib_si_engine_rs`へ接続する依存方向は`DESIGN_JA.md`を正とする。本書では配置・起動・除去・test imageへの接続だけを所有する。

### 6.7 生成物とproduct配置

生成されるAOSP Tuner VTS XMLはderived artifactであり、`VtsEnvironmentProfile`と同格の正本ではない。恒久的・再現可能なproduct imageへの配置は、`../tuner_hal/DESIGN_JA.md`のfilename解決契約で得た解決済みfilenameを使用し、生成・検証済みXMLをproduct build graph経由でvendor imageへ正確に1個installする。`PRODUCT_COPY_FILES`、生成済みconfig moduleその他の具体的なbuild mechanismは、この一意なinstall契約を満たす限り実装詳細とする。

実機VTSの反復確認では、compile後に必ずAndroidを再build/reflashすることを要求しない。対象端末で`adb root`後のadbdが実際にuid 0となり、`adb remount`でvendor側へ書き込み可能な状態を確立できる場合、`install-device`はcompile済みでprofileから解決されるfilenameと一致するXMLをadb経由で`/vendor/etc/<resolved-filename>`へ配置し、その配置内容を読み戻して一致確認した上でVTSへ進めてよい。このadb配置は試験端末上の反復用経路であり、product imageの恒久的・再現可能な構成をbuild graphから切り離す根拠にはしない。

`install-device`はXMLを生成・補正・再解釈せず、完全解決済みprofileと既存compile成果物だけを受理する。profileから解決されるfilenameとartifact basenameが一致しない場合、`adb root`後もuid 0でない場合、`adb remount`が失敗する場合、push後の読み戻しが一致しない場合はfail-closedとし、未検証XMLを別pathへ配置して回避しない。root/remountできないuser build、AVB/verity構成その他の端末では、このadb経路を使用せずbuild graphへ生成物を接続して再build/reflashする。

variantを使用する場合、variant propertyの値と生成XML filenameは同一`VtsEnvironmentProfile`の入力から導出し、product makefile側で別値を独立定義しない。`ro.vendor.vts_tuner_configuration_variant`はboot後に`install-device`が書き換える設定面にせず、adb配置前に実機のproperty値がprofileのvariantと完全一致することを確認する。不一致の場合はfail-closedとし、propertyをadbで上書きせず、必要なら一致するproduct imageをbuild/flashする。variantを使用しない場合も、実機propertyが空であることを同様に確認する。

`config/product_integration.mk`は、`../tuner_hal/DESIGN_JA.md`のVTS状態契約に従って静的configをinstall可能と判定されたproductだけで、生成・検証済みVTS config artifactをvendor imageへのbuild graphへ接続できる構造にする。artifactを含めないproductでは、推測した既定XMLまたは旧`tuner_hal`のVTS XMLを代用しない。

### 6.8 統合完了条件への接続

本節は`VTS-STATE-BOUND`等の状態意味を追加定義しない。`../tuner_hal/DESIGN_JA.md`で静的VTS configをinstall可能と判定されたprofileについて、product integrationとしては次が成立していることを要求する。

- `VtsEnvironmentProfile`が実在する単一設定ファイルとして存在し、CLIがその同一ファイルを生成・更新する。
- 実機なしでも地域等から受信候補を同じprofileへ保存でき、未解決値を架空値で埋めない。
- 実機接続後はpublic Tuner AIDLとPAT/PMTによってfrequency / service / PID等を解決し、同じprofileへ保存できる。
- compilerはその単一profileファイルだけをprofile入力としてAOSP Tuner VTS XMLを生成する。
- profileの全永続フィールドがXML出力、XML値の決定、契約検証または生成物配置のいずれかに実際に消費され、未消費metadataがない。
- 環境依存値の人間編集入口が単一profileに集約され、生成XMLまたはproduct makefileに同じ値の独立正本がない。
- AOSP VTS契約とのbuild-time照合、HAL capability/resource contractとの静的照合、AOSP schema検証が自動化されている。
- 解決済みfilenameへのvendor image installがbuild graphに接続され、生成物が`tuner_hal2`のproduct integrationだけへ属する。
- adb root/remount可能な試験端末では、同じ解決済みfilenameへcompile成果物を`install-device`で一時配置し、再build/reflashなしでVTS反復確認へ進める。root/remount不可またはvariant property不一致ならbuild graph経路を使用する。
- VTS設定は`tuner_hal2`の試験設定にだけ使用され、HAL capability、frontend registry、backend probe結果または公開API成功範囲を書き換えない。
- device preflightとTuner VTS実行手順が`タスク完了判定の実施方法.md`から一意に実行できる。

これらが未接続の状態では、profileの値を手作業で複数箇所へ転記してVTS構成を成立させたことにしない。

## 7. section filter runtime契約の参照

`TableInfo repeat=false`を含むsection filterの公開意味、first-instance解決、停止条件、`repeat=true`との使い分け、未知の全instance集合の終端をHALが推測しない契約は`../tuner_hal/DESIGN_JA.md`を正とする。複数table instanceのinstance別完成・更新・寿命は`../arib_si_engine_rs/DESIGN_JA.md`の「複数table instanceの完成・更新・寿命」、操作ごとの必要instance集合と完成時の明示`stop()`は`../tis/DESIGN_JA.md`の「複数table instance収集と停止」を正とする。

本書が所有するのはproduct統合だけであり、VINTF/init/package/VTS設定の配置によって上記runtime契約を変更または再定義してはならない。

## 8. px4 device probe path契約

px4系device nodeのprobe prefixは本節をproduct integration上のSSOTとする。対象prefixは次のとおりである。

```text
/dev/px4video
/dev/pxmlt5video
/dev/pxmlt8video
/dev/isdb6014video
/dev/isdb2056video
/dev/pxm1urvideo
/dev/pxs1urvideo
/dev/isdbt2071video
```

このprefix集合を変更する場合は、次を同一変更で同期する。

- `tuner_hal2`のpx4 frontend probe adapterは本節のprefix集合だけを参照してdevice node候補を構成する。実装owner/anchorは`DESIGN_JA.md`のfrontend/backend実装ownerに従い、本書では別の実装ownerを設けない。
- `tuner_hal2/config/ueventd.tuner_hal2.rc`は同じdevice node集合のpermission entryを持つ。
- `tuner_hal2/sepolicy/file_contexts`その他のSELinux path設定で同device nodeを列挙する場合は、本節のprefix集合と一致させる。

probe adapter、ueventd、SELinux側のいずれかだけに別prefixを追加してはならない。具体device pathの正本を実装helper名やPR履歴へ置かず、本節から一方向に同期する。

## 9. LineageOS 22.1 Tuner AIDL null 許容境界の統合

`tuner_hal/DESIGN_JA.md` の `nullable Binder 境界` に従い、LineageOS 22.1 / Android 15 では frozen V1/V2 を変更せず、unfrozen current V3 で Rust から表現できない null 入力だけを補う。

`hardware/interfaces` には次の修正を適用する。

```text
vendor/maleicacid/tv/tuner_hal2/platform_patches/lineage-22.1/android_hardware_tv_tuner_nullable_current.patch
```

この修正は次を行う。

- `IFilter.setDataSource()` の Filter 引数を `@nullable` とする。
- `IDescrambler.addPid()` / `removePid()` の optional source Filter を `@nullable` とする。
- `ILnb.setCallback()` の callback を `@nullable` とする。
- `IFrontend.setCallback()` は変更しない。
- FCM 202404 の Tuner AIDL 許容版を `1-3` とする。

Filter の null 入力を Java API から Hardware HAL まで保持するため、次の二つも適用する。

```text
vendor/maleicacid/tv/tuner_hal2/platform_patches/lineage-22.1/android_frameworks_base_tuner_filter_null_data_source.patch
vendor/maleicacid/tv/tuner_hal2/platform_patches/lineage-22.1/android_frameworks_av_tuner_filter_null_data_source.patch
```

Android ビルド木の先頭からの適用順は次とする。

```bash
git -C hardware/interfaces apply \
  "$ANDROID_BUILD_TOP/vendor/maleicacid/tv/tuner_hal2/platform_patches/lineage-22.1/android_hardware_tv_tuner_nullable_current.patch"

git -C frameworks/base apply \
  "$ANDROID_BUILD_TOP/vendor/maleicacid/tv/tuner_hal2/platform_patches/lineage-22.1/android_frameworks_base_tuner_filter_null_data_source.patch"

git -C frameworks/av apply \
  "$ANDROID_BUILD_TOP/vendor/maleicacid/tv/tuner_hal2/platform_patches/lineage-22.1/android_frameworks_av_tuner_filter_null_data_source.patch"
```

`frameworks/base` 用修正は `FilterClient::setDataSource(nullptr)` を参照外しせず内部 Tuner Filter へ伝える。`frameworks/av` 用修正は内部 `ITunerFilter.setDataSource()` の引数を null 許容として宣言し、`TunerFilter::setDataSource(nullptr)` を `INVALID_ARGUMENT` にせず Hardware HAL の `IFilter.setDataSource(nullptr)` へ伝える。これにより Java API の null 入力から Hardware HAL まで demux 入力元への復帰要求を保持する。

Descrambler の null source Filter は LineageOS 22.1 の既存 `frameworks/base` / `frameworks/av` が既に保持して HAL へ伝えるため、追加のフレームワーク修正を行わない。

Frontend callback は非 null 契約を維持する。AIDL コメントの null 可という記載だけを根拠に V3 へ `@nullable` を追加しない。フレームワークおよび AOSP 参照 HAL と同様、callback の寿命終了は `close()` で扱う。

LNB callback の null は Hardware HAL 直接境界の契約として残す。LineageOS 22.1 のフレームワークはこの入力を使用せず、フレームワーク側 callback の追加・削除はフレームワーク内部で管理するため、`frameworks/base` / `frameworks/av` の LNB callback 処理は変更しない。

`aidl_api/android.hardware.tv.tuner/1` と `2` は変更しない。`android.hardware.tv.tuner-freeze-api` は実行せず、`versions_with_info` に V3 を追加しない。null 許容版は current/unfrozen V3 のまま使用する。

`hardware/interfaces` 修正適用後、通常ビルドへ進む前に、その LineageOS checkout 上で次を一度実行して source AIDL と `aidl_api/android.hardware.tv.tuner/current/` snapshot を同期する。

```bash
m android.hardware.tv.tuner-update-api
```

`update-api` が生成する `aidl_api/.../current` の差分は platform patch へ取り込まない。clean checkout へ修正を新たに適用した場合は、その checkout について通常ビルド前に上記 `update-api` を実行する。同じ checkout で source AIDL と current snapshot が同期済みである限り、通常の product build のたびに `update-api` を再実行しない。`aidl_api/.../current` を手編集して source AIDL と独立に変更してはならない。

`tuner_hal2` は current V3 Rust binding を `android.hardware.tv.tuner-V3-rust` として参照し、VINTF fragment も Tuner version 3 を宣言する。採用 build configuration では `RELEASE_AIDL_USE_UNFROZEN=true` を実効値とする。`false` の構成では最新 unfrozen API を製品契約として使用できないため、この V3 統合の完了 build として扱わない。

この統合は LineageOS 22.1 / Android 15 checkout を前提とする。LineageOS 21.0 / Android 14 checkout は本節の V3 current、FCM、Rust 生成物の契約を満たさないため、この統合の入力として使用しない。
